// Process syscalls: exit, getpid.

use crate::process::PROCESS_TABLE;

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

pub fn sys_getpid() -> i64 {
    let table = PROCESS_TABLE.lock();
    let current = table.current;
    match table.processes.get(current) {
        Some(process) => process.pid as i64,
        None => super::EINVAL,
    }
}
