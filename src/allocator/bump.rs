// Bump Allocator (educational — not used as the active allocator)
//
// The simplest possible allocator. Maintains a single "next" pointer
// that advances forward on every allocation. Never frees memory.
//
//   Heap:    [                    100KB                        ]
//             ^
//             next
//
//   alloc(64):
//   Heap:    [################                                 ]
//                            ^
//                            next (moved forward 64 bytes)
//
//   dealloc:
//   → does nothing. The memory is lost forever.
//
//   alloc(32):
//   Heap:    [################********                         ]
//                                     ^
//                                     next (moved forward 32 more)
//
// Why does this exist?
//   - It's the simplest allocator to understand
//   - It proves Box/Vec/String work before we add a real allocator
//   - It demonstrates WHY you need dealloc: without it, you eventually
//     run out of heap space even if you dropped everything
//
// Limitation: after enough allocations, the heap is exhausted and every
// alloc returns null (OOM). There's no way to reclaim freed memory.

use core::alloc::Layout;
use core::ptr::NonNull;
use super::align_up;

pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,       // Next free address to hand out
    allocations: usize, // Count of active allocations (for debugging)
}

impl BumpAllocator {
    // const fn — can be used in static initialization
    pub const fn new() -> Self {
        BumpAllocator {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            allocations: 0,
        }
    }

    // Called by heap::init_heap() after the heap pages are mapped.
    // Sets the address range this allocator manages.
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next = heap_start;
    }

    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        // Round up `next` to satisfy the requested alignment.
        // For example, if next=0x1003 and layout needs 8-byte alignment,
        // we skip to 0x1008 (wastes 5 bytes — "internal fragmentation").
        let alloc_start = align_up(self.next, layout.align());
        let alloc_end = alloc_start.checked_add(layout.size()).ok_or(())?;

        if alloc_end > self.heap_end {
            // Out of memory — heap is exhausted
            return Err(());
        }

        self.next = alloc_end;
        self.allocations += 1;

        Ok(unsafe { NonNull::new_unchecked(alloc_start as *mut u8) })
    }

    pub fn dealloc(&mut self, _ptr: NonNull<u8>, _layout: Layout) {
        // Bump allocator can't free individual blocks.
        // We track allocation count — if ALL allocations are freed,
        // we can reset the entire heap.
        self.allocations -= 1;
        if self.allocations == 0 {
            self.next = self.heap_start;
        }
    }
}
