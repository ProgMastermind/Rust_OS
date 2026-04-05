// Bitmap frame allocator. One bit per 4KB physical frame: 0=free, 1=used.
// Static 4KB bitmap supports up to 128MB RAM (32768 frames).

use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use spin::Mutex;
use x86_64::structures::paging::{FrameAllocator, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

const FRAME_SIZE: usize = 4096;
const MAX_FRAMES: usize = 32768;
const BITMAP_WORDS: usize = MAX_FRAMES / 64;

// All bits start as 1 (used). init() clears bits for usable regions.
static BITMAP: Mutex<[u64; BITMAP_WORDS]> = Mutex::new([!0u64; BITMAP_WORDS]);

/// Tracks free/used physical frames via a static bitmap. Must be initialized before the heap.
pub struct BitmapFrameAllocator {
    next_scan: usize, // scan optimization hint
}

impl BitmapFrameAllocator {
    /// Initialize from the bootloader memory map. Marks usable regions as free.
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        let mut bitmap = BITMAP.lock();

        for region in memory_map.iter() {
            if region.region_type == MemoryRegionType::Usable {
                let start_frame = region.range.start_addr() as usize / FRAME_SIZE;
                let end_frame = region.range.end_addr() as usize / FRAME_SIZE;

                for frame_idx in start_frame..end_frame.min(MAX_FRAMES) {
                    let word = frame_idx / 64;
                    let bit = frame_idx % 64;
                    bitmap[word] &= !(1u64 << bit);
                }
            }
        }

        BitmapFrameAllocator { next_scan: 0 }
    }

    /// Return a frame to the pool. O(1) -- just clears the bit.
    pub fn deallocate_frame(&mut self, frame: PhysFrame) {
        let frame_idx = frame.start_address().as_u64() as usize / FRAME_SIZE;
        if frame_idx < MAX_FRAMES {
            let mut bitmap = BITMAP.lock();
            let word = frame_idx / 64;
            let bit = frame_idx % 64;
            bitmap[word] &= !(1u64 << bit);

            if frame_idx < self.next_scan {
                self.next_scan = frame_idx;
            }
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BitmapFrameAllocator {
    /// Scan bitmap from next_scan for first free frame. O(n/64) worst case.
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let mut bitmap = BITMAP.lock();

        // trailing_ones() finds the first 0 bit in each word
        for i in 0..BITMAP_WORDS {
            let word_idx = (self.next_scan / 64 + i) % BITMAP_WORDS;
            let word = bitmap[word_idx];

            if word == !0u64 {
                continue;
            }

            let bit = word.trailing_ones() as usize;
            let frame_idx = word_idx * 64 + bit;

            if frame_idx >= MAX_FRAMES {
                continue;
            }

            bitmap[word_idx] |= 1u64 << bit;

            self.next_scan = frame_idx + 1;
            if self.next_scan >= MAX_FRAMES {
                self.next_scan = 0;
            }

            let addr = PhysAddr::new((frame_idx * FRAME_SIZE) as u64);
            return Some(PhysFrame::containing_address(addr));
        }

        None
    }
}
