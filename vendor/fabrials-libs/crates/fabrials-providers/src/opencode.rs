//! OpenCode console sign-in (the CLI device login) and the OpenCode Go
//! subscription it unlocks. The console token serves both the account API and
//! Go inference, so no API key is needed.

use serde_json::{json, Value};

pub const ORIGIN: &str = "https://opencode.ai";
pub const CONSOLE: &str = "https://opencode.ai/console";
pub const CLIENT_ID: &str = "opencode-cli";
/// Go inference; OpenAI routes live under `/openai/v1`, Messages under `/anthropic/v1`.
pub const GO_INFERENCE: &str = "https://opencode.ai/inference/go";
/// Workspace header for Go inference.
pub const ORG_HEADER: &str = "x-opencode-org-id";
/// Workspace header for console API calls.
pub const CONSOLE_ORG_HEADER: &str = "x-org-id";
/// A refresh that never got an answer: the token may or may not have rotated.
pub const REFRESH_UNANSWERED: &str = "OpenCode token refresh unavailable";

pub use crate::device_flow::{DeviceAuthorization, DeviceView, PollResult};
pub use crate::oauth::{ensure_access, valid_access};

/// The workspace of a stored sign-in.
pub fn org_id(document: &Value) -> Option<&str> {
    document
        .get("org_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_graphic()))
}

/// A sign-in that can serve requests: a live token and its workspace.
pub fn valid_grant(document: &Value, now_ms: i64) -> Result<(), String> {
    valid_access(document, now_ms).ok_or("Invalid or expired OpenCode grant")?;
    org_id(document).ok_or("OpenCode workspace missing")?;
    Ok(())
}

/// `true`, `false` or unknown auto-renew plus the period end, from `/api/go/status`.
pub fn go_billing(status: &Value) -> Option<crate::billing::PlanBilling> {
    let access = status.get("access").filter(|a| a.is_object())?;
    let cancel = access
        .get("cancelAtPeriodEnd")
        .or_else(|| status.get("cancelAtPeriodEnd"))
        .and_then(Value::as_bool);
    let starts = access.get("startsAt").and_then(Value::as_str);
    let ends = access.get("endsAt").and_then(Value::as_str);
    let days = match (starts.and_then(parse_ms), ends.and_then(parse_ms)) {
        (Some(start), Some(end)) if end > start => Some((end - start) / 86_400_000),
        _ => None,
    };
    let interval = days.map(|days| if days >= 300 { "year" } else { "month" });
    let mut billing = crate::billing::PlanBilling {
        status: Some("active".into()),
        interval: interval.map(str::to_owned),
        auto_renew: cancel.map(|cancel| !cancel),
        ..Default::default()
    };
    if let Some(end) = ends.filter(|end| !end.is_empty() && end.len() <= 64) {
        match billing.auto_renew {
            Some(true) => billing.renews_at = Some(end.into()),
            Some(false) => billing.ends_at = Some(end.into()),
            None => billing.paid_through = Some(end.into()),
        }
    }
    Some(billing)
}

/// Plan slug from `/api/go/status` (`go`, `go-plus`, ...), `None` without a subscription.
pub fn go_plan(status: &Value) -> Option<String> {
    status.get("access").filter(|a| a.is_object())?;
    let product = status
        .get("product")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|p| !p.is_empty() && p.len() <= 64)?;
    let slug: String = product
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let slug = match slug.as_str() {
        "goplus" => "go-plus".to_string(),
        _ => slug,
    };
    (!slug.is_empty()).then_some(slug)
}

/// Display name of a plan slug: `go` → `Go`, `go-plus` → `Go Plus`.
pub fn plan_label(slug: &str) -> Option<String> {
    let words: Vec<String> = slug
        .split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Model ids OpenCode Go offers this workspace, from `/api/config`.
pub fn go_models(config: &Value) -> Vec<String> {
    let provider = config
        .get("config")
        .unwrap_or(config)
        .get("provider")
        .and_then(|providers| providers.get("opencode-go"));
    let mut models: Vec<String> = provider
        .and_then(|p| p.get("models"))
        .and_then(Value::as_object)
        .map(|models| {
            models
                .iter()
                .filter(|(_, model)| {
                    model.get("status").and_then(Value::as_str) != Some("deprecated")
                })
                .map(|(id, _)| id.clone())
                .collect()
        })
        .unwrap_or_default();
    models
        .retain(|id| !id.is_empty() && id.len() <= 256 && id.bytes().all(|b| b.is_ascii_graphic()));
    models.sort();
    models
}

fn parse_ms(text: &str) -> Option<i64> {
    let parsed =
        time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000).ok()
}

pub struct Client {
    http: std::sync::Arc<dyn crate::http::HttpPort>,
}

impl Client {
    #[cfg(feature = "reqwest")]
    pub fn new() -> Result<Self, String> {
        crate::http::default_port(20)
            .map(Self::with_http)
            .map_err(|_| "Could not initialize OpenCode client".into())
    }

    pub fn with_http(http: std::sync::Arc<dyn crate::http::HttpPort>) -> Self {
        Self { http }
    }

    pub fn begin(&self) -> Result<DeviceAuthorization, String> {
        let response = self
            .http
            .post_json(
                &format!("{CONSOLE}/auth/device/code"),
                &[("Accept", "application/json")],
                &json!({ "client_id": CLIENT_ID }),
            )
            .map_err(|_| "OpenCode authorization unavailable")?;
        let value = read_response(response)?;
        let path = value
            .get("verification_uri_complete")
            .or_else(|| value.get("verification_uri"))
            .and_then(Value::as_str)
            .ok_or("OpenCode verification URL missing")?;
        let url = url::Url::parse(ORIGIN)
            .and_then(|origin| origin.join(path))
            .map_err(|_| "Invalid OpenCode verification URL")?;
        if url.scheme() != "https"
            || url.host_str() != Some("opencode.ai")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err("Invalid OpenCode verification origin".into());
        }
        let required = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty() && s.len() <= 4096)
                .map(str::to_string)
                .ok_or_else(|| "Incomplete OpenCode authorization".to_string())
        };
        Ok(DeviceAuthorization {
            device_code: required("device_code")?,
            view: DeviceView {
                verification_uri: url.to_string(),
                user_code: required("user_code")?,
                expires_in: value
                    .get("expires_in")
                    .and_then(Value::as_u64)
                    .filter(|s| *s > 0 && *s <= 3600)
                    .ok_or("Invalid OpenCode authorization expiry")?,
                interval: value
                    .get("interval")
                    .and_then(Value::as_u64)
                    .unwrap_or(5)
                    .clamp(1, 60),
            },
        })
    }

    /// One check of a pending sign-in; on approval, the stored document with
    /// the user's email and first workspace.
    pub fn poll(&self, code: &str, now_ms: i64) -> Result<PollResult, String> {
        let response = self
            .http
            .post_json(
                &format!("{CONSOLE}/auth/device/token"),
                &[("Accept", "application/json")],
                &json!({
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                    "device_code": code,
                    "client_id": CLIENT_ID,
                }),
            )
            .map_err(|_| "OpenCode authorization check unavailable")?;
        let success = response.is_success();
        let status = response.status;
        let value = read_json(response)?;
        match value.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => return Ok(PollResult::Pending),
            Some("slow_down") => return Ok(PollResult::SlowDown),
            Some("access_denied") => return Err("OpenCode authorization denied".into()),
            Some("expired_token") => return Err("OpenCode authorization expired".into()),
            Some(_) => return Err("OpenCode authorization rejected".into()),
            _ => {}
        }
        if !success {
            return Err(format!("OpenCode authorization HTTP {status}"));
        }
        let mut document = token_document(&value, now_ms)?;
        let token = valid_access(&document, now_ms).ok_or("OpenCode access token missing")?;
        let user = self.user(&token)?;
        let org = self
            .orgs(&token)?
            .into_iter()
            .next()
            .ok_or("OpenCode workspace missing")?;
        document["org_id"] = Value::String(org.0);
        document["org_name"] = Value::String(org.1);
        if let Some(email) = user
            .get("email")
            .and_then(Value::as_str)
            .filter(|e| e.contains('@') && e.len() <= 254)
        {
            document["email"] = Value::String(email.into());
        }
        if let Some(id) = user
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| id.len() <= 128)
        {
            document["user_id"] = Value::String(id.into());
        }
        Ok(PollResult::Authorized(document))
    }

    /// A renewed document; workspace and identity fields carry over.
    pub fn refresh(&self, document: &Value, now_ms: i64) -> Result<Value, String> {
        let refresh = document
            .get("refresh_token")
            .and_then(Value::as_str)
            .ok_or("OpenCode sign-in required")?;
        let response = self
            .http
            .post_json(
                &format!("{CONSOLE}/auth/device/token"),
                &[("Accept", "application/json")],
                &json!({
                    "grant_type": "refresh_token",
                    "refresh_token": refresh,
                    "client_id": CLIENT_ID,
                }),
            )
            .map_err(|_| REFRESH_UNANSWERED)?;
        let value = read_response(response)?;
        let mut replacement = token_document(&value, now_ms)?;
        if replacement
            .get("refresh_token")
            .and_then(Value::as_str)
            .is_none()
        {
            replacement["refresh_token"] = Value::String(refresh.into());
        }
        for key in ["org_id", "org_name", "email", "user_id"] {
            if let Some(value) = document.get(key).filter(|v| v.is_string()) {
                replacement[key] = value.clone();
            }
        }
        Ok(replacement)
    }

    pub fn user(&self, token: &str) -> Result<Value, String> {
        self.console_get("/api/user", token, None)
    }

    /// `(id, name)` of each workspace, sorted by name like the CLI.
    pub fn orgs(&self, token: &str) -> Result<Vec<(String, String)>, String> {
        let value = self.console_get("/api/orgs", token, None)?;
        let mut orgs: Vec<(String, String)> = value
            .as_array()
            .map(|orgs| {
                orgs.iter()
                    .filter_map(|org| {
                        let id = org.get("id")?.as_str()?;
                        let name = org.get("name").and_then(Value::as_str).unwrap_or(id);
                        org_id(&json!({ "org_id": id }))?;
                        Some((id.to_string(), name.chars().take(128).collect()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        orgs.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
        Ok(orgs)
    }

    pub fn go_status(&self, token: &str, org: &str) -> Result<Value, String> {
        self.console_get("/api/go/status", token, Some(org))
    }

    pub fn config(&self, token: &str, org: &str) -> Result<Value, String> {
        self.console_get("/api/config", token, Some(org))
    }

    fn console_get(&self, path: &str, token: &str, org: Option<&str>) -> Result<Value, String> {
        let bearer = format!("Bearer {token}");
        let mut headers = vec![
            ("Authorization", bearer.as_str()),
            ("Accept", "application/json"),
        ];
        if let Some(org) = org {
            headers.push((CONSOLE_ORG_HEADER, org));
        }
        read_response(
            self.http
                .get(&format!("{CONSOLE}{path}"), &headers)
                .map_err(|_| "OpenCode console unavailable")?,
        )
    }
}

fn token_document(value: &Value, now_ms: i64) -> Result<Value, String> {
    let token = value
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("OpenCode access token missing")?;
    let seconds = value
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|s| *s > 0)
        .ok_or("OpenCode token expiry missing")?;
    let expires = seconds
        .checked_mul(1000)
        .and_then(|s| now_ms.checked_add(s))
        .ok_or("Invalid OpenCode token expiry")?;
    Ok(json!({
        "auth_type": "oauth",
        "access_token": token,
        "refresh_token": value.get("refresh_token").and_then(Value::as_str),
        "expires_at_ms": expires,
        "client_id": CLIENT_ID,
    }))
}

fn read_response(response: crate::http::HttpResponse) -> Result<Value, String> {
    if !response.is_success() {
        let body = serde_json::from_slice::<Value>(&response.body).ok();
        let oauth_error = body
            .as_ref()
            .and_then(|body| body.get("error")?.as_str().map(str::to_owned));
        if response.status == 400 && oauth_error.as_deref() == Some("invalid_grant") {
            return Err("OpenCode ended this sign-in. Authorize the account again.".into());
        }
        return Err(match response.status {
            401 => "OpenCode rejected this sign-in (HTTP 401). Authorize the account again.".into(),
            403 => "OpenCode denied access (HTTP 403). Check this account's workspace.".into(),
            status => format!("OpenCode HTTP {status}"),
        });
    }
    read_json(response)
}

fn read_json(response: crate::http::HttpResponse) -> Result<Value, String> {
    response.json_within(1_048_576, "OpenCode response too large")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::ScriptedHttp;

    fn client(replies: Vec<(u16, Value)>) -> Client {
        Client::with_http(std::sync::Arc::new(ScriptedHttp::new(replies)))
    }

    #[test]
    fn device_login_keeps_the_workspace_and_email() {
        let begin = client(vec![(
            200,
            json!({"device_code":"dc","user_code":"JCKZ-BRTC","verification_uri":"/console/device","verification_uri_complete":"/console/device?user_code=JCKZ-BRTC&client_id=opencode-cli","expires_in":600,"interval":5}),
        )])
        .begin()
        .unwrap();
        assert_eq!(
            begin.view.verification_uri,
            "https://opencode.ai/console/device?user_code=JCKZ-BRTC&client_id=opencode-cli"
        );
        let result = client(vec![
            (200, json!({"access_token":"at","refresh_token":"rt","expires_in":2591999,"token_type":"Bearer"})),
            (200, json!({"id":"acc_1","email":"owner@example.test","name":null,"role":"user"})),
            (200, json!([{"id":"wrk_b","name":"Zeta"},{"id":"wrk_a","name":"Default"}])),
        ])
        .poll("dc", 1000)
        .unwrap();
        let PollResult::Authorized(document) = result else {
            panic!("expected a grant");
        };
        assert_eq!(document["org_id"], "wrk_a");
        assert_eq!(document["email"], "owner@example.test");
        assert_eq!(document["expires_at_ms"], 1000 + 2_591_999_000i64);
        assert!(valid_grant(&document, 2000).is_ok());
        assert!(valid_grant(&json!({"access_token":"at","expires_at_ms":5000}), 2000).is_err());
    }

    #[test]
    fn pending_and_revoked_sign_ins() {
        assert!(matches!(
            client(vec![(400, json!({"error":"authorization_pending"}))])
                .poll("dc", 0)
                .unwrap(),
            PollResult::Pending
        ));
        let error = client(vec![(400, json!({"error":"invalid_grant"}))])
            .refresh(&json!({"refresh_token":"old","org_id":"wrk_a"}), 0)
            .unwrap_err();
        assert!(error.contains("Authorize the account again"), "{error}");
    }

    #[test]
    fn refresh_rotates_and_keeps_the_workspace() {
        let renewed = client(vec![(
            200,
            json!({"access_token":"new","refresh_token":"rt2","expires_in":60}),
        )])
        .refresh(
            &json!({"refresh_token":"rt","org_id":"wrk_a","email":"owner@example.test"}),
            0,
        )
        .unwrap();
        assert_eq!(renewed["access_token"], "new");
        assert_eq!(renewed["refresh_token"], "rt2");
        assert_eq!(renewed["org_id"], "wrk_a");
        assert_eq!(renewed["email"], "owner@example.test");
    }

    #[test]
    fn go_status_reports_renewal_or_end() {
        let status = json!({"product":"go","cancelAtPeriodEnd":false,"access":{"startsAt":"2026-09-10T09:41:27.000Z","endsAt":"2026-10-10T09:41:27.000Z","cancelAtPeriodEnd":false}});
        let billing = go_billing(&status).unwrap();
        assert_eq!(billing.auto_renew, Some(true));
        assert_eq!(billing.interval.as_deref(), Some("month"));
        assert_eq!(
            billing.renews_at.as_deref(),
            Some("2026-10-10T09:41:27.000Z")
        );
        let cancelled = json!({"access":{"startsAt":"2026-09-10T09:41:27.000Z","endsAt":"2026-10-10T09:41:27.000Z","cancelAtPeriodEnd":true}});
        let billing = go_billing(&cancelled).unwrap();
        assert_eq!(billing.auto_renew, Some(false));
        assert_eq!(billing.ends_at.as_deref(), Some("2026-10-10T09:41:27.000Z"));
        assert!(go_billing(&json!({"product":null})).is_none());
    }

    #[test]
    fn plan_names_follow_the_product() {
        let status = |product: &str| json!({"product": product, "access": {"endsAt": "2026-10-10T09:41:27.000Z"}});
        assert_eq!(go_plan(&status("go")).as_deref(), Some("go"));
        assert_eq!(go_plan(&status("go_plus")).as_deref(), Some("go-plus"));
        assert_eq!(go_plan(&status("GoPlus")).as_deref(), Some("go-plus"));
        assert_eq!(go_plan(&json!({"product": "go"})), None);
        assert_eq!(plan_label("go").as_deref(), Some("Go"));
        assert_eq!(plan_label("go-plus").as_deref(), Some("Go Plus"));
    }

    #[test]
    fn go_models_come_from_the_workspace_config() {
        let config = json!({"config":{"provider":{"opencode-go":{"models":{
            "glm-5.3":{},"deepseek-v4-flash":{},"old":{"status":"deprecated"}
        }}}}});
        assert_eq!(go_models(&config), vec!["deepseek-v4-flash", "glm-5.3"]);
    }
}
