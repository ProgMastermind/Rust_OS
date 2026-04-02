// Virtual File System (VFS) + File Descriptor Types
//
// The VFS trait abstracts over different filesystem implementations.
// Right now we only have a ramdisk, but the kernel doesn't care —
// it talks to the VFS trait, which could be backed by FAT, ext2,
// ramdisk, or anything else.
//
// File descriptors are per-process integers that map to open files.
// The classic convention: fd 0 = stdin, fd 1 = stdout, fd 2 = stderr.

pub mod initrd;

// A file descriptor entry in a process's fd table.
// Each process has a Vec<Option<FdEntry>> where the index IS the fd number.
#[derive(Debug, Clone)]
pub enum FdEntry {
    Stdin,   // fd 0 — keyboard input (not yet implemented)
    Stdout,  // fd 1 — writes go to VGA screen
    Stderr,  // fd 2 — writes go to serial port
    File {
        file_index: usize, // Index into the ramdisk's file list
        position: usize,   // Current read offset within the file
    },
}

// Information about a file.
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: &'static str,
    pub size: usize,
}

// The VFS trait. Any filesystem implementation provides these methods.
pub trait FileSystem {
    // Look up a file by path. Returns an opaque file index (not an fd).
    fn open(&self, path: &str) -> Option<usize>;

    // Read from a file starting at `offset` into `buf`.
    // Returns the number of bytes actually read (may be less than buf.len()
    // if we hit end-of-file).
    fn read(&self, file_index: usize, offset: usize, buf: &mut [u8]) -> usize;

    // Get info about a file by index.
    fn file_info(&self, file_index: usize) -> Option<FileInfo>;

    // Return the number of files in the filesystem.
    fn file_count(&self) -> usize;

    // Get info about the Nth file (for directory listing).
    fn file_at(&self, index: usize) -> Option<FileInfo>;
}
