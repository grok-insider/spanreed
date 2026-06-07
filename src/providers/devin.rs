//! Devin (Cognition) provider.
//!
//! Auth: local Devin CLI credentials `~/.local/share/devin/credentials.toml`
//!       (`windsurf_api_key = "devin-session-token$..."`).
//! Usage: Connect-RPC `POST <server>/exa.seat_management_pb.SeatManagementService/GetUserStatus`.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "devin";
const NAME: &str = "Devin";
const DEFAULT_SERVER: &str = "https://server.codeium.com";
const SERVICE: &str = "exa.seat_management_pb.SeatManagementService";

pub struct Devin;

fn creds_path() -> std::path::PathBuf {
    creds::data_home().join("devin").join("credentials.toml")
}

/// Read a `key = "value"` string from a minimal TOML document.
fn toml_string(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let val = after_eq.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn clean_server(url: Option<String>) -> String {
    url.map(|u| u.trim_end_matches('/').to_string())
        .filter(|u| u.starts_with("http"))
        .unwrap_or_else(|| DEFAULT_SERVER.to_string())
}

impl Provider for Devin {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        creds_path().exists()
    }

    fn probe(&self) -> ProviderOutput {
        let text = match creds::read_file(&creds_path()) {
            Some(t) => t,
            None => {
                return ProviderOutput::error(ID, NAME, "Not signed in. Run `devin auth login`.")
            }
        };
        let api_key = match toml_string(&text, "windsurf_api_key") {
            Some(k) => k,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Devin credentials missing windsurf_api_key.",
                )
            }
        };
        let server = clean_server(toml_string(&text, "api_server_url"));

        let body = serde_json::json!({
            "metadata": {
                "apiKey": api_key,
                "ideName": "devin",
                "ideVersion": "1.108.2",
                "extensionName": "devin",
                "extensionVersion": "1.108.2",
                "locale": "en"
            }
        });
        let resp = match Request::post(format!("{server}/{SERVICE}/GetUserStatus"))
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .body(body.to_string())
            .send()
        {
            Ok(r) => r,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };
        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, "Token rejected. Run `devin auth login`.");
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(
                ID,
                NAME,
                format!("status request failed (HTTP {})", resp.status),
            );
        }
        let data = match resp.json() {
            Some(d) => d,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Devin quota data unavailable. Try again later.",
                )
            }
        };

        let plan_status = match data.get("userStatus").and_then(|u| u.get("planStatus")) {
            Some(p) => p,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "Devin quota data unavailable. Try again later.",
                )
            }
        };

        let lines = parse_status(plan_status);
        if lines.is_empty() {
            return ProviderOutput::error(
                ID,
                NAME,
                "Devin quota data unavailable. Try again later.",
            );
        }

        let plan = plan_status
            .get("planInfo")
            .and_then(|pi| pi.get("planName"))
            .and_then(|v| v.as_str())
            .map(util::plan_label);

        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}

/// Parse `planStatus` into a weekly quota line + optional overage balance.
fn parse_status(plan_status: &serde_json::Value) -> Vec<MetricLine> {
    let mut lines = Vec::new();

    if let Some(weekly_remaining) = plan_status
        .get("weeklyQuotaRemainingPercent")
        .and_then(|v| v.as_f64())
    {
        let used = (100.0 - weekly_remaining).clamp(0.0, 100.0);
        let resets = plan_status
            .get("weeklyQuotaResetAtUnix")
            .and_then(util::to_iso);
        lines.push(MetricLine::percent("Weekly", used, resets));
    }

    if let Some(micros) = plan_status
        .get("overageBalanceMicros")
        .and_then(|v| v.as_f64())
    {
        if micros > 0.0 {
            lines.push(MetricLine::text(
                "Extra usage",
                format!("${:.2}", micros / 1_000_000.0),
            ));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_string_reads_key() {
        let toml = "windsurf_api_key = \"devin-session-token$abc\"\napi_server_url = \"https://server.codeium.com\"\n";
        assert_eq!(
            toml_string(toml, "windsurf_api_key").as_deref(),
            Some("devin-session-token$abc")
        );
        assert_eq!(
            toml_string(toml, "api_server_url").as_deref(),
            Some("https://server.codeium.com")
        );
    }

    #[test]
    fn parse_status_weekly_and_overage() {
        let ps = serde_json::json!({
            "weeklyQuotaRemainingPercent": 65.0,
            "weeklyQuotaResetAtUnix": 1738900000_i64,
            "overageBalanceMicros": 2_500_000.0
        });
        let lines = parse_status(&ps);
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Progress { label, used, .. } if label == "Weekly" && *used == 35.0)));
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Text { label, value, .. } if label == "Extra usage" && value == "$2.50")));
    }
}
