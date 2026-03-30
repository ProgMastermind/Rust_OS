// Fixed-Size Block Allocator
//
// The primary allocator used by our kernel. Maintains separate free lists
// for common allocation sizes, with a fallback to linked-list for large ones.
//
// Block sizes: 8, 16, 32, 64, 128, 256, 512, 1024, 2048 bytes
//
// How it works:
//
//   Each block size has its own singly-linked free list:
//
//     8-byte list:    [free] → [free] → [free] → null
//     16-byte list:   [free] → [free] → null
//     32-byte list:   [free] → null
//     ...
//     2048-byte list: null  (empty — no freed 2048-byte blocks yet)
//     fallback:       LinkedListAllocator (for anything > 2048 bytes)
//
//   alloc(20 bytes):
//     1. Round up to next block size: 20 → 32
//     2. Pop from the 32-byte free list
//     3. If empty, allocate a fresh 32-byte block from the fallback
//     4. Return pointer — O(1)
//
//   dealloc(ptr, 20 bytes):
//     1. Round up: 20 → 32
//     2. Push ptr onto the 32-byte free list — O(1)
//
// Why is this fast?
//   - alloc and dealloc for common sizes are O(1) — just push/pop a pointer
//   - No searching, no splitting, no merging
//   - Only large/unusual sizes fall through to the O(n) linked-list allocator
//
// Tradeoff: wastes some memory due to rounding up (internal fragmentation).
// A 20-byte allocation uses a 32-byte block — 12 bytes wasted. But the speed
// benefit is worth it for an OS kernel where alloc/dealloc happens constantly.

use core::alloc::Layout;
use core::mem;
use core::ptr::NonNull;
use super::linked_list::LinkedListAllocator;

// The available block sizes. Must be powers of two (for alignment).
// Each size gets its own free list.
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

// A node in a per-size free list. When a block is free, we store this
// pointer at the start of the block (the block is unused, so we can
// repurpose its memory for bookkeeping — same trick as LinkedListAllocator).
struct ListNode {
    next: Option<&'static mut ListNode>,
}

pub struct FixedSizeBlockAllocator {
    // One free list head per block size. list_heads[i] is the head of
    // the free list for BLOCK_SIZES[i].
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],

    // Fallback for allocations larger than the biggest block size.
    fallback_allocator: LinkedListAllocator,
}

impl FixedSizeBlockAllocator {
    pub const fn new() -> Self {
        // Initialize all free lists as empty (None).
        // We can't use [None; N] because &mut ListNode isn't Copy.
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: LinkedListAllocator::new(),
        }
    }

    // Called by heap::init_heap(). Passes the heap region to the fallback allocator.
    // The per-size free lists start empty — they get populated as blocks are freed.
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.fallback_allocator.init(heap_start, heap_size);
    }

    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        match list_index(&layout) {
            Some(index) => {
                // This allocation fits in one of our fixed-size block lists
                match self.list_heads[index].take() {
                    Some(node) => {
                        // Free list has a block — pop it off and return it
                        self.list_heads[index] = node.next.take();
                        Ok(NonNull::from(node).cast())
                    }
                    None => {
                        // Free list is empty — allocate a fresh block from the fallback.
                        // We allocate BLOCK_SIZES[index] bytes, not layout.size(),
                        // so the block can be reused for any allocation of this size class.
                        let block_size = BLOCK_SIZES[index];
                        let block_align = block_size; // Powers of two are self-aligned
                        let layout =
                            Layout::from_size_align(block_size, block_align).unwrap();
                        self.fallback_allocator.alloc(layout)
                    }
                }
            }
            None => {
                // Too large for any block size — use the fallback directly
                self.fallback_allocator.alloc(layout)
            }
        }
    }

    pub fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        match list_index(&layout) {
            Some(index) => {
                // Push the freed block onto the appropriate free list.
                // We write a ListNode at the start of the freed block.
                let new_node = ListNode {
                    next: self.list_heads[index].take(),
                };
                // Verify the block is large enough and aligned for a ListNode
                assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node_ptr = ptr.as_ptr() as *mut ListNode;
                unsafe {
                    new_node_ptr.write(new_node);
                    self.list_heads[index] = Some(&mut *new_node_ptr);
                }
            }
            None => {
                // Large allocation — return to the fallback allocator
                self.fallback_allocator.dealloc(ptr, layout);
            }
        }
    }
}

// Find which block size list an allocation should use.
// Returns None if the allocation is too large for any fixed-size list.
fn list_index(layout: &Layout) -> Option<usize> {
    // The block must be at least as large as the requested size AND alignment.
    // (Because our block sizes are powers of two, a block of size N is
    // automatically N-aligned.)
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}
