# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`dip-v1` is an educational embedded database engine written in Rust, inspired by DDIA and DuckDB. It implements a full database stack from raw disk I/O up through a SQL REPL.

## Commands

```bash
# Run the interactive REPL (persists to dip.db / dip.meta)
cargo run

# Build
cargo build

# Run all tests
cargo test

# Run a single test by name
cargo test test_insert_atomicity

# Run all tests in a file
cargo test --test atomicity_test
```

Tests use `tempfile::NamedTempFile` for isolated scratch databases—no test fixtures need setup.

## Architecture

The stack is layered: **SQL → Execution → Catalog → Storage → Disk**

### Storage (`src/storage/`)
- **`DiskManager`** — raw file I/O using fixed 4KB pages (little-endian `PageId` = `u64`)
- **`BufferPoolManager`** — 64-shard LRU cache wrapping `BufferPoolInstance`; sharded by `hash(page_id) % 64` to reduce mutex contention. Flushes all dirty pages on `Drop`.
- **`SlottedPage`** — variable-length tuple storage within a 4KB page; supports tombstone-based soft deletion (`mark_delete`) and rollback (`rollback_delete`)
- **`TableHeap`** — linked list of `SlottedPage`s; provides `insert_tuple`, `get_tuple`, `mark_delete`, `rollback_delete`, and an `Iterator`
- **`BPlusTree`** (`src/storage/index/`) — B+Tree backed by the BPM; maps `i64` keys → `RID`; supports `insert`, `get_value`, `update_value`, `delete`

### Catalog (`src/catalog/`)
- **`CatalogManager`** — owns the BPM and a thread-safe `HashMap<String, Arc<TableMetadata>>`
- **`TableMetadata`** — bundles `Schema`, `TableHeap`, a `RwLock<HashMap<col_idx, Arc<RwLock<BPlusTree>>>>` for indexes, and `RwLock<HashMap<PageId, PageStats>>` for zone maps
- **Persistence** — `save_metadata` / `load_metadata` write a custom binary format (magic `DIPM`) to `dip.meta`; raw page data lives in `dip.db`

### Execution (`src/execution/`)
All executors implement the Volcano iterator trait:
```rust
trait Executor {
    fn init(&mut self);
    fn next(&mut self) -> Option<Tuple>;
    fn schema(&self) -> &Schema;
}
```
`ExecutorContext` carries `Arc<TableMetadata>`, `Arc<Mutex<Transaction>>`, and `Arc<LockManager>`.

Executors: `SeqScanExecutor`, `FilterExecutor`, `InsertExecutor`, `UpdateExecutor`, `DeleteExecutor`, `IndexScanExecutor`.

**Important limitation:** `next()` returns `Option<Tuple>` (not `Result`). Errors inside executors are signaled by returning fewer tuples than expected; the SQL engine detects this by comparing counts.

### SQL Engine (`src/sql/engine.rs`)
`SQLEngine` parses SQL via `sqlparser-rs` (`GenericDialect`) and dispatches to executors. Key behaviors:
- **Auto-commit:** A single `execute()` call with no explicit `BEGIN` wraps all statements in one auto-committed transaction.
- **Multi-statement atomicity:** Multiple statements in one `execute()` call share a single transaction and roll back together on any error.
- **Explicit transactions:** `BEGIN` / `COMMIT` / `ROLLBACK` span multiple `execute()` calls via `self.current_txn`.
- **Query optimization:** `SELECT ... WHERE col = val` uses an `IndexScanExecutor` if an index on that column exists.

Supported SQL: `CREATE TABLE`, `INSERT INTO ... VALUES`, `SELECT * FROM ... WHERE`, `UPDATE ... SET ... WHERE`, `DELETE FROM ... WHERE`, `CREATE INDEX ON`, `BEGIN`, `COMMIT`, `ROLLBACK`.

Supported types: `INT` / `INTEGER`, `BOOLEAN`, `VARCHAR` / `STRING`. Only integer-keyed indexes are supported.

### Concurrency (`src/concurrency/`)
- **`TransactionManager`** — assigns monotonically increasing `TxnId`s, maintains active transaction registry, delegates lock release to `LockManager` on commit/abort
- **`LockManager`** — 64-shard sharded lock table (`hash(RID) % 64`) with per-shard `Condvar`; implements Strict 2PL (row-level shared/exclusive locks held until commit); supports lock upgrade (S→X) when no other holders exist
- **`Transaction`** — tracks `shared_locks`, `exclusive_locks`, and a `write_set: Vec<WriteRecord>` used for undo-based rollback
- **Rollback logic** lives in `SQLEngine::rollback()`, which iterates `write_set` in reverse to undo inserts, deletes, and updates in both the heap and all affected indexes

### Errors (`src/errors.rs`)
`DipError` uses numeric prefixes: 1xxx Parse, 2xxx Catalog, 3xxx Execution, 4xxx Storage/IO.

## Key Design Constants
- Page size: **4096 bytes**
- Shard count (both BPM and LockManager): **64**
- Serialization: **little-endian**
- No WAL yet — durability relies on `BufferPoolManager::flush_all()` at shutdown
