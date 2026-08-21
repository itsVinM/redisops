use std::sync::Arc;

use super::btree::BTree;
use super::buffer_pool::BufferPool;
use super::wal::WAL;

pub struct Storage {
    pub btree: BTree,
    pool: Arc<BufferPool>,
    wal: Arc<WAL>,
}

impl Storage {
    pub fn new(db_path: &str) -> Self {
        let pool = Arc::new(BufferPool::new(db_path, 1024));
        let wal = Arc::new(WAL::new(&format!("{}.wal", db_path)));
        let btree = BTree::new(pool.clone(), wal.clone());

        Storage { btree, pool, wal }
    }

    pub async fn init(&self) -> Result<(), String> {
        self.wal.open().await?;
        self.btree.init().await
    }

    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
        self.btree.get(key).await
    }

    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), String> {
        self.btree.put(key, value).await
    }

    pub async fn delete(&self, key: &[u8]) -> Result<bool, String> {
        self.btree.delete(key).await
    }

    pub async fn scan(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
        self.btree.scan(start, end).await
    }

    pub async fn count(&self) -> Result<usize, String> {
        self.btree.count().await
    }

    pub async fn flush(&self) -> Result<(), String> {
        self.pool.flush_all().await
    }

    pub async fn page_count(&self) -> usize {
        self.pool.page_count().await
    }
}
