// Initial Ramdisk (initrd) — In-Memory Filesystem
//
// A simple filesystem where all files are baked into the kernel binary
// at compile time. No disk I/O, no persistence — files are just static
// byte slices in kernel memory.
//
// This is the simplest possible filesystem implementation. Real OSes
// use initrd for early boot (before disk drivers are loaded), then
// switch to a real filesystem. Our kernel uses it permanently.
//
// The VFS trait abstracts over this, so the syscall layer doesn't know
// whether it's talking to a ramdisk, FAT partition, or network mount.

use super::{FileInfo, FileSystem};

// A file in the ramdisk: just a name and a byte slice.
struct RamdiskFile {
    name: &'static str,
    contents: &'static [u8],
}

// All files in our ramdisk, defined at compile time.
static FILES: &[RamdiskFile] = &[
    RamdiskFile {
        name: "hello.txt",
        contents: b"Hello from the ramdisk filesystem!\n",
    },
    RamdiskFile {
        name: "readme.txt",
        contents: b"Welcome to my_os!\nThis is an in-memory filesystem.\nFiles are baked into the kernel binary.\n",
    },
    RamdiskFile {
        name: "numbers.txt",
        contents: b"1\n2\n3\n4\n5\n",
    },
];

// The ramdisk "instance." Has no state — everything is in the static FILES array.
pub struct InitRamDisk;

// Global ramdisk instance. Since it has no mutable state, no lock is needed.
pub static RAMDISK: InitRamDisk = InitRamDisk;

impl FileSystem for InitRamDisk {
    fn open(&self, path: &str) -> Option<usize> {
        // Find the file by name. Returns its index in the FILES array.
        FILES.iter().position(|f| f.name == path)
    }

    fn read(&self, file_index: usize, offset: usize, buf: &mut [u8]) -> usize {
        let file = match FILES.get(file_index) {
            Some(f) => f,
            None => return 0,
        };

        // If offset is past end of file, nothing to read
        if offset >= file.contents.len() {
            return 0;
        }

        // Read as many bytes as possible (up to buf.len() or remaining file data)
        let remaining = &file.contents[offset..];
        let to_read = buf.len().min(remaining.len());
        buf[..to_read].copy_from_slice(&remaining[..to_read]);
        to_read
    }

    fn file_info(&self, file_index: usize) -> Option<FileInfo> {
        FILES.get(file_index).map(|f| FileInfo {
            name: f.name,
            size: f.contents.len(),
        })
    }

    fn file_count(&self) -> usize {
        FILES.len()
    }

    fn file_at(&self, index: usize) -> Option<FileInfo> {
        self.file_info(index)
    }
}
