// Context switch. Push callee-saved regs, save RSP, load new RSP, pop regs, ret.
// Only callee-saved registers (rbp, rbx, r12-r15) need saving since
// context_switch looks like a function call to the old process.
// rdi = pointer to old process's saved RSP, rsi = new process's RSP.

use core::arch::naked_asm;

#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(old_rsp: *mut u64, new_rsp: u64) {
    naked_asm!(
        // Save old process
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdi], rsp",

        // Load new process
        "mov rsp, rsi",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    );
}
