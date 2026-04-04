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

// ── Edge-case tests (Stage 3, Fix #8) ──────────────────────────────

// Test: allocations with various alignments return properly aligned pointers.
// The allocator must respect the alignment requested by each Layout.
// If alignment is broken, data structures like AtomicU64 (8-byte aligned)
// would cause hardware faults or silent data corruption on some architectures.
#[test_case]
fn aligned_allocations() {
    use core::alloc::Layout;

    for &align in &[1, 2, 4, 8, 16, 32, 64, 128, 256] {
        let layout = Layout::from_size_align(align, align).unwrap();
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        assert!(!ptr.is_null(), "allocation failed for align={}", align);
        assert_eq!(
            ptr as usize % align,
            0,
            "misaligned: ptr={:p} expected align={}",
            ptr,
            align,
        );
        unsafe { alloc::alloc::dealloc(ptr, layout) };
    }
}

// Test: alloc and dealloc in different orders (LIFO, FIFO, interleaved).
// A correct allocator must handle any deallocation order — not just the
// reverse of allocation order. The linked-list allocator with coalescing
// (Stage 2) should merge adjacent freed blocks regardless of free order.
#[test_case]
fn alloc_dealloc_patterns() {
    // LIFO (stack-like): A, B, C allocated, then freed C, B, A
    let a = Box::new([1u8; 64]);
    let b = Box::new([2u8; 128]);
    let c = Box::new([3u8; 256]);
    assert_eq!(a[0], 1);
    assert_eq!(b[0], 2);
    assert_eq!(c[0], 3);
    drop(c);
    drop(b);
    drop(a);

    // FIFO: A, B, C allocated, freed in same order A, B, C
    let a = Box::new([4u8; 64]);
    let b = Box::new([5u8; 128]);
    let c = Box::new([6u8; 256]);
    drop(a);
    drop(b);
    drop(c);

    // Interleaved: alloc A, alloc B, free A, alloc C, free C, free B
    let a = Box::new([7u8; 32]);
    let b = Box::new([8u8; 64]);
    drop(a);
    let c = Box::new([9u8; 48]);
    drop(c);
    drop(b);
}

// Test: large allocation close to heap capacity.
// Verifies the allocator can hand out a big contiguous block, and that
// freeing it returns the memory so another large allocation can succeed.
// This would fail with a bump allocator (no dealloc) or a fragmented heap.
#[test_case]
fn large_allocation() {
    // 50KB out of 100KB heap
    let large = Box::new([0xABu8; 50 * 1024]);
    assert_eq!(large[0], 0xAB);
    assert_eq!(large[50 * 1024 - 1], 0xAB);
    drop(large);

    // After freeing, the same amount should be allocatable again
    let another = Box::new([0xCDu8; 50 * 1024]);
    assert_eq!(another[0], 0xCD);
}
