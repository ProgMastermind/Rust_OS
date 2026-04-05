// Syscall interface via int 0x80. Args passed through a shared static
// (interrupts disabled during access, single core, no nesting).

pub mod fs;
pub mod process;

use core::cell::UnsafeCell;
use x86_64::structures::idt::InterruptStackFrame;

pub const SYS_WRITE: u64 = 0;
pub const SYS_READ: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_EXIT: u64 = 4;
pub const SYS_GETPID: u64 = 5;

pub const ENOENT: i64 = -1;
pub const EBADF: i64 = -2;
pub const EINVAL: i64 = -3;
pub const EFAULT: i64 = -4;
pub const EPERM: i64 = -5;
pub const ENOSYS: i64 = -6;
pub const EROFS: i64 = -7;

pub fn errno_name(code: i64) -> &'static str {
    match code {
        x if x >= 0 => "OK",
        -1 => "ENOENT (file not found)",
        -2 => "EBADF (bad file descriptor)",
        -3 => "EINVAL (invalid argument)",
        -4 => "EFAULT (bad address)",
        -5 => "EPERM (not permitted)",
        -6 => "ENOSYS (unknown syscall)",
        -7 => "EROFS (read-only filesystem)",
        _ => "UNKNOWN",
    }
}

struct SyscallArgs {
    number: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    return_value: i64,
}

// SAFETY: exclusive access guaranteed by IF=0 + single core + no nesting
struct SyncUnsafeCell(UnsafeCell<SyscallArgs>);
unsafe impl Sync for SyncUnsafeCell {}

static SYSCALL_ARGS: SyncUnsafeCell = SyncUnsafeCell(UnsafeCell::new(SyscallArgs {
    number: 0,
    arg1: 0,
    arg2: 0,
    arg3: 0,
    return_value: 0,
}));

const MAX_SYSCALL_BUFFER_LEN: u64 = 1024 * 1024;

pub fn validate_ptr(ptr: u64, len: u64) -> Result<(), i64> {
    if ptr == 0 {
        return Err(EFAULT);
    }
    if len > MAX_SYSCALL_BUFFER_LEN {
        return Err(EINVAL);
    }
    if ptr.checked_add(len).is_none() {
        return Err(EFAULT);
    }
    Ok(())
}

pub unsafe fn slice_from_user_ptr(ptr: u64, len: u64) -> Result<&'static [u8], i64> {
    validate_ptr(ptr, len)?;
    Ok(unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) })
}

pub unsafe fn slice_from_user_ptr_mut(ptr: u64, len: u64) -> Result<&'static mut [u8], i64> {
    validate_ptr(ptr, len)?;
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len as usize) })
}

pub fn syscall(number: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    use core::arch::asm;

    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: IF=0, single core, handler runs to completion
        let args = unsafe { &mut *SYSCALL_ARGS.0.get() };
        args.number = number;
        args.arg1 = arg1;
        args.arg2 = arg2;
        args.arg3 = arg3;

        unsafe { asm!("int 0x80", options(nostack)) };

        let args = unsafe { &*SYSCALL_ARGS.0.get() };
        args.return_value
    })
}

pub extern "x86-interrupt" fn syscall_handler(_frame: InterruptStackFrame) {
    // SAFETY: inside int 0x80 handler, IF=0
    let args = unsafe { &*SYSCALL_ARGS.0.get() };
    let (number, arg1, arg2, arg3) = (args.number, args.arg1, args.arg2, args.arg3);

    let result = match number {
        SYS_WRITE => fs::sys_write(arg1, arg2, arg3),
        SYS_READ => fs::sys_read(arg1, arg2, arg3),
        SYS_OPEN => fs::sys_open(arg1, arg2),
        SYS_CLOSE => fs::sys_close(arg1),
        SYS_EXIT => process::sys_exit(arg1),
        SYS_GETPID => process::sys_getpid(),
        unknown => {
            crate::serial_println!("WARNING: unknown syscall {}", unknown);
            ENOSYS
        }
    };

    let args = unsafe { &mut *SYSCALL_ARGS.0.get() };
    args.return_value = result;
}
