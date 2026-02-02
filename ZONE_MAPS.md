# Zone Maps (Min/Max Pruning) Design

## Concept
Zone Maps (also known as Data Skipping or Min/Max Indices) optimize scan performance by maintaining metadata about chunks of data (in our case, **Pages**).

For every Page, and for every Column in that page, we store:
- **Min Value**
- **Max Value**

## Query Execution Flow
When executing a query like `SELECT * FROM users WHERE age > 90`:

1.  **Normal Scan:** Read Page 1 -> Decode Tuple -> Check `age > 90` -> Match/No Match.
2.  **Zone Map Scan:**
    *   Check Page 1 Metadata: `age` Min=10, Max=50.
    *   **Pruning:** Since Max (50) < 90, it is *impossible* for this page to contain a matching tuple.
    *   **Action:** Skip reading Page 1 entirely.

## Implementation Plan

### 1. Data Structures (`src/catalog/stats.rs`)
We need structures to hold these statistics.

```rust
pub struct ColumnStats {
    pub min: Value,
    pub max: Value,
}

pub struct PageStats {
    // Key: Column Index, Value: Stats
    pub columns: HashMap<usize, ColumnStats>,
}
```

### 2. Maintenance (Write Path)
We need to update these stats whenever we write data.
*   **Insert:** When inserting a tuple into a Page, update the `PageStats` for that page.
    *   `new_min = min(current_min, new_val)`
    *   `new_max = max(current_max, new_val)`

### 3. Integration (Read Path)
Update `SeqScanExecutor`:
*   Before fetching a page from `TableHeap`, ask the `ZoneMapManager`: "Does this page satisfy the predicate?"
*   If No -> Skip Page.
*   If Yes -> Fetch and iterate tuples.

## Architecture
For this phase, we will store the Zone Maps **in-memory** within the `TableMetadata`.
*(In a production system, these would be serialized to a separate metadata file or the page header).*

## Tasks
1.  [ ] **Define Structs:** Create `ColumnStats` and `PageStats`.
2.  [ ] **Update Table Metadata:** Add a registry of `PageId -> PageStats`.
3.  [ ] **Capture Stats:** Update `TableHeap::insert_tuple` to calculate and store stats.
4.  [ ] **Pruning Logic:** Add `satisfies(predicate)` method to `PageStats`.
5.  [ ] **Executor Update:** Modify `SeqScan` to use the pruning logic.
