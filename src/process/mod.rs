// Process Management
//
// A "process" in our kernel is:
//   - A saved stack pointer (RSP) pointing to saved registers on the stack
//   - A kernel stack (heap-allocated memory for this process's stack)
//   - A state (Ready, Running, Terminated)
//   - A unique PID
//   - The entry function to execute (stored directly in the PCB)
//
// The process table holds all processes. The scheduler picks which one
// runs next, and context_switch swaps the CPU to it.
//
// NOTE: These are kernel threads — they share the same address space.
// Per-process address spaces (CR3 switching, Ring 3) come in Session 6.

pub mod context_switch;
pub mod scheduler;

use alloc::vec::Vec;
use spin::Mutex;

// Size of each process's kernel stack: 16KB.
// This is where the process's local variables, function call frames,
// and saved registers live.
const STACK_SIZE: usize = 4096 * 4; // 16KB

// ── Process States ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Ready,      // Can be scheduled — waiting for CPU time
    Running,    // Currently executing on the CPU
    Terminated, // Finished — will be cleaned up
}

// ── Process Struct ────────────────────────────────────────────────────
//
// This is the Process Control Block (PCB). Everything the kernel needs
// to know about a process to suspend and resume it.

pub struct Process {
    pub pid: u64,
    pub state: ProcessState,
    // The saved stack pointer. When this process is not running, this
    // points to the top of its saved register state on its stack.
    // When context_switch saves this process, it stores RSP here.
    // When context_switch restores this process, it loads RSP from here.
    pub stack_pointer: u64,
    // The entry function this process executes. Stored directly in the PCB
    // so process_entry can read it without a separate global registry.
    // None for the idle process (kernel_main), which doesn't need one.
    pub entry_fn: Option<fn()>,
    // The actual stack memory. We heap-allocate it as a Vec<u8>.
    // The process's RSP points somewhere inside this Vec.
    // We keep ownership here so the stack isn't freed while in use.
    pub _stack: Vec<u8>,
}

// ── Process Table ─────────────────────────────────────────────────────
//
// Global table of all processes, protected by a spinlock.
// We also track which process is currently running (by index).

pub struct ProcessTable {
    pub processes: Vec<Process>,
    pub current: usize, // Index of the currently running process
    pub next_pid: u64,
}

// Global process table — accessed from the scheduler (timer interrupt)
// and from kernel_main (spawn).
pub static PROCESS_TABLE: Mutex<ProcessTable> = Mutex::new(ProcessTable {
    processes: Vec::new(),
    current: 0,
    next_pid: 0,
});

impl ProcessTable {
    // Spawn a new process that will execute `entry_point`.
    //
    // The entry_point function pointer is stored directly in the Process
    // struct (PCB). When this process is first scheduled, context_switch
    // jumps to process_entry(), which reads entry_fn from the PCB and
    // calls it. No separate registry needed.
    //
    // Stack alignment (System V ABI requirement):
    //   The x86_64 ABI requires that at function entry, RSP ≡ 8 (mod 16).
    //   This is because a `call` instruction pushes an 8-byte return address
    //   onto a 16-aligned stack, so the callee sees RSP = 16n - 8.
    //
    //   Our context_switch does `ret`, which pops the return address and
    //   jumps to it. After `ret`, RSP moves up by 8. So we need RSP after
    //   ret to be ≡ 8 (mod 16) for process_entry to have correct alignment.
    //
    //   Trace through the math:
    //     stack_top is 16-aligned (0 mod 16)
    //     We place 8 values (64 bytes) below it:
    //       [r15] [r14] [r13] [r12] [rbx] [rbp] [ret_addr] [padding]
    //     frame_start = stack_top - 64 = 0 mod 16
    //     After 6 pops (48 bytes): RSP = frame_start + 48 = stack_top - 16
    //     ret pops return addr (8 bytes): RSP = stack_top - 8 = 8 mod 16 ✓
    //
    //   The padding slot at the top ensures the return address sits at the
    //   right offset for correct post-ret alignment.
    //
    // Initial stack layout (low address → high address):
    //   [r15=0] [r14=0] [r13=0] [r12=0] [rbx=0] [rbp=0] [ret_addr] [padding=0]
    //   ^                                                                        ^
    //   RSP (frame_start)                                      stack_top (16-aligned)
    pub fn spawn(&mut self, entry_point: fn()) {
        let pid = self.next_pid;
        self.next_pid += 1;

        // Allocate a stack for this process (zeroed out).
        // Vec<u8> may not be 16-byte aligned, so we align stack_top manually.
        let stack = alloc::vec![0u8; STACK_SIZE];

        // stack_top = highest usable address, aligned DOWN to 16 bytes.
        // The & !0xF mask clears the bottom 4 bits, giving a multiple of 16.
        let stack_end = stack.as_ptr() as usize + STACK_SIZE;
        let stack_top = stack_end & !0xF; // 16-byte aligned

        // 8 values × 8 bytes = 64 bytes below stack_top.
        // The 8th slot is padding to fix alignment:
        //   After 6 pops: RSP = stack_top - 16
        //   After ret:    RSP = stack_top - 8 = 8 mod 16 ✓ (ABI correct)
        let frame_start = stack_top - 64;

        // Build the initial stack frame.
        // The first 6 entries are callee-saved registers (all zero for fresh process).
        // Entry 7 is the return address — where `ret` will jump.
        // Entry 8 is padding to ensure process_entry sees RSP ≡ 8 (mod 16).
        let frame: [u64; 8] = [
            0,                                       // r15
            0,                                       // r14
            0,                                       // r13
            0,                                       // r12
            0,                                       // rbx
            0,                                       // rbp
            process_entry as *const () as u64,        // return address
            0,                                       // alignment padding
        ];

        // Copy the frame onto the stack
        unsafe {
            let dest = frame_start as *mut [u64; 8];
            dest.write(frame);
        }

        let process = Process {
            pid,
            state: ProcessState::Ready,
            stack_pointer: frame_start as u64,
            entry_fn: Some(entry_point), // Stored in the PCB — no global registry
            _stack: stack,
        };

        self.processes.push(process);
    }
}

// ── Process Entry Wrapper ─────────────────────────────────────────────
//
// The first function a new process executes. context_switch's `ret` lands
// here. We:
//   1. Re-enable interrupts (they were disabled by the timer handler that
//      triggered the context switch — without this, the timer never fires
//      again and scheduling stops)
//   2. Read our entry function from the PCB (stored directly by spawn)
//   3. Call it
//   4. If it returns, call exit() to mark ourselves as Terminated

fn process_entry() {
    // Re-enable interrupts. The context switch happened inside a timer
    // interrupt handler where the CPU disabled IF. If we don't re-enable,
    // no more timer ticks = no scheduling = this process runs forever.
    x86_64::instructions::interrupts::enable();

    // Read our entry function directly from the PCB — no global registry.
    let entry = {
        let table = PROCESS_TABLE.lock();
        table.processes[table.current].entry_fn
    };

    if let Some(func) = entry {
        func();
    }

    // If the function returns, mark process as terminated
    exit();
}

// Mark the current process as terminated.
// The scheduler will skip it from now on.
pub fn exit() {
    let mut table = PROCESS_TABLE.lock();
    let current = table.current;
    table.processes[current].state = ProcessState::Terminated;
    drop(table); // Release lock before halting

    // Wait for the scheduler to switch us out.
    // We'll never be scheduled again because state = Terminated.
    loop {
        x86_64::instructions::hlt();
    }
}
