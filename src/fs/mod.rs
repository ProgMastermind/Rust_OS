// VFS trait and file descriptor types.

pub mod initrd;

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

pub trait FileSystem {
    fn open(&self, path: &str) -> Option<usize>;
    fn read(&self, file_index: usize, offset: usize, buf: &mut [u8]) -> usize;
    fn file_info(&self, file_index: usize) -> Option<FileInfo>;
    fn file_count(&self) -> usize;
    fn file_at(&self, index: usize) -> Option<FileInfo>;
}
