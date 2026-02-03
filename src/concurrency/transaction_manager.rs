use crate::concurrency::lock_manager::LockManager;
use crate::concurrency::transaction::{Transaction, TransactionState, TxnId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Manages the global state of transactions.
pub struct TransactionManager {
    /// Atomic counter to ensure every transaction gets a unique ID.
    next_txn_id: AtomicUsize,
    
    /// OPTIONAL: Registry of active transactions.
    active_txns: Mutex<HashMap<TxnId, Arc<Mutex<Transaction>>>>,
    
    /// The Lock Manager used to acquire/release locks.
    pub lock_manager: Arc<LockManager>,
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            next_txn_id: AtomicUsize::new(0),
            active_txns: Mutex::new(HashMap::new()),
            lock_manager: Arc::new(LockManager::new()),
        }
    }

    /// Starts a new transaction.
    /// Returns a thread-safe reference (Arc<Mutex<...>>) to the transaction.
    pub fn begin(&self) -> Arc<Mutex<Transaction>> {
        // 1. Get a unique ID. `fetch_add` does this atomically.
        let txn_id = self.next_txn_id.fetch_add(1, Ordering::SeqCst);
        
        // 2. Create the transaction object.
        let txn = Arc::new(Mutex::new(Transaction::new(txn_id)));
        
        // 3. Register it.
        let mut map = self.active_txns.lock().unwrap();
        map.insert(txn_id, txn.clone());
        
        // println!("Txn {} began", txn_id);
        
        txn
    }

    /// Commits a transaction.
    pub fn commit(&self, txn: Arc<Mutex<Transaction>>) {
        // 1. Collect locks to release (Avoid deadlock by not holding lock while calling release)
        let locks_to_release: Vec<_> = {
            let txn_guard = txn.lock().unwrap();
            if txn_guard.state != TransactionState::Running {
                return;
            }
            txn_guard.shared_locks.iter().chain(txn_guard.exclusive_locks.iter()).cloned().collect()
        };

        // 2. Release all locks (Strict 2PL)
        for rid in locks_to_release {
            self.lock_manager.release_lock(txn.clone(), rid);
        }

        // 3. Update State
        let mut txn_guard = txn.lock().unwrap();
        txn_guard.state = TransactionState::Committed;
        
        // Remove from active list
        let mut map = self.active_txns.lock().unwrap();
        map.remove(&txn_guard.txn_id);
        
        // println!("Txn {} committed", txn_guard.txn_id);
    }

    /// Aborts a transaction.
    pub fn abort(&self, txn: Arc<Mutex<Transaction>>) {
        // 1. Collect locks to release
        let locks_to_release: Vec<_> = {
            let txn_guard = txn.lock().unwrap();
             if txn_guard.state != TransactionState::Running {
                return;
            }
            txn_guard.shared_locks.iter().chain(txn_guard.exclusive_locks.iter()).cloned().collect()
        };

        // 2. Release all locks
        for rid in locks_to_release {
            self.lock_manager.release_lock(txn.clone(), rid);
        }

        // 3. Update State
        let mut txn_guard = txn.lock().unwrap();
        txn_guard.state = TransactionState::Aborted;
        
        // Remove from active list
        let mut map = self.active_txns.lock().unwrap();
        map.remove(&txn_guard.txn_id);
        
        // println!("Txn {} aborted", txn_guard.txn_id);
    }
}
