// Round-Robin Scheduler
//
// The simplest fair scheduling algorithm:
//   1. Timer interrupt fires (~18 times/sec)
//   2. Save current process's state
//   3. Pick the NEXT Ready process (wrap around at the end)
//   4. Context switch to it
//
// Every process gets an equal time slice (~55ms per tick).
// No priorities, no fairness weights — just take turns.
//
// This is called from the timer interrupt handler in interrupts.rs.

use super::context_switch::context_switch;
use super::{ProcessState, PROCESS_TABLE};

// Called from the timer interrupt handler.
// Decides whether to switch processes and performs the context switch.
pub fn schedule() {
    // Try to lock the process table. If we can't (another interrupt is
    // holding it), skip this tick. try_lock() is essential here — if we
    // used lock() and the timer interrupted code that already holds the
    // lock, we'd deadlock.
    let mut table = match PROCESS_TABLE.try_lock() {
        Some(table) => table,
        None => return, // Skip this tick — table is locked
    };

    let num_processes = table.processes.len();

    // Nothing to schedule if there are 0 or 1 processes
    if num_processes <= 1 {
        return;
    }

    let current_idx = table.current;

    // Find the next Ready process (round-robin: check each one in order)
    let mut next_idx = (current_idx + 1) % num_processes;
    let mut found = false;

    for _ in 0..num_processes {
        if table.processes[next_idx].state == ProcessState::Ready {
            found = true;
            break;
        }
        next_idx = (next_idx + 1) % num_processes;
    }

    if !found || next_idx == current_idx {
        return;
    }

    // ── Perform the switch ────────────────────────────────────────────

    // Mark old process as Ready (it's not Running anymore, but can be
    // scheduled again). Only if it's still Running — it might have been
    // marked Terminated by exit().
    if table.processes[current_idx].state == ProcessState::Running {
        table.processes[current_idx].state = ProcessState::Ready;
    }

    // Mark new process as Running
    table.processes[next_idx].state = ProcessState::Running;
    table.current = next_idx;

    // Get the raw pointers we need for context_switch.
    // We need:
    //   - A pointer to old process's stack_pointer field (to save RSP into)
    //   - The value of new process's stack_pointer (to load RSP from)
    let old_rsp_ptr = &mut table.processes[current_idx].stack_pointer as *mut u64;
    let new_rsp = table.processes[next_idx].stack_pointer;

    // CRITICAL: Drop the lock BEFORE context switching.
    // If we hold the lock during the switch, the new process can't
    // acquire it when IT gets interrupted (deadlock).
    drop(table);

    // Perform the actual context switch.
    // After this call returns (from the perspective of the OLD process),
    // we've been switched back. Time has passed — other processes ran.
    unsafe {
        context_switch(old_rsp_ptr, new_rsp);
    }
}
