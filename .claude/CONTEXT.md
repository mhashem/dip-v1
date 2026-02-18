# CONTEXT.md — dip-v1 Codebase State

Last updated: 2026-02-18 (after stability pass + plan creation)

---

## What the codebase can do today

### SQL supported
- `CREATE TABLE t (col TYPE, ...)` — types: `INT`, `BOOLEAN`, `VARCHAR`; constraint: `PRIMARY KEY`
- `CREATE INDEX idx ON t (col)` — integer columns only
- `INSERT INTO t VALUES (...), (...)` — positional, multi-row
- `SELECT * FROM t WHERE condition` — full table scan; single column `col = val` uses index scan
- `UPDATE t SET col = val WHERE condition`
- `DELETE FROM t WHERE condition`
- `BEGIN` / `COMMIT` / `ROLLBACK`

### What is NOT supported yet
- Column projections (`SELECT col1, col2`)
- `AND` / `OR` / `NOT` in `WHERE`
- `BETWEEN`, `IN(...)`
- `JOIN` (any kind)
- Aggregations (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `GROUP BY`, `HAVING`)
- `ORDER BY`, `LIMIT`, `OFFSET`
- `DROP TABLE` / `DROP INDEX`
- `ALTER TABLE`
- `DISTINCT`
- `NULL` support
- Types beyond `INT`, `BOOLEAN`, `VARCHAR`

---

## Architecture layers (bottom-up)

### Storage (`src/storage/`)
- **`DiskManager`** (`disk_manager.rs`): raw file I/O, 4096-byte pages, synchronous writes; page IDs are `u64`; no free-list
- **`LRUReplacer`** (`replacer.rs`): VecDeque-based LRU; O(N) pin/unpin (acceptable for small pools)
- **`BufferPoolInstance`** (`buffer_pool_instance.rs`): one shard of the buffer pool; owns page frames + replacer
- **`BufferPoolManager`** (`buffer_pool_manager.rs`): 64-shard wrapper; routes pages by `hash(page_id) % 64`; calls `flush_all()` on `Drop`
- **`SlottedPage`** (`table/slotted_page.rs`): variable-length tuples; header = `[LSN:4][FreeSpacePtr:2][SlotCount:2][NextPageId:4]`; soft delete via high bit of slot length; `mark_delete` / `rollback_delete` / `is_tuple_marked_for_delete` — **`get_tuple` returns `None` for deleted slots (fixed 2026-02-18)**
- **`TableHeap`** (`table/table_heap.rs`): linked list of SlottedPages; `insert_tuple` / `get_tuple` / `mark_delete` / `rollback_delete`; `iter()` returns `TableIterator` that skips deleted slots
- **`RID`** (`table/rid.rs`): `(page_id: u32, slot_num: u32)`
- **`Tuple`** (`tuple.rs`): `Vec<u8>` data + optional `RID`; `from_values(values, schema)` / `get_value(schema, col_idx)`
- **B+Tree** (`index/b_plus_tree*.rs`): integer keys (`i32`) → `RID`; internal pages have binary search; **leaf pages use linear search** (planned fix); leaf pages have `next_leaf_page_id` chain (enables range scans); `insert` with splitting works; `delete` is tombstone (no rebalancing yet); `get_value` / `update_value` / `insert` / `delete`

### Types (`src/types/mod.rs`)
- `TypeId`: `Integer`, `Boolean`, `Varchar`
- `Value`: `Integer(i32)`, `Boolean(bool)`, `Varchar(String)`; serialization / deserialization

### Catalog (`src/catalog/`)
- **`Column`**: name, type_id, is_primary; serialization
- **`Schema`**: `Vec<Column>`; `get_col_index(name)`, `get_primary_key_index()`, `to_bytes` / `from_bytes`
- **`PageStats`** / **`ColumnStats`** (`stats.rs`): zone maps (min/max per column per page); `might_satisfy(predicate)` used in `SeqScanExecutor`
- **`TableMetadata`**: Schema + TableHeap + `RwLock<HashMap<col_idx, Arc<RwLock<BPlusTree>>>>` (indexes) + `RwLock<HashMap<PageId, PageStats>>` (zone maps)
- **`CatalogManager`**: `Arc<RwLock<HashMap<String, Arc<TableMetadata>>>>` + BPM; `create_table` auto-creates PK index for integer PK; `save_metadata` / `load_metadata` using custom binary format with magic `DIPM`

### Execution (`src/execution/`)
- **`ExecutorContext`** (`executor.rs`): `Arc<TableMetadata>` + `Arc<Mutex<Transaction>>` + `Arc<LockManager>`; **currently has `&'a` lifetime** (planned to remove)
- **`Executor` trait**: `init()`, `next() -> Option<Tuple>`, `schema() -> &Schema`
- **`Expression`** (`expression.rs`): `Constant(Value)`, `Column(usize)`, `Binary{left, op, right}`; operators: `Eq`, `NotEq`, `Lt`, `Gt`, `LtEq`, `GtEq` — **AND/OR/NOT not yet supported**
- **`SeqScanExecutor`**: zone map pruning, shared lock per row, deleted-row re-check after lock
- **`FilterExecutor`**: wraps any executor with a predicate
- **`IndexScanExecutor`**: single point lookup via B+Tree → RID → get_tuple
- **`InsertExecutor`**: acquires X lock, writes zone map, updates all indexes, records `WriteRecord::Insert`
- **`UpdateExecutor`**: materializes RIDs first (Halloween Problem), mark_delete + insert_new, updates indexes, records `WriteRecord::Update`
- **`DeleteExecutor`**: materializes RIDs first, mark_delete, updates indexes, records `WriteRecord::Delete`
- **`vectorized.rs`**: `TupleBatch` struct + `VectorizedExecutor` trait defined; **no implementations**

### SQL Engine (`src/sql/`)
- **`SQLEngine`** (`engine.rs`): wraps `CatalogManager` + `TransactionManager` + optional active `Transaction`
- `execute(sql)`: parses via `sqlparser-rs` (`GenericDialect`); dispatches to handlers
- Auto-commit: if no `BEGIN` is active when `execute()` is called, a new transaction is started and committed at the end of the call
- Multi-statement atomicity: all statements in one `execute()` call share one transaction; any error rolls back all
- Explicit transactions: `BEGIN` / `COMMIT` / `ROLLBACK` persist `self.current_txn` across multiple `execute()` calls
- Index scan optimization: `WHERE col = val` uses index if one exists on `col`
- Error mapping: executor errors are signaled by returning fewer tuples than expected (limitation of `Option<Tuple>` return)

### Concurrency (`src/concurrency/`)
- **`Transaction`** (`transaction.rs`): `txn_id`, `state` (`Running/Committed/Aborted`), `shared_locks: HashSet<RID>`, `exclusive_locks: HashSet<RID>`, `write_set: Vec<WriteRecord>`
- **`WriteRecord`**: `Insert{table, rid}`, `Delete{table, rid, old_tuple}`, `Update{table, old_rid, old_tuple, new_rid, new_tuple}`
- **`TransactionManager`** (`transaction_manager.rs`): atomic `TxnId` counter, active txn registry, holds `Arc<LockManager>`; `begin()` / `commit()` / `abort()`; commit/abort release all locks
- **`LockManager`** (`lock_manager.rs`): 64-shard `Vec<Mutex<HashMap<RID, LockRequestQueue>>>` + per-shard `Condvar`; Strict 2PL; supports S lock, X lock, S→X upgrade; **blocks indefinitely on conflict** (deadlock detection planned: timeout via `condvar.wait_timeout`)
- **Rollback** (`SQLEngine::rollback`): iterates `write_set` in reverse to undo inserts (mark_delete + index cleanup), deletes (rollback_delete + index restore), updates (undo new insert + undo old delete + index fix)
- **No WAL** — durability relies only on BPM flush at process exit; crash can lose data

### Errors (`src/errors.rs`)
- `DipError`: 1xxx Parse, 2xxx Catalog, 3xxx Execution (TypeMismatch, PkViolation, UniqueViolation, TransactionAborted, Internal), 4xxx Storage

---

## Known issues / limitations (as of 2026-02-18)

1. **No deadlock detection** — `LockManager::acquire_lock` blocks forever if a cycle forms
2. **No WAL** — process crash between BPM flush events loses committed data
3. **B+Tree leaf linear search** — O(N) per lookup/insert
4. **B+Tree delete no rebalancing** — tree degrades (nodes can be near-empty after many deletes)
5. **No range scans on index** — `IndexScanExecutor` is point-lookup only
6. **SELECT * only** — no column projection
7. **No AND/OR/NOT in WHERE** — only single binary conditions
8. **No JOINs, aggregations, ORDER BY, LIMIT**
9. **No DROP TABLE / DROP INDEX**
10. **Executor `next()` returns `Option<Tuple>` not `Result`** — errors propagated via tuple count mismatch
11. **`vectorized.rs` is a stub** — framework defined, no implementations

---

## Bugs fixed in this session (2026-02-18)

1. **`SlottedPage::get_tuple` returned tombstoned tuples** — fixed by checking the high bit of `raw_size` before stripping it
2. **`test_10_concurrency_exclusive_lock_conflict` deadlocked** — restructured to commit engine_a before joining the thread (follows same pattern as `test_exclusive_lock_conflict_and_release`)

---

## Persistence format

- **`dip.db`**: raw 4KB page data (BPM managed)
- **`dip.meta`**: catalog metadata — magic `DIPM`, then for each table: name, schema bytes, first heap page ID, index entries (col_idx → B+Tree root page ID), zone map entries

---

## Test files

| File | What it covers |
|------|---------------|
| `atomicity_test.rs` | INSERT/UPDATE/DELETE atomicity, BEGIN/COMMIT/ROLLBACK |
| `b_plus_tree_test.rs` | B+Tree insert, search |
| `complex_workflow_test.rs` | Bulk inserts + mixed workload |
| `concurrency_integration_test.rs` | LockManager: S+S concurrent, X conflicts, abort releases |
| `delete_test.rs` | TableHeap mark_delete + iterator skipping |
| `execution_test.rs` | SeqScan + InsertExecutor end-to-end |
| `features_test.rs` | 15 integration tests covering all SQL features + concurrency |
| `load_test.rs` | 10k inserts, performance baseline |
| `load_test_concurrency.rs` | 12-thread concurrent insert/update (measures TPS) |
| `lock_manager_test.rs` | Direct LockManager unit tests |
| `persistence_test.rs` | save_metadata / load_metadata round-trip |
| `primary_key_test.rs` | PK uniqueness enforcement |
| `secondary_index_test.rs` | CREATE INDEX + query via index |
| `sql_test.rs` | SQL engine end-to-end |
| `sql_update_delete_test.rs` | UPDATE + DELETE via SQL |
| `storage_integration.rs` | DiskManager + BPM + TableHeap stack |
| `transaction_test.rs` | TransactionManager basics |
| `type_storage_integration.rs` | Value serialization round-trip |
| `update_test.rs` | UpdateExecutor unit test |
| `zonemap_test.rs` | Zone map pruning |

---

## Dependency graph (simplified)

```
main.rs
  └── SQLEngine (sql/engine.rs)
        ├── CatalogManager (catalog/)
        │     └── BufferPoolManager → BufferPoolInstance → LRUReplacer → DiskManager
        ├── TransactionManager (concurrency/)
        │     └── LockManager
        └── Executors (execution/)
              ├── SeqScanExecutor → TableHeap → SlottedPage
              ├── FilterExecutor
              ├── IndexScanExecutor → BPlusTree
              ├── InsertExecutor
              ├── UpdateExecutor
              └── DeleteExecutor
```
