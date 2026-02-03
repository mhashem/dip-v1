use dip_v1::concurrency::lock_manager::{LockManager, LockMode};
use dip_v1::concurrency::transaction::Transaction;
use dip_v1::storage::table::rid::RID;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[test]
fn test_lock_manager_shared_compatibility() {
    let lm = Arc::new(LockManager::new());
    let rid = RID::new(1, 0);

    let t1 = Arc::new(Mutex::new(Transaction::new(1)));
    let t2 = Arc::new(Mutex::new(Transaction::new(2)));

    // T1 acquires Shared
    assert!(lm.acquire_lock(t1.clone(), rid, LockMode::Shared));
    
    // T2 acquires Shared (Should succeed immediately)
    assert!(lm.acquire_lock(t2.clone(), rid, LockMode::Shared));
    
    lm.release_lock(t1, rid);
    lm.release_lock(t2, rid);
}

#[test]
fn test_lock_manager_exclusive_conflict() {
    let lm = Arc::new(LockManager::new());
    let rid = RID::new(2, 0);

    let t1 = Arc::new(Mutex::new(Transaction::new(1)));
    let t2 = Arc::new(Mutex::new(Transaction::new(2)));

    // T1 acquires Exclusive
    assert!(lm.acquire_lock(t1.clone(), rid, LockMode::Exclusive));

    let lm_clone = lm.clone();
    let t2_clone = t2.clone();
    
    // Spawn thread for T2 (it should block)
    let handle = thread::spawn(move || {
        // This should block until T1 releases
        assert!(lm_clone.acquire_lock(t2_clone, rid, LockMode::Shared));
    });

    // Sleep to ensure T2 is waiting
    thread::sleep(Duration::from_millis(100));
    
    // Release T1
    lm.release_lock(t1, rid);
    
    // Join T2
    handle.join().unwrap();
}
