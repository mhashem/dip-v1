use crate::storage::buffer_pool_manager::BufferPoolManager;
use crate::storage::disk_manager::{PageId, PAGE_SIZE};
use crate::storage::index::b_plus_tree_internal_page::BPlusTreeInternalPage;
use crate::storage::index::b_plus_tree_leaf_page::BPlusTreeLeafPage;
use crate::storage::index::b_plus_tree_page::{BPlusTreePage, INDEX_PAGE_HEADER_SIZE};
use crate::storage::table::rid::RID;
use std::sync::Arc;

// Constants
const LEAF_ENTRY_SIZE: usize = 12;
const INTERNAL_ENTRY_SIZE: usize = 8;
const LEAF_MAX_SIZE: u32 = (PAGE_SIZE - INDEX_PAGE_HEADER_SIZE) as u32 / LEAF_ENTRY_SIZE as u32 - 1; 
const INTERNAL_MAX_SIZE: u32 = (PAGE_SIZE - INDEX_PAGE_HEADER_SIZE) as u32 / INTERNAL_ENTRY_SIZE as u32 - 1;

pub struct BPlusTree {
    bpm: Arc<BufferPoolManager>,
    root_page_id: PageId,
}

impl BPlusTree {
    pub fn new(bpm: Arc<BufferPoolManager>) -> Self {
        let root_page_id = bpm.new_page().expect("Failed to allocate root");
        
        let _frame_id = bpm.fetch_page(root_page_id).expect("Failed to fetch root");
        let page_data = bpm.read_from_page(root_page_id);
        
        // Init as Leaf
        let mut temp_page = crate::storage::page::Page::new();
        temp_page.data.copy_from_slice(&page_data);
        let header = BPlusTreePage::new(&mut temp_page.data);
        let mut leaf = BPlusTreeLeafPage::new(header);
        leaf.init(root_page_id, 0, LEAF_MAX_SIZE); // 0 as invalid parent
        
        bpm.write_to_page(root_page_id, &temp_page.data);
        bpm.unpin_page(root_page_id, true);

        Self {
            bpm,
            root_page_id,
        }
    }
    
    pub fn from_root_page_id(bpm: Arc<BufferPoolManager>, root_page_id: PageId) -> Self {
        Self { bpm, root_page_id }
    }

    pub fn get_root_page_id(&self) -> PageId {
        self.root_page_id
    }

    pub fn is_empty(&self) -> bool {
        if self.bpm.fetch_page(self.root_page_id).is_none() { return true; }
        
        let page_data = self.bpm.read_from_page(self.root_page_id);
        let mut temp_page = crate::storage::page::Page::new();
        temp_page.data.copy_from_slice(&page_data);
        let header = BPlusTreePage::new(&mut temp_page.data);
        
        let size = header.get_size();
        self.bpm.unpin_page(self.root_page_id, false);
        
        size == 0
    }

    pub fn get_value(&self, key: i32) -> Option<RID> {
        let mut current_page_id = self.root_page_id;
        
        loop {
            if self.bpm.fetch_page(current_page_id).is_none() { return None; }
            let page_data = self.bpm.read_from_page(current_page_id);
            let mut temp_page = crate::storage::page::Page::new();
            temp_page.data.copy_from_slice(&page_data);
            let header = BPlusTreePage::new(&mut temp_page.data);
            
            if header.is_leaf_page() {
                let leaf = BPlusTreeLeafPage::new(header);
                // Search in leaf
                let size = leaf.header.get_size();
                // Simple linear search
                for i in 0..size {
                    if leaf.key_at(i) == key {
                        let rid = leaf.value_at(i);
                        self.bpm.unpin_page(current_page_id, false);
                        return Some(rid);
                    }
                }
                self.bpm.unpin_page(current_page_id, false);
                return None;
            } else {
                // Internal Node
                let internal = BPlusTreeInternalPage::new(header);
                let next_page_id = internal.lookup(key);
                self.bpm.unpin_page(current_page_id, false);
                current_page_id = next_page_id;
            }
        }
    }

    pub fn insert(&mut self, key: i32, value: RID) -> bool {
        // 1. Find Leaf
        let leaf_page_id = self.find_leaf_page(key);
        
        let _ = self.bpm.fetch_page(leaf_page_id).unwrap();
        let page_data = self.bpm.read_from_page(leaf_page_id);
        let mut temp_page = crate::storage::page::Page::new();
        temp_page.data.copy_from_slice(&page_data);
        
        let header = BPlusTreePage::new(&mut temp_page.data);
        let mut leaf = BPlusTreeLeafPage::new(header);
        
        let size = leaf.header.get_size();
        
        // Check for duplicate
        for i in 0..size {
            if leaf.key_at(i) == key {
                self.bpm.unpin_page(leaf_page_id, false);
                return false;
            }
        }

        let max_size = leaf.header.get_max_size();
        
        if size < max_size {
            leaf.insert(key, value);
            drop(leaf);
            self.bpm.write_to_page(leaf_page_id, &temp_page.data);
            self.bpm.unpin_page(leaf_page_id, true);
            true
        } else {
            // Split Leaf
            // 1. New Page
            let new_page_id = self.bpm.new_page().unwrap();
            
            // Init New Page
            let mut new_temp_page = crate::storage::page::Page::new();
            let new_header = BPlusTreePage::new(&mut new_temp_page.data);
            let mut new_leaf = BPlusTreeLeafPage::new(new_header);
            new_leaf.init(new_page_id, leaf.header.get_parent_page_id(), max_size);
            
            // Hack: Insert into sorted vector, then redistribute.
            let mut entries = Vec::new();
            for i in 0..size {
                entries.push((leaf.key_at(i), leaf.value_at(i)));
            }
            // Add new
            entries.push((key, value));
            entries.sort_by_key(|k| k.0);
            
            let split_idx = entries.len() / 2;
            let parent_id = leaf.header.get_parent_page_id();
            
            // Rewrite Old Leaf
            leaf.header.set_size(0);
            for i in 0..split_idx {
                leaf.insert(entries[i].0, entries[i].1);
            }
            // Update Next Page Id
            let old_next = leaf.get_next_page_id();
            leaf.set_next_page_id(new_page_id);
            
            // Write New Leaf
            new_leaf.set_next_page_id(old_next);
            for i in split_idx..entries.len() {
                new_leaf.insert(entries[i].0, entries[i].1);
            }
            
            let middle_key = new_leaf.key_at(0);
            
            // Drop wrappers to release borrow on temp_page.data
            drop(leaf);
            drop(new_leaf);

            self.bpm.write_to_page(leaf_page_id, &temp_page.data);
            self.bpm.write_to_page(new_page_id, &new_temp_page.data);
            
            self.bpm.unpin_page(leaf_page_id, true);
            self.bpm.unpin_page(new_page_id, true);
            
            self.insert_into_parent(leaf_page_id, middle_key, new_page_id, parent_id);
            true
        }
    }

    fn find_leaf_page(&self, key: i32) -> PageId {
        let mut current_page_id = self.root_page_id;
        
        loop {
            self.bpm.fetch_page(current_page_id).unwrap();
            let page_data = self.bpm.read_from_page(current_page_id);
            let mut temp_page = crate::storage::page::Page::new();
            temp_page.data.copy_from_slice(&page_data);
            let header = BPlusTreePage::new(&mut temp_page.data);
            
            if header.is_leaf_page() {
                self.bpm.unpin_page(current_page_id, false);
                return current_page_id;
            }
            
            let internal = BPlusTreeInternalPage::new(header);
            let next_page_id = internal.lookup(key);
            self.bpm.unpin_page(current_page_id, false);
            current_page_id = next_page_id;
        }
    }

    fn insert_into_parent(&mut self, left_child: PageId, key: i32, right_child: PageId, parent_id: PageId) {
        if parent_id == 0 {
            // New Root
            let new_root_id = self.bpm.new_page().unwrap();
            
            let _ = self.bpm.fetch_page(new_root_id).unwrap();
             // Init New Root (Internal)
            let mut temp_page = crate::storage::page::Page::new();
            let header = BPlusTreePage::new(&mut temp_page.data);
            let mut internal = BPlusTreeInternalPage::new(header);
            internal.init(new_root_id, 0, INTERNAL_MAX_SIZE);
            
            // Pointer 0 -> left_child (keys < key)
            // Pointer 1 -> right_child (keys >= key) associated with Key 1
            internal.set_value_at(0, left_child);
            internal.set_key_at(1, key);
            internal.set_value_at(1, right_child);
            internal.header.set_size(2);
            
            drop(internal);
            self.bpm.write_to_page(new_root_id, &temp_page.data);
            
            // Update children parents
            // Left
            let _ = self.bpm.fetch_page(left_child).unwrap();
            let l_data = self.bpm.read_from_page(left_child);
            let mut l_temp = crate::storage::page::Page::new();
            l_temp.data.copy_from_slice(&l_data);
            BPlusTreePage::new(&mut l_temp.data).set_parent_page_id(new_root_id);
            self.bpm.write_to_page(left_child, &l_temp.data);
            self.bpm.unpin_page(left_child, true);
            
            // Right
            let _ = self.bpm.fetch_page(right_child).unwrap();
            let r_data = self.bpm.read_from_page(right_child);
            let mut r_temp = crate::storage::page::Page::new();
            r_temp.data.copy_from_slice(&r_data);
            BPlusTreePage::new(&mut r_temp.data).set_parent_page_id(new_root_id);
            self.bpm.write_to_page(right_child, &r_temp.data);
            self.bpm.unpin_page(right_child, true);
            
            self.bpm.unpin_page(new_root_id, true);
            self.root_page_id = new_root_id;
            return;
        }
        
        let _ = self.bpm.fetch_page(parent_id).unwrap();
        let page_data = self.bpm.read_from_page(parent_id);
        let mut temp_page = crate::storage::page::Page::new();
        temp_page.data.copy_from_slice(&page_data);
        
        let header = BPlusTreePage::new(&mut temp_page.data);
        let mut internal = BPlusTreeInternalPage::new(header);
        
        if internal.header.get_size() < internal.header.get_max_size() {
            internal.insert_node_after(left_child, key, right_child);
            drop(internal);
            self.bpm.write_to_page(parent_id, &temp_page.data);
            self.bpm.unpin_page(parent_id, true);
        } else {
             // Split Internal
             let size = internal.header.get_size();
             let grandparent_id = internal.header.get_parent_page_id();
             let mut entries = Vec::new(); // (Key, Val)
             // Entry 0 has invalid key, but Val is P0.
             // We treat it as (Dummy, P0).
             for i in 0..size {
                 entries.push((internal.key_at(i), internal.value_at(i)));
             }
             
             // Find insertion point
             let mut insert_idx = size as usize;
             for i in 0..size {
                 if entries[i as usize].1 == left_child {
                     insert_idx = (i + 1) as usize;
                     break;
                 }
             }
             entries.insert(insert_idx, (key, right_child));
             
             let split_idx = entries.len() / 2;
             let up_key = entries[split_idx].0;
             
             // New Internal Page
             let new_page_id = self.bpm.new_page().unwrap();
             let mut new_temp_page = crate::storage::page::Page::new();
             let new_header = BPlusTreePage::new(&mut new_temp_page.data);
             let mut new_internal = BPlusTreeInternalPage::new(new_header);
             new_internal.init(new_page_id, grandparent_id, INTERNAL_MAX_SIZE);
             
             // Rewrite Old
             internal.header.set_size(0);
             for i in 0..split_idx {
                 internal.set_key_at(i as u32, entries[i].0);
                 internal.set_value_at(i as u32, entries[i].1);
             }
             internal.header.set_size(split_idx as u32);
             
             // Write New (Start from split_idx + 1 ? Ptr 0 of new page is Val[split_idx])
             // Keys > Up Key start at split_idx + 1
             
             // New Page P0 = entries[split_idx].1
             new_internal.set_value_at(0, entries[split_idx].1);
             new_internal.set_key_at(0, 0); // Dummy
             
             let mut new_idx = 1;
             for i in (split_idx + 1)..entries.len() {
                 new_internal.set_key_at(new_idx, entries[i].0);
                 new_internal.set_value_at(new_idx, entries[i].1);
                 
                 // Update children parent pointers!
                 let child_page_id = entries[i].1;
                 
                 if let Some(_frame) = self.bpm.fetch_page(child_page_id) {
                     let child_data = self.bpm.read_from_page(child_page_id);
                     let mut child_temp = crate::storage::page::Page::new();
                     child_temp.data.copy_from_slice(&child_data);
                     
                     BPlusTreePage::new(&mut child_temp.data).set_parent_page_id(new_page_id);
                     
                     self.bpm.write_to_page(child_page_id, &child_temp.data);
                     self.bpm.unpin_page(child_page_id, true);
                 }
                 
                 new_idx += 1;
             }
             new_internal.header.set_size(new_idx);
             
             drop(internal);
             drop(new_internal);

             self.bpm.write_to_page(parent_id, &temp_page.data);
             self.bpm.write_to_page(new_page_id, &new_temp_page.data);
             
             self.bpm.unpin_page(parent_id, true);
             self.bpm.unpin_page(new_page_id, true);
             
             self.insert_into_parent(parent_id, up_key, new_page_id, grandparent_id);
        }
    }
}