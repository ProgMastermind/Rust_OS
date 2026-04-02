// System Call Interface
//
// System calls are the ONLY way user code can request kernel services.
// Even though our "user" code currently runs in Ring 0 (kernel mode),
// we implement the full syscall mechanism:
//
//   1. User code calls syscall(number, arg1, arg2, arg3)
//   2. The wrapper stores args in a shared struct
//   3. Executes `int 0x80` — software interrupt
//   4. CPU jumps to our IDT[0x80] handler (same as hardware interrupts)
//   5. Handler reads args, dispatches to the right kernel function
//   6. Stores return value, returns via `iretq`
//   7. Wrapper reads return value and returns it to the caller
//
// Note: args are passed through a shared struct instead of registers because
// the `extern "x86-interrupt"` calling convention doesn't give us access to
// the caller's general-purpose registers. A production OS would use a naked
// asm handler stub to read registers directly.

pub mod fs;
pub mod process;

use core::cell::UnsafeCell;
use x86_64::structures::idt::InterruptStackFrame;

// ── Syscall Numbers ─────────────────────────────────────────────────

pub const SYS_WRITE: u64 = 0;   // write(fd, buf_ptr, len) -> bytes_written
pub const SYS_READ: u64 = 1;    // read(fd, buf_ptr, len) -> bytes_read
pub const SYS_OPEN: u64 = 2;    // open(path_ptr, path_len, _) -> fd
pub const SYS_CLOSE: u64 = 3;   // close(fd, _, _) -> 0 or error
pub const SYS_EXIT: u64 = 4;    // exit(code, _, _) -> never returns
pub const SYS_GETPID: u64 = 5;  // getpid(_, _, _) -> pid

// ── Error Codes (errno-style) ───────────────────────────────────────
//
// Syscalls return negative values on error. Each negative value identifies
// a specific error condition, so callers can distinguish "file not found"
// from "bad pointer" from "invalid fd."

pub const ENOENT: i64 = -1;  // No such file or directory
pub const EBADF: i64 = -2;   // Bad file descriptor
pub const EINVAL: i64 = -3;  // Invalid argument
pub const EFAULT: i64 = -4;  // Bad address (pointer validation failed)
pub const EPERM: i64 = -5;   // Operation not permitted
pub const ENOSYS: i64 = -6;  // Unknown syscall number
pub const EROFS: i64 = -7;   // Read-only file system (can't write to ramdisk files)

// Convert an error code to a human-readable name.
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

// ── Shared Args Struct ──────────────────────────────────────────────
//
// Uses UnsafeCell instead of `static mut` for interior mutability.
// This is the correct Rust primitive for "I need to mutate a static,
// and I'll ensure safety through external synchronization."
//
// Safety contract (enforced by the caller, not the type system):
//   1. The syscall wrapper disables interrupts before writing args
//   2. `int 0x80` keeps interrupts disabled in the handler
//   3. Single CPU core — no concurrent access from another core
//   4. Only one syscall can be in-flight at a time (no nesting)
//
// These four guarantees mean only one thread of execution accesses
// the struct at any time. UnsafeCell + unsafe get/set is appropriate.

struct SyscallArgs {
    number: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    return_value: i64,
}

// Wrapper to make UnsafeCell<SyscallArgs> usable in a static.
// UnsafeCell is !Sync by default (can't be shared between threads).
// We implement Sync because our safety contract (above) guarantees
// exclusive access via interrupt disabling.
struct SyncUnsafeCell(UnsafeCell<SyscallArgs>);
unsafe impl Sync for SyncUnsafeCell {}

static SYSCALL_ARGS: SyncUnsafeCell = SyncUnsafeCell(UnsafeCell::new(SyscallArgs {
    number: 0,
    arg1: 0,
    arg2: 0,
    arg3: 0,
    return_value: 0,
}));

// ── Pointer Validation ──────────────────────────────────────────────
//
// Before creating a slice from a raw pointer, we validate that:
//   1. The pointer is not null
//   2. ptr + len doesn't overflow (wrap around address space)
//   3. len is within a reasonable bound (prevents absurd allocations)
//
// In a real Ring 3 setup, we'd also check that the address range falls
// within the caller's user-space region. For Ring 0 (current setup),
// these basic checks catch the most common bugs.

const MAX_SYSCALL_BUFFER_LEN: u64 = 1024 * 1024; // 1MB sanity limit

pub fn validate_ptr(ptr: u64, len: u64) -> Result<(), i64> {
    if ptr == 0 {
        return Err(EFAULT); // Null pointer
    }
    if len > MAX_SYSCALL_BUFFER_LEN {
        return Err(EINVAL); // Unreasonably large
    }
    if ptr.checked_add(len).is_none() {
        return Err(EFAULT); // Overflow — ptr + len wraps around
    }
    Ok(())
}

// Create a read-only slice from a validated pointer.
// Returns Err(EFAULT) if the pointer fails validation.
pub unsafe fn slice_from_user_ptr(ptr: u64, len: u64) -> Result<&'static [u8], i64> {
    validate_ptr(ptr, len)?;
    Ok(unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) })
}

// Create a mutable slice from a validated pointer.
pub unsafe fn slice_from_user_ptr_mut(ptr: u64, len: u64) -> Result<&'static mut [u8], i64> {
    validate_ptr(ptr, len)?;
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len as usize) })
}

// ── Syscall Wrapper (caller side) ───────────────────────────────────

pub fn syscall(number: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    use core::arch::asm;

    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: Exclusive access guaranteed by:
        //   - Interrupts disabled (without_interrupts)
        //   - Single core
        //   - int 0x80 handler runs to completion before iretq
        let args = unsafe { &mut *SYSCALL_ARGS.0.get() };
        args.number = number;
        args.arg1 = arg1;
        args.arg2 = arg2;
        args.arg3 = arg3;

        unsafe { asm!("int 0x80", options(nostack)) };

        // Read return value (handler stored it before iretq)
        let args = unsafe { &*SYSCALL_ARGS.0.get() };
        args.return_value
    })
}

// ── IDT Handler (kernel side) ───────────────────────────────────────

pub extern "x86-interrupt" fn syscall_handler(_frame: InterruptStackFrame) {
    // SAFETY: We're inside an interrupt handler with IF=0.
    // No other code can access SYSCALL_ARGS concurrently.
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
