use crate::storage::disk_manager::PageId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexPageType {
    LeafPage,
    InternalPage,
}

pub const INDEX_PAGE_HEADER_SIZE: usize = 24;
pub const SIZE_OF_PAGE_HEADER: usize = 24; // Match above

// Header Layout:
// PageType (4) | CurrentSize (4) | MaxSize (4) | ParentPageId (4) | PageId (4) | NextPageId (4) - mainly for Leaf
// Total: 24 bytes

pub struct BPlusTreePage<'a> {
    pub data: &'a mut [u8],
}

impl<'a> BPlusTreePage<'a> {
    pub fn new(data: &'a mut [u8]) -> Self {
        Self { data }
    }

    pub fn get_page_type(&self) -> IndexPageType {
        let val = u32::from_le_bytes(self.data[0..4].try_into().unwrap());
        if val == 0 { IndexPageType::LeafPage } else { IndexPageType::InternalPage }
    }

    pub fn set_page_type(&mut self, page_type: IndexPageType) {
        let val = match page_type {
            IndexPageType::LeafPage => 0u32,
            IndexPageType::InternalPage => 1u32,
        };
        self.data[0..4].copy_from_slice(&val.to_le_bytes());
    }

    pub fn get_size(&self) -> u32 {
        u32::from_le_bytes(self.data[4..8].try_into().unwrap())
    }

    pub fn set_size(&mut self, size: u32) {
        self.data[4..8].copy_from_slice(&size.to_le_bytes());
    }

    pub fn get_max_size(&self) -> u32 {
        u32::from_le_bytes(self.data[8..12].try_into().unwrap())
    }

    pub fn set_max_size(&mut self, max_size: u32) {
        self.data[8..12].copy_from_slice(&max_size.to_le_bytes());
    }

    pub fn get_parent_page_id(&self) -> PageId {
        u32::from_le_bytes(self.data[12..16].try_into().unwrap())
    }

    pub fn set_parent_page_id(&mut self, parent_id: PageId) {
        self.data[12..16].copy_from_slice(&parent_id.to_le_bytes());
    }

    pub fn get_page_id(&self) -> PageId {
        u32::from_le_bytes(self.data[16..20].try_into().unwrap())
    }

    pub fn set_page_id(&mut self, page_id: PageId) {
        self.data[16..20].copy_from_slice(&page_id.to_le_bytes());
    }
    
    pub fn is_leaf_page(&self) -> bool {
        self.get_page_type() == IndexPageType::LeafPage
    }
    
    pub fn is_root_page(&self) -> bool {
        self.get_parent_page_id() == 0 // Assuming 0 is invalid/null page id for parent
    }
}
