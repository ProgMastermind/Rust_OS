// Page table initialization and global mapper/frame allocator access.

use spin::Mutex;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

use crate::frame_allocator::BitmapFrameAllocator;

/// Create an OffsetPageTable from the active PML4.
/// SAFETY: physical memory must be mapped at `physical_memory_offset`. Call only once.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = unsafe { active_level_4_table(physical_memory_offset) };
    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    unsafe { &mut *page_table_ptr }
}

// Global mapper and frame allocator, available after kernel_main stores them.

static MAPPER: Mutex<Option<OffsetPageTable<'static>>> = Mutex::new(None);
static FRAME_ALLOC: Mutex<Option<BitmapFrameAllocator>> = Mutex::new(None);

pub fn store_globals(mapper: OffsetPageTable<'static>, frame_allocator: BitmapFrameAllocator) {
    *MAPPER.lock() = Some(mapper);
    *FRAME_ALLOC.lock() = Some(frame_allocator);
}

/// Map `count` pages starting at `start_page` to fresh physical frames.
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

/// Unmap `count` pages and return frames to the allocator.
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
