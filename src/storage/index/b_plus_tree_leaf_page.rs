use crate::storage::disk_manager::PageId;
use crate::storage::index::b_plus_tree_page::{BPlusTreePage, IndexPageType, INDEX_PAGE_HEADER_SIZE};
use crate::storage::table::rid::RID;

// Key is i32 (4 bytes)
// RID is u64 equivalent (PageId u32 + Slot u32) -> 8 bytes
// Entry Size = 4 + 8 = 12 bytes

const KEY_SIZE: usize = 4;
const RID_SIZE: usize = 8;
const ENTRY_SIZE: usize = KEY_SIZE + RID_SIZE;

pub struct BPlusTreeLeafPage<'a> {
    pub header: BPlusTreePage<'a>,
}

impl<'a> BPlusTreeLeafPage<'a> {
    pub fn new(header: BPlusTreePage<'a>) -> Self {
        Self { header }
    }

    pub fn init(&mut self, page_id: PageId, parent_id: PageId, max_size: u32) {
        self.header.set_page_type(IndexPageType::LeafPage);
        self.header.set_size(0);
        self.header.set_max_size(max_size);
        self.header.set_parent_page_id(parent_id);
        self.header.set_page_id(page_id);
        self.set_next_page_id(0);
    }

    pub fn get_next_page_id(&self) -> PageId {
        u32::from_le_bytes(self.header.data[20..24].try_into().unwrap())
    }

    pub fn set_next_page_id(&mut self, next_page_id: PageId) {
        self.header.data[20..24].copy_from_slice(&next_page_id.to_le_bytes());
    }

    // Key Access
    pub fn key_at(&self, index: u32) -> i32 {
        let offset = INDEX_PAGE_HEADER_SIZE + (index as usize * ENTRY_SIZE);
        i32::from_le_bytes(self.header.data[offset..offset+4].try_into().unwrap())
    }

    pub fn set_key_at(&mut self, index: u32, key: i32) {
        let offset = INDEX_PAGE_HEADER_SIZE + (index as usize * ENTRY_SIZE);
        self.header.data[offset..offset+4].copy_from_slice(&key.to_le_bytes());
    }

    // Value Access (RID)
    pub fn value_at(&self, index: u32) -> RID {
        let offset = INDEX_PAGE_HEADER_SIZE + (index as usize * ENTRY_SIZE) + KEY_SIZE;
        let page_id = u32::from_le_bytes(self.header.data[offset..offset+4].try_into().unwrap());
        let slot_num = u32::from_le_bytes(self.header.data[offset+4..offset+8].try_into().unwrap());
        RID::new(page_id, slot_num)
    }

    pub fn set_value_at(&mut self, index: u32, rid: RID) {
        let offset = INDEX_PAGE_HEADER_SIZE + (index as usize * ENTRY_SIZE) + KEY_SIZE;
        self.header.data[offset..offset+4].copy_from_slice(&rid.page_id.to_le_bytes());
        self.header.data[offset+4..offset+8].copy_from_slice(&rid.slot_num.to_le_bytes());
    }

    // Insert (Assume free space exists, returns current size)
    // Returns index where inserted.
    pub fn insert(&mut self, key: i32, value: RID) -> u32 {
        let size = self.header.get_size();
        
        // Find position to insert (Sorted)
        // Simple linear search for now (Binary search better later)
        let mut index = 0;
        while index < size && self.key_at(index) < key {
            index += 1;
        }

        // Shift elements
        for i in (index..size).rev() {
            let k = self.key_at(i);
            let v = self.value_at(i);
            self.set_key_at(i + 1, k);
            self.set_value_at(i + 1, v);
        }

        self.set_key_at(index, key);
        self.set_value_at(index, value);
        self.header.set_size(size + 1);
        
        index
    }
    
    // Split logic requires copying data to another page.
    // That involves buffer pool interaction. The Page struct helper works on slice.
    // So we can implement `move_half_to(&mut other_leaf)`
    pub fn move_half_to(&mut self, recipient: &mut BPlusTreeLeafPage) {
        let size = self.header.get_size();
        let split_idx = size / 2;
        let num_to_move = size - split_idx;
        
        // Copy entries
        for i in 0..num_to_move {
            let src_idx = split_idx + i;
            let key = self.key_at(src_idx);
            let val = self.value_at(src_idx);
            
            // Recipient insert (append usually)
            recipient.set_key_at(i, key);
            recipient.set_value_at(i, val);
        }
        
        recipient.header.set_size(num_to_move);
        self.header.set_size(split_idx);
    }
}
