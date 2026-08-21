use async_recursion::async_recursion;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::buffer_pool::BufferPool;
use super::page::{Page, PageId, PAGE_SIZE};
use super::wal::WAL;

pub const ORDER: usize = 64;
pub const HEADER_SIZE: usize = 32;
pub const KEY_SIZE: usize = 32;
pub const VALUE_SIZE: usize = 64;

#[derive(Clone)]
struct BTreeNode {
    page_id: PageId,
    is_leaf: bool,
    num_keys: u32,
    parent_id: PageId,
    next_leaf: PageId,
}

impl BTreeNode {
    fn from_page(page: &Page) -> Self {
        BTreeNode {
            page_id: page.id,
            is_leaf: page.read_u32(0) == 1,
            num_keys: page.read_u32(1),
            parent_id: page.read_u64(5),
            next_leaf: page.read_u64(13),
        }
    }

    fn save_to_page(&self, page: &mut Page) {
        page.write_u32(0, if self.is_leaf { 1 } else { 0 });
        page.write_u32(1, self.num_keys);
        page.write_u64(5, self.parent_id);
        page.write_u64(13, self.next_leaf);
    }

    fn key_offset(&self, index: usize) -> usize {
        HEADER_SIZE + index * (KEY_SIZE + VALUE_SIZE)
    }

    fn value_offset(&self, index: usize) -> usize {
        HEADER_SIZE + index * (KEY_SIZE + VALUE_SIZE) + KEY_SIZE
    }

    fn child_offset(&self, index: usize) -> usize {
        HEADER_SIZE + ORDER * (KEY_SIZE + VALUE_SIZE) + index * 8
    }

    fn read_key(&self, page: &Page, index: usize) -> Vec<u8> {
        page.read_slice(self.key_offset(index), KEY_SIZE).to_vec()
    }

    fn write_key(&self, page: &mut Page, index: usize, key: &[u8]) {
        let mut buf = [0u8; KEY_SIZE];
        let len = key.len().min(KEY_SIZE);
        buf[..len].copy_from_slice(&key[..len]);
        page.write_slice(self.key_offset(index), &buf);
    }

    fn read_value(&self, page: &Page, index: usize) -> Vec<u8> {
        page.read_slice(self.value_offset(index), VALUE_SIZE)
            .to_vec()
    }

    fn write_value(&self, page: &mut Page, index: usize, value: &[u8]) {
        let mut buf = [0u8; VALUE_SIZE];
        let len = value.len().min(VALUE_SIZE);
        buf[..len].copy_from_slice(&value[..len]);
        page.write_slice(self.value_offset(index), &buf);
    }

    fn read_child_id(&self, page: &Page, index: usize) -> PageId {
        page.read_u64(self.child_offset(index))
    }

    fn write_child_id(&self, page: &mut Page, index: usize, child_id: PageId) {
        page.write_u64(self.child_offset(index), child_id);
    }
}

pub struct BTree {
    pool: Arc<BufferPool>,
    wal: Arc<WAL>,
    root_id: Arc<RwLock<PageId>>,
    next_page_id: Arc<RwLock<PageId>>,
}

impl BTree {
    pub fn new(pool: Arc<BufferPool>, wal: Arc<WAL>) -> Self {
        BTree {
            pool,
            wal,
            root_id: Arc::new(RwLock::new(0)),
            next_page_id: Arc::new(RwLock::new(1)),
        }
    }

    pub async fn init(&self) -> Result<(), String> {
        let size = self.pool.file_size().await?;
        if size > 0 {
            let entries = self.wal.replay().await?;
            for (page_id, data) in entries {
                let mut page = Page::new(page_id);
                let data_bytes: [u8; PAGE_SIZE] =
                    data.as_slice().try_into().unwrap_or([0u8; PAGE_SIZE]);
                page.data.copy_from_slice(&data_bytes);
                page.dirty = true;
                self.pool.put_page(page).await;
            }
            return Ok(());
        }

        let root = self.alloc_page().await?;
        let mut page = Page::new(root);
        let node = BTreeNode {
            page_id: root,
            is_leaf: true,
            num_keys: 0,
            parent_id: 0,
            next_leaf: 0,
        };
        node.save_to_page(&mut page);
        self.pool.put_page(page).await;

        let mut root_id = self.root_id.write().await;
        *root_id = root;

        Ok(())
    }

    pub async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let root_id = *self.root_id.read().await;
        self.find_value(root_id, key).await
    }

    pub async fn put(&self, key: &[u8], value: &[u8]) -> Result<(), String> {
        let root_id = *self.root_id.read().await;
        let tx_id = self.wal.next_tx_id().await;
        self.insert_into(root_id, key, value, tx_id).await?;
        self.wal.append_commit(tx_id).await
    }

    pub async fn delete(&self, key: &[u8]) -> Result<bool, String> {
        let root_id = *self.root_id.read().await;
        let tx_id = self.wal.next_tx_id().await;
        let result = self.delete_from(root_id, key, tx_id).await?;
        self.wal.append_commit(tx_id).await?;
        Ok(result)
    }

    pub async fn scan(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, String> {
        let root_id = *self.root_id.read().await;
        let mut results = Vec::new();
        self.scan_range(root_id, start, end, &mut results).await?;
        Ok(results)
    }

    pub async fn count(&self) -> Result<usize, String> {
        let root_id = *self.root_id.read().await;
        self.count_keys(root_id).await
    }

    async fn alloc_page(&self) -> Result<PageId, String> {
        let mut next_id = self.next_page_id.write().await;
        let id = *next_id;
        *next_id += 1;
        Ok(id)
    }

    #[async_recursion]
    async fn find_value(&self, page_id: PageId, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);

        if node.is_leaf {
            for i in 0..node.num_keys as usize {
                let k = node.read_key(&page, i);
                if k == key {
                    return Ok(Some(node.read_value(&page, i)));
                }
            }
            return Ok(None);
        }

        for i in 0..node.num_keys as usize {
            let k = node.read_key(&page, i);
            if key < k.as_slice() {
                let child_id = node.read_child_id(&page, i);
                return self.find_value(child_id, key).await;
            }
        }
        let child_id = node.read_child_id(&page, node.num_keys as usize);
        self.find_value(child_id, key).await
    }

    #[async_recursion]
    async fn insert_into(
        &self,
        page_id: PageId,
        key: &[u8],
        value: &[u8],
        tx_id: u64,
    ) -> Result<(), String> {
        let page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);

        if node.is_leaf {
            return self.insert_into_leaf(page_id, key, value, tx_id).await;
        }

        for i in 0..node.num_keys as usize {
            let k = node.read_key(&page, i);
            if key < k.as_slice() {
                let child_id = node.read_child_id(&page, i);
                return self.insert_into(child_id, key, value, tx_id).await;
            }
        }
        let child_id = node.read_child_id(&page, node.num_keys as usize);
        self.insert_into(child_id, key, value, tx_id).await
    }

    async fn insert_into_leaf(
        &self,
        page_id: PageId,
        key: &[u8],
        value: &[u8],
        tx_id: u64,
    ) -> Result<(), String> {
        let mut page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);

        let mut pos = 0;
        for i in 0..node.num_keys as usize {
            let k = node.read_key(&page, i);
            if k == key {
                node.write_value(&mut page, i, value);
                let serialized = page.clone().serialize();
                self.pool.put_page(page).await;
                self.wal.append_write(page_id, serialized).await?;
                return Ok(());
            }
            if key > k.as_slice() {
                pos = i + 1;
            }
        }

        for i in (pos..node.num_keys as usize).rev() {
            let k = node.read_key(&page, i);
            let v = node.read_value(&page, i);
            node.write_key(&mut page, i + 1, &k);
            node.write_value(&mut page, i + 1, &v);
        }

        node.write_key(&mut page, pos, key);
        node.write_value(&mut page, pos, value);

        page.dirty = true;
        let mut new_node = node.clone();
        new_node.num_keys += 1;
        new_node.save_to_page(&mut page);
        let serialized = page.clone().serialize();
        self.pool.put_page(page).await;
        self.wal.append_write(page_id, serialized).await?;

        if new_node.num_keys as usize >= ORDER {
            self.split_leaf(page_id, tx_id).await?;
        }

        Ok(())
    }

    #[async_recursion]
    async fn split_leaf(&self, page_id: PageId, tx_id: u64) -> Result<(), String> {
        let page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);
        let mid = node.num_keys as usize / 2;

        let right_id = self.alloc_page().await?;
        let mut right_page = Page::new(right_id);
        let mut right_node = BTreeNode {
            page_id: right_id,
            is_leaf: true,
            num_keys: 0,
            parent_id: node.parent_id,
            next_leaf: node.next_leaf,
        };

        for i in mid..node.num_keys as usize {
            let k = node.read_key(&page, i);
            let v = node.read_value(&page, i);
            right_node.write_key(&mut right_page, right_node.num_keys as usize, &k);
            right_node.write_value(&mut right_page, right_node.num_keys as usize, &v);
            right_node.num_keys += 1;
        }
        right_node.save_to_page(&mut right_page);
        let cloned = right_page.clone();
        let right_data = cloned.serialize();
        self.pool.put_page(right_page).await;
        self.wal.append_write(right_id, right_data).await?;

        let mut left_page = page.clone();
        left_page.dirty = true;
        let mut left_node = node.clone();
        left_node.num_keys = mid as u32;
        left_node.next_leaf = right_id;
        left_node.save_to_page(&mut left_page);
        let left_cloned = left_page.clone();
        let left_data = left_cloned.serialize();
        self.pool.put_page(left_page).await;
        self.wal.append_write(page_id, left_data).await?;

        let middle_key = node.read_key(&page, mid);
        if node.parent_id == 0 {
            let new_root_id = self.alloc_page().await?;
            let mut root_page = Page::new(new_root_id);
            let root_node = BTreeNode {
                page_id: new_root_id,
                is_leaf: false,
                num_keys: 1,
                parent_id: 0,
                next_leaf: 0,
            };
            root_node.write_key(&mut root_page, 0, &middle_key);
            root_node.write_child_id(&mut root_page, 0, page_id);
            root_node.write_child_id(&mut root_page, 1, right_id);
            root_node.save_to_page(&mut root_page);
            let root_cloned = root_page.clone();
            let root_data = root_cloned.serialize();
            self.pool.put_page(root_page).await;
            self.wal.append_write(new_root_id, root_data).await?;

            let mut root_id = self.root_id.write().await;
            *root_id = new_root_id;
        } else {
            self.insert_into_internal(node.parent_id, &middle_key, page_id, right_id, tx_id)
                .await?;
        }

        Ok(())
    }

    #[async_recursion]
    async fn insert_into_internal(
        &self,
        page_id: PageId,
        key: &[u8],
        left_child: PageId,
        right_child: PageId,
        tx_id: u64,
    ) -> Result<(), String> {
        let mut page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);

        let mut pos = 0;
        for i in 0..node.num_keys as usize {
            let k = node.read_key(&page, i);
            if key > k.as_slice() {
                pos = i + 1;
            }
        }

        for i in (pos..node.num_keys as usize).rev() {
            let k = node.read_key(&page, i);
            let child = node.read_child_id(&page, i + 1);
            node.write_key(&mut page, i + 1, &k);
            node.write_child_id(&mut page, i + 2, child);
        }

        node.write_key(&mut page, pos, key);
        node.write_child_id(&mut page, pos, left_child);
        node.write_child_id(&mut page, pos + 1, right_child);

        page.dirty = true;
        let mut new_node = node.clone();
        new_node.num_keys += 1;
        new_node.save_to_page(&mut page);
        let serialized = page.clone().serialize();
        self.pool.put_page(page).await;
        self.wal.append_write(page_id, serialized).await?;

        if new_node.num_keys as usize >= ORDER {
            self.split_internal(page_id, tx_id).await?;
        }

        Ok(())
    }

    #[async_recursion]
    async fn split_internal(&self, page_id: PageId, tx_id: u64) -> Result<(), String> {
        let page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);
        let mid = node.num_keys as usize / 2;

        let right_id = self.alloc_page().await?;
        let mut right_page = Page::new(right_id);
        let mut right_node = BTreeNode {
            page_id: right_id,
            is_leaf: false,
            num_keys: 0,
            parent_id: node.parent_id,
            next_leaf: 0,
        };

        for i in (mid + 1)..node.num_keys as usize {
            let k = node.read_key(&page, i);
            let child = node.read_child_id(&page, i);
            right_node.write_key(&mut right_page, right_node.num_keys as usize, &k);
            right_node.write_child_id(&mut right_page, right_node.num_keys as usize, child);
            right_node.num_keys += 1;
        }
        let last_child = node.read_child_id(&page, node.num_keys as usize);
        right_node.write_child_id(&mut right_page, right_node.num_keys as usize, last_child);

        right_node.save_to_page(&mut right_page);
        let right_cloned = right_page.clone();
        let right_data = right_cloned.serialize();
        self.pool.put_page(right_page).await;
        self.wal.append_write(right_id, right_data).await?;

        let mut left_page = page.clone();
        left_page.dirty = true;
        let mut left_node = node.clone();
        left_node.num_keys = mid as u32;
        left_node.save_to_page(&mut left_page);
        let left_cloned = left_page.clone();
        let left_data = left_cloned.serialize();
        self.pool.put_page(left_page).await;
        self.wal.append_write(page_id, left_data).await?;

        let middle_key = node.read_key(&page, mid);
        if node.parent_id == 0 {
            let new_root_id = self.alloc_page().await?;
            let mut root_page = Page::new(new_root_id);
            let root_node = BTreeNode {
                page_id: new_root_id,
                is_leaf: false,
                num_keys: 1,
                parent_id: 0,
                next_leaf: 0,
            };
            root_node.write_key(&mut root_page, 0, &middle_key);
            root_node.write_child_id(&mut root_page, 0, page_id);
            root_node.write_child_id(&mut root_page, 1, right_id);
            root_node.save_to_page(&mut root_page);
            let root_cloned = root_page.clone();
            let root_data = root_cloned.serialize();
            self.pool.put_page(root_page).await;
            self.wal.append_write(new_root_id, root_data).await?;

            let mut root_id = self.root_id.write().await;
            *root_id = new_root_id;
        } else {
            self.insert_into_internal(node.parent_id, &middle_key, page_id, right_id, tx_id)
                .await?;
        }

        Ok(())
    }

    #[async_recursion]
    async fn delete_from(&self, page_id: PageId, key: &[u8], tx_id: u64) -> Result<bool, String> {
        let page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);

        if node.is_leaf {
            return self.delete_from_leaf(page_id, key, tx_id).await;
        }

        for i in 0..node.num_keys as usize {
            let k = node.read_key(&page, i);
            if key <= k.as_slice() {
                let child_id = node.read_child_id(&page, i);
                return self.delete_from(child_id, key, tx_id).await;
            }
        }
        let child_id = node.read_child_id(&page, node.num_keys as usize);
        self.delete_from(child_id, key, tx_id).await
    }

    async fn delete_from_leaf(
        &self,
        page_id: PageId,
        key: &[u8],
        _tx_id: u64,
    ) -> Result<bool, String> {
        let mut page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);

        let mut found = false;
        let mut pos = 0;
        for i in 0..node.num_keys as usize {
            let k = node.read_key(&page, i);
            if k == key {
                found = true;
                pos = i;
                break;
            }
        }

        if !found {
            return Ok(false);
        }

        for i in pos..(node.num_keys as usize - 1) {
            let k = node.read_key(&page, i + 1);
            let v = node.read_value(&page, i + 1);
            node.write_key(&mut page, i, &k);
            node.write_value(&mut page, i, &v);
        }

        page.dirty = true;
        let mut new_node = node.clone();
        new_node.num_keys -= 1;
        new_node.save_to_page(&mut page);
        let serialized = page.clone().serialize();
        self.pool.put_page(page).await;

        Ok(true)
    }

    #[async_recursion]
    async fn scan_range(
        &self,
        page_id: PageId,
        start: &[u8],
        end: &[u8],
        results: &mut Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Result<(), String> {
        let page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);

        if node.is_leaf {
            for i in 0..node.num_keys as usize {
                let k = node.read_key(&page, i);
                if k.as_slice() >= start && k.as_slice() <= end {
                    let v = node.read_value(&page, i);
                    results.push((k, v));
                }
            }
            return Ok(());
        }

        for i in 0..node.num_keys as usize {
            let k = node.read_key(&page, i);
            if start < k.as_slice() {
                let child_id = node.read_child_id(&page, i);
                self.scan_range(child_id, start, end, results).await?;
            }
        }
        let child_id = node.read_child_id(&page, node.num_keys as usize);
        self.scan_range(child_id, start, end, results).await
    }

    #[async_recursion]
    async fn count_keys(&self, page_id: PageId) -> Result<usize, String> {
        let page = self.pool.get_page(page_id).await?;
        let node = BTreeNode::from_page(&page);

        if node.is_leaf {
            return Ok(node.num_keys as usize);
        }

        let mut count = 0;
        for i in 0..=node.num_keys as usize {
            let child_id = node.read_child_id(&page, i);
            count += self.count_keys(child_id).await?;
        }
        Ok(count)
    }
}
