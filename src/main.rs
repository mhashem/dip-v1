use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use std::sync::Arc;
use std::io::{self, Write};

fn main() {
    println!("DIP-DB: Educational Embedded Database");
    println!("Type 'exit' to quit.");
    
    // Initialize DB
    let path = std::env::current_dir().unwrap().join("dip.db");
    let meta_path = std::env::current_dir().unwrap().join("dip.meta");
    println!("Database file: {:?}", path);
    
    let dm = DiskManager::new(&path).unwrap();
    // Sharded BPM is thread-safe internally
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let mut catalog = CatalogManager::new(bpm);
    
    // Load Metadata
    if let Err(e) = catalog.load_metadata(&meta_path) {
        println!("Warning: Failed to load metadata (may be new DB): {}", e);
    } else {
        println!("Loaded metadata from {:?}", meta_path);
    }

    let mut engine = SQLEngine::new(catalog);

    loop {
        print!("dip-db> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        let input = input.trim();
        if input.eq_ignore_ascii_case("exit") {
             // Save Metadata on Exit
             if let Err(e) = engine.catalog.save_metadata(&meta_path) {
                 println!("Error saving metadata: {}", e);
             } else {
                 println!("Saved metadata to {:?}", meta_path);
             }
            break;
        }
        
        if input.is_empty() {
            continue;
        }

        match engine.execute(input) {
            Ok(output) => println!("{}", output),
            Err(e) => println!("Error: {}", e),
        }
    }
}
