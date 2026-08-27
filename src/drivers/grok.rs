//! Grok SuperGrok identity driver: native device-code, refresh, billing.

use std::time::{Duration, Instant};

use crate::accounts;
use crate::creds;
use crate::http::Request;
use crate::model::{MetricLine, ProviderOutput};
use crate::util;

const ISSUER: &str = "https://auth.x.ai";
const CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const SCOPES: &str = "openid profile email offline_access grok-cli:access api:access conversations:read conversations:write workspaces:read workspaces:write";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const REFRESH_URL: &str = "https://auth.x.ai/oauth2/token";
const BILLING_CREDITS_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const BILLING_LEGACY_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const SUBS_URL: &str = "https://grok.com/rest/subscriptions";
const TOKEN_AUTH: &str = "xai-grok-cli";
const REFRESH_BUFFER_MS: i64 = 5 * 60 * 1000;
const AUTH_JSON_ENTRY: &str = "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828";

pub fn login_add(requested: Option<&str>) -> Result<String, String> {
    eprintln!("spanreed: device login (Grok / SuperGrok)");
    let blob = device_login()?;
    commit_doc(requested, &blob, "added")
}

/// Re-run device-code into an existing account (same canonical id).
pub fn login_refresh(raw: &str) -> Result<String, String> {
    let acc = accounts::resolve(raw).ok_or_else(|| format!("unknown account {raw}"))?;
    eprintln!("spanreed: re-auth {}", acc.id);
    let blob = device_login()?;
    accounts::put_secret(
        &acc.provider,
        &acc.alias,
        &serde_json::to_string_pretty(&blob).unwrap(),
    )?;
    if let Some(token) = token_from_doc(&blob) {
        let snap = snapshot_from_token(&token);
        persist_snap(&acc.id, snap);
    }
    Ok(format!("re-authed {}", acc.id))
}

pub fn import_cli(requested: Option<&str>) -> Result<String, String> {
    let path = creds::expand("~/.grok/auth.json");
    let doc =
        creds::read_json(&path).ok_or("no ~/.grok/auth.json — account add grok, or grok login")?;
    if !doc.is_object() {
        return Err("auth.json is not an object".into());
    }
    commit_doc(requested, &doc, "imported")
}

fn commit_doc(
    requested: Option<&str>,
    doc: &serde_json::Value,
    verb: &str,
) -> Result<String, String> {
    let token = token_from_doc(doc).ok_or("auth blob has no access token")?;
    let snap = snapshot_from_token(&token);
    let canon = accounts::unique_alias("grok", snap.slug.as_deref().unwrap_or("acct"));
    accounts::put_secret("grok", &canon, &serde_json::to_string_pretty(doc).unwrap())?;
    let mut acc = accounts::Account::new("grok", &canon)?;
    acc.plan_slug = snap.slug.clone();
    acc.plan_label = snap.label.clone();
    acc.used_pct = snap.used_pct;
    acc.resets_at = snap.resets_at.clone();
    acc.quota_at = Some(util::now_ms());
    acc.billing_interval = snap.billing.interval.clone();
    acc.renews_at = snap.billing.renews_at.clone();
    acc.cancel_at_period_end = snap.billing.cancel_at_period_end;
    acc.billing_checked = true;
    if let Some(l) = &snap.label {
        acc.label = l.clone();
    }
    accounts::upsert(acc)?;
    let mut extra = String::new();
    if let Some(nick) = requested.filter(|n| !n.is_empty() && *n != canon) {
        match accounts::add_nick(&format!("grok/{canon}"), nick) {
            Ok(_) => extra = format!("  alias {nick}"),
            Err(e) => extra = format!("  (alias {nick} skipped: {e})"),
        }
    }
    let plan = snap.label.unwrap_or_else(|| "unknown plan".into());
    Ok(format!(
        "{verb} grok/{canon} ({plan}){extra} — /v1 or /acct/{canon}/v1"
    ))
}

pub fn relabel_generic() -> Result<String, String> {
    let list = accounts::list_provider("grok");
    let mut out = String::new();
    for acc in list {
        if !accounts::is_generic_alias(&acc.alias) {
            continue;
        }
        refresh_snapshot(&acc);
        let acc = accounts::get(&acc.id).unwrap_or(acc);
        let base = acc.plan_slug.as_deref().unwrap_or("acct");
        // unique_alias sees current names including this one — temporarily skip self
        let want = if acc.alias == base {
            continue;
        } else {
            let taken_conflict = accounts::list_provider("grok")
                .iter()
                .any(|a| a.alias == base && a.id != acc.id);
            if taken_conflict {
                accounts::unique_alias("grok", base)
            } else {
                base.to_string()
            }
        };
        match accounts::rename("grok", &acc.alias, &want) {
            Ok(n) => out.push_str(&format!("{} → {}\n", acc.id, n.id)),
            Err(e) => out.push_str(&format!("{}: {e}\n", acc.id)),
        }
    }
    if out.is_empty() {
        out.push_str("nothing to relabel\n");
    }
    Ok(out)
}

/// Host account id stamped on a fabric hop. Path alias wins; else active.
pub fn resolve_account_id(alias: Option<&str>) -> Option<String> {
    if alias.is_none() {
        maybe_autosteer();
    }
    match alias {
        Some(a) => accounts::resolve(a).map(|acc| acc.id),
        None => accounts::active("grok").map(|a| a.id),
    }
}

pub fn probe_accounts() -> Vec<ProviderOutput> {
    let list = accounts::list_provider("grok");
    if list.is_empty() {
        return Vec::new();
    }
    list.into_iter()
        .map(|acc| probe_one(&acc.alias, acc.active))
        .collect()
}

fn probe_one(alias: &str, active: bool) -> ProviderOutput {
    let id = format!("grok/{alias}");
    let name = if active {
        format!("Grok ({alias})*")
    } else {
        format!("Grok ({alias})")
    };
    let Some(token) = ensure_token(alias) else {
        return ProviderOutput::error(
            &id,
            &name,
            "auth expired. spanreed account login grok/{alias}",
        );
    };
    let (mut lines, _start) = match load_billing(&token, BILLING_CREDITS_URL) {
        Ok(cfg) => match parse_credits(&cfg) {
            Some(l) => (l, ()),
            None => match load_billing(&token, BILLING_LEGACY_URL) {
                Ok(legacy) => match parse_legacy(&legacy) {
                    Some(l) => (l, ()),
                    None => {
                        return ProviderOutput::error(&id, &name, "Grok billing response changed.")
                    }
                },
                Err(msg) => return ProviderOutput::error(&id, &name, msg),
            },
        },
        Err(msg) => return ProviderOutput::error(&id, &name, msg),
    };
    let weekly_pct = lines.iter().find_map(|l| match l {
        MetricLine::Progress { label, used, .. } if label == "Weekly" => Some(*used),
        _ => None,
    });
    let week_end_ms = lines.iter().find_map(|l| match l {
        MetricLine::Progress {
            label,
            resets_at: Some(iso),
            ..
        } if label == "Weekly" => {
            util::parse_iso_dt(iso).map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
        }
        _ => None,
    });
    lines.extend(crate::grok_ledger::cost_lines_for_account(
        &id,
        None,
        weekly_pct,
        week_end_ms,
    ));
    let plan = fetch_plan(&token);
    let slug = plan.as_deref().map(|d| classify_plan(d).0.to_string());
    let used = weekly_pct;
    let reset = lines.iter().find_map(|l| match l {
        MetricLine::Progress {
            label, resets_at, ..
        } if label == "Weekly" => resets_at.clone(),
        _ => None,
    });
    persist_snap(
        &id,
        QuotaSnap {
            slug,
            label: plan.clone(),
            used_pct: used,
            resets_at: reset,
            billing: fetch_plan_billing(&token),
        },
    );
    ProviderOutput::new(&id, &name, lines).with_plan(plan)
}

fn device_login() -> Result<serde_json::Value, String> {
    let code_url = format!("{ISSUER}/oauth2/device/code");
    let body = format!(
        "client_id={}&scope={}&referrer=grok-build",
        urlenc(CLIENT_ID),
        urlenc(SCOPES)
    );
    let resp = Request::post(code_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("x-grok-client-surface", "cli")
        .body(body)
        .send()
        .map_err(|e| format!("device code: {e}"))?;
    if resp.status == 404 {
        return Err("device-code login is not enabled for this xAI deployment".into());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "device code HTTP {}: {}",
            resp.status,
            resp.body.chars().take(200).collect::<String>()
        ));
    }
    let json = resp.json().ok_or("device code json")?;
    let device_code = json
        .get("device_code")
        .and_then(|v| v.as_str())
        .ok_or("no device_code")?
        .to_string();
    let user_code = json
        .get("user_code")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    let uri = json
        .get("verification_uri_complete")
        .and_then(|v| v.as_str())
        .or_else(|| json.get("verification_uri").and_then(|v| v.as_str()))
        .unwrap_or("https://auth.x.ai")
        .to_string();
    let interval = json.get("interval").and_then(|v| v.as_u64()).unwrap_or(5);
    let expires = json
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or(900);

    eprintln!("Open:  {uri}");
    eprintln!("Code:  {user_code}");
    eprintln!("Waiting for approval…");

    let token_url = format!("{ISSUER}/oauth2/token");
    let deadline = Instant::now() + Duration::from_secs(expires.max(60));
    let mut wait = Duration::from_secs(interval.max(1));
    std::thread::sleep(wait);
    loop {
        if Instant::now() > deadline {
            return Err("device code expired".into());
        }
        let poll = format!(
            "grant_type={}&device_code={}&client_id={}",
            urlenc(DEVICE_GRANT),
            urlenc(&device_code),
            urlenc(CLIENT_ID)
        );
        let resp = Request::post(&token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("x-grok-client-surface", "cli")
            .body(poll)
            .send()
            .map_err(|e| format!("token poll: {e}"))?;
        if (200..300).contains(&resp.status) {
            let tok = resp.json().ok_or("token json")?;
            return Ok(tokens_to_auth_json(&tok));
        }
        let err = resp
            .json()
            .and_then(|j| j.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_default();
        match err.as_str() {
            "authorization_pending" => {
                std::thread::sleep(wait);
            }
            "slow_down" => {
                wait += Duration::from_secs(5);
                std::thread::sleep(wait);
            }
            "access_denied" => return Err("authorization denied".into()),
            "expired_token" => return Err("device code expired".into()),
            other => {
                return Err(format!(
                    "token exchange: {} ({})",
                    other,
                    resp.body.chars().take(160).collect::<String>()
                ));
            }
        }
    }
}

fn tokens_to_auth_json(tok: &serde_json::Value) -> serde_json::Value {
    let access = tok
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let refresh = tok.get("refresh_token").and_then(|v| v.as_str());
    let now = util::now_ms();
    let expires_at = tok
        .get("expires_in")
        .and_then(|v| v.as_f64())
        .filter(|n| *n > 0.0)
        .map(|n| now + (n as i64) * 1000)
        .or_else(|| util::jwt_exp_ms(access))
        .unwrap_or(now + 3600 * 1000);
    let iso = util::ms_to_iso(expires_at).unwrap_or_default();
    let mut entry = serde_json::json!({
        "key": access,
        "expires_at": iso,
        "oidc_client_id": CLIENT_ID,
    });
    if let Some(rt) = refresh {
        entry["refresh_token"] = serde_json::json!(rt);
    }
    serde_json::json!({ AUTH_JSON_ENTRY: entry })
}

fn load_doc(alias: &str) -> Option<serde_json::Value> {
    let raw = accounts::get_secret("grok", alias)?;
    serde_json::from_str(&raw).ok()
}

fn save_doc(alias: &str, doc: &serde_json::Value) {
    if let Ok(s) = serde_json::to_string_pretty(doc) {
        let _ = accounts::put_secret("grok", alias, &s);
    }
}

pub fn token_for_alias(alias: &str) -> Option<String> {
    ensure_token(alias)
}

fn ensure_token(alias: &str) -> Option<String> {
    let mut doc = load_doc(alias)?;
    if !doc.is_object() {
        return None;
    }
    let now = util::now_ms();
    let keys: Vec<String> = doc.as_object()?.keys().cloned().collect();
    for entry_key in keys {
        let entry = doc.get(&entry_key)?.clone();
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
            if let Some(new_tok) = refresh(&mut doc, &entry_key) {
                save_doc(alias, &doc);
                return Some(new_tok);
            }
        }
        return Some(token);
    }
    None
}

fn needs_refresh(entry: &serde_json::Value, token: &str, now_ms: i64) -> bool {
    let entry_ms = entry
        .get("expires_at")
        .or_else(|| entry.get("expires"))
        .and_then(util::to_iso)
        .and_then(|iso| {
            time::OffsetDateTime::parse(&iso, &time::format_description::well_known::Rfc3339)
                .ok()
                .map(|t| (t.unix_timestamp_nanos() / 1_000_000) as i64)
        });
    let token_ms = util::jwt_exp_ms(token);
    entry_ms
        .map(|ms| now_ms + REFRESH_BUFFER_MS >= ms)
        .unwrap_or(false)
        || token_ms
            .map(|ms| now_ms + REFRESH_BUFFER_MS >= ms)
            .unwrap_or(false)
}

fn refresh(doc: &mut serde_json::Value, entry_key: &str) -> Option<String> {
    let entry = doc.get(entry_key)?.clone();
    let refresh_token = ["refresh_token", "refresh"]
        .iter()
        .find_map(|k| entry.get(*k).and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let client_id = entry
        .get("oidc_client_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(CLIENT_ID);
    let body = format!(
        "grant_type=refresh_token&client_id={}&refresh_token={}",
        urlenc(client_id),
        urlenc(refresh_token)
    );
    let resp = Request::post(REFRESH_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    let json = resp.json()?;
    let access = json.get("access_token")?.as_str()?.trim().to_string();
    if access.is_empty() {
        return None;
    }
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
    Some(access)
}

fn load_billing(token: &str, url: &str) -> Result<serde_json::Value, String> {
    let resp = Request::get(url)
        .bearer(token)
        .header("X-XAI-Token-Auth", TOKEN_AUTH)
        .header("Accept", "application/json")
        .send()
        .map_err(|_| "Grok billing request failed. Check your connection.".to_string())?;
    if resp.is_auth_error() {
        return Err("Grok auth expired. spanreed account login grok/<alias>".into());
    }
    if !(200..300).contains(&resp.status) {
        return Err(format!(
            "Grok billing request failed (HTTP {}).",
            resp.status
        ));
    }
    let data = resp.json().ok_or("Grok billing response changed.")?;
    data.get("config")
        .cloned()
        .ok_or_else(|| "Grok billing response changed.".into())
}

fn fetch_plan(token: &str) -> Option<String> {
    let resp = Request::get(SETTINGS_URL)
        .bearer(token)
        .header("X-XAI-Token-Auth", TOKEN_AUTH)
        .header("Accept", "application/json")
        .send()
        .ok()?;
    if !(200..300).contains(&resp.status) {
        return None;
    }
    resp.json()?
        .get("subscription_tier_display")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn units(obj: Option<&serde_json::Value>) -> Option<f64> {
    obj?.get("val")?.as_f64()
}

fn period_end_iso(config: &serde_json::Value) -> Option<String> {
    config
        .get("currentPeriod")
        .and_then(|p| p.get("end"))
        .and_then(util::to_iso)
        .or_else(|| config.get("billingPeriodEnd").and_then(util::to_iso))
}

/// Same Heavy-0% rule as `providers/grok.rs`.
fn parse_credits(config: &serde_json::Value) -> Option<Vec<MetricLine>> {
    let limit = units(config.get("monthlyLimit")).unwrap_or(0.0);
    let used_units = units(config.get("used")).unwrap_or(0.0);
    let has_credit_pct = config
        .get("creditUsagePercent")
        .and_then(|v| v.as_f64())
        .is_some();
    let used_pct = match config.get("creditUsagePercent").and_then(|v| v.as_f64()) {
        Some(pct) => pct.clamp(0.0, 100.0),
        None if limit > 0.0 => (used_units / limit * 100.0).clamp(0.0, 100.0),
        None => 0.0,
    };
    let period_type = config
        .get("currentPeriod")
        .and_then(|p| p.get("type"))
        .and_then(|v| v.as_str());
    let resets_at = period_end_iso(config);
    if !has_credit_pct && resets_at.is_none() && limit <= 0.0 {
        return None;
    }
    let label = match period_type {
        Some(t) if t.contains("WEEKLY") => "Weekly",
        Some(t) if t.contains("MONTHLY") => "Monthly",
        Some(_) => "Usage",
        None if has_credit_pct => "Usage",
        None if limit > 0.0 => "Credits used",
        None => "Usage",
    };
    Some(vec![MetricLine::percent(label, used_pct, resets_at)])
}

fn parse_legacy(config: &serde_json::Value) -> Option<Vec<MetricLine>> {
    let limit = units(config.get("monthlyLimit"))?;
    if limit <= 0.0 {
        return None;
    }
    let used = units(config.get("used")).unwrap_or(0.0);
    let pct = (used / limit * 100.0).clamp(0.0, 100.0);
    Some(vec![MetricLine::percent(
        "Monthly",
        pct,
        period_end_iso(config),
    )])
}

struct QuotaSnap {
    slug: Option<String>,
    label: Option<String>,
    used_pct: Option<f64>,
    resets_at: Option<String>,
    billing: accounts::PlanBilling,
}

fn persist_snap(id: &str, snap: QuotaSnap) {
    let _ = accounts::apply_snapshot(
        id,
        snap.slug,
        snap.label,
        snap.used_pct,
        snap.resets_at,
        util::now_ms(),
        Some(snap.billing),
    );
}

fn token_from_doc(doc: &serde_json::Value) -> Option<String> {
    let obj = doc.as_object()?;
    for entry in obj.values() {
        let t = entry.get("key").and_then(|v| v.as_str())?.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

/// Map CCP `subscription_tier_display` to slug + autosteer rank.
/// Official Grok Build names: SuperGrok Heavy/Plus/Lite, SuperGrok,
/// X Premium+, X Premium (see xai-grok-telemetry `normalize_tier`).
pub fn classify_plan(display: &str) -> (&'static str, u8) {
    let s = display.to_ascii_lowercase();
    let compact: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if s.contains("heavy") {
        return ("heavy", 5);
    }
    if s.contains("lite") {
        return ("lite", 2);
    }
    let super_g = s.contains("supergrok") || s.contains("super grok") || s.contains("super_grok");
    if (s.contains("premium+") || compact.contains("premiumplus") || s.contains("premium plus"))
        && !super_g
    {
        return ("premium-plus", 1);
    }
    if (s.contains("x premium") || compact == "xpremium" || s.contains("x_premium")) && !super_g {
        return ("premium", 0);
    }
    if s.contains("plus") && super_g {
        return ("plus", 4);
    }
    if super_g {
        return ("normal", 3);
    }
    if s.contains("plus") {
        return ("plus", 4);
    }
    if s.contains("premium") {
        return ("premium", 0);
    }
    ("unknown", 0)
}

fn snapshot_from_token(token: &str) -> QuotaSnap {
    let label = fetch_plan(token);
    let slug = label.as_deref().map(|d| classify_plan(d).0.to_string());
    let (used_pct, resets_at) = match load_billing(token, BILLING_CREDITS_URL) {
        Ok(cfg) => credits_pct_reset(&cfg),
        Err(_) => (None, None),
    };
    QuotaSnap {
        slug,
        label,
        used_pct,
        resets_at,
        billing: fetch_plan_billing(token),
    }
}

fn fetch_plan_billing(token: &str) -> accounts::PlanBilling {
    let Some(root) = Request::get(SUBS_URL)
        .bearer(token)
        .header("Accept", "application/json")
        .send()
        .ok()
        .filter(|r| (200..300).contains(&r.status))
        .and_then(|r| r.json())
    else {
        return accounts::PlanBilling::default();
    };
    parse_plan_billing(&root).unwrap_or_default()
}

/// Next charge date + month/year from grok.com subscriptions (not weekly usage).
pub fn parse_plan_billing(root: &serde_json::Value) -> Option<accounts::PlanBilling> {
    let arr = root
        .get("subscriptions")
        .and_then(|v| v.as_array())
        .or_else(|| root.as_array())?;
    let mut best: Option<(i32, accounts::PlanBilling)> = None;
    for sub in arr {
        let status = sub.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let active = status.ends_with("_ACTIVE") || status.eq_ignore_ascii_case("ACTIVE");
        if !active {
            continue;
        }
        let period_end = sub
            .get("billingPeriodEnd")
            .or_else(|| sub.get("currentPeriodEnd"))
            .and_then(util::to_iso);
        let tier = sub.get("tier").and_then(|v| v.as_str()).unwrap_or("");
        let score = if tier.contains("SUPER_GROK") {
            100
        } else if tier.contains("GROK") {
            50
        } else if tier.contains("PREMIUM") {
            20
        } else {
            10
        };
        let interval_raw = sub
            .get("billingInterval")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let interval = if interval_raw.contains("YEAR") {
            Some("year".into())
        } else if interval_raw.contains("MONTH") || interval_raw.eq_ignore_ascii_case("month") {
            Some("month".into())
        } else if !interval_raw.is_empty() {
            Some(interval_raw.to_ascii_lowercase())
        } else {
            None
        };
        let cancel = sub.get("cancelAtPeriodEnd").and_then(|v| v.as_bool());
        let row = accounts::PlanBilling {
            interval,
            renews_at: period_end,
            cancel_at_period_end: cancel,
        };
        match &best {
            Some((s, _)) if *s >= score => {}
            _ => best = Some((score, row)),
        }
    }
    best.map(|(_, p)| p)
}

fn credits_pct_reset(config: &serde_json::Value) -> (Option<f64>, Option<String>) {
    let limit = units(config.get("monthlyLimit")).unwrap_or(0.0);
    let used_units = units(config.get("used")).unwrap_or(0.0);
    let used_pct = match config.get("creditUsagePercent").and_then(|v| v.as_f64()) {
        Some(pct) => Some(pct.clamp(0.0, 100.0)),
        None if limit > 0.0 => Some((used_units / limit * 100.0).clamp(0.0, 100.0)),
        None if period_end_iso(config).is_some() => Some(0.0),
        None => None,
    };
    (used_pct, period_end_iso(config))
}

const SNAP_TTL_MS: i64 = 60_000;

pub fn refresh_snapshot(acc: &accounts::Account) {
    let stale = acc
        .quota_at
        .map(|t| util::now_ms().saturating_sub(t) > SNAP_TTL_MS)
        .unwrap_or(true);
    if !stale && acc.used_pct.is_some() && acc.billing_checked {
        return;
    }
    let Some(token) = ensure_token(&acc.alias) else {
        return;
    };
    persist_snap(&acc.id, snapshot_from_token(&token));
}

fn autosteer_cfg() -> (bool, f64) {
    let path = crate::app::config_dir().join("config.json");
    let cfg = creds::read_json(&path);
    let g = cfg
        .as_ref()
        .and_then(|v| v.get("accounts"))
        .and_then(|v| v.get("grok"));
    let on = g
        .and_then(|v| v.get("autosteer"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let pct = g
        .and_then(|v| v.get("exhausted_pct"))
        .and_then(|v| v.as_f64())
        .unwrap_or(100.0);
    (on, pct)
}

pub fn autosteer_set(on: bool) -> Result<String, String> {
    let path = crate::app::config_dir().join("config.json");
    let mut root = creds::read_json(&path).unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    let accounts = root
        .as_object_mut()
        .unwrap()
        .entry("accounts")
        .or_insert_with(|| serde_json::json!({}));
    let grok = accounts
        .as_object_mut()
        .unwrap()
        .entry("grok")
        .or_insert_with(|| serde_json::json!({}));
    grok.as_object_mut()
        .unwrap()
        .insert("autosteer".into(), serde_json::json!(on));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&root).unwrap() + "\n")
        .map_err(|e| e.to_string())?;
    Ok(format!("autosteer {}", if on { "on" } else { "off" }))
}

pub fn autosteer_status() -> String {
    let (on, pct) = autosteer_cfg();
    format!(
        "autosteer={}  exhausted>={pct}  (only default /v1; /acct/… stays pinned)",
        if on { "on" } else { "off" }
    )
}

fn maybe_autosteer() {
    let (on, exhausted) = autosteer_cfg();
    if !on {
        return;
    }
    let list = accounts::list_provider("grok");
    if list.len() < 2 {
        return;
    }
    let Some(active) = list.iter().find(|a| a.active).cloned() else {
        return;
    };
    for a in &list {
        refresh_snapshot(a);
    }
    let fresh = accounts::list_provider("grok");
    let Some(pick) = pick_autosteer(&fresh, exhausted, util::now_ms()) else {
        return;
    };
    let pick = pick.clone();
    if pick.id == active.id {
        return;
    }
    if accounts::set_active(&pick.id).is_ok() {
        log::info!(
            "autosteer {} → {} ({:.0}% used)",
            active.id,
            pick.id,
            active.used_pct.unwrap_or(0.0)
        );
    }
}

pub fn pick_autosteer(
    accounts: &[accounts::Account],
    exhausted_pct: f64,
    now_ms: i64,
) -> Option<&accounts::Account> {
    // crates.io `fabrials-accounts` 0.1.1 has plan-first `pick_autosteer` only.
    // Deadline-first lives here until that crate is published with the helpers.
    let mut best: Option<(f64, &accounts::Account)> = None;
    for a in accounts {
        let used = a.used_pct.unwrap_or(0.0);
        if used >= exhausted_pct {
            continue;
        }
        let slug = a.plan_slug.as_deref().or_else(|| {
            a.plan_label
                .as_deref()
                .map(|d| classify_plan(d).0)
                .filter(|s| !s.is_empty())
        });
        let rank = slug.map(grok_burn_rank).unwrap_or(0);
        let hours = fabrials_accounts::hours_to_reset(a.resets_at.as_deref(), now_ms);
        let score = deadline_first_score(used, hours, rank);
        match &best {
            Some((s, _)) if *s >= score => {}
            _ => best = Some((score, a)),
        }
    }
    best.map(|(_, a)| a)
}

fn grok_burn_rank(slug: &str) -> u8 {
    5u8.saturating_sub(fabrials_accounts::grok_plan_rank(slug))
}

fn deadline_first_score(used: f64, hours: Option<f64>, rank: u8) -> f64 {
    let h = hours.unwrap_or(0.0);
    1e12 / h.max(0.01) + 10_000.0 * f64::from(rank) + 100.0 * used
}

fn urlenc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_monthly_renews() {
        let root = serde_json::json!({
            "subscriptions": [{
                "status": "SUBSCRIPTION_STATUS_ACTIVE",
                "tier": "SUBSCRIPTION_TIER_SUPER_GROK_HEAVY",
                "billingInterval": "BILLING_INTERVAL_MONTHLY",
                "billingPeriodEnd": "2026-09-16T12:00:00Z",
                "cancelAtPeriodEnd": false
            }]
        });
        let b = parse_plan_billing(&root).unwrap();
        assert_eq!(b.interval.as_deref(), Some("month"));
        assert!(b.renews_at.unwrap().starts_with("2026-09-16"));
        assert_eq!(b.cancel_at_period_end, Some(false));
    }

    #[test]
    fn classify_plans() {
        assert_eq!(classify_plan("SuperGrok Heavy").0, "heavy");
        assert_eq!(classify_plan("SuperGrok Lite").0, "lite");
        assert_eq!(classify_plan("SuperGrok Plus").0, "plus");
        assert_eq!(classify_plan("SuperGrok").0, "normal");
        assert_eq!(classify_plan("X Premium+").0, "premium-plus");
        assert_eq!(classify_plan("x_premium_plus").0, "premium-plus");
        assert_eq!(classify_plan("X Premium").0, "premium");
        assert_eq!(classify_plan("x_premium").0, "premium");
    }

    fn acc(
        alias: &str,
        slug: &str,
        used: f64,
        reset_hours: Option<f64>,
        now: i64,
    ) -> accounts::Account {
        let mut a = accounts::Account::new("grok", alias).unwrap();
        a.plan_slug = Some(slug.into());
        a.used_pct = Some(used);
        if let Some(h) = reset_hours {
            let end = now + (h * 3_600_000.0) as i64;
            a.resets_at = util::ms_to_iso(end);
        }
        a
    }

    #[test]
    fn autosteer_skips_exhausted_prefers_plan() {
        let now = 1_700_000_000_000;
        let list = vec![
            acc("heavy", "heavy", 100.0, Some(80.0), now),
            acc("plus", "plus", 40.0, Some(80.0), now),
        ];
        let pick = pick_autosteer(&list, 100.0, now).unwrap();
        assert_eq!(pick.alias, "plus");
    }

    #[test]
    fn autosteer_burns_premium_plus_before_live_heavy() {
        let now = 1_700_000_000_000;
        let list = vec![
            acc("heavy-2", "heavy", 3.0, Some(80.0), now),
            acc("premium-plus-1", "premium-plus", 0.0, Some(80.0), now),
            acc("heavy-1", "heavy", 100.0, Some(80.0), now),
        ];
        let pick = pick_autosteer(&list, 100.0, now).unwrap();
        assert_eq!(pick.alias, "premium-plus-1");
    }

    #[test]
    fn autosteer_picks_sooner_reset_over_smaller_plan() {
        let now = 1_700_000_000_000;
        let list = vec![
            acc("premium-plus-1", "premium-plus", 0.0, Some(120.0), now),
            acc("heavy-2", "heavy", 3.0, Some(48.0), now),
        ];
        let pick = pick_autosteer(&list, 100.0, now).unwrap();
        assert_eq!(pick.alias, "heavy-2");
    }

    #[test]
    fn autosteer_urgent_lite_beats_fresh_heavy() {
        let now = 1_700_000_000_000;
        let list = vec![
            acc("heavy", "heavy", 20.0, Some(120.0), now),
            acc("lite", "lite", 90.0, Some(2.0), now),
        ];
        let pick = pick_autosteer(&list, 100.0, now).unwrap();
        assert_eq!(pick.alias, "lite");
    }

    #[test]
    fn autosteer_all_full_none() {
        let now = 1_700_000_000_000;
        let list = vec![
            acc("a", "heavy", 100.0, Some(10.0), now),
            acc("b", "plus", 100.0, Some(10.0), now),
        ];
        assert!(pick_autosteer(&list, 100.0, now).is_none());
    }

    #[test]
    fn credits_heavy_missing_pct_is_zero() {
        let cfg = serde_json::json!({
            "currentPeriod": { "type": "WEEKLY", "end": "2026-08-21T00:00:00Z" }
        });
        let lines = parse_credits(&cfg).unwrap();
        match &lines[0] {
            MetricLine::Progress { label, used, .. } => {
                assert_eq!(label, "Weekly");
                assert_eq!(*used, 0.0);
            }
            _ => panic!("expected progress"),
        }
    }
}
