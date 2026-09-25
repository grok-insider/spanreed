//! IBM Bob monthly Bobcoin usage.
//!
//! API keys use `Authorization: Apikey`. JWTs use `Bearer`.
//! Regional hosts must stay under `bob.ibm.com`.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "ibmbob";
const NAME: &str = "IBM Bob";

pub struct IbmBob;

fn auth_value(token: &str) -> String {
    if util::jwt_payload(token).is_some() {
        format!("Bearer {token}")
    } else {
        format!("Apikey {token}")
    }
}

fn regional_base(domain: Option<&str>) -> Result<String, String> {
    let Some(domain) = domain.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok("https://api.us-east.bob.ibm.com".into());
    };
    let host = if domain.to_ascii_lowercase().starts_with("api.") {
        domain.to_ascii_lowercase()
    } else {
        format!("api.{}", domain.to_ascii_lowercase())
    };
    if host.contains('/') || host.contains('@') || host.contains(':') {
        return Err("IBM Bob returned an untrusted regional API host.".into());
    }
    if host != "bob.ibm.com" && !host.ends_with(".bob.ibm.com") {
        return Err("IBM Bob returned an untrusted regional API host.".into());
    }
    Ok(format!("https://{host}"))
}

pub(super) fn parse_profile(body: &serde_json::Value) -> Vec<serde_json::Value> {
    body.get("instances")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

impl Provider for IbmBob {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["BOBSHELL_API_KEY"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(token) = env_any(&["BOBSHELL_API_KEY"]) else {
            return ProviderOutput::error(
                ID,
                NAME,
                "No IBM Bob API key found. Set BOBSHELL_API_KEY.",
            );
        };
        let auth = auth_value(&token);
        let profile = match json_api::get_headers(
            "https://api.us-east.bob.ibm.com/admin/v1/profile",
            &[("Authorization", &auth)],
        ) {
            Ok(profile) => profile,
            Err(err) => return ProviderOutput::error(ID, NAME, err),
        };
        let mut lines = Vec::new();
        let mut plan = None;
        for instance in parse_profile(&profile).into_iter().take(4) {
            let Some(user_id) = json_api::text_field(&instance, "user_id")
                .as_deref()
                .and_then(|id| json_api::safe_segment(id, 128))
                .map(str::to_owned)
            else {
                continue;
            };
            if plan.is_none() {
                plan = json_api::text_field(&instance, "plan_name");
            }
            let base = match regional_base(instance.get("region_domain").and_then(|v| v.as_str())) {
                Ok(base) => base,
                Err(err) => return ProviderOutput::error(ID, NAME, err),
            };
            let teams = instance
                .get("teams")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for team in teams.into_iter().take(4) {
                let Some(team_id) = json_api::text_field(&team, "id")
                    .as_deref()
                    .and_then(|id| json_api::safe_segment(id, 128))
                    .map(str::to_owned)
                else {
                    continue;
                };
                let instance_id =
                    json_api::text_field(&instance, "instance_id").unwrap_or_default();
                let url = format!("{base}/admin/v1/teams/{team_id}/users/{user_id}");
                let mut headers = vec![
                    ("Authorization", auth.as_str()),
                    ("x-team-id", team_id.as_str()),
                ];
                if !instance_id.is_empty() {
                    headers.push(("x-instance-id", instance_id.as_str()));
                }
                let budget = match json_api::get_headers(&url, &headers) {
                    Ok(budget) => budget,
                    Err(err) => return ProviderOutput::error(ID, NAME, err),
                };
                let used = field(&budget, "usage").unwrap_or(0.0).max(0.0);
                let limit = field(&budget, "budget_limit").filter(|value| *value >= 0.0);
                let label = json_api::text_field(&team, "name").unwrap_or_else(|| team_id.clone());
                let resets = instance.get("refresh_at").and_then(util::to_iso);
                if let Some(limit) = limit {
                    if let Some(pct) = json_api::used_percent(used, limit) {
                        lines.push(MetricLine::percent(label, pct, resets));
                        continue;
                    }
                }
                lines.push(json_api::text_line(&label, format!("{used:.0} Bobcoins")));
            }
        }
        if lines.is_empty() {
            ProviderOutput::error(
                ID,
                NAME,
                "IBM Bob returned no subscription instances or teams for this API key.",
            )
        } else {
            ProviderOutput::new(ID, NAME, lines).with_plan(plan)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_untrusted_regions_and_reads_instances() {
        assert!(regional_base(Some("evil.example")).is_err());
        assert!(regional_base(Some("us-east.bob.ibm.com")).is_ok());
        let instances = parse_profile(&serde_json::json!({
            "instances": [{"instance_id": "i", "user_id": "u", "teams": []}]
        }));
        assert_eq!(instances.len(), 1);
    }
}
