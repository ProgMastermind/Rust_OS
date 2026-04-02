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
    use my_os::frame_allocator::BootInfoFrameAllocator;
    use my_os::fs::{self, FileSystem};
    use my_os::memory;
    use my_os::process::{self, ProcessState, PROCESS_TABLE};
    use my_os::syscall;
    use x86_64::VirtAddr;

    my_os::init(); // Initialize GDT, IDT, PICs (interrupts NOT enabled yet)

    serial_println!("Kernel booted successfully!");

    // ── Memory + Heap Setup ─────────────────────────────────────────

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    my_os::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    serial_println!("Heap initialized");

    // ── Process Setup ───────────────────────────────────────────────
    //
    // Register kernel_main as process 0. We need at least one process
    // in the table so syscalls like getpid can read the current process.

    {
        let mut table = PROCESS_TABLE.lock();
        table.processes.push(process::Process {
            pid: 0,
            state: ProcessState::Running,
            stack_pointer: 0,
            entry_fn: None,
            _stack: alloc::vec::Vec::new(),
            fd_table: alloc::vec![
                Some(fs::FdEntry::Stdin),  // fd 0
                Some(fs::FdEntry::Stdout), // fd 1
                Some(fs::FdEntry::Stderr), // fd 2
            ],
        });
        table.next_pid = 1;
        table.current = 0;
    }

    // ── Syscall + Filesystem Demo (Session 6) ───────────────────────
    //
    // All interaction goes through the syscall interface:
    //   user code → int 0x80 → kernel dispatcher → kernel function → return

    println!("=== Syscall Demo ===");
    println!();

    // 1. sys_write: write to stdout (fd 1) via syscall instead of print!
    let msg = b"Hello via sys_write to stdout!\n";
    let written = syscall::syscall(
        syscall::SYS_WRITE,
        1, // fd 1 = stdout
        msg.as_ptr() as u64,
        msg.len() as u64,
    );
    serial_println!("sys_write returned: {} bytes", written);

    // 2. sys_getpid: ask the kernel for our process ID
    let pid = syscall::syscall(syscall::SYS_GETPID, 0, 0, 0);
    println!("My PID (via sys_getpid): {}", pid);

    // 3. List files in the ramdisk (direct VFS call, not a syscall)
    println!();
    println!("=== Ramdisk Files ===");
    let ramdisk = &fs::initrd::RAMDISK;
    for i in 0..ramdisk.file_count() {
        if let Some(info) = ramdisk.file_at(i) {
            println!("  {} ({} bytes)", info.name, info.size);
        }
    }

    // 4. sys_open: open a file from the ramdisk
    println!();
    println!("=== File I/O via Syscalls ===");
    let path = "hello.txt";
    let fd = syscall::syscall(
        syscall::SYS_OPEN,
        path.as_ptr() as u64,
        path.len() as u64,
        0,
    );
    println!("sys_open('{}') -> fd {}", path, fd);
    serial_println!("sys_open('{}') -> fd {}", path, fd);

    // 5. sys_read: read the file contents
    let mut buf = [0u8; 128];
    let bytes_read = syscall::syscall(
        syscall::SYS_READ,
        fd as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );
    let content = core::str::from_utf8(&buf[..bytes_read as usize]).unwrap_or("???");
    println!("sys_read(fd {}) -> {} bytes: {}", fd, bytes_read, content.trim());
    serial_println!("sys_read: {} bytes", bytes_read);

    // 6. sys_close: close the file
    let close_result = syscall::syscall(syscall::SYS_CLOSE, fd as u64, 0, 0);
    println!("sys_close(fd {}) -> {}", fd, close_result);

    // 7. Open and read a second file to prove it works for multiple files
    let path2 = "readme.txt";
    let fd2 = syscall::syscall(
        syscall::SYS_OPEN,
        path2.as_ptr() as u64,
        path2.len() as u64,
        0,
    );
    let mut buf2 = [0u8; 128];
    let bytes2 = syscall::syscall(
        syscall::SYS_READ,
        fd2 as u64,
        buf2.as_mut_ptr() as u64,
        buf2.len() as u64,
    );
    let content2 = core::str::from_utf8(&buf2[..bytes2 as usize]).unwrap_or("???");
    println!();
    println!("sys_open('{}') -> fd {}", path2, fd2);
    println!("sys_read: {}", content2.trim());
    syscall::syscall(syscall::SYS_CLOSE, fd2 as u64, 0, 0);

    // 8. Try opening a nonexistent file — should return -1
    let bad_path = "nope.txt";
    let bad_fd = syscall::syscall(
        syscall::SYS_OPEN,
        bad_path.as_ptr() as u64,
        bad_path.len() as u64,
        0,
    );
    println!();
    println!("sys_open('{}') -> {} (expected -1: file not found)", bad_path, bad_fd);

    println!();
    println!("Syscall interface and filesystem working.");

    // Enable interrupts so keyboard works
    my_os::enable_interrupts();

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
