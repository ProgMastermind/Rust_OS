// Bump allocator. Advances a pointer forward on every alloc, never frees
// individual blocks. Only resets when ALL allocations are dropped.
// Not the active allocator -- kept here as a reference implementation.

use core::alloc::Layout;
use core::ptr::NonNull;
use super::align_up;

pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,       // next free address to hand out
    allocations: usize, // tracks active allocations for bulk reset
}

impl BumpAllocator {
    pub const fn new() -> Self {
        BumpAllocator {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            allocations: 0,
        }
    }

    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next = heap_start;
    }

    /// Align `next` up to the requested alignment, then bump forward by `size`.
    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        let alloc_start = align_up(self.next, layout.align());
        let alloc_end = alloc_start.checked_add(layout.size()).ok_or(())?;

        if alloc_end > self.heap_end {
            return Err(());
        }

        self.next = alloc_end;
        self.allocations += 1;

        Ok(unsafe { NonNull::new_unchecked(alloc_start as *mut u8) })
    }

    /// Can't free individual blocks. Resets the entire heap only when all allocations are dropped.
    pub fn dealloc(&mut self, _ptr: NonNull<u8>, _layout: Layout) {
        self.allocations -= 1;
        if self.allocations == 0 {
            self.next = self.heap_start;
        }
    }
}
