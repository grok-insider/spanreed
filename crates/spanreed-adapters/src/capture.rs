//! Local capture command configuration and runtime composition.
use crate::local_relay::LocalRelay;
use fabrials_fabric::listener::{self, ConnectionHost};
use std::net::SocketAddr;
use std::sync::{Arc, atomic::AtomicBool};

pub use spanreed_app::app::capture::{DEFAULT_GROK_CLI_BIND, Options};

/// Serve the capture listeners of `options` in this process.
pub fn serve(options: &Options) -> Result<(), String> {
    let mut hosts: Vec<Arc<dyn ConnectionHost>> = vec![Arc::new(LocalRelay::new(&options.bind)?)];
    if let Some(bind) = &options.xai_bind {
        hosts.push(Arc::new(LocalRelay::new(bind)?.with_xai_compat()));
    }
    listener::serve_hosts(
        hosts,
        Arc::new(AtomicBool::new(false)),
        64,
        128 * 1024 * 1024,
    )
}

/// Every listener of `options` accepts a connection.
pub fn listeners_up(options: &Options) -> bool {
    std::iter::once(&options.bind)
        .chain(options.xai_bind.iter())
        .all(|bind| {
            bind.parse::<SocketAddr>().is_ok_and(|address| {
                std::net::TcpStream::connect_timeout(
                    &address,
                    std::time::Duration::from_millis(300),
                )
                .is_ok()
            })
        })
}
