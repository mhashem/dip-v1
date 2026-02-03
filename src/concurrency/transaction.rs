use crate::storage::table::rid::RID;
use std::collections::HashSet;

pub type TxnId = usize;

/// Transaction States based on the standard lifecycle:
/// Running -> Committed
/// Running -> Aborted
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionState {
    Running,
    Committed,
    Aborted,
}

/// The Transaction context.
/// 
/// This struct holds the dynamic state of a single transaction.
/// It is usually wrapped in an Arc<Mutex<Transaction>> because multiple parts
/// of the engine (Executors, Lock Manager) need to read/modify it properly.
pub struct Transaction {
    /// Unique identifier for this transaction.
    pub txn_id: TxnId,
    
    /// Current state of execution.
    pub state: TransactionState,
    
    /// The set of shared locks (Read locks) held by this transaction.
    /// We track these so we can release them when the transaction finishes.
    pub shared_locks: HashSet<RID>,
    
    /// The set of exclusive locks (Write locks) held by this transaction.
    pub exclusive_locks: HashSet<RID>,
    
    // In the future, we will add:
    // pub write_set: Vec<WriteRecord>, // For rollback (Undo)
}

impl Transaction {
    pub fn new(txn_id: TxnId) -> Self {
        Self {
            txn_id,
            state: TransactionState::Running,
            shared_locks: HashSet::new(),
            exclusive_locks: HashSet::new(),
        }
    }

    pub fn is_aborted(&self) -> bool {
        self.state == TransactionState::Aborted
    }
    
    pub fn is_committed(&self) -> bool {
        self.state == TransactionState::Committed
    }
}
