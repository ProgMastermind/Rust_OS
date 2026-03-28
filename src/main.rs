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
    use x86_64::structures::paging::Translate;
    use x86_64::VirtAddr;

    my_os::init(); // Initialize GDT, IDT, PICs, enable interrupts

    serial_println!("Kernel booted successfully!");
    println!("Hello from our OS!");
    println!("We are running bare-metal Rust on x86_64.");
    println!();

    // ── Memory Management Demo ──────────────────────────────────────
    //
    // Now we can translate virtual addresses to physical addresses and
    // create new page mappings.

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    // Demo 1: Translate some known virtual addresses to physical addresses
    let addresses = [
        // VGA buffer — identity mapped by bootloader
        0xb8000,
        // Some code page of our kernel
        0x201008,
        // Stack page
        0x0100_0020_1a10,
    ];

    serial_println!("Virtual → Physical address translations:");
    for &address in &addresses {
        let virt = VirtAddr::new(address);
        let phys = mapper.translate_addr(virt);
        serial_println!("  {:>16x} -> {:?}", address, phys);
        println!("  {:>16x} -> {:?}", address, phys);
    }

    // Demo 2: Create a new page mapping
    // Map an unused virtual page to the VGA buffer's physical frame.
    // Writing to this new virtual address will write to VGA — proving the mapping works.
    use x86_64::structures::paging::{Mapper, Page, PageTableFlags, PhysFrame, Size4KiB};
    use x86_64::PhysAddr;

    // Pick an arbitrary unused virtual address
    let page = Page::<Size4KiB>::containing_address(VirtAddr::new(0xdead_0000));
    // Map it to the VGA buffer's physical frame (0xb8000)
    let frame = PhysFrame::containing_address(PhysAddr::new(0xb8000));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    let map_result = unsafe { mapper.map_to(page, frame, flags, &mut frame_allocator) };
    // flush() updates the TLB so the CPU sees the new mapping immediately
    map_result.expect("map_to failed").flush();
    serial_println!("Mapped page 0xdead_0000 -> physical frame 0xb8000");

    // Write through the NEW virtual address — should appear on VGA screen.
    // We mapped 0xdead_0000 to physical 0xb8000 (VGA buffer start).
    // Each VGA row = 80 chars × 2 bytes = 160 bytes.
    // We write to row 0, col 0 — but println! scrolls, so we need to write
    // AFTER we're done printing. We'll print a message that says to look at
    // row 0 after the write.
    //
    // Each VGA character = 2 bytes: [ascii_char, color_attribute]
    // 0x4f = white text on red background (very visible!)
    //
    // The write goes through virtual address 0xdead_0000, which the page table
    // maps to physical 0xb8000 (VGA buffer). If the mapping didn't work,
    // this would page fault and crash.
    println!();
    println!("Memory management initialized.");
    println!("Frame allocator and page tables ready.");
    println!("Look at the top-left corner -> 'New!' written via remapped page");

    // NOW write "New!" — after all println! calls are done, so scrolling
    // won't overwrite row 0. We write to virtual 0xdead_0000 which we
    // mapped to physical 0xb8000 (VGA buffer start = row 0, col 0).
    // 0x4f = white text on red background — very visible!
    let page_ptr: *mut u64 = page.start_address().as_mut_ptr();
    unsafe { page_ptr.write_volatile(0x_4f21_4f77_4f65_4f4e) };

    serial_println!("Wrote 'New!' to VGA via remapped page at 0xdead_0000");

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
