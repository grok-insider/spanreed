//! The crate must never pick endpoints up from the process environment:
//! only the host decides where credentials and tokens are sent.

use fabrials_providers::{claude, kimi};

#[test]
fn relay_variables_do_not_redirect_claude_or_kimi() {
    std::env::set_var("AI_RELAY_CLAUDE_TOKEN_URL", "http://127.0.0.1:9/token");
    std::env::set_var("AI_RELAY_CLAUDE_API_BASE", "http://127.0.0.1:9");
    std::env::set_var("AI_RELAY_CLAUDE_AUTHORIZE_URL", "http://127.0.0.1:9/authorize");
    std::env::set_var("AI_RELAY_KIMI_TOKEN_URL", "http://127.0.0.1:9/token");
    std::env::set_var("AI_RELAY_KIMI_API_BASE", "http://127.0.0.1:9");

    let claude_client = claude::Client::new().unwrap();
    assert_eq!(claude_client.origins().token_url, claude::TOKEN_URL);
    assert_eq!(claude_client.origins().api_base, claude::API_BASE);
    assert!(claude::authorize_url("state", "challenge").starts_with(claude::AUTHORIZE_URL));

    let kimi_client = kimi::Client::new().unwrap();
    assert_eq!(kimi_client.origins().token_url, kimi::TOKEN_URL);
    assert_eq!(kimi_client.origins().api_base, kimi::API_BASE);
}

#[test]
fn host_supplied_origins_replace_the_pinned_ones() {
    let origins = claude::Origins::with_overrides(
        Some("http://127.0.0.1:4000/"),
        Some("not-a-url"),
        Some("https://example.test/authorize"),
    );
    assert_eq!(origins.api_base, "http://127.0.0.1:4000");
    assert_eq!(origins.token_url, claude::TOKEN_URL);
    let client = claude::Client::with_origins(origins.clone()).unwrap();
    assert_eq!(client.origins(), &origins);
    assert!(claude::authorize_url_at(&origins, "s", "c")
        .starts_with("https://example.test/authorize?"));

    let kimi_origins = kimi::Origins::with_overrides(Some("http://127.0.0.1:4001"), None);
    let kimi_client = kimi::Client::with_origins(kimi_origins).unwrap();
    assert_eq!(kimi_client.origins().api_base, "http://127.0.0.1:4001");
    assert_eq!(kimi_client.origins().token_url, kimi::TOKEN_URL);
}
