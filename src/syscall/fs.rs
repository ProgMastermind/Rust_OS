// File system syscalls: read, write, open, close.

use crate::fs::initrd::RAMDISK;
use crate::fs::{FdEntry, FileSystem};
use crate::process::PROCESS_TABLE;
use super::{EBADF, EINVAL, ENOENT, EPERM, EROFS};

pub fn sys_write(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    let buf = match unsafe { super::slice_from_user_ptr(buf_ptr, len) } {
        Ok(b) => b,
        Err(e) => return e,
    };

    match fd {
        1 => {
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
            if let Ok(s) = core::str::from_utf8(buf) {
                crate::serial_print!("{}", s);
            } else {
                for &byte in buf {
                    crate::serial_print!("\\x{:02x}", byte);
                }
            }
            len as i64
        }
        0 => EBADF,
        _ => EROFS,
    }
}

pub fn sys_read(fd: u64, buf_ptr: u64, len: u64) -> i64 {
    let buf = match unsafe { super::slice_from_user_ptr_mut(buf_ptr, len) } {
        Ok(b) => b,
        Err(e) => return e,
    };

    let fd_idx = fd as usize;

    match fd {
        0 => {
            // stdin: non-blocking read from keyboard buffer
            let mut bytes_read = 0usize;
            for byte in buf.iter_mut() {
                if let Some(c) = crate::keyboard::pop_char() {
                    *byte = c;
                    bytes_read += 1;
                } else {
                    break;
                }
            }
            bytes_read as i64
        }
        1 | 2 => EBADF,
        _ => {
            let mut table = PROCESS_TABLE.lock();
            let current = table.current;

            if current >= table.processes.len() {
                return EINVAL;
            }

            let process = &mut table.processes[current];

            if fd_idx >= process.fd_table.len() {
                return EBADF;
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
                None => EBADF,
            }
        }
    }
}

pub fn sys_open(path_ptr: u64, path_len: u64) -> i64 {
    let path_bytes = match unsafe { super::slice_from_user_ptr(path_ptr, path_len) } {
        Ok(b) => b,
        Err(e) => return e,
    };

    let path = match core::str::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return EINVAL,
    };

    let file_index = match RAMDISK.open(path) {
        Some(idx) => idx,
        None => return ENOENT,
    };

    let mut table = PROCESS_TABLE.lock();
    let current = table.current;

    if current >= table.processes.len() {
        return EINVAL;
    }

    let process = &mut table.processes[current];

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
            let fd = process.fd_table.len();
            process.fd_table.push(Some(FdEntry::File {
                file_index,
                position: 0,
            }));
            fd as i64
        }
    }
}

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
        Some(FdEntry::Stdin) | Some(FdEntry::Stdout) | Some(FdEntry::Stderr) => EPERM,
        Some(FdEntry::File { .. }) => {
            process.fd_table[fd_idx] = None;
            0
        }
        None => EBADF,
    }
}
