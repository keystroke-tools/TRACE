use std::{env, net::SocketAddr};

use trace_server::{ServerConfig, app};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = env::var("TRACE_BIND").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let public_base_url =
        env::var("TRACE_PUBLIC_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_owned());
    let address: SocketAddr = bind.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;

    println!("TRACE live service listening on {address}");
    axum::serve(listener, app(ServerConfig::new(public_base_url))).await?;
    Ok(())
}
