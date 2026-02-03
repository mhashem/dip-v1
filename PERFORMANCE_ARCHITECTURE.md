# DIP-DB Performance Architecture (Pro Upgrades)

This document outlines the architectural overhaul designed to transform `dip-v1` from a functional prototype into a high-performance database engine.

## 1. Concurrency: Sharded Lock Manager
**Problem:** The original `LockManager` used a single `Mutex<HashMap>` to store all locks. This created a "Global Lock" bottleneck where every transaction, regardless of which row it touched, contended on the same mutex.
**Solution:** **Sharding**.
*   **Structure:** `Vec<Mutex<HashMap<RID, LockRequestQueue>>>`.
*   **Logic:** `shard_idx = hash(RID) % NUM_SHARDS`.
*   **Benefit:** Transactions operating on different rows (mapping to different shards) can acquire/release locks purely in parallel with zero contention.
*   **Implementation:** `src/concurrency/lock_manager.rs` will be refactored to use 64 shards.

## 2. Execution: Vectorized Processing (Batch-at-a-Time)
**Problem:** The "Volcano" iterator model (`next() -> Tuple`) incurs a virtual function call and poor instruction cache locality for *every single row*.
**Solution:** **Vectorization**.
*   **Structure:** `TupleBatch` struct containing `columns: Vec<Vec<Value>>` representing ~1024 rows.
*   **Logic:** Executors now return `Option<TupleBatch>`.
*   **Benefit:** 
    *   Amortizes function call overhead over 1024 rows.
    *   Enables CPU prefetching and better L1 cache usage.
    *   (Future) Allows SIMD optimizations.

## 3. Memory: Zero-Copy Access
**Problem:** `Tuple::get_value` allocates new `Value` objects and copies strings (`String::from_utf8`) every time data is read.
**Solution:** **Borrowed Types**.
*   **Structure:** `Value::Varchar` will hold `Cow<'a, str>` (Clone-on-Write) or a raw slice pointer where safe.
*   **Refactoring:** This is a deep change requiring lifetime propagation (`Tuple<'a>`). For this phase, we will implement **Lazy Materialization**: only copy data when absolutely necessary (e.g., when passing to a user or modifying).

---

## Performance Benchmark Plan
We will use `tests/load_test_concurrency.rs` as the baseline.
1.  **Baseline:** ~1558 TPS.
2.  **Phase 1 (Sharded Lock Manager):** **~2589 TPS**.
    *   **Result:** **~66% improvement** in throughput by simply reducing mutex contention.
3.  **Phase 2 (Vectorization):** Expected 10x improvement in Sequential Scan speed.
