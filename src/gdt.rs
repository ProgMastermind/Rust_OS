// Global Descriptor Table (GDT) + Task State Segment (TSS)
//
// The GDT is a legacy x86 structure. In 64-bit Long Mode, segmentation is
// mostly dead — but the GDT is STILL required for two things:
//   1. Defining the kernel code segment (the CPU checks segment permissions)
//   2. Pointing to the TSS (Task State Segment)
//
// The TSS contains the Interrupt Stack Table (IST) — a list of known-good
// stack pointers. When a double fault occurs, the CPU automatically switches
// to the stack specified in the IST. Without this, a stack overflow that
// triggers a page fault would cause a double fault on the SAME broken stack,
// which then triple faults and reboots the machine.

use lazy_static::lazy_static;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

// IST index for the double fault handler's stack.
// The IST has 7 entries (indices 0-6). We use index 0 for double faults.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

lazy_static! {
    // Create a TSS with a dedicated stack for double fault handling.
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();

        // IST entry 0: allocate a 20KB stack for the double fault handler.
        // We use a static array as the stack memory. In a real OS, you'd
        // allocate this from the kernel heap.
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5; // 20KB

            // This static mut is safe here because only the CPU reads it
            // (during double fault handling), and we only write it once
            // during initialization.
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            // Stack grows downward on x86, so the "top" is the END of the array.
            let stack_start = VirtAddr::from_ptr(&raw const STACK);
            stack_start + STACK_SIZE as u64  // stack_end = top of stack
        };

        tss
    };
}

lazy_static! {
    // Build the GDT with a kernel code segment and TSS segment.
    // We need to remember the segment selectors so we can load them into
    // the CPU's segment registers after loading the GDT.
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();

        // add_entry returns a SegmentSelector — a 16-bit value the CPU uses
        // to index into the GDT. We save these to load into CS and TSS registers.
        let code_selector = gdt.add_entry(Descriptor::kernel_code_segment());
        let tss_selector = gdt.add_entry(Descriptor::tss_segment(&TSS));

        (gdt, Selectors { code_selector, tss_selector })
    };
}

// Holds the segment selectors we need after loading the GDT.
struct Selectors {
    code_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

// Initialize the GDT: load it into the CPU and update segment registers.
pub fn init() {
    use x86_64::instructions::segmentation::{CS, Segment};
    use x86_64::instructions::tables::load_tss;

    // lgdt instruction — tells the CPU where the GDT is in memory.
    GDT.0.load();

    unsafe {
        // Reload the code segment register (CS) with our kernel code selector.
        // This is needed because the old GDT (set up by the bootloader) is
        // no longer valid after we loaded our own GDT.
        CS::set_reg(code_selector());

        // ltr instruction — tells the CPU where the TSS is.
        // Now the CPU knows about our IST stacks.
        load_tss(GDT.1.tss_selector);
    }
}

// Public accessor for the code selector — needed if other modules need it.
fn code_selector() -> SegmentSelector {
    GDT.1.code_selector
}
