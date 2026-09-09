//! Read-only usage protocols. The host owns HTTP, credentials and persistence.
//! API shapes researched in Tokscale 0d621ca (MIT; see formats/TOKSCALE-LICENSE).
use fabrials_core::usage::{CostOrigin, Granularity, UsageCost, UsageOrigin, UsageRecord};
use serde_json::{json, Value};
use std::path::Path;

pub struct Request {
    pub endpoint: &'static str,
    pub auth_header: &'static str,
    pub auth_prefix: &'static str,
    pub origin: Option<&'static str>,
    pub body: Value,
}

pub fn collect(
    client: &str,
    account: &str,
    now_ms: i64,
    mut send: impl FnMut(Request) -> Result<Value, String>,
) -> Result<Vec<UsageRecord>, String> {
    let mut records = match client {
        "cursor" | "trae" => {
            let mut rows = Vec::new();
            let mut complete = false;
            let mut total = None;
            for page in 1..=100 {
                let request = if client == "cursor" {
                    Request {
                        endpoint: "https://cursor.com/api/dashboard/get-filtered-usage-events",
                        auth_header: "Cookie",
                        auth_prefix: "WorkosCursorSessionToken=",
                        origin: Some("https://cursor.com"),
                        body: json!({"teamId":0,"page":page,"pageSize":500}),
                    }
                } else {
                    Request {endpoint:"https://api-sg-central.trae.ai/trae/api/v1/pay/query_user_usage_group_by_session",auth_header:"Authorization",auth_prefix:"Cloud-IDE-JWT ",origin:None,body:json!({"start_time":now_ms/1000-31*86400,"end_time":now_ms/1000,"page_num":page,"page_size":500,"usage_type":[1,2,3,4,5,6,7,8]})}
                };
                let response = send(request)?;
                let (array, count) = if client == "cursor" {
                    ("usageEventsDisplay", "totalUsageEventsCount")
                } else {
                    ("user_usage_group_by_sessions", "total")
                };
                let batch = response
                    .get(array)
                    .and_then(Value::as_array)
                    .ok_or("Usage response is missing its records array")?;
                if total.is_none() {
                    total = response.get(count).and_then(Value::as_u64);
                }
                let length = batch.len();
                rows.extend(batch.iter().cloned());
                if total.is_some_and(|total| rows.len() as u64 >= total)
                    || total.is_none() && length == 0
                {
                    complete = true;
                    break;
                }
                if length == 0 {
                    return Err("Usage response ended before its advertised total".into());
                }
            }
            if !complete {
                return Err("Usage pagination limit reached; previous snapshot retained".into());
            }
            let path = Path::new(account);
            let messages = if client == "cursor" {
                super::formats::sessions::cursor::parse_cursor_value(
                    &json!({"usageEventsDisplay":rows}),
                    path,
                )
            } else {
                rows.iter()
                    .filter_map(|row| super::formats::sessions::trae::parse_session("trae", row))
                    .collect()
            };
            super::files::normalize(client, path, messages)?
        }
        "warp" => {
            let response = send(Request {
                endpoint: "https://app.warp.dev/graphql/v2",
                auth_header: "Authorization",
                auth_prefix: "Bearer ",
                origin: None,
                body: json!({"operationName":"GetRequestLimitInfo","variables":{},"query":"query GetRequestLimitInfo { requestLimitInfo { requestLimit requestsUsedSinceLastRefresh nextRefreshTime bonusGrantsInfo { spendingInfo { currentMonthSpendCents currentMonthCreditsPurchased } } } }"}),
            })?;
            if response
                .get("errors")
                .and_then(Value::as_array)
                .is_some_and(|errors| !errors.is_empty())
            {
                return Err("Warp returned GraphQL errors".into());
            }
            let usage = response
                .pointer("/data/requestLimitInfo")
                .filter(|v| v.is_object())
                .ok_or("Warp usage response is missing requestLimitInfo")?;
            let cents = usage
                .pointer("/bonusGrantsInfo/spendingInfo/currentMonthSpendCents")
                .and_then(Value::as_f64)
                .ok_or("Warp did not report spend")?;
            let now = time::OffsetDateTime::from_unix_timestamp(now_ms / 1000)
                .map_err(|_| "Invalid collection date")?;
            let start = now
                .date()
                .replace_day(1)
                .map_err(|_| "Invalid collection month")?
                .midnight()
                .assume_utc()
                .unix_timestamp()
                * 1000;
            let mut record = super::record(
                "warp",
                format!("{account}:{}-{}", now.year(), now.month() as u8),
                start,
                Default::default(),
            );
            record.tokens = None;
            record.session = None;
            record.granularity = Granularity::AccountPeriod;
            record.period_end_ms = Some(now_ms);
            record.cost = Some(UsageCost {
                usd: cents / 100.0,
                origin: CostOrigin::ProviderReported,
                pricing_revision: None,
            });
            vec![record]
        }
        _ => return Err("Unknown remote usage collector".into()),
    };
    for record in &mut records {
        record.account = Some(account.into());
        record.origin = UsageOrigin::RemoteReport;
        record.id = format!("{account}:{}", record.id);
        record.validate()?;
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_clamped_pages_continue_and_missing_arrays_fail() {
        let mut pages = 0;
        let rows=collect("cursor","account",1000,|_|{
            pages+=1;Ok(json!({"totalUsageEventsCount":2,"usageEventsDisplay":[{"timestamp":1000000000000i64+pages,"model":"auto","tokenUsage":{"inputTokens":10,"outputTokens":2,"totalCents":1}}]}))
        }).unwrap();
        assert_eq!(pages, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].account.as_deref(), Some("account"));
        assert!(collect("cursor", "account", 1000, |_| Ok(json!({}))).is_err());
    }
    #[test]
    fn empty_page_before_total_never_replaces_a_good_snapshot() {
        assert!(collect("trae", "account", 1000, |_| Ok(
            json!({"total":2,"user_usage_group_by_sessions":[]})
        ))
        .is_err());
    }
    #[test]
    fn warp_is_a_period_cost_without_invented_tokens() {
        let rows=collect("warp","a",1789000000000,|_|Ok(json!({"data":{"requestLimitInfo":{"bonusGrantsInfo":{"spendingInfo":{"currentMonthSpendCents":250}}}}}))).unwrap();
        assert!(rows[0].tokens.is_none());
        assert_eq!(rows[0].cost.as_ref().unwrap().usd, 2.5);
        assert_eq!(rows[0].granularity, Granularity::AccountPeriod);
    }
}

/// Local Antigravity language-server protocol. Only normalized usage leaves it.
pub fn antigravity(
    mut rpc: impl FnMut(&str, Value) -> Result<Value, String>,
) -> Result<Vec<UsageRecord>, String> {
    let response = rpc("GetAllCascadeTrajectories", json!({}))?;
    let summaries = response
        .get("trajectorySummaries")
        .or_else(|| response.get("cascadeTrajectories"))
        .ok_or("Missing Antigravity trajectory list")?;
    let ids: Vec<String> = if let Some(map) = summaries.as_object() {
        map.keys().cloned().collect()
    } else if let Some(array) = summaries.as_array() {
        array
            .iter()
            .map(|item| {
                ["cascadeId", "trajectoryId", "id", "sessionId"]
                    .iter()
                    .find_map(|key| item.get(*key).and_then(Value::as_str))
                    .map(str::to_string)
                    .ok_or("Invalid Antigravity session identity")
            })
            .collect::<Result<_, _>>()?
    } else {
        return Err("Invalid Antigravity trajectory list".into());
    };
    if ids.len() > 1000 {
        return Err("Antigravity session limit exceeded".into());
    }
    let mut records = std::collections::BTreeMap::new();
    for session in ids {
        let response = rpc(
            "GetCascadeTrajectoryGeneratorMetadata",
            json!({"cascadeId":session}),
        )?;
        let metadata = response
            .get("generatorMetadata")
            .and_then(Value::as_array)
            .ok_or("Missing Antigravity generator metadata")?;
        let mut timestamps = std::collections::BTreeMap::<String, i64>::new();
        let needs_times = metadata.iter().any(|meta| {
            meta.get("chatModel")
                .unwrap_or(meta)
                .get("retryInfos")
                .and_then(Value::as_array)
                .is_some_and(|retries| {
                    retries.iter().any(|retry| {
                        let usage = retry.get("usage").unwrap_or(retry);
                        usage
                            .get("createdAt")
                            .or_else(|| usage.get("timestamp"))
                            .and_then(super::timestamp)
                            .is_none()
                    })
                })
        });
        if needs_times {
            let trajectory = rpc("GetCascadeTrajectory", json!({"cascadeId":session}))?;
            if let Some(steps) = trajectory
                .get("trajectory")
                .unwrap_or(&trajectory)
                .get("steps")
                .and_then(Value::as_array)
            {
                for step in steps {
                    let meta = &step["metadata"];
                    let usage = &meta["modelUsage"];
                    if let Some(at) = [
                        "createdAt",
                        "startedAt",
                        "completedAt",
                        "finishedGeneratingAt",
                        "viewableAt",
                    ]
                    .iter()
                    .find_map(|key| meta.get(*key).and_then(super::timestamp))
                    {
                        for key in ["responseId", "messageId"] {
                            if let Some(id) = usage.get(key).and_then(Value::as_str) {
                                timestamps.insert(id.into(), at);
                            }
                        }
                    }
                }
            }
        }
        for (meta_index, meta) in metadata.iter().enumerate() {
            let chat = meta.get("chatModel").unwrap_or(meta);
            let model = chat
                .get("responseModel")
                .or_else(|| chat.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let Some(retries) = chat.get("retryInfos").and_then(Value::as_array) else {
                continue;
            };
            for (index, retry) in retries.iter().enumerate() {
                let usage = retry.get("usage").unwrap_or(retry);
                let read = |key| {
                    usage
                        .get(key)
                        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
                        .unwrap_or(0u64)
                };
                let input = read("inputTokens");
                let output = read("outputTokens");
                let cached = read("cacheReadTokens");
                let reasoning = read("thinkingOutputTokens");
                if input == 0 && output == 0 && cached == 0 && reasoning == 0 {
                    continue;
                }
                let identity = usage
                    .get("responseId")
                    .or_else(|| usage.get("messageId"))
                    .and_then(Value::as_str);
                let at = usage
                    .get("createdAt")
                    .or_else(|| usage.get("timestamp"))
                    .and_then(super::timestamp)
                    .or_else(|| identity.and_then(|id| timestamps.get(id).copied()))
                    .ok_or("Antigravity usage has no attributable timestamp")?;
                let id = format!(
                    "{session}:{}",
                    identity
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("{meta_index}:{index}"))
                );
                let mut record = super::record(
                    "antigravity",
                    id.clone(),
                    at,
                    fabrials_core::usage::Tokens {
                        input: input.saturating_add(cached),
                        output: output.saturating_add(reasoning),
                        cache_read: cached,
                        cache_write: 0,
                        reasoning,
                    },
                );
                record.model = model.clone();
                record.session = Some(session.clone());
                record.request_id = identity.map(str::to_string);
                record.origin = UsageOrigin::RemoteReport;
                record.validate()?;
                records.insert(id, record);
            }
        }
    }
    Ok(records.into_values().collect())
}

#[cfg(test)]
mod antigravity_tests {
    use super::*;
    #[test]
    fn timestamps_are_enriched_by_response_identity_without_retaining_content() {
        let records=antigravity(|method,_|Ok(match method {
            "GetAllCascadeTrajectories"=>json!({"trajectorySummaries":{"session":{}}}),
            "GetCascadeTrajectoryGeneratorMetadata"=>json!({"generatorMetadata":[{"chatModel":{"model":"gemini-test","retryInfos":[{"usage":{"responseId":"r1","inputTokens":10,"cacheReadTokens":2,"outputTokens":3,"thinkingOutputTokens":4}}]}}]}),
            "GetCascadeTrajectory"=>json!({"trajectory":{"steps":[{"metadata":{"createdAt":"2026-09-01T00:00:00Z","modelUsage":{"responseId":"r1"}},"content":"PRIVATE PROMPT"}]}}),
            _=>panic!("unexpected RPC")
        })).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tokens.as_ref().unwrap().total(), 19);
        assert_eq!(records[0].request_id.as_deref(), Some("r1"));
        assert!(!serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE PROMPT"));
    }
    #[test]
    fn missing_usage_timestamp_does_not_become_current_time() {
        assert!(antigravity(|method, _| Ok(match method {
            "GetAllCascadeTrajectories" => json!({"trajectorySummaries":{"session":{}}}),
            "GetCascadeTrajectoryGeneratorMetadata" =>
                json!({"generatorMetadata":[{"retryInfos":[{"inputTokens":1}]}]}),
            _ => json!({}),
        }))
        .is_err());
    }
}
