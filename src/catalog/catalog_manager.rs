use crate::catalog::schema::Schema;
use crate::storage::table::table_heap::TableHeap;
use crate::storage::buffer_pool_manager::BufferPoolManager;
use crate::catalog::stats::PageStats;
use crate::storage::disk_manager::PageId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

pub struct TableMetadata {
    pub name: String,
    pub schema: Schema,
    pub table: TableHeap,
    pub page_stats: RwLock<HashMap<PageId, PageStats>>,
}

pub struct CatalogManager {
    bpm: Arc<Mutex<BufferPoolManager>>,
    tables: HashMap<String, Arc<TableMetadata>>,
}

impl CatalogManager {
    pub fn new(bpm: Arc<Mutex<BufferPoolManager>>) -> Self {
        Self {
            bpm,
            tables: HashMap::new(),
        }
    }

    pub fn create_table(&mut self, name: String, schema: Schema) -> Arc<TableMetadata> {
        let table = TableHeap::new(self.bpm.clone());
        let metadata = Arc::new(TableMetadata {
            name: name.clone(),
            schema,
            table,
            page_stats: RwLock::new(HashMap::new()),
        });
        
        self.tables.insert(name, metadata.clone());
        metadata
    }

    pub fn get_table(&self, name: &str) -> Option<Arc<TableMetadata>> {
        self.tables.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::disk_manager::DiskManager;
    use crate::catalog::column::Column;
    use crate::types::TypeId;
    use tempfile::NamedTempFile;

    #[test]
    fn test_catalog_manager() {
        let temp_file = NamedTempFile::new().unwrap();
        let dm = DiskManager::new(temp_file.path()).unwrap();
        let bpm = Arc::new(Mutex::new(BufferPoolManager::new(10, dm)));
        
        let mut catalog = CatalogManager::new(bpm);
        
        let schema = Schema::new(vec![
            Column::new("id", TypeId::Integer),
            Column::new("name", TypeId::Varchar),
        ]);
        
        let name = "users".to_string();
        catalog.create_table(name.clone(), schema.clone());
        
        let metadata = catalog.get_table("users").expect("Table should exist");
        assert_eq!(metadata.name, name);
        assert_eq!(metadata.schema, schema);
    }
}