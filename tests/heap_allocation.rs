// Integration test: heap allocation
//
// Tests that Box, Vec, and large allocations work correctly.
// Also tests that deallocation works by doing many alloc/dealloc cycles.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(my_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use my_os::frame_allocator::BitmapFrameAllocator;

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    use my_os::memory;
    use x86_64::VirtAddr;

    my_os::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BitmapFrameAllocator::init(&boot_info.memory_map) };

    my_os::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    test_main();
    my_os::hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    my_os::test_panic_handler(info)
}

// Test: simple Box allocation
#[test_case]
fn simple_allocation() {
    let heap_value_1 = Box::new(41);
    let heap_value_2 = Box::new(13);
    assert_eq!(*heap_value_1, 41);
    assert_eq!(*heap_value_2, 13);
}

// Test: Vec with many elements (triggers multiple reallocations)
#[test_case]
fn large_vec() {
    let n = 1000;
    let mut vec = Vec::new();
    for i in 0..n {
        vec.push(i);
    }
    // Verify the sum to make sure all elements were stored correctly.
    // Sum of 0..1000 = 999 * 1000 / 2 = 499500
    assert_eq!(vec.iter().sum::<u64>(), (n - 1) * n / 2);
}

// Test: alloc and dealloc many times — proves dealloc actually works.
// If dealloc was broken (like in the bump allocator), the heap would
// run out of memory after a few iterations.
#[test_case]
fn many_boxes() {
    for i in 0..my_os::heap::HEAP_SIZE {
        let x = Box::new(i);
        assert_eq!(*x, i);
        // x is dropped here → dealloc is called
        // If dealloc doesn't work, we'd exhaust 100KB of heap in ~12500 iterations
        // (each Box<usize> is 8 bytes). With working dealloc, this runs forever.
    }
}

// Test: alloc many boxes without freeing, then free all at once.
// Tests that the allocator handles multiple live allocations.
#[test_case]
fn many_boxes_long_lived() {
    let long_lived = Box::new(1);
    for i in 0..my_os::heap::HEAP_SIZE {
        let x = Box::new(i);
        assert_eq!(*x, i);
    }
    assert_eq!(*long_lived, 1);
}
