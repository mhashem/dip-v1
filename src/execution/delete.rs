use crate::execution::executor::{Executor, ExecutorContext};
use crate::storage::tuple::Tuple;
use crate::concurrency::lock_manager::LockMode;
use crate::catalog::schema::Schema;

pub struct DeleteExecutor<'a> {
    context: &'a ExecutorContext,
    child: Box<dyn Executor + 'a>,
    pub rids_to_delete: Vec<crate::storage::table::rid::RID>,
    cursor: usize,
}

impl<'a> DeleteExecutor<'a> {
    pub fn new(context: &'a ExecutorContext, child: Box<dyn Executor + 'a>) -> Self {
        Self { 
            context, 
            child,
            rids_to_delete: Vec::new(),
            cursor: 0,
        }
    }
}

impl<'a> Executor for DeleteExecutor<'a> {
    fn init(&mut self) {
        self.child.init();
        self.rids_to_delete.clear();
        self.cursor = 0;
        
        while let Some(tuple) = self.child.next() {
            if let Some(rid) = tuple.rid {
                self.rids_to_delete.push(rid);
            }
        }
    }

    fn next(&mut self) -> Option<Tuple> {
        if self.cursor >= self.rids_to_delete.len() {
            return None;
        }

        let rid = self.rids_to_delete[self.cursor];
        self.cursor += 1;

        // 1. Fetch tuple (to ensure it still exists and for returning it)
        let tuple = self.context.catalog.table.get_tuple(rid)?;

        // 2. Acquire Exclusive Lock
        if !self.context.lock_manager.acquire_lock(self.context.txn.clone(), rid, LockMode::Exclusive) {
            return None;
        }

        // 3. Mark as Deleted
        if self.context.catalog.table.mark_delete(rid) {
            // Record for Undo
            self.context.txn.lock().unwrap().write_set.push(crate::concurrency::transaction::WriteRecord::Delete { 
                table_name: self.context.catalog.name.clone(),
                rid, 
                old_tuple: tuple.clone() 
            });

            // Update Indexes
            let indexes = self.context.catalog.indexes.read().unwrap();
            let schema = &self.context.catalog.schema;
            for (col_idx, index) in indexes.iter() {
                if let crate::types::Value::Integer(k) = tuple.get_value(schema, *col_idx) {
                    index.write().unwrap().delete(k);
                }
            }

            Some(tuple)
        } else {
            None
        }
    }

    fn schema(&self) -> &Schema {
        &self.context.catalog.schema
    }
}
