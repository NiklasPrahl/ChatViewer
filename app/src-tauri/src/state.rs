//! Open archives, kept in memory only for the session (ADR 0003).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cv_core::{ChatArchive, ChatSummary};
use cv_model::{ConversationData, ConversationId};

pub struct OpenArchive {
    pub archive: Box<dyn ChatArchive>,
    pub importer: &'static str,
    pub chats: Vec<ChatSummary>,
    /// The chat shown last, decoded once and paged from memory.
    pub loaded: Option<(ConversationId, Arc<ConversationData>)>,
}

/// Recently opened media, so seeking in audio/video does not decrypt the
/// file again for every range request.
#[derive(Default)]
pub struct BlobCache {
    entries: VecDeque<(String, Arc<Vec<u8>>)>,
    bytes: usize,
}

impl BlobCache {
    const MAX_BYTES: usize = 256 * 1024 * 1024;

    pub fn get(&self, key: &str) -> Option<Arc<Vec<u8>>> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    pub fn put(&mut self, key: String, data: Arc<Vec<u8>>) {
        self.bytes += data.len();
        self.entries.push_back((key, data));
        while self.bytes > Self::MAX_BYTES && self.entries.len() > 1 {
            if let Some((_, old)) = self.entries.pop_front() {
                self.bytes -= old.len();
            }
        }
    }

    pub fn clear_prefix(&mut self, prefix: &str) {
        self.entries.retain(|(k, _)| !k.starts_with(prefix));
        self.bytes = self.entries.iter().map(|(_, v)| v.len()).sum();
    }
}

#[derive(Default)]
pub struct AppState {
    pub archives: Mutex<HashMap<u32, OpenArchive>>,
    pub blobs: Mutex<BlobCache>,
    next: AtomicU32,
}

impl AppState {
    pub fn insert(&self, archive: OpenArchive) -> u32 {
        let handle = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.archives
            .lock()
            .expect("state lock")
            .insert(handle, archive);
        handle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_cache_evicts_oldest_beyond_limit() {
        let mut cache = BlobCache::default();
        let big = Arc::new(vec![0u8; BlobCache::MAX_BYTES / 2 + 1]);
        cache.put("1/a".into(), big.clone());
        cache.put("1/b".into(), big.clone());
        assert!(cache.get("1/a").is_none());
        assert!(cache.get("1/b").is_some());
        cache.clear_prefix("1/");
        assert!(cache.get("1/b").is_none());
    }
}
