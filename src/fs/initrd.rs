// In-memory ramdisk. Files are static byte slices compiled into the kernel.

use super::{FileInfo, FileSystem};

struct RamdiskFile {
    name: &'static str,
    contents: &'static [u8],
}

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

pub struct InitRamDisk;

pub static RAMDISK: InitRamDisk = InitRamDisk;

impl FileSystem for InitRamDisk {
    fn open(&self, path: &str) -> Option<usize> {
        FILES.iter().position(|f| f.name == path)
    }

    fn read(&self, file_index: usize, offset: usize, buf: &mut [u8]) -> usize {
        let file = match FILES.get(file_index) {
            Some(f) => f,
            None => return 0,
        };

        if offset >= file.contents.len() {
            return 0;
        }

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
