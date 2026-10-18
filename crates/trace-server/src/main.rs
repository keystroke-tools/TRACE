use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use trace_server::{ServerConfig, app};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let address: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;

    println!("TRACE live service listening on {address}");
    let config = ServerConfig::new(public_base_url)
        .with_share_directory(data_directory)
        .with_shared_session_ttl(Duration::from_secs(share_ttl_days * 24 * 60 * 60));
    axum::serve(listener, app(config)).await?;
    Ok(())
}
