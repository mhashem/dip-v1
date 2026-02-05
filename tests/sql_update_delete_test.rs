use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[test]
fn test_sql_update_delete() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    // 1. Create and Insert
    engine.execute("CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR, age INT)").unwrap();
    engine.execute("INSERT INTO users VALUES (1, 'Alice', 25)").unwrap();
    engine.execute("INSERT INTO users VALUES (2, 'Bob', 30)").unwrap();
    engine.execute("INSERT INTO users VALUES (3, 'Charlie', 35)").unwrap();

    // 2. Update Alice's age
    let res = engine.execute("UPDATE users SET age = 26 WHERE id = 1").unwrap();
    println!("Update Alice: {}", res);
    assert_eq!(res, "Updated 1 rows.\n");

    // Verify Update
    let res = engine.execute("SELECT * FROM users WHERE id = 1").unwrap();
    println!("Select Alice: {}", res);
    assert!(res.contains("26"));

    // 3. Delete Bob
    let res = engine.execute("DELETE FROM users WHERE name = 'Bob'").unwrap();
    println!("Delete Bob: {}", res);
    assert_eq!(res, "Deleted 1 rows.\n");

    // Verify Delete
    let res = engine.execute("SELECT * FROM users").unwrap();
    println!("Select all: {}", res);
    assert!(!res.contains("Bob"));
    assert!(res.contains("Alice"));
    assert!(res.contains("Charlie"));

    // 4. Update all (no WHERE)
    let res = engine.execute("UPDATE users SET age = 40").unwrap();
    println!("Update all: {}", res);
    assert_eq!(res, "Updated 2 rows.\n");

    // Verify
    let res = engine.execute("SELECT * FROM users WHERE age = 40").unwrap();
    println!("Select age 40: {}", res);
    assert!(res.contains("Alice"));
    assert!(res.contains("Charlie"));
}