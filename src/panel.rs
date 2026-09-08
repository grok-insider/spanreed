//! Cached local-only presentation for Eww. Opening a panel never probes providers.
use serde_json::{json, Value};

pub fn snapshot() -> Value {
    let outputs = crate::http::Request::get("http://127.0.0.1:6736/usage")
        .send_limited(4 * 1024 * 1024)
        .ok()
        .filter(|response| response.status == 200)
        .and_then(|response| response.json());
    match outputs.filter(Value::is_array) {
        Some(outputs) => present(&outputs, crate::util::now_ms()),
        None => json!({"status":"Local Spanreed service unavailable", "providers":[]}),
    }
}
fn remaining(value: &Value, now: i64) -> String {
    let expiry = value.as_i64().or_else(|| {
        value
            .as_str()
            .and_then(crate::util::parse_iso_dt)
            .map(|date| (date.unix_timestamp_nanos() / 1_000_000) as i64)
    });
    let Some(expiry) = expiry else {
        return "Date unavailable".into();
    };
    if expiry <= now {
        return "Expired".into();
    }
    let minutes = expiry.saturating_sub(now) / 60_000;
    if minutes >= 1440 {
        format!("{}d {}h", minutes / 1440, (minutes / 60) % 24)
    } else {
        format!("{}h {}m", minutes / 60, minutes % 60)
    }
}
fn row(label: &str, value: String) -> Value {
    json!({"label":label,"value":value,"bar":false,"percent":0,"detail":"","state":"normal"})
}
fn present(outputs: &Value, now: i64) -> Value {
    let mut providers = Vec::new();
    for provider in outputs.as_array().into_iter().flatten() {
        let mut rows = Vec::new();
        for metric in provider["lines"].as_array().into_iter().flatten() {
            let kind = metric["kind"].as_str().unwrap_or("");
            let label = metric["label"].as_str().unwrap_or("");
            if matches!(kind, "cost" | "models" | "cache" | "trend")
                || label.eq_ignore_ascii_case("plan")
            {
                continue;
            }
            if provider["resetInventory"].is_object()
                && (label.to_lowercase().starts_with("reset ")
                    || label.eq_ignore_ascii_case("Limit reset credits"))
            {
                continue;
            }
            let mut output = row(
                label,
                metric["value"]
                    .as_str()
                    .or_else(|| metric["text"].as_str())
                    .unwrap_or("")
                    .into(),
            );
            match metric["type"].as_str() {
                Some("progress") => {
                    let used = metric["used"].as_f64().filter(|value| value.is_finite());
                    let limit = metric["limit"]
                        .as_f64()
                        .filter(|value| value.is_finite() && *value > 0.0);
                    let format = metric["format"]["kind"].as_str().unwrap_or("");
                    if let Some(used) = used {
                        let percent = if format == "percent" {
                            Some(used)
                        } else {
                            limit.map(|limit| used / limit * 100.0)
                        };
                        output["value"] = json!(match format {
                            "percent" => format!("{used:.0}%"),
                            "dollars" => limit
                                .map(|limit| format!("${used:.2} / ${limit:.2}"))
                                .unwrap_or_else(|| format!("${used:.2}")),
                            _ => limit
                                .map(|limit| format!(
                                    "{used} / {limit} {}",
                                    metric["format"]["suffix"].as_str().unwrap_or("")
                                ))
                                .unwrap_or_else(|| used.to_string()),
                        });
                        output["bar"] = json!(percent.is_some());
                        output["percent"] = json!(percent.unwrap_or(0.0).clamp(0.0, 100.0));
                        output["state"] = json!(if percent.is_some_and(|value| value >= 95.0) {
                            "critical"
                        } else if percent.is_some_and(|value| value >= 80.0) {
                            "warning"
                        } else {
                            "normal"
                        });
                    } else {
                        output["value"] = json!("Unavailable");
                    }
                    if metric["resetsAt"].is_string() {
                        output["detail"] =
                            json!(format!("Resets in {}", remaining(&metric["resetsAt"], now)));
                    }
                }
                Some("text" | "badge") => {}
                _ => continue,
            }
            if kind == "error" {
                output["state"] = json!("error");
            }
            rows.push(output);
        }
        let observation = &provider["resetInventory"];
        if observation.is_object() && observation["availability"] != "unsupported" {
            let value = &observation["value"];
            let current =
                observation["availability"] == "available" && observation["freshness"] == "fresh";
            let label = value["available"]
                .as_u64()
                .map(|count| {
                    format!(
                        "{count} {}",
                        if current {
                            "available"
                        } else {
                            "last reported"
                        }
                    )
                })
                .unwrap_or_else(|| "Unavailable".into());
            let mut output = row("Limit reset credits", label);
            let mut details = Vec::new();
            if !current {
                details.push("Refresh to check current availability".into());
                output["state"] = json!("warning");
            }
            for credit in value["credits"].as_array().into_iter().flatten() {
                let mut detail = format!("Expires · {}", remaining(&credit["expiresAtMs"], now));
                if credit["validFromMs"]
                    .as_i64()
                    .is_some_and(|start| start > now)
                {
                    detail = format!(
                        "Available in {} · {detail}",
                        remaining(&credit["validFromMs"], now)
                    );
                }
                details.push(detail);
            }
            if value.is_object() && value["detailsComplete"] != true {
                details.push("Some expiry dates are unavailable".into());
            }
            output["detail"] = json!(details.join("\n"));
            rows.push(output);
        }
        providers.push(json!({"name":provider["displayName"].as_str().unwrap_or("Provider"), "plan":provider["plan"].as_str().unwrap_or("Plan not reported"), "rows":rows}));
    }
    json!({"status":if providers.is_empty() { "No locally detected providers" } else { "Local workspace" }, "providers":providers})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counts_and_money_use_their_limits_and_unknown_limits_have_no_bar() {
        let result = present(
            &json!([{"displayName":"Fixture", "lines":[
                {"type":"progress","label":"Requests","used":20,"limit":25,"format":{"kind":"count","suffix":"requests"}},
                {"type":"progress","label":"Cost","used":2.5,"limit":0,"format":{"kind":"dollars"}}
            ]}]),
            0,
        );
        assert_eq!(result["providers"][0]["rows"][0]["percent"], 80.0);
        assert_eq!(result["providers"][0]["rows"][1]["bar"], false);
        assert_eq!(result["providers"][0]["rows"][1]["value"], "$2.50");
    }
    #[test]
    fn stale_reset_inventory_is_not_reported_as_current() {
        let result = present(
            &json!([{"resetInventory":{"availability":"available","freshness":"stale","value":{"available":3,"credits":[],"detailsComplete":false}}}]),
            0,
        );
        assert_eq!(
            result["providers"][0]["rows"][0]["value"],
            "3 last reported"
        );
        assert_eq!(remaining(&json!(0), 1), "Expired");
        assert_eq!(remaining(&Value::Null, 1), "Date unavailable");
    }
}
