//! CodeRabbit review usage from the local CLI.
//!
//! Runs a bounded `coderabbit usage`. CodexBar does not read the CLI's
//! credential files, and neither does this provider.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::env_any;
use crate::providers::Provider;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ID: &str = "coderabbit";
const NAME: &str = "CodeRabbit";

pub struct CodeRabbit;

fn cli_path() -> Option<PathBuf> {
    if let Some(path) = env_any(&["CODERABBIT_CLI_PATH"]) {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        for name in ["coderabbit", "coderabbit.exe"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub(super) fn parse_usage(text: &str) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let Some((label, value)) = raw.split_once(':') else {
            continue;
        };
        let label = label.trim();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match label {
            "Organization" => {
                lines.push(crate::providers::json_api::text_line("Organization", value))
            }
            "Usage billing" => lines.push(crate::providers::json_api::text_line("Billing", value)),
            "User" => lines.push(crate::providers::json_api::text_line("User", value)),
            "Your reviews" => lines.push(crate::providers::json_api::text_line("Reviews", value)),
            "Period resets" => lines.push(crate::providers::json_api::text_line(
                "Period resets",
                value,
            )),
            "Plan" => lines.push(crate::providers::json_api::text_line("Plan", value)),
            _ => {}
        }
    }
    lines
}

fn run_usage(path: &std::path::Path) -> Result<String, String> {
    let mut child = Command::new(path)
        .arg("usage")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "coderabbit usage could not be started.".to_string())?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                if !status.success() {
                    return Err("coderabbit usage failed. Run `coderabbit auth login`.".into());
                }
                return Ok(out);
            }
            Ok(None) if started.elapsed() > Duration::from_secs(8) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("coderabbit usage timed out.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(err.to_string()),
        }
    }
}

impl Provider for CodeRabbit {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        cli_path().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(path) = cli_path() else {
            return ProviderOutput::error(
                ID,
                NAME,
                "coderabbit was not found on PATH. Install the CLI or set CODERABBIT_CLI_PATH.",
            );
        };
        match run_usage(&path) {
            Ok(text) => {
                let lines = parse_usage(&text);
                if lines.is_empty() {
                    ProviderOutput::error(
                        ID,
                        NAME,
                        "coderabbit usage returned no recognized fields.",
                    )
                } else {
                    ProviderOutput::new(ID, NAME, lines)
                }
            }
            Err(err) => ProviderOutput::error(ID, NAME, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_report() {
        let lines = parse_usage(
            "CodeRabbit Usage — current billing period\n\
             Organization  : Example Org\n\
             Usage billing : inactive\n\
             User          : example-user\n\
             Your reviews  : 25\n\
             Period resets : 2026-09-30\n",
        );
        assert_eq!(lines.len(), 5);
    }
}
