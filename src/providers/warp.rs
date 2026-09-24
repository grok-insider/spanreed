//! Warp monthly request credits.
//!
//! GraphQL `GetRequestLimitInfo` on `app.warp.dev`. The edge limiter requires
//! a `Warp/1.0` user agent; the OS in the query is this machine's OS.

use crate::model::{MetricLine, ProviderOutput};
use crate::providers::json_api::{self, env_any, field};
use crate::providers::Provider;
use crate::util;

const ID: &str = "warp";
const NAME: &str = "Warp";

pub struct Warp;

fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    }
}

pub(super) fn parse_limit(body: &serde_json::Value) -> Vec<MetricLine> {
    let info = body
        .pointer("/data/user/user/requestLimitInfo")
        .or_else(|| body.pointer("/data/requestLimitInfo"))
        .unwrap_or(body);
    if info.get("isUnlimited").and_then(|v| v.as_bool()) == Some(true) {
        return vec![json_api::text_line("Credits", "Unlimited")];
    }
    let used = field(info, "requestsUsedSinceLastRefresh");
    let limit = field(info, "requestLimit");
    let mut lines = Vec::new();
    if let (Some(used), Some(limit)) = (used, limit) {
        if let Some(pct) = json_api::used_percent(used, limit) {
            let resets = info.get("nextRefreshTime").and_then(util::to_iso);
            lines.push(MetricLine::percent("Credits", pct, resets));
        }
    }
    let grants = info
        .pointer("/bonusGrants")
        .and_then(|v| v.as_array())
        .or_else(|| {
            body.pointer("/data/user/user/bonusGrants")
                .and_then(|v| v.as_array())
        });
    if let Some(grants) = grants {
        let total: f64 = grants
            .iter()
            .filter_map(|g| field(g, "requestCreditsGranted"))
            .sum();
        let remaining: f64 = grants
            .iter()
            .filter_map(|g| field(g, "requestCreditsRemaining"))
            .sum();
        if let Some(pct) = json_api::used_percent((total - remaining).max(0.0), total) {
            lines.push(MetricLine::percent("Add-on credits", pct, None));
        }
    }
    lines
}

impl Provider for Warp {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        env_any(&["WARP_API_KEY", "WARP_TOKEN"]).is_some()
    }

    fn probe(&self) -> ProviderOutput {
        let Some(key) = env_any(&["WARP_API_KEY", "WARP_TOKEN"]) else {
            return ProviderOutput::error(ID, NAME, "No Warp API key found. Set WARP_API_KEY.");
        };
        let os = os_name();
        let body = serde_json::json!({
            "operationName": "GetRequestLimitInfo",
            "query": "query GetRequestLimitInfo($requestContext: RequestContext!) { user(requestContext: $requestContext) { __typename ... on UserOutput { user { requestLimitInfo { isUnlimited nextRefreshTime requestLimit requestsUsedSinceLastRefresh } bonusGrants { requestCreditsGranted requestCreditsRemaining expiration } } } } }",
            "variables": {
                "requestContext": {
                    "clientContext": {},
                    "osContext": {"category": os, "name": os, "version": "spanreed"}
                }
            }
        });
        let body = match serde_json::to_string(&body) {
            Ok(body) => body,
            Err(_) => return ProviderOutput::error(ID, NAME, "Could not build the Warp query."),
        };
        match json_api::post_headers(
            "https://app.warp.dev/graphql/v2?op=GetRequestLimitInfo",
            &[
                ("Authorization", &format!("Bearer {key}")),
                ("User-Agent", "Warp/1.0"),
                ("x-warp-client-id", "warp-app"),
            ],
            &body,
        ) {
            Ok(data) => {
                if data
                    .get("errors")
                    .and_then(|v| v.as_array())
                    .is_some_and(|errors| !errors.is_empty())
                {
                    return ProviderOutput::error(ID, NAME, "Warp returned a GraphQL error.");
                }
                let lines = parse_limit(&data);
                if lines.is_empty() {
                    ProviderOutput::error(ID, NAME, "Warp response had no request limit.")
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
    fn parses_credits_and_addons() {
        let lines = parse_limit(&serde_json::json!({
            "data": {
                "user": {
                    "user": {
                        "requestLimitInfo": {
                            "isUnlimited": false,
                            "requestLimit": 1000,
                            "requestsUsedSinceLastRefresh": 250,
                            "nextRefreshTime": "2026-10-01T00:00:00Z"
                        },
                        "bonusGrants": [
                            {"requestCreditsGranted": 100, "requestCreditsRemaining": 40}
                        ]
                    }
                }
            }
        }));
        assert_eq!(lines.len(), 2);
    }
}
