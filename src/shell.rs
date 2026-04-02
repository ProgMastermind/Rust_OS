// Interactive Shell
//
// A kernel process that reads keyboard input and executes commands.
// This is the primary user interface of our OS.
//
// The shell loop:
//   1. Print prompt "my_os> "
//   2. Read characters from keyboard buffer until Enter
//   3. Parse the line into command + arguments
//   4. Match command and execute
//   5. Repeat
//
// This shell runs as a regular kernel task (spawned by the scheduler).
// It uses the keyboard ring buffer to read input and print!/println!
// for output. File operations go through the syscall interface.

use crate::fs::FileSystem;
use crate::keyboard;
use crate::syscall;
use crate::{print, println};

// Maximum length of a single input line.
const MAX_LINE_LENGTH: usize = 256;

// Entry point for the shell process. Called by the scheduler when
// this process first runs.
pub fn shell_main() {
    println!();
    println!("Welcome to my_os shell!");
    println!("Type 'help' for available commands.");
    println!();

    let mut line_buf = [0u8; MAX_LINE_LENGTH];

    loop {
        print!("my_os> ");
        let len = read_line(&mut line_buf);

        if len == 0 {
            continue; // Empty line — just print a new prompt
        }

        let line = match core::str::from_utf8(&line_buf[..len]) {
            Ok(s) => s.trim(),
            Err(_) => {
                println!("Invalid input");
                continue;
            }
        };

        execute_command(line);
    }
}

// Read a line of input from the keyboard buffer.
// Echoes characters as they're typed. Handles backspace.
// Returns the number of bytes in the line (not including newline).
fn read_line(buf: &mut [u8]) -> usize {
    let mut pos = 0;

    loop {
        let c = keyboard::read_char();

        match c {
            // Enter — line is complete
            b'\n' => {
                println!(); // Move to next line
                return pos;
            }
            // Backspace (0x08) or DEL (0x7F)
            0x08 | 0x7F => {
                if pos > 0 {
                    pos -= 1;
                    // Erase the character on screen: move cursor back, overwrite
                    // with space, move cursor back again.
                    print!("\x08 \x08");
                }
            }
            // Printable ASCII
            0x20..=0x7E => {
                if pos < buf.len() - 1 {
                    buf[pos] = c;
                    pos += 1;
                    // Echo the character to screen
                    print!("{}", c as char);
                }
            }
            // Ignore everything else (control chars, extended keys)
            _ => {}
        }
    }
}

// Parse and execute a command line.
fn execute_command(line: &str) {
    // Split into command and arguments at the first space
    let (cmd, args) = match line.find(' ') {
        Some(pos) => (&line[..pos], line[pos + 1..].trim()),
        None => (line, ""),
    };

    match cmd {
        "help" => cmd_help(),
        "echo" => cmd_echo(args),
        "clear" => cmd_clear(),
        "ls" => cmd_ls(),
        "cat" => cmd_cat(args),
        "pid" => cmd_pid(),
        "uptime" => cmd_uptime(),
        "exit" => cmd_exit(),
        "" => {} // Empty command — do nothing
        _ => println!("Unknown command: '{}'. Type 'help' for available commands.", cmd),
    }
}

fn cmd_help() {
    println!("Available commands:");
    println!("  help        - Show this help message");
    println!("  echo <text> - Print text to screen");
    println!("  clear       - Clear the screen");
    println!("  ls          - List files in the ramdisk");
    println!("  cat <file>  - Print file contents");
    println!("  pid         - Show current process ID");
    println!("  uptime      - Show timer ticks since boot");
    println!("  exit        - Exit the shell");
}

fn cmd_echo(args: &str) {
    println!("{}", args);
}

fn cmd_clear() {
    crate::vga_buffer::WRITER.lock().clear_screen();
}

fn cmd_ls() {
    let ramdisk = &crate::fs::initrd::RAMDISK;
    for i in 0..ramdisk.file_count() {
        if let Some(info) = ramdisk.file_at(i) {
            println!("  {} ({} bytes)", info.name, info.size);
        }
    }
}

fn cmd_cat(args: &str) {
    if args.is_empty() {
        println!("Usage: cat <filename>");
        return;
    }

    // Open the file via syscall
    let fd = syscall::syscall(
        syscall::SYS_OPEN,
        args.as_ptr() as u64,
        args.len() as u64,
        0,
    );

    if fd < 0 {
        println!("File not found: '{}'", args);
        return;
    }

    // Read and print in chunks
    let mut buf = [0u8; 256];
    loop {
        let bytes_read = syscall::syscall(
            syscall::SYS_READ,
            fd as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        );

        if bytes_read <= 0 {
            break; // EOF or error
        }

        // Write to stdout via syscall
        syscall::syscall(
            syscall::SYS_WRITE,
            1, // stdout
            buf.as_ptr() as u64,
            bytes_read as u64,
        );
    }

    // Close the file
    syscall::syscall(syscall::SYS_CLOSE, fd as u64, 0, 0);
}

fn cmd_pid() {
    let pid = syscall::syscall(syscall::SYS_GETPID, 0, 0, 0);
    println!("PID: {}", pid);
}

fn cmd_uptime() {
    use crate::interrupts::TICKS;
    use core::sync::atomic::Ordering;

    let ticks = TICKS.load(Ordering::Relaxed);
    // PIT fires at ~18.2 Hz, so ticks / 18 ≈ seconds
    let seconds = ticks / 18;
    println!("Uptime: {} ticks (~{} seconds)", ticks, seconds);
}

fn cmd_exit() {
    println!("Shell exited.");
    crate::process::exit();
}
