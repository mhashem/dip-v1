use crate::execution::executor::{Executor, ExecutorContext};
use crate::storage::tuple::Tuple;
use crate::catalog::schema::Schema;
use crate::types::Value;

pub struct IndexScanExecutor<'a> {
    context: &'a ExecutorContext,
    key: i32,
    finished: bool,
}

impl<'a> IndexScanExecutor<'a> {
    pub fn new(context: &'a ExecutorContext, key: i32) -> Self {
        Self {
            context,
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

        if let Some(index) = &self.context.catalog.index {
            if let Some(rid) = index.lock().unwrap().get_value(self.key) {
                return self.context.catalog.table.get_tuple(rid);
            }
        }
        None
    }

    fn schema(&self) -> &Schema {
        &self.context.catalog.schema
    }
}
