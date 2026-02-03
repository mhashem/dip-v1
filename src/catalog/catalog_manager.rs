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
    pub page_stats: RwLock<HashMap<PageId, PageStats>>,
    pub index: Option<Arc<RwLock<BPlusTree>>>,
}

pub struct CatalogManager {
    bpm: Arc<BufferPoolManager>,
    tables: HashMap<String, Arc<TableMetadata>>,
}

impl CatalogManager {
    pub fn new(bpm: Arc<BufferPoolManager>) -> Self {
        Self {
            bpm,
            tables: HashMap::new(),
        }
    }

    pub fn create_table(&mut self, name: String, schema: Schema) -> Arc<TableMetadata> {
        let table = TableHeap::new(self.bpm.clone());
        
        // Check for Index (Primary Key is Integer)
        let index = if let Some(pk_idx) = schema.get_primary_key_index() {
            if schema.columns[pk_idx].type_id == TypeId::Integer {
                Some(Arc::new(RwLock::new(BPlusTree::new(self.bpm.clone()))))
            } else {
                None
            }
        } else {
            None
        };

        let metadata = Arc::new(TableMetadata {
            name: name.clone(),
            schema,
            table,
            page_stats: RwLock::new(HashMap::new()),
            index,
        });
        
        self.tables.insert(name, metadata.clone());
        metadata
    }

    pub fn get_table(&self, name: &str) -> Option<Arc<TableMetadata>> {
        self.tables.get(name).cloned()
    }

    pub fn save_metadata(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        
        // Magic
        file.write_all(b"DIPM")?;
        
        // Num Tables
        file.write_all(&(self.tables.len() as u32).to_le_bytes())?;
        
        for (name, metadata) in &self.tables {
            // Name
            file.write_all(&(name.len() as u32).to_le_bytes())?;
            file.write_all(name.as_bytes())?;
            
            // Schema
            let schema_bytes = metadata.schema.to_bytes();
            file.write_all(&schema_bytes)?;
            
            // Root Page ID
            let root_pid = metadata.table.get_first_page_id();
            file.write_all(&(root_pid as u32).to_le_bytes())?;
            
            // Index Root Page ID (0 if None)
            let index_root = if let Some(idx) = &metadata.index {
                idx.read().unwrap().get_root_page_id()
            } else {
                0
            };
            file.write_all(&(index_root as u32).to_le_bytes())?;
            
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
        if !path.exists() {
            return Ok(());
        }
        let mut file = std::fs::File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        
        let mut offset = 0;
        if buffer.len() < 4 || &buffer[0..4] != b"DIPM" {
             return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid magic"));
        }
        offset += 4;
        
        if offset + 4 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading num tables")); }
        let num_tables = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as usize;
        offset += 4;
        
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
            
            // Index Root Page ID
            if offset + 4 > buffer.len() { return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "EOF reading index root")); }
            let index_root = u32::from_le_bytes(buffer[offset..offset+4].try_into().unwrap()) as PageId;
            offset += 4;
            
            let index = if index_root != 0 {
                Some(Arc::new(RwLock::new(BPlusTree::from_root_page_id(self.bpm.clone(), index_root))))
            } else {
                None
            };
            
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
                page_stats: page_stats_map,
                index,
            });
            
            self.tables.insert(name, metadata);
        }
        
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