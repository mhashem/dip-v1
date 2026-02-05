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
    assignments: HashMap<usize, Expression>,
    /// RIDs to update, collected during init or first next() call
    rids_to_update: Vec<crate::storage::table::rid::RID>,
    cursor: usize,
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
            rids_to_update: Vec::new(),
            cursor: 0,
        }
    }
}

impl<'a> Executor for UpdateExecutor<'a> {
    fn init(&mut self) {
        self.child.init();
        self.rids_to_update.clear();
        self.cursor = 0;
        
        // Materialize RIDs to avoid Halloween Problem
        while let Some(tuple) = self.child.next() {
            if let Some(rid) = tuple.rid {
                self.rids_to_update.push(rid);
            }
        }
    }

    fn next(&mut self) -> Option<Tuple> {
        if self.cursor >= self.rids_to_update.len() {
            return None;
        }

        let rid = self.rids_to_update[self.cursor];
        self.cursor += 1;

        // 1. Fetch the tuple by RID (to get current data)
        let tuple = self.context.catalog.table.get_tuple(rid)?;
        
        // 2. Acquire Exclusive Lock
        if !self.context.lock_manager.acquire_lock(self.context.txn.clone(), rid, LockMode::Exclusive) {
            return None;
        }
        
        // 3. Calculate New Values
        let mut new_values = Vec::new();
        let schema = &self.context.catalog.schema;
        
        for i in 0..schema.column_count() {
            if let Some(expr) = self.assignments.get(&i) {
                let val = expr.evaluate(&tuple, schema);
                new_values.push(val);
            } else {
                let val = tuple.get_value(schema, i);
                new_values.push(val);
            }
        }
        
        let new_tuple = Tuple::from_values(new_values, schema);
        
        // 4. Mark Old as Deleted
        if !self.context.catalog.table.mark_delete(rid) {
            return None;
        }
        
        // 5. Insert New Tuple
        if let Some(new_rid) = self.context.catalog.table.insert_tuple(&new_tuple) {
            // Record for Undo
            self.context.txn.lock().unwrap().write_set.push(crate::concurrency::transaction::WriteRecord::Update { 
                table_name: self.context.catalog.name.clone(),
                old_rid: rid, 
                old_tuple: tuple.clone(),
                new_rid,
                new_tuple: new_tuple.clone(),
            });

            // Update Zone Maps
            {
                let mut stats_map = self.context.catalog.page_stats.write().unwrap();
                let page_stats = stats_map.entry(new_rid.page_id).or_insert_with(crate::catalog::stats::PageStats::new);
                for i in 0..schema.column_count() {
                    let val = new_tuple.get_value(schema, i);
                    page_stats.update(i, &val);
                }
            }

            // Update Indexes
            let indexes = self.context.catalog.indexes.read().unwrap();
            for (col_idx, index) in indexes.iter() {
                let old_val = tuple.get_value(schema, *col_idx);
                let new_val = new_tuple.get_value(schema, *col_idx);
                
                if let (crate::types::Value::Integer(old_k), crate::types::Value::Integer(new_k)) = (old_val, new_val) {
                    let mut idx = index.write().unwrap();
                    if old_k == new_k {
                        idx.update_value(new_k, new_rid);
                    } else {
                        idx.delete(old_k);
                        idx.insert(new_k, new_rid);
                    }
                }
            }
            Some(new_tuple)
        } else {
            None
        }
    }

    fn schema(&self) -> &Schema {
        &self.context.catalog.schema
    }
}
