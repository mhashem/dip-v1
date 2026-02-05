use crate::catalog::schema::Schema;
use crate::execution::executor::{Executor, ExecutorContext};
use crate::storage::tuple::Tuple;

pub struct IndexScanExecutor<'a> {
    context: &'a ExecutorContext,
    col_idx: usize,
    key: i32,
    finished: bool,
}

impl<'a> IndexScanExecutor<'a> {
    pub fn new(context: &'a ExecutorContext, col_idx: usize, key: i32) -> Self {
        Self {
            context,
            col_idx,
            key,
            finished: false,
        }
    }
}

impl<'a> Executor for IndexScanExecutor<'a> {
    fn init(&mut self) {
        self.finished = false;
    }

    fn next(&mut self) -> Option<Tuple> {
        if self.finished {
            return None;
        }
        self.finished = true;

        let indexes = self.context.catalog.indexes.read().unwrap();
        if let Some(index) = indexes.get(&self.col_idx) {
            if let Some(rid) = index.read().unwrap().get_value(self.key) {
                if let Some(tuple) = self.context.catalog.table.get_tuple(rid) {
                     // IMPORTANT: Double check the key still matches (could have been updated)
                     // and that it's NOT deleted (get_tuple already checks for deleted in SlottedPage).
                     
                     // ACQUIRE SHARED LOCK
                     if !self.context.lock_manager.acquire_lock(self.context.txn.clone(), rid, crate::concurrency::lock_manager::LockMode::Shared) {
                         return None;
                     }
                     return Some(tuple);
                }
            }
        }
        None
    }

    fn schema(&self) -> &Schema {
        &self.context.catalog.schema
    }
}
