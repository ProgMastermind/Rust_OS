// my_os — An educational operating system built from scratch in Rust
//
// This is the kernel entry point. When the machine boots:
//   BIOS -> bootloader -> Long Mode (64-bit) -> kernel_main() right here
//
// We use the entry_point! macro instead of a manual _start function.
// It generates _start for us and passes BootInfo (memory map, physical
// memory offset) which we need for memory management.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(my_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use my_os::{println, serial_println};

// The entry_point! macro:
//   1. Creates the actual _start function with correct calling convention
//   2. Passes us BootInfo from the bootloader (memory map, phys offset)
//   3. Type-checks our function signature at compile time
entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use my_os::frame_allocator::BootInfoFrameAllocator;
    use my_os::memory;
    use my_os::process::{self, ProcessState, PROCESS_TABLE};
    use x86_64::VirtAddr;

    my_os::init(); // Initialize GDT, IDT, PICs (interrupts NOT enabled yet)

    serial_println!("Kernel booted successfully!");

    // ── Memory + Heap Setup ─────────────────────────────────────────

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    my_os::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    serial_println!("Heap initialized");

    // ── Process Setup (Session 5) ───────────────────────────────────
    //
    // Spawn three processes that each print a letter in a loop.
    // The round-robin scheduler switches between them on each timer tick.
    // Expected output: interleaved A, B, C characters.

    println!("Spawning 3 processes...");
    serial_println!("Spawning 3 processes...");

    {
        let mut table = PROCESS_TABLE.lock();

        // Register kernel_main itself as process 0 (the "idle" process).
        // This is the currently running process — we need it in the table
        // so the scheduler can save its state and switch away from it.
        table.processes.push(process::Process {
            pid: 0,
            state: ProcessState::Running,
            stack_pointer: 0, // Will be filled by context_switch when we switch away
            entry_fn: None,   // Idle process has no entry — it IS kernel_main
            _stack: alloc::vec::Vec::new(), // kernel_main uses the bootloader's stack
        });
        table.next_pid = 1;
        table.current = 0;

        // Spawn the three worker processes.
        // spawn() stores the entry function directly in the Process struct (PCB).
        table.spawn(process_a);
        table.spawn(process_b);
        table.spawn(process_c);

        serial_println!("Process table: {} processes", table.processes.len());
    }

    println!("Processes spawned. Scheduler active.");
    println!("Watch for interleaved A/B/C output:");
    println!();

    // NOW enable interrupts — heap, process table, and scheduler are all ready.
    // Before this point, no timer interrupts fire and no scheduling happens.
    my_os::enable_interrupts();

    #[cfg(test)]
    test_main();

    // kernel_main becomes the idle process.
    // When no other process is Ready, the scheduler runs this.
    // hlt saves power while waiting for the next timer interrupt.
    my_os::hlt_loop();
}

// ── Demo Processes ────────────────────────────────────────────────────
//
// Each process prints a single character in a loop.
// The scheduler preemptively switches between them.
// Expected output: ABCABCABC... (interleaved)

fn process_a() {
    loop {
        my_os::print!("A");
        // hlt waits for next interrupt — saves CPU, and the timer interrupt
        // will preempt us and switch to the next process
        x86_64::instructions::hlt();
    }
}

fn process_b() {
    loop {
        my_os::print!("B");
        x86_64::instructions::hlt();
    }
}

fn process_c() {
    loop {
        my_os::print!("C");
        x86_64::instructions::hlt();
    }
}

/// Panic handler — called when the kernel panics.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    my_os::hlt_loop();
}

/// Panic handler for test mode — reports failure via serial port.
#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    my_os::test_panic_handler(info)
}
