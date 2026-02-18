use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[test]
fn test_insert_atomicity() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("CREATE TABLE accounts (id INT PRIMARY KEY, balance INT)").unwrap();
    engine.execute("INSERT INTO accounts VALUES (1, 100)").unwrap();
    
    // This should fail and rollback row 3
    let res = engine.execute("INSERT INTO accounts VALUES (3, 100), (1, 500)");
    assert!(res.is_err());

    let select_res = engine.execute("SELECT * FROM accounts WHERE id = 3").unwrap();
    assert!(!select_res.contains("3"), "Row 3 should have been rolled back");
}

#[test]
fn test_update_atomicity() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("CREATE TABLE users (id INT PRIMARY KEY, age INT)").unwrap();
    engine.execute("INSERT INTO users VALUES (1, 20)").unwrap();
    engine.execute("INSERT INTO users VALUES (2, 30)").unwrap();

    // Transaction that updates then fails
    // Note: We need a way to fail AFTER an update.
    // Let's try to update id=1, then insert a duplicate.
    let sql = "UPDATE users SET age = 21 WHERE id = 1; INSERT INTO users VALUES (2, 40);";
    let res = engine.execute(sql);
    assert!(res.is_err());

    // Verify id=1 is still 20, not 21
    let select_res = engine.execute("SELECT * FROM users WHERE id = 1").unwrap();
    assert!(select_res.contains("20"), "Update should have been rolled back. Result: {}", select_res);
    assert!(!select_res.contains("21"));
}

#[test]
fn test_delete_atomicity() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("CREATE TABLE items (id INT PRIMARY KEY, name VARCHAR)").unwrap();
    engine.execute("INSERT INTO items VALUES (1, 'Book')").unwrap();

    // Delete then fail
    let sql = "DELETE FROM items WHERE id = 1; INSERT INTO items VALUES (2, 'Pen'), (2, 'Pencil');";
    let res = engine.execute(sql);
    assert!(res.is_err());

    // Verify id=1 still exists
    let select_res = engine.execute("SELECT * FROM items WHERE id = 1").unwrap();
    assert!(select_res.contains("Book"), "Delete should have been rolled back");
}

#[test]
fn test_explicit_transaction_commit() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("CREATE TABLE test_commit (id INT PRIMARY KEY, val INT)").unwrap();
    engine.execute("BEGIN").unwrap();
    engine.execute("INSERT INTO test_commit VALUES (1, 10)").unwrap();
    engine.execute("INSERT INTO test_commit VALUES (2, 20)").unwrap();
    
    let select_before_commit = engine.execute("SELECT * FROM test_commit").unwrap();
    assert!(select_before_commit.contains("10")); // Should see uncommitted changes

    engine.execute("COMMIT").unwrap();
    
    let select_after_commit = engine.execute("SELECT * FROM test_commit").unwrap();
    assert!(select_after_commit.contains("10"));
    assert!(select_after_commit.contains("20"));
}

#[test]
fn test_explicit_transaction_rollback() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("CREATE TABLE test_rollback (id INT PRIMARY KEY, val INT)").unwrap();
    engine.execute("INSERT INTO test_rollback VALUES (10, 100)").unwrap(); // Auto-committed

    engine.execute("BEGIN").unwrap();
    engine.execute("INSERT INTO test_rollback VALUES (1, 10)").unwrap();
    engine.execute("UPDATE test_rollback SET val = 101 WHERE id = 10").unwrap();
    
    let select_before_rollback = engine.execute("SELECT * FROM test_rollback").unwrap();
    assert!(select_before_rollback.contains("10"));
    assert!(select_before_rollback.contains("101"));

    engine.execute("ROLLBACK").unwrap();
    
    let select_after_rollback = engine.execute("SELECT * FROM test_rollback").unwrap();
    assert!(!select_after_rollback.contains("|               1 |")); // ID 1 should be rolled back
    assert!(select_after_rollback.contains("100")); // Should revert to original
    assert!(!select_after_rollback.contains("101"));
}

#[test]
fn test_nested_transaction_error() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("BEGIN").unwrap();
    let res = engine.execute("BEGIN"); // Attempt to nest
    assert!(res.is_err());
    assert!(format!("{:?}", res).contains("Nested transactions not supported"));

    engine.execute("ROLLBACK").unwrap(); // Clean up
}

#[test]
fn test_commit_without_begin_error() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    let res = engine.execute("COMMIT");
    assert!(res.is_err());
    assert!(format!("{:?}", res).contains("No active transaction to commit"));
}

#[test]
fn test_rollback_without_begin_error() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    let res = engine.execute("ROLLBACK");
    assert!(res.is_err());
    assert!(format!("{:?}", res).contains("No active transaction to rollback"));
}

#[test]
fn test_multi_statement_atomic_success() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("CREATE TABLE multi_stmt (id INT PRIMARY KEY, val INT)").unwrap();
    let sql = "INSERT INTO multi_stmt VALUES (1, 10); UPDATE multi_stmt SET val = 11 WHERE id = 1;";
    let res = engine.execute(sql);
    assert!(res.is_ok());

    let check = engine.execute("SELECT * FROM multi_stmt WHERE id = 1").unwrap();
    assert!(check.contains("11"));
}

#[test]
fn test_multi_statement_atomic_failure_rollback() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("CREATE TABLE multi_fail (id INT PRIMARY KEY, val INT)").unwrap();
    engine.execute("INSERT INTO multi_fail VALUES (1, 10)").unwrap(); // Auto-committed

    let sql = "INSERT INTO multi_fail VALUES (2, 20); INSERT INTO multi_fail VALUES (1, 100);"; // Second insert fails
    let res = engine.execute(sql);
    assert!(res.is_err());

    let check_id_2 = engine.execute("SELECT * FROM multi_fail WHERE id = 2").unwrap();
    assert!(!check_id_2.contains("2"), "Insert of ID 2 should be rolled back");
}
