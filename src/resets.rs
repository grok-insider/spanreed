use crate::http::Request;
use crate::model::{MetricKind, MetricLine};
use fabrials_core::{Availability, Freshness, Observation, ResetInventory};

fn observation(
    source: &str,
    result: Result<ResetInventory, String>,
) -> Observation<ResetInventory> {
    match result {
        Ok(value) => Observation {
            availability: Availability::Available,
            freshness: Freshness::Fresh,
            observed_at_ms: Some(crate::util::now_ms()),
            source: source.into(),
            value: Some(value),
            error: None,
        },
        Err(error) => Observation {
            availability: Availability::Unavailable,
            freshness: Freshness::Fresh,
            observed_at_ms: None,
            source: source.into(),
            value: None,
            error: Some(error),
        },
    }
}

pub fn codex(
    token: &str,
    account: Option<&str>,
    usage: &serde_json::Value,
) -> Observation<ResetInventory> {
    let mut request = Request::get(fabrials_providers::codex::RESET_URL)
        .bearer(token)
        .header("Accept", "application/json")
        .header("OpenAI-Beta", "codex-1")
        .header("originator", "Codex Desktop");
    if let Some(account) = account {
        request = request.header("ChatGPT-Account-Id", account);
    }
    let result = request
        .send_limited(2 * 1024 * 1024)
        .map_err(|_| "Reset inventory request failed".to_string())
        .and_then(|response| {
            if !(200..300).contains(&response.status) {
                return Err(format!("Reset inventory HTTP {}", response.status));
            }
            let value = response.json().ok_or("Invalid reset inventory")?;
            fabrials_providers::codex::parse_resets(&value, crate::util::now_ms())
                .map_err(str::to_string)
        });
    let mut observed = observation("codex-reset-inventory", result);
    if observed.value.is_none() {
        if let Some(available) = fabrials_providers::codex::reset_count(usage) {
            observed.availability = Availability::Available;
            observed.source = "codex-usage-count".into();
            observed.observed_at_ms = Some(crate::util::now_ms());
            observed.value = Some(ResetInventory {
                available,
                credits: vec![],
                details_complete: available == 0,
            });
        }
    }
    observed
}

pub fn grok(token: &str) -> Observation<ResetInventory> {
    let result = Request::post(fabrials_providers::grok::RESET_URL)
        .bearer(token)
        .header("Content-Type", "application/grpc-web+proto")
        .header("x-grpc-web", "1")
        .header("Origin", "https://grok.com")
        .body("\0\0\0\0\0")
        .send_bytes_limited(2 * 1024 * 1024)
        .map_err(|_| "Reset inventory request failed".to_string())
        .and_then(|response| {
            if !(200..300).contains(&response.status) {
                return Err(format!("Reset inventory HTTP {}", response.status));
            }
            for status in response.headers.get_all("grpc-status") {
                fabrials_providers::grok::validate_reset_status(
                    status.to_str().map_err(|_| "Invalid reset status")?,
                )?;
            }
            fabrials_providers::grok::parse_resets(&response.body, crate::util::now_ms())
                .map_err(str::to_string)
        });
    observation("supergrok-reset-inventory", result)
}

pub fn append_lines(lines: &mut Vec<MetricLine>, observed: &Observation<ResetInventory>) {
    let Some(inventory) = &observed.value else {
        lines.push(MetricLine::text(
            MetricKind::Plan,
            "Limit reset credits",
            "Unavailable",
        ));
        return;
    };
    lines.push(MetricLine::text(
        MetricKind::Plan,
        "Limit reset credits",
        format!("{} available", inventory.available),
    ));
    for (index, credit) in inventory.credits.iter().enumerate() {
        let expiry = credit
            .expires_at_ms
            .and_then(crate::util::ms_to_iso)
            .unwrap_or_else(|| "Expiry unavailable".into());
        lines.push(MetricLine::text(
            MetricKind::Plan,
            format!("Reset {} expires", index + 1),
            expiry,
        ));
    }
}
