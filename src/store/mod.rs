use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::zset::ZSet;

#[derive(Clone)]
pub enum Value {
    Str(String),
    ZSet(Arc<RwLock<ZSet>>),
}

struct Entry {
    typ: Value,
    exp_at: Option<Instant>,
}

impl Entry {
    fn expired(&self) -> bool {
        self.exp_at
            .map(|t| Instant::now() >= t)
            .unwrap_or(false)
    }
}

#[derive(Clone)]
pub struct Store {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    data: HashMap<String, Entry>,
}

impl Store {
    pub fn new() -> Self {
        Store {
            inner: Arc::new(RwLock::new(Inner {
                data: HashMap::new(),
            })),
        }
    }

    pub fn new_with_expiry() -> (Self, tokio::sync::watch::Sender<()>) {
        let store = Self::new();
        let (tx, mut rx) = tokio::sync::watch::channel(());
        let inner = store.inner.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let mut inner = inner.write().await;
                        let now = Instant::now();
                        inner.data.retain(|_, e| {
                            if let Some(exp) = e.exp_at {
                                now < exp
                            } else {
                                true
                            }
                        });
                    }
                    _ = rx.changed() => break,
                }
            }
        });
        (store, tx)
    }

    pub async fn len(&self) -> usize {
        self.inner.read().await.data.len()
    }

    pub async fn get_str(&self, key: &str) -> (Option<String>, bool) {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) if !e.expired() => match &e.typ {
                Value::Str(s) => (Some(s.clone()), false),
                _ => (None, true),
            },
            _ => (None, false),
        }
    }

    pub async fn set_str(&self, key: String, val: String) {
        let mut inner = self.inner.write().await;
        inner.data.insert(
            key,
            Entry {
                typ: Value::Str(val),
                exp_at: None,
            },
        );
    }

    pub async fn del(&self, key: &str) -> bool {
        let mut inner = self.inner.write().await;
        inner.data.remove(key).is_some()
    }

    pub async fn keys(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        let now = Instant::now();
        inner
            .data
            .iter()
            .filter(|(_, e)| e.exp_at.map_or(true, |t| now < t))
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub async fn expire(&self, key: &str, ms: i64) -> bool {
        let mut inner = self.inner.write().await;
        if let Some(e) = inner.data.get_mut(key) {
            e.exp_at = Some(Instant::now() + std::time::Duration::from_millis(ms as u64));
            true
        } else {
            false
        }
    }

    pub async fn ttl_ms(&self, key: &str) -> i64 {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) => match e.exp_at {
                Some(t) => {
                    let rem = t.saturating_duration_since(Instant::now());
                    rem.as_millis() as i64
                }
                None => -1,
            },
            None => -2,
        }
    }

    pub async fn z_add(&self, key: String, name: String, score: f64) -> (bool, bool) {
        let mut inner = self.inner.write().await;
        match inner.data.get_mut(&key) {
            None => {
                let mut z = ZSet::new();
                z.add(name, score);
                inner.data.insert(
                    key,
                    Entry {
                        typ: Value::ZSet(Arc::new(RwLock::new(z))),
                        exp_at: None,
                    },
                );
                (true, false)
            }
            Some(e) => match &mut e.typ {
                Value::ZSet(zs) => {
                    let mut zs = zs.write().await;
                    (zs.add(name, score), false)
                }
                _ => (false, true),
            },
        }
    }

    pub async fn z_rem(&self, key: &str, name: &str) -> (bool, bool) {
        let mut inner = self.inner.write().await;
        match inner.data.get_mut(key) {
            Some(e) => match &mut e.typ {
                Value::ZSet(zs) => {
                    let mut zs = zs.write().await;
                    (zs.remove(name), false)
                }
                _ => (false, true),
            },
            None => (false, false),
        }
    }

    pub async fn z_score(&self, key: &str, name: &str) -> (Option<f64>, bool) {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) => match &e.typ {
                Value::ZSet(zs) => {
                    let zs = zs.read().await;
                    (zs.score(name), false)
                }
                _ => (None, true),
            },
            None => (None, false),
        }
    }

    pub async fn z_query(
        &self,
        key: &str,
        min_score: f64,
        min_name: &str,
        offset: i64,
        limit: i64,
    ) -> (Vec<crate::zset::Entry>, bool) {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) => match &e.typ {
                Value::ZSet(zs) => {
                    let zs = zs.read().await;
                    (zs.query(min_score, min_name, offset, limit), false)
                }
                _ => (vec![], true),
            },
            None => (vec![], false),
        }
    }
}
