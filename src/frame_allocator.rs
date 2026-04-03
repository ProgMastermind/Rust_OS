// Bitmap Frame Allocator
//
// Physical RAM is divided into 4KB chunks called "frames." The frame allocator
// tracks which frames are free and which are in use via a BITMAP:
//   - One bit per frame: 0 = free, 1 = used
//   - alloc: scan for first 0 bit, set to 1, return the frame — O(n/64)
//   - dealloc: set bit to 0 — O(1)
//
// This replaces the old bump allocator which was O(n²) per allocation
// (it recreated the iterator each time) and could never free frames.
//
// The bitmap lives in a static array (4KB, supports up to 128MB RAM).
// We use a static instead of the heap because the frame allocator is needed
// BEFORE the heap is initialized (the heap itself needs frames).

use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use spin::Mutex;
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

const FRAME_SIZE: usize = 4096;

// Support up to 128MB of physical RAM (QEMU default).
// 128MB / 4KB = 32768 frames. 32768 bits = 512 u64s = 4KB for the bitmap.
const MAX_FRAMES: usize = 32768;
const BITMAP_WORDS: usize = MAX_FRAMES / 64;

// The bitmap: one bit per frame.
//   bit = 1 → frame is in use (or reserved/non-existent)
//   bit = 0 → frame is free
//
// Starts with ALL bits set to 1 (everything "used"). init() clears bits
// for usable frames based on the bootloader's memory map. This is safe:
// unknown/reserved memory stays marked as used and is never handed out.
static BITMAP: Mutex<[u64; BITMAP_WORDS]> = Mutex::new([!0u64; BITMAP_WORDS]);

pub struct BitmapFrameAllocator {
    // Optimization: start scanning from here instead of frame 0 every time.
    // After a successful alloc, next_scan advances past the allocated frame.
    // Wraps around to 0 when reaching MAX_FRAMES.
    next_scan: usize,
}

impl BitmapFrameAllocator {
    // Initialize the bitmap from the bootloader's memory map.
    //
    // Walks every region in the map. For regions marked as Usable,
    // clears the corresponding bits in the bitmap (marking them free).
    //
    // SAFETY: Caller must guarantee the memory map is valid and that
    // usable regions are truly unused. The bootloader guarantees this.
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        let mut bitmap = BITMAP.lock();

        for region in memory_map.iter() {
            if region.region_type == MemoryRegionType::Usable {
                let start_frame = region.range.start_addr() as usize / FRAME_SIZE;
                let end_frame = region.range.end_addr() as usize / FRAME_SIZE;

                for frame_idx in start_frame..end_frame.min(MAX_FRAMES) {
                    let word = frame_idx / 64;
                    let bit = frame_idx % 64;
                    bitmap[word] &= !(1u64 << bit); // Clear bit = mark free
                }
            }
        }

        BitmapFrameAllocator { next_scan: 0 }
    }

    // Free a frame, making it available for future allocation.
    //
    // This is the key improvement over the bump allocator: frames can be
    // returned to the pool. Used when unmapping pages or cleaning up processes.
    pub fn deallocate_frame(&mut self, frame: PhysFrame) {
        let frame_idx = frame.start_address().as_u64() as usize / FRAME_SIZE;
        if frame_idx < MAX_FRAMES {
            let mut bitmap = BITMAP.lock();
            let word = frame_idx / 64;
            let bit = frame_idx % 64;
            bitmap[word] &= !(1u64 << bit); // Clear bit = mark free

            // Update scan hint: if this freed frame is before our scan position,
            // move the scan back so we find it sooner.
            if frame_idx < self.next_scan {
                self.next_scan = frame_idx;
            }
        }
    }
}

// Implement the FrameAllocator trait from the x86_64 crate.
// This is the interface that OffsetPageTable::map_to() uses when it needs
// a new physical frame (e.g., for a new page table level).
unsafe impl FrameAllocator<Size4KiB> for BitmapFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let mut bitmap = BITMAP.lock();

        // Scan the bitmap starting from next_scan, wrapping around.
        // Uses trailing_ones() to find the first 0 bit in each u64 word.
        // trailing_ones() counts how many consecutive 1-bits there are from
        // the least-significant bit. If the word is all 1s, it returns 64.
        for i in 0..BITMAP_WORDS {
            let word_idx = (self.next_scan / 64 + i) % BITMAP_WORDS;
            let word = bitmap[word_idx];

            if word == !0u64 {
                continue; // All 64 frames in this word are used
            }

            // Found a word with at least one free bit
            let bit = word.trailing_ones() as usize; // Index of first 0 bit
            let frame_idx = word_idx * 64 + bit;

            if frame_idx >= MAX_FRAMES {
                continue; // Past the end of tracked memory
            }

            // Mark as used (set bit to 1)
            bitmap[word_idx] |= 1u64 << bit;

            // Advance scan position for next allocation
            self.next_scan = frame_idx + 1;
            if self.next_scan >= MAX_FRAMES {
                self.next_scan = 0;
            }

            let addr = PhysAddr::new((frame_idx * FRAME_SIZE) as u64);
            return Some(PhysFrame::containing_address(addr));
        }

        None // All frames are in use
    }
}
