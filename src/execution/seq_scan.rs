use crate::catalog::schema::Schema;
use crate::concurrency::lock_manager::LockMode;
use crate::execution::executor::{Executor, ExecutorContext};
use crate::execution::expression::Expression;
use crate::storage::table::table_heap::TableIterator;
use crate::storage::tuple::Tuple;

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
                 // println!("SeqScan checking page {}", pid); // DEBUG
                 
                 let stats_map = self.context.catalog.page_stats.read().unwrap();
                 let should_skip = if let Some(page_stats) = stats_map.get(&pid) {
                     !page_stats.might_satisfy(predicate)
                 } else {
                     false
                 };
                 drop(stats_map); // Release lock immediately

                 if should_skip {
                     // println!("Skipping page {}", pid); // DEBUG
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
                    let rid = tuple.rid.expect("Tuple must have RID from iterator");
                    
                    // ACQUIRE SHARED LOCK
                    if !self.context.lock_manager.acquire_lock(self.context.txn.clone(), rid, LockMode::Shared) {
                        return None; // Transaction Aborted
                    }

                    // After acquiring lock, re-read the page to see if it's still deleted
                    // Actually, TableHeap::get_tuple handles fetching the page.
                    // But we need to check the SlottedPage's mark.
                    let is_deleted = {
                        let _frame = self.context.catalog.bpm.fetch_page(rid.page_id).unwrap();
                        let data = self.context.catalog.bpm.read_from_page(rid.page_id);
                        let mut temp_page = crate::storage::page::Page::new();
                        temp_page.data.copy_from_slice(&data);
                        let slotted = crate::storage::table::slotted_page::SlottedPage::new(&mut temp_page);
                        let deleted = slotted.is_tuple_marked_for_delete(rid.slot_num as u16);
                        self.context.catalog.bpm.unpin_page(rid.page_id, false);
                        deleted
                    };

                    if is_deleted {
                        continue; // Skip logically deleted tuple
                    }

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
