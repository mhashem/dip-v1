use crate::catalog::schema::Schema;
use crate::execution::executor::{Executor, ExecutorContext};
use crate::storage::tuple::Tuple;
use crate::types::Value;

/// Inserts values into a table.
/// In a real DB, this would read from a child executor.
/// Here, we'll give it a list of raw values to insert for simplicity.
pub struct InsertExecutor<'a> {
    context: &'a ExecutorContext,
    values: Vec<Vec<Value>>, // Rows to insert
    cursor: usize,
}

impl<'a> InsertExecutor<'a> {
    pub fn new(context: &'a ExecutorContext, values: Vec<Vec<Value>>) -> Self {
        Self {
            context,
            values,
            cursor: 0,
        }
    }
}

impl<'a> Executor for InsertExecutor<'a> {
    fn init(&mut self) {
        self.cursor = 0;
    }

    fn next(&mut self) -> Option<Tuple> {
        // TASK 3: Insert a row and return the inserted tuple.
        // 1. Check if self.cursor < self.values.len()
        // 2. Get the values for the current row.
        // 3. Create a Tuple using `Tuple::from_values`.
        // 4. Insert into table heap: `self.context.catalog.table.insert_tuple(&tuple)`
        // 5. Increment cursor.
        // 6. Return Some(tuple).
        
        // Note: If cursor >= len, return None.
        
        if self.cursor >= self.values.len() {
            return None;
        }
        
        // todo!("Implement Task 3")
        // I will implement part of it to guide you, you finish the insert logic.
        
        let values = self.values[self.cursor].clone();
        let tuple = Tuple::from_values(values.clone(), &self.context.catalog.schema);
        
        // YOUR CODE HERE: Insert `tuple` into `self.context.catalog.table`
        if let Some(rid) = self.context.catalog.table.insert_tuple(&tuple) {
             // Record for Undo
             self.context.txn.lock().unwrap().write_set.push(crate::concurrency::transaction::WriteRecord::Insert { 
                 table_name: self.context.catalog.name.clone(),
                 rid 
             });

             // ACQUIRE EXCLUSIVE LOCK
             if !self.context.lock_manager.acquire_lock(self.context.txn.clone(), rid, crate::concurrency::lock_manager::LockMode::Exclusive) {
                 return None;
             }

             // Update Zone Maps
             let mut stats_map = self.context.catalog.page_stats.write().unwrap();
             let page_stats = stats_map.entry(rid.page_id).or_insert_with(crate::catalog::stats::PageStats::new);
             
             for (i, val) in values.iter().enumerate() {
                 page_stats.update(i, val);
             }

             // Update Indexes
             let indexes = self.context.catalog.indexes.read().unwrap();
             for (col_idx, index) in indexes.iter() {
                 if let Value::Integer(k) = tuple.get_value(&self.context.catalog.schema, *col_idx) {
                     if !index.write().unwrap().insert(k, rid) {
                         // Duplicate Key (Primary Key constraint usually)
                         return None; 
                     }
                 }
             }
        }
        
        self.cursor += 1;
        Some(tuple)
    }

    fn schema(&self) -> &Schema {
        &self.context.catalog.schema
    }
}
