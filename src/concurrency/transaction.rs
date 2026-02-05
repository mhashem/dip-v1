use crate::storage::table::rid::RID;
use crate::storage::tuple::Tuple;
use std::collections::HashSet;

pub type TxnId = usize;

#[derive(Debug, Clone)]
pub enum WriteRecord {
    Insert { table_name: String, rid: RID },
    Delete { table_name: String, rid: RID, old_tuple: Tuple },
    Update { table_name: String, old_rid: RID, old_tuple: Tuple, new_rid: RID, new_tuple: Tuple },
}

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
pub struct Transaction {
    /// Unique identifier for this transaction.
    pub txn_id: TxnId,
    
    /// Current state of execution.
    pub state: TransactionState,
    
    /// The set of shared locks (Read locks) held by this transaction.
    pub shared_locks: HashSet<RID>,
    
    /// The set of exclusive locks (Write locks) held by this transaction.
    pub exclusive_locks: HashSet<RID>,
    
    /// List of modifications made by this transaction (for Undo/Abort).
    pub write_set: Vec<WriteRecord>,
}

impl Transaction {
    pub fn new(txn_id: TxnId) -> Self {
        Self {
            txn_id,
            state: TransactionState::Running,
            shared_locks: HashSet::new(),
            exclusive_locks: HashSet::new(),
            write_set: Vec::new(),
        }
    }

    pub fn is_aborted(&self) -> bool {
        self.state == TransactionState::Aborted
    }
    
    pub fn is_committed(&self) -> bool {
        self.state == TransactionState::Committed
    }
}
