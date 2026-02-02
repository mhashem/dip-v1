use crate::execution::executor::{Executor, ExecutorContext};
use crate::storage::tuple::Tuple;
use crate::catalog::schema::Schema;
use crate::storage::table::table_heap::TableIterator;
use crate::execution::expression::Expression;

pub struct SeqScanExecutor<'a> {
    context: &'a ExecutorContext,
    iterator: Option<TableIterator>,
    predicate: Option<Expression>,
}

impl<'a> SeqScanExecutor<'a> {
    pub fn new(context: &'a ExecutorContext) -> Self {
        Self {
            context,
            iterator: None,
            predicate: None,
        }
    }
    
    pub fn set_predicate(&mut self, predicate: Expression) {
        self.predicate = Some(predicate);
    }
}

impl<'a> Executor for SeqScanExecutor<'a> {
    fn init(&mut self) {
        // TASK 1: Initialize the iterator.
        // The table heap is available in `self.context.catalog.table`.
        // Call `.iter()` on it and assign to `self.iterator`.
        
        // Example: self.iterator = Some(self.context.catalog.table.iter());
        self.iterator = Some(self.context.catalog.table.iter());
    }

    fn next(&mut self) -> Option<Tuple> {
        let iterator = self.iterator.as_mut()?;
        
        loop {
            // Zone Map Pruning Logic
            if let Some(predicate) = &self.predicate {
                 let pid = iterator.get_current_page_id();
                 println!("SeqScan checking page {}", pid); // DEBUG
                 
                 let stats_map = self.context.catalog.page_stats.read().unwrap();
                 let should_skip = if let Some(page_stats) = stats_map.get(&pid) {
                     !page_stats.might_satisfy(predicate)
                 } else {
                     false
                 };
                 drop(stats_map); // Release lock immediately

                 if should_skip {
                     println!("Skipping page {}", pid); // DEBUG
                     iterator.skip_page();
                     if iterator.is_end() {
                         return None;
                     }
                     continue; // Loop again to check the next page
                 }
            }
            
            // println!("Scanning page {}", iterator.get_current_page_id());
            match iterator.next() {
                Some(tuple) => {
                    let matches = if let Some(predicate) = &self.predicate {
                        match predicate.evaluate(&tuple, &self.context.catalog.schema) {
                            crate::types::Value::Boolean(b) => b,
                            _ => panic!("Predicate must return boolean"),
                        }
                    } else {
                        true
                    };

                    if matches {
                        return Some(tuple);
                    }
                    // Else continue loop to next tuple
                }
                None => return None,
            }
        }
    }

    fn schema(&self) -> &Schema {
        &self.context.catalog.schema
    }
}
