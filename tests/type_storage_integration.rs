use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::table::table_heap::TableHeap;
use dip_v1::storage::tuple::Tuple;
use dip_v1::types::{Value, TypeId};
use std::sync::{Arc, Mutex};

#[test]
fn test_value_storage_integration() {
    let file_path = std::env::temp_dir().join("dip_type_storage.db");
    if file_path.exists() {
        std::fs::remove_file(&file_path).unwrap();
    }

    {
        let dm = DiskManager::new(&file_path).unwrap();
        let bpm = Arc::new(Mutex::new(BufferPoolManager::new(10, dm)));
        let table = TableHeap::new(bpm);

        // 1. Create different types of values
        let val_int = Value::Integer(12345);
        let val_bool = Value::Boolean(true);
        let val_str = Value::Varchar("DIP-DB is growing!".to_string());

        // 2. Manually pack them into a single "row" (Tuple)
        // [Int (4 bytes) | Bool (1 byte) | Varchar (4+N bytes)]
        let mut row_data = Vec::new();
        row_data.extend(val_int.to_bytes());
        row_data.extend(val_bool.to_bytes());
        row_data.extend(val_str.to_bytes());

        let tuple = Tuple::new(row_data);
        let rid = table.insert_tuple(&tuple).expect("Insert failed");

        // 3. Retrieve and unpack
        let fetched_tuple = table.get_tuple(rid).expect("Fetch failed");
        let data = &fetched_tuple.data;

        let res_int = Value::from_bytes(&data[0..4], TypeId::Integer);
        let res_bool = Value::from_bytes(&data[4..5], TypeId::Boolean);
        let res_str = Value::from_bytes(&data[5..], TypeId::Varchar);

        assert_eq!(res_int, val_int);
        assert_eq!(res_bool, val_bool);
        assert_eq!(res_str, val_str);
    }

    if file_path.exists() {
        std::fs::remove_file(&file_path).unwrap();
    }
}
