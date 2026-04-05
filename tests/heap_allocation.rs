// Integration test: heap allocation (Box, Vec, alignment, dealloc patterns).

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

#[test_case]
fn simple_allocation() {
    let heap_value_1 = Box::new(41);
    let heap_value_2 = Box::new(13);
    assert_eq!(*heap_value_1, 41);
    assert_eq!(*heap_value_2, 13);
}

#[test_case]
fn large_vec() {
    let n = 1000;
    let mut vec = Vec::new();
    for i in 0..n {
        vec.push(i);
    }
    assert_eq!(vec.iter().sum::<u64>(), (n - 1) * n / 2);
}

#[test_case]
fn many_boxes() {
    for i in 0..my_os::heap::HEAP_SIZE {
        let x = Box::new(i);
        assert_eq!(*x, i);
    }
}

#[test_case]
fn many_boxes_long_lived() {
    let long_lived = Box::new(1);
    for i in 0..my_os::heap::HEAP_SIZE {
        let x = Box::new(i);
        assert_eq!(*x, i);
    }
    assert_eq!(*long_lived, 1);
}

#[test_case]
fn aligned_allocations() {
    use core::alloc::Layout;

    for &align in &[1, 2, 4, 8, 16, 32, 64, 128, 256] {
        let layout = Layout::from_size_align(align, align).unwrap();
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        assert!(!ptr.is_null(), "allocation failed for align={}", align);
        assert_eq!(ptr as usize % align, 0, "misaligned: ptr={:p} align={}", ptr, align);
        unsafe { alloc::alloc::dealloc(ptr, layout) };
    }
}

#[test_case]
fn alloc_dealloc_patterns() {
    // LIFO
    let a = Box::new([1u8; 64]);
    let b = Box::new([2u8; 128]);
    let c = Box::new([3u8; 256]);
    assert_eq!(a[0], 1);
    assert_eq!(b[0], 2);
    assert_eq!(c[0], 3);
    drop(c);
    drop(b);
    drop(a);

    // FIFO
    let a = Box::new([4u8; 64]);
    let b = Box::new([5u8; 128]);
    let c = Box::new([6u8; 256]);
    drop(a);
    drop(b);
    drop(c);

    // Interleaved
    let a = Box::new([7u8; 32]);
    let b = Box::new([8u8; 64]);
    drop(a);
    let c = Box::new([9u8; 48]);
    drop(c);
    drop(b);
}

#[test_case]
fn large_allocation() {
    let large = Box::new([0xABu8; 50 * 1024]);
    assert_eq!(large[0], 0xAB);
    assert_eq!(large[50 * 1024 - 1], 0xAB);
    drop(large);

    let another = Box::new([0xCDu8; 50 * 1024]);
    assert_eq!(another[0], 0xCD);
}
