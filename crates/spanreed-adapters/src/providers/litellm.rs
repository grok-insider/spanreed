//! LiteLLM virtual-key spend and budget.
//!
//! Requires `LITELLM_BASE_URL` and `LITELLM_API_KEY`. A master key is not used.

use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::providers::json_api::{self, env_any, field};
use crate::util;

const ID: &str = "litellm";
const NAME: &str = "LiteLLM";

pub struct LiteLLM;

fn budget_line(label: &str, node: &serde_json::Value) -> Option<MetricLine> {
    let used = field(node, "spend")?;
    let limit = field(node, "max_budget").unwrap_or(0.0);
    let resets = node.get("budget_reset_at").and_then(util::to_iso);
    if limit > 0.0 {
        Some(MetricLine::dollars(
            MetricKind::Quota,
            label,
            used,
            limit,
            resets,
        ))
    } else if used > 0.0 {
        Some(json_api::text_line(label, json_api::usd(used)))
    } else {
        None
    }
}

pub(super) fn parse_info(body: &serde_json::Value) -> Vec<MetricLine> {
    let user = body.get("user").unwrap_or(body);
    let mut lines = Vec::new();
    if let Some(line) = budget_line("Personal", user) {
        lines.push(line);
    }
    if let Some(teams) = user.get("teams").and_then(|v| v.as_array()) {
        for team in teams.iter().take(3) {
            let info = team.get("team_info").unwrap_or(team);
            if let Some(line) = budget_line("Team", info) {
                lines.push(line);
                break;
            }
        }
    }
    if let Some(info) = body.get("team_info")
        && let Some(line) = budget_line("Team", info)
    {
        lines.push(line);
    }
    lines
}

pub(super) fn parse_spend_report(body: &serde_json::Value) -> Option<f64> {
    let rows = body
        .as_array()
        .or_else(|| body.get("data").and_then(|v| v.as_array()))?;
    let mut total = 0.0;
    let mut saw = false;
    for row in rows {
        if let Some(cost) = field(row, "total_cost") {
            total += cost;
            saw = true;
        }
    }
    saw.then_some(total)
}

impl Provider for LiteLLM {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["LITELLM_API_KEY"]).is_some() && env_any(&["LITELLM_BASE_URL"]).is_some()
    }

    fn probe(&self, _ports: crate::ports::ProbePorts<'_>) -> ProviderOutput {
        let Some(key) = env_any(&["LITELLM_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No LiteLLM API key found. Set LITELLM_API_KEY.",
            );
        };
        let Some(raw) = env_any(&["LITELLM_BASE_URL"]) else {
            return ProviderOutput::error(ID, NAME, "Set LITELLM_BASE_URL for the proxy origin.");
        };
        let base = match json_api::allowed_base(&raw, true) {
            Ok(base) => base
                .trim_end_matches('/')
                .trim_end_matches("/v1")
                .to_string(),
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        match json_api::get_bearer(&json_api::join_url(&base, "key/info"), &key) {
            Ok(data) => {
                let info = data.get("info").cloned().unwrap_or(data);
                let user_id = json_api::text_field(&info, "user_id");
                let team_id = json_api::text_field(&info, "team_id");
                let target = if let Some(user_id) = user_id
                    .as_deref()
                    .and_then(|id| json_api::safe_segment(id, 128))
                {
                    format!(
                        "{}?user_id={}",
                        json_api::join_url(&base, "user/info"),
                        json_api::query_escape(user_id)
                    )
                } else if let Some(team_id) = team_id
                    .as_deref()
                    .and_then(|id| json_api::safe_segment(id, 128))
                {
                    format!(
                        "{}?team_id={}",
                        json_api::join_url(&base, "team/info"),
                        json_api::query_escape(team_id)
                    )
                } else {
                    return ProviderOutput::error(
                        ID,
                        NAME,
                        "LiteLLM key info had no user or team id.",
                    );
                };
                match json_api::get_bearer(&target, &key) {
                    Ok(body) => {
                        let lines = parse_info(&body);
                        if lines.is_empty() {
                            ProviderOutput::error(ID, NAME, "LiteLLM budget response was empty.")
                        } else {
                            ProviderOutput::new(ID, NAME, lines)
                        }
                    }
                    Err(err) => ProviderOutput::error(ID, NAME, err),
                }
            }
            Err(err)
                if err.contains("HTTP 401")
                    || err.contains("HTTP 403")
                    || err.contains("HTTP 404") =>
            {
                let end = json_api::utc_ymd(0);
                let start = format!("{}-01", &end[..7]);
                let url = format!(
                    "{}?start_date={start}&end_date={end}",
                    json_api::join_url(&base, "key/spend/report")
                );
                match json_api::get_bearer(&url, &key) {
                    Ok(data) => match parse_spend_report(&data) {
                        Some(total) => ProviderOutput::new(
                            ID,
                            NAME,
                            vec![json_api::text_line("Key spend only", json_api::usd(total))],
                        ),
                        None => ProviderOutput::error(ID, NAME, "LiteLLM spend report was empty."),
                    },
                    Err(err) => ProviderOutput::error(ID, NAME, err),
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
    fn parses_budget_and_spend_rows() {
        let lines = parse_info(&serde_json::json!({
            "spend": 2.5,
            "max_budget": 10.0,
            "budget_reset_at": "2026-10-01T00:00:00Z"
        }));
        assert_eq!(lines.len(), 1);
        assert_eq!(
            parse_spend_report(&serde_json::json!([{"total_cost": 1.0}, {"total_cost": 0.5}])),
            Some(1.5)
        );
    }
}
