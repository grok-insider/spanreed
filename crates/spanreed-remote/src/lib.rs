use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fabrials_agent_host::{
    HostConfig, HostIdentity,
    control::{self, ControlRequest, ControlResponse},
};
use fabrials_remote_host::{Credentials, state};
use std::{path::PathBuf, sync::Arc};
use zeroize::Zeroizing;

mod doctor;
mod roots;
mod services;

const USAGE: &str = "usage: spanreed remote enable <label>|serve|private-serve <https://host.example> [--web-origin <https://desktop.grok.me|https://grok.fabrials.com>]|doctor [--inspect [--private-origin <https://host.example>]]|status|private-stop|pair|private-pair <https://host.example> [--web-origin <https://desktop.grok.me|https://grok.fabrials.com>]|devices|roots <list|authorize <path>|revoke <root-id>>|revoke <device>|disable|reset|service print|install|uninstall [--private-origin <https://host.example>] [--web-origin <https://desktop.grok.me|https://grok.fabrials.com>]";

fn private_options(args: &[String]) -> Result<(&str, &str), String> {
    let (endpoint, web_origin) = match args {
        [_, endpoint] => (
            endpoint.as_str(),
            fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN,
        ),
        [_, endpoint, flag, web_origin] if flag == "--web-origin" => {
            (endpoint.as_str(), web_origin.as_str())
        }
        _ => return Err(USAGE.into()),
    };
    fabrials_agent_host::origin::validate_private_origin(endpoint)
        .map_err(|_| "invalid_private_origin")?;
    fabrials_agent_host::origin::validate_hosted_web_origin(web_origin)
        .map_err(|_| "invalid_hosted_web_origin")?;
    Ok((endpoint, web_origin))
}

struct Installation;

#[async_trait::async_trait]
impl Credentials for Installation {
    async fn access_token(&self) -> Result<Zeroizing<String>, String> {
        tokio::task::spawn_blocking(spanreed_adapters::fabrials_login::remote_access_token)
            .await
            .map_err(|_| "installation_login_unavailable")?
            .map(Zeroizing::new)
    }
}

fn directory() -> Result<PathBuf, String> {
    fabrials_agent_host::state_dir::resolve_state_dirs()
        .map(|dirs| dirs.state_dir)
        .map_err(|_| "agent_state_unavailable".into())
}

pub fn run(args: &[String]) -> std::process::ExitCode {
    let result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| "runtime_unavailable".to_owned())
        .and_then(|runtime| runtime.block_on(execute(args)));
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("spanreed remote: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn execute(args: &[String]) -> Result<(), String> {
    if args == ["--help"] || args == ["-h"] {
        println!("{USAGE}");
        return Ok(());
    }
    if args == ["--version"] || args == ["-V"] {
        println!("Spanreed Remote {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.starts_with(&["doctor".into(), "--inspect".into()]) {
        return doctor::run(&args[2..]).await;
    }
    if args == ["doctor"] {
        for path in ["/index.html", "/manifest.webmanifest", "/sw.js"] {
            if fabrials_agent_host::assets::lookup(path).is_none() {
                return Err(format!("remote_bundle_incomplete: {path}"));
            }
        }
        let wasm = fabrials_agent_host::assets::ASSETS
            .iter()
            .filter(|asset| {
                asset.path.starts_with("/crypto/")
                    && asset.path.ends_with("/fabrials_remote_noise_bg.wasm")
            })
            .collect::<Vec<_>>();
        if wasm.len() != 1
            || !wasm[0].bytes.starts_with(b"\0asm\x01\0\0\0")
            || fabrials_agent_host::assets::lookup(&wasm[0].path.replace("_bg.wasm", ".js"))
                .is_none()
        {
            return Err("remote_crypto_bundle_incomplete".into());
        }
        println!(
            "Spanreed Remote {}: embedded PWA and Noise WASM present ({} assets).",
            env!("CARGO_PKG_VERSION"),
            fabrials_agent_host::assets::ASSETS.len()
        );
        return Ok(());
    }
    if args.first().is_some_and(|arg| arg == "service") {
        return services::run(&args[1..]);
    }
    let directory = directory()?;
    if args.first().is_some_and(|arg| arg == "roots") {
        return roots::run(&directory, &args[1..]).await;
    }
    let credential = Arc::new(Installation);
    match args.first().map(String::as_str) {
        Some("enable") if args.len() == 2 => {
            let config = state::enable(&directory, &args[1], &*credential).await?;
            println!(
                "Registered {} ({}). Start with `spanreed remote serve`, then pair on this machine.",
                config.label, config.host_id
            );
        }
        Some("serve") if args.len() == 1 => {
            let remote = state::load(&directory)?;
            if !remote.enabled {
                return Err("remote_disabled".into());
            }
            let mut config = HostConfig::new(
                &directory,
                HostIdentity::new("spanreed", env!("CARGO_PKG_VERSION")),
            );
            config.agent_mode = fabrials_agent_host::acp::AgentMode::Shared;
            if let Some(program) = std::env::var_os("FABRIALS_AGENT_PROGRAM") {
                config.agent_program =
                    program.into_string().map_err(|_| "invalid_agent_program")?;
            }
            let running = fabrials_agent_host::start(config)
                .await
                .map_err(|error| error.to_string())?;
            let state = running.state().clone();
            let remote = fabrials_remote_host::run(state.clone(), directory.clone(), credential);
            tokio::select! {
                result = remote => { state.request_shutdown(); result?; },
                result = stop_signal() => { result?; state.request_shutdown(); },
            }
            running.wait().await.map_err(|error| error.to_string())?;
        }
        Some("private-serve") => {
            let (endpoint, web_origin) = private_options(args)?;
            fabrials_remote_host::direct::validate_configuration(&directory, endpoint, web_origin)?;
            let mut config = HostConfig::new(
                &directory,
                HostIdentity::new("spanreed", env!("CARGO_PKG_VERSION")),
            );
            config.agent_mode = fabrials_agent_host::acp::AgentMode::Shared;
            config.private_origin = Some(endpoint.into());
            config.allowed_origins = vec![web_origin.into(), endpoint.into()];
            if let Some(program) = std::env::var_os("FABRIALS_AGENT_PROGRAM") {
                config.agent_program =
                    program.into_string().map_err(|_| "invalid_agent_program")?;
            }
            let running = fabrials_agent_host::start(config)
                .await
                .map_err(|error| error.to_string())?;
            let state = running.state().clone();
            let direct = fabrials_remote_host::direct::prepare_for_web_origin(
                &directory,
                state.origin.install_id(),
                endpoint,
                web_origin,
            );
            let direct = match direct {
                Ok(identity) => identity,
                Err(error) => {
                    state.request_shutdown();
                    let _ = running.wait().await;
                    return Err(error);
                }
            };
            let direct =
                match fabrials_remote_host::direct::DirectHost::new(&directory, direct, &state)
                    .await
                {
                    Ok(direct) => direct,
                    Err(error) => {
                        state.request_shutdown();
                        let _ = running.wait().await;
                        return Err(error);
                    }
                };
            state.enable_direct_connection(Arc::new(direct))?;
            println!(
                "Private host ready. Proxy HTTPS preserving Host to http://127.0.0.1:{}; pair with `spanreed remote private-pair {} --web-origin {}`.",
                state.origin.port(),
                endpoint,
                web_origin
            );
            tokio::select! {
                result = stop_signal() => { result?; state.request_shutdown(); },
                _ = state.shutdown_requested() => {},
            }
            running.wait().await.map_err(|error| error.to_string())?;
        }
        Some("status") if args.len() == 1 => {
            if directory.join("direct.json").exists() {
                let config = fabrials_remote_host::direct::load(&directory)?;
                let running = control::call(&directory, &ControlRequest::Status)
                    .await
                    .is_ok();
                println!(
                    "{}",
                    serde_json::json!({"transport":"direct","installationId":config.installation_id,"endpoint":config.endpoint,"webOrigin":config.web_origin,"publicKey":config.public_key,"configured":true,"agentRunning":running})
                );
                return Ok(());
            }
            let config = state::load(&directory)?;
            let running = control::call(&directory, &ControlRequest::Status)
                .await
                .is_ok();
            let runtime = fabrials_remote_host::status::read(&directory)?;
            let runtime_fresh = running && runtime.as_ref().is_some_and(|status| status.is_fresh());
            println!(
                "{}",
                serde_json::json!({"hostId":config.host_id,"label":config.label,"enabled":config.enabled,"agentRunning":running,"publicKey":config.public_key,"runtime":runtime,"runtimeFresh":runtime_fresh})
            );
        }
        Some("pair") if args.len() == 1 => {
            let config = state::load(&directory)?;
            if !config.enabled {
                return Err("remote_disabled".into());
            }
            let response = control::call(&directory, &ControlRequest::MintNonce)
                .await
                .map_err(|_| "remote_host_not_running")?;
            let ControlResponse::Paired { url, .. } = response else {
                return Err("pairing_unavailable".into());
            };
            let secret = url
                .split("#pair=")
                .nth(1)
                .and_then(|part| part.split('&').next())
                .ok_or("invalid_pairing")?;
            let pair = serde_json::json!({"hostId":config.host_id,"hostPublicKey":config.public_key,"secret":secret});
            let link = format!(
                "https://remote.fabrials.com/#remotePair={}",
                URL_SAFE_NO_PAD.encode(pair.to_string())
            );
            let qr = qrcode::QrCode::new(link.as_bytes()).map_err(|_| "pairing_qr_unavailable")?;
            println!(
                "{}",
                qr.render::<qrcode::render::unicode::Dense1x2>()
                    .quiet_zone(true)
                    .build()
            );
            println!("{link}");
            eprintln!(
                "One-use pairing link expires in 90 seconds. Open it only on the device you are pairing."
            );
        }
        Some("private-pair") => {
            let (endpoint, web_origin) = private_options(args)?;
            fabrials_remote_host::direct::validate_configuration(&directory, endpoint, web_origin)?;
            let identity = fabrials_remote_host::direct::load(&directory)?;
            let response = control::call(&directory, &ControlRequest::MintNonce)
                .await
                .map_err(|_| "remote_host_not_running")?;
            let ControlResponse::Paired { url, .. } = response else {
                return Err("pairing_unavailable".into());
            };
            let nonce = url
                .split("#pair=")
                .nth(1)
                .and_then(|part| part.split('&').next())
                .ok_or("invalid_pairing")?;
            let pair = serde_json::json!({
                "endpoint": identity.endpoint,
                "installationId": identity.installation_id,
                "hostPublicKey": identity.public_key,
                "nonce": nonce,
            });
            let link = format!(
                "{}/#directPair={}",
                identity.web_origin,
                URL_SAFE_NO_PAD.encode(pair.to_string())
            );
            let qr = qrcode::QrCode::new(link.as_bytes()).map_err(|_| "pairing_qr_unavailable")?;
            println!(
                "{}",
                qr.render::<qrcode::render::unicode::Dense1x2>()
                    .quiet_zone(true)
                    .build()
            );
            println!("{link}");
            eprintln!(
                "One-use pairing link expires in 90 seconds. Open it only on the device you are pairing."
            );
        }
        Some("devices") if args.len() == 1 => {
            let response = control::call(&directory, &ControlRequest::ListPairings)
                .await
                .map_err(|_| "remote_host_not_running")?;
            let ControlResponse::Pairings { session_ids } = response else {
                return Err("devices_unavailable".into());
            };
            for id in session_ids {
                println!("{id}");
            }
        }
        Some("revoke") if args.len() == 2 => {
            let response = control::call(
                &directory,
                &ControlRequest::RevokePairing {
                    session_id: args[1].clone(),
                },
            )
            .await
            .map_err(|_| "remote_host_not_running")?;
            if !matches!(response, ControlResponse::Revoked) {
                return Err("revocation_not_durable".into());
            }
            println!("Device revoked.");
        }
        Some("private-stop") if args.len() == 1 => {
            let response = control::call(&directory, &ControlRequest::Stop)
                .await
                .map_err(|_| "remote_host_not_running")?;
            if !matches!(response, ControlResponse::Stopping) {
                return Err("host_stop_unconfirmed".into());
            }
            println!("The private host accepted the stop request. Device pairings are preserved.");
        }
        Some("disable") if args.len() == 1 => {
            let result = state::disable(&directory, &*credential).await;
            let _ = control::call(&directory, &ControlRequest::Stop).await;
            result?;
            println!("Remote access disabled and host registration revoked.");
        }
        Some("reset") if args.len() == 1 => {
            let _host_lock = fabrials_agent_host::instance::InstanceLock::acquire(&directory)
                .map_err(|error| format!("stop_agent_before_reset: {error}"))?;
            state::reset(&directory, &*credential).await?;
            println!(
                "Revoked host identity removed. Link a fresh installation before enabling Remote again."
            );
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

async fn stop_signal() -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|_| "signal_unavailable")?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.map_err(|_| "signal_unavailable".into()),
            _ = term.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|_| "signal_unavailable".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_cli_origin_is_explicit_and_closed() {
        let args = ["private-serve", "https://station.example.ts.net"].map(String::from);
        assert_eq!(
            private_options(&args).unwrap().1,
            fabrials_agent_host::origin::PRODUCTION_WEB_ORIGIN
        );
        let args = [
            "private-pair",
            "https://station.example.ts.net",
            "--web-origin",
            "https://grok.fabrials.com",
        ]
        .map(String::from);
        assert_eq!(
            private_options(&args).unwrap().1,
            fabrials_agent_host::origin::QUALIFICATION_WEB_ORIGIN
        );
        for origin in [
            "https://grok.fabrials.com/",
            "https://grok.fabrials.com.evil.example",
            "https://grok.fabrials.com:443",
            "http://grok.fabrials.com",
        ] {
            let args = [
                "private-serve",
                "https://station.example.ts.net",
                "--web-origin",
                origin,
            ]
            .map(String::from);
            assert_eq!(
                private_options(&args).unwrap_err(),
                "invalid_hosted_web_origin"
            );
        }
        let args = [
            "private-serve",
            "https://station.example.ts.net",
            "--web-origin",
        ]
        .map(String::from);
        assert!(private_options(&args).is_err());
    }
}
