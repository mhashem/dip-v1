use crate::catalog::catalog_manager::TableMetadata;
use crate::catalog::schema::Schema;
use crate::storage::tuple::Tuple;
use crate::concurrency::transaction::Transaction;
use crate::concurrency::lock_manager::LockManager;
use std::sync::{Arc, Mutex};

pub struct ExecutorContext {
    pub catalog: Arc<TableMetadata>,
    pub txn: Arc<Mutex<Transaction>>,
    pub lock_manager: Arc<LockManager>,
}

pub trait Executor {
    fn init(&mut self);
    fn next(&mut self) -> Option<Tuple>;
    fn schema(&self) -> &Schema;
}
