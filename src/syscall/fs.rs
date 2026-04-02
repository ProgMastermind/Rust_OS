// File System Syscalls
//
// These functions are called by the syscall dispatcher (mod.rs).
// They bridge between the syscall interface and the VFS/ramdisk.
//
// All of these run in kernel mode with interrupts disabled (inside
// the int 0x80 handler). They access the process table to get the
// current process's file descriptor table.

use crate::fs::initrd::RAMDISK;
use crate::fs::{FdEntry, FileSystem};
use crate::process::PROCESS_TABLE;

// sys_write(fd, buf_ptr, len) -> bytes_written or -1
//
// Write `len` bytes from the buffer at `buf_ptr` to file descriptor `fd`.
//   fd 1 (stdout) → print to VGA screen
//   fd 2 (stderr) → print to serial port
//   Other fds → error (ramdisk is read-only)
pub fn sys_write(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    // Reconstruct the byte slice from the raw pointer and length.
    // SAFETY: In Ring 0, all addresses are valid kernel addresses.
    // In a real Ring 3 setup, we'd validate that buf_ptr is in user space.
    let buf = unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, len as usize) };

    match fd {
        1 => {
            // stdout — write to VGA screen
            if let Ok(s) = core::str::from_utf8(buf) {
                crate::print!("{}", s);
            } else {
                // Not valid UTF-8 — print byte by byte as hex
                for &byte in buf {
                    crate::print!("\\x{:02x}", byte);
                }
            }
            len as i64
        }
        2 => {
            // stderr — write to serial port
            if let Ok(s) = core::str::from_utf8(buf) {
                crate::serial_print!("{}", s);
            } else {
                for &byte in buf {
                    crate::serial_print!("\\x{:02x}", byte);
                }
            }
            len as i64
        }
        _ => {
            // Ramdisk files are read-only. Writing to them is an error.
            -1
        }
    }
}

// sys_read(fd, buf_ptr, len) -> bytes_read or -1
//
// Read up to `len` bytes from file descriptor `fd` into the buffer at `buf_ptr`.
// Returns the number of bytes actually read (0 at end of file).
pub fn sys_read(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, len as usize) };
    let fd_idx = fd as usize;

    match fd {
        0 => {
            // stdin — keyboard input. Not yet implemented.
            // A real implementation would block until input is available.
            0
        }
        1 | 2 => {
            // Can't read from stdout or stderr
            -1
        }
        _ => {
            // Read from a file descriptor
            let mut table = PROCESS_TABLE.lock();
            let current = table.current;
            let process = &mut table.processes[current];

            if fd_idx >= process.fd_table.len() {
                return -1; // Invalid fd
            }

            match &mut process.fd_table[fd_idx] {
                Some(FdEntry::File {
                    file_index,
                    position,
                }) => {
                    let bytes_read = RAMDISK.read(*file_index, *position, buf);
                    *position += bytes_read; // Advance the read position
                    bytes_read as i64
                }
                _ => -1, // fd exists but isn't a file (or is None/closed)
            }
        }
    }
}

// sys_open(path_ptr, path_len) -> fd or -1
//
// Open a file by path. Returns a new file descriptor number.
// The fd is an index into the process's fd_table.
pub fn sys_open(path_ptr: u64, path_len: u64) -> i64 {
    // Reconstruct the path string
    let path = unsafe {
        let bytes = core::slice::from_raw_parts(path_ptr as *const u8, path_len as usize);
        match core::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return -1, // Invalid UTF-8 path
        }
    };

    // Look up the file in the ramdisk
    let file_index = match RAMDISK.open(path) {
        Some(idx) => idx,
        None => return -1, // File not found
    };

    // Add to the current process's fd table
    let mut table = PROCESS_TABLE.lock();
    let current = table.current;
    let process = &mut table.processes[current];

    // Find the first free slot (None) in the fd table
    let free_slot = process.fd_table.iter().position(|entry| entry.is_none());

    match free_slot {
        Some(fd) => {
            process.fd_table[fd] = Some(FdEntry::File {
                file_index,
                position: 0, // Start reading from the beginning
            });
            fd as i64
        }
        None => {
            // No free slot — extend the table
            let fd = process.fd_table.len();
            process.fd_table.push(Some(FdEntry::File {
                file_index,
                position: 0,
            }));
            fd as i64
        }
    }
}

// sys_close(fd) -> 0 or -1
//
// Close a file descriptor. The fd slot becomes available for reuse.
pub fn sys_close(fd: u64) -> i64 {
    let fd_idx = fd as usize;

    let mut table = PROCESS_TABLE.lock();
    let current = table.current;
    let process = &mut table.processes[current];

    if fd_idx >= process.fd_table.len() {
        return -1; // Invalid fd
    }

    // Don't allow closing stdin, stdout, stderr
    match &process.fd_table[fd_idx] {
        Some(FdEntry::Stdin) | Some(FdEntry::Stdout) | Some(FdEntry::Stderr) => -1,
        Some(FdEntry::File { .. }) => {
            process.fd_table[fd_idx] = None; // Free the slot
            0
        }
        None => -1, // Already closed
    }
}
