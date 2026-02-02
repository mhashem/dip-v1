# DIP-DB (Data Intensive Project Database)

This project is an educational, embedded database built from scratch in Rust. It draws inspiration from "Designing Data-Intensive Applications" (DDIA) and modern OLAP databases like DuckDB.

## Features Implemented
- **Storage Engine:**
  - `DiskManager`: Raw file I/O (4KB pages).
  - `BufferPoolManager`: LRU Caching and Dirty Page Flushing.
  - `SlottedPage`: Variable-length tuple storage.
  - `TableHeap`: Linked list of pages.
- **Execution Engine:**
  - `Type System`: Integer, Boolean, Varchar serialization.
  - `Catalog`: Schema definition and Table registry.
  - `Executors`: SeqScan and Insert operators.
- **Frontend:**
  - `SQL Parser`: Powered by `sqlparser-rs`.
  - `REPL`: Interactive command-line interface.

## How to Run (Playground)

1. **Build and Run the REPL:**
   ```bash
   cargo run
   ```

2. **Execute SQL Commands:**
   ```sql
   create table users (id int, name varchar, active boolean)
   insert into users values (1, 'Alice', true)
   insert into users values (2, 'Bob', false)
   select * from users
   ```

3. **Persistence:**
   Type `exit` to quit. The data is saved to `dip.db` in the current directory.
   Run `cargo run` again, and your data will still be there!

## Project Structure
- `src/storage`: Disk, Buffer, Pages.
- `src/types`: Value system.
- `src/catalog`: Metadata (Schema, Columns).
- `src/execution`: Query operators.
- `src/sql`: Parsing and Binding.

## Future Phases (Advanced)
- [ ] **Transactions (ACID):** Transaction Manager and Lock Manager.
- [ ] **Concurrency Control:** MVCC.
- [ ] **Recovery:** Write-Ahead Logging (WAL).
