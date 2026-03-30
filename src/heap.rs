// Kernel Heap Setup
//
// This module maps a region of virtual memory for the kernel heap.
// The heap is where dynamic allocations (Box, Vec, String) live.
//
// We pick a virtual address range and map each page to a physical frame
// using the mapper and frame allocator from Session 3.
//
// The actual allocator (which manages blocks within this mapped region)
// lives in src/allocator/. This module just sets up the backing memory.

use x86_64::structures::paging::{
    FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

// Where the heap lives in virtual memory.
// We pick a high address that doesn't collide with kernel code, stack, or VGA.
// These are just arbitrary choices — any unused virtual range works.
pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 100 * 1024; // 100 KiB

// Map the heap pages to physical frames.
//
// This function:
//   1. Calculates the page range for [HEAP_START .. HEAP_START + HEAP_SIZE]
//   2. For each page, allocates a physical frame from the frame allocator
//   3. Calls map_to() to create the virtual → physical mapping
//   4. Flushes the TLB entry so the CPU sees the new mapping
//
// After this returns, reading/writing to addresses in the heap range
// works — they're backed by real physical RAM.
//
// Then we initialize the allocator, telling it about this memory region.
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
        // Allocate a physical frame for this page
        let frame = frame_allocator
            .allocate_frame()
            .ok_or("failed to allocate frame for heap page")?;

        // Flags: PRESENT (page exists) + WRITABLE (heap must be read-write)
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        // Create the mapping: this virtual page → this physical frame
        // map_to is unsafe because the caller must ensure the frame isn't
        // already mapped elsewhere (double-mapping can cause aliasing bugs)
        // Create the mapping: this virtual page → this physical frame
        // map_to returns MapToError on failure — we convert to &str for our Result type
        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .map_err(|_| "map_to failed during heap initialization")?
                .flush(); // Update TLB
        }
    }

    // Now the heap region is backed by physical memory.
    // Initialize the allocator with this region.
    crate::allocator::ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);

    Ok(())
}
