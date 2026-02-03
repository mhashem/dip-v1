use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::catalog::column::Column;
use dip_v1::catalog::schema::Schema;
use dip_v1::concurrency::transaction_manager::TransactionManager;
use dip_v1::execution::executor::{Executor, ExecutorContext};
use dip_v1::execution::expression::Expression;
use dip_v1::execution::insert::InsertExecutor;
use dip_v1::execution::seq_scan::SeqScanExecutor;
use dip_v1::execution::update::UpdateExecutor;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::types::{TypeId, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[test]
fn test_update_executor() {
    let temp_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(temp_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(10, dm));
    let mut catalog = CatalogManager::new(bpm);
    let txn_mgr = Arc::new(TransactionManager::new());

    let schema = Schema::new(vec![
        Column::new("id", TypeId::Integer),
        Column::new("score", TypeId::Integer),
    ]);
    let table_meta = catalog.create_table("scores".to_string(), schema);

    // 1. Insert (id=1, score=10)
    let txn = txn_mgr.begin();
    let ctx = ExecutorContext { 
        catalog: table_meta.clone(), 
        txn: txn.clone(), 
        lock_manager: txn_mgr.lock_manager.clone() 
    };
    
    let values = vec![vec![Value::Integer(1), Value::Integer(10)]];
    let mut insert = InsertExecutor::new(&ctx, values);
    insert.init();
    insert.next();
    txn_mgr.commit(txn);

    // 2. Update: SET score = 20 WHERE id = 1
    // We simulate "WHERE id=1" by using a SeqScan (which will find the row).
    // We assign column 1 (score) = Constant(20).
    
    let txn2 = txn_mgr.begin();
    let ctx2 = ExecutorContext { 
        catalog: table_meta.clone(), 
        txn: txn2.clone(), 
        lock_manager: txn_mgr.lock_manager.clone() 
    };

    let mut scan = SeqScanExecutor::new(&ctx2);
    // (Predicate technically goes here, but we only have 1 row, so scan finds it)
    scan.init();

    let mut assignments = HashMap::new();
    assignments.insert(1, Expression::Constant(Value::Integer(20)));

    let mut update = UpdateExecutor::new(&ctx2, Box::new(scan), assignments);
    update.init();
    
    // !!! THIS WILL PANIC UNTIL YOU IMPLEMENT IT !!!
    let updated_tuple = update.next().expect("Should update one row");
    
    // Verify result immediately returned
    assert_eq!(updated_tuple.get_value(&table_meta.schema, 1), Value::Integer(20));
    
    txn_mgr.commit(txn2);

    // 3. Verify Persistence (Scan again)
    let txn3 = txn_mgr.begin();
    let ctx3 = ExecutorContext { 
        catalog: table_meta.clone(), 
        txn: txn3.clone(), 
        lock_manager: txn_mgr.lock_manager.clone() 
    };
    let mut verify_scan = SeqScanExecutor::new(&ctx3);
    verify_scan.init();
    
    let row = verify_scan.next().expect("Should have the updated row");
    assert_eq!(row.get_value(&table_meta.schema, 1), Value::Integer(20));
    
    // Should be only 1 row (old one deleted)
    assert!(verify_scan.next().is_none());
    
    txn_mgr.commit(txn3);
}
