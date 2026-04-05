// Kernel heap setup. Maps virtual pages for the heap region and initializes the allocator.

use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

pub const HEAP_START: usize = 0x_4444_4444_0000; // arbitrary, avoids kernel code/stack/VGA regions
pub const HEAP_SIZE: usize = 100 * 1024; // 100 KiB

/// Map pages for the heap region and initialize the allocator.
/// Must be called before any heap allocations (Box, Vec, String).
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), &'static str> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = VirtAddr::new((HEAP_START + HEAP_SIZE - 1) as u64);
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or("failed to allocate frame for heap page")?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .map_err(|_| "map_to failed during heap initialization")?
                .flush();
        }
    }

    crate::allocator::ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);

    Ok(())
}
