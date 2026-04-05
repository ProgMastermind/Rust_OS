// Linked-list free-list allocator with coalescing.
// List nodes are stored inside the free regions themselves.

use core::alloc::Layout;
use core::mem;
use core::ptr::NonNull;
use super::align_up;

// Stored inside the free region itself -- no separate metadata allocation needed.
// This is why the minimum alloc size must be >= size_of::<ListNode>().
struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new(size: usize) -> Self {
        ListNode { size, next: None }
    }

    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

/// Free-list allocator. Walks a linked list of free regions for each alloc. O(n) but supports real dealloc.
pub struct LinkedListAllocator {
    head: ListNode, // dummy head, not a real free region
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        LinkedListAllocator {
            head: ListNode::new(0),
        }
    }

    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        unsafe {
            self.add_free_region(heap_start, heap_size);
        }
    }

    /// Insert a free region, merging with any adjacent free blocks to prevent fragmentation.
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        assert!(align_up(addr, mem::align_of::<ListNode>()) == addr);
        assert!(size >= mem::size_of::<ListNode>());

        let mut new_start = addr;
        let mut new_size = size;

        // Walk free list and absorb adjacent regions
        let mut current = &mut self.head;
        while current.next.is_some() {
            let region = current.next.as_ref().unwrap();
            let region_start = region.start_addr();
            let region_end = region.end_addr();
            let region_size = region.size;

            if region_end == new_start {
                // region is directly before us, merge
                new_start = region_start;
                new_size += region_size;
                current.next = current.next.as_mut().unwrap().next.take();
            } else if new_start + new_size == region_start {
                // region is directly after us, merge
                new_size += region_size;
                current.next = current.next.as_mut().unwrap().next.take();
            } else {
                current = current.next.as_mut().unwrap();
            }
        }

        let mut node = ListNode::new(new_size);
        node.next = self.head.next.take();
        let node_ptr = new_start as *mut ListNode;
        unsafe {
            node_ptr.write(node);
            self.head.next = Some(&mut *node_ptr);
        }
    }

    /// Walk the free list for a region that fits. Returns the node and aligned start address.
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut ListNode, usize)> {
        let mut current = &mut self.head;

        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(region, size, align) {
                let next = region.next.take();
                let ret = Some((current.next.take().unwrap(), alloc_start));
                current.next = next;
                return ret;
            } else {
                current = current.next.as_mut().unwrap();
            }
        }

        None
    }

    /// Check if a region can satisfy the allocation. Returns aligned start if yes.
    fn alloc_from_region(region: &ListNode, size: usize, align: usize) -> Result<usize, ()> {
        let alloc_start = align_up(region.start_addr(), align);
        let alloc_end = alloc_start.checked_add(size).ok_or(())?;

        if alloc_end > region.end_addr() {
            return Err(());
        }

        // Can't split if leftover is too small for a ListNode
        let excess_size = region.end_addr() - alloc_end;
        if excess_size > 0 && excess_size < mem::size_of::<ListNode>() {
            return Err(());
        }

        Ok(alloc_start)
    }

    // Round up layout so freed blocks are large enough to hold a ListNode (16 bytes).
    // Without this, small allocations couldn't be tracked in the free list after dealloc.
    fn size_align(layout: Layout) -> (usize, usize) {
        let layout = layout
            .align_to(mem::align_of::<ListNode>())
            .expect("adjusting alignment failed")
            .pad_to_align();
        let size = layout.size().max(mem::size_of::<ListNode>());
        (size, layout.align())
    }

    /// Find a fitting region, split if larger than needed, return the allocation.
    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        let (size, align) = Self::size_align(layout);

        if let Some((region, alloc_start)) = self.find_region(size, align) {
            let alloc_end = alloc_start.checked_add(size).expect("overflow");
            let excess_size = region.end_addr() - alloc_end;

            if excess_size > 0 {
                unsafe {
                    self.add_free_region(alloc_end, excess_size);
                }
            }

            Ok(unsafe { NonNull::new_unchecked(alloc_start as *mut u8) })
        } else {
            Err(())
        }
    }

    /// Return a block to the free list. Coalesces with neighbors automatically.
    pub fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let (size, _) = Self::size_align(layout);
        unsafe {
            self.add_free_region(ptr.as_ptr() as usize, size);
        }
    }
}
