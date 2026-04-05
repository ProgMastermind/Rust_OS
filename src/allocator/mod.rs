// Global allocator. Bridges Rust's alloc crate to our FixedSizeBlockAllocator.

pub mod bump;
pub mod fixed_size_block;
pub mod linked_list;

use spin::Mutex;
use spin::MutexGuard;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use fixed_size_block::FixedSizeBlockAllocator;

#[global_allocator]
pub static ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());

/// Spinlock wrapper for GlobalAlloc (which requires &self, not &mut self).
pub struct Locked<A> {
    inner: Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: Mutex::new(inner),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, A> {
        self.inner.lock()
    }
}

// SAFETY: Locked<> provides mutual exclusion via spinlock.
unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        match allocator.alloc(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        unsafe {
            allocator.dealloc(core::ptr::NonNull::new_unchecked(ptr), layout);
        }
    }
}

/// Round `addr` up to the nearest multiple of `align` (must be power of two).
pub fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn test_align_up_basic() {
        assert_eq!(align_up(0, 4), 0);
        assert_eq!(align_up(1, 4), 4);
        assert_eq!(align_up(3, 4), 4);
        assert_eq!(align_up(4, 4), 4);
        assert_eq!(align_up(5, 4), 8);
        assert_eq!(align_up(7, 8), 8);
        assert_eq!(align_up(8, 8), 8);
        assert_eq!(align_up(9, 8), 16);
    }

    #[test_case]
    fn test_align_up_powers_of_two() {
        assert_eq!(align_up(0x1001, 8), 0x1008);
        assert_eq!(align_up(0x1000, 4096), 0x1000);
        assert_eq!(align_up(0x1001, 4096), 0x2000);
        assert_eq!(align_up(1, 1), 1);
        assert_eq!(align_up(42, 1), 42);
    }
}
