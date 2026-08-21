use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub type Sender = broadcast::Sender<String>;
pub type Receiver = broadcast::Receiver<String>;

#[derive(Clone)]
pub struct PubSub {
    channels: Arc<RwLock<HashMap<String, Sender>>>,
    max_subscribers: usize,
}

impl PubSub {
    pub fn new() -> Self {
        Self::with_capacity(1024)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        PubSub {
            channels: Arc::new(RwLock::new(HashMap::new())),
            max_subscribers: capacity,
        }
    }

    pub async fn subscribe(&self, channel: &str) -> Receiver {
        let mut channels = self.channels.write().await;
        channels
            .entry(channel.to_string())
            .or_insert_with(|| broadcast::channel(self.max_subscribers).0)
            .subscribe()
    }

    pub async fn publish(&self, channel: &str, message: &str) -> usize {
        let channels = self.channels.read().await;
        match channels.get(channel) {
            Some(tx) => {
                let _ = tx.send(message.to_string());
                tx.receiver_count()
            }
            None => 0,
        }
    }

    pub async fn unsubscribe(&self, channel: &str) {
        let mut channels = self.channels.write().await;
        channels.remove(channel);
    }

    pub async fn channel_count(&self) -> usize {
        let channels = self.channels.read().await;
        channels.len()
    }

    pub async fn subscriber_count(&self, channel: &str) -> usize {
        let channels = self.channels.read().await;
        channels
            .get(channel)
            .map(|tx| tx.receiver_count())
            .unwrap_or(0)
    }
}
