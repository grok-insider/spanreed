//! `spanreed grok-proxy [--bind HOST:PORT]`.

use std::process::ExitCode;

use crate::app::AppContext;

/// Compatibility alias: `grok-proxy --bind A` is `capture serve --grok-cli-bind A`
/// on the shared runtime. `--upstream` is no longer supported.
pub(super) fn run(ctx: &AppContext, args: &[String]) -> ExitCode {
    match grok_proxy_serve_args(args) {
        Ok(serve) => super::capture::run(ctx, &serve),
        Err(error) => {
            eprintln!("grok-proxy: {error}");
            ExitCode::FAILURE
        }
    }
}

fn grok_proxy_serve_args(args: &[String]) -> Result<Vec<String>, String> {
    let mut serve = vec!["serve".to_string()];
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--bind" => {
                let address = args.next().ok_or("--bind needs HOST:PORT")?;
                serve.push("--grok-cli-bind".into());
                serve.push(address.clone());
            }
            other => {
                return Err(format!(
                    "unsupported option {other}; use `spanreed capture serve`"
                ));
            }
        }
    }
    Ok(serve)
}

#[cfg(test)]
mod grok_proxy_alias_tests {
    use super::grok_proxy_serve_args;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn bind_becomes_the_capture_fabric_bind() {
        assert_eq!(
            grok_proxy_serve_args(&args(&["--bind", "127.0.0.1:19000"])).unwrap(),
            args(&["serve", "--grok-cli-bind", "127.0.0.1:19000"])
        );
        assert_eq!(grok_proxy_serve_args(&[]).unwrap(), args(&["serve"]));
    }

    #[test]
    fn the_retired_upstream_override_is_refused() {
        assert!(grok_proxy_serve_args(&args(&["--upstream", "https://example.test"])).is_err());
        assert!(grok_proxy_serve_args(&args(&["--bind"])).is_err());
    }

    #[test]
    fn the_alias_options_parse_as_capture_options() {
        let serve = grok_proxy_serve_args(&args(&["--bind", "127.0.0.1:19001"])).unwrap();
        let options = crate::app::capture::Options::parse(&serve[1..]).unwrap();
        assert_eq!(options.bind, "127.0.0.1:19001");
        assert!(options.xai_bind.is_none());
    }
}
