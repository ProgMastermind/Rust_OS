// Page Table Initialization + Address Translation
//
// This module sets up the kernel's access to the 4-level page table hierarchy.
//
// x86_64 virtual address translation:
//   A 48-bit virtual address is split into indices that walk a 4-level tree:
//
//     Virtual Address (48 bits):
//     [PML4 index (9)] [PDPT index (9)] [PD index (9)] [PT index (9)] [Offset (12)]
//         bits 47-39       bits 38-30      bits 29-21     bits 20-12     bits 11-0
//
//   The CPU walks: CR3 → PML4[idx] → PDPT[idx] → PD[idx] → PT[idx] → physical frame
//   Then adds the 12-bit offset to get the final physical address.
//
//   This walk happens on EVERY memory access. The TLB (Translation Lookaside Buffer)
//   caches recent translations so the CPU doesn't actually walk the table every time.
//s
// The bootloader maps ALL physical memory at a virtual offset. For example, if
// physical_memory_offset = 0x1000_0000_0000, then:
//   physical address 0x0     → virtual address 0x1000_0000_0000
//   physical address 0xB8000 → virtual address 0x1000_000B_8000
//
// This lets us access any physical address by adding the offset. We need this
// to read/write page table entries (which contain physical addresses).

use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{OffsetPageTable, PageTable};
use x86_64::VirtAddr;

// Initialize an OffsetPageTable.
//
// The OffsetPageTable type from the x86_64 crate provides safe methods for:
//   - translate_addr(virt) → Option<phys>   (walk the page table)
//   - map_to(page, frame, flags)            (create a new mapping)
//   - unmap(page)                           (remove a mapping)
//
// It needs to know the physical memory offset so it can convert the physical
// addresses stored in page table entries into virtual addresses it can read.
//
// SAFETY: The caller must guarantee that:
//   1. The complete physical memory is mapped at `physical_memory_offset`
//   2. This function is called only once (to avoid aliasing &mut references)
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = unsafe { active_level_4_table(physical_memory_offset) };
    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}

// Returns a mutable reference to the active level 4 page table (PML4).
//
// How it works:
//   1. Read CR3 register — the CPU stores the PHYSICAL address of the PML4 here
//   2. Add physical_memory_offset to convert to a VIRTUAL address we can access
//   3. Cast to a PageTable pointer and dereference
//
// SAFETY: Same requirements as init() above.
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    let (level_4_table_frame, _) = Cr3::read();

    // CR3 gives us the physical address of the PML4 table
    let phys = level_4_table_frame.start_address();

    // Convert physical → virtual by adding the offset
    let virt = physical_memory_offset + phys.as_u64();

    // Cast to a PageTable pointer and create a mutable reference
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}
