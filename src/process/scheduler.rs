// Round-robin scheduler. Called from the timer ISR ~18 times/sec.

use super::context_switch::context_switch;
use super::{ProcessState, PROCESS_TABLE};
use alloc::vec::Vec;

pub fn schedule() {
    let mut table = match PROCESS_TABLE.try_lock() {
        Some(table) => table,
        None => return,
    };

    let num_processes = table.processes.len();
    if num_processes <= 1 {
        return;
    }

    // Reap terminated processes (except the currently running one)
    let current_idx = table.current;
    for i in 0..num_processes {
        if i == current_idx {
            continue;
        }
        if table.processes[i].state == ProcessState::Terminated {
            // Stack pages stay mapped until spawn() reuses the slot (avoids locking mapper in ISR)
            table.processes[i].fd_table = Vec::new();
            table.processes[i].entry_fn = None;
            table.processes[i].state = ProcessState::Empty;
            crate::serial_println!(
                "Reaped process PID {}",
                table.processes[i].pid
            );
        }
    }

    // Find next Ready process
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

    if table.processes[current_idx].state == ProcessState::Running {
        table.processes[current_idx].state = ProcessState::Ready;
    }

    table.processes[next_idx].state = ProcessState::Running;
    table.current = next_idx;

    let old_rsp_ptr = &mut table.processes[current_idx].stack_pointer as *mut u64;
    let new_rsp = table.processes[next_idx].stack_pointer;

    // Must drop lock before switching -- otherwise the new process can't acquire it
    drop(table);

    unsafe {
        context_switch(old_rsp_ptr, new_rsp);
    }
}
