use crate::types::TypeId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub type_id: TypeId,
    // Note: In advanced DBs, we'd store byte offsets here. 
    // For now, we keep it simple.
}

impl Column {
    pub fn new(name: impl Into<String>, type_id: TypeId) -> Self {
        Self {
            name: name.into(),
            type_id,
        }
    }

    /// Returns the fixed size of this column if applicable.
    /// Integers are 4 bytes, Booleans 1 byte.
    /// Varchars are variable (so we return 0 or a length prefix size).
    pub fn fixed_len(&self) -> usize {
        match self.type_id {
            TypeId::Integer => 4,
            TypeId::Boolean => 1,
            TypeId::Varchar => 4, // Just the length prefix
        }
    }
}
