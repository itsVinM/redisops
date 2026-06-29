use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let srv = redis_rs::server::Server::new(redis_rs::server::Config::default());

    tokio::select! {
        res = srv.listen_and_serve() => {
            if let Err(e) = res {
                tracing::error!("server error: {}", e);
                std::process::exit(1);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutting down");
        }
    }
}
