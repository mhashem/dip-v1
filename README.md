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
- **Concurrency Control (New!):**
  - `TransactionManager`: ACID transaction lifecycle (Begin, Commit, Abort).
  - `LockManager`: Row-level Strict Two-Phase Locking (2PL) with deadlock-free release logic.
  - `Thread Safety`: Use of `Condvar` and `Mutex` for efficient thread synchronization.
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

## Roadmap: Path to Production

The goal is to elevate **DIP-DB** from an educational prototype to a robust, usable embedded database. Future agents should follow this roadmap:

### Phase 4: Transactions & Concurrency (ACID)
*   **Transaction Manager:** Implement `BEGIN`, `COMMIT`, `ROLLBACK`. Track transaction states and assign Transaction IDs (TIDs).
*   **Lock Manager:** Implement strict Two-Phase Locking (2PL) or MVCC (Multi-Version Concurrency Control) to handle concurrent readers and writers safely.
*   **Isolation Levels:** Target `REPEATABLE READ` or `SNAPSHOT ISOLATION`.

### Phase 5: Durability & Recovery
*   **Write-Ahead Logging (WAL):** Implement a log manager. All modifications must be logged before pages are flushed to disk.
*   **ARIES Recovery:** Implement the Analysis, Redo, and Undo phases to ensure database consistency after a crash.
*   **Checkpoints:** Periodic flushing of dirty pages to truncate the log.

### Phase 6: Query Engine Enhancements
*   **Joins:** Implement `NestedLoopJoin` and `HashJoin` executors to support multi-table queries.
*   **Aggregations:** Support `GROUP BY`, `COUNT`, `SUM`, `AVG`.
*   **Sorting:** Implement `ORDER BY` using external merge sort for datasets larger than memory.
*   **Optimizer:** Implement a Cost-Based Optimizer (CBO) utilizing table statistics (Zone Maps) to select the best plan (e.g., Index Scan vs. Seq Scan).

### Phase 7: SQL & Features
*   **UPDATE / DELETE:** Support record modification and deletion (using tombstones in SlottedPage).
*   **Primary Keys:** Explicitly define and enforce uniqueness using B+Trees.
*   **Secondary Indexes:** Allow creating indexes on non-primary columns.
