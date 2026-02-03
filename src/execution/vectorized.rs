use crate::types::Value;
use crate::catalog::schema::Schema;

/// A batch of tuples, organized by column (Columnar Layout).
/// This structure is CPU-cache friendly and allows for SIMD processing.
#[derive(Debug, Clone)]
pub struct TupleBatch {
    /// Columns of data.
    /// Outer Vec: Columns (aligned with Schema).
    /// Inner Vec: Values for that column (aligned with row index).
    pub columns: Vec<Vec<Value>>,
    
    /// Number of valid rows in this batch.
    pub row_count: usize,
}

impl TupleBatch {
    pub fn new(schema: &Schema, capacity: usize) -> Self {
        let mut columns = Vec::with_capacity(schema.column_count());
        for _ in 0..schema.column_count() {
            columns.push(Vec::with_capacity(capacity));
        }
        Self {
            columns,
            row_count: 0,
        }
    }

    pub fn push(&mut self, values: Vec<Value>) {
        // Warning: This row-wise push is slow.
        // In a real vectorized engine, we would push columnar arrays directly.
        for (col_idx, val) in values.into_iter().enumerate() {
            self.columns[col_idx].push(val);
        }
        self.row_count += 1;
    }
    
    pub fn get_row(&self, row_idx: usize) -> Vec<Value> {
        let mut row = Vec::with_capacity(self.columns.len());
        for col in &self.columns {
            row.push(col[row_idx].clone());
        }
        row
    }
}

/// The Pro Executor Trait.
/// Instead of `next() -> Option<Tuple>`, it returns `next_batch() -> Option<TupleBatch>`.
pub trait VectorizedExecutor {
    fn init(&mut self);
    fn next_batch(&mut self) -> Option<TupleBatch>;
    fn schema(&self) -> &Schema;
}
