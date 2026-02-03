use crate::concurrency::transaction::{Transaction, TxnId};
use crate::storage::table::rid::RID;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Condvar, Mutex};

const NUM_SHARDS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    Shared,
    Exclusive,
}

#[derive(Debug, Clone)]
struct LockRequest {
    txn_id: TxnId,
    mode: LockMode,
    granted: bool,
}

#[derive(Default)]
struct LockRequestQueue {
    request_queue: VecDeque<LockRequest>,
    /// How many shared locks are currently *granted* on this resource?
    shared_count: usize,
    /// Is there an exclusive lock *granted* on this resource?
    exclusive_granted: bool,
}

pub struct LockManager {
    /// Sharded lock tables.
    /// Each shard is an independent Mutex.
    shards: Vec<Mutex<HashMap<RID, LockRequestQueue>>>,
    /// Independent condition variables for each shard.
    condvars: Vec<Condvar>,
}

impl LockManager {
    pub fn new() -> Self {
        let mut shards = Vec::with_capacity(NUM_SHARDS);
        let mut condvars = Vec::with_capacity(NUM_SHARDS);
        
        for _ in 0..NUM_SHARDS {
            shards.push(Mutex::new(HashMap::new()));
            condvars.push(Condvar::new());
        }

        Self {
            shards,
            condvars,
        }
    }

    fn get_shard_idx(&self, rid: RID) -> usize {
        let mut hasher = DefaultHasher::new();
        rid.hash(&mut hasher);
        (hasher.finish() as usize) % NUM_SHARDS
    }

    /// Acquires a lock on a resource (RID) in the specified mode.
    /// Blocks until the lock is granted.
    /// Returns true if successful, false if aborted (deadlock detection not yet implemented).
    pub fn acquire_lock(&self, txn: Arc<Mutex<Transaction>>, rid: RID, mode: LockMode) -> bool {
        let shard_idx = self.get_shard_idx(rid);
        let lock_shard = &self.shards[shard_idx];
        let condvar = &self.condvars[shard_idx];

        let mut table = lock_shard.lock().unwrap();
        let queue = table.entry(rid).or_insert_with(LockRequestQueue::default);
        
        let txn_id = txn.lock().unwrap().txn_id;
        
        // 0. Re-entrancy Check
        // Use iter_mut to allow upgrade
        if let Some(existing_req) = queue.request_queue.iter_mut().find(|r| r.txn_id == txn_id && r.granted) {
            match existing_req.mode {
                LockMode::Exclusive => {
                    // Already hold X. Grants everything.
                    return true;
                }
                LockMode::Shared => {
                    if mode == LockMode::Shared {
                        // Already hold S. Grant S.
                        return true;
                    } else {
                        // Hold S, Want X. UPGRADE.
                        if queue.shared_count == 1 {
                            // We are the only holder. Upgrade allowed.
                            existing_req.mode = LockMode::Exclusive;
                            queue.shared_count -= 1;
                            queue.exclusive_granted = true;
                            
                            // Update Transaction State
                            let mut txn_guard = txn.lock().unwrap();
                            txn_guard.shared_locks.remove(&rid);
                            txn_guard.exclusive_locks.insert(rid);
                            
                            return true;
                        } else {
                            // Shared by others. Cannot upgrade immediately.
                            // For strict 2PL without wait-die, we abort to avoid deadlock.
                            return false; 
                        }
                    }
                }
            }
        }
        
        // Add request to queue
        queue.request_queue.push_back(LockRequest {
            txn_id,
            mode,
            granted: false,
        });

        // 2. Wait loop
        loop {
            let queue = table.get_mut(&rid).unwrap(); 
            
            // Check if we can grant the lock
            if self.can_grant_lock(queue, txn_id, mode) {
                // Grant logic
                self.grant_lock(queue, txn_id, mode);
                break;
            }

            // Wait
            table = condvar.wait(table).unwrap();
            
            // Check if transaction was aborted externally while waiting?
            if txn.lock().unwrap().is_aborted() {
                // Remove request and return
                let queue = table.get_mut(&rid).unwrap();
                self.remove_request(queue, txn_id);
                condvar.notify_all();
                return false;
            }
        }

        let mut txn_guard = txn.lock().unwrap();
        match mode {
            LockMode::Shared => { txn_guard.shared_locks.insert(rid); }
            LockMode::Exclusive => { txn_guard.exclusive_locks.insert(rid); }
        }
        
        true
    }

    /// Releases a lock on a resource.
    pub fn release_lock(&self, txn: Arc<Mutex<Transaction>>, rid: RID) -> bool {
        let shard_idx = self.get_shard_idx(rid);
        let lock_shard = &self.shards[shard_idx];
        let condvar = &self.condvars[shard_idx];

        let mut table = lock_shard.lock().unwrap();
        
        if let Some(queue) = table.get_mut(&rid) {
            let txn_id = txn.lock().unwrap().txn_id;
            
            // Logic to decrement counts
            let mut found = false;
            if let Some(req) = queue.request_queue.iter().find(|r| r.txn_id == txn_id && r.granted) {
                 match req.mode {
                     LockMode::Shared => {
                         if queue.shared_count > 0 { queue.shared_count -= 1; }
                     }
                     LockMode::Exclusive => {
                         queue.exclusive_granted = false;
                     }
                 }
                 found = true;
            }
            
            if found {
                self.remove_request(queue, txn_id);
                // Clean up empty queue
                if queue.request_queue.is_empty() {
                    table.remove(&rid);
                }
                
                // Wake up waiters in this shard
                condvar.notify_all();
            }
        }
        
        // Remove from transaction bookkeeping
        let mut txn_guard = txn.lock().unwrap();
        txn_guard.shared_locks.remove(&rid);
        txn_guard.exclusive_locks.remove(&rid);
        
        true
    }
    
    // --- Helpers ---

    fn can_grant_lock(&self, queue: &LockRequestQueue, txn_id: TxnId, mode: LockMode) -> bool {
        // Check 1: Conflict with granted
        match mode {
            LockMode::Shared => {
                if queue.exclusive_granted { return false; }
            }
            LockMode::Exclusive => {
                if queue.exclusive_granted || queue.shared_count > 0 { return false; }
            }
        }
        
        // Check 2: Queue position (Strict FIFO)
        for req in &queue.request_queue {
            if req.txn_id == txn_id {
                return true; 
            }
            if !req.granted {
                return false;
            }
        }
        
        false
    }

    fn grant_lock(&self, queue: &mut LockRequestQueue, txn_id: TxnId, mode: LockMode) {
        for req in &mut queue.request_queue {
            if req.txn_id == txn_id {
                req.granted = true;
                match mode {
                    LockMode::Shared => queue.shared_count += 1,
                    LockMode::Exclusive => queue.exclusive_granted = true,
                }
                return;
            }
        }
    }

    fn remove_request(&self, queue: &mut LockRequestQueue, txn_id: TxnId) {
        queue.request_queue.retain(|r| r.txn_id != txn_id);
    }
}
