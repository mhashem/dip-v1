use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;

#[test]
fn test_primary_key_definition() {
    let temp_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(temp_file.path()).unwrap();
    let bpm = Arc::new(Mutex::new(BufferPoolManager::new(100, dm)));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    // 1. Create table with Primary Key
    let sql = "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR)";
    engine.execute(sql).expect("Create table failed");

    let table = engine.catalog.get_table("users").expect("Table not found");
    
    // Check Schema
    let id_col = &table.schema.columns[0];
    assert!(id_col.is_primary, "ID column should be primary key");
    
    let name_col = &table.schema.columns[1];
    assert!(!name_col.is_primary, "Name column should NOT be primary key");

    // Check Index creation
    assert!(table.index.is_some(), "Index should be created for Primary Key");

    // 2. Create table WITHOUT Primary Key
    let sql2 = "CREATE TABLE logs (msg VARCHAR, level INT)";
    engine.execute(sql2).expect("Create table logs failed");
    
    let logs_table = engine.catalog.get_table("logs").expect("Table logs not found");
    assert!(!logs_table.schema.columns[0].is_primary);
    assert!(!logs_table.schema.columns[1].is_primary);
    assert!(logs_table.index.is_none(), "Index should NOT be created if no Primary Key");
}
