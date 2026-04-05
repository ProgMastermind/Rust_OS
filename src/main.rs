// Kernel entry point. BIOS -> bootloader -> Long Mode -> kernel_main().

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(my_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use my_os::{println, serial_println};

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use my_os::frame_allocator::BitmapFrameAllocator;
    use my_os::fs;
    use my_os::memory;
    use my_os::process::{self, ProcessState, PROCESS_TABLE};
    use x86_64::VirtAddr;

    my_os::init();
    serial_println!("Kernel booted successfully!");

    // Memory + heap
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BitmapFrameAllocator::init(&boot_info.memory_map) };

    my_os::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    serial_println!("Heap initialized");

    // Store globals for spawn() to map process stacks
    memory::store_globals(mapper, frame_allocator);

    // Process setup
    println!("Booting my_os...");

    {
        let mut table = PROCESS_TABLE.lock();

        // PID 0: idle process (kernel_main itself, uses boot stack)
        table.processes.push(process::Process {
            pid: 0,
            state: ProcessState::Running,
            stack_pointer: 0,
            entry_fn: None,
            stack_region: None,
            fd_table: alloc::vec![
                Some(fs::FdEntry::Stdin),
                Some(fs::FdEntry::Stdout),
                Some(fs::FdEntry::Stderr),
            ],
        });
        table.next_pid = 1;
        table.current = 0;

        table.spawn(my_os::shell::shell_main);
        serial_println!("Shell spawned as PID 1");
    }

    my_os::enable_interrupts();

    #[cfg(test)]
    test_main();

    my_os::hlt_loop();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    my_os::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    my_os::test_panic_handler(info)
}
