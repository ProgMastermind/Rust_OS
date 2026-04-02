// Process Syscalls
//
// System calls related to process management.

use crate::process::PROCESS_TABLE;

// sys_exit(code) -> never returns
//
// Terminate the current process. The scheduler will never run it again.
pub fn sys_exit(code: u64) -> i64 {
    crate::serial_println!("Process exited with code {}", code);

    let mut table = PROCESS_TABLE.lock();
    let current = table.current;
    if current < table.processes.len() {
        table.processes[current].state = crate::process::ProcessState::Terminated;
    }
    drop(table);

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
    // Bounds check: don't panic on invalid index
    match table.processes.get(current) {
        Some(process) => process.pid as i64,
        None => super::EINVAL,
    }
}
