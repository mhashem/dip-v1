use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::catalog::column::Column;
use dip_v1::catalog::schema::Schema;
use dip_v1::concurrency::transaction_manager::TransactionManager;
use dip_v1::execution::executor::{Executor, ExecutorContext};
use dip_v1::execution::insert::InsertExecutor;
use dip_v1::execution::seq_scan::SeqScanExecutor;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::types::{TypeId, Value};
use std::sync::Arc;

#[test]
fn test_execution_engine() {
    let file_path = std::env::temp_dir().join("dip_exec_engine.db");
    if file_path.exists() {
        std::fs::remove_file(&file_path).unwrap();
    }

    let dm = DiskManager::new(&file_path).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(10, dm));
    let mut catalog = CatalogManager::new(bpm);

    let schema = Schema::new(vec![
        Column::new("id", TypeId::Integer),
        Column::new("name", TypeId::Varchar),
    ]);
    
    let table_meta = catalog.create_table("users".to_string(), schema);

    // Dummy Txn
    let txn_mgr = TransactionManager::new();
    let txn = txn_mgr.begin();

    // 1. Prepare Data
    let values_batch = vec![
        vec![Value::Integer(1), Value::Varchar("Alice".to_string())],
        vec![Value::Integer(2), Value::Varchar("Bob".to_string())],
    ];

    // 2. Execute Insert
    let context = ExecutorContext { 
        catalog: table_meta.clone(),
        txn: txn.clone(),
        lock_manager: txn_mgr.lock_manager.clone(),
    };
    let mut insert_exec = InsertExecutor::new(&context, values_batch);
    insert_exec.init();
    
    let t1 = insert_exec.next();
    let t2 = insert_exec.next();
    let t3 = insert_exec.next();
    
    assert!(t1.is_some());
    assert!(t2.is_some());
    assert!(t3.is_none());

    // 3. Execute SeqScan
    let mut scan_exec = SeqScanExecutor::new(&context);
    scan_exec.init();
    
    // Fetch Row 1
    let row1 = scan_exec.next().expect("Should have row 1");
    let val1_id = row1.get_value(&table_meta.schema, 0);
    let val1_name = row1.get_value(&table_meta.schema, 1);
    assert_eq!(val1_id, Value::Integer(1));
    assert_eq!(val1_name, Value::Varchar("Alice".to_string()));
    
    // Fetch Row 2
    let row2 = scan_exec.next().expect("Should have row 2");
    let val2_id = row2.get_value(&table_meta.schema, 0);
    let val2_name = row2.get_value(&table_meta.schema, 1);
    assert_eq!(val2_id, Value::Integer(2));
    assert_eq!(val2_name, Value::Varchar("Bob".to_string()));

    // End of Scan
    assert!(scan_exec.next().is_none());
}
