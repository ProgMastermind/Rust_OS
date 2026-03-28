// Physical Frame Allocator
//
// Physical RAM is divided into 4KB chunks called "frames." The frame allocator
// tracks which frames are free and which are in use.
//
// We implement a BUMP ALLOCATOR: the simplest possible strategy.
//   - Keep a counter `next` starting at 0
//   - On allocate: find the Nth usable frame, increment counter
//   - On deallocate: do nothing (frames are never freed)
//
// This is wasteful but perfectly fine for now. We only allocate frames when
// creating new page table entries, and we won't be freeing page tables yet.
// Session 4 will add proper deallocation.
//
// The bootloader provides a MemoryMap that tells us which physical memory
// regions are usable. We filter for MemoryRegionType::Usable and iterate
// over all frames within those regions.

use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

// A frame allocator that returns usable frames from the bootloader's memory map.
pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryMap,
    next: usize, // Index of the next frame to return
}

impl BootInfoFrameAllocator {
    // Create a new allocator from the bootloader's memory map.
    //
    // SAFETY: The caller must guarantee that the memory map is valid and that
    // all frames marked as USABLE are truly unused. The bootloader guarantees
    // this — it marks its own frames as Bootloader/PageTable/etc.
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        BootInfoFrameAllocator {
            memory_map,
            next: 0,
        }
    }

    // Returns an iterator over all usable physical frames.
    //
    // The chain of operations:
    //   1. Iterate over all memory regions in the map
    //   2. Filter for regions marked as Usable
    //   3. For each region, generate all frame-aligned addresses within it
    //   4. Convert each address to a PhysFrame
    //
    // This creates a lazy iterator — no allocation needed.
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        // Step 1: Get all memory regions from the map
        let regions = self.memory_map.iter();

        // Step 2: Keep only usable regions (not reserved, not BIOS, not kernel, etc.)
        let usable_regions = regions.filter(|r| r.region_type == MemoryRegionType::Usable);

        // Step 3: Convert each region to a range of frame-aligned physical addresses
        // A region has start_addr and end_addr. We generate addresses at 4KB intervals.
        let addr_ranges = usable_regions.map(|r| r.range.start_addr()..r.range.end_addr());

        // Step 4: Flatten all ranges into a single iterator of addresses,
        // stepping by 4096 (one frame = 4KB = 4096 bytes)
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(4096));

        // Step 5: Convert each physical address to a PhysFrame type
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

// Implement the FrameAllocator trait from the x86_64 crate.
// This is the interface that OffsetPageTable uses when it needs a new frame
// (e.g., when creating a new page table level during map_to).
unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        // Get the Nth usable frame and increment the counter.
        // nth() consumes elements from the iterator — but we recreate the
        // iterator each time, so `self.next` acts as our skip counter.
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}
