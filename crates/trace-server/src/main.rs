use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use trace_server::{ServerConfig, app};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args_os()
        .nth(1)
        .is_some_and(|argument| matches!(argument.to_str(), Some("--version" | "-V")))
    {
        println!("trace-server {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let bind = env::var("TRACE_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let public_base_url =
        env::var("TRACE_PUBLIC_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_owned());
    let data_directory = env::var_os("TRACE_DATA_DIR")
        .map_or_else(|| PathBuf::from("trace-server-data"), PathBuf::from);
    let share_ttl_days = env::var("TRACE_SHARE_TTL_DAYS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=365).contains(value))
        .unwrap_or(7);
    let live_retention_minutes = env::var("TRACE_LIVE_RETENTION_MINUTES")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| (1..=10).contains(value))
        .unwrap_or(10);
    let address: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;

    println!("TRACE live service listening on {address}");
    let config = ServerConfig::new(public_base_url)
        .with_share_directory(data_directory)
        .with_live_retention(Duration::from_secs(live_retention_minutes * 60))
        .with_shared_session_ttl(Duration::from_secs(share_ttl_days * 24 * 60 * 60));
    axum::serve(listener, app(config)).await?;
    Ok(())
}
