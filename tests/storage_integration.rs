use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::table::table_heap::TableHeap;
use dip_v1::storage::tuple::Tuple;
use std::sync::{Arc, Mutex};

#[test]
fn test_storage_engine_integration() {
    let file_path = std::env::temp_dir().join("dip_storage_integration.db");
    if file_path.exists() {
        std::fs::remove_file(&file_path).unwrap();
    }
    
    // 1. Setup: Sufficient buffer pool size
    let pool_size = 10; 
    let mut rids = Vec::new();
    let tuple_count = 500;

    {
        let dm = DiskManager::new(&file_path).unwrap();
        let bpm = Arc::new(Mutex::new(BufferPoolManager::new(pool_size, dm)));
        let table = TableHeap::new(bpm.clone());

        // 2. Insert many tuples (this will span multiple pages and cause evictions)
        for i in 0..tuple_count {
            let data = format!("Tuple-data-with-index-{}", i).into_bytes();
            let tuple = Tuple::new(data);
            let rid = table.insert_tuple(&tuple).expect("Insert should succeed");
            rids.push(rid);
        }
        
        // 3. Verify data in memory
        for (i, rid) in rids.iter().enumerate() {
            let expected_data = format!("Tuple-data-with-index-{}", i).into_bytes();
            let tuple = table.get_tuple(*rid).expect("Tuple should be found");
            assert_eq!(tuple.data, expected_data);
        }
        
        // Force flush all pages to disk before closing by letting bpm drop
        // Or we could implement a flush_all in BPM.
    }

    // 4. Persistence Check: Close and Re-open the database
    {
        let dm = DiskManager::new(&file_path).unwrap();
        let num_pages = dm.num_pages();
        assert!(num_pages > 1, "Should have spanned multiple pages, got {}", num_pages);

        let bpm = Arc::new(Mutex::new(BufferPoolManager::new(pool_size, dm)));
        let table = TableHeap::from_first_page_id(bpm, 0);

        // Verify all data is still there and correct
        for (i, rid) in rids.iter().enumerate() {
            let expected_data = format!("Tuple-data-with-index-{}", i).into_bytes();
            let tuple = table.get_tuple(*rid).expect("Tuple should be found after reload");
            assert_eq!(tuple.data, expected_data);
        }
    }
    
    std::fs::remove_file(&file_path).unwrap();
}