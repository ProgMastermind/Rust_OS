// Process Management
//
// A "process" in our kernel is:
//   - A saved stack pointer (RSP) pointing to saved registers on the stack
//   - A kernel stack mapped at a dedicated virtual address with a guard page
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
use x86_64::structures::paging::{Page, Size4KiB};
use x86_64::VirtAddr;

// ── Stack Layout with Guard Page ─────────────────────────────────────
//
// Each process's stack is mapped at a dedicated virtual address range,
// with an UNMAPPED guard page below it. If the stack overflows (grows
// past the bottom of the mapped region), the CPU writes into the guard
// page → page fault → clean "STACK OVERFLOW" message.
//
// Without a guard page, stack overflow silently corrupts whatever memory
// happens to be adjacent — an extremely hard bug to track down.
//
// Per-process virtual address layout (each "slot"):
//   [Guard page (4KB, NOT mapped)] [Stack (16KB, mapped R/W)]
//   ^                               ^                         ^
//   guard_page_addr                 stack_bottom               stack_top
//
// Slot N starts at STACK_REGION_BASE + N * PAGES_PER_SLOT * 4096.

const STACK_PAGES: usize = 4;       // 4 pages = 16KB per process stack
const GUARD_PAGES: usize = 1;       // 1 unmapped page below each stack
const PAGES_PER_SLOT: usize = STACK_PAGES + GUARD_PAGES; // 5 pages total

// Virtual address base for all process stacks.
// Chosen to not collide with heap (0x4444_4444_0000) or kernel code.
const STACK_REGION_BASE: u64 = 0x5555_0000_0000;

// ── Stack Region ─────────────────────────────────────────────────────
//
// Tracks the virtual address range of a process's stack and its guard page.
// Used for cleanup (unmapping pages when the slot is reused) and for
// detecting stack overflow in the page fault handler.

pub struct StackRegion {
    pub guard_page_addr: u64, // Virtual address of the unmapped guard page
    pub stack_bottom: u64,    // First byte of the mapped stack
    pub stack_top: u64,       // One past the last byte of the mapped stack
}

// ── Process States ────────────────────────────────────────────────────

// ── Wait Reasons ─────────────────────────────────────────────────────
//
// When a process blocks, we record WHY it's blocked. This lets the
// wakeup code be targeted: a keyboard interrupt only wakes processes
// blocked on stdin, not processes blocked on (future) disk I/O or sleep.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaitReason {
    Stdin, // Waiting for keyboard input
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Ready,                    // Can be scheduled — waiting for CPU time
    Running,                  // Currently executing on the CPU
    Blocked(WaitReason),      // Waiting for an event — scheduler skips it
    Terminated,               // Finished — will be reaped by scheduler next tick
    Empty,                    // Slot has been reaped — resources freed, available for reuse
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
    // The stack region for this process. Contains the guard page address
    // and the mapped stack range. None for the idle process (which uses
    // the kernel boot stack set up by the bootloader).
    // When the process is reaped, stack_region stays Some until spawn()
    // reuses the slot and unmaps the old pages.
    pub stack_region: Option<StackRegion>,
    // Per-process file descriptor table (Session 6).
    // Index = fd number. Entry = what that fd points to.
    //   fd 0 = Stdin, fd 1 = Stdout, fd 2 = Stderr
    //   fd 3+ = opened files (from sys_open)
    //   None = closed/available slot
    pub fd_table: Vec<Option<crate::fs::FdEntry>>,
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
    // The stack is mapped at a dedicated virtual address with a guard page
    // below it. The guard page is intentionally NOT mapped — any access
    // to it causes a page fault that we catch and report as stack overflow.
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
    //     stack_top is page-aligned (0 mod 16, since 4096 is a multiple of 16)
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

        // Find slot: reuse an Empty slot or append a new entry.
        let slot_idx = self
            .processes
            .iter()
            .position(|p| p.state == ProcessState::Empty)
            .unwrap_or(self.processes.len());

        // If reusing a slot that still has mapped stack pages, unmap them first.
        // This frees the physical frames back to the frame allocator.
        if slot_idx < self.processes.len() {
            if let Some(ref region) = self.processes[slot_idx].stack_region {
                let start_page =
                    Page::<Size4KiB>::containing_address(VirtAddr::new(region.stack_bottom));
                crate::memory::unmap_pages(start_page, STACK_PAGES);
            }
        }

        // Calculate the virtual address region for this slot's stack.
        // Guard page = first page (unmapped), stack = next STACK_PAGES pages (mapped).
        let region_base =
            STACK_REGION_BASE + (slot_idx as u64) * (PAGES_PER_SLOT as u64) * 4096;
        let guard_page_addr = region_base;
        let stack_bottom = region_base + (GUARD_PAGES as u64) * 4096;
        let stack_top = stack_bottom + (STACK_PAGES as u64) * 4096;

        // Map the stack pages. The guard page is deliberately NOT mapped —
        // that's the whole point. Any write to it triggers a page fault.
        let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(stack_bottom));
        crate::memory::map_pages(start_page, STACK_PAGES)
            .expect("failed to map process stack pages");

        // Zero out the stack memory (fresh frames may contain stale data).
        unsafe {
            core::ptr::write_bytes(stack_bottom as *mut u8, 0, STACK_PAGES * 4096);
        }

        // stack_top is page-aligned (4096 * N), which is also 16-aligned.
        // The & !0xF is technically a no-op here but kept for clarity.
        let aligned_stack_top = (stack_top as usize) & !0xF;

        // 8 values × 8 bytes = 64 bytes below stack_top.
        let frame_start = aligned_stack_top - 64;

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

        let stack_region = StackRegion {
            guard_page_addr,
            stack_bottom,
            stack_top,
        };

        let process = Process {
            pid,
            state: ProcessState::Ready,
            stack_pointer: frame_start as u64,
            entry_fn: Some(entry_point),
            stack_region: Some(stack_region),
            fd_table: alloc::vec![
                Some(crate::fs::FdEntry::Stdin),  // fd 0
                Some(crate::fs::FdEntry::Stdout), // fd 1
                Some(crate::fs::FdEntry::Stderr), // fd 2
            ],
        };

        if slot_idx < self.processes.len() {
            self.processes[slot_idx] = process;
        } else {
            self.processes.push(process);
        }
    }
}

// ── Guard Page Detection ─────────────────────────────────────────────
//
// Called by the page fault handler to check if the faulting address
// is a stack guard page. Each process slot has a guard page at the
// first page of its region. If the address falls in any guard page,
// we know it's a stack overflow.

pub fn is_guard_page(addr: u64) -> bool {
    if addr < STACK_REGION_BASE {
        return false;
    }
    let offset = addr - STACK_REGION_BASE;
    let slot_size = (PAGES_PER_SLOT as u64) * 4096;
    let within_slot = offset % slot_size;
    // The first page (bytes 0..4095) of each slot is the guard page
    within_slot < 4096
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

// ── Blocking / Wakeup ────────────────────────────────────────────────
//
// These functions implement real blocking semantics. When a process needs
// to wait for an event (like keyboard input), it marks itself as Blocked
// instead of busy-waiting in a hlt loop.
//
// The difference matters for the scheduler:
//   - hlt-polling: scheduler switches to this process every tick, process
//     immediately halts again → wasted context switch (~55ms of nothing)
//   - Blocked: scheduler SKIPS this process entirely → zero overhead until
//     the event actually arrives and wake_blocked() is called

// Mark the current process as Blocked and wait for wakeup.
// The caller specifies WHY we're blocking, so the wakeup code can be
// targeted (e.g., only wake stdin-waiters on keyboard interrupt).
//
// After marking as Blocked, we enable interrupts and hlt-loop.
// The scheduler will skip us. When the event arrives (e.g., keyboard IRQ),
// the ISR calls wake_blocked() to move us back to Ready. The next
// scheduler tick will then pick us up.
pub fn block_current(reason: WaitReason) {
    {
        let mut table = PROCESS_TABLE.lock();
        let current = table.current;
        table.processes[current].state = ProcessState::Blocked(reason);
    } // Lock released before hlt

    // Enable interrupts and halt until woken. The keyboard ISR (or timer)
    // will fire, and wake_blocked() will set us back to Ready.
    // The scheduler will then context-switch to us on its next pass.
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();

        // After wakeup, check if we're no longer Blocked (= woken up).
        // If still Blocked, another interrupt woke us (e.g., timer) but
        // our event hasn't arrived yet — go back to sleep.
        let still_blocked = {
            let table = PROCESS_TABLE.lock();
            let current = table.current;
            matches!(table.processes[current].state, ProcessState::Blocked(_))
        };

        if !still_blocked {
            break; // We've been woken — return to caller
        }
    }
}

// Wake all processes blocked on the given reason.
// Called from interrupt handlers (e.g., keyboard ISR calls this with
// WaitReason::Stdin when a key is pressed).
//
// Uses try_lock because this runs in interrupt context — if the process
// table is already locked (e.g., by spawn()), we skip wakeup this time.
// The next keyboard interrupt will try again.
pub fn wake_blocked(reason: WaitReason) {
    if let Some(mut table) = PROCESS_TABLE.try_lock() {
        for process in table.processes.iter_mut() {
            if process.state == ProcessState::Blocked(reason) {
                process.state = ProcessState::Ready;
            }
        }
    }
}
