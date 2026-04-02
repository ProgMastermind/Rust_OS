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
// This is the same mechanism Linux used before `syscall`/`sysret` instructions.
// `int 0x80` is slower than `syscall`, but conceptually identical.
//
// Note: args are passed through a shared struct instead of registers because
// the `extern "x86-interrupt"` calling convention doesn't give us access to
// the caller's general-purpose registers. A production OS would use a naked
// asm handler stub to read registers directly. Our approach is functionally
// correct and demonstrates the concept.

pub mod fs;
pub mod process;

use x86_64::structures::idt::InterruptStackFrame;

// ── Syscall Numbers ─────────────────────────────────────────────────
// Each syscall has a unique number. The caller puts this in the args.

pub const SYS_WRITE: u64 = 0;   // write(fd, buf_ptr, len) -> bytes_written
pub const SYS_READ: u64 = 1;    // read(fd, buf_ptr, len) -> bytes_read
pub const SYS_OPEN: u64 = 2;    // open(path_ptr, path_len, _) -> fd
pub const SYS_CLOSE: u64 = 3;   // close(fd, _, _) -> 0 or -1
pub const SYS_EXIT: u64 = 4;    // exit(code, _, _) -> never returns
pub const SYS_GETPID: u64 = 5;  // getpid(_, _, _) -> pid

// ── Shared Args Struct ──────────────────────────────────────────────
//
// Passed between the syscall wrapper (caller side) and the IDT handler
// (kernel side). This is safe because:
//   - The caller disables interrupts before writing (no preemption)
//   - `int 0x80` keeps interrupts disabled (CPU clears IF on entry)
//   - Single CPU core — no concurrent access
//   - The handler runs to completion before returning

struct SyscallArgs {
    number: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    return_value: i64,
}

static mut SYSCALL_ARGS: SyscallArgs = SyscallArgs {
    number: 0,
    arg1: 0,
    arg2: 0,
    arg3: 0,
    return_value: 0,
};

// ── Syscall Wrapper (caller side) ───────────────────────────────────
//
// This is what "user" code calls. It:
//   1. Disables interrupts (prevents scheduler from switching us out
//      between writing args and executing int 0x80)
//   2. Writes syscall number + args to the shared struct
//   3. Triggers software interrupt 0x80
//   4. Reads the return value set by the handler
//
// Returns the syscall result (meaning depends on which syscall).

pub fn syscall(number: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    use core::arch::asm;

    x86_64::instructions::interrupts::without_interrupts(|| {
        unsafe {
            SYSCALL_ARGS.number = number;
            SYSCALL_ARGS.arg1 = arg1;
            SYSCALL_ARGS.arg2 = arg2;
            SYSCALL_ARGS.arg3 = arg3;

            // int 0x80: trigger software interrupt.
            // CPU saves RFLAGS/CS/RIP, clears IF, jumps to IDT[0x80].
            // The handler processes the syscall and stores the result.
            // iretq returns here with RFLAGS restored.
            //
            // No `nomem` option: the handler reads/writes SYSCALL_ARGS,
            // and the asm! block acts as a compiler fence preventing
            // reordering of the writes above past this point.
            asm!("int 0x80", options(nostack));

            SYSCALL_ARGS.return_value
        }
    })
}

// ── IDT Handler (kernel side) ───────────────────────────────────────
//
// Registered in IDT entry 0x80 (see interrupts.rs).
// Dispatches to the appropriate syscall implementation based on the number.

pub extern "x86-interrupt" fn syscall_handler(_frame: InterruptStackFrame) {
    let (number, arg1, arg2, arg3) = unsafe {
        (
            SYSCALL_ARGS.number,
            SYSCALL_ARGS.arg1,
            SYSCALL_ARGS.arg2,
            SYSCALL_ARGS.arg3,
        )
    };

    let result = match number {
        SYS_WRITE => fs::sys_write(arg1, arg2, arg3),
        SYS_READ => fs::sys_read(arg1, arg2, arg3),
        SYS_OPEN => fs::sys_open(arg1, arg2),
        SYS_CLOSE => fs::sys_close(arg1),
        SYS_EXIT => process::sys_exit(arg1),
        SYS_GETPID => process::sys_getpid(),
        _ => -1, // Unknown syscall
    };

    unsafe {
        SYSCALL_ARGS.return_value = result;
    }
}
