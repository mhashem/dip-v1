use crate::catalog::schema::Schema;
use crate::catalog::stats::PageStats;
use crate::storage::buffer_pool_manager::BufferPoolManager;
use crate::storage::disk_manager::PageId;
use crate::storage::index::b_plus_tree::BPlusTree;
use crate::storage::table::table_heap::TableHeap;
use crate::types::TypeId;
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::{Read, Write};
use std::sync::{Arc, RwLock};

pub struct TableMetadata {
    pub name: String,
    pub schema: Schema,
    pub table: TableHeap,
    pub bpm: Arc<BufferPoolManager>,
    pub page_stats: RwLock<HashMap<PageId, PageStats>>,
    /// Map of Column Index -> B+Tree Index
    pub indexes: RwLock<HashMap<usize, Arc<RwLock<BPlusTree>>>>,
}

#[derive(Clone)]
pub struct CatalogManager {
    pub bpm: Arc<BufferPoolManager>,
    tables: Arc<RwLock<HashMap<String, Arc<TableMetadata>>>>,
}

impl CatalogManager {
    pub fn new(bpm: Arc<BufferPoolManager>) -> Self {
        Self {
            bpm,
            tables: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn create_table(&mut self, name: String, schema: Schema) -> Arc<TableMetadata> {
        let table = TableHeap::new(self.bpm.clone());
        let indexes = HashMap::new();
        
        // Initial Primary Key Index
        let metadata = TableMetadata {
            name: name.clone(),
            schema: schema.clone(),
            table,
            bpm: self.bpm.clone(),
            page_stats: RwLock::new(HashMap::new()),
            indexes: RwLock::new(indexes),
        };

        if let Some(pk_idx) = schema.get_primary_key_index() {
            if schema.columns[pk_idx].type_id == TypeId::Integer {
                let index = Arc::new(RwLock::new(BPlusTree::new(self.bpm.clone())));
                metadata.indexes.write().unwrap().insert(pk_idx, index);
            }
        }

        let metadata_arc = Arc::new(metadata);
        self.tables.write().unwrap().insert(name, metadata_arc.clone());
        metadata_arc
    }

    pub fn get_table(&self, name: &str) -> Option<Arc<TableMetadata>> {
        self.tables.read().unwrap().get(name).cloned()
    }

    pub fn save_metadata(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        
        // Magic
        file.write_all(b"DIPM")?;
        
        let tables = self.tables.read().unwrap();
        // Num Tables
        file.write_all(&(tables.len() as u32).to_le_bytes())?;
        
        for (name, metadata) in tables.iter() {
            // Name
            file.write_all(&(name.len() as u32).to_le_bytes())?;
            file.write_all(name.as_bytes())?;
            
            // Schema
            let schema_bytes = metadata.schema.to_bytes();
            file.write_all(&schema_bytes)?;
            
            // Root Page ID
            let root_pid = metadata.table.get_first_page_id();
            file.write_all(&(root_pid as u32).to_le_bytes())?;
            
            // Num Indexes
            let indexes = metadata.indexes.read().unwrap();
            file.write_all(&(indexes.len() as u32).to_le_bytes())?;
            
            for (col_idx, index) in indexes.iter() {
                file.write_all(&(*col_idx as u32).to_le_bytes())?;
                let index_root = index.read().unwrap().get_root_page_id();
                file.write_all(&(index_root as u32).to_le_bytes())?;
            }
            
            // Zone Maps
            let stats = metadata.page_stats.read().unwrap();
            file.write_all(&(stats.len() as u32).to_le_bytes())?;
            
            for (pid, p_stats) in stats.iter() {
                file.write_all(&(*pid as u32).to_le_bytes())?;
                file.write_all(&p_stats.to_bytes())?;
            }
        }
        
        Ok(())
    }

    pub fn load_metadata(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        println!("CatalogManager: Loading metadata from {:?}", path);
        if !path.exists() {
            println!("CatalogManager: Metadata file does not exist");
            return Ok(());
        }
        let mut file = std::fs::File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        println!("CatalogManager: Read {} bytes", buffer.len());
        
        let mut offset = 0;
        if buffer.len() < 4 || &buffer[0..4] != b"DIPM" {
             return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid magic"));
        }
        offset += 4;
        
        if offset + 4 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading num tables")); }
        let num_tables = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as usize;
        offset += 4;
        println!("CatalogManager: Found {} tables", num_tables);
        
        let mut tables_map = HashMap::new();
        for _ in 0..num_tables {
            // Name
            if offset + 4 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading name len")); }
            let name_len = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            
            if offset + name_len > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading name")); }
            let name = String::from_utf8(buffer[offset..offset+name_len].to_vec())
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            offset += name_len;
            
            // Schema
            let (schema, len) = Schema::from_bytes(&buffer[offset..])
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid schema"))?;
            offset += len;
            
            // Root Page ID
            if offset + 4 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading root pid")); }
            let root_pid = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as PageId;
            offset += 4;
            
            // Num Indexes
            if offset + 4 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading num indexes")); }
            let num_indexes = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            
            let indexes_map = RwLock::new(HashMap::new());
            {
                let mut map = indexes_map.write().unwrap();
                for _ in 0..num_indexes {
                    if offset + 8 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading index entry")); }
                    let col_idx = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as usize;
                    offset += 4;
                    let index_root = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as PageId;
                    offset += 4;
                    
                    let index = Arc::new(RwLock::new(BPlusTree::from_root_page_id(self.bpm.clone(), index_root)));
                    map.insert(col_idx, index);
                }
            }
            
            // Reconstruct Table
            let table = TableHeap::from_first_page_id(self.bpm.clone(), root_pid);
            let page_stats_map = RwLock::new(HashMap::new());
            
            // Zone Maps
            if offset + 4 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading num stats")); }
            let num_stats = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as usize;
            offset += 4;
            
            {
                let mut map = page_stats_map.write().unwrap();
                for _ in 0..num_stats {
                     if offset + 4 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading stat pid")); }
                     let pid = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as PageId;
                     offset += 4;
                     
                     let (p_stats, s_len) = PageStats::from_bytes(&buffer[offset..])
                        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid stats"))?;
                     offset += s_len;
                     
                     map.insert(pid, p_stats);
                }
            }
            
            let metadata = Arc::new(TableMetadata {
                name: name.clone(),
                schema,
                table,
                bpm: self.bpm.clone(),
                page_stats: page_stats_map,
                indexes: indexes_map,
            });
            
            tables_map.insert(name, metadata);
        }
        
        *self.tables.write().unwrap() = tables_map;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::column::Column;
    use crate::storage::disk_manager::DiskManager;
    use crate::types::TypeId;
    use tempfile::NamedTempFile;

    #[test]
    fn test_catalog_manager() {
        let temp_file = NamedTempFile::new().unwrap();
        let dm = DiskManager::new(temp_file.path()).unwrap();
        // Remove Mutex
        let bpm = Arc::new(BufferPoolManager::new(10, dm));
        
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