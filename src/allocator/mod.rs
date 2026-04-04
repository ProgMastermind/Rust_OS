// Kernel Memory Allocator
//
// This module provides the #[global_allocator] that Rust's alloc crate uses.
// When you write Box::new(42), it calls our allocator's alloc() method.
// When the Box is dropped, it calls dealloc().
//
// We provide three allocator implementations (in order of sophistication):
//   1. BumpAllocator      — alloc only, never frees (simplest, for learning)
//   2. LinkedListAllocator — alloc + dealloc via free list (correct, O(n))
//   3. FixedSizeBlockAllocator — per-size free lists (fast, O(1) common case)
//
// The active allocator is FixedSizeBlockAllocator (the best one).
// The others exist for educational comparison.

pub mod bump;
pub mod fixed_size_block;
pub mod linked_list;

use spin::Mutex;
use spin::MutexGuard;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use fixed_size_block::FixedSizeBlockAllocator;

// ── Global Allocator ─────────────────────────────────────────────────
//
// This is THE allocator that Rust's alloc crate calls into.
// #[global_allocator] tells the compiler: "Use this for all heap allocations."
//
// We wrap our allocator in a Locked<T> (spinlock wrapper) because
// GlobalAlloc requires &self (shared reference), but allocating/freeing
// mutates internal state. The spinlock provides interior mutability safely.

#[global_allocator]
pub static ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());

// ── Locked<T> Wrapper ────────────────────────────────────────────────
//
// The GlobalAlloc trait requires &self (not &mut self) for alloc/dealloc.
// This is because multiple threads/interrupts might allocate concurrently.
// But allocators need mutable state internally. Solution: wrap in a spinlock.
//
// We can't use spin::Mutex directly as the GlobalAlloc implementor because
// GlobalAlloc is a trait on our allocator type, not on Mutex. So we create
// this thin wrapper.

pub struct Locked<A> {
    inner: Mutex<A>,
}

impl<A> Locked<A> {
    // const fn so it can be used in static initialization
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: Mutex::new(inner),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, A> {
        self.inner.lock()
    }
}

// ── GlobalAlloc for Locked<FixedSizeBlockAllocator> ──────────────────
//
// This bridges Rust's alloc crate to our allocator.
// Every Box::new(), Vec::push(), String::from() ends up here.

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

// ── Alignment Helper ─────────────────────────────────────────────────
//
// Allocators need to return addresses aligned to the requested alignment.
// For example, a u64 needs 8-byte alignment (address divisible by 8).
//
// align_up rounds an address UP to the nearest multiple of `align`.
// Example: align_up(0x1003, 8) = 0x1008

// Rounds `addr` up to the nearest multiple of `align`.
// `align` must be a power of two.
pub fn align_up(addr: usize, align: usize) -> usize {
    // Trick: for power-of-two alignment, (align - 1) is a bitmask.
    // Example: align=8 → align-1 = 0b0111
    //   addr & !(align-1) rounds DOWN
    //   Adding (align-1) first makes it round UP
    //
    // Alternative bit trick (equivalent, faster):
    //   (addr + align - 1) & !(align - 1)
    (addr + align - 1) & !(align - 1)
}

// ── Unit Tests ──────────────────────────────────────────────────────
//
// These tests run via `cargo test` on the lib.rs test binary.
// They don't need the heap — just pure arithmetic.

#[cfg(test)]
mod tests {
    use super::*;

    #[test_case]
    fn test_align_up_basic() {
        assert_eq!(align_up(0, 4), 0);     // 0 is aligned to everything
        assert_eq!(align_up(1, 4), 4);     // round 1 up to 4
        assert_eq!(align_up(3, 4), 4);     // round 3 up to 4
        assert_eq!(align_up(4, 4), 4);     // already aligned — no change
        assert_eq!(align_up(5, 4), 8);     // round 5 up to 8
        assert_eq!(align_up(7, 8), 8);     // round 7 up to 8
        assert_eq!(align_up(8, 8), 8);     // already aligned
        assert_eq!(align_up(9, 8), 16);    // round 9 up to 16
    }

    #[test_case]
    fn test_align_up_powers_of_two() {
        // Common allocator alignment values
        assert_eq!(align_up(0x1001, 8), 0x1008);
        assert_eq!(align_up(0x1000, 4096), 0x1000); // page-aligned stays
        assert_eq!(align_up(0x1001, 4096), 0x2000); // rounds up to next page
        assert_eq!(align_up(1, 1), 1);               // align=1 is identity
        assert_eq!(align_up(42, 1), 42);             // align=1 never changes
    }
}
