// Round-Robin Scheduler
//
// The simplest fair scheduling algorithm:
//   1. Timer interrupt fires (~18 times/sec)
//   2. Reap any terminated processes (free their resources)
//   3. Save current process's state
//   4. Pick the NEXT Ready process (wrap around at the end)
//   5. Context switch to it
//
// Every process gets an equal time slice (~55ms per tick).
// No priorities, no fairness weights — just take turns.
//
// This is called from the timer interrupt handler in interrupts.rs.

use super::context_switch::context_switch;
use super::{ProcessState, PROCESS_TABLE};
use alloc::vec::Vec;

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

    // ── Reap terminated processes ────────────────────────────────────
    //
    // Free the stack and fd_table of any Terminated process (except the
    // currently running one — we can't free our own stack while using it).
    // The slot stays in the Vec but its resources are released. The state
    // changes to Empty so spawn() can reuse the slot later.
    let current_idx = table.current;
    for i in 0..num_processes {
        if i == current_idx {
            continue; // Don't reap ourselves
        }
        if table.processes[i].state == ProcessState::Terminated {
            // Free the process's resources
            table.processes[i]._stack = Vec::new();     // Drop the 16KB stack
            table.processes[i].fd_table = Vec::new();   // Drop fd table
            table.processes[i].entry_fn = None;
            table.processes[i].state = ProcessState::Empty;
            crate::serial_println!(
                "Reaped process PID {}",
                table.processes[i].pid
            );
        }
    }

    // ── Find the next Ready process ─────────────────────────────────

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
        return; // No other Ready process — keep running current
    }

    // ── Perform the switch ──────────────────────────────────────────

    if table.processes[current_idx].state == ProcessState::Running {
        table.processes[current_idx].state = ProcessState::Ready;
    }

    table.processes[next_idx].state = ProcessState::Running;
    table.current = next_idx;

    let old_rsp_ptr = &mut table.processes[current_idx].stack_pointer as *mut u64;
    let new_rsp = table.processes[next_idx].stack_pointer;

    // Drop the lock BEFORE context switching to avoid deadlock.
    drop(table);

    unsafe {
        context_switch(old_rsp_ptr, new_rsp);
    }
}
