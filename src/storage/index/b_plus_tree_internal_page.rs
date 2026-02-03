use crate::storage::index::b_plus_tree_page::{BPlusTreePage, IndexPageType, INDEX_PAGE_HEADER_SIZE};
use crate::storage::disk_manager::PageId;

// Key is i32 (4 bytes)
// Value is PageId (4 bytes)
// Entry Size = 8 bytes

const KEY_SIZE: usize = 4;
const PAGE_ID_SIZE: usize = 4;
const ENTRY_SIZE: usize = KEY_SIZE + PAGE_ID_SIZE;

pub struct BPlusTreeInternalPage<'a> {
    pub header: BPlusTreePage<'a>,
}

impl<'a> BPlusTreeInternalPage<'a> {
    pub fn new(header: BPlusTreePage<'a>) -> Self {
        Self { header }
    }

    pub fn init(&mut self, page_id: PageId, parent_id: PageId, max_size: u32) {
        self.header.set_page_type(IndexPageType::InternalPage);
        self.header.set_size(0);
        self.header.set_max_size(max_size);
        self.header.set_parent_page_id(parent_id);
        self.header.set_page_id(page_id);
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

    // Value Access (PageId)
    pub fn value_at(&self, index: u32) -> PageId {
        let offset = INDEX_PAGE_HEADER_SIZE + (index as usize * ENTRY_SIZE) + KEY_SIZE;
        u32::from_le_bytes(self.header.data[offset..offset+4].try_into().unwrap())
    }

    pub fn set_value_at(&mut self, index: u32, page_id: PageId) {
        let offset = INDEX_PAGE_HEADER_SIZE + (index as usize * ENTRY_SIZE) + KEY_SIZE;
        self.header.data[offset..offset+4].copy_from_slice(&page_id.to_le_bytes());
    }

    // Lookup: Find value associated with key.
    // Returns the PageId of the child that *might* contain the key.
    pub fn lookup(&self, key: i32) -> PageId {
        let size = self.header.get_size();
        // Keys: [X, 10, 20, 30]
        // Ptrs: [P0, P1, P2, P3]
        // If search 5: 5 < 10 -> P0.
        // If search 15: 10 <= 15 < 20 -> P1.
        // If search 35: 35 >= 30 -> P3.

        // We skip index 0 (invalid key). Start from 1.
        let mut left = 1;
        let mut right = size - 1;
        let mut result_idx = 0; // Default to P0

        // Binary Search
        while left <= right {
             let mid = (left + right) / 2;
             if self.key_at(mid) <= key {
                 result_idx = mid;
                 left = mid + 1;
             } else {
                 right = mid - 1;
             }
        }
        
        self.value_at(result_idx)
    }

    // Insert new entry (Key, PageId) after splitting child.
    // Usually inserts at correct sorted position.
    pub fn insert_node_after(&mut self, old_value: PageId, new_key: i32, new_value: PageId) {
        let size = self.header.get_size();
        let mut index = size; // Default append
        
        // Find index of old_value
        for i in 0..size {
            if self.value_at(i) == old_value {
                index = i + 1;
                break;
            }
        }

        // Shift
        for i in (index..size).rev() {
            self.set_key_at(i + 1, self.key_at(i));
            self.set_value_at(i + 1, self.value_at(i));
        }

        self.set_key_at(index, new_key);
        self.set_value_at(index, new_value);
        self.header.set_size(size + 1);
    }

    pub fn move_half_to(&mut self, recipient: &mut BPlusTreeInternalPage) {
        let size = self.header.get_size();
        let split_idx = size / 2;
        let num_to_move = size - split_idx;
        
        for i in 0..num_to_move {
            let src_idx = split_idx + i;
            let key = self.key_at(src_idx);
            let val = self.value_at(src_idx);
            
            recipient.set_key_at(i, key);
            recipient.set_value_at(i, val);
        }
        
        recipient.header.set_size(num_to_move);
        self.header.set_size(split_idx);
    }
}
