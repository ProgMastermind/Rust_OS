# rust-os

A minimal x86_64 kernel written in Rust. Boots to a shell prompt in QEMU, runs about 2400 lines of code with no standard library underneath.

This was built as a learning project to understand what actually happens between pressing the power button and seeing a blinking cursor. It covers the full stack: boot, interrupts, paging, heap allocation, preemptive multitasking, syscalls, a ramdisk filesystem, and an interactive shell.

It is not Linux. It runs on a single core, everything executes in Ring 0, and the filesystem is compiled into the binary. But every layer is real and written from scratch.

## What it does

- Boots via BIOS into 64-bit Long Mode
- VGA text output (0xB8000) and serial debug output (UART 0x3F8)
- Handles CPU exceptions (page fault, double fault with IST stack) and hardware interrupts (PIC timer at ~18Hz, PS/2 keyboard)
- 4-level page table management via the `x86_64` crate
- Bitmap physical frame allocator (O(n/64) alloc, O(1) dealloc, 128MB support)
- Three heap allocators: bump (educational), linked-list with coalescing, and fixed-size block (active, O(1))
- Preemptive round-robin scheduler with 15 lines of naked assembly for context switching
- Process stacks mapped at dedicated virtual addresses with unmapped guard pages for stack overflow detection
- Proper Blocked process state with wakeup from ISR (keyboard interrupt wakes stdin readers)
- Syscall interface via `int 0x80` with errno-style error codes and pointer validation
- In-memory ramdisk with a VFS trait
- Per-process file descriptor tables (stdin/stdout/stderr + opened files)
- Shell with 8 built-in commands: `help`, `echo`, `clear`, `ls`, `cat`, `pid`, `uptime`, `exit`
- 23 integration and unit tests that boot in QEMU and report over serial

## Requirements

- **Rust nightly** (tested on 1.96.0-nightly, 2026-03-25)
- **QEMU** (`qemu-system-x86_64`)
- **bootimage** tool

On Ubuntu/WSL:

```bash
rustup default nightly
rustup component add rust-src llvm-tools
cargo install bootimage
sudo apt install qemu-system-x86
```

## Running

```bash
cargo run          # boots in QEMU, opens a graphical window
cargo test         # runs all 23 tests headless via serial
```

`cargo run` opens a QEMU window with the shell. `cargo test` boots each test binary in QEMU with no display, reports pass/fail over serial, and exits via the `isa-debug-exit` device.

Currently supported shell commands:

```
my_os> help              # list available commands
my_os> echo hello world  # print text
my_os> ls                # list files in the ramdisk
my_os> cat hello.txt     # print file contents
my_os> cat readme.txt    # another file
my_os> pid               # show current process ID
my_os> uptime            # timer ticks since boot
my_os> clear             # clear the screen
my_os> exit              # terminate the shell process
```

## Write-up

[docs/blog-post.md](docs/blog-post.md) walks through the full build — the bugs, the design decisions, and what each layer actually does under the hood.

## Project layout

```
src/
  main.rs                 kernel entry point, boot sequence
  lib.rs                  crate root, test framework
  vga_buffer.rs           VGA text mode driver, println! macro
  serial.rs               UART serial driver, serial_println! macro
  gdt.rs                  GDT + TSS (double fault IST stack)
  interrupts.rs           IDT, exception/hardware/syscall handlers
  memory.rs               page table init, global mapper access
  frame_allocator.rs      bitmap physical frame allocator
  heap.rs                 kernel heap page mapping
  keyboard.rs             ring buffer between keyboard ISR and shell

  allocator/
    mod.rs                #[global_allocator], align_up
    bump.rs               bump allocator (educational, not active)
    linked_list.rs        free-list allocator with coalescing
    fixed_size_block.rs   per-size-class allocator (active)

  process/
    mod.rs                PCB, process table, spawn, exit, blocking
    context_switch.rs     naked asm register save/restore + RSP swap
    scheduler.rs          round-robin, called from timer ISR

  syscall/
    mod.rs                int 0x80 dispatch, pointer validation, errno codes
    fs.rs                 read, write, open, close
    process.rs            exit, getpid

  fs/
    mod.rs                VFS trait, FdEntry enum
    initrd.rs             static in-memory ramdisk (3 files)

  shell.rs                prompt, input via sys_read, built-in commands

tests/
  basic_boot.rs           kernel boots, println works
  heap_allocation.rs      Box, Vec, alignment, dealloc patterns, large alloc
  syscall_tests.rs        getpid, write, open/read/close, pointer validation, errno
```

## How the boot sequence works

1. `cargo run` builds the kernel ELF, `bootimage` stitches it with a bootloader, QEMU boots the disk image
2. The bootloader transitions Real Mode (16-bit) -> Protected Mode (32-bit) -> Long Mode (64-bit), sets up initial page tables mapping all physical memory at an offset, and jumps to `kernel_main`
3. `kernel_main` initializes in order: GDT/TSS, IDT, PICs, page table mapper, frame allocator, heap, global mapper storage, process table (idle process + shell), then enables interrupts
4. The timer fires, the scheduler context-switches to the shell, and the shell blocks on stdin waiting for keyboard input

## Dependencies

| Crate | What it handles |
|---|---|
| `bootloader` 0.9 | BIOS boot, mode transitions, memory map |
| `x86_64` 0.14 | Page tables, GDT, IDT, control registers |
| `pic8259` 0.10 | PIC initialization and EOI |
| `spin` 0.5 | Spinlock mutexes |
| `volatile` 0.2 | VGA buffer writes (prevents compiler elision) |
| `pc-keyboard` 0.7 | PS/2 scancode decoding |
| `uart_16550` 0.2 | Serial port setup |
| `lazy_static` 1.0 | Runtime-initialized statics |
