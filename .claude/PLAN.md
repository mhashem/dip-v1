# Plan: dip-v1 → Professional-Grade Embedded Database

## Context
The database has solid ACID foundations (S2PL, undo-based rollback, sharded BPM/LockManager) but is missing the features that define a production-capable embedded database: full SQL (JOINs, aggregations, projections, ORDER BY), crash durability (WAL), and deadlock safety. This plan addresses all four user-selected areas in dependency order.

---

## 1. Executor Refactoring: Remove Lifetimes (Prerequisite)

**Problem:** Every executor holds `context: &'a ExecutorContext`, which makes JOINs impossible (can't store a newly created `ExecutorContext` alongside an executor that borrows it — self-referential struct).

**Fix:** Make `ExecutorContext` derive `Clone` (all fields are `Arc<>`) and have every executor own its context by value.

**Files changed:**
- `src/execution/executor.rs` — add `#[derive(Clone)]` to `ExecutorContext`
- `src/execution/seq_scan.rs`, `filter.rs`, `insert.rs`, `update.rs`, `delete.rs`, `index_scan.rs` — change `context: &'a ExecutorContext` → `context: ExecutorContext`, remove `<'a>` lifetime from struct/impl, change `Box<dyn Executor + 'a>` → `Box<dyn Executor>`
- `src/sql/engine.rs` — pass `context` by value (cloned per-executor chain)

---

## 2. Expression Enhancements

Add to `src/execution/expression.rs`:
- `BinaryOperator::And`, `Or`
- `Expression::Not(Box<Expression>)`
- `Expression::Between { expr, low, high }` → sugar for `low <= expr AND expr <= high`
- `Expression::In { expr: Box<Expression>, list: Vec<Value> }`
- Update `evaluate()` for all new variants (short-circuit `And`/`Or`)

Add to `src/sql/engine.rs` `parse_expression`:
- `Expr::BinaryOp` with `And`/`Or` → `Expression::Binary { op: And/Or, ... }`
- `Expr::UnaryOp { op: Not, .. }` → `Expression::Not(...)`
- `Expr::Between { expr, low, high, .. }` → `Expression::Between { ... }`
- `Expr::InList { expr, list, .. }` → `Expression::In { ... }`

---

## 3. New Executors

### 3a. ProjectionExecutor (`src/execution/projection.rs`)
```rust
pub struct ProjectionExecutor {
    child: Box<dyn Executor>,
    child_schema: Schema,
    col_indices: Vec<usize>,  // which columns to keep, in order
    output_schema: Schema,
}
```
`next()` calls `child.next()`, then builds a new `Tuple::from_values(projected_values, &output_schema)`.

### 3b. SortExecutor (`src/execution/sort.rs`)
```rust
pub struct SortKey { pub col_idx: usize, pub ascending: bool }
pub struct SortExecutor {
    child: Box<dyn Executor>,
    child_schema: Schema,
    keys: Vec<SortKey>,
    buffer: Vec<Tuple>,
    cursor: usize,
}
```
`init()`: drain child → sort buffer by keys (compare `Value` using PartialOrd impl on Value).
`next()`: return `buffer[cursor++]`.

**Add `PartialOrd` to `Value`** in `src/types/mod.rs`: Integer < Boolean(false) < Varchar lexicographic.

### 3c. LimitExecutor (`src/execution/limit.rs`)
```rust
pub struct LimitExecutor {
    child: Box<dyn Executor>,
    limit: usize,
    offset: usize,
    count: usize,
    skipped: usize,
}
```
`next()`: skip `offset` rows first, then count up to `limit`.

### 3d. AggregationExecutor (`src/execution/aggregation.rs`)
```rust
pub enum AggFunc { CountStar, Count, Sum, Avg, Min, Max }
pub struct AggExpr { pub func: AggFunc, pub col_idx: Option<usize>, pub name: String }

pub struct AggregationExecutor {
    child: Box<dyn Executor>,
    child_schema: Schema,
    group_by: Vec<usize>,     // column indices
    aggregates: Vec<AggExpr>,
    output_schema: Schema,
    results: Vec<Tuple>,
    cursor: usize,
}
```
`init()`: call `child.init()` then drain into a `HashMap<String, Accum>` keyed by serialized group values. After consuming all rows, materialise one output `Tuple` per group into `results`.

`Accum` holds `Vec<i64>` for counts/sums, `Vec<Option<Value>>` for min/max.

Output schema columns = GROUP BY columns (same type) + aggregate columns (all `TypeId::Integer` for now, since we only have `i32`).

### 3e. NestedLoopJoinExecutor (`src/execution/nested_loop_join.rs`)
```rust
pub struct NestedLoopJoinExecutor {
    left: Box<dyn Executor>,
    left_schema: Schema,
    right_context: ExecutorContext,
    right_schema: Schema,
    joined_schema: Schema,      // left cols + right cols concatenated
    condition: Option<Expression>,
    current_left: Option<Tuple>,
    right_scan: Option<Box<dyn Executor>>,
}
```
`next()` loop:
1. If `right_scan` is `None`, get next left tuple (return `None` if exhausted), create new `SeqScanExecutor::new(right_context.clone())` and call `init()`.
2. Call `right_scan.next()`. If `Some(right)`, concatenate bytes into joined tuple, evaluate condition; return if match. If `None`, clear `right_scan` and loop.

Join tuple construction: `Tuple::new([left.data, right.data].concat())` — works because `Tuple::get_value` walks bytes sequentially and the combined schema has left cols followed by right cols.

**`src/execution/mod.rs`** — add `pub mod projection; sort; limit; aggregation; nested_loop_join;`

---

## 4. SQL Engine Overhaul (`src/sql/engine.rs`)

Refactor `handle_query` into a query-plan builder. Parse the full sqlparser `Select` AST:

```
FROM    → scan_node (SeqScan / IndexScan)
JOINs   → NestedLoopJoinExecutor wrapping scan nodes; build combined schema
WHERE   → FilterExecutor
GROUP BY + aggregates → AggregationExecutor (if any aggregate in SELECT)
HAVING  → FilterExecutor (wrapping AggregationExecutor)
SELECT  → ProjectionExecutor (if not SELECT *)
ORDER BY → SortExecutor
LIMIT/OFFSET → LimitExecutor
```

**Multi-table column resolution:** When parsing expressions with JOINs, match `Expr::CompoundIdentifier([table, col])` by looking up `table` in the schema list, then offset the column index by the left table's column count.

**Aggregate detection:** Scan `select.projection` items for `Expr::Function` calls (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`) to decide whether to use `AggregationExecutor`.

**New DDL:**
- `DROP TABLE name` → remove from catalog
- `DROP INDEX name ON table` → remove index from TableMetadata

**Output formatting:** After the executor chain runs, format using the outermost executor's `schema()` to label columns.

---

## 5. B+ Tree Improvements

### 5a. Binary search on leaf pages (`src/storage/index/b_plus_tree_leaf_page.rs`)
Add `find_position(key: i32) -> u32` using binary search; use it in `insert()`, `delete()`, and value lookup.

### 5b. Range scans (`src/storage/index/b_plus_tree.rs`)
```rust
pub fn get_range(&self, low: i32, high: i32) -> Vec<RID>
```
Navigate to first leaf with `find_leaf_page(low)`, iterate entries where `key >= low`, follow `next_leaf_page_id` pointers until `key > high` or end.

Update `IndexScanExecutor` to accept an optional high bound, enabling range predicates (`WHERE id > 5`) to use the index.

### 5c. Delete rebalancing (`src/storage/index/b_plus_tree.rs` + `b_plus_tree_leaf_page.rs`)
After deleting an entry from a leaf, if `size < ceil(max_size / 2)`:
1. Try to borrow from right sibling: move sibling's first key to this page, update parent separator.
2. Try to borrow from left sibling: move sibling's last key to this page, update parent separator.
3. Merge: concatenate entries, remove separator from parent; if parent underflows, recurse.

---

## 6. Deadlock Detection (Timeout-based)

**`src/concurrency/lock_manager.rs`:**
Replace `condvar.wait(table)` with `condvar.wait_timeout(table, Duration::from_millis(200))`.

If `timed_out()`, remove pending request, `notify_all()`, return `false`.

---

## 7. Write-Ahead Logging (WAL)

### 7a. New files

**`src/concurrency/log_record.rs`:**
```rust
pub type Lsn = u64;
pub enum LogRecord {
    Begin { lsn: Lsn, txn_id: TxnId },
    Commit { lsn: Lsn, txn_id: TxnId },
    Abort { lsn: Lsn, txn_id: TxnId },
    Insert { lsn: Lsn, txn_id: TxnId, table: String, rid: RID, after: Vec<u8> },
    Delete { lsn: Lsn, txn_id: TxnId, table: String, rid: RID, before: Vec<u8> },
    Update { lsn: Lsn, txn_id: TxnId, table: String, old_rid: RID, before: Vec<u8>, new_rid: RID, after: Vec<u8> },
}
```
Binary format: `[u32 total_len][u8 type][payload bytes...]`

**`src/concurrency/log_manager.rs`:**
```rust
pub struct LogManager {
    file: Mutex<BufWriter<File>>,
    next_lsn: AtomicU64,
}
impl LogManager {
    pub fn new(path: &Path) -> io::Result<Self>
    pub fn append(&self, record: &LogRecord) -> Lsn
    pub fn flush(&self) -> io::Result<()>
    pub fn read_all(path: &Path) -> io::Result<Vec<LogRecord>>
}
```

### 7b. Integration

**`ExecutorContext`** — add `pub log_manager: Option<Arc<LogManager>>`

**`TransactionManager`** — add `pub log_manager: Option<Arc<LogManager>>`; call `log_manager.append(Begin)` in `begin()`, `append(Commit)` + `flush()` in `commit()`, `append(Abort)` + `flush()` in `abort()`.

**`InsertExecutor::next()`** — call `log_manager.append(Insert { ..., after: tuple.data.clone() })` BEFORE `table.insert_tuple()`.

**`DeleteExecutor::next()`** — call `log_manager.append(Delete { ..., before: old_tuple.data.clone() })` BEFORE `table.mark_delete()`.

**`UpdateExecutor::next()`** — call `log_manager.append(Update { ... })` BEFORE `table.mark_delete()` + `table.insert_tuple()`.

### 7c. Recovery (`src/concurrency/log_manager.rs`)

```rust
pub fn recover(log_path: &Path, catalog: &mut CatalogManager) -> io::Result<()>
```
1. Read all log records.
2. Collect committed `txn_id`s (those with a `Commit` record).
3. Forward pass: for each `Insert/Delete/Update` record belonging to a committed txn, re-apply it to the catalog's heaps.
4. Transactions without a `Commit` record are implicitly aborted; their data is ignored.

**`main.rs`** — call `LogManager::recover(&log_path, &mut catalog)` before opening the REPL.

---

## 8. Critical Files Summary

| Change | Files |
|---|---|
| Remove executor lifetimes | `executor.rs`, all executor files, `sql/engine.rs` |
| AND/OR/NOT/BETWEEN/IN | `expression.rs`, `engine.rs` |
| New executors | `projection.rs`, `sort.rs`, `limit.rs`, `aggregation.rs`, `nested_loop_join.rs`, `mod.rs` |
| SQL engine overhaul | `sql/engine.rs` (major) |
| B+ Tree fixes | `b_plus_tree_leaf_page.rs`, `b_plus_tree.rs` |
| Deadlock timeout | `concurrency/lock_manager.rs` |
| WAL | `log_record.rs` (new), `log_manager.rs` (new), `concurrency/mod.rs`, `transaction_manager.rs`, `executor.rs`, all write executors, `main.rs` |
| Types | `types/mod.rs` (add PartialOrd) |

---

## 9. Verification

```bash
cargo test                              # All existing tests must still pass
cargo test --test features_test         # SQL completeness
cargo test --test atomicity_test        # Existing ACID tests
cargo test --test concurrency_integration_test  # Deadlock detection
cargo test --test b_plus_tree_test      # Range scans
```

New test files to add:
- `tests/sql_completeness_test.rs` — projections, AND/OR, GROUP BY, ORDER BY, JOINs, LIMIT
- `tests/wal_recovery_test.rs` — simulate crash, verify data survives after recovery
- `tests/b_plus_tree_range_test.rs` — range scans, delete rebalancing
