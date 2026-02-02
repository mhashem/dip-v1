use crate::storage::page::FrameId;
use std::collections::VecDeque;
use std::sync::Mutex;

/// Replacer tracks page usage and decides which page to evict when the buffer is full.
pub trait Replacer {
    /// Remove the victim frame as defined by the replacement policy.
    fn victim(&self) -> Option<FrameId>;

    /// Pin a frame, indicating that it is currently in use.
    /// Pinned frames are NOT candidates for eviction.
    fn pin(&self, frame_id: FrameId);

    /// Unpin a frame, indicating that it is no longer in use.
    /// It becomes a candidate for eviction.
    fn unpin(&self, frame_id: FrameId);

    /// Returns the number of elements in the replacer that can be victimized.
    fn size(&self) -> usize;
}

/// LRUReplacer implements the Least Recently Used policy.
/// It tracks frames that are *unpinned* (i.e., candidates for eviction).
pub struct LRUReplacer {
    // We use a VecDeque to simulate the LRU queue.
    // Front = Least Recently Used (Victim)
    // Back = Most Recently Used
    // NOTE: In a production DB, we'd want a Map + DoublyLinkedList for O(1) operations.
    // For simplicity/education with Rust's ownership, we use a simple VecDeque protected by a Mutex.
    // Since buffer pool size is usually finite and small-ish in this toy DB, O(N) scan for pin/unpin is acceptable.
    frames: Mutex<VecDeque<FrameId>>,
}

impl LRUReplacer {
    pub fn new(_num_pages: usize) -> Self {
        Self {
            frames: Mutex::new(VecDeque::new()),
        }
    }
}

impl Replacer for LRUReplacer {
    fn victim(&self) -> Option<FrameId> {
        let mut frames = self.frames.lock().unwrap();
        frames.pop_front()
    }

    fn pin(&self, frame_id: FrameId) {
        let mut frames = self.frames.lock().unwrap();
        // If it's in the LRU list, remove it (it's now pinned and safe from eviction)
        if let Some(pos) = frames.iter().position(|&x| x == frame_id) {
            frames.remove(pos);
        }
    }

    fn unpin(&self, frame_id: FrameId) {
        let mut frames = self.frames.lock().unwrap();
        // Add to the back (Most Recently Used) if not already there
        if !frames.contains(&frame_id) {
            frames.push_back(frame_id);
        }
    }

    fn size(&self) -> usize {
        let frames = self.frames.lock().unwrap();
        frames.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_replacer() {
        let replacer = LRUReplacer::new(10);

        // No candidates yet
        assert!(replacer.victim().is_none());

        // Unpin frames (make them candidates)
        replacer.unpin(1);
        replacer.unpin(2);
        replacer.unpin(3);

        // Order should be 1, 2, 3 (1 is LRU)
        assert_eq!(replacer.size(), 3);

        // Pin 1 (someone used it), now candidates are 2, 3
        replacer.pin(1);
        assert_eq!(replacer.size(), 2);

        // Victim should be 2
        assert_eq!(replacer.victim(), Some(2));
        
        // Victim should be 3
        assert_eq!(replacer.victim(), Some(3));

        // 1 was pinned, so it's not a victim
        assert!(replacer.victim().is_none());
        
        // Unpin 1 again
        replacer.unpin(1);
        assert_eq!(replacer.victim(), Some(1));
    }
}
