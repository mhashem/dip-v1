use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use std::sync::{Arc, Mutex};

#[test]
fn test_sql_engine_end_to_end() {
    let file_path = std::env::current_dir().unwrap().join("dip_sql_test.db");
    if file_path.exists() {
       std::fs::remove_file(&file_path).unwrap();
    }

    let dm = DiskManager::new(&file_path).unwrap();
    let bpm = Arc::new(Mutex::new(BufferPoolManager::new(100, dm)));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    // 1. Create Table
    let res = engine.execute("CREATE TABLE users (id INT, active BOOLEAN, name VARCHAR)");
    assert!(res.is_ok());

    // 2. Insert Data
    let res = engine.execute("INSERT INTO users VALUES (1, true, 'Alice')");
    assert!(res.is_ok());
    let res = engine.execute("INSERT INTO users VALUES (2, false, 'Bob')");
    assert!(res.is_ok());

    // 3. Query Data
    let res = engine.execute("SELECT * FROM users");
    assert!(res.is_ok());
    let output = res.unwrap();
    
    println!("Query Output:\n{}", output);
    
    assert!(output.contains("Alice"));
    assert!(output.contains("Bob"));
    assert!(output.contains("true"));
    assert!(output.contains("false"));
}
