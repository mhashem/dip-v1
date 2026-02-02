use crate::types::Value;
use crate::execution::expression::{Expression, BinaryOperator};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ColumnStats {
    pub min: Value,
    pub max: Value,
}

impl ColumnStats {
    pub fn new(val: Value) -> Self {
        Self {
            min: val.clone(),
            max: val,
        }
    }

    pub fn update(&mut self, val: &Value) {
        if val < &self.min {
            self.min = val.clone();
        }
        if val > &self.max {
            self.max = val.clone();
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PageStats {
    // Key: Column Index
    pub columns: HashMap<usize, ColumnStats>,
}

impl PageStats {
    pub fn new() -> Self {
        Self {
            columns: HashMap::new(),
        }
    }

    pub fn update(&mut self, col_idx: usize, val: &Value) {
        self.columns.entry(col_idx)
            .and_modify(|stats| stats.update(val))
            .or_insert_with(|| ColumnStats::new(val.clone()));
    }

    /// Checks if this page *might* contain a tuple satisfying the predicate.
    /// Returns:
    /// - true: The page MIGHT contain a match (MUST READ).
    /// - false: The page DEFINITELY does NOT contain a match (SKIP).
    pub fn might_satisfy(&self, predicate: &Expression) -> bool {
        match predicate {
            Expression::Binary { left, op, right } => {
                // We only optimize simple cases: Column <Op> Constant
                // e.g., age > 90
                
                if let (Expression::Column(idx), Expression::Constant(const_val)) = (left.as_ref(), right.as_ref()) {
                    if let Some(stats) = self.columns.get(idx) {
                        match op {
                            // col = val:  Range [min, max] must contain val
                            BinaryOperator::Eq => const_val >= &stats.min && const_val <= &stats.max,
                            // col > val:  Max must be > val
                            BinaryOperator::Gt => &stats.max > const_val,
                            // col >= val: Max must be >= val
                            BinaryOperator::GtEq => &stats.max >= const_val,
                            // col < val:  Min must be < val
                            BinaryOperator::Lt => &stats.min < const_val,
                            // col <= val: Min must be <= val
                            BinaryOperator::LtEq => &stats.min <= const_val,
                            // For NotEq, we generally can't prune efficiently unless range is a single point
                            _ => true, 
                        }
                    } else {
                        // No stats for this column? Assume true.
                        true
                    }
                } else {
                    // Complex expression? Assume true.
                    true
                }
            }
            // For constants/columns directly? Assume true.
            _ => true,
        }
    }
}
