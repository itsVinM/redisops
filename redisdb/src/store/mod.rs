use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::zset::ZSet;

#[derive(Clone)]
pub enum Value {
    Str(String),
    Bytes(Vec<u8>),
    ZSet(Arc<RwLock<ZSet>>),
    List(Arc<RwLock<Vec<String>>>),
}

struct Entry {
    typ: Value,
    exp_at: Option<Instant>,
}

impl Entry {
    fn expired(&self) -> bool {
        self.exp_at.map(|t| Instant::now() >= t).unwrap_or(false)
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
            let mut interval = tokio::time::interval(Duration::from_millis(100));
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
                Value::Bytes(b) => (Some(String::from_utf8_lossy(b).to_string()), false),
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
            e.exp_at = Some(Instant::now() + Duration::from_millis(ms as u64));
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

    // ── ZSet ──

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

    // ── List (for queues) ──

    pub async fn lpush(&self, key: String, val: String) {
        let mut inner = self.inner.write().await;
        match inner.data.get_mut(&key) {
            Some(e) => match &mut e.typ {
                Value::List(list) => {
                    let mut list = list.write().await;
                    list.insert(0, val);
                }
                _ => {}
            },
            None => {
                let list = Arc::new(RwLock::new(vec![val]));
                inner.data.insert(
                    key,
                    Entry {
                        typ: Value::List(list),
                        exp_at: None,
                    },
                );
            }
        }
    }

    pub async fn rpop(&self, key: &str) -> Option<String> {
        let mut inner = self.inner.write().await;
        match inner.data.get_mut(key) {
            Some(e) => match &mut e.typ {
                Value::List(list) => {
                    let mut list = list.write().await;
                    if list.is_empty() {
                        None
                    } else {
                        Some(list.pop().unwrap())
                    }
                }
                _ => None,
            },
            None => None,
        }
    }

    pub async fn lpop(&self, key: &str) -> Option<String> {
        let mut inner = self.inner.write().await;
        match inner.data.get_mut(key) {
            Some(e) => match &mut e.typ {
                Value::List(list) => {
                    let mut list = list.write().await;
                    if list.is_empty() {
                        None
                    } else {
                        Some(list.remove(0))
                    }
                }
                _ => None,
            },
            None => None,
        }
    }

    pub async fn lpush_all(&self, key: String, vals: Vec<String>) {
        let mut inner = self.inner.write().await;
        match inner.data.get_mut(&key) {
            Some(e) => match &mut e.typ {
                Value::List(list) => {
                    let mut list = list.write().await;
                    for v in vals.into_iter().rev() {
                        list.insert(0, v);
                    }
                }
                _ => {}
            },
            None => {
                let list = Arc::new(RwLock::new(vals));
                inner.data.insert(
                    key,
                    Entry {
                        typ: Value::List(list),
                        exp_at: None,
                    },
                );
            }
        }
    }

    pub async fn lrange(&self, key: &str, start: i64, stop: i64) -> Vec<String> {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) => match &e.typ {
                Value::List(list) => {
                    let list = list.read().await;
                    let len = list.len() as i64;
                    let s = if start < 0 {
                        (len + start).max(0)
                    } else {
                        start
                    } as usize;
                    let e = if stop < 0 {
                        (len + stop + 1).max(0)
                    } else {
                        (stop + 1).min(len)
                    } as usize;
                    if s >= e {
                        vec![]
                    } else {
                        list[s..e].to_vec()
                    }
                }
                _ => vec![],
            },
            None => vec![],
        }
    }

    pub async fn lindex(&self, key: &str, idx: i64) -> Option<String> {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) => match &e.typ {
                Value::List(list) => {
                    let list = list.read().await;
                    let len = list.len() as i64;
                    let i = if idx < 0 { len + idx } else { idx };
                    if i >= 0 && (i as usize) < list.len() {
                        Some(list[i as usize].clone())
                    } else {
                        None
                    }
                }
                _ => None,
            },
            None => None,
        }
    }

    pub async fn llen(&self, key: &str) -> i64 {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) => match &e.typ {
                Value::List(list) => {
                    let list = list.read().await;
                    list.len() as i64
                }
                _ => -1,
            },
            None => 0,
        }
    }

    pub async fn lrem(&self, key: &str, count: i64, val: &str) -> i64 {
        let mut inner = self.inner.write().await;
        match inner.data.get_mut(key) {
            Some(e) => match &mut e.typ {
                Value::List(list) => {
                    let mut list = list.write().await;
                    if count == 0 {
                        let before = list.len();
                        list.retain(|v| v != val);
                        (before - list.len()) as i64
                    } else if count > 0 {
                        let mut removed = 0i64;
                        list.retain(|v| {
                            if v == val && removed < count {
                                removed += 1;
                                false
                            } else {
                                true
                            }
                        });
                        removed
                    } else {
                        let mut removed = 0i64;
                        let abs_count = (-count) as usize;
                        let mut i = list.len();
                        while i > 0 && removed < abs_count as i64 {
                            i -= 1;
                            if list[i] == val {
                                list.remove(i);
                                removed += 1;
                            }
                        }
                        removed
                    }
                }
                _ => 0,
            },
            None => 0,
        }
    }

    // ── Bitfield (compact state storage) ──

    pub async fn set_bit(&self, key: &str, bit: u32, on: bool) {
        let mut inner = self.inner.write().await;
        let byte_idx = (bit / 8) as usize;
        let bit_idx = bit % 8;

        match inner.data.get_mut(key) {
            Some(e) => match &mut e.typ {
                Value::Bytes(b) => {
                    if byte_idx >= b.len() {
                        b.resize(byte_idx + 1, 0);
                    }
                    if on {
                        b[byte_idx] |= 1 << bit_idx;
                    } else {
                        b[byte_idx] &= !(1 << bit_idx);
                    }
                }
                Value::Str(s) => {
                    let mut bytes = s.as_bytes().to_vec();
                    if byte_idx >= bytes.len() {
                        bytes.resize(byte_idx + 1, 0);
                    }
                    if on {
                        bytes[byte_idx] |= 1 << bit_idx;
                    } else {
                        bytes[byte_idx] &= !(1 << bit_idx);
                    }
                    e.typ = Value::Bytes(bytes);
                }
                _ => {}
            },
            None => {
                let mut bytes = vec![0u8; byte_idx + 1];
                if on {
                    bytes[byte_idx] |= 1 << bit_idx;
                }
                inner.data.insert(
                    key.to_string(),
                    Entry {
                        typ: Value::Bytes(bytes),
                        exp_at: None,
                    },
                );
            }
        }
    }

    pub async fn get_bit(&self, key: &str, bit: u32) -> bool {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) if !e.expired() => {
                let bytes: &[u8] = match &e.typ {
                    Value::Bytes(b) => b,
                    Value::Str(s) => s.as_bytes(),
                    _ => return false,
                };
                let byte_idx = (bit / 8) as usize;
                let bit_idx = bit % 8;
                if byte_idx < bytes.len() {
                    (bytes[byte_idx] >> bit_idx) & 1 == 1
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub async fn bitcount(&self, key: &str) -> i64 {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) if !e.expired() => {
                let bytes: &[u8] = match &e.typ {
                    Value::Bytes(b) => b,
                    Value::Str(s) => s.as_bytes(),
                    _ => return 0,
                };
                bytes.iter().map(|b| b.count_ones() as i64).sum()
            }
            _ => 0,
        }
    }

    pub async fn bitfield_get(&self, key: &str, offset: u32, width: u32) -> u64 {
        let inner = self.inner.read().await;
        match inner.data.get(key) {
            Some(e) if !e.expired() => {
                let bytes: &[u8] = match &e.typ {
                    Value::Bytes(b) => b,
                    Value::Str(s) => s.as_bytes(),
                    _ => return 0,
                };
                let mut result: u64 = 0;
                for i in 0..width {
                    let bit = offset + i;
                    let byte_idx = (bit / 8) as usize;
                    let bit_idx = bit % 8;
                    if byte_idx < bytes.len() && (bytes[byte_idx] >> bit_idx) & 1 == 1 {
                        result |= 1 << i;
                    }
                }
                result
            }
            _ => 0,
        }
    }

    pub async fn bitfield_set(&self, key: &str, offset: u32, width: u32, val: u64) {
        for i in 0..width {
            let bit = offset + i;
            let on = (val >> i) & 1 == 1;
            self.set_bit(key, bit, on).await;
        }
    }
}
