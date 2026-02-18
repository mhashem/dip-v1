use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::concurrency::transaction_manager::TransactionManager;
use std::sync::Arc;
use std::thread;
use tempfile::NamedTempFile;

fn setup_engine() -> (SQLEngine, NamedTempFile) {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let engine = SQLEngine::new(catalog);
    (engine, db_file)
}

#[test]
fn test_1_full_transaction_lifecycle_commit() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE accounts (id INT PRIMARY KEY, balance INT)").unwrap();
    
    engine.execute("BEGIN").unwrap();
    engine.execute("INSERT INTO accounts VALUES (1, 1000)").unwrap();
    engine.execute("UPDATE accounts SET balance = 1100 WHERE id = 1").unwrap();
    engine.execute("INSERT INTO accounts VALUES (2, 500)").unwrap();
    engine.execute("DELETE FROM accounts WHERE id = 2").unwrap();
    engine.execute("COMMIT").unwrap();

    let res = engine.execute("SELECT * FROM accounts").unwrap();
    assert!(res.contains("1100"));
    assert!(!res.contains("|               2 |"));
}

#[test]
fn test_2_full_transaction_lifecycle_rollback() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE items (id INT PRIMARY KEY, price INT)").unwrap();
    engine.execute("INSERT INTO items VALUES (1, 100)").unwrap();

    engine.execute("BEGIN").unwrap();
    engine.execute("INSERT INTO items VALUES (2, 200)").unwrap();
    engine.execute("UPDATE items SET price = 150 WHERE id = 1").unwrap();
    engine.execute("DELETE FROM items WHERE id = 1").unwrap();
    engine.execute("ROLLBACK").unwrap();

    let res = engine.execute("SELECT * FROM items").unwrap();
    assert!(res.contains("100"));
    assert!(!res.contains("200"));
    assert!(!res.contains("150"));
}

#[test]
fn test_3_batch_insert_pk_violation_atomicity() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE users (id INT PRIMARY KEY, age INT)").unwrap();
    engine.execute("INSERT INTO users VALUES (1, 20)").unwrap();

    // Batch insert where the 3rd row violates PK
    let res = engine.execute("INSERT INTO users VALUES (2, 30), (3, 40), (1, 50), (4, 60)");
    assert!(res.is_err());

    let select_res = engine.execute("SELECT * FROM users").unwrap();
    assert!(!select_res.contains("30"));
    assert!(!select_res.contains("40"));
    assert!(!select_res.contains("60"));
    assert!(select_res.contains("20"));
}

#[test]
fn test_4_secondary_index_consistency() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE data (id INT PRIMARY KEY, val INT)").unwrap();
    engine.execute("INSERT INTO data VALUES (1, 10)").unwrap();
    engine.execute("CREATE INDEX idx_val ON data (val)").unwrap();

    let res = engine.execute("SELECT * FROM data WHERE val = 10").unwrap();
    assert!(res.contains("1") && res.contains("10"));

    engine.execute("UPDATE data SET val = 20 WHERE id = 1").unwrap();
    
    let res_old = engine.execute("SELECT * FROM data WHERE val = 10").unwrap();
    assert!(!res_old.contains("10"));
    
    let res_new = engine.execute("SELECT * FROM data WHERE val = 20").unwrap();
    assert!(res_new.contains("1") && res_new.contains("20"));
}

#[test]
fn test_5_multi_statement_atomic_failure() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE logs (id INT PRIMARY KEY, msg VARCHAR)").unwrap();
    
    let sql = "INSERT INTO logs VALUES (1, 'First'); INSERT INTO logs VALUES (1, 'Second');";
    let res = engine.execute(sql);
    assert!(res.is_err());

    let check = engine.execute("SELECT * FROM logs").unwrap();
    assert!(!check.contains("First"));
}

#[test]
fn test_6_update_pk_violation_rollback() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE test (id INT PRIMARY KEY, name VARCHAR)").unwrap();
    engine.execute("INSERT INTO test VALUES (1, 'A'), (2, 'B')").unwrap();

    let sql = "UPDATE test SET id = 2 WHERE id = 1";
    let res = engine.execute(sql);
    assert!(res.is_err(), "Expected error on PK violation during update");

    let check = engine.execute("SELECT * FROM test WHERE id = 1").unwrap();
    assert!(check.contains("A"));
    
    let check2 = engine.execute("SELECT * FROM test WHERE id = 2").unwrap();
    assert!(check2.contains("B"));
}

#[test]
fn test_7_delete_rollback_complex() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE t (id INT PRIMARY KEY)").unwrap();
    engine.execute("INSERT INTO t VALUES (1), (2), (3)").unwrap();

    engine.execute("BEGIN").unwrap();
    engine.execute("DELETE FROM t WHERE id = 1").unwrap();
    engine.execute("DELETE FROM t WHERE id = 2").unwrap();
    
    engine.execute("INSERT INTO t VALUES (2)").unwrap(); 
    engine.execute("INSERT INTO t VALUES (1)").unwrap();
    engine.execute("ROLLBACK").unwrap();

    let res = engine.execute("SELECT * FROM t").unwrap();
    assert!(res.contains("1") && res.contains("2") && res.contains("3"));
}

#[test]
fn test_8_varchar_storage_limits() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE docs (id INT PRIMARY KEY, content VARCHAR)").unwrap();
    
    let long_str = "A".repeat(100);
    let sql = format!("INSERT INTO docs VALUES (1, '{}')", long_str);
    engine.execute(&sql).unwrap();

    let res = engine.execute("SELECT * FROM docs WHERE id = 1").unwrap();
    assert!(res.contains(&long_str));
}

#[test]
fn test_9_boolean_filtering() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE flags (id INT PRIMARY KEY, active BOOLEAN)").unwrap();
    engine.execute("INSERT INTO flags VALUES (1, true), (2, false), (3, true)").unwrap();

    let res = engine.execute("SELECT * FROM flags WHERE active = true").unwrap();
    assert!(res.contains("1"));
    assert!(res.contains("3"));
    assert!(!res.contains("              2 |"));
}

#[test]
fn test_10_concurrency_exclusive_lock_conflict() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let txn_manager = Arc::new(TransactionManager::new());

    let mut engine_a = SQLEngine::new_with_txn_manager(catalog.clone(), txn_manager.clone());

    engine_a.execute("CREATE TABLE shared (id INT PRIMARY KEY, val INT)").unwrap();
    engine_a.execute("INSERT INTO shared VALUES (1, 10)").unwrap();

    // Engine A holds an exclusive lock on row 1 inside an open transaction.
    engine_a.execute("BEGIN").unwrap();
    engine_a.execute("UPDATE shared SET val = 20 WHERE id = 1").unwrap();

    // Engine B starts in a thread and will block waiting for the lock.
    let catalog_b = catalog.clone();
    let txn_manager_b = txn_manager.clone();
    let handle = thread::spawn(move || {
        let mut engine_b = SQLEngine::new_with_txn_manager(catalog_b, txn_manager_b);
        // This blocks until Engine A releases the lock via COMMIT.
        let res = engine_b.execute("UPDATE shared SET val = 30 WHERE id = 1");
        res.is_ok()
    });

    // Give Engine B time to start and block on the lock.
    thread::sleep(std::time::Duration::from_millis(200));

    // Engine A commits, releasing the lock and unblocking Engine B.
    engine_a.execute("COMMIT").unwrap();

    let b_succeeded = handle.join().unwrap();
    assert!(b_succeeded, "Engine B should have succeeded after Engine A released its lock");

    // Verify Engine B's update is the final committed value.
    let mut engine_c = SQLEngine::new_with_txn_manager(catalog, txn_manager);
    let res = engine_c.execute("SELECT * FROM shared WHERE id = 1").unwrap();
    assert!(res.contains("30"), "Engine B's update should be the final value");
}

#[test]
fn test_11_persistence_across_restarts() {
    let db_path = std::env::current_dir().unwrap().join("persist_test.db");
    if db_path.exists() { std::fs::remove_file(&db_path).unwrap(); }
    let meta_path = db_path.with_extension("meta");
    if meta_path.exists() { std::fs::remove_file(&meta_path).unwrap(); }

    {
        let dm = DiskManager::new(&db_path).unwrap();
        let bpm = Arc::new(BufferPoolManager::new(100, dm));
        let mut catalog = CatalogManager::new(bpm);
        let mut engine = SQLEngine::new(catalog.clone());
        engine.execute("CREATE TABLE persist (id INT PRIMARY KEY, name VARCHAR)").unwrap();
        engine.execute("INSERT INTO persist VALUES (1, 'PersistentData')").unwrap();
        catalog.save_metadata(&meta_path).unwrap();
    } 

    {
        let dm = DiskManager::new(&db_path).unwrap();
        let bpm = Arc::new(BufferPoolManager::new(100, dm));
        let mut catalog = CatalogManager::new(bpm);
        catalog.load_metadata(&meta_path).unwrap();
        let mut engine = SQLEngine::new(catalog);
        let res = engine.execute("SELECT * FROM persist").unwrap();
        assert!(res.contains("PersistentData"));
    }
    
    std::fs::remove_file(&db_path).ok();
    std::fs::remove_file(&meta_path).ok();
}

#[test]
fn test_12_buffer_pool_eviction_correctness() {
    let db_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(db_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(2, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    engine.execute("CREATE TABLE many_pages (id INT PRIMARY KEY, val INT)").unwrap();
    
    for i in 0..50 {
        let sql = format!("INSERT INTO many_pages VALUES ({}, {})", i, i * 10);
        engine.execute(&sql).unwrap();
    }

    let res = engine.execute("SELECT * FROM many_pages").unwrap();
    for i in 0..50 {
        assert!(res.contains(&format!("{}", i)), "Missing id {}", i);
    }
}

#[test]
fn test_13_nested_transaction_attempt() {
    let (mut engine, _file) = setup_engine();
    engine.execute("BEGIN").unwrap();
    let res = engine.execute("BEGIN");
    assert!(res.is_err());
    assert!(format!("{:?}", res).contains("Nested transactions not supported"));
    
    engine.execute("CREATE TABLE nested (id INT PRIMARY KEY)").unwrap();
    engine.execute("COMMIT").unwrap();
    
    let res = engine.execute("SELECT * FROM nested").unwrap();
    assert!(res.contains("id"));
}

#[test]
fn test_14_update_with_complex_predicate() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE nums (id INT PRIMARY KEY, val INT)").unwrap();
    for i in 1..=10 {
        engine.execute(&format!("INSERT INTO nums VALUES ({}, {})", i, i)).unwrap();
    }

    engine.execute("UPDATE nums SET val = 999 WHERE id > 5").unwrap();

    let res = engine.execute("SELECT * FROM nums WHERE val = 999").unwrap();
    for i in 6..=10 {
        assert!(res.contains(&format!("{}", i)));
    }
}

#[test]
fn test_15_delete_with_index_scan() {
    let (mut engine, _file) = setup_engine();
    engine.execute("CREATE TABLE idx_test (id INT PRIMARY KEY, name VARCHAR)").unwrap();
    engine.execute("INSERT INTO idx_test VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Charlie')").unwrap();
    
    engine.execute("DELETE FROM idx_test WHERE id = 2").unwrap();
    
    let res = engine.execute("SELECT * FROM idx_test").unwrap();
    assert!(res.contains("Alice"));
    assert!(res.contains("Charlie"));
    assert!(!res.contains("Bob"));
    
    let res_idx = engine.execute("SELECT * FROM idx_test WHERE id = 2").unwrap();
    assert!(!res_idx.contains("Bob"));
}