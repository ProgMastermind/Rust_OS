/// Serial Port (UART 16550) Driver
///
/// Outputs text to QEMU's host terminal via the serial port at I/O port 0x3F8.
/// This is invaluable for debugging — VGA output only shows in the QEMU window,
/// but serial output appears in your regular terminal.
///
/// We use the `uart_16550` crate which handles the UART initialization protocol
/// (baud rate, FIFO, etc.) so we can focus on using it.

use lazy_static::lazy_static;
use spin::Mutex;
use uart_16550::SerialPort;

lazy_static! {
    /// Global serial port instance at COM1 (I/O port 0x3F8).
    pub static ref SERIAL1: Mutex<SerialPort> = {
        // SAFETY: 0x3F8 is the standard COM1 I/O port address.
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}

/// Print to host terminal via serial port.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// Print to host terminal via serial port, with newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial::_print(format_args!("\n")));
    ($($arg:tt)*) => ($crate::serial::_print(format_args!("{}\n", format_args!($($arg)*))));
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    SERIAL1.lock().write_fmt(args).expect("Printing to serial failed");
}
