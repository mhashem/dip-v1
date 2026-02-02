use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// PAGE_SIZE is the unit of data transfer between disk and memory.
/// 4KB is a standard size for OS pages and SSD blocks.
pub const PAGE_SIZE: usize = 4096;

/// PageId represents a unique identifier for a page in the database file.
pub type PageId = u32;

/// DiskManager is responsible for the low-level interactions with the file system.
/// It treats the database file as an array of fixed-size pages.
pub struct DiskManager {
    file: File,
    next_page_id: PageId,
}

impl DiskManager {
    /// Opens a database file or creates it if it doesn't exist.
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;

        let metadata = file.metadata()?;
        let next_page_id = (metadata.len() / PAGE_SIZE as u64) as PageId;

        Ok(Self {
            file,
            next_page_id,
        })
    }

    /// Writes the content of a page to the disk.
    /// The offset is calculated as page_id * PAGE_SIZE.
    pub fn write_page(&mut self, page_id: PageId, data: &[u8]) -> io::Result<()> {
        if data.len() != PAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Page data must be exactly {} bytes, got {}",
                    PAGE_SIZE,
                    data.len()
                ),
            ));
        }

        let offset = page_id as u64 * PAGE_SIZE as u64;
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(data)?;
        self.file.sync_all()?; // Ensure data is flushed to physical disk
        
        Ok(())
    }

    /// Reads a page from disk into the provided buffer.
    pub fn read_page(&mut self, page_id: PageId, data: &mut [u8]) -> io::Result<()> {
        if data.len() != PAGE_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Buffer must be exactly {} bytes, got {}",
                    PAGE_SIZE,
                    data.len()
                ),
            ));
        }

        let offset = page_id as u64 * PAGE_SIZE as u64;
        let file_len = self.file.metadata()?.len();

        if offset >= file_len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("PageId {} is out of bounds", page_id),
            ));
        }

        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(data)?;

        Ok(())
    }

    /// Allocates a new page ID.
    /// In a real system, this might involve tracking free pages. 
    /// For now, we just append to the end.
    pub fn allocate_page(&mut self) -> PageId {
        let page_id = self.next_page_id;
        self.next_page_id += 1;
        page_id
    }
    
    /// Returns the number of pages currently in the file
    pub fn num_pages(&self) -> u64 {
        self.next_page_id as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_write_read_page() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();
        
        let mut dm = DiskManager::new(path).unwrap();
        
        let page_id = dm.allocate_page();
        let mut data = [0u8; PAGE_SIZE];
        let test_str = b"Hello, Database World!";
        
        // Fill data with some content
        data[0..test_str.len()].copy_from_slice(test_str);
        
        // Write to disk
        dm.write_page(page_id, &data).unwrap();
        
        // Read back
        let mut read_buf = [0u8; PAGE_SIZE];
        dm.read_page(page_id, &mut read_buf).unwrap();
        
        assert_eq!(data, read_buf);
        assert_eq!(&read_buf[0..test_str.len()], test_str);
    }

    #[test]
    fn test_persistence() {
        let temp_file = NamedTempFile::new().unwrap();
        let _path = temp_file.path().to_owned(); // Keep path, let temp_file drop later? 
        // Actually NamedTempFile deletes on drop. 
        // We can reuse the path if we keep the NamedTempFile in scope or use a standard file for this test.
        // Let's use a explicit file path in a temp dir for persistence test across instances.
        
        let file_path = std::env::temp_dir().join("test_persistence.db");
        // Clean up previous run
        let _ = std::fs::remove_file(&file_path);

        {
            let mut dm = DiskManager::new(&file_path).unwrap();
            let pid = dm.allocate_page();
            let mut data = [0u8; PAGE_SIZE];
            data[0] = 55;
            dm.write_page(pid, &data).unwrap();
        }

        {
            // Re-open
            let mut dm = DiskManager::new(&file_path).unwrap();
            let mut buffer = [0u8; PAGE_SIZE];
            dm.read_page(0, &mut buffer).unwrap();
            assert_eq!(buffer[0], 55);
        }
        
        let _ = std::fs::remove_file(&file_path);
    }
}
