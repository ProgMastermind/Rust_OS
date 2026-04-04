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
    use my_os::frame_allocator::BitmapFrameAllocator;
    use my_os::fs;
    use my_os::memory;
    use my_os::process::{self, ProcessState, PROCESS_TABLE};
    use x86_64::VirtAddr;

    my_os::init(); // Initialize GDT, IDT, PICs (interrupts NOT enabled yet)

    serial_println!("Kernel booted successfully!");

    // ── Memory + Heap Setup ─────────────────────────────────────────

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BitmapFrameAllocator::init(&boot_info.memory_map) };

    my_os::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    serial_println!("Heap initialized");

    // Store mapper and frame allocator globally so spawn() can map
    // process stacks with guard pages. After this, all page mapping
    // goes through the global accessors in memory.rs.
    memory::store_globals(mapper, frame_allocator);

    // ── Process Setup ───────────────────────────────────────────────

    println!("Booting my_os...");

    {
        let mut table = PROCESS_TABLE.lock();

        // Register kernel_main as process 0 (idle process).
        // stack_region is None because the idle process uses the kernel
        // boot stack (set up by the bootloader), not a mapped stack.
        table.processes.push(process::Process {
            pid: 0,
            state: ProcessState::Running,
            stack_pointer: 0,
            entry_fn: None,
            stack_region: None,
            fd_table: alloc::vec![
                Some(fs::FdEntry::Stdin),  // fd 0
                Some(fs::FdEntry::Stdout), // fd 1
                Some(fs::FdEntry::Stderr), // fd 2
            ],
        });
        table.next_pid = 1;
        table.current = 0;

        // Spawn the shell as process 1.
        // spawn() maps 4 stack pages at a dedicated virtual address
        // with an unmapped guard page below for stack overflow detection.
        table.spawn(my_os::shell::shell_main);
        serial_println!("Shell spawned as PID 1");
    }

    // Enable interrupts — heap, process table, and shell are all ready.
    // The timer will start firing, the scheduler will switch to the shell,
    // and the keyboard interrupt will push characters into the buffer.
    my_os::enable_interrupts();

    #[cfg(test)]
    test_main();

    // kernel_main becomes the idle process.
    my_os::hlt_loop();
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
