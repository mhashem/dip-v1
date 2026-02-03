use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use dip_v1::storage::table::table_heap::TableHeap;
use dip_v1::storage::tuple::Tuple;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[test]
fn test_delete_tuple() {
    let temp_file = NamedTempFile::new().unwrap();
    let dm = DiskManager::new(temp_file.path()).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(10, dm));
    let table = TableHeap::new(bpm);

    let t1 = Tuple::new(b"Alive".to_vec());
    let t2 = Tuple::new(b"Dead".to_vec());
    let t3 = Tuple::new(b"Alive2".to_vec());

    let rid1 = table.insert_tuple(&t1).unwrap();
    let rid2 = table.insert_tuple(&t2).unwrap();
    let rid3 = table.insert_tuple(&t3).unwrap();

    // Verify all exist
    assert!(table.get_tuple(rid1).is_some());
    assert!(table.get_tuple(rid2).is_some());
    assert!(table.get_tuple(rid3).is_some());

    // Delete middle one
    assert!(table.mark_delete(rid2));

    // Verify
    assert!(table.get_tuple(rid1).is_some());
    assert!(table.get_tuple(rid2).is_none(), "Deleted tuple should not be returned");
    assert!(table.get_tuple(rid3).is_some());

    // Verify Scan
    let mut iter = table.iter();
    let mut count = 0;
    while let Some(tuple) = iter.next() {
        if tuple.data == b"Dead" {
            panic!("Found deleted tuple in scan!");
        }
        count += 1;
    }
    assert_eq!(count, 2);
}
