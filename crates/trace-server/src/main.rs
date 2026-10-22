use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use trace_server::{RateLimits, ServerConfig, app};

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
    let instance_name = env::var("TRACE_INSTANCE_NAME").unwrap_or_else(|_| "TRACE".to_owned());
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
    let rate_limits = RateLimits {
        bootstrap_per_minute: env_limit("TRACE_BOOTSTRAP_PER_MINUTE", 30),
        session_creations_per_minute: env_limit("TRACE_SESSION_CREATIONS_PER_MINUTE", 20),
        publisher_upgrades_per_minute: env_limit("TRACE_PUBLISHER_UPGRADES_PER_MINUTE", 60),
        publisher_messages_per_second: env_limit("TRACE_PUBLISH_MESSAGES_PER_SECOND", 120),
        publisher_bytes_per_second: env_limit("TRACE_PUBLISH_BYTES_PER_SECOND", 8 * 1024 * 1024),
        live_sessions_per_installation: env_limit("TRACE_MAX_LIVE_SESSIONS", 4),
        spectators_per_session: env_limit("TRACE_MAX_SPECTATORS_PER_SESSION", 64),
        spectators_per_address: env_limit("TRACE_MAX_SPECTATORS_PER_ADDRESS", 16),
        installation_records: env_limit("TRACE_MAX_INSTALLATION_RECORDS", 100_000),
        live_session_records: env_limit("TRACE_MAX_LIVE_SESSION_RECORDS", 10_000),
    };
    let trust_proxy_headers = env::var("TRACE_TRUST_PROXY_HEADERS")
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes"));
    let address: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;

    println!("TRACE live service listening on {address}");
    let config = ServerConfig::new(public_base_url)
        .with_instance_name(instance_name)
        .with_share_directory(data_directory)
        .with_live_retention(Duration::from_secs(live_retention_minutes * 60))
        .with_shared_session_ttl(Duration::from_secs(share_ttl_days * 24 * 60 * 60))
        .with_rate_limits(rate_limits)
        .with_trusted_proxy_headers(trust_proxy_headers);
    axum::serve(
        listener,
        app(config).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn env_limit<T>(name: &str, default: T) -> T
where
    T: std::str::FromStr + Copy,
{
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
