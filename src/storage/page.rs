use crate::storage::disk_manager::{PageId, PAGE_SIZE};

/// FrameId is the index of the page in the buffer pool.
pub type FrameId = usize;

/// Page represents a page in memory.
/// It wraps the raw data and metadata (pin_count, dirty flag).
#[derive(Clone)]
pub struct Page {
    pub id: Option<PageId>,
    pub pin_count: u32,
    pub is_dirty: bool,
    pub data: [u8; PAGE_SIZE],
}

impl Page {
    pub fn new() -> Self {
        Self {
            id: None,
            pin_count: 0,
            is_dirty: false,
            data: [0; PAGE_SIZE],
        }
    }

    pub fn reset_memory(&mut self) {
        self.data = [0; PAGE_SIZE];
    }
}
