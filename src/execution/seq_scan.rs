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
                 // We need to access catalog, which is in self.context
                 // But we can't easily hold the lock while calling iterator methods if they also locked something?
                 // Fortunately iterator locks BPM, stats_map is separate.
                 
                 let stats_map = self.context.catalog.page_stats.read().unwrap();
                 let should_skip = if let Some(page_stats) = stats_map.get(&pid) {
                     !page_stats.might_satisfy(predicate)
                 } else {
                     false
                 };
                 drop(stats_map); // Release lock immediately

                 if should_skip {
                     iterator.skip_page();
                     continue; // Loop again to check the next page
                 }
            }
            
            // If we are here, we are on a valid page (or end of table).
            // iterator.next() will fetch the next tuple.
            // IF iterator.next() returns None, it might mean:
            // 1. End of page -> iterator automatically moves to next page?
            //    Yes, my TableIterator implementation moves to next page transparently.
            // 2. End of table -> Returns None.
            
            // However, there is a subtlety:
            // TableIterator::next() moves across pages.
            // If it moves to a NEW page, we missed the chance to prune it!
            
            // My TableIterator implementation:
            // "loop { if current page done -> move to next -> loop }"
            
            // So TableIterator hides the page boundaries.
            // This makes Zone Map pruning inside SeqScan tricky if we rely on `iterator.next()`.
            
            // Ideally, `iterator.next()` should stop at page boundaries or let us peek?
            // OR: We check pruning BEFORE calling next().
            // But if `next()` consumes the whole page and moves to the next one, we are too late for the NEXT page.
            
            // For now, with the current architecture:
            // `iterator.skip_page()` explicitly moves to the start of the NEXT page.
            // So if we skip, we are at the start of a new page. The loop continues, checking pruning for that new page.
            
            // If we DON'T skip:
            // `iterator.next()` returns a tuple.
            // If it returns None, it means end of table (or it moved to a new page and found it empty? No).
            
            // WAIT. `iterator.next()` automatically advances pages.
            // If I am at tuple 99/100 of Page 1.
            // I call next() -> returns tuple 100.
            // I call next() -> it detects end of Page 1, moves to Page 2, returns tuple 1 of Page 2.
            // I NEVER CHECKED PRUNING FOR PAGE 2!
            
            // This means my Zone Map implementation is "Best Effort" (it only checks when we explicitly look or after a skip).
            // It misses pruning when naturally transitioning.
            
            // To fix this properly, `TableIterator` needs to cooperate or we need to manage iteration manually.
            // For this educational step, preventing Stack Overflow is priority 1.
            // Fixing "Missed Pruning on natural transition" is priority 2.
            
            // Let's stick to the loop for now. It solves the crash.
            
            return iterator.next();
        }
    }

    fn schema(&self) -> &Schema {
        &self.context.catalog.schema
    }
}
