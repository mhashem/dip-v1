use crate::catalog::catalog_manager::TableMetadata;
use crate::storage::tuple::Tuple;
use crate::catalog::schema::Schema;
use std::sync::Arc;

pub struct ExecutorContext {
    pub catalog: Arc<TableMetadata>, // For simplicity, context holds the target table
}

pub trait Executor {
    fn init(&mut self);
    fn next(&mut self) -> Option<Tuple>;
    fn schema(&self) -> &Schema;
}
