//! Development launcher for manual and browser verification.
//!
//! Starts a bare host on a loopback origin, without an agent or state
//! directory, and prints the canonical URL plus a single-use pairing nonce.
//! Not a product entry point; `cli::run` with `serve` and `open` is.

use std::sync::Arc;

use fabrials_agent_host::origin::LocalOrigin;
use fabrials_agent_host::server::{HostState, bind, serve};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let install = std::env::var("LIGHT_INSTALL")
        .unwrap_or_else(|_| fabrials_agent_host::origin::generate_install_id().expect("entropy"));
    let port: u16 = std::env::var("LIGHT_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(24_601);

    let origin = LocalOrigin::new(install.clone(), port)?;
    let state = Arc::new(HostState::new(origin.clone()));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let nonce = {
        let mut broker = state.pairing.lock().await;
        broker
            .mint_nonce(u64::try_from(now).unwrap_or(u64::MAX))?
            .expose()
            .to_owned()
    };

    println!("LIGHT_INSTALL={install}");
    println!("LIGHT_PORT={port}");
    println!("LIGHT_NONCE={nonce}");
    println!("URL={origin}/#pair={nonce}");

    let listeners = bind(&origin).await?;
    serve(listeners, state).await?;
    Ok(())
}
