use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::handler;
use crate::pubsub::PubSub;
use crate::stats::Stats;
use crate::store::Store;
use crate::streams::Streams;

pub struct HttpApi {
    store: Store,
    pubsub: PubSub,
    streams: Streams,
    stats: Arc<Stats>,
    port: u16,
}

impl HttpApi {
    pub fn new(
        store: Store,
        pubsub: PubSub,
        streams: Streams,
        stats: Arc<Stats>,
        port: u16,
    ) -> Self {
        HttpApi {
            store,
            pubsub,
            streams,
            stats,
            port,
        }
    }

    pub async fn start(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", self.port)).await?;
        info!("HTTP API on port {}", self.port);

        let store = self.store;
        let pubsub = self.pubsub;
        let streams = self.streams;
        let stats = self.stats;

        loop {
            let (mut stream, peer) = match listener.accept().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("accept error: {}", e);
                    continue;
                }
            };

            let store = store.clone();
            let pubsub = pubsub.clone();
            let streams = streams.clone();
            let stats = stats.clone();

            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = match stream.read(&mut buf).await {
                    Ok(n) if n > 0 => n,
                    _ => return,
                };

                let request = String::from_utf8_lossy(&buf[..n]);
                let response = handle_request(&request, &store, &pubsub, &streams, &stats).await;
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    }
}

async fn handle_request(
    request: &str,
    store: &Store,
    pubsub: &PubSub,
    streams: &Streams,
    stats: &Stats,
) -> String {
    let first_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() < 2 {
        return http_response(400, "Bad Request");
    }

    let path = parts[1];

    match path {
        "/health" => http_response(200, "ok"),
        "/status" => {
            let snap = stats.snapshot();
            let body = format!(
                "{{\"connections\":{},\"commands\":{},\"uptime\":{}}}",
                snap.active_connections, snap.total_commands, snap.uptime_secs
            );
            http_json(200, &body)
        }
        "/stats" => {
            let snap = stats.snapshot();
            let body = format!(
                "{{\"total_connections\":{},\"active_connections\":{},\"total_commands\":{},\"last_cmds\":{}}}",
                snap.total_connections, snap.active_connections, snap.total_commands,
                snap.last_cmds.len()
            );
            http_json(200, &body)
        }
        "/pubsub/channels" => {
            let count = pubsub.channel_count().await;
            http_json(200, &format!("{{\"channels\":{}}}", count))
        }
        "/streams" => {
            let list = streams.list_streams().await;
            let items: Vec<String> = list.into_iter().map(|s| format!("\"{}\"", s)).collect();
            http_json(200, &format!("[{}]", items.join(",")))
        }
        "/metrics" => {
            let snap = stats.snapshot();
            let body = format!(
                "# HELP redisops_connections Total connections\n\
                 # TYPE redisops_connections counter\n\
                 redisops_connections {}\n\
                 # HELP redisops_commands_total Total commands\n\
                 # TYPE redisops_commands_total counter\n\
                 redisops_commands_total {}\n",
                snap.total_connections, snap.total_commands
            );
            http_response(200, &body)
        }
        _ => http_response(404, "Not Found"),
    }
}

fn http_response(code: u16, body: &str) -> String {
    let status = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code, status, body.len(), body
    )
}

fn http_json(code: u16, body: &str) -> String {
    let status = match code {
        200 => "OK",
        _ => "Not Found",
    };
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code, status, body.len(), body
    )
}
