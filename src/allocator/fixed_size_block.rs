// Fixed-size block allocator. Per-size free lists for 8..2048 bytes, O(1) alloc/dealloc.
// Falls back to linked-list allocator for larger sizes.

use core::alloc::Layout;
use core::mem;
use core::ptr::NonNull;
use super::linked_list::LinkedListAllocator;

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

struct ListNode {
    next: Option<&'static mut ListNode>,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: LinkedListAllocator,
}

impl FixedSizeBlockAllocator {
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: LinkedListAllocator::new(),
        }
    }

    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.fallback_allocator.init(heap_start, heap_size);
    }

    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        match list_index(&layout) {
            Some(index) => {
                match self.list_heads[index].take() {
                    Some(node) => {
                        self.list_heads[index] = node.next.take();
                        Ok(NonNull::from(node).cast())
                    }
                    None => {
                        // Free list empty, carve a new block from the fallback
                        let block_size = BLOCK_SIZES[index];
                        let block_align = block_size;
                        let layout =
                            Layout::from_size_align(block_size, block_align).unwrap();
                        self.fallback_allocator.alloc(layout)
                    }
                }
            }
            None => self.fallback_allocator.alloc(layout),
        }
    }

    pub fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        match list_index(&layout) {
            Some(index) => {
                let new_node = ListNode {
                    next: self.list_heads[index].take(),
                };
                assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node_ptr = ptr.as_ptr() as *mut ListNode;
                unsafe {
                    new_node_ptr.write(new_node);
                    self.list_heads[index] = Some(&mut *new_node_ptr);
                }
            }
            None => {
                self.fallback_allocator.dealloc(ptr, layout);
            }
        }
    }
}

fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}
