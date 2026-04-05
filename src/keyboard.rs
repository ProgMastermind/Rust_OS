// Keyboard ring buffer. ISR pushes characters, shell pops them.

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

    fn push(&mut self, byte: u8) -> bool {
        let next_write = (self.write_pos + 1) % BUFFER_SIZE;
        if next_write == self.read_pos {
            return false;
        }
        self.data[self.write_pos] = byte;
        self.write_pos = next_write;
        true
    }

    fn pop(&mut self) -> Option<u8> {
        if self.read_pos == self.write_pos {
            return None;
        }
        let byte = self.data[self.read_pos];
        self.read_pos = (self.read_pos + 1) % BUFFER_SIZE;
        Some(byte)
    }
}

static KEYBOARD_BUFFER: Mutex<RingBuffer> = Mutex::new(RingBuffer::new());

/// Called from keyboard ISR. Wakes any stdin-blocked process after push.
pub fn push_char(c: u8) {
    if !KEYBOARD_BUFFER.lock().push(c) {
        crate::serial_println!("WARNING: keyboard buffer full, dropped '{}'", c as char);
        return;
    }
    crate::process::wake_blocked(crate::process::WaitReason::Stdin);
}

pub fn pop_char() -> Option<u8> {
    KEYBOARD_BUFFER.lock().pop()
}

/// Blocking read. Marks process as Blocked(Stdin) until keyboard ISR wakes it.
pub fn read_char() -> u8 {
    loop {
        if let Some(c) = pop_char() {
            return c;
        }
        crate::process::block_current(crate::process::WaitReason::Stdin);
    }
}
