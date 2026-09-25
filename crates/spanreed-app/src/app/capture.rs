//! The capture fabric on 127.0.0.1:18736 (service, health and log).
use std::net::SocketAddr;

use crate::context::AppContext;

/// Default capture fabric listener (Grok Build and xAI clients point here).
pub const DEFAULT_GROK_CLI_BIND: &str = "127.0.0.1:18736";
/// Optional compatibility listener that treats bare `/v1` as xAI.
pub const DEFAULT_XAI_API_BIND: &str = "127.0.0.1:18737";

pub struct Options {
    pub bind: String,
    pub xai_bind: Option<String>,
    pub watchdog: bool,
}

impl Options {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self {
            bind: DEFAULT_GROK_CLI_BIND.into(),
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

/// Port: the capture listener and its user service (process/service manager).
pub trait CaptureService: Send + Sync {
    /// Start capture (with its watchdog) when the ports are down.
    fn ensure(&self, dry_run: bool) -> Result<String, String>;
    fn is_up(&self) -> bool;
    fn log_path(&self) -> std::path::PathBuf;
    fn log(&self, message: &str);
    /// Serve in this process until the listeners stop.
    fn serve(&self, options: &Options) -> Result<(), String>;
    /// Keep a capture worker (`serve_args`) alive.
    fn watchdog(&self, serve_args: &[String]) -> Result<(), String>;
}

fn service(ctx: &AppContext) -> &dyn CaptureService {
    ctx.services().capture.as_ref()
}

pub fn ensure(ctx: &AppContext, dry_run: bool) -> Result<String, String> {
    service(ctx).ensure(dry_run)
}

pub fn is_up(ctx: &AppContext) -> bool {
    service(ctx).is_up()
}

pub fn log_path(ctx: &AppContext) -> std::path::PathBuf {
    service(ctx).log_path()
}

/// Serve capture in this process, or keep a worker alive with `watchdog`.
pub fn serve(ctx: &AppContext, options: &Options, serve_args: &[String]) -> Result<(), String> {
    let capture = service(ctx);
    if options.watchdog {
        return capture.watchdog(serve_args).inspect_err(|error| {
            capture.log(&format!("watchdog fatal: {error}"));
        });
    }
    capture.log(&format!("capture serve integrated bind={}", options.bind));
    capture
        .serve(options)
        .inspect_err(|error| capture.log(&format!("capture error: {error}")))
}
