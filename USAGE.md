# DIP-DB Usage Guide

This guide demonstrates how to integrate and use the **DIP-DB** engine within your Rust application.

## Prerequisites

Ensure you have `dip-v1` as a dependency in your `Cargo.toml`.

```toml
[dependencies]
dip-v1 = { path = "../dip-v1" } # Adjust path as necessary
```

## Setup

Every example below assumes you have the following basic setup to initialize the engine components:

```rust
use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use std::sync::{Arc, Mutex};
use tempfile::NamedTempFile;

fn setup_engine() -> SQLEngine {
    let temp_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(temp_file.path()).unwrap();
    let bpm = Arc::new(Mutex::new(BufferPoolManager::new(100, dm)));
    let catalog = CatalogManager::new(bpm);
    SQLEngine::new(catalog)
}
```

---

## Example 1: Creating a Table

This example shows how to create a simple table using standard SQL syntax.

```rust
fn example_create_table() {
    let mut engine = setup_engine();
    
    let sql = "CREATE TABLE users (id INT, name VARCHAR, active BOOLEAN)";
    match engine.execute(sql) {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

**Output:**
```text
Table created.
```

---

## Example 2: Inserting Data

Insert rows into the table. The engine automatically updates Zone Maps and B+Tree indices (if applicable).

```rust
fn example_insert_data() {
    let mut engine = setup_engine();
    engine.execute("CREATE TABLE users (id INT, name VARCHAR)").unwrap();
    
    let sql = "INSERT INTO users VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie')";
    match engine.execute(sql) {
        Ok(output) => println!("{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

**Output:**
```text
Inserted 3 rows.
```

---

## Example 3: Sequential Scan with Filter (Zone Map Pruning)

When you query with a filter (e.g., `WHERE age > 25`), the engine uses **Zone Maps** to skip entire pages of data that cannot possibly match the condition, significantly speeding up scans on large datasets.

```rust
fn example_query_filter() {
    let mut engine = setup_engine();
    engine.execute("CREATE TABLE items (price INT, name VARCHAR)").unwrap();
    
    // Insert enough data to fill multiple pages (simulated)
    engine.execute("INSERT INTO items VALUES (10, 'Cheap'), (100, 'Expensive')").unwrap();
    
    let sql = "SELECT * FROM items WHERE price > 50";
    match engine.execute(sql) {
        Ok(output) => println!("Query Results:\n{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

---

## Example 4: Index Scan Optimization (B+Tree)

If a table's first column is an `INT` (Primary Key), a **B+Tree Index** is automatically created. Queries filtering on this column with equality (`=`) will use the index for O(log N) lookup instead of a sequential scan.

```rust
fn example_index_lookup() {
    let mut engine = setup_engine();
    // 'id' is the first column and INT, so it gets an index.
    engine.execute("CREATE TABLE products (id INT, sku VARCHAR)").unwrap();
    
    engine.execute("INSERT INTO products VALUES (101, 'ABC-1'), (102, 'ABC-2')").unwrap();
    
    // This query triggers the IndexScanExecutor optimization
    let sql = "SELECT * FROM products WHERE id = 102";
    match engine.execute(sql) {
        Ok(output) => println!("Index Lookup Results:\n{}", output),
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

---

## Example 5: Persistence (Save & Load)

Metadata (Schema, Zone Maps, Index Roots) can be saved to disk and reloaded later.

```rust
use std::path::Path;

fn example_persistence() {
    let db_path = Path::new("mydb.db");
    let meta_path = Path::new("mydb.meta");
    
    // 1. Create and populate
    {
        let dm = DiskManager::new(db_path).unwrap();
        let bpm = Arc::new(Mutex::new(BufferPoolManager::new(100, dm)));
        let catalog = CatalogManager::new(bpm);
        let mut engine = SQLEngine::new(catalog);
        
        engine.execute("CREATE TABLE config (key INT, val VARCHAR)").unwrap();
        engine.execute("INSERT INTO config VALUES (1, 'dark_mode')").unwrap();
        
        // Save metadata
        engine.catalog.save_metadata(meta_path).unwrap();
    }
    
    // 2. Load back
    {
        let dm = DiskManager::new(db_path).unwrap();
        let bpm = Arc::new(Mutex::new(BufferPoolManager::new(100, dm)));
        let mut catalog = CatalogManager::new(bpm);
        
        // Load metadata
        catalog.load_metadata(meta_path).unwrap();
        let mut engine = SQLEngine::new(catalog);
        
        // Query exists!
        let output = engine.execute("SELECT * FROM config").unwrap();
        println!("Restored Data:\n{}", output);
    }
}
```
