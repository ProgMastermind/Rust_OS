// File System Syscalls
//
// These functions are called by the syscall dispatcher (mod.rs).
// They bridge between the syscall interface and the VFS/ramdisk.
//
// All of these run in kernel mode with interrupts disabled (inside
// the int 0x80 handler). They access the process table to get the
// current process's file descriptor table.
//
// Error codes follow the errno convention defined in syscall/mod.rs:
//   EFAULT = bad pointer, EBADF = bad fd, ENOENT = file not found, etc.

use crate::fs::initrd::RAMDISK;
use crate::fs::{FdEntry, FileSystem};
use crate::process::PROCESS_TABLE;
use super::{EBADF, EINVAL, ENOENT, EPERM, EROFS};

// sys_write(fd, buf_ptr, len) -> bytes_written or negative error
//
// Write `len` bytes from the buffer at `buf_ptr` to file descriptor `fd`.
//   fd 1 (stdout) → VGA screen
//   fd 2 (stderr) → serial port
//   Other file fds → EROFS (ramdisk is read-only)
pub fn sys_write(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    // Validate the pointer before creating a slice.
    let buf = match unsafe { super::slice_from_user_ptr(buf_ptr, len) } {
        Ok(b) => b,
        Err(e) => return e, // EFAULT or EINVAL
    };

    match fd {
        1 => {
            // stdout — write to VGA screen
            if let Ok(s) = core::str::from_utf8(buf) {
                crate::print!("{}", s);
            } else {
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
        0 => EBADF, // Can't write to stdin
        _ => EROFS,  // Ramdisk files are read-only
    }
}

// sys_read(fd, buf_ptr, len) -> bytes_read or negative error
//
// Read up to `len` bytes from file descriptor `fd` into the buffer at `buf_ptr`.
// Returns 0 at end of file.
pub fn sys_read(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    // Validate the pointer before creating a mutable slice.
    let buf = match unsafe { super::slice_from_user_ptr_mut(buf_ptr, len) } {
        Ok(b) => b,
        Err(e) => return e,
    };

    let fd_idx = fd as usize;

    match fd {
        0 => {
            // stdin — read from the keyboard buffer (non-blocking).
            // Returns however many bytes are currently available (up to len).
            // Returns 0 if no data is available — the caller is responsible
            // for blocking and retrying if it wants to wait for input.
            //
            // We don't block inside the syscall handler because we're in
            // interrupt context (int 0x80, IF=0). Blocking here would
            // prevent the timer and keyboard interrupts from firing.
            // Instead, the shell uses keyboard::read_char() which calls
            // block_current(Stdin) between sys_read attempts.
            let mut bytes_read = 0usize;
            for byte in buf.iter_mut() {
                if let Some(c) = crate::keyboard::pop_char() {
                    *byte = c;
                    bytes_read += 1;
                } else {
                    break; // Buffer empty — return what we have so far
                }
            }
            bytes_read as i64
        }
        1 | 2 => EBADF, // Can't read from stdout or stderr
        _ => {
            let mut table = PROCESS_TABLE.lock();
            let current = table.current;

            // Bounds check on process table
            if current >= table.processes.len() {
                return EINVAL;
            }

            let process = &mut table.processes[current];

            if fd_idx >= process.fd_table.len() {
                return EBADF; // fd number exceeds table size
            }

            match &mut process.fd_table[fd_idx] {
                Some(FdEntry::File {
                    file_index,
                    position,
                }) => {
                    let bytes_read = RAMDISK.read(*file_index, *position, buf);
                    *position += bytes_read;
                    bytes_read as i64
                }
                Some(FdEntry::Stdin) | Some(FdEntry::Stdout) | Some(FdEntry::Stderr) => EBADF,
                None => EBADF, // Slot is closed/empty
            }
        }
    }
}

// sys_open(path_ptr, path_len) -> fd or negative error
//
// Open a file by path. Returns a new file descriptor number.
pub fn sys_open(path_ptr: u64, path_len: u64) -> i64 {
    // Validate the path pointer
    let path_bytes = match unsafe { super::slice_from_user_ptr(path_ptr, path_len) } {
        Ok(b) => b,
        Err(e) => return e,
    };

    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return EINVAL, // Invalid UTF-8 in path
    };

    // Look up the file in the ramdisk
    let file_index = match RAMDISK.open(path) {
        Some(idx) => idx,
        None => return ENOENT, // File not found
    };

    // Add to the current process's fd table
    let mut table = PROCESS_TABLE.lock();
    let current = table.current;

    if current >= table.processes.len() {
        return EINVAL;
    }

    let process = &mut table.processes[current];

    // Find the first free slot (None) in the fd table
    let free_slot = process.fd_table.iter().position(|entry| entry.is_none());

    match free_slot {
        Some(fd) => {
            process.fd_table[fd] = Some(FdEntry::File {
                file_index,
                position: 0,
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

// sys_close(fd) -> 0 or negative error
//
// Close a file descriptor. The fd slot becomes available for reuse.
pub fn sys_close(fd: u64) -> i64 {
    let fd_idx = fd as usize;

    let mut table = PROCESS_TABLE.lock();
    let current = table.current;

    if current >= table.processes.len() {
        return EINVAL;
    }

    let process = &mut table.processes[current];

    if fd_idx >= process.fd_table.len() {
        return EBADF;
    }

    match &process.fd_table[fd_idx] {
        Some(FdEntry::Stdin) | Some(FdEntry::Stdout) | Some(FdEntry::Stderr) => {
            EPERM // Cannot close stdin/stdout/stderr
        }
        Some(FdEntry::File { .. }) => {
            process.fd_table[fd_idx] = None;
            0
        }
        None => EBADF, // Already closed
    }
}
