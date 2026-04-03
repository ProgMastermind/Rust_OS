// Keyboard Input Buffer
//
// A shared ring buffer between the keyboard interrupt handler and consumers
// (like the shell). The interrupt handler pushes characters in, and the
// shell pops them out.
//
// Ring buffer: a fixed-size circular array with read and write pointers.
//   - write_pos: where the next character will be written
//   - read_pos: where the next character will be read from
//   - When write_pos == read_pos, the buffer is empty
//   - When (write_pos + 1) % SIZE == read_pos, the buffer is full
//
// Protected by a spinlock since the interrupt handler and shell access it
// concurrently.

use spin::Mutex;

const BUFFER_SIZE: usize = 256;

struct RingBuffer {
    data: [u8; BUFFER_SIZE],
    read_pos: usize,
    write_pos: usize,
}

impl RingBuffer {
    const fn new() -> Self {
        RingBuffer {
            data: [0; BUFFER_SIZE],
            read_pos: 0,
            write_pos: 0,
        }
    }

    // Push a byte into the buffer. Returns false if the buffer is full.
    fn push(&mut self, byte: u8) -> bool {
        let next_write = (self.write_pos + 1) % BUFFER_SIZE;
        if next_write == self.read_pos {
            return false; // Buffer full — drop the character
        }
        self.data[self.write_pos] = byte;
        self.write_pos = next_write;
        true
    }

    // Pop a byte from the buffer. Returns None if empty.
    fn pop(&mut self) -> Option<u8> {
        if self.read_pos == self.write_pos {
            return None; // Empty
        }
        let byte = self.data[self.read_pos];
        self.read_pos = (self.read_pos + 1) % BUFFER_SIZE;
        Some(byte)
    }
}

// Global keyboard buffer, accessed from interrupt handler and shell.
static KEYBOARD_BUFFER: Mutex<RingBuffer> = Mutex::new(RingBuffer::new());

// Called by the keyboard interrupt handler to push a character.
// Logs a warning to serial if the buffer is full and input is dropped.
pub fn push_char(c: u8) {
    if !KEYBOARD_BUFFER.lock().push(c) {
        // Buffer is full. Log to serial (not VGA — we're in an interrupt handler
        // and don't want to contend for the VGA lock). This tells the developer
        // that input is being lost, rather than failing silently.
        crate::serial_println!("WARNING: keyboard buffer full, dropped '{}'", c as char);
    }
}

// Called by the shell (or any consumer) to read a character.
// Returns None if no character is available.
pub fn pop_char() -> Option<u8> {
    KEYBOARD_BUFFER.lock().pop()
}

// Blocking read: waits until a character is available.
// Uses hlt to sleep between checks (saves CPU).
// The timer interrupt wakes us from hlt, and we check again.
pub fn read_char() -> u8 {
    loop {
        if let Some(c) = pop_char() {
            return c;
        }
        // No character available — halt until next interrupt.
        // The keyboard interrupt will push a char, and the timer
        // interrupt will wake us from hlt so we can check again.
        x86_64::instructions::hlt();
    }
}
