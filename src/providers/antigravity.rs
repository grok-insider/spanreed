//! Antigravity (Google, "Jetski") provider.
//!
//! Antigravity runs a local Codeium-derived language server. We discover it
//! via the process list (process name `language_server*`, marker
//! `antigravity`), extract the `--csrf_token` and listening ports, then probe
//! each port's Connect-RPC `GetUserStatus` endpoint (self-signed HTTPS) for
//! per-model `remainingFraction` quota. Local-process discovery is Linux/macOS
//! only; on other platforms the provider simply reports "not detected".

use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::proc;
use crate::providers::Provider;
use crate::util;

const ID: &str = "antigravity";
const NAME: &str = "Antigravity";
const SERVICE: &str = "exa.language_server_pb.LanguageServerService";

pub struct Antigravity;

struct Discovered {
    csrf: String,
    ports: Vec<u16>,
}

fn discover() -> Option<Discovered> {
    // language_server process carrying an antigravity marker.
    let procs = proc::find_processes(&["language_server", "antigravity"]);
    for p in procs {
        let csrf = proc::extract_flag(&p.cmdline, "--csrf_token");
        let mut ports = proc::listening_ports(p.pid);
        // Prefer the explicit extension server port if advertised.
        if let Some(port) = proc::extract_flag(&p.cmdline, "--extension_server_port")
            .and_then(|v| v.parse::<u16>().ok())
        {
            if !ports.contains(&port) {
                ports.insert(0, port);
            }
        }
        if let Some(csrf) = csrf {
            if !ports.is_empty() {
                return Some(Discovered { csrf, ports });
            }
        }
    }
    None
}

fn rpc_user_status(port: u16, csrf: &str) -> Option<serde_json::Value> {
    let url = format!("https://127.0.0.1:{port}/{SERVICE}/GetUserStatus");
    let resp = Request::post(url)
        .insecure()
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .header("x-codeium-csrf-token", csrf)
        .body("{}")
        .send()
        .ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    resp.json()
}

/// Collect per-model quota lines from the clientModelConfigs array.
fn parse_models(data: &serde_json::Value) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    let configs = data
        .get("cascadeModelConfigData")
        .and_then(|c| c.get("clientModelConfigs"))
        .and_then(|c| c.as_array());
    let configs = match configs {
        Some(c) => c,
        None => return lines,
    };
    for model in configs {
        let label = model.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let quota = model.get("quotaInfo");
        let fraction = quota
            .and_then(|q| q.get("remainingFraction"))
            .and_then(|v| v.as_f64());
        if let (Some(fraction), false) = (fraction, label.is_empty()) {
            let used_pct = ((1.0 - fraction) * 100.0).clamp(0.0, 100.0);
            let resets = quota
                .and_then(|q| q.get("resetTime"))
                .and_then(util::to_iso);
            lines.push(MetricLine::percent(label, used_pct, resets));
        }
    }
    lines
}

impl Provider for Antigravity {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        !proc::find_processes(&["language_server", "antigravity"]).is_empty()
    }

    fn probe(&self) -> ProviderOutput {
        let disc = match discover() {
            Some(d) => d,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Antigravity not running. Open the Antigravity app/IDE.",
                )
            }
        };

        // Probe each candidate port until one answers.
        let mut data = None;
        for port in &disc.ports {
            if let Some(d) = rpc_user_status(*port, &disc.csrf) {
                data = Some(d);
                break;
            }
        }
        let data = match data {
            Some(d) => d,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Could not reach Antigravity language server.",
                )
            }
        };

        let lines = parse_models(&data);
        if lines.is_empty() {
            return ProviderOutput::error(ID, NAME, "No model quota returned.");
        }

        let plan = data
            .get("planStatus")
            .and_then(|p| p.get("planInfo"))
            .and_then(|pi| pi.get("planName"))
            .and_then(|v| v.as_str())
            .map(util::plan_label);

        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_per_model_fraction_quota() {
        let data = serde_json::json!({
            "cascadeModelConfigData": {
                "clientModelConfigs": [
                    { "label": "Gemini 3 Pro", "quotaInfo": { "remainingFraction": 0.25, "resetTime": "2026-02-07T14:23:01Z" } },
                    { "label": "Claude Sonnet 4.5", "quotaInfo": { "remainingFraction": 1.0 } }
                ]
            }
        });
        let lines = parse_models(&data);
        // remaining 0.25 -> 75% used
        let gemini = lines.iter().find_map(|l| match l {
            MetricLine::Progress { label, used, .. } if label == "Gemini 3 Pro" => Some(*used),
            _ => None,
        });
        assert_eq!(gemini, Some(75.0));
        let claude = lines.iter().find_map(|l| match l {
            MetricLine::Progress { label, used, .. } if label == "Claude Sonnet 4.5" => Some(*used),
            _ => None,
        });
        assert_eq!(claude, Some(0.0));
    }
}
