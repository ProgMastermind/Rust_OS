// Interactive shell. Reads stdin via syscall, executes built-in commands.

use crate::fs::FileSystem;
use crate::syscall;
use crate::{print, println};

const MAX_LINE_LENGTH: usize = 256;

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
            continue;
        }

        let line = match core::str::from_utf8(&line_buf[..len]) {
            Ok(s) => s.trim(),
            Err(_) => {
                println!("Error: invalid UTF-8 input");
                continue;
            }
        };

        execute_command(line);
    }
}

/// Read one byte from stdin. Blocks via Blocked(Stdin) if no data available.
fn read_stdin_char() -> u8 {
    let mut byte = [0u8; 1];
    loop {
        let result = syscall::syscall(
            syscall::SYS_READ,
            0,
            byte.as_mut_ptr() as u64,
            1,
        );
        if result > 0 {
            return byte[0];
        }
        crate::process::block_current(crate::process::WaitReason::Stdin);
    }
}

fn read_line(buf: &mut [u8]) -> usize {
    let mut pos = 0;

    loop {
        let c = read_stdin_char();

        match c {
            b'\n' => {
                println!();
                return pos;
            }
            0x08 | 0x7F => {
                if pos > 0 {
                    pos -= 1;
                    print!("\x08 \x08");
                }
            }
            // printable ASCII only
            0x20..=0x7E => {
                if pos < buf.len() - 1 {
                    buf[pos] = c;
                    pos += 1;
                    print!("{}", c as char);
                }
            }
            _ => {}
        }
    }
}

fn execute_command(line: &str) {
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
        "" => {}
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

    let fd = syscall::syscall(
        syscall::SYS_OPEN,
        args.as_ptr() as u64,
        args.len() as u64,
        0,
    );

    if fd < 0 {
        match fd {
            syscall::ENOENT => println!("cat: {}: No such file", args),
            syscall::EFAULT => println!("cat: internal error (bad pointer)"),
            syscall::EINVAL => println!("cat: {}: Invalid filename", args),
            _ => println!("cat: {}: Error ({})", args, syscall::errno_name(fd)),
        }
        return;
    }

    let mut buf = [0u8; 256];
    loop {
        let bytes_read = syscall::syscall(
            syscall::SYS_READ,
            fd as u64,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        );

        if bytes_read < 0 {
            println!("cat: read error: {}", syscall::errno_name(bytes_read));
            break;
        }
        if bytes_read == 0 {
            break;
        }

        let write_result = syscall::syscall(
            syscall::SYS_WRITE,
            1,
            buf.as_ptr() as u64,
            bytes_read as u64,
        );

        if write_result < 0 {
            println!("cat: write error: {}", syscall::errno_name(write_result));
            break;
        }
    }

    let close_result = syscall::syscall(syscall::SYS_CLOSE, fd as u64, 0, 0);
    if close_result < 0 {
        crate::serial_println!("cat: warning: close failed: {}", syscall::errno_name(close_result));
    }
}

fn cmd_pid() {
    let pid = syscall::syscall(syscall::SYS_GETPID, 0, 0, 0);
    if pid < 0 {
        println!("Error getting PID: {}", syscall::errno_name(pid));
    } else {
        println!("PID: {}", pid);
    }
}

fn cmd_uptime() {
    use crate::interrupts::TICKS;
    use core::sync::atomic::Ordering;

    let ticks = TICKS.load(Ordering::Relaxed);
    let seconds = ticks / 18;
    println!("Uptime: {} ticks (~{} seconds)", ticks, seconds);
}

fn cmd_exit() {
    println!("Shell exited.");
    crate::process::exit();
}
