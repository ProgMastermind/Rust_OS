// Bump allocator (educational, not the active allocator).
// Moves a pointer forward on alloc. Never frees individual blocks.

use core::alloc::Layout;
use core::ptr::NonNull;
use super::align_up;

pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
    allocations: usize,
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

    pub fn dealloc(&mut self, _ptr: NonNull<u8>, _layout: Layout) {
        self.allocations -= 1;
        if self.allocations == 0 {
            self.next = self.heap_start;
        }
    }
}
