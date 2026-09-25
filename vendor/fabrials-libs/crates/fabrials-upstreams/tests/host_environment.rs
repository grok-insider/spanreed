//! Grok client identity comes from the host, never from the process environment.

use fabrials_fabric::provider::{CredentialInjector, Router};
use fabrials_upstreams::grok::{GrokAdapter, GrokClientIdentity};

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

#[test]
fn relay_variables_do_not_change_the_grok_identity() {
    std::env::set_var("AI_RELAY_GROK_CLIENT_VERSION", "9.9.9");
    std::env::set_var("AI_RELAY_GROK_CLIENT_IDENTIFIER", "from-env");
    let adapter = GrokAdapter::default();
    let hop = adapter.resolve("/grok/v1/chat/completions").unwrap();
    let headers = adapter.inject_for("cli-oauth-token", &hop);
    assert_eq!(header(&headers, "x-grok-client-version"), Some("0.2.84"));
    assert_eq!(
        header(&headers, "x-grok-client-identifier"),
        Some("grok-cli")
    );
}

#[test]
fn host_supplied_identity_is_sent_upstream() {
    let adapter = GrokAdapter {
        identity: GrokClientIdentity {
            version: "1.2.3".into(),
            identifier: "fabrials-test".into(),
        },
        ..Default::default()
    };
    let hop = adapter.resolve("/grok/v1/chat/completions").unwrap();
    let headers = adapter.inject_for("cli-oauth-token", &hop);
    assert_eq!(header(&headers, "x-grok-client-version"), Some("1.2.3"));
    assert_eq!(
        header(&headers, "x-grok-client-identifier"),
        Some("fabrials-test")
    );
}
