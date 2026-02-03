use crate::types::{Value, TypeId};
use crate::execution::expression::{Expression, BinaryOperator};
use std::collections::HashMap;
use std::convert::TryInto;

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

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // TypeId (u8) - assume min/max have same type
        let type_code = match self.min.get_type_id() {
            TypeId::Integer => 0,
            TypeId::Boolean => 1,
            TypeId::Varchar => 2,
        };
        bytes.push(type_code);
        
        // Min
        bytes.extend(self.min.to_bytes());
        // Max
        bytes.extend(self.max.to_bytes());
        
        bytes
    }

    pub fn from_bytes(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 1 { return None; }
        let type_code = data[0];
        let type_id = match type_code {
            0 => TypeId::Integer,
            1 => TypeId::Boolean,
            2 => TypeId::Varchar,
            _ => return None,
        };
        
        let mut offset = 1;
        
        // Helper to read value knowing type
        // Value::from_bytes doesn't return consumed len for Fixed types easily if we don't know it.
        // But we implemented fixed_len logic? No, Value::from_bytes takes slice.
        // We need to know how many bytes to slice.
        
        let (min, len1) = Self::read_value(&data[offset..], type_id)?;
        offset += len1;
        
        let (max, len2) = Self::read_value(&data[offset..], type_id)?;
        offset += len2;
        
        Some((Self { min, max }, offset))
    }

    fn read_value(data: &[u8], type_id: TypeId) -> Option<(Value, usize)> {
         match type_id {
            TypeId::Integer => {
                if data.len() < 4 { return None; }
                Some((Value::from_bytes(data, type_id), 4))
            }
            TypeId::Boolean => {
                if data.len() < 1 { return None; }
                Some((Value::from_bytes(data, type_id), 1))
            }
            TypeId::Varchar => {
                if data.len() < 4 { return None; }
                let len = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
                if data.len() < 4 + len { return None; }
                Some((Value::from_bytes(data, type_id), 4 + len))
            }
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

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Num entries
        bytes.extend_from_slice(&(self.columns.len() as u32).to_le_bytes());
        
        for (col_idx, stats) in &self.columns {
            bytes.extend_from_slice(&(*col_idx as u32).to_le_bytes());
            bytes.extend(stats.to_bytes());
        }
        bytes
    }

    pub fn from_bytes(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 { return None; }
        let num_entries = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
        let mut offset = 4;
        let mut columns = HashMap::new();
        
        for _ in 0..num_entries {
            if offset + 4 > data.len() { return None; }
            let col_idx = u32::from_le_bytes(data[offset..offset+4].try_into().ok()?) as usize;
            offset += 4;
            
            let (stats, len) = ColumnStats::from_bytes(&data[offset..])?;
            columns.insert(col_idx, stats);
            offset += len;
        }
        
        Some((Self { columns }, offset))
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
