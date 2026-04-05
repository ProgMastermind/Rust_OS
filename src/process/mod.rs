// Process management. Kernel threads sharing one address space.

pub mod context_switch;
pub mod scheduler;

use alloc::vec::Vec;
use spin::Mutex;
use x86_64::structures::paging::{Page, Size4KiB};
use x86_64::VirtAddr;

// Stack layout: [guard page (unmapped, 4KB)] [stack (mapped, 16KB)]
// Guard page catches stack overflow with a page fault instead of silent corruption.
const STACK_PAGES: usize = 4;
const GUARD_PAGES: usize = 1;
const PAGES_PER_SLOT: usize = STACK_PAGES + GUARD_PAGES;
const STACK_REGION_BASE: u64 = 0x5555_0000_0000;

pub struct StackRegion {
    pub guard_page_addr: u64,
    pub stack_bottom: u64,
    pub stack_top: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaitReason {
    Stdin,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked(WaitReason),
    Terminated,
    Empty,
}

pub struct Process {
    pub pid: u64,
    pub state: ProcessState,
    pub stack_pointer: u64,
    pub entry_fn: Option<fn()>,
    pub stack_region: Option<StackRegion>, // None for idle process (boot stack)
    pub fd_table: Vec<Option<crate::fs::FdEntry>>,
}

pub struct ProcessTable {
    pub processes: Vec<Process>,
    pub current: usize,
    pub next_pid: u64,
}

pub static PROCESS_TABLE: Mutex<ProcessTable> = Mutex::new(ProcessTable {
    processes: Vec::new(),
    current: 0,
    next_pid: 0,
});

impl ProcessTable {
    /// Spawn a new process. Maps stack pages at a dedicated virtual address
    /// with an unmapped guard page below.
    ///
    /// Stack alignment: x86_64 SysV ABI requires RSP = 8 mod 16 at function entry.
    /// Initial frame: [r15 r14 r13 r12 rbx rbp ret_addr padding]
    ///   frame_start = stack_top - 64, after 6 pops + ret: RSP = stack_top - 8 = 8 mod 16
    pub fn spawn(&mut self, entry_point: fn()) {
        let pid = self.next_pid;
        self.next_pid += 1;

        let slot_idx = self
            .processes
            .iter()
            .position(|p| p.state == ProcessState::Empty)
            .unwrap_or(self.processes.len());

        // Unmap old stack if reusing a slot
        if slot_idx < self.processes.len() {
            if let Some(ref region) = self.processes[slot_idx].stack_region {
                let start_page =
                    Page::<Size4KiB>::containing_address(VirtAddr::new(region.stack_bottom));
                crate::memory::unmap_pages(start_page, STACK_PAGES);
            }
        }

        let region_base =
            STACK_REGION_BASE + (slot_idx as u64) * (PAGES_PER_SLOT as u64) * 4096;
        let guard_page_addr = region_base;
        let stack_bottom = region_base + (GUARD_PAGES as u64) * 4096;
        let stack_top = stack_bottom + (STACK_PAGES as u64) * 4096;

        let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack_bottom));
        crate::memory::map_pages(start_page, STACK_PAGES)
            .expect("failed to map process stack pages");

        unsafe {
            core::ptr::write_bytes(stack_bottom as *mut u8, 0, STACK_PAGES * 4096);
        }

        let aligned_stack_top = (stack_top as usize) & !0xF;
        let frame_start = aligned_stack_top - 64;

        let frame: [u64; 8] = [
            0,                                        // r15
            0,                                        // r14
            0,                                        // r13
            0,                                        // r12
            0,                                        // rbx
            0,                                        // rbp
            process_entry as *const () as u64,         // return address
            0,                                        // padding (ABI alignment)
        ];

        unsafe {
            let dest = frame_start as *mut [u64; 8];
            dest.write(frame);
        }

        let process = Process {
            pid,
            state: ProcessState::Ready,
            stack_pointer: frame_start as u64,
            entry_fn: Some(entry_point),
            stack_region: Some(StackRegion {
                guard_page_addr,
                stack_bottom,
                stack_top,
            }),
            fd_table: alloc::vec![
                Some(crate::fs::FdEntry::Stdin),
                Some(crate::fs::FdEntry::Stdout),
                Some(crate::fs::FdEntry::Stderr),
            ],
        };

        if slot_idx < self.processes.len() {
            self.processes[slot_idx] = process;
        } else {
            self.processes.push(process);
        }
    }
}

/// Check if `addr` falls in a stack guard page region.
pub fn is_guard_page(addr: u64) -> bool {
    if addr < STACK_REGION_BASE {
        return false;
    }
    let offset = addr - STACK_REGION_BASE;
    let slot_size = (PAGES_PER_SLOT as u64) * 4096;
    let within_slot = offset % slot_size;
    within_slot < 4096
}

fn process_entry() {
    // Re-enable interrupts (context switch happened inside timer ISR with IF=0)
    x86_64::instructions::interrupts::enable();

    let entry = {
        let table = PROCESS_TABLE.lock();
        table.processes[table.current].entry_fn
    };

    if let Some(func) = entry {
        func();
    }

    exit();
}

pub fn exit() {
    let mut table = PROCESS_TABLE.lock();
    let current = table.current;
    table.processes[current].state = ProcessState::Terminated;
    drop(table);

    loop {
        x86_64::instructions::hlt();
    }
}

/// Mark current process as Blocked and wait until woken by wake_blocked().
pub fn block_current(reason: WaitReason) {
    {
        let mut table = PROCESS_TABLE.lock();
        let current = table.current;
        table.processes[current].state = ProcessState::Blocked(reason);
    }

    loop {
        x86_64::instructions::interrupts::enable_and_hlt();

        let still_blocked = {
            let table = PROCESS_TABLE.lock();
            let current = table.current;
            matches!(table.processes[current].state, ProcessState::Blocked(_))
        };

        if !still_blocked {
            break;
        }
    }
}

/// Wake all processes blocked on the given reason. Called from ISR context.
pub fn wake_blocked(reason: WaitReason) {
    if let Some(mut table) = PROCESS_TABLE.try_lock() {
        for process in table.processes.iter_mut() {
            if process.state == ProcessState::Blocked(reason) {
                process.state = ProcessState::Ready;
            }
        }
    }
}
