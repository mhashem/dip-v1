use crate::storage::disk_manager::{DiskManager, PageId};
use crate::storage::page::FrameId;
use crate::storage::buffer_pool_instance::BufferPoolInstance;
use std::sync::{Arc, Mutex};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const NUM_SHARDS: usize = 64;

pub struct BufferPoolManager {
    disk_manager: Arc<Mutex<DiskManager>>,
    shards: Vec<Mutex<BufferPoolInstance>>,
    num_shards: usize, 
}

impl BufferPoolManager {
    pub fn new(total_pool_size: usize, disk_manager: DiskManager) -> Self {
        let dm_arc = Arc::new(Mutex::new(disk_manager));
        let mut shards = Vec::with_capacity(NUM_SHARDS);
        let shard_size = total_pool_size / NUM_SHARDS;
        // Ensure at least 1 page per shard if pool is small
        let shard_size = if shard_size == 0 { 1 } else { shard_size };

        for _ in 0..NUM_SHARDS {
            shards.push(Mutex::new(BufferPoolInstance::new(shard_size, dm_arc.clone())));
        }

        Self {
            disk_manager: dm_arc,
            shards,
            num_shards: NUM_SHARDS,
        }
    }

    fn get_shard_idx(&self, page_id: PageId) -> usize {
        let mut hasher = DefaultHasher::new();
        page_id.hash(&mut hasher);
        (hasher.finish() as usize) % self.num_shards
    }

    pub fn fetch_page(&self, page_id: PageId) -> Option<FrameId> {
        let shard_idx = self.get_shard_idx(page_id);
        let mut shard = self.shards[shard_idx].lock().unwrap();
        shard.fetch_page(page_id)
    }

    pub fn unpin_page(&self, page_id: PageId, is_dirty: bool) -> bool {
        let shard_idx = self.get_shard_idx(page_id);
        let shard = self.shards[shard_idx].lock().unwrap();
        shard.unpin_page(page_id, is_dirty)
    }

    pub fn new_page(&self) -> Option<PageId> {
        // 1. Allocate PageID globally
        let page_id = self.disk_manager.lock().unwrap().allocate_page();
        
        // 2. Delegate to shard
        let shard_idx = self.get_shard_idx(page_id);
        let mut shard = self.shards[shard_idx].lock().unwrap();
        
        if shard.new_page(page_id).is_some() {
            Some(page_id)
        } else {
            // If shard is full, we fail. 
            // In a real system we would handle this better (global eviction or stealing).
            None
        }
    }

    pub fn flush_page(&self, page_id: PageId) -> bool {
        let shard_idx = self.get_shard_idx(page_id);
        let mut shard = self.shards[shard_idx].lock().unwrap();
        shard.flush_page(page_id)
    }

    pub fn flush_all(&self) {
        for shard in &self.shards {
            let mut s = shard.lock().unwrap();
            s.flush_all();
        }
    }
    
    pub fn write_to_page(&self, page_id: PageId, data: &[u8]) {
        let shard_idx = self.get_shard_idx(page_id);
        let shard = self.shards[shard_idx].lock().unwrap();
        shard.write_to_page(page_id, data);
    }
    
    pub fn read_from_page(&self, page_id: PageId) -> Vec<u8> {
        let shard_idx = self.get_shard_idx(page_id);
        let shard = self.shards[shard_idx].lock().unwrap();
        shard.read_from_page(page_id)
    }
}

impl Drop for BufferPoolManager {
    fn drop(&mut self) {
        self.flush_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_buffer_pool_manager_new_page() {
        let temp_file = NamedTempFile::new().unwrap();
        let dm = DiskManager::new(temp_file.path()).unwrap();
        // Use large enough pool so shards aren't empty (64 shards)
        let bpm = BufferPoolManager::new(128, dm); 

        let page_id1 = bpm.new_page().unwrap();
        let page_id2 = bpm.new_page().unwrap();
        
        assert_ne!(page_id1, page_id2);

        bpm.unpin_page(page_id1, false);
        bpm.unpin_page(page_id2, false);
    }

    #[test]
    fn test_buffer_pool_manager_rw() {
        let temp_file = NamedTempFile::new().unwrap();
        let dm = DiskManager::new(temp_file.path()).unwrap();
        let bpm = BufferPoolManager::new(128, dm);

        let pid1 = bpm.new_page().unwrap();
        let data = b"Hello";
        bpm.write_to_page(pid1, data);
        
        let read = bpm.read_from_page(pid1);
        assert_eq!(&read[0..5], data);
        
        bpm.unpin_page(pid1, true);
        
        // Fetch again
        let _ = bpm.fetch_page(pid1).unwrap();
        let read_back = bpm.read_from_page(pid1);
        assert_eq!(&read_back[0..5], data);
    }
}