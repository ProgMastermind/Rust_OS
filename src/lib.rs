// Kernel library crate. Integration tests import from here.

#![no_std]
#![cfg_attr(test, no_main)]
#![feature(abi_x86_interrupt)]
#![feature(custom_test_frameworks)]
#![test_runner(test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

pub mod allocator;
pub mod frame_allocator;
pub mod fs;
pub mod gdt;
pub mod heap;
pub mod interrupts;
pub mod keyboard;
pub mod memory;
pub mod process;
pub mod serial;
pub mod shell;
pub mod syscall;
pub mod vga_buffer;

use core::panic::PanicInfo;

/// GDT, IDT, PICs. Does NOT enable interrupts.
pub fn init() {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
}

/// Separate from init() so interrupts stay off until heap/processes are ready.
pub fn enable_interrupts() {
    x86_64::instructions::interrupts::enable();
}

// Test framework

/// Wraps test functions to print name before running and [ok] after.
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

/// Run all #[test_case] functions, then exit QEMU with success.
pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

/// Panic handler for integration test binaries. Prints failure and exits QEMU.
pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

// QEMU exit device: writing to port 0xf4 exits with (value << 1) | 1.
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

/// Halt loop. Uses hlt to save power instead of busy-spinning.
pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[cfg(test)]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    init();
    enable_interrupts();
    test_main();
    hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}
