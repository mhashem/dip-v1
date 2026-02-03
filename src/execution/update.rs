use crate::catalog::schema::Schema;
use crate::concurrency::lock_manager::LockMode;
use crate::execution::executor::{Executor, ExecutorContext};
use crate::execution::expression::Expression;
use crate::storage::tuple::Tuple;
use std::collections::HashMap;

/// Updates tuples in a table.
/// Strategy: Delete (Mark as deleted) + Insert (New tuple with updated values).
pub struct UpdateExecutor<'a> {
    context: &'a ExecutorContext,
    child: Box<dyn Executor + 'a>,
    /// Maps Column Index -> Expression to calculate new value.
    /// If a column index is missing, the old value is kept.
    assignments: HashMap<usize, Expression>,
}

impl<'a> UpdateExecutor<'a> {
    pub fn new(
        context: &'a ExecutorContext,
        child: Box<dyn Executor + 'a>,
        assignments: HashMap<usize, Expression>,
    ) -> Self {
        Self {
            context,
            child,
            assignments,
        }
    }
}

impl<'a> Executor for UpdateExecutor<'a> {
    fn init(&mut self) {
        self.child.init();
    }

    fn next(&mut self) -> Option<Tuple> {
        // 1. Fetch next tuple
        let tuple = self.child.next()?;
        
        // 2. Extract RID
        let rid = tuple.rid.expect("Tuple must have RID for update");
        
        // 3. Acquire Exclusive Lock
        if !self.context.lock_manager.acquire_lock(self.context.txn.clone(), rid, LockMode::Exclusive) {
            // Transaction aborted or lock failed
            return None;
        }
        
        // 4. Calculate New Values
        let mut new_values = Vec::new();
        let schema = &self.context.catalog.schema;
        
        for i in 0..schema.column_count() {
            if let Some(expr) = self.assignments.get(&i) {
                // Calculate new value (e.g., SET a = a + 1)
                let val = expr.evaluate(&tuple, schema);
                new_values.push(val);
            } else {
                // Keep old value
                let val = tuple.get_value(schema, i);
                new_values.push(val);
            }
        }
        
        let new_tuple = Tuple::from_values(new_values, schema);
        
        // 5. Mark Old as Deleted
        if !self.context.catalog.table.mark_delete(rid) {
            // Failed to delete? Maybe already deleted by someone else?
            // In 2PL, we hold the lock, so this shouldn't happen unless deleted before we locked?
            // But we just read it via SeqScan (which took Shared lock).
            return None;
        }
        
        // 6. Insert New Tuple
        if let Some(_new_rid) = self.context.catalog.table.insert_tuple(&new_tuple) {
            // Note: We are NOT updating the index here yet.
            // This means the Index still points to the OLD (deleted) RID.
            // A subsequent IndexScan will find the old RID, check the heap, find it deleted, and skip it.
            // This is "correct" for visibility, but the new row is effectively "unindexed" until we rebuild/update index.
            
            Some(new_tuple)
        } else {
            None
        }
    }

    fn schema(&self) -> &Schema {
        &self.context.catalog.schema
    }
}
