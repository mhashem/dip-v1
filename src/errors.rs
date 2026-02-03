use thiserror::Error;

#[derive(Error, Debug)]
pub enum DipError {
    // 1xxx: Parse
    #[error("[1001] Syntax Error: {0}")]
    ParseError(String),
    #[error("[1002] Unsupported Statement: {0}")]
    Unsupported(String),

    // 2xxx: Catalog
    #[error("[2001] Table Not Found: {0}")]
    TableNotFound(String),
    #[error("[2002] Table Already Exists: {0}")]
    TableAlreadyExists(String),
    #[error("[2003] Column Not Found: {0}")]
    ColumnNotFound(String),

    // 3xxx: Execution
    #[error("[3001] Type Mismatch: {0}")]
    TypeMismatch(String),
    #[error("[3002] Primary Key Violation: Duplicate key {0}")]
    PkViolation(String),
    #[error("[3003] Unique Constraint Violation: {0}")]
    UniqueViolation(String),
    #[error("[3004] Transaction Aborted")]
    TransactionAborted,

    // 4xxx: Storage & IO
    #[error("[4001] IO Error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("[4003] Buffer Pool Full")]
    BufferPoolFull,
}
