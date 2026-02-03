use crate::types::TypeId;
use std::convert::TryInto;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub type_id: TypeId,
    pub is_primary: bool,
    // Note: In advanced DBs, we'd store byte offsets here. 
    // For now, we keep it simple.
}

impl Column {
    pub fn new(name: impl Into<String>, type_id: TypeId) -> Self {
        Self {
            name: name.into(),
            type_id,
            is_primary: false,
        }
    }

    pub fn new_with_primary(name: impl Into<String>, type_id: TypeId, is_primary: bool) -> Self {
        Self {
            name: name.into(),
            type_id,
            is_primary,
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

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Name length (u32)
        bytes.extend_from_slice(&(self.name.len() as u32).to_le_bytes());
        // Name bytes
        bytes.extend_from_slice(self.name.as_bytes());
        // TypeId (u8)
        let type_code = match self.type_id {
            TypeId::Integer => 0,
            TypeId::Boolean => 1,
            TypeId::Varchar => 2,
        };
        bytes.push(type_code);
        // Is Primary (u8) - 1 or 0
        bytes.push(if self.is_primary { 1 } else { 0 });
        bytes
    }

    pub fn from_bytes(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 { return None; }
        
        let name_len = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        // name + type(1) + primary(1)
        if data.len() < 4 + name_len + 2 { return None; }
        
        let name_bytes = &data[4..4+name_len];
        let name = String::from_utf8(name_bytes.to_vec()).ok()?;
        
        let type_code = data[4+name_len];
        let type_id = match type_code {
            0 => TypeId::Integer,
            1 => TypeId::Boolean,
            2 => TypeId::Varchar,
            _ => return None,
        };
        
        let is_primary = data[4+name_len+1] == 1;
        
        Some((Self {
            name,
            type_id,
            is_primary,
        }, 4 + name_len + 2))
    }
}
