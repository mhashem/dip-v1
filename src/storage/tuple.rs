use crate::catalog::schema::Schema;
use crate::types::{Value, TypeId};

#[derive(Debug, Clone)]
pub struct Tuple {
    pub data: Vec<u8>,
}

impl Tuple {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// TASK 1: Construct a Tuple from a list of Values based on a Schema.
    /// 1. Verify values.len() == schema.column_count().
    /// 2. Iterate through values, serialize each to bytes, and collect them.
    pub fn from_values(values: Vec<Value>, schema: &Schema) -> Self {
        assert_eq!(values.len(), schema.column_count(), "Value count mismatch");
        
        let mut data = Vec::new();
        for val in values {
            // TASK 1.1: Call val.to_bytes() and push the results into `data`
            // Example: data.extend(val.to_bytes());
            data.extend(val.to_bytes());
        }

        Self { data }
    }

    /// TASK 2: Extract a specific Value from the raw bytes using the Schema.
    /// This requires "walking" through the bytes because Varchars have variable lengths.
    pub fn get_value(&self, schema: &Schema, col_idx: usize) -> Value {
        let mut offset = 0;

        // 1. Iterate through columns UP TO the one we want (col_idx)
        for i in 0..col_idx {
            let col = &schema.columns[i];
            
            // 2. We need to skip the bytes of the previous columns
            match col.type_id {
                TypeId::Integer => offset += 4,
                TypeId::Boolean => offset += 1,
                TypeId::Varchar => {
                    // For Varchar, we read the 4-byte length prefix to know how many more bytes to skip
                    let len_bytes: [u8; 4] = self.data[offset..offset+4].try_into().unwrap();
                    let len = u32::from_le_bytes(len_bytes) as usize;
                    offset += 4 + len;
                }
            }
        }

        // 3. Now 'offset' is at the start of our target column!
        let target_col = &schema.columns[col_idx];
        
        // TASK 2.1: Call Value::from_bytes with the data starting at `offset`.
        // Note: You only need to pass the slice starting at offset: &self.data[offset..]
        // Example: Value::from_bytes(&self.data[offset..], target_col.type_id)
        Value::from_bytes(&self.data[offset..], target_col.type_id)
    }
    
    pub fn size(&self) -> usize {
        self.data.len()
    }
    
    pub fn to_bytes(&self) -> &[u8] {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::column::Column;
    use crate::types::TypeId;

    #[test]
    fn test_tuple_from_values() {
        let schema = Schema::new(vec![
            Column::new("id", TypeId::Integer),
            Column::new("name", TypeId::Varchar),
            Column::new("is_active", TypeId::Boolean),
        ]);

        let values = vec![
            Value::Integer(1),
            Value::Varchar("Alice".to_string()),
            Value::Boolean(true),
        ];

        let tuple = Tuple::from_values(values.clone(), &schema);
        
        // Once Task 2 is done, this will verify everything
        assert_eq!(tuple.get_value(&schema, 0), values[0]);
        assert_eq!(tuple.get_value(&schema, 1), values[1]);
        assert_eq!(tuple.get_value(&schema, 2), values[2]);
    }
}