//! JetBrains AI Assistant provider.
//!
//! Reads the local IDE quota cache `AIAssistantQuotaManager2.xml` from
//! `~/.config/JetBrains/<IDE>/options/` (Linux path). Picks the IDE directory
//! with the latest quota window.

use crate::creds;
use crate::model::{MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "jetbrains-ai-assistant";
const NAME: &str = "JetBrains AI";
const QUOTA_FILE: &str = "AIAssistantQuotaManager2.xml";
const CREDIT_SCALE: f64 = 100_000.0;

pub struct JetBrains;

fn base_dir() -> std::path::PathBuf {
    creds::config_home().join("JetBrains")
}

/// Find every `AIAssistantQuotaManager2.xml` under the JetBrains config base.
fn quota_files() -> Vec<std::path::PathBuf> {
    let base = base_dir();
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let ide_dir = entry.path();
        if !ide_dir.is_dir() {
            continue;
        }
        // JetBrains stores per-app config under <IDE>/options/<file>.
        let candidate = ide_dir.join("options").join(QUOTA_FILE);
        if candidate.exists() {
            out.push(candidate);
        }
    }
    out
}

/// Extract an `<option name="X" value="Y"/>` numeric value from XML text.
fn option_num(xml: &str, name: &str) -> Option<f64> {
    let re = regex_lite::Regex::new(&format!(
        r#"<option\b[^>]*\bname="{}"[^>]*\bvalue="([^"]*)""#,
        regex_lite::escape(name)
    ))
    .ok()?;
    let caps = re.captures(xml)?;
    caps.get(1)?.as_str().trim().parse::<f64>().ok()
}

/// Extract a string option value.
fn option_str(xml: &str, name: &str) -> Option<String> {
    let re = regex_lite::Regex::new(&format!(
        r#"<option\b[^>]*\bname="{}"[^>]*\bvalue="([^"]*)""#,
        regex_lite::escape(name)
    ))
    .ok()?;
    let caps = re.captures(xml)?;
    Some(caps.get(1)?.as_str().to_string())
}

struct Quota {
    used: f64,
    maximum: f64,
    until: Option<String>,
}

fn parse_quota(xml: &str) -> Option<Quota> {
    let maximum = option_num(xml, "maximum")?;
    let current = option_num(xml, "current").unwrap_or(0.0);
    let available = option_num(xml, "available");
    let used = if current > 0.0 {
        current
    } else if let Some(avail) = available {
        (maximum - avail).max(0.0)
    } else {
        0.0
    };
    if maximum <= 0.0 {
        return None;
    }
    // `until` / `next` is an ISO-ish timestamp.
    let until = option_str(xml, "until")
        .or_else(|| option_str(xml, "next"))
        .and_then(|s| util::to_iso(&serde_json::Value::String(s)));
    Some(Quota {
        used,
        maximum,
        until,
    })
}

impl Provider for JetBrains {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        !quota_files().is_empty()
    }

    fn probe(&self) -> ProviderOutput {
        let files = quota_files();
        if files.is_empty() {
            return ProviderOutput::error(
                ID,
                NAME,
                "JetBrains AI Assistant not detected. Open a JetBrains IDE with AI Assistant enabled.",
            );
        }

        // Pick the quota with the largest `maximum` and a parseable block; in
        // practice the latest IDE's file is the meaningful one.
        let mut best: Option<Quota> = None;
        for file in &files {
            if let Some(text) = creds::read_file(file) {
                if let Some(q) = parse_quota(&text) {
                    if best.as_ref().map(|b| q.maximum > b.maximum).unwrap_or(true) {
                        best = Some(q);
                    }
                }
            }
        }

        let quota = match best {
            Some(q) => q,
            None => {
                return ProviderOutput::error(
                    ID,
                    NAME,
                    "JetBrains AI Assistant quota data unavailable. Open AI Assistant once and try again.",
                )
            }
        };

        let used_pct = (quota.used / quota.maximum * 100.0).clamp(0.0, 100.0);
        let used_credits = quota.used / CREDIT_SCALE;
        let max_credits = quota.maximum / CREDIT_SCALE;

        let lines = vec![
            MetricLine::percent("Quota", used_pct, quota.until.clone()),
            MetricLine::text("Used", format!("{used_credits:.1}")),
            MetricLine::text(
                "Remaining",
                format!("{:.1}", (max_credits - used_credits).max(0.0)),
            ),
        ];

        ProviderOutput::new(ID, NAME, lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_option_quota_xml() {
        let xml = r#"<application><component name="AIAssistantQuotaManager2">
            <option name="current" value="2500000" />
            <option name="maximum" value="10000000" />
            <option name="available" value="7500000" />
            <option name="until" value="2026-03-01T00:00:00Z" />
        </component></application>"#;
        let q = parse_quota(xml).unwrap();
        assert_eq!(q.maximum, 10_000_000.0);
        assert_eq!(q.used, 2_500_000.0);
        assert_eq!(q.until.as_deref(), Some("2026-03-01T00:00:00Z"));
    }

    #[test]
    fn no_maximum_is_none() {
        assert!(parse_quota("<x><option name=\"current\" value=\"1\"/></x>").is_none());
    }
}
