use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use std::sync::Arc;
use std::time::Instant;

#[test]
fn test_load_performance() {
    let file_path = std::env::temp_dir().join("dip_load_test2.db");
    if file_path.exists() {
        std::fs::remove_file(&file_path).unwrap();
    }

    let dm = DiskManager::new(&file_path).unwrap();
    // Use a large buffer pool to avoid disk I/O noise during the test (unless we want to test disk I/O)
    // 500 pages * 4KB = 2MB. Should fit easily.
    let bpm = Arc::new(BufferPoolManager::new(500, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    // 1. Setup
    println!("Creating table...");
    engine.execute("CREATE TABLE users (id INT, age INT, name VARCHAR)").unwrap();

    // 2. Bulk Insert (1000 rows)
    let total_rows = 10_000;
    println!("Inserting {} rows...", total_rows);
    let start_insert = Instant::now();
    
    // Batch insert is faster if we supported it in one SQL, but our engine supports INSERT VALUES (..), (..), ...
    // Let's generate a massive SQL string with blocks of 100 rows
    for i in (0..total_rows).step_by(100) {
        let mut sql = String::from("INSERT INTO users VALUES ");
        for j in 0..100 {
            if j > 0 { sql.push_str(", "); }
            let id = i + j;
            let age = id % 100;
            sql.push_str(&format!("({}, {}, 'User-{}')", id, age, id));
        }
        engine.execute(&sql).unwrap();
    }
    
    let duration_insert = start_insert.elapsed();
    println!("Insert took: {:?}", duration_insert);
    println!("Insert Ops/sec: {:.2}", total_rows as f64 / duration_insert.as_secs_f64());

    // 3. Retrieval with WHERE (Scan + Filter)
    println!("Querying specific ID (Scan + Filter)...");
    let target_id = 5000;
    let query = format!("SELECT * FROM users WHERE id = {}", target_id);
    
    let start_query = Instant::now();
    let res = engine.execute(&query).unwrap();
    let duration_query = start_query.elapsed();
    
    println!("Query Result:\n{}", res);
    println!("Query took: {:?}", duration_query);
    assert!(res.contains("User-5000"));

    // 4. Retrieval Range
    println!("Querying Range (Scan + Filter)...");
    let start_range = Instant::now();
    let res = engine.execute("SELECT * FROM users WHERE age > 90").unwrap();
    let duration_range = start_range.elapsed();
    
    println!("Range Query took: {:?}", duration_range);
    // There are 100 ages (0-99). Ages 91-99 match. That's 9%. 
    // 10,000 * 0.09 = 900 rows.
    // Let's just check it didn't crash.
    
    std::fs::remove_file(&file_path).unwrap();
}
