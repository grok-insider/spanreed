//! Local capture command configuration and runtime composition.
use crate::local_relay::LocalRelay;
use fabrials_runtime::listener::{self, ConnectionHost};
use std::net::SocketAddr;
use std::sync::{atomic::AtomicBool, Arc};

pub struct Options {
    pub bind: String,
    pub xai_bind: Option<String>,
    pub watchdog: bool,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            bind: crate::grok_proxy::DEFAULT_GROK_CLI_BIND.into(),
            xai_bind: None,
            watchdog: false,
        };
        let mut bind_seen = false;
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--watchdog" if !options.watchdog => options.watchdog = true,
                "--bind" | "--grok-cli-bind" if !bind_seen => {
                    options.bind = address(args.next(), flag)?;
                    bind_seen = true;
                }
                "--xai-api-bind" if options.xai_bind.is_none() => {
                    options.xai_bind = Some(address(args.next(), flag)?);
                }
                _ => return Err(format!("Unknown or repeated capture option: {flag}")),
            }
        }
        if options.xai_bind.as_ref() == Some(&options.bind) {
            return Err("The compatibility listener needs a different address".into());
        }
        Ok(options)
    }

    pub fn serve(&self) -> Result<(), String> {
        let mut hosts: Vec<Arc<dyn ConnectionHost>> = vec![Arc::new(LocalRelay::new(&self.bind)?)];
        if let Some(bind) = &self.xai_bind {
            hosts.push(Arc::new(LocalRelay::new(bind)?.with_xai_compat()));
        }
        listener::serve_hosts(
            hosts,
            Arc::new(AtomicBool::new(false)),
            64,
            128 * 1024 * 1024,
        )
    }

    pub fn listeners_up(&self) -> bool {
        std::iter::once(&self.bind)
            .chain(self.xai_bind.iter())
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
}

fn address(value: Option<&String>, flag: &str) -> Result<String, String> {
    let address: SocketAddr = value
        .ok_or_else(|| format!("{flag} requires a loopback address"))?
        .parse()
        .map_err(|_| format!("{flag} requires a numeric loopback address"))?;
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(format!(
            "{flag} requires a loopback address and nonzero port"
        ));
    }
    Ok(address.to_string())
}
