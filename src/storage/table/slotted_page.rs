use crate::storage::disk_manager::PAGE_SIZE;
use crate::storage::page::Page;
use crate::storage::tuple::Tuple;

/// Header size: 
/// - page_id (4 bytes) - skipped for now, kept in wrapper
/// - lsn (4 bytes) - Log Sequence Number (for recovery)
/// - free_space_pointer (2 bytes) - Offset to the start of free space (grows down)
/// - slot_count (2 bytes) - Number of slots
/// - next_page_id (4 bytes) - for table scans
const HEADER_SIZE: usize = 12;

/// Slotted Page Layout:
/// ---------------------------------------------------------
/// | Header | Slot Array ... | ... Free Space ... | Tuples |
/// ---------------------------------------------------------
/// 
/// Header: [LSN (4), FreeSpacePtr (2), SlotCount (2), NextPageId (4)]
/// Slot: [Offset (2), Length (2)]
pub struct SlottedPage<'a> {
    data: &'a mut [u8; PAGE_SIZE],
}

impl<'a> SlottedPage<'a> {
    pub fn new(page: &'a mut Page) -> Self {
        Self { data: &mut page.data }
    }

    /// Initialize a new page (clear header)
    pub fn init(&mut self, next_page_id: u32) {
        self.set_lsn(0);
        self.set_free_space_pointer(PAGE_SIZE as u16);
        self.set_slot_count(0);
        self.set_next_page_id(next_page_id);
    }
    
    // --- Header Accessors ---

    fn get_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes(self.data[offset..offset+2].try_into().unwrap())
    }

    fn set_u16(&mut self, offset: usize, value: u16) {
        self.data[offset..offset+2].copy_from_slice(&value.to_le_bytes());
    }

    fn _get_u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset+4].try_into().unwrap())
    }

    fn set_u32(&mut self, offset: usize, value: u32) {
        self.data[offset..offset+4].copy_from_slice(&value.to_le_bytes());
    }

    pub fn get_free_space_pointer(&self) -> u16 {
        self.get_u16(4)
    }

    fn set_free_space_pointer(&mut self, ptr: u16) {
        self.set_u16(4, ptr);
    }

    pub fn get_slot_count(&self) -> u16 {
        self.get_u16(6)
    }

    fn set_slot_count(&mut self, count: u16) {
        self.set_u16(6, count);
    }

    pub fn set_lsn(&mut self, lsn: u32) {
        self.set_u32(0, lsn);
    }
    
    pub fn set_next_page_id(&mut self, pid: u32) {
        self.set_u32(8, pid);
    }

    pub fn get_next_page_id(&self) -> u32 {
        self._get_u32(8)
    }

    // --- Slot Operations ---

    /// Tries to insert a tuple. Returns the slot index if successful, None if not enough space.
    pub fn insert_tuple(&mut self, tuple: &Tuple) -> Option<u16> {
        let size = tuple.size() as u16;
        let free_space = self.get_free_space_remaining();
        
        // Need space for: Tuple data + Slot entry (4 bytes)
        if free_space < size + 4 {
            return None;
        }

        let slot_id = self.get_slot_count();
        let free_ptr = self.get_free_space_pointer();
        
        let new_offset = free_ptr - size;
        
        // Write tuple data
        self.data[new_offset as usize..(new_offset + size) as usize].copy_from_slice(tuple.to_bytes());
        
        // Update header
        self.set_free_space_pointer(new_offset);
        self.set_slot_count(slot_id + 1);
        
        // Write slot info
        self.set_slot(slot_id, new_offset, size);

        Some(slot_id)
    }
    
    pub fn get_tuple(&self, slot_id: u16) -> Option<Tuple> {
        if slot_id >= self.get_slot_count() {
            return None;
        }

        let (offset, size) = self.get_slot(slot_id);
        
        // Deleted tuple check (we can use size 0 or offset 0 to mark deletion)
        if size == 0 {
            return None;
        }

        let data = self.data[offset as usize..(offset + size) as usize].to_vec();
        Some(Tuple::new(data))
    }

    fn get_free_space_remaining(&self) -> u16 {
        let free_ptr = self.get_free_space_pointer();
        let header_end = HEADER_SIZE as u16 + (self.get_slot_count() * 4); // 4 bytes per slot
        
        if free_ptr < header_end {
            0 // Should not happen in healthy page
        } else {
            free_ptr - header_end
        }
    }

    fn set_slot(&mut self, slot_id: u16, offset: u16, length: u16) {
        let slot_offset = HEADER_SIZE + (slot_id as usize * 4);
        self.set_u16(slot_offset, offset);
        self.set_u16(slot_offset + 2, length);
    }
    
    fn get_slot(&self, slot_id: u16) -> (u16, u16) {
        let slot_offset = HEADER_SIZE + (slot_id as usize * 4);
        let offset = self.get_u16(slot_offset);
        let length = self.get_u16(slot_offset + 2);
        (offset, length)
    }

    pub fn mark_delete(&mut self, slot_id: u16) -> bool {
        if slot_id >= self.get_slot_count() {
            return false;
        }
        // Set length to 0 to mark as deleted
        let slot_offset = HEADER_SIZE + (slot_id as usize * 4);
        self.set_u16(slot_offset + 2, 0); 
        true
    }

    pub fn rollback_delete(&mut self, slot_id: u16, original_len: u16) -> bool {
        if slot_id >= self.get_slot_count() {
            return false;
        }
        let slot_offset = HEADER_SIZE + (slot_id as usize * 4);
        self.set_u16(slot_offset + 2, original_len);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slotted_page_insert_read() {
        let mut page = Page::new();
        let mut slotted = SlottedPage::new(&mut page);
        slotted.init(0);

        let t1 = Tuple::new(b"Hello".to_vec());
        let t2 = Tuple::new(b"World".to_vec());

        let sid1 = slotted.insert_tuple(&t1).unwrap();
        let sid2 = slotted.insert_tuple(&t2).unwrap();

        assert_eq!(sid1, 0);
        assert_eq!(sid2, 1);

        let rt1 = slotted.get_tuple(sid1).unwrap();
        let rt2 = slotted.get_tuple(sid2).unwrap();

        assert_eq!(rt1.data, b"Hello");
        assert_eq!(rt2.data, b"World");
        
        // Verify free space shrunk
        // 4096 - 12 (header) - 8 (slots) - 10 (data) = 4066 roughly
        assert!(slotted.get_free_space_remaining() < 4096);
    }
}
