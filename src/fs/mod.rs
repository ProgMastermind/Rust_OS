// VFS trait and file descriptor types.

pub mod initrd;

/// Per-process file descriptor entry. Index in the fd_table Vec is the fd number.
#[derive(Debug, Clone)]
pub enum FdEntry {
    Stdin,
    Stdout,
    Stderr,
    File {
        file_index: usize,
        position: usize,
    },
}

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: &'static str,
    pub size: usize,
}

/// Filesystem abstraction. The ramdisk implements this; syscall layer uses it.
pub trait FileSystem {
    /// Look up file by path. Returns a file index (not an fd).
    fn open(&self, path: &str) -> Option<usize>;
    /// Read bytes from file at offset into buf. Returns bytes read (0 = EOF).
    fn read(&self, file_index: usize, offset: usize, buf: &mut [u8]) -> usize;
    fn file_info(&self, file_index: usize) -> Option<FileInfo>;
    fn file_count(&self) -> usize;
    /// Get info about the Nth file (for directory listing).
    fn file_at(&self, index: usize) -> Option<FileInfo>;
}
