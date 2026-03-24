/// my_os — An educational operating system built from scratch in Rust
///
/// This is the kernel entry point. When the machine boots:
///   BIOS → bootloader → Long Mode (64-bit) → _start() right here
///
/// We are `#![no_std]` because there IS no standard library — we ARE the OS.
/// We are `#![no_main]` because the C runtime's `main()` won't be called.
/// Our entry point is `_start`, which the bootloader jumps to.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(my_os::test_runner)]
#![reexport_test_harness_entry = "test_main"]

use core::panic::PanicInfo;
use my_os::println;

/// Kernel entry point — called by the bootloader after setting up Long Mode.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("Hello from our OS!");
    println!("We are running bare-metal Rust on x86_64.");
    println!();
    println!("There is no standard library here.");
    println!("No filesystem. No processes. No memory allocator.");
    println!("Just us and the hardware.");

    #[cfg(test)]
    test_main();

    my_os::hlt_loop();
}

/// Panic handler — called when the kernel panics.
/// In a real OS, this would trigger a kernel dump or reboot.
/// For now, we just print the error and halt.
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
