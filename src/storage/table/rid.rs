use crate::storage::disk_manager::PageId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RID {
    pub page_id: PageId,
    pub slot_num: u32,
}

impl RID {
    pub fn new(page_id: PageId, slot_num: u32) -> Self {
        Self { page_id, slot_num }
    }
}
