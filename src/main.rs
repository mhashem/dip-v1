use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use std::sync::{Arc, Mutex};
use std::io::{self, Write};

fn main() {
    println!("DIP-DB: Educational Embedded Database");
    println!("Type 'exit' to quit.");
    
    // Initialize DB
    let path = std::env::current_dir().unwrap().join("dip.db");
    println!("Database file: {:?}", path);
    
    let dm = DiskManager::new(&path).unwrap();
    let bpm = Arc::new(Mutex::new(BufferPoolManager::new(100, dm)));
    let catalog = CatalogManager::new(bpm);
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
