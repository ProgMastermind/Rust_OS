/// Integration test: basic boot
///
/// Verifies that the kernel boots successfully and println! works.
/// This runs as a completely separate binary in QEMU.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(my_os::test_runner)]
#![reexport_test_harness_entry = "test_main"]

use core::panic::PanicInfo;
use my_os::println;

#[no_mangle]
pub extern "C" fn _start() -> ! {
    test_main();
    my_os::hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    my_os::test_panic_handler(info)
}

#[test_case]
fn test_println_simple() {
    println!("test_println_simple output");
}

#[test_case]
fn test_println_many() {
    for i in 0..200 {
        println!("test_println_many line {}", i);
    }
}
