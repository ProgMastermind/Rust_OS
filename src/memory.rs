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

use spin::Mutex;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

use crate::frame_allocator::BitmapFrameAllocator;

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

// ── Global Mapper and Frame Allocator ────────────────────────────────
//
// After kernel_main initializes the mapper and frame allocator (and uses
// them to set up the heap), it stores them here so other kernel code
// (like spawn()) can allocate and map pages without threading references.
//
// Option<> because they start as None — only valid after store_globals().

static MAPPER: Mutex<Option<OffsetPageTable<'static>>> = Mutex::new(None);
static FRAME_ALLOC: Mutex<Option<BitmapFrameAllocator>> = Mutex::new(None);

/// Store the mapper and frame allocator for global access.
/// Called once from kernel_main after heap initialization.
pub fn store_globals(mapper: OffsetPageTable<'static>, frame_allocator: BitmapFrameAllocator) {
    *MAPPER.lock() = Some(mapper);
    *FRAME_ALLOC.lock() = Some(frame_allocator);
}

/// Map `count` consecutive virtual pages to freshly allocated physical frames.
/// The guard page (if any) is the caller's responsibility to leave unmapped.
///
/// Returns Err if frame allocation or page mapping fails.
pub fn map_pages(start_page: Page<Size4KiB>, count: usize) -> Result<(), &'static str> {
    let mut mapper_guard = MAPPER.lock();
    let mut fa_guard = FRAME_ALLOC.lock();
    let mapper = mapper_guard.as_mut().ok_or("mapper not initialized")?;
    let fa = fa_guard.as_mut().ok_or("frame allocator not initialized")?;

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for i in 0..count {
        let page = Page::containing_address(start_page.start_address() + (i as u64) * 4096);
        let frame = fa.allocate_frame().ok_or("out of physical frames")?;
        unsafe {
            mapper
                .map_to(page, frame, flags, fa)
                .map_err(|_| "map_to failed")?
                .flush();
        }
    }
    Ok(())
}

/// Unmap `count` consecutive virtual pages and return their physical frames
/// to the frame allocator. Silently skips pages that aren't mapped.
pub fn unmap_pages(start_page: Page<Size4KiB>, count: usize) {
    let mut mapper_guard = MAPPER.lock();
    let mut fa_guard = FRAME_ALLOC.lock();
    if let (Some(mapper), Some(fa)) = (mapper_guard.as_mut(), fa_guard.as_mut()) {
        for i in 0..count {
            let page =
                Page::containing_address(start_page.start_address() + (i as u64) * 4096);
            if let Ok((frame, flush)) = mapper.unmap(page) {
                flush.flush();
                fa.deallocate_frame(frame);
            }
        }
    }
}
