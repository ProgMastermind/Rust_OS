// Kernel library crate
//
// This exists so that integration tests can import kernel functionality.
// The main.rs uses this as a dependency via `use my_os::...`.

#![no_std]
#![cfg_attr(test, no_main)]
#![feature(abi_x86_interrupt)]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]
#![reexport_test_harness_main = "test_main"]

// Enable the alloc crate — gives us Box, Vec, String, etc.
// This works because we provide a #[global_allocator] in src/allocator/mod.rs.
// Without that, linking would fail with "no global memory allocator found."
extern crate alloc;

pub mod allocator;
pub mod frame_allocator;
pub mod gdt;
pub mod heap;
pub mod interrupts;
pub mod memory;
pub mod process;
pub mod serial;
pub mod vga_buffer;

use core::panic::PanicInfo;

// Initialize CPU subsystems (GDT, IDT, PIC).
// Does NOT enable interrupts — the caller should do that only after
// all kernel state (heap, process table, etc.) is fully initialized.
// Otherwise a timer interrupt could fire before the kernel is ready.
//
// Order matters: GDT must be loaded before IDT (because the double fault
// handler's IST entry references the TSS, which lives in the GDT).
pub fn init() {
    gdt::init();            // Load GDT + TSS (sets up IST stacks)
    interrupts::init_idt(); // Load IDT (registers all exception/interrupt handlers)
    unsafe { interrupts::PICS.lock().initialize() }; // Initialize + remap PICs
    // NOTE: interrupts are NOT enabled here. Call
    // x86_64::instructions::interrupts::enable() explicitly after
    // heap and process table are ready.
}

// Enable hardware interrupts. Call this after init() and after all kernel
// subsystems (heap, process table, etc.) are initialized.
pub fn enable_interrupts() {
    x86_64::instructions::interrupts::enable(); // sti
}

// ── Custom Test Framework ──────────────────────────────────────────────
//
// Rust's default test framework needs `std`. We define our own:
// - `#[test_case]` marks test functions
// - `test_runner` iterates over them
// - Tests report results via serial port
// - QEMU exit device signals pass/fail to the host

/// Trait for test functions — prints name, runs, prints result.
pub trait Testable {
    fn run(&self);
}

impl<T: Fn()> Testable for T {
    fn run(&self) {
        serial_print!("{}...\t", core::any::type_name::<T>());
        self();
        serial_println!("[ok]");
    }
}

/// Runs all `#[test_case]` functions, then exits QEMU.
pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

/// Called on panic during tests — print error and exit with failure code.
pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

// ── QEMU Exit Device ──────────────────────────────────────────────────
//
// The `isa-debug-exit` device in QEMU maps an I/O port that, when written to,
// causes QEMU to exit with status `(value << 1) | 1`.
// So writing 0x10 gives exit code 33 (success), and 0x11 gives 35 (failure).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

/// Halt the CPU in a loop — saves power vs busy-spinning.
pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

// ── Test mode entry point (for `cargo test` on lib.rs) ────────────────

#[cfg(test)]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    init();              // GDT, IDT, PICs
    enable_interrupts(); // Tests don't use scheduler, safe to enable immediately
    test_main();
    hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}
