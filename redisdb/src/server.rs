use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, info, warn};

use crate::handler;
use crate::handler::AuthConfig;
use crate::proto;
use crate::stats::Stats;
use crate::store::Store;

pub struct Config {
    pub addr: String,
    pub max_connections: usize,
    pub read_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            addr: "127.0.0.1:1234".into(),
            max_connections: 1000,
            read_timeout: Duration::from_secs(30),
        }
    }
}

pub struct Server {
    cfg: Config,
    store: Store,
    sem: Arc<Semaphore>,
    stats: Arc<Stats>,
    auth: Arc<RwLock<AuthConfig>>,
}

impl Server {
    fn max_sem(cfg: &Config) -> Arc<Semaphore> {
        let max = if cfg.max_connections == 0 {
            1000
        } else {
            cfg.max_connections
        };
        Arc::new(Semaphore::new(max))
    }

    pub fn new(cfg: Config) -> Self {
        let (store, _shutdown_tx) = Store::new_with_expiry();
        let sem = Self::max_sem(&cfg);
        Server {
            cfg,
            store,
            sem,
            stats: Stats::new(),
            auth: Arc::new(RwLock::new(AuthConfig::default())),
        }
    }

    pub fn new_with_stats(cfg: Config, stats: Arc<Stats>) -> Self {
        let (store, _shutdown_tx) = Store::new_with_expiry();
        let sem = Self::max_sem(&cfg);
        Server {
            cfg,
            store,
            sem,
            stats,
            auth: Arc::new(RwLock::new(AuthConfig::default())),
        }
    }

    pub fn store(&self) -> Store {
        self.store.clone()
    }

    pub fn stats(&self) -> Arc<Stats> {
        self.stats.clone()
    }

    pub async fn listen_and_serve(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(&self.cfg.addr).await?;
        if let Ok(mut a) = self.stats.addr.lock() {
            *a = self.cfg.addr.clone();
        }
        info!("listening on {}", self.cfg.addr);
        self.serve(listener).await
    }

    pub async fn serve(
        self,
        listener: TcpListener,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        loop {
            let permit = match self.sem.clone().acquire_owned().await {
                Ok(p) => p,
                Err(_) => continue,
            };

            let (stream, peer) = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("accept error: {}", e);
                    continue;
                }
            };
            debug!("accepted connection from {}", peer);

            if let Err(e) = stream.set_nodelay(true) {
                warn!("set_nodelay error: {}", e);
            }

            let store = self.store.clone();
            let timeout = self.cfg.read_timeout;
            let stats = self.stats.clone();
            let auth = self.auth.clone();

            stats.inc_conn();

            tokio::spawn(async move {
                if let Err(e) = handle_conn(stream, store, timeout, stats.clone(), auth).await {
                    debug!("connection error from {}: {}", peer, e);
                }
                stats.dec_conn();
                drop(permit);
            });
        }
    }
}

async fn handle_conn(
    mut stream: TcpStream,
    store: Store,
    read_timeout: Duration,
    stats: Arc<Stats>,
    auth: Arc<RwLock<AuthConfig>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
        let cmd = tokio::time::timeout(read_timeout, proto::read_request(&mut stream)).await;
        let args = match cmd {
            Ok(Ok(args)) => args,
            Ok(Err(e)) => {
                debug!("read error: {}", e);
                return Ok(());
            }
            Err(_) => {
                debug!("read timeout");
                return Ok(());
            }
        };

        let cmd_name = args.first().cloned().unwrap_or_default();
        stats.inc_cmd(cmd_name);

        let resp = handler::dispatch(store.clone(), args, auth.clone()).await;
        proto::write_response(&mut stream, &resp).await?;
    }
}
