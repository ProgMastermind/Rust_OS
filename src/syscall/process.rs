// Process Syscalls
//
// System calls related to process management.

use crate::process::PROCESS_TABLE;

// sys_exit(code) -> never returns
//
// Terminate the current process. The scheduler will never run it again.
// In a real OS, the exit code would be stored for the parent to read
// via waitpid(). For now, we just mark the process as terminated.
pub fn sys_exit(code: u64) -> i64 {
    crate::serial_println!("Process exited with code {}", code);

    let mut table = PROCESS_TABLE.lock();
    let current = table.current;
    table.processes[current].state = crate::process::ProcessState::Terminated;
    drop(table);

    // Halt until the scheduler switches us out.
    // We'll never be scheduled again because state = Terminated.
    loop {
        x86_64::instructions::hlt();
    }
}

// sys_getpid() -> pid
//
// Return the current process's PID.
pub fn sys_getpid() -> i64 {
    let table = PROCESS_TABLE.lock();
    let current = table.current;
    table.processes[current].pid as i64
}
