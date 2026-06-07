//! Amp (ampcode.com) provider.
//!
//! Auth: API key from `~/.local/share/amp/secrets.json`
//!       (`"apiKey@https://ampcode.com/": "sgamp_user_..."`).
//! Usage: JSON-RPC `POST https://ampcode.com/api/internal`
//!        `{"method":"userDisplayBalanceInfo","params":{}}` -> parse displayText.

use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;

const ID: &str = "amp";
const NAME: &str = "Amp";
const API_URL: &str = "https://ampcode.com/api/internal";

pub struct Amp;

fn secrets_path() -> std::path::PathBuf {
    creds::data_home().join("amp").join("secrets.json")
}

fn api_key() -> Option<String> {
    let v = creds::read_json(&secrets_path())?;
    // The key is stored under "apiKey@<host>"; scan for any apiKey@ field.
    let obj = v.as_object()?;
    for (k, val) in obj {
        if k.starts_with("apiKey@") {
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

fn money(re: &regex_lite::Regex, text: &str, group: usize) -> Option<f64> {
    let caps = re.captures(text)?;
    let raw = caps.get(group)?.as_str().replace(',', "");
    raw.parse::<f64>().ok()
}

impl Provider for Amp {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        api_key().is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let key = match api_key() {
            Some(k) => k,
            None => {
                return ProviderOutput::error(ID, NAME, "Not signed in. Run the Amp CLI to sign in.")
            }
        };

        let resp = match Request::post(API_URL)
            .bearer(&key)
            .header("Content-Type", "application/json")
            .body(r#"{"method":"userDisplayBalanceInfo","params":{}}"#)
            .send()
        {
            Ok(r) => r,
            Err(e) => return ProviderOutput::error(ID, NAME, e),
        };
        if resp.is_auth_error() {
            return ProviderOutput::error(ID, NAME, "API key rejected. Re-run Amp CLI sign in.");
        }
        if !(200..300).contains(&resp.status) {
            return ProviderOutput::error(ID, NAME, format!("request failed (HTTP {})", resp.status));
        }
        let json = match resp.json() {
            Some(j) => j,
            None => return ProviderOutput::error(ID, NAME, "invalid response"),
        };
        let text = json
            .get("result")
            .and_then(|r| r.get("displayText"))
            .and_then(|v| v.as_str());
        let text = match text {
            Some(t) => t,
            None => return ProviderOutput::error(ID, NAME, "no balance info returned"),
        };

        let mut lines = Vec::new();
        let mut plan: Option<String> = None;

        // Amp Free: "$<remaining>/$<total> remaining"
        let balance_re =
            regex_lite::Regex::new(r"\$([0-9][0-9,]*(?:\.[0-9]+)?)/\$([0-9][0-9,]*(?:\.[0-9]+)?) remaining").unwrap();
        let remaining = money(&balance_re, text, 1);
        let total = money(&balance_re, text, 2);
        if let (Some(remaining), Some(total)) = (remaining, total) {
            if total > 0.0 {
                let used = (total - remaining).max(0.0);
                lines.push(MetricLine::dollars("Amp Free", used, total, None));
                plan = Some("Free".into());
            }
        }

        // Individual credits: "Individual credits: $<credits> remaining"
        let credits_re =
            regex_lite::Regex::new(r"Individual credits: \$([0-9][0-9,]*(?:\.[0-9]+)?) remaining").unwrap();
        if let Some(credits) = money(&credits_re, text, 1) {
            lines.push(MetricLine::Text {
                label: "Credits".into(),
                value: format!("${credits:.2}"),
                color: None,
                subtitle: None,
            });
            if plan.is_none() {
                plan = Some("Credits".into());
            }
        }

        if lines.is_empty() {
            return ProviderOutput::error(ID, NAME, "no usable balance in response");
        }
        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}
