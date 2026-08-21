use super::page::{Page, PageId, PAGE_SIZE};
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub enum WALEntry {
    Write(PageId, Vec<u8>), // page_id, data
    Commit(u64),            // tx_id
    Rollback(u64),          // tx_id
}

pub struct WAL {
    path: String,
    file: Arc<Mutex<Option<File>>>,
    next_tx_id: Arc<Mutex<u64>>,
}

impl WAL {
    pub fn new(path: &str) -> Self {
        WAL {
            path: path.to_string(),
            file: Arc::new(Mutex::new(None)),
            next_tx_id: Arc::new(Mutex::new(1)),
        }
    }

    pub async fn open(&self) -> Result<(), String> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .map_err(|e| format!("WAL open: {}", e))?;

        let mut guard = self.file.lock().await;
        *guard = Some(file);
        Ok(())
    }

    pub async fn append_write(&self, page_id: PageId, data: Vec<u8>) -> Result<(), String> {
        let mut guard = self.file.lock().await;
        let file = guard.as_mut().ok_or("WAL not opened")?;

        // Entry format: [type:u8][tx_id:u64][page_id:u64][len:u32][data]
        file.write_all(&[1u8]).await.map_err(|e| e.to_string())?;

        let tx_id = {
            let tx = self.next_tx_id.lock().await;
            *tx
        };
        file.write_all(&tx_id.to_le_bytes())
            .await
            .map_err(|e| e.to_string())?;
        file.write_all(&page_id.to_le_bytes())
            .await
            .map_err(|e| e.to_string())?;
        file.write_all(&(data.len() as u32).to_le_bytes())
            .await
            .map_err(|e| e.to_string())?;
        file.write_all(&data).await.map_err(|e| e.to_string())?;
        file.flush().await.map_err(|e| e.to_string())?;

        Ok(())
    }

    pub async fn append_commit(&self, tx_id: u64) -> Result<(), String> {
        let mut guard = self.file.lock().await;
        let file = guard.as_mut().ok_or("WAL not opened")?;

        // Entry: [type:u8][tx_id:u64]
        file.write_all(&[2u8]).await.map_err(|e| e.to_string())?;
        file.write_all(&tx_id.to_le_bytes())
            .await
            .map_err(|e| e.to_string())?;
        file.flush().await.map_err(|e| e.to_string())?;

        Ok(())
    }

    pub async fn append_rollback(&self, tx_id: u64) -> Result<(), String> {
        let mut guard = self.file.lock().await;
        let file = guard.as_mut().ok_or("WAL not opened")?;

        file.write_all(&[3u8]).await.map_err(|e| e.to_string())?;
        file.write_all(&tx_id.to_le_bytes())
            .await
            .map_err(|e| e.to_string())?;
        file.flush().await.map_err(|e| e.to_string())?;

        Ok(())
    }

    pub async fn next_tx_id(&self) -> u64 {
        let mut tx = self.next_tx_id.lock().await;
        let id = *tx;
        *tx += 1;
        id
    }

    pub async fn replay(&self) -> Result<Vec<(PageId, Vec<u8>)>, String> {
        let mut file = File::open(&self.path)
            .await
            .map_err(|e| format!("WAL replay open: {}", e))?;

        let mut entries = Vec::new();
        let mut committed_txs = std::collections::HashSet::new();

        loop {
            let mut type_buf = [0u8; 1];
            match file.read_exact(&mut type_buf).await {
                Ok(_) => {}
                Err(_) => break,
            }

            match type_buf[0] {
                1 => {
                    // Write
                    let mut tx_buf = [0u8; 8];
                    file.read_exact(&mut tx_buf)
                        .await
                        .map_err(|e| e.to_string())?;
                    let tx_id = u64::from_le_bytes(tx_buf);

                    let mut page_buf = [0u8; 8];
                    file.read_exact(&mut page_buf)
                        .await
                        .map_err(|e| e.to_string())?;
                    let page_id = u64::from_le_bytes(page_buf);

                    let mut len_buf = [0u8; 4];
                    file.read_exact(&mut len_buf)
                        .await
                        .map_err(|e| e.to_string())?;
                    let len = u32::from_le_bytes(len_buf) as usize;

                    let mut data = vec![0u8; len];
                    file.read_exact(&mut data)
                        .await
                        .map_err(|e| e.to_string())?;

                    if committed_txs.contains(&tx_id) {
                        entries.push((page_id, data));
                    }
                }
                2 => {
                    // Commit
                    let mut tx_buf = [0u8; 8];
                    file.read_exact(&mut tx_buf)
                        .await
                        .map_err(|e| e.to_string())?;
                    let tx_id = u64::from_le_bytes(tx_buf);
                    committed_txs.insert(tx_id);
                }
                3 => {
                    // Rollback
                    let mut tx_buf = [0u8; 8];
                    file.read_exact(&mut tx_buf)
                        .await
                        .map_err(|e| e.to_string())?;
                    let tx_id = u64::from_le_bytes(tx_buf);
                    committed_txs.remove(&tx_id);
                }
                _ => break,
            }
        }

        Ok(entries)
    }

    pub async fn truncate(&self) -> Result<(), String> {
        let mut guard = self.file.lock().await;
        if let Some(file) = guard.as_mut() {
            file.set_len(0).await.map_err(|e| e.to_string())?;
            file.seek(std::io::SeekFrom::Start(0))
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
