use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::index::b_plus_tree::BPlusTree;
use dip_v1::storage::table::rid::RID;
use std::sync::{Arc, Mutex};
use std::fs;

#[test]
fn test_b_plus_tree_insert_search() {
    let file_path = std::env::temp_dir().join("dip_btree.db");
    if file_path.exists() { fs::remove_file(&file_path).unwrap(); }

    let dm = DiskManager::new(&file_path).unwrap();
    let bpm = Arc::new(Mutex::new(BufferPoolManager::new(50, dm)));
    
    let mut tree = BPlusTree::new(bpm.clone());
    
    // 1. Insert small number
    for i in 0..10 {
        let rid = RID::new(i as u32, i as u32);
        tree.insert(i, rid);
    }
    
    // 2. Search
    for i in 0..10 {
        let rid = tree.get_value(i).expect("Key not found");
        assert_eq!(rid.page_id, i as u32);
    }
    
    // 3. Insert enough to cause splits
    // Max size is large, so let's insert 500 more.
    for i in 10..500 {
        let rid = RID::new(i as u32, i as u32);
        tree.insert(i, rid);
    }
    
    // 4. Verify all
    for i in 0..500 {
        let rid = tree.get_value(i).expect("Key not found after split");
        assert_eq!(rid.page_id, i as u32);
    }
}
