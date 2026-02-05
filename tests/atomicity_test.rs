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