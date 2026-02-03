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
use std::thread;
use std::time::Instant;
use tempfile::NamedTempFile;

#[test]
fn test_concurrency_load() {
    let temp_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(temp_file.path()).unwrap();
    // Large buffer pool to avoid IO bottleneck
    let bpm = Arc::new(BufferPoolManager::new(1000, dm));
    let mut catalog = CatalogManager::new(bpm);
    
    let schema = Schema::new(vec![Column::new("v", TypeId::Integer)]);
    let table_meta = catalog.create_table("bench".to_string(), schema);
    let txn_mgr = Arc::new(TransactionManager::new());

    // 1. Populate 1000 rows
    {
        let txn = txn_mgr.begin();
        let ctx = ExecutorContext { catalog: table_meta.clone(), txn: txn.clone(), lock_manager: txn_mgr.lock_manager.clone() };
        let mut values = Vec::new();
        for i in 0..10_000 { values.push(vec![Value::Integer(i)]); }
        let mut exec = InsertExecutor::new(&ctx, values);
        exec.init();
        while exec.next().is_some() {}
        txn_mgr.commit(txn);
    }
    
    println!("Population complete. Starting benchmark...");

    let start = Instant::now();
    let num_threads = 12;
    let ops_per_thread = 50;
    let mut handles = Vec::new();

    let table_meta_arc = table_meta; // Already Arc
    
    for t_idx in 0..num_threads {
        let tm = txn_mgr.clone();
        let tm_meta = table_meta_arc.clone();
        
        handles.push(thread::spawn(move || {
            let mut ops = 0;
            for i in 0..ops_per_thread {
                // Mix of Read (Scan) and Write (Insert)
                // Since Scan locks EVERYTHING, it conflicts heavily with Inserts.
                // To make it run, we'll do:
                // 90% Inserts (New rows, locking new RIDs - Low contention)
                // 10% Scans (Locks all rows - High contention)
                
                let is_scan = (t_idx + i) % 10 == 0; 
                
                let txn = tm.begin();
                let ctx = ExecutorContext { catalog: tm_meta.clone(), txn: txn.clone(), lock_manager: tm.lock_manager.clone() };
                
                if is_scan {
                    // Scan
                    let mut exec = SeqScanExecutor::new(&ctx);
                    exec.init();
                    while exec.next().is_some() {}
                } else {
                    // Insert
                    let val = (t_idx * 10000) + i;
                    let mut exec = InsertExecutor::new(&ctx, vec![vec![Value::Integer(val as i32)]]);
                    exec.init();
                    exec.next();
                }
                
                tm.commit(txn);
                ops += 1;
            }
            ops
        }));
    }
    
    let mut total_ops = 0;
    for h in handles {
        total_ops += h.join().unwrap();
    }
    
    let duration = start.elapsed();
    println!("Benchmark finished in {:?}. Total Ops: {}. TPS: {:.2}", 
             duration, total_ops, total_ops as f64 / duration.as_secs_f64());
}
