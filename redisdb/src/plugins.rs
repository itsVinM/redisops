use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type PluginFn = Box<dyn Fn(&[String]) -> Vec<u8> + Send + Sync>;

pub struct Plugin {
    pub name: String,
    pub description: String,
    pub handler: PluginFn,
}

#[derive(Clone)]
pub struct Plugins {
    plugins: Arc<RwLock<HashMap<String, Plugin>>>,
}

impl Plugins {
    pub fn new() -> Self {
        Plugins {
            plugins: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, name: &str, description: &str, handler: PluginFn) {
        let mut plugins = self.plugins.write().await;
        plugins.insert(
            name.to_string(),
            Plugin {
                name: name.to_string(),
                description: description.to_string(),
                handler,
            },
        );
    }

    pub async fn execute(&self, name: &str, args: &[String]) -> Option<Vec<u8>> {
        let plugins = self.plugins.read().await;
        match plugins.get(name) {
            Some(plugin) => Some((plugin.handler)(args)),
            None => None,
        }
    }

    pub async fn list(&self) -> Vec<(String, String)> {
        let plugins = self.plugins.read().await;
        plugins
            .values()
            .map(|p| (p.name.clone(), p.description.clone()))
            .collect()
    }

    pub async fn has(&self, name: &str) -> bool {
        let plugins = self.plugins.read().await;
        plugins.contains_key(name)
    }
}

// ── Built-in plugins ──

pub fn register_builtins(plugins: &Plugins) {
    let p = plugins.clone();

    // PING
    tokio::spawn(async move {
        p.register(
            "ping",
            "Simple ping/pong",
            Box::new(|_| b"+PONG\r\n".to_vec()),
        )
        .await;
    });

    let p = plugins.clone();
    tokio::spawn(async move {
        p.register(
            "time",
            "Current server time",
            Box::new(|_| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                format!(":{}\r\n", now).into_bytes()
            }),
        )
        .await;
    });

    let p = plugins.clone();
    tokio::spawn(async move {
        p.register(
            "echo",
            "Echo back arguments",
            Box::new(|args| {
                if args.is_empty() {
                    return b"$-1\r\n".to_vec();
                }
                let msg = args.join(" ");
                format!("${}\r\n{}\r\n", msg.len(), msg).into_bytes()
            }),
        )
        .await;
    });

    let p = plugins.clone();
    tokio::spawn(async move {
        p.register(
            "uuid",
            "Generate a UUID v4",
            Box::new(|_| {
                let t = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let uuid = format!("{:032x}", t);
                format!("+{}\r\n", uuid).into_bytes()
            }),
        )
        .await;
    });
}
