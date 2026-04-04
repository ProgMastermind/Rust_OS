// Integration test: syscall interface, pointer validation, and ramdisk I/O
//
// Tests that the syscall mechanism works end-to-end: write args, trigger
// int 0x80, handler dispatches, return value is correct.
//
// These tests exercise the safety and correctness fixes from Stages 1-3:
//   - Pointer validation (EFAULT for null/invalid pointers)
//   - Error codes (EBADF, ENOENT, EINVAL, EPERM)
//   - Ramdisk file lifecycle (open → read → close → EOF)
//   - getpid returns a valid PID

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(my_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use my_os::frame_allocator::BitmapFrameAllocator;
use my_os::syscall;

entry_point!(test_kernel_main);

fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    use my_os::memory;
    use my_os::process::{self, ProcessState, PROCESS_TABLE};
    use x86_64::VirtAddr;

    my_os::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BitmapFrameAllocator::init(&boot_info.memory_map) };

    my_os::heap::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    // Store globals so spawn() works (needed for process table setup)
    memory::store_globals(mapper, frame_allocator);

    // Register this test runner as process 0 (idle process) so
    // syscalls that read PROCESS_TABLE.current have a valid entry.
    {
        let mut table = PROCESS_TABLE.lock();
        table.processes.push(process::Process {
            pid: 0,
            state: ProcessState::Running,
            stack_pointer: 0,
            entry_fn: None,
            stack_region: None,
            fd_table: alloc::vec![
                Some(my_os::fs::FdEntry::Stdin),
                Some(my_os::fs::FdEntry::Stdout),
                Some(my_os::fs::FdEntry::Stderr),
            ],
        });
        table.next_pid = 1;
        table.current = 0;
    }

    // Enable interrupts so int 0x80 can fire
    my_os::enable_interrupts();

    test_main();
    my_os::hlt_loop();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    my_os::test_panic_handler(info)
}

// ── SYS_GETPID ─────────────────────────────────────────────────────

// Test: getpid returns the PID of the current process.
// We registered this test runner as PID 0 above.
#[test_case]
fn syscall_getpid() {
    let pid = syscall::syscall(syscall::SYS_GETPID, 0, 0, 0);
    assert_eq!(pid, 0, "expected PID 0 for test process");
}

// ── SYS_WRITE ───────────────────────────────────────────────────────

// Test: write to stdout returns the number of bytes written.
#[test_case]
fn syscall_write_stdout() {
    let msg = b"test output";
    let result = syscall::syscall(
        syscall::SYS_WRITE,
        1, // stdout
        msg.as_ptr() as u64,
        msg.len() as u64,
    );
    assert_eq!(result, msg.len() as i64);
}

// Test: write to stderr returns the number of bytes written.
#[test_case]
fn syscall_write_stderr() {
    let msg = b"test error";
    let result = syscall::syscall(
        syscall::SYS_WRITE,
        2, // stderr
        msg.as_ptr() as u64,
        msg.len() as u64,
    );
    assert_eq!(result, msg.len() as i64);
}

// Test: write to stdin (fd 0) fails with EBADF.
#[test_case]
fn syscall_write_stdin_fails() {
    let msg = b"nope";
    let result = syscall::syscall(
        syscall::SYS_WRITE,
        0, // stdin — can't write to it
        msg.as_ptr() as u64,
        msg.len() as u64,
    );
    assert_eq!(result, syscall::EBADF);
}

// ── Pointer Validation ──────────────────────────────────────────────

// Test: null pointer in sys_write returns EFAULT.
#[test_case]
fn syscall_write_null_pointer() {
    let result = syscall::syscall(
        syscall::SYS_WRITE,
        1, // stdout
        0, // null pointer
        10,
    );
    assert_eq!(result, syscall::EFAULT);
}

// Test: absurdly large length returns EINVAL.
#[test_case]
fn syscall_write_huge_length() {
    let msg = b"x";
    let result = syscall::syscall(
        syscall::SYS_WRITE,
        1,
        msg.as_ptr() as u64,
        0xFFFF_FFFF_FFFF, // way beyond 1MB limit
    );
    assert_eq!(result, syscall::EINVAL);
}

// ── SYS_OPEN / SYS_READ / SYS_CLOSE (ramdisk lifecycle) ────────────

// Test: open a file, read its contents, verify, close.
// This exercises the full ramdisk I/O path through syscalls.
#[test_case]
fn syscall_ramdisk_open_read_close() {
    let path = b"hello.txt";

    // Open the file
    let fd = syscall::syscall(
        syscall::SYS_OPEN,
        path.as_ptr() as u64,
        path.len() as u64,
        0,
    );
    assert!(fd >= 3, "expected fd >= 3, got {}", fd); // 0-2 are stdin/stdout/stderr

    // Read the contents
    let mut buf = [0u8; 256];
    let bytes = syscall::syscall(
        syscall::SYS_READ,
        fd as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );
    assert!(bytes > 0, "expected to read some bytes");

    let content = core::str::from_utf8(&buf[..bytes as usize]).unwrap();
    assert_eq!(content, "Hello from the ramdisk filesystem!\n");

    // Read again — should return 0 (EOF, position is past end)
    let eof = syscall::syscall(
        syscall::SYS_READ,
        fd as u64,
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );
    assert_eq!(eof, 0, "expected EOF after reading entire file");

    // Close the file
    let close_result = syscall::syscall(syscall::SYS_CLOSE, fd as u64, 0, 0);
    assert_eq!(close_result, 0, "close should return 0");
}

// Test: opening a non-existent file returns ENOENT.
#[test_case]
fn syscall_open_nonexistent() {
    let path = b"does_not_exist.txt";
    let result = syscall::syscall(
        syscall::SYS_OPEN,
        path.as_ptr() as u64,
        path.len() as u64,
        0,
    );
    assert_eq!(result, syscall::ENOENT);
}

// Test: reading from an invalid fd returns EBADF.
#[test_case]
fn syscall_read_bad_fd() {
    let mut buf = [0u8; 16];
    let result = syscall::syscall(
        syscall::SYS_READ,
        99, // no such fd
        buf.as_mut_ptr() as u64,
        buf.len() as u64,
    );
    assert_eq!(result, syscall::EBADF);
}

// Test: closing stdin/stdout/stderr returns EPERM.
#[test_case]
fn syscall_close_stdio_fails() {
    assert_eq!(syscall::syscall(syscall::SYS_CLOSE, 0, 0, 0), syscall::EPERM);
    assert_eq!(syscall::syscall(syscall::SYS_CLOSE, 1, 0, 0), syscall::EPERM);
    assert_eq!(syscall::syscall(syscall::SYS_CLOSE, 2, 0, 0), syscall::EPERM);
}

// Test: closing an already-closed fd returns EBADF.
#[test_case]
fn syscall_close_twice() {
    let path = b"hello.txt";
    let fd = syscall::syscall(
        syscall::SYS_OPEN,
        path.as_ptr() as u64,
        path.len() as u64,
        0,
    );
    assert!(fd >= 3);

    // First close succeeds
    assert_eq!(syscall::syscall(syscall::SYS_CLOSE, fd as u64, 0, 0), 0);

    // Second close fails — fd is already closed
    assert_eq!(
        syscall::syscall(syscall::SYS_CLOSE, fd as u64, 0, 0),
        syscall::EBADF
    );
}

// ── Unknown Syscall ─────────────────────────────────────────────────

// Test: unknown syscall number returns ENOSYS.
#[test_case]
fn syscall_unknown_number() {
    let result = syscall::syscall(999, 0, 0, 0);
    assert_eq!(result, syscall::ENOSYS);
}
