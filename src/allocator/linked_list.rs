// Linked-List Free-List Allocator
//
// Maintains a linked list of free memory regions. Each free region stores:
//   - Its size
//   - A pointer to the next free region
//
// The list is sorted by memory address (not required, but helps with merging).
//
//   Initial state (entire heap is one big free block):
//   HEAD → [FREE: 100KB, next: null]
//
//   After alloc(64):
//   HEAD → [FREE: 99.9KB, next: null]
//   [USED: 64B]  ← returned to caller, not in the free list
//
//   After alloc(32):
//   HEAD → [FREE: 99.8KB, next: null]
//   [USED: 64B] [USED: 32B]  ← both used, not in free list
//
//   After dealloc(first 64B block):
//   HEAD → [FREE: 64B, next: ──→] [USED: 32B] [FREE: 99.8KB, next: null]
//   The 64B block is back in the free list!
//
// alloc() walks the list looking for a block big enough. If found:
//   - If the block is exactly the right size: remove it from the list
//   - If the block is bigger: split it — return the requested part, keep the rest
//
// dealloc() inserts the freed block back into the list.
//
// Limitations:
//   - O(n) alloc — must walk the list to find a fit
//   - External fragmentation — free blocks may be scattered, too small individually

use core::alloc::Layout;
use core::mem;
use core::ptr::NonNull;
use super::align_up;

// A node in the free list. Stored INSIDE the free memory region itself —
// no separate metadata allocation needed. This is why the minimum allocation
// size must be large enough to hold a ListNode (16 bytes on x86_64).
struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new(size: usize) -> Self {
        ListNode { size, next: None }
    }

    // Returns the start address of the region this node represents.
    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    // Returns the end address of the region this node represents.
    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

pub struct LinkedListAllocator {
    head: ListNode, // Dummy head node (not a real free region)
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        LinkedListAllocator {
            head: ListNode::new(0),
        }
    }

    // Called by heap::init_heap(). Adds the entire heap as one big free block.
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        unsafe {
            self.add_free_region(heap_start, heap_size);
        }
    }

    // Add a free region to the front of the free list.
    //
    // SAFETY: The caller must ensure that the memory region at [addr, addr+size)
    // is unused and large enough to hold a ListNode.
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        // Ensure the freed region can hold a ListNode
        assert!(align_up(addr, mem::align_of::<ListNode>()) == addr);
        assert!(size >= mem::size_of::<ListNode>());

        // Write a new ListNode at the start of the freed region.
        // The node IS the free region — it's stored inside the free space.
        let mut node = ListNode::new(size);
        node.next = self.head.next.take();
        let node_ptr = addr as *mut ListNode;
        unsafe {
            node_ptr.write(node);
            self.head.next = Some(&mut *node_ptr);
        }
    }

    // Find a free region that fits the given layout.
    // Returns a tuple of (the ListNode of the region, the aligned start address).
    //
    // Walks the free list from head to tail, checking each region.
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut ListNode, usize)> {
        let mut current = &mut self.head;

        // Walk the free list
        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(region, size, align) {
                // Found a fitting region — remove it from the list
                let next = region.next.take();
                let ret = Some((current.next.take().unwrap(), alloc_start));
                current.next = next;
                return ret;
            } else {
                // Region too small, move to next
                current = current.next.as_mut().unwrap();
            }
        }

        None // No fitting region found (out of memory)
    }

    // Check if a region can satisfy an allocation of `size` bytes with `align` alignment.
    // If yes, returns the aligned start address.
    fn alloc_from_region(region: &ListNode, size: usize, align: usize) -> Result<usize, ()> {
        let alloc_start = align_up(region.start_addr(), align);
        let alloc_end = alloc_start.checked_add(size).ok_or(())?;

        if alloc_end > region.end_addr() {
            return Err(()); // Region too small
        }

        let excess_size = region.end_addr() - alloc_end;
        if excess_size > 0 && excess_size < mem::size_of::<ListNode>() {
            // The leftover space is too small to hold a ListNode.
            // We can't split the region — we'd lose track of the leftover.
            // Only allow this allocation if it uses the ENTIRE region.
            return Err(());
        }

        Ok(alloc_start)
    }

    // Adjust a Layout so the resulting allocation is large enough to store a ListNode.
    // This ensures that when the block is freed, we can write a ListNode into it.
    fn size_align(layout: Layout) -> (usize, usize) {
        let layout = layout
            .align_to(mem::align_of::<ListNode>())
            .expect("adjusting alignment failed")
            .pad_to_align();
        let size = layout.size().max(mem::size_of::<ListNode>());
        (size, layout.align())
    }

    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        let (size, align) = Self::size_align(layout);

        if let Some((region, alloc_start)) = self.find_region(size, align) {
            let alloc_end = alloc_start.checked_add(size).expect("overflow");
            let excess_size = region.end_addr() - alloc_end;

            if excess_size > 0 {
                // Split: return the requested part, put the remainder back in the free list
                unsafe {
                    self.add_free_region(alloc_end, excess_size);
                }
            }

            Ok(unsafe { NonNull::new_unchecked(alloc_start as *mut u8) })
        } else {
            Err(())
        }
    }

    pub fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let (size, _) = Self::size_align(layout);
        unsafe {
            self.add_free_region(ptr.as_ptr() as usize, size);
        }
    }
}
