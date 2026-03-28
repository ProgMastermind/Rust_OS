// Integration test: basic boot
//
// Verifies that the kernel boots successfully and println! works.
// This runs as a completely separate binary in QEMU.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(my_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;

entry_point!(test_kernel_main);

fn test_kernel_main(_boot_info: &'static BootInfo) -> ! {
    test_main();
    my_os::hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    my_os::test_panic_handler(info)
}

#[test_case]
fn test_println_simple() {
    my_os::println!("test_println_simple output");
}

#[test_case]
fn test_println_many() {
    for i in 0..200 {
        my_os::println!("test_println_many line {}", i);
    }
}
