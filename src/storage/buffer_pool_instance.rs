use crate::storage::disk_manager::{DiskManager, PageId, PAGE_SIZE};
use crate::storage::page::{FrameId, Page};
use crate::storage::replacer::{LRUReplacer, Replacer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct BufferPoolInstance {
    disk_manager: Arc<Mutex<DiskManager>>,
    replacer: LRUReplacer,
    pages: Vec<Mutex<Page>>,
    page_table: Mutex<HashMap<PageId, FrameId>>,
    free_list: Mutex<Vec<FrameId>>,
}

impl BufferPoolInstance {
    pub fn new(pool_size: usize, disk_manager: Arc<Mutex<DiskManager>>) -> Self {
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

    fn find_victim_frame(&mut self) -> Option<FrameId> {
        let mut free_list = self.free_list.lock().unwrap();
        if let Some(frame_id) = free_list.pop() {
            return Some(frame_id);
        }
        drop(free_list);

        if let Some(frame_id) = self.replacer.victim() {
            let mut page = self.pages[frame_id].lock().unwrap();
            
            if page.is_dirty && page.id.is_some() {
                // LOCK DISK MANAGER
                self.disk_manager.lock().unwrap().write_page(page.id.unwrap(), &page.data).unwrap();
            }

            if let Some(pid) = page.id {
                let mut page_table = self.page_table.lock().unwrap();
                page_table.remove(&pid);
            }

            page.id = None;
            page.pin_count = 0;
            page.is_dirty = false;
            page.reset_memory();
            
            return Some(frame_id);
        }

        None
    }

    pub fn fetch_page(&mut self, page_id: PageId) -> Option<FrameId> {
        {
            let page_table = self.page_table.lock().unwrap();
            if let Some(&frame_id) = page_table.get(&page_id) {
                let mut page = self.pages[frame_id].lock().unwrap();
                page.pin_count += 1;
                self.replacer.pin(frame_id);
                return Some(frame_id);
            }
        }

        let frame_id = self.find_victim_frame()?;

        let mut page = self.pages[frame_id].lock().unwrap();
        // LOCK DISK MANAGER
        self.disk_manager.lock().unwrap().read_page(page_id, &mut page.data).ok()?;
        
        page.id = Some(page_id);
        page.pin_count = 1;
        page.is_dirty = false;

        let mut page_table = self.page_table.lock().unwrap();
        page_table.insert(page_id, frame_id);
        
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

    /// Creates a new page IN THIS INSTANCE using a pre-allocated PageID.
    /// This differs from the original which allocated the ID internally.
    pub fn new_page(&mut self, page_id: PageId) -> Option<FrameId> {
        let frame_id = self.find_victim_frame()?;
        
        // We assume page_id was just allocated by the Manager globally.
        // We initialize the frame.

        let mut page = self.pages[frame_id].lock().unwrap();
        page.id = Some(page_id);
        page.pin_count = 1;
        page.is_dirty = false; 

        let mut page_table = self.page_table.lock().unwrap();
        page_table.insert(page_id, frame_id);
        
        self.replacer.pin(frame_id);

        Some(frame_id)
    }

    pub fn flush_page(&mut self, page_id: PageId) -> bool {
         let page_table = self.page_table.lock().unwrap();
        let frame_id = match page_table.get(&page_id) {
            Some(&fid) => fid,
            None => return false,
        };

        let mut page = self.pages[frame_id].lock().unwrap();
        if page.is_dirty {
            // LOCK DISK MANAGER
            self.disk_manager.lock().unwrap().write_page(page_id, &page.data).unwrap();
            page.is_dirty = false;
        }
        
        true
    }

    pub fn flush_all(&mut self) {
        let page_table = self.page_table.lock().unwrap();
        let page_ids: Vec<PageId> = page_table.keys().cloned().collect();
        drop(page_table);

        for pid in page_ids {
            self.flush_page(pid);
        }
    }
    
    pub fn write_to_page(&self, page_id: PageId, data: &[u8]) {
        let page_table = self.page_table.lock().unwrap();
        if let Some(&frame_id) = page_table.get(&page_id) {
             let mut page = self.pages[frame_id].lock().unwrap();
             let len = data.len().min(PAGE_SIZE);
             page.data[0..len].copy_from_slice(&data[0..len]);
             page.is_dirty = true;
        }
    }
    
    pub fn read_from_page(&self, page_id: PageId) -> Vec<u8> {
        let page_table = self.page_table.lock().unwrap();
        if let Some(&frame_id) = page_table.get(&page_id) {
             let page = self.pages[frame_id].lock().unwrap();
             return page.data.to_vec();
        }
        vec![]
    }
}
