use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct StreamEntry {
    pub id: String,
    pub fields: HashMap<String, String>,
}

pub struct Stream {
    entries: Vec<StreamEntry>,
    next_id: AtomicU64,
}

impl Stream {
    pub fn new() -> Self {
        Stream {
            entries: Vec::new(),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn append(&mut self, fields: HashMap<String, String>) -> String {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let entry_id = format!("{}-0", id);
        self.entries.push(StreamEntry {
            id: entry_id.clone(),
            fields,
        });
        entry_id
    }

    pub fn range(&self, start: &str, end: &str) -> Vec<&StreamEntry> {
        let start_str = start.to_string();
        let end_str = end.to_string();
        self.entries
            .iter()
            .filter(|e| e.id >= start_str && e.id <= end_str)
            .collect()
    }

    pub fn last_n(&self, count: usize) -> Vec<&StreamEntry> {
        let start = self.entries.len().saturating_sub(count);
        self.entries[start..].iter().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn trim(&mut self, max_len: usize) {
        if self.entries.len() > max_len {
            let drain = self.entries.len() - max_len;
            self.entries.drain(..drain);
        }
    }
}

#[derive(Clone)]
pub struct Streams {
    inner: Arc<RwLock<HashMap<String, Stream>>>,
}

impl Streams {
    pub fn new() -> Self {
        Streams {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn append(&self, stream: &str, fields: HashMap<String, String>) -> String {
        let mut inner = self.inner.write().await;
        inner
            .entry(stream.to_string())
            .or_insert_with(Stream::new)
            .append(fields)
    }

    pub async fn range(&self, stream: &str, start: &str, end: &str) -> Vec<StreamEntry> {
        let inner = self.inner.read().await;
        match inner.get(stream) {
            Some(s) => s.range(start, end).into_iter().cloned().collect(),
            None => vec![],
        }
    }

    pub async fn last_n(&self, stream: &str, count: usize) -> Vec<StreamEntry> {
        let inner = self.inner.read().await;
        match inner.get(stream) {
            Some(s) => s.last_n(count).into_iter().cloned().collect(),
            None => vec![],
        }
    }

    pub async fn len(&self, stream: &str) -> usize {
        let inner = self.inner.read().await;
        inner.get(stream).map(|s| s.len()).unwrap_or(0)
    }

    pub async fn trim(&self, stream: &str, max_len: usize) {
        let mut inner = self.inner.write().await;
        if let Some(s) = inner.get_mut(stream) {
            s.trim(max_len);
        }
    }

    pub async fn list_streams(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.keys().cloned().collect()
    }
}
