use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct Stats {
    pub total_connections: AtomicU64,
    pub active_connections: AtomicI64,
    pub total_commands: AtomicU64,
    pub last_cmds: Mutex<VecDeque<String>>,
    pub started_at: Instant,
    pub addr: Mutex<String>,
}

impl Stats {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            total_connections: AtomicU64::new(0),
            active_connections: AtomicI64::new(0),
            total_commands: AtomicU64::new(0),
            last_cmds: Mutex::new(VecDeque::with_capacity(64)),
            started_at: Instant::now(),
            addr: Mutex::new(String::new()),
        })
    }

    pub fn inc_conn(&self) {
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec_conn(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn inc_cmd(&self, cmd: String) {
        self.total_commands.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut q) = self.last_cmds.lock() {
            if q.len() >= 64 {
                q.pop_front();
            }
            q.push_back(cmd);
        }
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let cmds = self.last_cmds.lock().map(|q| q.iter().cloned().collect()).unwrap_or(Vec::new());
        StatsSnapshot {
            total_connections: self.total_connections.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            total_commands: self.total_commands.load(Ordering::Relaxed),
            last_cmds: cmds,
            uptime_secs: self.started_at.elapsed().as_secs(),
            addr: self.addr.lock().map(|a| a.clone()).unwrap_or_default(),
        }
    }
}

pub struct StatsSnapshot {
    pub total_connections: u64,
    pub active_connections: i64,
    pub total_commands: u64,
    pub last_cmds: Vec<String>,
    pub uptime_secs: u64,
    pub addr: String,
}
