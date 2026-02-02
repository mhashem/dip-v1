use crate::storage::disk_manager::{DiskManager, PageId, PAGE_SIZE};
use crate::storage::page::{FrameId, Page};
use crate::storage::replacer::{LRUReplacer, Replacer};
use std::collections::HashMap;
use std::sync::Mutex;

const DEFAULT_BUFFER_POOL_SIZE: usize = 10;

pub struct BufferPoolManager {
    disk_manager: DiskManager,
    replacer: LRUReplacer,
    // The pool of pages. Index is FrameId.
    pages: Vec<Mutex<Page>>,
    // Map PageId -> FrameId
    page_table: Mutex<HashMap<PageId, FrameId>>,
    // List of free frames
    free_list: Mutex<Vec<FrameId>>,
}

impl BufferPoolManager {
    pub fn new(pool_size: usize, disk_manager: DiskManager) -> Self {
        let mut pages = Vec::with_capacity(pool_size);
        let mut free_list = Vec::with_capacity(pool_size);
        
        for i in 0..pool_size {
            pages.push(Mutex::new(Page::new()));
            free_list.push(i);
        }

        Self {
            disk_manager,
            replacer: LRUReplacer::new(pool_size),
            pages,
            page_table: Mutex::new(HashMap::new()),
            free_list: Mutex::new(free_list),
        }
    }

    /// Helper to find a victim frame:
    /// 1. Check free list.
    /// 2. If free list empty, ask replacer for victim.
    /// 3. If replacer gives victim, write it to disk if dirty.
    fn find_victim_frame(&mut self) -> Option<FrameId> {
        // 1. Check free list
        let mut free_list = self.free_list.lock().unwrap();
        if let Some(frame_id) = free_list.pop() {
            return Some(frame_id);
        }
        drop(free_list); // Release lock

        // 2. Replacer
        if let Some(frame_id) = self.replacer.victim() {
            let mut page = self.pages[frame_id].lock().unwrap();
            
            // If dirty, write back
            if page.is_dirty && page.id.is_some() {
                self.disk_manager.write_page(page.id.unwrap(), &page.data).unwrap();
            }

            // Remove from page table
            if let Some(pid) = page.id {
                let mut page_table = self.page_table.lock().unwrap();
                page_table.remove(&pid);
            }

            // Reset page metadata
            page.id = None;
            page.pin_count = 0;
            page.is_dirty = false;
            page.reset_memory();
            
            return Some(frame_id);
        }

        None
    }

    pub fn fetch_page(&mut self, page_id: PageId) -> Option<FrameId> {
        // 1. Check if page is already in buffer
        {
            let page_table = self.page_table.lock().unwrap();
            if let Some(&frame_id) = page_table.get(&page_id) {
                let mut page = self.pages[frame_id].lock().unwrap();
                page.pin_count += 1;
                self.replacer.pin(frame_id);
                return Some(frame_id);
            }
        }

        // 2. Not in buffer, need a frame
        let frame_id = self.find_victim_frame()?;

        // 3. Read page from disk
        let mut page = self.pages[frame_id].lock().unwrap();
        self.disk_manager.read_page(page_id, &mut page.data).ok()?;
        
        page.id = Some(page_id);
        page.pin_count = 1;
        page.is_dirty = false;

        // 4. Update page table
        let mut page_table = self.page_table.lock().unwrap();
        page_table.insert(page_id, frame_id);
        
        // 5. Pin in replacer
        self.replacer.pin(frame_id);

        Some(frame_id)
    }

    pub fn unpin_page(&self, page_id: PageId, is_dirty: bool) -> bool {
        let page_table = self.page_table.lock().unwrap();
        let frame_id = match page_table.get(&page_id) {
            Some(&fid) => fid,
            None => return false,
        };

        let mut page = self.pages[frame_id].lock().unwrap();
        if page.pin_count == 0 {
            return false;
        }

        page.pin_count -= 1;
        if is_dirty {
            page.is_dirty = true;
        }

        if page.pin_count == 0 {
            self.replacer.unpin(frame_id);
        }

        true
    }

    pub fn new_page(&mut self) -> Option<PageId> {
        let frame_id = self.find_victim_frame()?;
        let page_id = self.disk_manager.allocate_page();

        let mut page = self.pages[frame_id].lock().unwrap();
        page.id = Some(page_id);
        page.pin_count = 1;
        page.is_dirty = false; // New page is "clean" in the sense it doesn't need to be flushed *yet*? 
                               // Actually, it's empty memory. We probably want to write it eventually.
                               // But for now, it matches memory.

        let mut page_table = self.page_table.lock().unwrap();
        page_table.insert(page_id, frame_id);
        
        self.replacer.pin(frame_id);

        Some(page_id)
    }

    pub fn flush_page(&mut self, page_id: PageId) -> bool {
         let page_table = self.page_table.lock().unwrap();
        let frame_id = match page_table.get(&page_id) {
            Some(&fid) => fid,
            None => return false,
        };

        let mut page = self.pages[frame_id].lock().unwrap();
        if page.is_dirty {
            self.disk_manager.write_page(page_id, &page.data).unwrap();
            page.is_dirty = false;
        }
        
        true
    }

    pub fn flush_all(&mut self) {
        let page_table = self.page_table.lock().unwrap();
        // Clone keys to avoid deadlock if we were to lock pages inside the loop 
        // while holding page_table lock (though here it's fine as we don't call other BPM methods)
        let page_ids: Vec<PageId> = page_table.keys().cloned().collect();
        drop(page_table);

        for pid in page_ids {
            self.flush_page(pid);
        }
    }
    
    // Accessor to write data to a page (for testing/usage)
    // In a real impl, we'd return a guard/wrapper
    pub fn write_to_page(&self, page_id: PageId, data: &[u8]) {
        let page_table = self.page_table.lock().unwrap();
        if let Some(&frame_id) = page_table.get(&page_id) {
             let mut page = self.pages[frame_id].lock().unwrap();
             let len = data.len().min(PAGE_SIZE);
             page.data[0..len].copy_from_slice(&data[0..len]);
             page.is_dirty = true;
        }
    }
    
    // Accessor to read data (for testing)
    pub fn read_from_page(&self, page_id: PageId) -> Vec<u8> {
        let page_table = self.page_table.lock().unwrap();
        if let Some(&frame_id) = page_table.get(&page_id) {
             let page = self.pages[frame_id].lock().unwrap();
             return page.data.to_vec();
        }
        vec![]
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
        let mut bpm = BufferPoolManager::new(3, dm);

        let page_id1 = bpm.new_page().unwrap();
        let page_id2 = bpm.new_page().unwrap();
        let page_id3 = bpm.new_page().unwrap();

        // Pool is full now (size 3)
        assert_eq!(page_id1, 0);
        assert_eq!(page_id2, 1);
        assert_eq!(page_id3, 2);

        // We can't allocate a 4th page because all 3 are pinned (pin_count=1 from new_page)
        let page_id4 = bpm.new_page();
        assert!(page_id4.is_none());

        // Unpin page 1
        bpm.unpin_page(page_id1, false);
        
        // Now we can allocate
        let page_id4 = bpm.new_page();
        assert!(page_id4.is_some());
    }

    #[test]
    fn test_buffer_pool_manager_rw() {
        let temp_file = NamedTempFile::new().unwrap();
        let dm = DiskManager::new(temp_file.path()).unwrap();
        let mut bpm = BufferPoolManager::new(2, dm); // Small pool

        let pid1 = bpm.new_page().unwrap();
        let data = b"Hello";
        bpm.write_to_page(pid1, data);
        
        // Read back from memory
        let read = bpm.read_from_page(pid1);
        assert_eq!(&read[0..5], data);
        
        // Unpin and mark dirty
        bpm.unpin_page(pid1, true);
        
        // Create new pages to force eviction of pid1
        let pid2 = bpm.new_page().unwrap();
        bpm.unpin_page(pid2, false);
        let _pid3 = bpm.new_page().unwrap(); // Should evict pid1 (LRU)
        
        // Fetch pid1 again -> should come from disk
        // Since we evicted it dirty, it should have written "Hello" to disk
        let _frame_id = bpm.fetch_page(pid1).unwrap();
        let read_back = bpm.read_from_page(pid1);
        assert_eq!(&read_back[0..5], data);
    }
}
