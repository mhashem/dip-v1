use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::catalog::schema::Schema;
use dip_v1::catalog::column::Column;
use dip_v1::types::{Value, TypeId};
use dip_v1::execution::executor::{Executor, ExecutorContext};
use dip_v1::execution::insert::InsertExecutor;
use dip_v1::execution::seq_scan::SeqScanExecutor;
use dip_v1::execution::expression::{Expression, BinaryOperator};
use dip_v1::concurrency::transaction_manager::TransactionManager;
use std::sync::Arc;

#[test]
fn test_zonemap_pruning_infinite_loop() {
    let file_path = std::env::temp_dir().join("dip_zonemap_test.db");
    if file_path.exists() {
        std::fs::remove_file(&file_path).unwrap();
    }

    let dm = DiskManager::new(&file_path).unwrap();
    // Use small buffer pool to force eviction if needed, though not strictly necessary here
    let bpm = Arc::new(BufferPoolManager::new(50, dm));
    let mut catalog = CatalogManager::new(bpm);

    let schema = Schema::new(vec![
        Column::new("id", TypeId::Integer),
    ]);
    
    let table_meta = catalog.create_table("items".to_string(), schema);
    
    // Dummy Txn
    let txn_mgr = TransactionManager::new();
    let txn = txn_mgr.begin();
    
    let context = ExecutorContext { 
        catalog: table_meta.clone(),
        txn: txn.clone(),
        lock_manager: txn_mgr.lock_manager.clone(),
    };

    // 1. Insert enough data to create multiple pages.
    // Assuming Page Size 4096. Tuple overhead ~something. 
    // Integer is 4 bytes + tuple header.
    // Let's insert 2000 items. Should definitely span multiple pages.
    let mut values_batch = Vec::new();
    for i in 0..2000 {
        values_batch.push(vec![Value::Integer(i)]);
    }

    let mut insert_exec = InsertExecutor::new(&context, values_batch);
    insert_exec.init();
    
    let mut count = 0;
    while insert_exec.next().is_some() {
        count += 1;
    }
    assert_eq!(count, 2000);
    
    // Verify we have multiple pages
    let table_iter = table_meta.table.iter();
    // We can't easily check page count via iterator without scanning.
    // But we trust TableHeap allocated new pages.
    
    println!("Insertion complete.");

    // 2. Query that matches only the FIRST part of the data.
    // e.g. id < 100.
    // This should allow page 1 (min=0, max=~something).
    // And REJECT later pages (min=~500, max=~1000).
    // Specifically, we want to ensure it rejects the LAST page and terminates.
    
    let predicate = Expression::Binary {
        left: Box::new(Expression::Column(0)), // id
        op: BinaryOperator::Lt,
        right: Box::new(Expression::Constant(Value::Integer(100))),
    };

    let mut scan_exec = SeqScanExecutor::new(&context);
    scan_exec.set_predicate(predicate);
    scan_exec.init();

    let mut match_count = 0;
    loop {
        // Use a timeout or max iterations to avoid hanging the test runner indefinitely if bug persists?
        // Rust tests usually handle hangs by eventually timing out, but in CLI it might be annoying.
        // We'll trust the fix.
        if let Some(tuple) = scan_exec.next() {
            let val = tuple.get_value(&table_meta.schema, 0);
            match val {
                Value::Integer(v) => {
                    assert!(v < 100, "Value {} should be < 100", v);
                }
                _ => panic!("Unexpected type"),
            }
            match_count += 1;
        } else {
            break;
        }
    }
    
    println!("Matched {} rows", match_count);
    assert_eq!(match_count, 100); 
}
