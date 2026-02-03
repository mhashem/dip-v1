use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::catalog::column::Column;
use dip_v1::catalog::schema::Schema;
use dip_v1::concurrency::transaction_manager::TransactionManager;
use dip_v1::execution::executor::{Executor, ExecutorContext};
use dip_v1::execution::insert::InsertExecutor;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::types::{TypeId, Value};
use std::fs;
use std::sync::Arc;

#[test]
fn test_metadata_persistence() {
    let db_path = std::env::temp_dir().join("dip_persist.db");
    let meta_path = std::env::temp_dir().join("dip_persist.meta");
    
    if db_path.exists() { fs::remove_file(&db_path).unwrap(); }
    if meta_path.exists() { fs::remove_file(&meta_path).unwrap(); }

    // 1. Create DB, Table, Insert Data, Check Stats
    {
        let dm = DiskManager::new(&db_path).unwrap();
        let bpm = Arc::new(BufferPoolManager::new(50, dm));
        let mut catalog = CatalogManager::new(bpm);

        let schema = Schema::new(vec![
            Column::new("val", TypeId::Integer),
        ]);
        
        let table_meta = catalog.create_table("t1".to_string(), schema);
        
        // Insert 0 and 100 to generate stats
        let txn_mgr = TransactionManager::new();
        let txn = txn_mgr.begin();
        
        let context = ExecutorContext { 
            catalog: table_meta.clone(),
            txn: txn.clone(),
            lock_manager: txn_mgr.lock_manager.clone(),
        };
        let values = vec![
            vec![Value::Integer(0)],
            vec![Value::Integer(100)],
        ];
        
        let mut insert = InsertExecutor::new(&context, values);
        insert.init();
        while insert.next().is_some() {}
        
        txn_mgr.commit(txn);
        
        // Check Stats exist in memory
        let stats = table_meta.page_stats.read().unwrap();
        assert!(!stats.is_empty());
        
        // Save Metadata
        catalog.save_metadata(&meta_path).expect("Failed to save metadata");
    }

    // 2. Restart (New Catalog, Load Metadata)
    {
        let dm = DiskManager::new(&db_path).unwrap();
        let bpm = Arc::new(BufferPoolManager::new(50, dm));
        let mut catalog = CatalogManager::new(bpm);
        
        // Load
        catalog.load_metadata(&meta_path).expect("Failed to load metadata");
        
        // Check Table Exists
        let table_meta = catalog.get_table("t1").expect("Table t1 should exist after load");
        assert_eq!(table_meta.name, "t1");
        assert_eq!(table_meta.schema.column_count(), 1);
        
        // Check Stats Restored
        let stats = table_meta.page_stats.read().unwrap();
        assert!(!stats.is_empty());
        
        // Check contents (Optional, but good sanity check)
        // Accessing stats for the first page (likely page 0)
        // But first page is header? No, TableHeap allocates first page as data page (slotted).
        // Wait, TableHeap::new allocates "first_page_id".
        // Let's iterate stats.
        for (_, p_stat) in stats.iter() {
            let col_stat = p_stat.columns.get(&0).unwrap();
            // We inserted 0 and 100. Min should be <= 0, Max >= 100.
            assert!(col_stat.min <= Value::Integer(0));
            assert!(col_stat.max >= Value::Integer(100));
        }
    }
}
