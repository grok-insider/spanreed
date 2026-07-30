//! xAI / Grok (Grok CLI / SuperGrok Build) provider.
//!
//! Reads the Grok CLI auth file `~/.grok/auth.json`, which is a map of entries
//! keyed by an account/client identifier. Each entry has `key` (the access
//! token, a JWT), optional `refresh_token`, and `expires_at`. We pick the first
//! usable entry, refresh proactively (or on 401), then query billing with the
//! special `X-XAI-Token-Auth: xai-grok-cli` header.
//!
//! Prefer `GET .../v1/billing?format=credits` for the shared **weekly** SuperGrok
//! pool (`creditUsagePercent` + `currentPeriod`). Some accounts currently get a
//! credits payload with period / PAYG metadata but no usage percent; in that
//! case fall back to bare `GET .../v1/billing` (monthly `used` / `monthlyLimit`).

use crate::creds;
use crate::http::Request;
use crate::model::{MetricKind, MetricLine, ProviderOutput};
use crate::providers::Provider;
use crate::util;

const ID: &str = "grok";
const NAME: &str = "Grok";
const BILLING_CREDITS_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const BILLING_LEGACY_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const SUBS_URL: &str = "https://grok.com/rest/subscriptions";
const REFRESH_URL: &str = "https://auth.x.ai/oauth2/token";
const TOKEN_AUTH_HEADER: &str = "xai-grok-cli";
const DEFAULT_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const REFRESH_BUFFER_MS: i64 = 5 * 60 * 1000;
const LOGIN_HINT: &str = "Grok auth expired. Run `grok login` again.";

pub struct Grok;

fn auth_path() -> std::path::PathBuf {
    creds::expand("~/.grok/auth.json")
}

/// One resolved auth entry plus the bookkeeping needed to refresh/persist it.
struct AuthState {
    /// The whole auth.json document (mutated on refresh).
    doc: serde_json::Value,
    /// The key of the entry in use.
    entry_key: String,
    /// Current access token.
    token: String,
}

fn entry_expires_at_ms(entry: &serde_json::Value) -> Option<i64> {
    let raw = entry.get("expires_at").or_else(|| entry.get("expires"))?;
    util::to_iso(raw).and_then(|iso| {
        time::OffsetDateTime::parse(&iso, &time::format_description::well_known::Rfc3339)
            .ok()
            .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
    })
}

fn needs_refresh(entry: &serde_json::Value, token: &str, now_ms: i64) -> bool {
    let entry_ms = entry_expires_at_ms(entry);
    let token_ms = util::jwt_exp_ms(token);
    let entry_due = entry_ms
        .map(|ms| now_ms + REFRESH_BUFFER_MS >= ms)
        .unwrap_or(false);
    let token_due = token_ms
        .map(|ms| now_ms + REFRESH_BUFFER_MS >= ms)
        .unwrap_or(false);
    entry_due || token_due
}

fn is_expired(entry: &serde_json::Value, token: &str, now_ms: i64) -> bool {
    let ms = util::jwt_exp_ms(token).or_else(|| entry_expires_at_ms(entry));
    match ms {
        Some(ms) => now_ms >= ms,
        None => false,
    }
}

fn read_refresh_token(entry: &serde_json::Value) -> Option<String> {
    for key in ["refresh_token", "refresh"] {
        if let Some(s) = entry.get(key).and_then(|v| v.as_str()) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn read_client_id(entry_key: &str, entry: &serde_json::Value) -> String {
    if let Some(cid) = entry.get("oidc_client_id").and_then(|v| v.as_str()) {
        let cid = cid.trim();
        if !cid.is_empty() {
            return cid.to_string();
        }
    }
    // Some keys encode the client id as the last `::`-separated segment.
    if let Some(last) = entry_key.split("::").last() {
        let last = last.trim();
        if !last.is_empty() && last != entry_key {
            return last.to_string();
        }
    }
    DEFAULT_CLIENT_ID.to_string()
}

/// Attempt a refresh of `entry_key`; on success mutate the doc and return the
/// new token. Returns Err(message) on a hard auth failure.
fn refresh(doc: &mut serde_json::Value, entry_key: &str) -> Result<Option<String>, String> {
    let entry = match doc.get(entry_key) {
        Some(e) => e.clone(),
        None => return Ok(None),
    };
    let refresh_token = match read_refresh_token(&entry) {
        Some(t) => t,
        None => return Ok(None),
    };
    let client_id = read_client_id(entry_key, &entry);

    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}",
        urlencode(&client_id),
        urlencode(&refresh_token)
    );
    let resp = Request::post(REFRESH_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()?;

    if resp.status == 400 || resp.status == 401 || resp.status == 403 {
        return Err(LOGIN_HINT.to_string());
    }
    if !(200..300).contains(&resp.status) {
        return Ok(None); // transient
    }
    let json = match resp.json() {
        Some(j) => j,
        None => return Ok(None),
    };
    let access = match json.get("access_token").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Ok(None),
    };

    // Mutate the entry in the doc.
    if let Some(obj) = doc.get_mut(entry_key).and_then(|v| v.as_object_mut()) {
        obj.insert("key".into(), serde_json::json!(access));
        if let Some(rt) = json.get("refresh_token").and_then(|v| v.as_str()) {
            if !rt.trim().is_empty() {
                obj.insert("refresh_token".into(), serde_json::json!(rt.trim()));
            }
        }
        let now = util::now_ms();
        let expires_at = json
            .get("expires_in")
            .and_then(|v| v.as_f64())
            .filter(|n| *n > 0.0)
            .map(|n| now + (n as i64) * 1000)
            .or_else(|| util::jwt_exp_ms(&access))
            .unwrap_or(now + 3600 * 1000);
        if let Some(iso) = util::ms_to_iso(expires_at) {
            obj.insert("expires_at".into(), serde_json::json!(iso));
        }
    }

    // Best-effort persist.
    if let Ok(text) = serde_json::to_string_pretty(doc) {
        let _ = std::fs::write(auth_path(), text);
    }
    Ok(Some(access))
}

/// Load the first usable auth entry, refreshing if needed.
fn load_auth() -> Result<AuthState, String> {
    let doc = creds::read_json(&auth_path())
        .ok_or_else(|| "Grok not logged in. Run `grok login`.".to_string())?;
    if !doc.is_object() {
        return Err("Grok not logged in. Run `grok login`.".into());
    }

    let now = util::now_ms();
    let keys: Vec<String> = doc.as_object().unwrap().keys().cloned().collect();
    let mut doc = doc;
    let mut expired_candidate = false;

    for entry_key in keys {
        let entry = doc
            .get(&entry_key)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if !entry.is_object() {
            continue;
        }
        let token = entry
            .get("key")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if token.is_empty() {
            continue;
        }

        if needs_refresh(&entry, &token, now) {
            match refresh(&mut doc, &entry_key)? {
                Some(new_token) => {
                    return Ok(AuthState {
                        doc,
                        entry_key,
                        token: new_token,
                    })
                }
                None => {
                    if !is_expired(&entry, &token, now) {
                        return Ok(AuthState {
                            doc,
                            entry_key,
                            token,
                        });
                    }
                    expired_candidate = true;
                    continue;
                }
            }
        }
        return Ok(AuthState {
            doc,
            entry_key,
            token,
        });
    }

    if expired_candidate {
        Err(LOGIN_HINT.into())
    } else {
        Err("Grok auth invalid. Run `grok login` again.".into())
    }
}

fn units(obj: Option<&serde_json::Value>) -> Option<f64> {
    obj?.get("val")?.as_f64()
}

fn fetch_billing(token: &str, url: &str) -> Result<crate::http::Response, String> {
    Request::get(url)
        .bearer(token)
        .header("X-XAI-Token-Auth", TOKEN_AUTH_HEADER)
        .header("Accept", "application/json")
        .header("User-Agent", "spanreed")
        .send()
}

/// GET a billing URL and return the `config` object, refreshing once on auth error.
/// Updates `auth.token` in place when a refresh succeeds.
fn load_billing_config(auth: &mut AuthState, url: &str) -> Result<serde_json::Value, String> {
    let mut resp = match fetch_billing(&auth.token, url) {
        Ok(r) => r,
        Err(_) => {
            return Err("Grok billing request failed. Check your connection.".into());
        }
    };
    if resp.is_auth_error() {
        match refresh(&mut auth.doc, &auth.entry_key) {
            Ok(Some(new_token)) => {
                auth.token = new_token;
                match fetch_billing(&auth.token, url) {
                    Ok(r) => resp = r,
                    Err(_) => {
                        return Err("Grok billing request failed. Check your connection.".into());
                    }
                }
            }
            Ok(None) => return Err(LOGIN_HINT.into()),
            Err(msg) => return Err(msg),
        }
    }

    if resp.is_auth_error() {
        return Err(LOGIN_HINT.into());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "Grok billing request failed (HTTP {}). Try again later.",
            resp.status
        ));
    }

    let data = match resp.json() {
        Some(d) => d,
        None => return Err("Grok billing response changed.".into()),
    };
    match data.get("config") {
        Some(c) if c.is_object() => Ok(c.clone()),
        _ => Err("Grok billing response changed.".into()),
    }
}

fn fetch_plan(token: &str) -> Option<String> {
    let resp = Request::get(SETTINGS_URL)
        .bearer(token)
        .header("X-XAI-Token-Auth", TOKEN_AUTH_HEADER)
        .header("Accept", "application/json")
        .header("User-Agent", "spanreed")
        .send()
        .ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    let data = resp.json()?;
    data.get("subscription_tier_display")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Soft-fail fetch of paid plan billing period (renew / cancel-at-end).
fn fetch_subscriptions(token: &str) -> Option<serde_json::Value> {
    let resp = Request::get(SUBS_URL)
        .bearer(token)
        .header("Accept", "application/json")
        .header("User-Agent", "spanreed")
        .send()
        .ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    resp.json()
}

#[derive(Debug, Clone, PartialEq)]
struct PlanPeriod {
    period_end_iso: String,
    cancel_at_period_end: bool,
    /// Monthly / yearly / unknown — used to derive last renew.
    monthly: bool,
    create_time_iso: Option<String>,
}

/// Pick the best SuperGrok (or other paid) subscription row that has a period end.
fn parse_plan_period(root: &serde_json::Value) -> Option<PlanPeriod> {
    let arr = root
        .get("subscriptions")
        .and_then(|v| v.as_array())
        .or_else(|| root.as_array())?;

    let mut best: Option<(i32, PlanPeriod)> = None;
    for sub in arr {
        let status = sub.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let active = status.ends_with("_ACTIVE") || status.eq_ignore_ascii_case("ACTIVE");
        if !active {
            continue;
        }
        let Some(period_end) = sub
            .get("billingPeriodEnd")
            .or_else(|| sub.get("currentPeriodEnd"))
            .and_then(util::to_iso)
        else {
            continue;
        };
        let tier = sub.get("tier").and_then(|v| v.as_str()).unwrap_or("");
        // Prefer SuperGrok tiers; X Premium often has no period and is lower score.
        let score = if tier.contains("SUPER_GROK") {
            100
        } else if tier.contains("GROK") {
            50
        } else {
            10
        };
        let cancel = sub
            .get("cancelAtPeriodEnd")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let interval = sub
            .get("billingInterval")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let monthly = interval.contains("MONTHLY") || interval.eq_ignore_ascii_case("month");
        let create_time = sub
            .get("createTime")
            .or_else(|| sub.get("createdAt"))
            .and_then(util::to_iso);
        let period = PlanPeriod {
            period_end_iso: period_end,
            cancel_at_period_end: cancel,
            monthly,
            create_time_iso: create_time,
        };
        match &best {
            Some((s, _)) if *s >= score => {}
            _ => best = Some((score, period)),
        }
    }
    best.map(|(_, p)| p)
}

fn plan_period_lines(period: &PlanPeriod) -> Vec<MetricLine> {
    let mut lines = Vec::new();
    let renew_label = if period.cancel_at_period_end {
        "Plan ends"
    } else {
        "Plan renews"
    };
    lines.push(MetricLine::text(
        MetricKind::Plan,
        renew_label,
        util::format_plan_renew_value(&period.period_end_iso, false),
    ));

    // Last renew: for monthly, period_end − 1 calendar month; prefer createTime
    // when it is later (new sub / first cycle).
    let last_iso = if period.monthly {
        util::parse_iso_dt(&period.period_end_iso)
            .map(util::sub_calendar_month)
            .and_then(util::offset_dt_to_iso)
            .map(|derived| {
                if let Some(create) = &period.create_time_iso {
                    if let (Some(c), Some(d)) =
                        (util::parse_iso_dt(create), util::parse_iso_dt(&derived))
                    {
                        if c > d {
                            return create.clone();
                        }
                    }
                }
                derived
            })
    } else {
        period.create_time_iso.clone()
    };
    if let Some(iso) = last_iso {
        lines.push(MetricLine::text(
            MetricKind::Plan,
            "Last renew",
            util::format_plan_last_value(&iso, false),
        ));
    }
    lines
}

impl Provider for Grok {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        auth_path().exists()
    }

    fn probe(&self) -> ProviderOutput {
        let mut auth = match load_auth() {
            Ok(a) => a,
            Err(msg) => return ProviderOutput::error(ID, NAME, msg),
        };

        // Prefer weekly credits pool; fall back to bare monthly allotment when
        // the credits payload omits usage fields (seen on SuperGrok Heavy).
        let (mut lines, weekly_start_ms) = match load_billing_config(&mut auth, BILLING_CREDITS_URL)
        {
            Ok(config) => {
                let start = period_start_ms(&config);
                match parse_credits_billing(&config) {
                    Some(l) => (l, start),
                    None => match load_billing_config(&mut auth, BILLING_LEGACY_URL) {
                        Ok(legacy) => match parse_legacy_monthly_billing(&legacy) {
                            Some(l) => (l, None),
                            None => {
                                return ProviderOutput::error(
                                    ID,
                                    NAME,
                                    "Grok billing response changed.",
                                )
                            }
                        },
                        Err(msg) => return ProviderOutput::error(ID, NAME, msg),
                    },
                }
            }
            Err(msg) => return ProviderOutput::error(ID, NAME, msg),
        };

        // Paid plan renew / ends (monthly Stripe period — not weekly usage reset).
        if let Some(subs) = fetch_subscriptions(&auth.token) {
            if let Some(period) = parse_plan_period(&subs) {
                lines.extend(plan_period_lines(&period));
            }
        }

        // Accurate Last-30-Days tokens/cost from the local capture ledger only
        // (populated by `spanreed grok-proxy`). Never invents usage from sessions.
        lines.extend(crate::grok_ledger::cost_lines(weekly_start_ms));

        let plan = fetch_plan(&auth.token);
        ProviderOutput::new(ID, NAME, lines).with_plan(plan)
    }
}

fn payg_badge(on_demand_cap: f64) -> MetricLine {
    let payg = if on_demand_cap > 0.0 {
        format!("{} cap", on_demand_cap as i64)
    } else {
        "Disabled".to_string()
    };
    let mut line = MetricLine::badge(MetricKind::Plan, "Pay as you go", payg);
    if let MetricLine::Badge { color, .. } = &mut line {
        *color = Some(if on_demand_cap > 0.0 {
            "#22c55e".into()
        } else {
            "#a3a3a3".into()
        });
    }
    line
}

fn period_end_iso(config: &serde_json::Value) -> Option<String> {
    config
        .get("currentPeriod")
        .and_then(|p| p.get("end"))
        .and_then(util::to_iso)
        .or_else(|| config.get("billingPeriodEnd").and_then(util::to_iso))
}

fn period_start_iso(config: &serde_json::Value) -> Option<String> {
    config
        .get("currentPeriod")
        .and_then(|p| p.get("start"))
        .and_then(util::to_iso)
        .or_else(|| config.get("billingPeriodStart").and_then(util::to_iso))
}

/// Weekly usage epoch start (ms) from credits `currentPeriod.start`.
fn period_start_ms(config: &serde_json::Value) -> Option<i64> {
    let start = period_start_iso(config)?;
    let end = period_end_iso(config)?;
    let used = config
        .get("creditUsagePercent")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    crate::usage_stats::RateWindow::from_period_bounds("Weekly", used, &start, &end)
        .map(|w| w.window_start_ms)
}

/// Unified weekly SuperGrok pool (`?format=credits`).
fn parse_credits_billing(config: &serde_json::Value) -> Option<Vec<MetricLine>> {
    let used_pct = config
        .get("creditUsagePercent")?
        .as_f64()?
        .clamp(0.0, 100.0);
    let resets_at = period_end_iso(config)?;
    let on_demand_cap = units(config.get("onDemandCap")).unwrap_or(0.0);

    let mut lines = vec![MetricLine::percent(
        "Weekly",
        used_pct,
        Some(resets_at.clone()),
    )];
    // Product breakdown of the shared weekly pool (official fields only).
    if let Some(arr) = config.get("productUsage").and_then(|v| v.as_array()) {
        for item in arr {
            let Some(pct) = item.get("usagePercent").and_then(|v| v.as_f64()) else {
                continue;
            };
            let raw = item
                .get("product")
                .and_then(|v| v.as_str())
                .unwrap_or("Product");
            let label = product_label(raw);
            lines.push(MetricLine::percent(
                label,
                pct.clamp(0.0, 100.0),
                Some(resets_at.clone()),
            ));
        }
    }
    lines.push(payg_badge(on_demand_cap));
    Some(lines)
}

fn product_label(raw: &str) -> String {
    match raw {
        "GrokBuild" => "Build".into(),
        "GrokChat" => "Chat".into(),
        "Api" => "Api".into(),
        other => other.to_string(),
    }
}

/// Legacy monthly allotment (bare `/v1/billing` without `format=credits`).
fn parse_legacy_monthly_billing(config: &serde_json::Value) -> Option<Vec<MetricLine>> {
    let used = units(config.get("used"))?;
    let limit = units(config.get("monthlyLimit"))?;
    let on_demand_cap = units(config.get("onDemandCap"))?;
    if limit <= 0.0 {
        return None;
    }
    let resets_at = config.get("billingPeriodEnd").and_then(util::to_iso)?;

    let used_pct = (used / limit * 100.0).clamp(0.0, 100.0);
    let mut lines = vec![MetricLine::percent(
        "Credits used",
        used_pct,
        Some(resets_at),
    )];
    lines.push(payg_badge(on_demand_cap));
    Some(lines)
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_weekly_credits_format() {
        let config = serde_json::json!({
            "currentPeriod": {
                "type": "USAGE_PERIOD_TYPE_WEEKLY",
                "start": "2026-07-03T22:41:23.340272+00:00",
                "end": "2026-07-10T22:41:23.340272+00:00"
            },
            "creditUsagePercent": 1.0,
            "onDemandCap": { "val": 0 },
            "onDemandUsed": { "val": 0 },
            "productUsage": [
                { "product": "Api", "usagePercent": 1.0 },
                { "product": "GrokBuild" },
                { "product": "GrokChat" }
            ],
            "isUnifiedBillingUser": true,
            "prepaidBalance": { "val": 0 },
            "billingPeriodStart": "2026-07-03T22:41:23.340272+00:00",
            "billingPeriodEnd": "2026-07-10T22:41:23.340272+00:00"
        });
        let lines = parse_credits_billing(&config).unwrap();
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Progress { label, used, resets_at, .. }
            if label == "Weekly"
                && (*used - 1.0).abs() < f64::EPSILON
                && resets_at.as_deref() == Some("2026-07-10T22:41:23.340272+00:00")
        )));
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Progress { label, used, .. }
            if label == "Api" && (*used - 1.0).abs() < f64::EPSILON
        )));
        // Products without usagePercent are skipped.
        assert!(!lines.iter().any(|l| matches!(
            l,
            MetricLine::Progress { label, .. } if label == "Build" || label == "Chat"
        )));
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Badge { label, text, .. }
            if label == "Pay as you go" && text == "Disabled"
        )));
        let start = period_start_ms(&config).expect("period start");
        let expected = util::parse_iso_dt("2026-07-03T22:41:23.340272+00:00")
            .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
            .unwrap();
        assert_eq!(start, expected);
    }

    #[test]
    fn weekly_prefers_current_period_end_over_billing_period_end() {
        let config = serde_json::json!({
            "currentPeriod": {
                "type": "USAGE_PERIOD_TYPE_WEEKLY",
                "end": "2026-07-10T00:00:00+00:00"
            },
            "creditUsagePercent": 42.5,
            "billingPeriodEnd": "2026-08-01T00:00:00+00:00"
        });
        let lines = parse_credits_billing(&config).unwrap();
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Progress { label, used, resets_at, .. }
            if label == "Weekly"
                && (*used - 42.5).abs() < f64::EPSILON
                && resets_at.as_deref() == Some("2026-07-10T00:00:00+00:00")
        )));
    }

    #[test]
    fn parses_legacy_monthly_credits_used_and_payg_disabled() {
        let config = serde_json::json!({
            "monthlyLimit": { "val": 60000 },
            "used": { "val": 15000 },
            "onDemandCap": { "val": 0 },
            "billingPeriodEnd": "2026-06-01T00:00:00+00:00"
        });
        let lines = parse_legacy_monthly_billing(&config).unwrap();
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Progress { label, used, .. } if label == "Credits used" && *used == 25.0)));
        assert!(lines.iter().any(|l| matches!(l, MetricLine::Badge { label, text, .. } if label == "Pay as you go" && text == "Disabled")));
    }

    #[test]
    fn payg_cap_when_enabled() {
        let config = serde_json::json!({
            "creditUsagePercent": 0.0,
            "onDemandCap": { "val": 500 },
            "billingPeriodEnd": "2026-07-10T00:00:00+00:00"
        });
        let lines = parse_credits_billing(&config).unwrap();
        assert!(lines
            .iter()
            .any(|l| matches!(l, MetricLine::Badge { text, .. } if text == "500 cap")));
    }

    #[test]
    fn none_when_limit_missing() {
        assert!(
            parse_legacy_monthly_billing(&serde_json::json!({ "used": { "val": 1 } })).is_none()
        );
        assert!(parse_credits_billing(&serde_json::json!({ "used": { "val": 1 } })).is_none());
    }

    /// Live `?format=credits` for some SuperGrok accounts: period + PAYG only,
    /// no `creditUsagePercent` / `productUsage`. Must not parse as weekly.
    #[test]
    fn credits_without_usage_percent_is_not_weekly() {
        let config = serde_json::json!({
            "currentPeriod": {
                "type": "USAGE_PERIOD_TYPE_WEEKLY",
                "start": "2026-07-10T22:41:23.340272+00:00",
                "end": "2026-07-17T22:41:23.340272+00:00"
            },
            "onDemandCap": { "val": 0 },
            "onDemandUsed": { "val": 0 },
            "isUnifiedBillingUser": true,
            "prepaidBalance": { "val": 0 },
            "topUpMethod": "TOP_UP_METHOD_SAVED_PAYMENT_METHOD",
            "billingPeriodStart": "2026-07-10T22:41:23.340272+00:00",
            "billingPeriodEnd": "2026-07-17T22:41:23.340272+00:00"
        });
        assert!(parse_credits_billing(&config).is_none());
        assert!(parse_legacy_monthly_billing(&config).is_none());
    }

    #[test]
    fn legacy_monthly_still_parses_when_credits_shape_empty() {
        let legacy = serde_json::json!({
            "monthlyLimit": { "val": 150000 },
            "used": { "val": 46403 },
            "onDemandCap": { "val": 0 },
            "billingPeriodStart": "2026-07-01T00:00:00+00:00",
            "billingPeriodEnd": "2026-08-01T00:00:00+00:00"
        });
        let lines = parse_legacy_monthly_billing(&legacy).unwrap();
        let expected = 46403.0 / 150000.0 * 100.0;
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Progress { label, used, resets_at, .. }
            if label == "Credits used"
                && (*used - expected).abs() < 0.01
                && resets_at.as_deref() == Some("2026-08-01T00:00:00+00:00")
        )));
    }

    #[test]
    fn plan_period_picks_active_supergrok_renews() {
        let root = serde_json::json!({
            "subscriptions": [
                {
                    "tier": "SUBSCRIPTION_TIER_SUPER_GROK_PRO",
                    "status": "SUBSCRIPTION_STATUS_INACTIVE",
                    "billingPeriodEnd": "2026-06-15T22:40:11Z",
                    "cancelAtPeriodEnd": true
                },
                {
                    "tier": "SUBSCRIPTION_TIER_SUPER_GROK_PRO",
                    "status": "SUBSCRIPTION_STATUS_ACTIVE",
                    "createTime": "2026-06-21T05:34:05.760304Z",
                    "billingInterval": "BILLING_INTERVAL_MONTHLY",
                    "billingPeriodEnd": "2026-07-21T05:34:01Z",
                    "cancelAtPeriodEnd": false
                },
                {
                    "tier": "SUBSCRIPTION_TIER_X_PREMIUM_PLUS",
                    "status": "SUBSCRIPTION_STATUS_ACTIVE"
                }
            ]
        });
        let p = parse_plan_period(&root).unwrap();
        assert_eq!(p.period_end_iso, "2026-07-21T05:34:01Z");
        assert!(!p.cancel_at_period_end);
        assert!(p.monthly);

        let lines = plan_period_lines(&p);
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Text { label, value, .. }
            if label == "Plan renews" && value.starts_with("2026-07-21")
        )));
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Text { label, value, .. }
            if label == "Last renew" && value.starts_with("2026-06-21")
        )));
    }

    #[test]
    fn plan_period_cancel_at_end_labels_ends() {
        let root = serde_json::json!({
            "subscriptions": [{
                "tier": "SUBSCRIPTION_TIER_SUPER_GROK_PRO",
                "status": "SUBSCRIPTION_STATUS_ACTIVE",
                "billingInterval": "BILLING_INTERVAL_MONTHLY",
                "billingPeriodEnd": "2026-08-01T00:00:00Z",
                "cancelAtPeriodEnd": true,
                "createTime": "2026-07-01T00:00:00Z"
            }]
        });
        let p = parse_plan_period(&root).unwrap();
        let lines = plan_period_lines(&p);
        assert!(lines.iter().any(|l| matches!(
            l,
            MetricLine::Text { label, .. } if label == "Plan ends"
        )));
        assert!(!lines.iter().any(|l| matches!(
            l,
            MetricLine::Text { label, .. } if label == "Plan renews"
        )));
    }

    #[test]
    fn plan_period_none_without_period_end() {
        let root = serde_json::json!({
            "subscriptions": [{
                "tier": "SUBSCRIPTION_TIER_X_PREMIUM_PLUS",
                "status": "SUBSCRIPTION_STATUS_ACTIVE"
            }]
        });
        assert!(parse_plan_period(&root).is_none());
    }
}
