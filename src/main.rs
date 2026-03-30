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
    use alloc::{boxed::Box, string::String, vec, vec::Vec};
    use my_os::frame_allocator::BootInfoFrameAllocator;
    use my_os::memory;
    use x86_64::VirtAddr;

    my_os::init(); // Initialize GDT, IDT, PICs, enable interrupts

    serial_println!("Kernel booted successfully!");
    println!("Hello from our OS!");
    println!("We are running bare-metal Rust on x86_64.");
    println!();

    // ── Memory + Heap Setup ─────────────────────────────────────────
    //
    // 1. Initialize page tables (Session 3)
    // 2. Initialize frame allocator (Session 3)
    // 3. Map heap pages and initialize the allocator (Session 4 — NEW)

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    // Map virtual pages for the kernel heap and initialize the allocator.
    // After this call, Box, Vec, String all work!
    my_os::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    serial_println!("Heap initialized: {}KB at {:#x}",
        my_os::heap::HEAP_SIZE / 1024, my_os::heap::HEAP_START);

    // ── Heap Allocation Demo ────────────────────────────────────────
    //
    // These would have been impossible before Session 4.
    // Each one calls our GlobalAlloc implementation under the hood.

    // Box::new() allocates on the heap and returns a pointer.
    // Before: impossible (no allocator). Now: works!
    let heap_value = Box::new(42);
    println!("Box::new(42) = {}", heap_value);
    serial_println!("Box::new(42) at {:p} = {}", heap_value, heap_value);

    // Vec dynamically grows — reallocates as it fills up.
    // Starts with 0 capacity, grows to 4, 8, 16, ... as you push.
    let mut numbers: Vec<i32> = Vec::new();
    for i in 0..500 {
        numbers.push(i);
    }
    println!("Vec with {} elements, last = {}", numbers.len(), numbers[499]);
    serial_println!("Vec: len={}, capacity={}", numbers.len(), numbers.capacity());

    // vec![] macro — allocates and initializes in one step.
    let v = vec![1, 2, 3, 4, 5];
    println!("vec![1,2,3,4,5] sum = {}", v.iter().sum::<i32>());

    // String is a heap-allocated, growable UTF-8 string.
    let mut s = String::from("Hello");
    s.push_str(" from the kernel heap!");
    println!("{}", s);
    serial_println!("String: len={}, capacity={}", s.len(), s.capacity());

    println!();
    println!("Kernel heap working! Box, Vec, String all functional.");

    #[cfg(test)]
    test_main();

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
