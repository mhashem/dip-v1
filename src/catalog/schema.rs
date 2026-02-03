use crate::catalog::column::Column;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub columns: Vec<Column>,
}

impl Schema {
    pub fn new(columns: Vec<Column>) -> Self {
        Self { columns }
    }

    /// TASK 1: Calculate the total fixed size of the schema.
    /// Iterate through all columns and sum their `fixed_len()`.
    /// 
    /// Example: 
    /// let total: usize = self.columns.iter().map(|c| c.fixed_len()).sum();
    pub fn fixed_len(&self) -> usize {
        let total: usize = self.columns.iter().map(|c| c.fixed_len()).sum();
        total
    }

    /// TASK 2: Find a column index by its name.
    /// Returns Some(index) if found, None otherwise.
    pub fn get_col_index(&self, name: &str) -> Option<usize> {
        // Example: 
        // self.columns.iter().position(|c| c.name == name)
        self.columns.iter().position(|c| c.name == name)
    }
    
    /// Returns the number of columns in the schema.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Returns the index of the primary key column, if any.
    pub fn get_primary_key_index(&self) -> Option<usize> {
        self.columns.iter().position(|c| c.is_primary)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Num columns (u32)
        bytes.extend_from_slice(&(self.columns.len() as u32).to_le_bytes());
        for col in &self.columns {
            bytes.extend(col.to_bytes());
        }
        bytes
    }

    pub fn from_bytes(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 { return None; }
        
        let num_cols = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
        let mut offset = 4;
        let mut columns = Vec::with_capacity(num_cols);
        
        for _ in 0..num_cols {
            if offset >= data.len() { return None; }
            let (col, len) = Column::from_bytes(&data[offset..])?;
            columns.push(col);
            offset += len;
        }
        
        Some((Self::new(columns), offset))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TypeId;

    #[test]
    fn test_schema_basics() {
        let col1 = Column::new("id", TypeId::Integer);
        let col2 = Column::new("is_active", TypeId::Boolean);
        let col3 = Column::new("name", TypeId::Varchar);
        let schema = Schema::new(vec![col1, col2, col3]);

        // Once Task 1 is done, this should pass
        assert_eq!(schema.fixed_len(), 9); // 4 + 1

        // Once Task 2 is done, this should pass
        assert_eq!(schema.get_col_index("id"), Some(0));
        assert_eq!(schema.get_col_index("is_active"), Some(1));
        assert_eq!(schema.get_col_index("name"), Some(2));
        assert_eq!(schema.get_col_index("missing"), None);
    }
}
