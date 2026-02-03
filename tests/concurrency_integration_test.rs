use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::catalog::catalog_manager::{CatalogManager, TableMetadata};
use dip_v1::catalog::schema::Schema;
use dip_v1::catalog::column::Column;
use dip_v1::types::{Value, TypeId};
use dip_v1::execution::executor::{Executor, ExecutorContext};
use dip_v1::execution::insert::InsertExecutor;
use dip_v1::execution::seq_scan::SeqScanExecutor;
use dip_v1::concurrency::transaction_manager::TransactionManager;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::NamedTempFile;

// Helper to setup environment
struct TestEnv {
    catalog: CatalogManager,
    txn_mgr: Arc<TransactionManager>,
    table_meta: Arc<TableMetadata>,
}

fn setup() -> TestEnv {
    let temp_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(temp_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(50, dm));
    let mut catalog = CatalogManager::new(bpm);
    
    let schema = Schema::new(vec![Column::new("v", TypeId::Integer)]);
    let table_meta = catalog.create_table("t1".to_string(), schema);
    let txn_mgr = Arc::new(TransactionManager::new());

    TestEnv { catalog, txn_mgr, table_meta }
}

#[test]
fn test_shared_lock_concurrency() {
    let env = Arc::new(setup());
    
    // Insert one row (Txn 0)
    {
        let txn = env.txn_mgr.begin();
        let ctx = ExecutorContext { catalog: env.table_meta.clone(), txn: txn.clone(), lock_manager: env.txn_mgr.lock_manager.clone() };
        let values = vec![vec![Value::Integer(10)]];
        let mut exec = InsertExecutor::new(&ctx, values);
        exec.init();
        exec.next();
        env.txn_mgr.commit(txn);
    }

    // T1 and T2 read same row concurrently
    let env1 = env.clone();
    let env2 = env.clone();

    let h1 = thread::spawn(move || {
        let txn = env1.txn_mgr.begin();
        let ctx = ExecutorContext { catalog: env1.table_meta.clone(), txn: txn.clone(), lock_manager: env1.txn_mgr.lock_manager.clone() };
        let mut exec = SeqScanExecutor::new(&ctx);
        exec.init();
        assert!(exec.next().is_some());
        env1.txn_mgr.commit(txn);
    });

    let h2 = thread::spawn(move || {
        let txn = env2.txn_mgr.begin();
        let ctx = ExecutorContext { catalog: env2.table_meta.clone(), txn: txn.clone(), lock_manager: env2.txn_mgr.lock_manager.clone() };
        let mut exec = SeqScanExecutor::new(&ctx);
        exec.init();
        assert!(exec.next().is_some());
        env2.txn_mgr.commit(txn);
    });

    h1.join().unwrap();
    h2.join().unwrap();
}

#[test]
fn test_exclusive_lock_conflict_and_release() {
    let env = Arc::new(setup());

    // T1 inserts a row (Acquires Exclusive Lock)
    let txn1 = env.txn_mgr.begin();
    let ctx1 = ExecutorContext { catalog: env.table_meta.clone(), txn: txn1.clone(), lock_manager: env.txn_mgr.lock_manager.clone() };
    let values = vec![vec![Value::Integer(100)]];
    let mut insert_exec = InsertExecutor::new(&ctx1, values);
    insert_exec.init();
    insert_exec.next(); // Lock Acquired here

    // T2 tries to read (Should Block)
    let env2 = env.clone();
    let h2 = thread::spawn(move || {
        let txn2 = env2.txn_mgr.begin();
        let ctx2 = ExecutorContext { catalog: env2.table_meta.clone(), txn: txn2.clone(), lock_manager: env2.txn_mgr.lock_manager.clone() };
        let mut scan_exec = SeqScanExecutor::new(&ctx2);
        scan_exec.init();
        
        // This call should block until T1 commits
        let tuple = scan_exec.next(); 
        assert!(tuple.is_some());
        
        env2.txn_mgr.commit(txn2);
    });

    // Sleep to ensure T2 is waiting
    thread::sleep(Duration::from_millis(200));
    
    // T1 commits (Releases Lock)
    env.txn_mgr.commit(txn1);
    
    // T2 should now complete
    h2.join().unwrap();
}

#[test]
fn test_abort_releases_lock() {
    let env = Arc::new(setup());

    // T1 inserts but then aborts
    let txn1 = env.txn_mgr.begin();
    let ctx1 = ExecutorContext { catalog: env.table_meta.clone(), txn: txn1.clone(), lock_manager: env.txn_mgr.lock_manager.clone() };
    let mut insert_exec = InsertExecutor::new(&ctx1, vec![vec![Value::Integer(999)]]);
    insert_exec.init();
    insert_exec.next(); // Locks row

    let env2 = env.clone();
    let h2 = thread::spawn(move || {
        let txn2 = env2.txn_mgr.begin();
        let ctx2 = ExecutorContext { catalog: env2.table_meta.clone(), txn: txn2.clone(), lock_manager: env2.txn_mgr.lock_manager.clone() };
        let mut scan_exec = SeqScanExecutor::new(&ctx2);
        scan_exec.init();
        
        // Should block until T1 aborts
        let tuple = scan_exec.next();
        // Since T1 aborted, the row might still physically exist (we don't implement Undo yet),
        // but the lock should be released.
        // NOTE: In our current implementation, we don't undo the insert. So T2 WILL see the dirty row.
        // This is a known limitation of Phase 1 (No Undo). 
        // But the test is checking that T2 *unblocks*, which is the LockManager's job.
        assert!(tuple.is_some());
        
        env2.txn_mgr.commit(txn2);
    });

    thread::sleep(Duration::from_millis(200));
    env.txn_mgr.abort(txn1); // Should wake T2
    h2.join().unwrap();
}

#[test]
fn test_multiple_row_locks() {
    let env = Arc::new(setup());
    
    // Insert 10 rows
    {
        let txn = env.txn_mgr.begin();
        let ctx = ExecutorContext { catalog: env.table_meta.clone(), txn: txn.clone(), lock_manager: env.txn_mgr.lock_manager.clone() };
        let mut values = Vec::new();
        for i in 0..10 { values.push(vec![Value::Integer(i)]); }
        let mut exec = InsertExecutor::new(&ctx, values);
        exec.init();
        while exec.next().is_some() {}
        env.txn_mgr.commit(txn);
    }

    // T1 Scans (Acquires 10 Shared Locks)
    let txn1 = env.txn_mgr.begin();
    let ctx1 = ExecutorContext { catalog: env.table_meta.clone(), txn: txn1.clone(), lock_manager: env.txn_mgr.lock_manager.clone() };
    let mut scan_exec = SeqScanExecutor::new(&ctx1);
    scan_exec.init();
    while scan_exec.next().is_some() {} // Locks all 10

    {
        let t = txn1.lock().unwrap();
        assert_eq!(t.shared_locks.len(), 10);
    }
    
    env.txn_mgr.commit(txn1);
}
