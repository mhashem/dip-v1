use dip_v1::concurrency::transaction::TransactionState;
use dip_v1::concurrency::transaction_manager::TransactionManager;
use std::sync::Arc;
use std::thread;

#[test]
fn test_txn_manager_basics() {
    let txn_manager = Arc::new(TransactionManager::new());
    
    // 1. Single Thread Test
    let txn1 = txn_manager.begin();
    {
        let t = txn1.lock().unwrap();
        assert_eq!(t.txn_id, 0);
        assert_eq!(t.state, TransactionState::Running);
    }
    
    txn_manager.commit(txn1.clone());
    {
        let t = txn1.lock().unwrap();
        assert_eq!(t.state, TransactionState::Committed);
    }

    // 2. Multi-Threaded Test (Concurrency!)
    let txn_manager_clone = txn_manager.clone();
    let handle = thread::spawn(move || {
        let txn2 = txn_manager_clone.begin();
        {
            let t = txn2.lock().unwrap();
            assert!(t.txn_id > 0); // Should be at least 1
        }
        txn_manager_clone.abort(txn2);
    });
    
    handle.join().unwrap();
    
    // Check that IDs are incrementing
    let txn3 = txn_manager.begin();
    {
        let t = txn3.lock().unwrap();
        assert!(t.txn_id >= 2);
    }
}
