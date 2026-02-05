use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[test]
fn test_secondary_index() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    // 1. Create and Insert
    engine.execute("CREATE TABLE users (id INT PRIMARY KEY, age INT, score INT)").unwrap();
    engine.execute("INSERT INTO users VALUES (1, 25, 100)").unwrap();
    engine.execute("INSERT INTO users VALUES (2, 30, 200)").unwrap();
    
    // 2. Create Secondary Index on 'age'
    engine.execute("CREATE INDEX idx_age ON users (age)").unwrap();
    
    // 3. Verify Index is used (hard to verify purely via output, but we can verify it works)
    let res = engine.execute("SELECT * FROM users WHERE age = 30").unwrap();
    assert!(res.contains("30"));
    assert!(res.contains("200"));

    // 4. Insert more data (ensure index is updated)
    engine.execute("INSERT INTO users VALUES (3, 35, 300)").unwrap();
    let res = engine.execute("SELECT * FROM users WHERE age = 35").unwrap();
    assert!(res.contains("300"));

    // 5. Update data (ensure index is updated)
    engine.execute("UPDATE users SET age = 40 WHERE id = 3").unwrap();
    
    // Old age search should fail
    let res = engine.execute("SELECT * FROM users WHERE age = 35").unwrap();
    assert!(!res.contains("300"));
    
    // New age search should succeed
    let res = engine.execute("SELECT * FROM users WHERE age = 40").unwrap();
    assert!(res.contains("300"));
}
