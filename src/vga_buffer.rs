/// VGA Text Mode Driver
///
/// The VGA text buffer is a hardware-mapped region at physical address 0xB8000.
/// It's a 2D array of 25 rows x 80 columns. Each cell is 2 bytes:
///   - Byte 0: ASCII character code
///   - Byte 1: Color attribute (foreground + background)
///
/// We use `volatile` writes because the compiler would otherwise optimize away
/// writes to memory that's "never read" — but the hardware IS reading it to
/// draw characters on screen.

use core::fmt;
use lazy_static::lazy_static;
use spin::Mutex;
use volatile::Volatile;

/// Standard VGA color palette (4-bit).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// A VGA color attribute byte: 4 bits foreground + 4 bits background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ColorCode(u8);

impl ColorCode {
    fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

/// A single character cell in the VGA buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

/// The VGA text buffer — a 2D array of volatile character cells.
/// `Volatile` prevents the compiler from optimizing away writes.
#[repr(transparent)]
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

/// Writer that manages the current position and color in the VGA buffer.
pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    /// Write a single byte to the VGA buffer.
    /// Handles newlines and wraps at column 80.
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                let color_code = self.color_code;
                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code,
                });
                self.column_position += 1;
            }
        }
    }

    /// Write a string, replacing non-printable/non-ASCII chars with 0xFE (■).
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // Printable ASCII or newline
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                // Not part of printable ASCII range — show a placeholder
                _ => self.write_byte(0xfe),
            }
        }
    }

    /// Shift all rows up by one, clearing the bottom row.
    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    /// Fill a row with blank spaces.
    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

// Global writer instance, protected by a spinlock.
// `lazy_static!` lets us initialize it at runtime (we need a mutable reference
// to 0xB8000, which can't be created at compile time).
lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::Yellow, Color::Black),
        buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
    });
}

/// Print to VGA buffer (like `print!` but for our kernel).
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

/// Print to VGA buffer with newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Internal print function — called by the macros.
/// Public so macros can access it, but prefixed with _ to signal "don't call directly."
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    WRITER.lock().write_fmt(args).unwrap();
}
