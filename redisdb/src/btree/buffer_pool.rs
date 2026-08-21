use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::RwLock;

use super::page::{Page, PageId, PAGE_SIZE};

pub struct BufferPool {
    pages: Arc<RwLock<HashMap<PageId, Page>>>,
    lru_queue: Arc<RwLock<VecDeque<PageId>>>,
    capacity: usize,
    db_path: String,
}

impl BufferPool {
    pub fn new(db_path: &str, capacity: usize) -> Self {
        BufferPool {
            pages: Arc::new(RwLock::new(HashMap::new())),
            lru_queue: Arc::new(RwLock::new(VecDeque::new())),
            capacity,
            db_path: db_path.to_string(),
        }
    }

    pub async fn get_page(&self, page_id: PageId) -> Result<Page, String> {
        // Check buffer pool first
        {
            let pages = self.pages.read().await;
            if let Some(page) = pages.get(&page_id) {
                self.touch_lru(page_id).await;
                return Ok(page.clone());
            }
        }

        // Load from disk
        let page = self.load_page(page_id).await?;

        // Insert into pool
        {
            let mut pages = self.pages.write().await;
            pages.insert(page_id, page.clone());
        }

        // Evict if over capacity
        self.evict_if_needed().await;

        Ok(page)
    }

    pub async fn put_page(&self, page: Page) {
        let page_id = page.id;
        let mut pages = self.pages.write().await;
        pages.insert(page_id, page);
        self.touch_lru(page_id).await;
    }

    pub async fn mark_dirty(&self, page_id: PageId) {
        let mut pages = self.pages.write().await;
        if let Some(page) = pages.get_mut(&page_id) {
            page.dirty = true;
        }
    }

    pub async fn flush_page(&self, page_id: PageId) -> Result<(), String> {
        let page = {
            let pages = self.pages.read().await;
            match pages.get(&page_id) {
                Some(p) => p.clone(),
                None => return Ok(()),
            }
        };

        if !page.dirty {
            return Ok(());
        }

        self.save_page(&page).await?;

        let mut pages = self.pages.write().await;
        if let Some(p) = pages.get_mut(&page_id) {
            p.dirty = false;
        }

        Ok(())
    }

    pub async fn flush_all(&self) -> Result<(), String> {
        let dirty_ids: Vec<PageId> = {
            let pages = self.pages.read().await;
            pages
                .iter()
                .filter(|(_, p)| p.dirty)
                .map(|(id, _)| *id)
                .collect()
        };

        for id in dirty_ids {
            self.flush_page(id).await?;
        }
        Ok(())
    }

    pub async fn page_count(&self) -> usize {
        let pages = self.pages.read().await;
        pages.len()
    }

    pub async fn file_size(&self) -> Result<u64, String> {
        match tokio::fs::metadata(&self.db_path).await {
            Ok(m) => Ok(m.len()),
            Err(_) => Ok(0),
        }
    }

    async fn load_page(&self, page_id: PageId) -> Result<Page, String> {
        let mut file = File::open(&self.db_path)
            .await
            .map_err(|e| format!("open: {}", e))?;

        let offset = page_id as usize * PAGE_SIZE;
        file.seek(std::io::SeekFrom::Start(offset as u64))
            .await
            .map_err(|e| format!("seek: {}", e))?;

        let mut buf = vec![0u8; PAGE_SIZE];
        file.read_exact(&mut buf)
            .await
            .map_err(|e| format!("read: {}", e))?;

        Ok(Page::deserialize(page_id, &buf))
    }

    async fn save_page(&self, page: &Page) -> Result<(), String> {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&self.db_path)
            .await
            .map_err(|e| format!("open: {}", e))?;

        let offset = page.id as usize * PAGE_SIZE;
        file.seek(std::io::SeekFrom::Start(offset as u64))
            .await
            .map_err(|e| format!("seek: {}", e))?;

        file.write_all(&page.serialize())
            .await
            .map_err(|e| format!("write: {}", e))?;

        Ok(())
    }

    async fn touch_lru(&self, page_id: PageId) {
        let mut lru = self.lru_queue.write().await;
        lru.retain(|&id| id != page_id);
        lru.push_back(page_id);
    }

    async fn evict_if_needed(&self) {
        let should_evict = {
            let pages = self.pages.read().await;
            pages.len() > self.capacity
        };

        if should_evict {
            let evict_id = {
                let mut lru = self.lru_queue.write().await;
                lru.pop_front()
            };

            if let Some(id) = evict_id {
                self.flush_page(id).await.ok();
                let mut pages = self.pages.write().await;
                pages.remove(&id);
            }
        }
    }
}
