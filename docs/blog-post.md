# I Wrote an OS in Rust to Understand What My Code Runs On

---

I typed `cat hello.txt` into a shell I wrote, running on an OS I wrote, and 34 bytes appeared in bright yellow on a black screen. I stared at it for a solid minute. Not because it was impressive to look at. It was text on a screen. But I knew what had just happened underneath.

To get those 34 bytes from a file onto a display, the CPU walked four levels of page tables to translate a virtual address into a physical one. A software interrupt trapped through `int 0x80` into a syscall handler. A ramdisk returned static bytes through a per-process file descriptor table. A preemptive scheduler had been context-switching between the shell and an idle kernel loop the entire time I was typing, ~18 times per second, and I never noticed. The VGA hardware at address `0xB8000` painted each character cell. Two bytes each, one for the letter, one for the color.

All of this was built from nothing. About 3,300 lines of Rust. No standard library. No libc. No operating system underneath — just a bare CPU in 64-bit mode and a bunch of `unsafe` blocks that I can defend every single one of.

This is the story of how I built it. Not the polished version. The real one, with the bugs that took hours to find and the design decisions that only made sense after the third attempt.

> **Run it yourself:** The full source is at [github.com/ProgMastermind/rust-os](https://github.com/ProgMastermind/rust-os). You'll need Rust nightly (`rustup default nightly`), the `rust-src` and `llvm-tools` components, `bootimage` (`cargo install bootimage`), and QEMU. Then: `cargo run`. That's it. Boots to a shell prompt in under 2 seconds.

**Scope:** This is a single-CPU, Ring 0 kernel. All processes share one address space. There's no userspace isolation, no virtual memory per process, no Ring 3. The filesystem is a ramdisk compiled into the binary. The frame allocator caps at 128MB. There's no real disk driver, no networking, no signals. It's not mini-Linux. It's the minimum viable kernel that teaches you what a real OS is actually doing underneath.

---

## The Toolchain and the Void

The first line of any OS project in Rust is `#![no_std]`. I typed it, hit save, and the compiler vomited 37 errors. No `println!`. No heap. No `String` or `Vec`. No `main()`. No panic handler either. If your code panics, the compiler literally doesn't know what to do, so you have to write a function that tells it. Every single thing I'd ever taken for granted in Rust was gone in two words.

The second line is `#![no_main]`. Without a standard library, there's no runtime to call `main()`. The bootloader jumps directly to a function you define, passing a `BootInfo` struct with the physical memory map and a key number: the virtual offset where all of physical RAM has been identity-mapped.

To compile Rust for a machine with no OS, you need to rebuild the core library from source. That's what this file does:

```toml
[unstable]
build-std = ["core", "compiler_builtins", "alloc"]
build-std-features = ["compiler-builtins-mem"]

[build]
target = "x86_64-unknown-none"

[target.x86_64-unknown-none]
runner = "bootimage runner"
rustflags = ["-C", "relocation-model=static"]
```

Nine lines, and every one matters. `build-std` tells Cargo to compile `core` (the OS-independent foundation of Rust), `compiler_builtins` (things like `memcpy` and `memset` that the compiler expects to exist), and `alloc` (the heap allocation interfaces we'll need later). The target is `x86_64-unknown-none`: 64-bit x86, no vendor, no operating system.

The `runner` line is where it gets interesting. `bootimage runner` is a tool that takes our compiled ELF binary, wraps it in a bootloader, and produces a flat disk image that QEMU can boot. The bootloader itself does the real heavy lifting of early boot: it starts in 16-bit Real Mode (because every x86 CPU starts there, for backwards compatibility with the 8086 from 1978), transitions through 32-bit Protected Mode, and finally reaches 64-bit Long Mode. It sets up initial page tables, maps all physical RAM at a virtual offset, and then jumps to our kernel.

When I type `cargo run`, here's what actually happens:

```
cargo run
  |
  v
+-------------------------------------------+
|  rustc rebuilds core, alloc from source   |
|  target: x86_64-unknown-none              |
+-------------------+-----------------------+
                    v
+-------------------------------------------+
|  Compiles kernel -> ELF binary            |
+-------------------+-----------------------+
                    v
+-------------------------------------------+
|  bootimage stitches bootloader + kernel   |
|  -> bootimage-my_os.bin (flat disk)       |
+-------------------+-----------------------+
                    v
+-------------------------------------------+
|  QEMU boots the disk image                |
|  BIOS -> Real Mode (16-bit)              |
|       -> Protected Mode (32-bit)          |
|       -> Long Mode (64-bit)               |
|       -> kernel_main(boot_info)           |
+-------------------------------------------+
```

From `cargo run` to my code executing: one command, four major stages, three CPU mode transitions. Testing uses the same pipeline. `cargo test` boots each test binary in QEMU, runs every `#[test_case]` function, and reports pass/fail over the serial port. The test binary exits QEMU via a debug I/O port (`isa-debug-exit`), so a failing test kills the VM with exit code 1 and CI sees it immediately. The bootloader handles the part that can't be written in Rust (you need 16-bit and 32-bit assembly for the mode transitions), and we get to start in a clean 64-bit environment with a stack and a memory map.

Once `kernel_main` is called, there's a strict initialization order that matters. GDT first (because the IDT's double fault handler needs the TSS, which lives in the GDT). IDT second (so exception handlers are ready). PICs third (to remap hardware interrupts away from CPU exception numbers). But interrupts stay *disabled* through all of this. Memory management comes next: initialize the page table mapper, create the bitmap frame allocator from the bootloader's memory map, map heap pages, and initialize the heap allocator. Only after the heap exists can we use `Vec` and `Box`, which means the process table, file descriptor tables, and everything else that allocates must wait.

After the heap, we set up the process table, spawn the shell, and finally enable interrupts. That `enable_interrupts()` call is the point of no return. The timer starts firing immediately, and if any of the previous steps were incomplete, the first interrupt will crash into uninitialized state. The ordering isn't arbitrary. It's a dependency graph, and getting it wrong gives you a page fault before you've set up the page fault handler.

---

## First Signs of Life

After two days of fighting linker errors, toolchain configs, and a panic handler that itself panicked, I got `kernel_main` to execute. The VGA text buffer is the simplest possible display: a 2D array of 25 rows and 80 columns, starting at physical address `0xB8000`. Each cell is two bytes: one for the ASCII character, one for the color. Write a byte to `0xB8000`, and a character appears in the top-left corner of the screen.

```rust
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}
```

`#[repr(C)]` forces the struct layout to match what the hardware expects: one byte for the character, one byte for the color, no padding, no reordering. Without it, the Rust compiler is free to rearrange fields however it likes.

But there's a catch that cost me an embarrassing amount of time. The compiler sees you writing to `0xB8000` and thinks: nobody ever *reads* this memory in the program, so these writes are dead code. It optimizes them away. My kernel booted, ran `write_byte`, did everything correctly, and the screen was blank. No crash, no error, just... nothing. The fix is `Volatile<ScreenChar>`, a wrapper that tells the compiler "this memory has side effects, the hardware is reading it, do not touch these writes." The moment I added it, bright yellow text appeared on a black screen. I may have fist-pumped at my desk.

I also set up a serial port at I/O address `0x3F8`. The serial port writes to your host terminal instead of the QEMU window, which is a lifesaver for debugging when the VGA output is broken (which it will be, often). A pattern appears immediately that will recur throughout the entire kernel: `without_interrupts(|| WRITER.lock().write_fmt(args))`. Disable interrupts, acquire the spinlock, do the work, release the lock, re-enable interrupts. Skip any part of that ritual and you'll deadlock. The timer interrupt handler tries to print, grabs the same lock you're holding, and spins forever.

At this point I had a kernel that could scream into the void. It could paint characters on a screen and push bytes down a serial wire. What it couldn't do was react to anything. Press a key, nothing. Wait a second, nothing. The CPU executed `kernel_main`, hit the `hlt` loop, and sat there, deaf to the world.

---

## Teaching the CPU to Listen

Interrupts are the CPU's event system. They come from three sources: the CPU itself (exceptions like divide-by-zero or page fault), external hardware (timer, keyboard), and software (`int N` instructions, used for syscalls).

The Interrupt Descriptor Table (IDT) has 256 entries, one per interrupt number. Each entry says "when interrupt N fires, jump to this handler function." We set up handlers for six of them: breakpoint (entry 3), double fault (entry 8), page fault (entry 14), timer (entry 32), keyboard (entry 33), and our syscall entry (entry 0x80).

The handler functions use Rust's `extern "x86-interrupt"` calling convention. This is unusual. Normally a function returns with `ret`, which pops an 8-byte return address from the stack and jumps to it. But interrupt handlers return with `iretq`, which pops *five* values: the instruction pointer, code segment, flags register, stack pointer, and stack segment. These are the exact values the CPU pushed onto the stack when the interrupt fired. The `extern "x86-interrupt"` convention makes the compiler generate `iretq` instead of `ret`, so the interrupted code resumes exactly where it left off with all its state intact.

### The Double Fault Problem

Here's a scenario that took me an entire evening to understand, mostly because the symptom is "QEMU reboots instantly and you have no idea why." Say the kernel stack overflows. The CPU tries to push the page fault's stack frame onto the (broken) stack. That write hits unmapped memory. Another page fault. The CPU tries to push *that* stack frame onto the same broken stack. Page fault again. That's a double fault. And if the double fault handler also tries to use the broken stack, the CPU gives up entirely. Triple fault. Reboot. No error message. No stack trace. Just a reboot.

The fix is the Interrupt Stack Table (IST), stored in the Task State Segment (TSS). You allocate a dedicated 20KB stack, completely separate from any process's stack, and tell the CPU: "when a double fault happens, switch to *this* stack before calling the handler." The TSS lives inside the GDT (Global Descriptor Table), which is a legacy x86 structure that's mostly dead in 64-bit mode but still required for exactly two things: holding the kernel code segment selector and pointing to the TSS. Now the double fault handler runs on its own clean stack and can at least print a useful error message before halting.

This creates an initialization ordering constraint that's easy to get wrong. The GDT must be loaded first (because it contains the TSS pointer), and the IDT must be loaded after (because the double fault IDT entry references the IST index). Load them in the wrong order and the double fault handler has no stack to switch to.

### PIC Remapping

The Programmable Interrupt Controller (PIC) is a chip between hardware devices and the CPU. There are two of them, chained: PIC1 handles IRQs 0-7, PIC2 handles IRQs 8-15. By default, PIC1 maps IRQ 0-7 to interrupt numbers 0-7. But interrupt numbers 0-31 are reserved for CPU exceptions. So IRQ 0 (the timer) becomes interrupt 0 (divide error). Every timer tick, the CPU thinks you divided by zero. I actually hit this. My timer handler wasn't being called, but my divide-error handler was firing 18 times a second. It took me longer than I'd like to admit to connect those dots.

```rust
pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;
```

Two lines. We remap the PICs so IRQs 0-7 become interrupts 32-39, and IRQs 8-15 become interrupts 40-47. Timer becomes interrupt 32. Keyboard becomes interrupt 33. No more collisions.

The keyboard interrupt handler reads a scancode from I/O port `0x60` (you *must* read this port, otherwise the keyboard controller stops sending interrupts), decodes it through the `pc_keyboard` crate into a character, and pushes it into a 256-byte ring buffer. The ring buffer is a circular array with `read_pos` and `write_pos` indices. When `write_pos` catches up to `read_pos`, the buffer is full and new characters are dropped. The interrupt handler doesn't print directly. It just drops the character in and moves on. The shell, running as a separate process later, will read from the other end of that buffer at its own pace.

This separation (producer in an interrupt, consumer in a process) is a pattern that shows up everywhere in OS design. The interrupt handler must be fast (it blocks other interrupts while running), so it does the absolute minimum: read the hardware register, stash the result, get out. All the real work happens later.

After handling the scancode, both the timer and keyboard handlers must send an End-of-Interrupt (EOI) signal to the PIC. If you forget EOI, the PIC assumes the interrupt is still being handled and blocks all further interrupts at that priority level. Your timer stops ticking. Your keyboard goes dead. The system appears frozen, but the CPU is actually running fine. It just never hears from the hardware again.

So now I had interrupts. The timer ticked, the keyboard decoded scancodes, the ring buffer filled up. The kernel could both talk and listen. But everything so far had been working with raw physical addresses like "write to `0xB8000`," "read port `0x60`." That's fine for a few hardware registers, but to actually manage memory (allocate it, protect it, give each process its own view of it), I needed virtual memory. And virtual memory on x86_64 means page tables.

---

## The 4-Level Page Table

This is where the project stopped feeling like "Rust on bare metal" and started feeling like actual OS development. Every modern x86_64 CPU translates virtual addresses to physical addresses through a 4-level tree of page tables. A 48-bit virtual address is split into five fields:

```
[PML4 index (9 bits)] [PDPT index (9 bits)] [PD index (9 bits)] [PT index (9 bits)] [Offset (12 bits)]
    bits 47-39             bits 38-30           bits 29-21          bits 20-12          bits 11-0
```

The CPU reads a register called CR3, which holds the physical address of the top-level table (PML4). It uses the first 9 bits of the virtual address as an index into PML4, which gives the physical address of the next table (PDPT). It uses the next 9 bits to index into PDPT, getting the PD table. Then the next 9 bits for the PT table. The PT entry finally contains the physical frame address. Add the 12-bit offset, and you have the physical address.

Here's what the walk looks like for a concrete address. Say we're accessing our heap at `0x4444_4444_0000`:

```
Virtual address: 0x4444_4444_0000

Binary breakdown:
  PML4 index:  bits 47-39 = 0b 000_0100_01 = 68
  PDPT index:  bits 38-30 = 0b 000_0100_01 = 68
  PD index:    bits 29-21 = 0b 000_0100_01 = 68
  PT index:    bits 20-12 = 0b 000_000_000 = 0
  Offset:      bits 11-0  = 0x000

Walk:
  CR3 --> PML4[68] --> PDPT[68] --> PD[68] --> PT[0] --> physical frame
```

Each arrow is a memory read. Four dereferences to get from a virtual address to the physical frame backing it.

This walk happens on *every memory access*. The TLB (Translation Lookaside Buffer) caches recent translations so the CPU isn't actually traversing four pointer dereferences for every load and store. The TLB holds a few hundred entries, and for typical code with good locality, it hits on 99%+ of accesses. But when you change a page table entry (map or unmap a page), you have to flush the TLB entry for that address. Otherwise the CPU uses stale cached data and reads or writes to the wrong physical frame.

The whole thing looks like this:

```
                   CR3 register
                        |
                        v
                +---------------+
                |     PML4      |  512 entries, 4KB
                |   (Level 4)   |
                +-------+-------+
                        | index from bits 47-39
                        v
                +---------------+
                |     PDPT      |  512 entries, 4KB
                |   (Level 3)   |
                +-------+-------+
                        | index from bits 38-30
                        v
                +---------------+
                |      PD       |  512 entries, 4KB
                |   (Level 2)   |
                +-------+-------+
                        | index from bits 29-21
                        v
                +---------------+
                |      PT       |  512 entries, 4KB
                |   (Level 1)   |
                +-------+-------+
                        | index from bits 20-12
                        v
               +-----------------+
               |  Physical Frame  |  4KB of actual RAM
               |  + 12-bit offset |
               +-----------------+
```

### Why 4 Levels?

This is the part that made me stop and stare at my whiteboard for ten minutes. With 9 bits per level, each table has 2^9 = 512 entries. Each entry is 8 bytes. 512 × 8 = 4,096 bytes. That's exactly one 4KB page. I got chills when I realized this is not a coincidence. The entire hierarchy is designed so that page tables themselves are exactly one page. You allocate a page table using the same mechanism you'd allocate any page. It's page tables all the way down.

With 4 levels of 512 entries each: 512 * 512 * 512 * 512 * 4KB = 256 terabytes of addressable virtual memory. That's the theoretical maximum.

But here's the elegant part. You don't allocate tables you don't use. If your kernel uses addresses in one small region, you need exactly one PML4 entry (pointing to one PDPT), one PDPT entry (pointing to one PD), one PD entry (pointing to one PT), and however many PT entries for the pages you actually map. For a kernel with 128MB of usable RAM and sparse mappings, you might need 32KB of page tables total.

Compare that to a flat page table: mapping the full 48-bit address space with a single table would require 2^36 entries at 8 bytes each. That's 512GB just for the table. The 4-level hierarchy trades a few pointer dereferences per access for massive savings in metadata.

### Accessing the Tables

There's a bootstrapping problem. Page table entries contain *physical* addresses (the CPU's MMU reads them directly). But our code can only access *virtual* addresses. How do you read a page table entry if you need to translate the entry's address through page tables to find it?

The bootloader solves this by mapping all of physical RAM at a known virtual offset. If the offset is `0x1000_0000_0000`, then physical address `0x0` is accessible at virtual address `0x1000_0000_0000`. Physical address `0xB8000` (the VGA buffer) is at `0x1000_000B_8000`. And the physical address of PML4, read from CR3, is accessible at `CR3 + 0x1000_0000_0000`.

```rust
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    unsafe { &mut *page_table_ptr }
}
```

Five lines. Read the physical address from CR3, add the offset to get a virtual address, cast to a pointer, dereference. That's how you get from "the CPU has page tables" to "Rust code can read and modify them."

### The Frame Allocator

Before you can map new pages, you need physical frames to back them. The frame allocator tracks which 4KB frames are free using a bitmap: one bit per frame. 32,768 bits for 128MB of RAM, stored in 512 `u64` words. That's exactly 4KB, which itself fits in a single page.

There's a chicken-and-egg problem here. The bitmap is a kernel data structure, so you might think it belongs on the heap. But the heap doesn't exist yet. You need the frame allocator to *create* the heap (the heap needs physical frames backing its pages). The solution is to make the bitmap a `static` array. Static data lives in the BSS section of the ELF binary, which the bootloader loads into memory before calling `kernel_main`. No heap needed.

The bitmap starts with all bits set to 1, every frame marked "in use." The initialization function walks the bootloader's memory map, which describes which regions of physical RAM are usable and which are reserved (BIOS data, ACPI tables, memory-mapped I/O). For each usable region, it clears the corresponding bits. This is safe by default: if the bootloader doesn't mention a region, it stays marked as used and never gets handed out. Unknown memory is protected memory.

Allocation scans the bitmap for the first 0 bit using `trailing_ones()`, a Rust intrinsic that counts consecutive 1-bits from the least significant bit. On most x86_64 targets, LLVM lowers this to a single `tzcnt` or `bsf` instruction, making it effectively O(1) per word. If a u64 word is all ones, all 64 frames it tracks are in use, so skip it. If not, `trailing_ones()` tells you exactly which bit is the first free frame. Set it to 1 and return the frame.

```rust
let bit = word.trailing_ones() as usize;
let frame_idx = word_idx * 64 + bit;
bitmap[word_idx] |= 1u64 << bit;
```

A `next_scan` optimization avoids rescanning from the beginning every time. After a successful allocation, `next_scan` advances to the next frame. If the freed frame is before `next_scan`, we move the pointer back so it's found sooner. This turns the common case (sequential allocations) from O(n/64) to O(1).

Deallocation is always O(1): compute the frame index from the physical address, clear the bit. Done. This is a huge improvement over the bump allocator the project started with, which could never free frames. Once allocated, the memory was gone forever.

At this point I tried to write `let v = Vec::new();` and the compiler told me, politely, that there's no allocator. Right. I can map pages and track frames, but Rust's `alloc` crate needs a `#[global_allocator]` to back `Box`, `Vec`, and `String`. Time to build one.

---

## The Allocator Progression

This was the most satisfying part of the entire project. I built three allocators. Each one works. Each one has a fatal flaw that the next one fixes. The progression taught me more about systems programming than any textbook chapter on memory management.

### Heap Setup

First, you need to give the allocator some memory to work with. I mapped 25 pages (100KB) at virtual address `0x4444_4444_0000`, an arbitrary address chosen to avoid colliding with the kernel, the stack, or the VGA buffer. For each page, the frame allocator provides a physical frame, and the page table mapper creates the virtual-to-physical mapping. After this, reads and writes to the heap range hit real RAM.

### The Bump Allocator (and Why It Fails)

The simplest possible allocator. A single pointer called `next` that starts at the beginning of the heap and moves forward on every allocation. Want 64 bytes? Here's the current `next`, now `next` advances by 64. Want 32 more? Here's the new `next`, advance by 32.

```
Initial:   [___________________________________] next=start
            ^
After 64B: [################___________________] next moved forward
                            ^
After 32B: [################********___________] next moved again
                                    ^
```

Allocation is O(1). Bump the pointer, done. It also handles alignment: if you need 8-byte alignment and `next` is at `0x1003`, it skips to `0x1008` (wasting 5 bytes of "alignment padding"). But this is fine because allocation is absurdly fast.

What about deallocation?

```rust
pub fn dealloc(&mut self, _ptr: NonNull<u8>, _layout: Layout) {
    self.allocations -= 1;
    if self.allocations == 0 {
        self.next = self.heap_start;
    }
}
```

That's the complete deallocation logic. Decrement a counter. If every single allocation has been freed, reset the pointer to the start. Otherwise? The memory is gone. You can never get it back. Notice the parameters: `_ptr` and `_layout` are prefixed with underscores because the function never uses them. It doesn't *know* which block to free. It just counts.

This is a hack, and I knew it. It passes the test suite because test functions allocate inside a scope, drop everything, and the allocation count returns to zero. The heap resets. Clever. But the moment I plugged it into a real workload, a shell that allocates `String`s for commands, parses arguments, opens file descriptors, and never frees everything at once. The bump allocator hit 100KB and died. `alloc` returned null, Rust's `alloc_error_handler` fired, kernel panic. Three shell commands. That's all it took.

But that death was useful. `Box::new(42)` had compiled and run. `Vec::push()` worked. The heap infrastructure was sound. The mapping, the page table entries, the `#[global_allocator]` wiring. The allocator itself was the bottleneck, and I now understood *exactly* why: you can't just march forward forever. You need to track freed regions and reuse them, which means building a data structure *inside the very memory the allocator manages*.

### The Linked-List Allocator (and Its Costs)

A linked list of free memory regions. The trick that makes this possible: the list nodes are stored *inside* the free regions themselves. A free block of 64 bytes has a `ListNode` at its start: 8 bytes for the block's size and 8 bytes for the pointer to the next free block. When the block is allocated, the node vanishes (the caller owns that memory now, including the bytes where the node used to live). When the block is freed, a new node is written back into the freed space.

This leads to a consequence that's easy to miss: the minimum allocation size must be at least `size_of::<ListNode>()`, which is 16 bytes on x86_64. Ask for 1 byte and you'll actually consume 16, because when that 1-byte allocation is eventually freed, the allocator needs to write a 16-byte `ListNode` into it. If the block were smaller than 16 bytes, the node wouldn't fit, and the freed memory would be permanently lost. The allocator enforces this with a `size_align` function that silently rounds up every request.

Allocation walks the free list looking for a block big enough. If the block is bigger than needed, it splits: return the requested part to the caller, put the remainder back in the list as a smaller free block. But there's an edge case in splitting. If the leftover piece is smaller than 16 bytes (not enough for a `ListNode`), you can't split. You either use the entire block (wasting a few bytes) or skip it and keep searching. This is the kind of detail that's invisible in textbook descriptions but shows up the moment you implement it.

But there's a nastier problem called external fragmentation. Imagine you allocate 100 bytes, then 100, then 100. Free the first and third:

```
[FREE 100B] [USED 100B] [FREE 100B]
```

Total free space: 200 bytes. But if someone requests 200 bytes, neither free block is big enough. The memory is "free" but unusable. After enough alloc/free cycles with varying sizes, the heap can fragment into dozens of tiny free blocks, none large enough for the next allocation, even though the total free space is ample.

The fix is coalescing. When a block is freed, walk the list and merge it with any adjacent free blocks:

```rust
unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
    let mut new_start = addr;
    let mut new_size = size;

    let mut current = &mut self.head;
    while current.next.is_some() {
        let region = current.next.as_ref().unwrap();
        let region_start = region.start_addr();
        let region_end = region.end_addr();
        let region_size = region.size;

        if region_end == new_start {
            // Region is directly before us — absorb it
            new_start = region_start;
            new_size += region_size;
            current.next = current.next.as_mut().unwrap().next.take();
        } else if new_start + new_size == region_start {
            // Region is directly after us — absorb it
            new_size += region_size;
            current.next = current.next.as_mut().unwrap().next.take();
        } else {
            current = current.next.as_mut().unwrap();
        }
    }

    // Insert the merged region
    let mut node = ListNode::new(new_size);
    node.next = self.head.next.take();
    let node_ptr = new_start as *mut ListNode;
    node_ptr.write(node);
    self.head.next = Some(&mut *node_ptr);
}
```

When you free a block, the allocator checks: is there a free block immediately before it? Absorb it. Is there one immediately after? Absorb that too:

```
Before coalescing:
  HEAD -> [FREE 64B] -> [FREE 48B] -> [FREE 100KB]
  Can't satisfy a 100-byte allocation (64 and 48 are too small)

After freeing the USED block between the 64B and 48B regions:
  HEAD -> [FREE 160B] -> [FREE 100KB]
  The three adjacent blocks merged into one 160-byte block
```

Two free blocks next to each other become one larger block. This is the difference between an allocator that works in theory and one that survives real workloads.

The cost? Every allocation walks the free list. O(n). Every deallocation walks the list to find merge candidates. O(n). For an OS kernel where allocations happen constantly, this is too slow.

### The Fixed-Size Block Allocator (The Sweet Spot)

Nine separate free lists, one for each power-of-two block size from 8 to 2048 bytes:

```rust
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];
```

Need 20 bytes? Round up to 32. Pop a block from the 32-byte free list. O(1).

Free that 20-byte allocation? Push it back onto the 32-byte list. O(1).

```rust
pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
    match list_index(&layout) {
        Some(index) => {
            match self.list_heads[index].take() {
                Some(node) => {
                    self.list_heads[index] = node.next.take();
                    Ok(NonNull::from(node).cast())
                }
                None => {
                    let block_size = BLOCK_SIZES[index];
                    let block_align = block_size;
                    let layout = Layout::from_size_align(block_size, block_align).unwrap();
                    self.fallback_allocator.alloc(layout)
                }
            }
        }
        None => self.fallback_allocator.alloc(layout),
    }
}
```

If the appropriate free list is empty, fall back to the linked-list allocator to carve out a fresh block. If the allocation is larger than 2048 bytes, use the linked-list allocator directly. In practice, most kernel allocations are small (8-256 bytes), so the fast path handles 99% of cases.

Deallocation is just as fast. When a block is freed, the allocator writes a `ListNode` at the start of the freed memory (same trick as the linked-list allocator: reuse the freed space for bookkeeping) and pushes it onto the front of the appropriate free list. One pointer write, one pointer update. O(1).

The free lists start empty. When the kernel boots, there are no freed blocks to reuse. Every early allocation falls through to the linked-list allocator. But as the kernel runs and objects are created and destroyed, the per-size lists fill up. After the first dozen `Box<[u8; 32]>` allocations are freed, the 32-byte list has a dozen entries ready to go. The system gets faster over time as the free lists warm up. This is basically a simplified version of what production allocators like jemalloc call "slab allocation": pre-sized buckets that eliminate per-allocation bookkeeping.

The tradeoff is internal fragmentation. A 20-byte allocation sits in a 32-byte block. Those 12 bytes are wasted. But for an OS kernel, O(1) alloc/dealloc is worth far more than a few wasted bytes per allocation. Walking a free list on every `Vec::push()` would make the kernel noticeably slower.

Three allocators. Bump can't free. Linked list can free but is slow. Fixed-size block is fast with an acceptable tradeoff. Each one exists because the previous one broke. That progression (build it, watch it fail, understand *why* it failed, build the next one) taught me more than any textbook chapter ever did.

---

## Processes and the Magic of Context Switch

With `Box`, `Vec`, and `String` working, the kernel finally felt like a real Rust program. I could allocate data structures, build abstractions, use the standard collection types. But everything still ran as a single thread. To run more than one thing, like a shell that reads keyboard input while the kernel idles, you need processes.

A process in this kernel is a struct holding a PID, a state, a saved stack pointer, an entry function, a per-process file descriptor table, and a stack region. These are kernel threads. They share the same address space, same page tables, same heap. The only thing each process owns exclusively is its stack.

The state machine looks like this:

```
     spawn()
       |
       v
    +-------+   scheduler    +---------+
    | Ready | -------------> | Running |
    +-------+                +----+----+
       ^                          |
       |    wake_blocked()        | block_current()
       |                          v
       |                    +------------------+
       +------------------- | Blocked(reason)  |
                            +------------------+
    +---------+                   |
    |  Empty  | <-- reap <--------+ exit()
    +---------+            +------+------+
                           | Terminated  |
                           +-------------+
```

The process table starts in `kernel_main`. PID 0 is `kernel_main` itself, registered as the "idle process" with state `Running`. It doesn't get a dedicated mapped stack because it's already running on the bootloader's kernel stack. Then `spawn(shell_main)` creates PID 1, the shell. After that, `kernel_main` enables interrupts and enters an infinite `hlt` loop. From this point on, `kernel_main` is just the idle process, the thing the scheduler runs when nothing else is Ready. It does nothing but halt and wait for the next interrupt.

The scheduler is round-robin, called from the timer interrupt handler ~18.2 times per second. On each tick, it scans the process table starting from `current + 1`, wrapping around, looking for the first process with state `Ready`. If it finds one, it switches. If all other processes are Blocked, Terminated, or Empty, the idle process keeps running, which is exactly what you want, because the idle process calls `hlt`, which puts the CPU into a low-power state until the next interrupt. No busy-waiting, no wasted cycles.

The scheduler also reaps terminated processes on each tick. When a process calls `exit()`, it marks itself as `Terminated` and halts. On the next scheduler pass, the terminated process's file descriptor table and entry function are cleared, and the state flips to `Empty`. The stack pages aren't unmapped immediately (we're in interrupt context, and the mapper lock might deadlock), but `spawn()` will unmap them when the slot is reused. Lazy cleanup, but safe.

### Guard Pages

Each process gets 16KB of stack (4 pages) mapped at a dedicated virtual address. Below the stack is one page that's deliberately *not* mapped. The guard page. If the stack overflows, it grows downward into this unmapped page. The CPU tries to write, hits a page table entry marked "not present," and triggers a page fault. The page fault handler checks whether the address falls in a guard page region and panics with a clean "STACK OVERFLOW in PID N" message.

Without the guard page, stack overflow writes into whatever memory happens to be adjacent. Maybe it's another process's stack. Maybe it's kernel data structures. The corruption is silent, and the crash, when it eventually comes, happens in completely unrelated code. I can't think of a harder class of bug to track down.

### The Stack Alignment Puzzle

The x86_64 System V ABI requires that at function entry, the stack pointer must be congruent to 8 mod 16. That sounds arbitrary, but it's because the `call` instruction pushes an 8-byte return address onto a 16-aligned stack. The callee expects this. SSE instructions require 16-byte alignment, and the ABI is designed so that local variables on the stack land on 16-byte boundaries.

When `spawn()` sets up a new process's stack, it has to get this right. The initial stack frame has 8 values (6 callee-saved registers, a return address, and a padding slot):

```rust
let frame: [u64; 8] = [
    0,                                       // r15
    0,                                       // r14
    0,                                       // r13
    0,                                       // r12
    0,                                       // rbx
    0,                                       // rbp
    process_entry as *const () as u64,        // return address
    0,                                       // alignment padding
];
```

Trace the math: `stack_top` is page-aligned (0 mod 16). The frame is 64 bytes below. After 6 register pops (48 bytes), RSP is at `stack_top - 16`. Then `ret` pops the return address (8 bytes), leaving RSP at `stack_top - 8`. That's 8 mod 16. Correct.

Get this wrong and you'll see crashes in completely innocent code. Any function that uses SSE instructions (which the compiler can insert anywhere for floating-point math) will fault on the misaligned stack.

### Fifteen Lines of Assembly

When I first read that you can implement multitasking in 15 lines of assembly, I didn't believe it. Then I wrote it and it worked on the first try. (That never happens. I'm still suspicious.)

```asm
; Save old process
push rbp
push rbx
push r12
push r13
push r14
push r15
mov [rdi], rsp       ; Save current RSP

; Load new process
mov rsp, rsi         ; Switch to new stack
pop r15
pop r14
pop r13
pop r12
pop rbx
pop rbp
ret                  ; Jump to new process
```

Push six callee-saved registers onto the current stack. Save the stack pointer into the old process's struct. Load the new process's stack pointer. Pop six registers from the new stack. `ret`.

After the pushes, the old process's stack looks like this:

```
High addresses (stack top)
  +------------------------+
  |    ... old stack ...   |
  +------------------------+
  |   return address       |  <-- pushed by the `call` that invoked us
  +------------------------+
  |   rbp                  |
  +------------------------+
  |   rbx                  |
  +------------------------+
  |   r12                  |
  +------------------------+
  |   r13                  |
  +------------------------+
  |   r14                  |
  +------------------------+
  |   r15                  |  <-- RSP points here (saved to process struct)
  +------------------------+
Low addresses
```

We save RSP, switch to the new process's stack (which has an identical layout), pop six registers, and `ret`. The CPU resumes the new process with all its state intact.

Why only six registers? The x86_64 has sixteen general-purpose registers. The System V ABI splits them into two groups: *caller-saved* (rax, rcx, rdx, rsi, rdi, r8-r11) and *callee-saved* (rbp, rbx, r12-r15). Caller-saved registers can be trashed by any function call, so the caller saves them before calling if it needs them later. Callee-saved registers must be preserved across calls. Since `context_switch` looks like a function call to the old process (the compiler generated a `call` instruction to get here), the compiler already saved any caller-saved registers it cared about. We only need to save the six that the ABI says we must preserve.

That last instruction is the magic. `ret` pops the return address from the *new* process's stack and jumps to it. For a process that was previously running, the return address is wherever it was when context_switch was last called, which is inside the timer interrupt handler. The process resumes exactly where it left off.

For a brand-new process, the return address is `process_entry` (we placed it on the stack in `spawn()`). The process starts executing for the first time.

The CPU doesn't know any of this. It just follows the stack pointer, pops a value, and jumps. The illusion of "multiple processes" is entirely created by saving and restoring the right pointers.

One detail that took me an hour to figure out: `process_entry()` immediately calls `x86_64::instructions::interrupts::enable()`. Why? Because context_switch runs inside the timer interrupt handler, where the CPU has automatically cleared the interrupt flag (IF=0). If the new process doesn't re-enable interrupts, the timer never fires again. No timer means no scheduler. The process runs until the heat death of the universe. My shell printed its welcome message perfectly. And then froze. No keyboard input, no timer ticks, nothing. Because I'd forgotten that one line.

The moment I added `interrupts::enable()` and both processes started taking turns, the idle loop halting, the shell waiting for input, the timer switching between them, I sat back and just watched the serial output. `schedule: switching PID 0 -> PID 1`. `schedule: switching PID 1 -> PID 0`. Back and forth, 18 times a second. Two processes, taking turns on a single CPU. Fifteen lines of assembly made it happen.

---

## Three Ways to Deadlock Your Kernel

Once you have a scheduler running inside an interrupt handler, three bugs become possible. Each one looks like the obvious thing to do, and each one freezes the system solid. I think of them as the three deadly sins of interrupt-context programming.

**Sin 1: Using `lock()` in interrupt context.** The scheduler needs the process table. The obvious call is `PROCESS_TABLE.lock()`. But imagine this sequence: `spawn()` acquires the process table lock, the timer interrupt fires mid-spawn, the interrupt handler calls `schedule()`, which calls `PROCESS_TABLE.lock()`. The lock is held by `spawn()`, which can't run because the interrupt handler hasn't returned. The interrupt handler can't return because it's waiting for the lock. Both sides wait forever. Your kernel is frozen, both cores of the deadlock visible only in your imagination.

The fix: `try_lock()`. If the table is already locked, return immediately and skip this scheduler tick. You miss one scheduling opportunity (~55ms), but the system doesn't freeze. The next timer tick will try again. The same pattern applies everywhere interrupts interact with shared state. `wake_blocked()` also uses `try_lock()` because it's called from the keyboard interrupt handler.

**Sin 2: Holding a lock across `context_switch`.** This one was my favorite because the symptoms were so confusing. The scheduler acquires the process table lock, picks the next process, and calls `context_switch()` while still holding the lock. The new process wakes up and... the lock is held. By whom? By the *old* process, which is now suspended. Every attempt to lock the process table (from the keyboard handler, from `spawn()`, from the next timer tick) will deadlock.

```rust
let old_rsp_ptr = &mut table.processes[current_idx].stack_pointer as *mut u64;
let new_rsp = table.processes[next_idx].stack_pointer;

// Drop the lock BEFORE context switching
drop(table);

unsafe { context_switch(old_rsp_ptr, new_rsp); }
```

That `drop(table)` on line 94 is the most important line in the scheduler. Without it, the kernel freezes the moment the second process runs.

**Sin 3: Sending EOI after `context_switch`.** The PIC chip blocks all further interrupts of equal or lower priority until it receives an End-of-Interrupt (EOI) signal. The timer handler needs to send EOI. If you send it *after* calling the scheduler, and the scheduler switches to a different process, you have a problem: `context_switch` doesn't return to the old process's timer handler. It returns to the *new* process. The old process's EOI line never executes. The PIC never gets its signal. No more timer interrupts. Scheduling stops.

The fix: send EOI *before* calling the scheduler. In `timer_interrupt_handler`, the EOI goes out immediately after incrementing the tick counter, before `schedule()` even runs. It doesn't matter if `schedule()` switches away. The PIC has already been acknowledged.

All three of these bugs have something in common: the code compiles perfectly, runs without warnings, and works fine with a single process. They only manifest when two processes are actively switching. That's what makes concurrent bugs so hard to catch. You think your code is correct until the second thread shows up.

---

## Syscalls: Crossing the Boundary

At this point, processes could run and take turns. But there was no way for a process to ask the kernel for anything. Read a file, write to the screen, get its own PID. The process and the kernel were isolated from each other. I needed a bridge.

The mechanism is the same one Linux used for years before `syscall`/`sysret`: software interrupt `int 0x80`. I chose it over the `syscall` instruction because `int 0x80` reuses the existing IDT infrastructure, so there's no MSR configuration, no separate entry point in assembly. Even though our processes run in Ring 0 (same privilege level as the kernel), I implemented the full ceremony, because the abstraction matters even without a real privilege boundary.

```
  Process                         Kernel
     |                               |
     |  syscall(SYS_READ, 0, buf, 1) |
     |  +- disable interrupts -+     |
     |  |  write args to struct |     |
     |  |  int 0x80 ------------------>
     |  |                      |  dispatch
     |  |                      |  sys_read
     |  |                      |  pop_char
     |  |                      |  store rv
     |  |  iretq <--------------------
     |  |  read return value   |     |
     |  +- enable interrupts --+     |
     |                               |
```

```rust
pub fn syscall(number: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let args = unsafe { &mut *SYSCALL_ARGS.0.get() };
        args.number = number;
        args.arg1 = arg1;
        args.arg2 = arg2;
        args.arg3 = arg3;

        unsafe { asm!("int 0x80", options(nostack)) };

        let args = unsafe { &*SYSCALL_ARGS.0.get() };
        args.return_value
    })
}
```

Disable interrupts. Write the syscall number and arguments to a shared static. Fire `int 0x80`. The handler reads the arguments, dispatches to the right kernel function, stores the return value, and returns via `iretq`. The wrapper reads the return value and returns it to the caller.

The shared static uses `UnsafeCell`, Rust's primitive for interior mutability. `UnsafeCell` is `!Sync` by default (the compiler refuses to share it between threads), so we implement `Sync` manually and document the safety contract: interrupts are disabled during access, there's only one CPU core, `int 0x80` runs to completion before `iretq`, and no nested syscalls are possible. Four invariants, all enforced by the architecture, not the type system. This is the kind of thing where Rust's `unsafe` earns its keep. The block forces you to write down exactly why this works, which means you notice when one of those invariants changes.

Why pass arguments through a struct instead of registers? The `extern "x86-interrupt"` calling convention doesn't expose the caller's general-purpose registers to the handler. A production OS would use a naked assembly stub to read `rdi`, `rsi`, `rdx` directly. That's what Linux's `syscall` entry point does. The shared struct is simpler to understand at the cost of a few extra memory accesses.

The arguments include raw pointers (for read/write buffers), so every syscall validates pointers before dereferencing them: null check, length bound (1MB sanity limit), overflow check (`ptr + len` must not wrap around the address space). Without this, a buggy or malicious caller could pass a pointer into kernel data structures and the handler would blindly dereference it, corrupting internal state or leaking sensitive memory. Even in Ring 0 (where everything has full access anyway), validation catches bugs early and documents the interface contract.

Six syscalls total: `read`, `write`, `open`, `close`, `exit`, `getpid`. Error codes follow the errno convention: negative return values with specific meanings. `-1` for ENOENT (file not found), `-2` for EBADF (bad file descriptor), `-4` for EFAULT (bad pointer). The `cat` shell command exercises the full pipeline: `sys_open` to get a file descriptor from the ramdisk, `sys_read` in a loop until EOF, `sys_write` to push each chunk to stdout (VGA), and `sys_close` to release the descriptor.

Each process has its own file descriptor table, a `Vec<Option<FdEntry>>` where the index is the fd number. Every process starts with three pre-opened descriptors: fd 0 (stdin, backed by the keyboard ring buffer), fd 1 (stdout, which writes to the VGA screen), and fd 2 (stderr, which writes to the serial port). When `sys_open` succeeds, it finds the first `None` slot in the table and inserts a `File` entry with the ramdisk file index and a read position starting at 0. The fd number returned to the caller is just the index into this Vec. `sys_close` sets the slot back to `None`. Closing stdin, stdout, or stderr returns `EPERM` because you can't close the standard streams.

The ramdisk is dead simple: a static array of files compiled into the kernel binary. Three files (`hello.txt`, `readme.txt`, `numbers.txt`) with their contents as byte arrays. Opening a file does a linear scan by name and returns an index. Reading copies bytes from the static slice into the caller's buffer, advancing the file position. When the position reaches the end of the content, `read` returns 0. EOF.

Behind the ramdisk is a `FileSystem` trait with methods for `open`, `read`, `file_info`, and `file_count`. The ramdisk is the only implementation, but the trait means the syscall layer doesn't know that. If I added a FAT filesystem backed by a disk driver, the syscalls wouldn't change. They'd just talk to a different `FileSystem` impl. I know this is over-designed for a project with one filesystem, but the VFS abstraction is one of the most important ideas in OS architecture. Unix's "everything is a file" philosophy starts here.

---

## The Shell and the Blocking I/O Story

Everything I've described so far has been infrastructure. Page tables, allocators, schedulers, syscalls. All plumbing. The shell is where the plumbing disappears and a user sees a prompt. It prints `my_os> `, waits for you to type a line, parses it into a command and arguments, and executes it. Eight commands: `help`, `echo`, `clear`, `ls`, `cat`, `pid`, `uptime`, `exit`.

The interesting part isn't the commands. It's how the shell waits for keyboard input.

When the shell needs a character, it calls `sys_read(fd=0, buf, 1)` to read one byte from stdin. The syscall handler for fd 0 checks the keyboard ring buffer. If a character is available, it pops and returns it. If the buffer is empty, it returns 0.

Here's the problem, and it's a fundamental one. The syscall handler runs inside `int 0x80` with interrupts disabled (IF=0). My first attempt at stdin was the obvious thing:

```rust
//  THE VERSION THAT FROZE THE KERNEL
// Inside sys_read handler — interrupts are OFF (IF=0)
fn sys_read_stdin(buf: &mut [u8]) -> i64 {
    loop {
        if let Some(c) = keyboard::pop_char() {
            buf[0] = c;
            return 1;
        }
        // Spin until a character arrives...
        // But the keyboard interrupt can never fire.
        // IF=0. We're inside int 0x80. Nothing can interrupt us.
        // This loop runs forever. Kernel is dead.
    }
}
```

I stared at a frozen QEMU window for twenty minutes before it clicked. The syscall handler runs with interrupts disabled. The keyboard interrupt *cannot fire*. The character will *never arrive*. The loop spins forever. The kernel is dead, and there's no error message. Just silence.

The fix required rethinking the entire approach. `sys_read` for stdin must be **non-blocking**: return immediately with whatever's in the buffer, or 0 if empty. The *blocking* moves out of the syscall handler and into the process itself, where interrupts are enabled:

```rust
// THE VERSION THAT WORKS
// Outside the syscall handler — interrupts can fire here
fn read_stdin_char() -> u8 {
    let mut byte = [0u8; 1];
    loop {
        let result = syscall::syscall(syscall::SYS_READ, 0, byte.as_mut_ptr() as u64, 1);
        if result > 0 {
            return byte[0];  // Got a character
        }
        // No data — block until keyboard ISR wakes us.
        // Interrupts are ON here, so the keyboard ISR can fire,
        // push a character, and call wake_blocked(Stdin).
        crate::process::block_current(crate::process::WaitReason::Stdin);
    }
}
```

The difference is *where* the waiting happens. Inside the syscall handler (IF=0): dead. Outside the syscall handler (IF=1): the keyboard interrupt fires, pushes the character, wakes us up, and the next `sys_read` call succeeds.

If `sys_read` returns 0 (no data), the shell calls `block_current(Stdin)`. This marks the shell process as `Blocked(Stdin)` and enters a halt-and-check loop. The scheduler sees the shell is blocked and skips it entirely. Zero overhead. No context switching to the shell, no checking if data is available, nothing.

On the other end, when you press a key, the keyboard interrupt handler pushes the character into the ring buffer and calls `wake_blocked(Stdin)`:

```rust
pub fn push_char(c: u8) {
    if !KEYBOARD_BUFFER.lock().push(c) {
        crate::serial_println!("WARNING: keyboard buffer full, dropped '{}'", c as char);
        return;
    }
    crate::process::wake_blocked(crate::process::WaitReason::Stdin);
}
```

`wake_blocked` flips every process with state `Blocked(Stdin)` to `Ready`. On the next timer tick, the scheduler finds the shell is Ready, context-switches to it, and the shell resumes inside `block_current`. It checks: still blocked? No, someone called `wake_blocked`. It breaks out of the loop, retries `sys_read`, gets the character, and returns it to `read_line`.

The first time this full loop worked, I pressed a key, the shell echoed it, I typed `help`, and eight commands printed. I just sat there watching it. It felt like the machine was *alive*. Not in a dramatic way. In the way that a thing you built from nothing suddenly responds to you.

Before I implemented proper blocking, the shell used hlt-polling. It called `hlt()`, got woken by every timer tick (~18 times per second), checked the keyboard buffer, found nothing, and went back to sleep. That's 18 context switches per second. Scheduler runs, picks the shell, context-switches to it, shell checks the buffer, finds nothing, halts, timer fires, scheduler runs, picks the idle process, context-switches back. All of that to accomplish nothing. With proper blocking, the scheduler sees `Blocked(Stdin)` and skips the shell entirely. Zero context switches until a key arrives.

Let me trace the full lifecycle of a single keystroke. Say you press the 'h' key while the shell is waiting at the prompt:

1. The shell is `Blocked(Stdin)`. The idle process (PID 0) is `Running`, sitting in its `hlt` loop. The CPU is halted, drawing minimal power.
2. You press 'h'. The keyboard controller raises IRQ 1. The PIC translates this to interrupt 33 and signals the CPU. The CPU wakes from `hlt`, pushes the idle process's state onto the stack (RIP, CS, RFLAGS, RSP, SS), and jumps to `keyboard_interrupt_handler`.
3. The handler reads scancode from port `0x60`, decodes it to the character 'h', and calls `push_char('h')`.
4. `push_char` pushes 'h' into the ring buffer and calls `wake_blocked(Stdin)`.
5. `wake_blocked` uses `try_lock` on the process table, finds the shell with state `Blocked(Stdin)`, flips it to `Ready`.
6. The handler sends EOI to the PIC and returns via `iretq`. The idle process resumes its `hlt` loop.
7. The next timer tick fires (~55ms later at most). The scheduler's `try_lock` succeeds, it scans from the current process (PID 0) and finds PID 1 (shell) is `Ready`.
8. The scheduler sets PID 1 to `Running`, sets PID 0 to `Ready`, extracts both stack pointers, drops the lock, and calls `context_switch`.
9. `context_switch` saves the idle process's registers, swaps to the shell's stack, restores the shell's registers, and `ret` jumps back into the shell's `block_current` function.
10. The shell checks: am I still `Blocked`? No, `wake_blocked` set me to `Ready` (now `Running`). Break out of the loop, return to `read_stdin_char`.
11. `read_stdin_char` retries `syscall(SYS_READ, 0, buf, 1)`. The syscall handler calls `pop_char()` on the ring buffer, finds 'h', copies it to the buffer, returns 1.
12. `read_stdin_char` returns 'h' to `read_line`, which adds it to the line buffer and echoes it to the screen. A yellow 'h' appears at the cursor position.

Twelve steps, five subsystems, two interrupt handlers, one context switch, and a character on the screen. This is the moment where every piece of the kernel works together. And that's the reason the whole project exists.

---

## What I Actually Learned

The dependency chain in this project is total:

```
  Shell prompt
       | needs
  Blocking I/O (block_current / wake_blocked)
       | needs
  Preemptive Scheduler (round-robin, timer-driven)
       | needs
  Context Switch (15 lines of assembly)
       | needs
  Process Stacks (mapped pages with guard pages)
       | needs
  Page Table Mapper (4-level walk, map_to / unmap)
       | needs
  Frame Allocator (bitmap, one bit per 4KB frame)
       | needs
  Bootloader Memory Map (which regions are usable?)
       | needs
  Bootloader (Real Mode -> Long Mode, built by bootimage)
```

Pull any piece out and everything above it collapses. It's the most vertically integrated thing I've ever built. Eight layers, and I wrote all of them except the bootloader.

I knew this intellectually before I started. But there's a difference between reading about page tables in a textbook and sitting in front of a QEMU window where nothing appears on screen because your stack alignment is off by 8 bytes and `ret` lands at an address that's technically valid but points to the middle of a different function's prologue. That kind of understanding doesn't come from reading. It comes from the hour you spend staring at a wrong RSP value in a hex dump, working backwards through the context switch, realizing the padding slot is missing, and adding one line of code that fixes everything.

The same goes for the allocator progression. You can read that "bump allocators can't free memory" and nod along. But when you watch your kernel run out of heap space on the third shell command because three `String` allocations never got reclaimed, you actually feel it. And then the linked-list allocator works, and coalescing works, and you watch the free list merge two blocks into one, and something clicks about why real allocators are shaped the way they are.

Rust helped. `Mutex<T>` forces you to think about lock ownership, `unsafe` blocks are a paper trail for every hardware interaction, and the compiler caught real bugs that would have been silent memory corruption in C. But Rust didn't save me from the three deadlocks in the scheduler, the interrupt ordering problem with EOI, or the ABI alignment requirement for process stacks. Those are logic errors, architecture knowledge, and calling conventions. The kind of bugs where the compiler shrugs and says "looks fine to me" and you stare at a frozen QEMU window until you figure out what you forgot. A kernel is the one piece of software that can't blame anything underneath it.

If I kept going, the next steps are clear: per-process page tables with CR3 switching, Ring 3 userspace with `sysret`/`syscall` instead of `int 0x80`, SMP support (multiple CPUs means real lock contention, not just interrupt-context deadlocks), a proper filesystem on a virtio block device, and ELF loading so processes come from disk instead of function pointers. Each one peels back another layer of "how does a real OS do this?" And each one will probably break something I thought was solid.

3,300 lines. No standard library. A shell prompt that blinks, waiting for you to type something. It's not much. But every character that appears on that screen passed through code I wrote, on a machine I told what to do, and there's nothing else underneath.
