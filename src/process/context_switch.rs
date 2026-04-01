// Context Switch — the mechanism that switches the CPU between processes.
//
// This is ~15 lines of inline assembly that implement ALL of multitasking.
//
// How it works:
//   1. Save the current process's callee-saved registers onto its stack
//   2. Save the current stack pointer (RSP) into the old process's struct
//   3. Load the new process's stack pointer
//   4. Pop the new process's registers from its stack
//   5. `ret` — pops the return address from the NEW stack, jumping to
//      wherever the new process was when it was last switched away from
//
// Why only callee-saved registers (rbp, rbx, r12-r15)?
//   The x86_64 System V ABI says that a function call may clobber
//   rax, rcx, rdx, rsi, rdi, r8-r11. The caller already saved those
//   if it needed them. But rbp, rbx, r12-r15 must be preserved across
//   function calls. Since context_switch looks like a function call to
//   the old process (it "calls" switch, and later "returns"), we only
//   need to save the callee-saved registers.

use core::arch::naked_asm;

// Switch from the currently running process to a new one.
//
// `old_rsp`: pointer to where we should save the current RSP
//            (this is &mut process.stack_pointer for the old process)
// `new_rsp`: the saved RSP of the process we're switching TO
//
// After this function:
//   - The old process's state is saved (registers on its stack, RSP stored)
//   - The CPU is now executing the new process, with its registers and stack
//
// SAFETY: The caller must ensure:
//   - old_rsp points to valid, writable memory
//   - new_rsp was previously saved by a context_switch call (or set up
//     by spawn() to look like one)
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(old_rsp: *mut u64, new_rsp: u64) {
    // #[unsafe(naked)] means the compiler generates NO prologue/epilogue for
    // this function. No push rbp, no sub rsp, nothing. We control every
    // instruction. This is essential because we're manipulating the stack
    // pointer directly — compiler-generated stack frame setup would break.
    naked_asm!(
        // ── Save old process ──────────────────────────────
        // Push callee-saved registers onto the OLD process's stack.
        // After these pushes, the stack looks like:
        //   [... old stack ...]
        //   [return address]  ← pushed by the `call` that invoked us
        //   [rbp]
        //   [rbx]
        //   [r12]
        //   [r13]
        //   [r14]
        //   [r15]            ← RSP points here
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Save the current RSP into the old process's struct.
        // rdi = old_rsp (first argument in System V ABI)
        // After this, old_process.stack_pointer holds the RSP value
        // that points to the saved registers above.
        "mov [rdi], rsp",

        // ── Load new process ──────────────────────────────
        // Switch to the new process's stack.
        // rsi = new_rsp (second argument in System V ABI)
        "mov rsp, rsi",

        // Pop the new process's callee-saved registers.
        // These were saved the last time this process was switched away from.
        // (Or set up by spawn() for a brand-new process.)
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",

        // Return to the new process.
        // `ret` pops the return address from the stack and jumps to it.
        // For a previously-running process: this returns to wherever it
        //   was when context_switch was last called (inside the timer handler).
        // For a brand-new process: this jumps to the process_entry function
        //   that spawn() placed on the stack.
        "ret",
    );
}
