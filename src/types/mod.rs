use std::convert::TryInto;

/// This enum represents the types of values our database supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeId {
    Integer,
    Boolean,
    Varchar,
}

/// The `Value` enum holds the actual data.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Value {
    Integer(i32),
    Boolean(bool),
    Varchar(String),
}

impl Value {
    /// Returns the TypeId of this value.
    pub fn get_type_id(&self) -> TypeId {
        match self {
            Self::Integer(_) => TypeId::Integer,
            Self::Boolean(_) => TypeId::Boolean,
            Self::Varchar(_) => TypeId::Varchar,
        }
    }

    /// Serialization: Convert the Value into a byte vector.
    /// 
    /// Format:
    /// - Integer: 4 bytes (Little Endian)
    /// - Boolean: 1 byte (1 for true, 0 for false)
    /// - Varchar: 4 bytes length + N bytes data
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Integer(v) => v.to_le_bytes().to_vec(),
            Self::Boolean(b) => vec![if *b { 1 } else { 0 }],
            Self::Varchar(s) => {
                let bytes = s.as_bytes();
                let len = bytes.len() as u32;
                let mut res = len.to_le_bytes().to_vec();
                res.extend_from_slice(bytes);
                res
            }
        }
    }

    /// Deserialization: Create a Value from bytes based on the TypeId.
    pub fn from_bytes(data: &[u8], type_id: TypeId) -> Self {
        match type_id {
            TypeId::Integer => {
                let bytes: [u8; 4] = data[0..4].try_into().expect("Slice to array failed");
                Self::Integer(i32::from_le_bytes(bytes))
            }
            TypeId::Boolean => {
                let val = data[0];
                Self::Boolean(val != 0)
            }
            TypeId::Varchar => {
                let len_bytes: [u8; 4] = data[0..4].try_into().expect("Slice to array failed");
                let len = u32::from_le_bytes(len_bytes) as usize;
                let str_bytes = &data[4..4 + len];
                let s = String::from_utf8(str_bytes.to_vec()).expect("Invalid UTF-8");
                Self::Varchar(s)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer() {
        let v = Value::Integer(42);
        assert_eq!(v.get_type_id(), TypeId::Integer);
        
        let bytes = v.to_bytes();
        assert_eq!(bytes.len(), 4);
        let v2 = Value::from_bytes(&bytes, TypeId::Integer);
        assert_eq!(v, v2);
    }

    #[test]
    fn test_boolean() {
        let v = Value::Boolean(true);
        assert_eq!(v.get_type_id(), TypeId::Boolean);

        let bytes = v.to_bytes();
        assert_eq!(bytes.len(), 1);
        assert_eq!(bytes[0], 1);
        let v2 = Value::from_bytes(&bytes, TypeId::Boolean);
        assert_eq!(v, v2);
    }

    #[test]
    fn test_varchar() {
        let v = Value::Varchar("hello".to_string());
        assert_eq!(v.get_type_id(), TypeId::Varchar);

        let bytes = v.to_bytes();
        // 4 bytes length + 5 bytes "hello" = 9 bytes
        assert_eq!(bytes.len(), 9); 
        let v2 = Value::from_bytes(&bytes, TypeId::Varchar);
        assert_eq!(v, v2);
    }
}