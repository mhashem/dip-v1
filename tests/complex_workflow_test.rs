use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

#[test]
fn test_complex_bulk_workflow_and_atomicity() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    // Setup
    engine.execute("CREATE TABLE sensor_data (id INT PRIMARY KEY, val INT)").unwrap();

    // START A LARGE TRANSACTION FOR THE WHOLE WORKFLOW
    engine.execute("BEGIN").unwrap();

    // 1. Insert 500 items
    println!("--- Step 1: Bulk Insert 500 items ---");
    let start = Instant::now();
    for i in 1..=500 {
        let sql = format!("INSERT INTO sensor_data VALUES ({}, {})", i, i * 10);
        engine.execute(&sql).unwrap();
    }
    let duration = start.elapsed();
    println!("Inserted 500 items in: {:?}", duration);

    // 2. Update 100 existing items
    println!("--- Step 2: Update 100 existing items ---");
    let start = Instant::now();
    for i in 1..=100 {
        let sql = format!("UPDATE sensor_data SET val = {} WHERE id = {}", i * 100, i);
        engine.execute(&sql).unwrap();
    }
    let duration = start.elapsed();
    println!("Updated 100 items in: {:?}", duration);

    // 3. Delete 50 items (25 existing, 25 non-existing)
    println!("--- Step 3: Delete 50 items (25 existing, 25 non-existing) ---");
    let start = Instant::now();
    for i in 101..=125 {
        let sql = format!("DELETE FROM sensor_data WHERE id = {}", i);
        let res = engine.execute(&sql).unwrap();
        assert!(res.contains("Deleted 1 rows"));
    }
    for i in 1001..=1025 {
        let sql = format!("DELETE FROM sensor_data WHERE id = {}", i);
        let res = engine.execute(&sql).unwrap();
        assert!(res.contains("Deleted 0 rows"));
    }
    let duration = start.elapsed();
    println!("Processed 50 deletions in: {:?}", duration);

    // 4. Atomic Insert 100 items with a duplicate in the middle
    // This statement will fail, causing the WHOLE transaction (Steps 1-4) to rollback.
    println!("--- Step 4: Atomic Insert with PK Violation ---");
    let mut insert_sql = String::from("INSERT INTO sensor_data VALUES ");
    for i in 601..=700 {
        if i == 650 {
            insert_sql.push_str("(1, 9999),"); // Duplicate PK!
        } else {
            insert_sql.push_str(&format!("({}, {}),", i, i));
        }
    }
    insert_sql.pop(); // Remove trailing comma

    let res = engine.execute(&insert_sql);
    
    // Check that PK violation is encountered
    match res {
        Err(e) => {
            println!("Caught expected error: {}", e);
            assert!(format!("{:?}", e).contains("PkViolation"), "Error should be PkViolation");
        },
        Ok(_) => panic!("Insert should have failed due to duplicate PK"),
    }

    // 5. Verify Atomicity (The WHOLE DB should be empty now, because Steps 1-4 were rolled back)
    println!("--- Step 5: Verify Full Rollback ---");
    
    // Row 1 should NOT exist (it was inserted in Step 1 which rolled back)
    let check_res = engine.execute("SELECT * FROM sensor_data WHERE id = 1").unwrap();
    assert!(!check_res.contains("Alice"), "Row 1 should not exist"); // Wait, it wouldn't contain Alice anyway, name is id/val
    assert!(!check_res.contains("|               1 |"), "Row 1 should have been rolled back");

    // Table should be empty
    let check_res = engine.execute("SELECT * FROM sensor_data").unwrap();
    // Header only
    assert!(check_res.lines().count() <= 3, "Table should be empty after full rollback. Found: {}", check_res);
    
    println!("--- Full Workflow Atomicity Verified Successfully ---");
}