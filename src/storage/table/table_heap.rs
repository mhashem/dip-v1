use crate::storage::buffer_pool_manager::BufferPoolManager;
use crate::storage::disk_manager::PageId;
use crate::storage::table::rid::RID;
use crate::storage::table::slotted_page::SlottedPage;
use crate::storage::tuple::Tuple;
use std::sync::{Arc, Mutex};

pub struct TableHeap {
    bpm: Arc<Mutex<BufferPoolManager>>,
    first_page_id: PageId,
}

impl TableHeap {
    /// Creates a new table heap. 
    /// Allocates the first page immediately.
    pub fn new(bpm: Arc<Mutex<BufferPoolManager>>) -> Self {
        let mut bpm_guard = bpm.lock().unwrap();
        let first_page_id = bpm_guard.new_page().expect("Failed to allocate first page for table heap");
        
        // Initialize the new page as a slotted page
        let _frame_id = bpm_guard.fetch_page(first_page_id).expect("Failed to fetch new page");
        
        // Since we are inside the BPM lock logic roughly (or owning it),
        // we can't easily get a Page reference without dealing with the BPM internal mutexes 
        // if we didn't expose them properly.
        // But wait, `fetch_page` returns a FrameId.
        // And `write_to_page` uses FrameId/PageId.
        
        // Let's rely on `fetch_page` which pins it. 
        // But `write_to_page` is a helper that blindly writes bytes. 
        // We need `SlottedPage` logic.
        
        // To properly initialize, we need to read, modify, write.
        // Let's assume for this simple version we can read the raw bytes, modify with SlottedPage, and write back.
        let page_data = bpm_guard.read_from_page(first_page_id);
        
        // Create a temporary Page wrapper to use SlottedPage logic
        // This is a bit inefficient (copying) but avoids complex locking refactoring for now.
        let mut temp_page = crate::storage::page::Page::new();
        temp_page.data.copy_from_slice(&page_data);
        
        let mut slotted = SlottedPage::new(&mut temp_page);
        slotted.init(0); // 0 as invalid next page
        
        bpm_guard.write_to_page(first_page_id, &temp_page.data);
        bpm_guard.unpin_page(first_page_id, true);
        
        drop(bpm_guard);

        Self {
            bpm,
            first_page_id,
        }
    }

    pub fn from_first_page_id(bpm: Arc<Mutex<BufferPoolManager>>, first_page_id: PageId) -> Self {
        Self {
            bpm,
            first_page_id,
        }
    }



    pub fn insert_tuple(&self, tuple: &Tuple) -> Option<RID> {
        let mut bpm = self.bpm.lock().unwrap();
        let mut current_page_id = self.first_page_id;

        loop {
            let _frame_id = bpm.fetch_page(current_page_id)?;
            
            let page_data = bpm.read_from_page(current_page_id);
            let mut temp_page = crate::storage::page::Page::new();
            temp_page.data.copy_from_slice(&page_data);
            let mut slotted = SlottedPage::new(&mut temp_page);

            if let Some(slot_id) = slotted.insert_tuple(tuple) {
                bpm.write_to_page(current_page_id, &temp_page.data);
                bpm.unpin_page(current_page_id, true);
                return Some(RID::new(current_page_id, slot_id as u32));
            }

            // Page full, try next page
            let next_page_id = slotted.get_next_page_id();
            
            if next_page_id != 0 {
                // Move to next page
                bpm.unpin_page(current_page_id, false);
                current_page_id = next_page_id;
            } else {
                // No next page, allocate new one
                let new_page_id = bpm.new_page()?; // Pin count 1
                
                // Update current page's next_page_id
                slotted.set_next_page_id(new_page_id);
                bpm.write_to_page(current_page_id, &temp_page.data);
                bpm.unpin_page(current_page_id, true); // Finished with current
                
                // Initialize new page
                // Note: new_page_id is already pinned once. 
                // We fetch it again to be safe with our current logic? 
                // Better: Just use it directly since we know it's in the pool.
                let mut new_temp_page = crate::storage::page::Page::new();
                let mut new_slotted = SlottedPage::new(&mut new_temp_page);
                new_slotted.init(0);
                
                bpm.write_to_page(new_page_id, &new_temp_page.data);
                bpm.unpin_page(new_page_id, true); 
                
                current_page_id = new_page_id;
            }
        }
    }

    pub fn get_tuple(&self, rid: RID) -> Option<Tuple> {
        let mut bpm = self.bpm.lock().unwrap();
        let _ = bpm.fetch_page(rid.page_id)?;
        
        let page_data = bpm.read_from_page(rid.page_id);
        let mut temp_page = crate::storage::page::Page::new();
        temp_page.data.copy_from_slice(&page_data);
        let slotted = SlottedPage::new(&mut temp_page);
        
        let tuple = slotted.get_tuple(rid.slot_num as u16);
        
        bpm.unpin_page(rid.page_id, false);
        tuple
    }

    pub fn get_first_page_id(&self) -> PageId {
        self.first_page_id
    }

    pub fn iter(&self) -> TableIterator {
        TableIterator::new(self.bpm.clone(), self.first_page_id)
    }
}

pub struct TableIterator {
    bpm: Arc<Mutex<BufferPoolManager>>,
    current_page_id: PageId,
    current_slot_id: u32,
    finished: bool, // NEW
}

impl TableIterator {
    pub fn new(bpm: Arc<Mutex<BufferPoolManager>>, start_page_id: PageId) -> Self {
        Self {
            bpm,
            current_page_id: start_page_id,
            current_slot_id: 0,
            finished: false,
        }
    }
    
    pub fn get_current_page_id(&self) -> PageId {
        self.current_page_id
    }

    pub fn is_end(&self) -> bool {
        self.finished
    }

    pub fn skip_page(&mut self) {
        if self.finished { return; }
        
        let mut bpm = self.bpm.lock().unwrap();
        if bpm.fetch_page(self.current_page_id).is_some() {
            let page_data = bpm.read_from_page(self.current_page_id);
            // ... (read next_page_id logic)
            // Simplified for brevity:
             let mut temp_page = crate::storage::page::Page::new();
            temp_page.data.copy_from_slice(&page_data);
            let slotted = SlottedPage::new(&mut temp_page);
            let next_page_id = slotted.get_next_page_id();
            
            bpm.unpin_page(self.current_page_id, false);
            
            if next_page_id != 0 {
                self.current_page_id = next_page_id;
                self.current_slot_id = 0;
            } else {
                self.finished = true;
            }
        } else {
             self.finished = true;
        }
    }
}

impl Iterator for TableIterator {
    type Item = Tuple;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished { return None; }
        
        let mut bpm = self.bpm.lock().unwrap();
        
        loop {
            // Fetch current page
            if bpm.fetch_page(self.current_page_id).is_none() {
                return None; 
            }
            
            let page_data = bpm.read_from_page(self.current_page_id);
            let mut temp_page = crate::storage::page::Page::new();
            temp_page.data.copy_from_slice(&page_data);
            let slotted = SlottedPage::new(&mut temp_page);
            
            // Try to find a valid tuple in current page starting from current_slot_id
            let slot_count = slotted.get_slot_count() as u32;
            
            while self.current_slot_id < slot_count {
                let slot_id = self.current_slot_id;
                self.current_slot_id += 1;
                
                if let Some(tuple) = slotted.get_tuple(slot_id as u16) {
                    bpm.unpin_page(self.current_page_id, false);
                    return Some(tuple);
                }
                // If None (deleted/empty slot), continue loop
            }
            
            // End of page reached
            let next_page_id = slotted.get_next_page_id();
            bpm.unpin_page(self.current_page_id, false);
            
            if next_page_id == 0 {
                self.finished = true; // NEW
                return None; 
            }
            
            // Move to next page
            self.current_page_id = next_page_id;
            self.current_slot_id = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::disk_manager::DiskManager;
    use tempfile::NamedTempFile;

    #[test]
    fn test_table_heap() {
        let temp_file = NamedTempFile::new().unwrap();
        let dm = DiskManager::new(temp_file.path()).unwrap();
        let bpm = Arc::new(Mutex::new(BufferPoolManager::new(10, dm)));
        
        let table = TableHeap::new(bpm);
        
        let t1 = Tuple::new(b"Hello".to_vec());
        let t2 = Tuple::new(b"World".to_vec());
        
        let rid1 = table.insert_tuple(&t1).unwrap();
        let rid2 = table.insert_tuple(&t2).unwrap();
        
        let rt1 = table.get_tuple(rid1).unwrap();
        let rt2 = table.get_tuple(rid2).unwrap();
        
        assert_eq!(rt1.data, b"Hello");
        assert_eq!(rt2.data, b"World");
    }
}
