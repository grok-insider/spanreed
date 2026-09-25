//! Layering and strict resolution of the shared price table.

use fabrials_pricing::pricing::{build_table, embedded_json, Usage};
use fabrials_pricing::{build_limits, compose_upstream, filter_upstream};

#[test]
fn embedded_table_prices_the_main_families() {
    let t = build_table(embedded_json(), None, None);
    assert!(
        t.exact("claude-opus-4-8").is_some(),
        "claude-opus-4-8 priced"
    );
    assert!(t.exact("gpt-5-codex").is_some(), "gpt-5-codex priced");
    assert!(t.exact("claude-fable-5").is_some(), "claude-fable-5 priced");
}

#[test]
fn the_user_layer_wins_over_published_overlays() {
    let user = r#"{"gpt-6-astra": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6}}"#;
    let default = build_table(embedded_json(), None, None);
    let overridden = build_table(embedded_json(), None, Some(user));
    assert_eq!(
        default.exact("gpt-6-astra").unwrap().input,
        10.0 / 1_000_000.0
    );
    assert_eq!(overridden.exact("gpt-6-astra").unwrap().input, 1e-6);
}

#[test]
fn strict_cost_refuses_an_unpublished_cache_rate_and_dated_guesses() {
    let user = r#"{"acme-1": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6}}"#;
    let t = build_table(embedded_json(), None, Some(user));
    let plain = Usage {
        input: 1_000,
        output: 500,
        ..Default::default()
    };
    let (usd, _) = t.exact_cost("acme-1", plain).expect("token rates known");
    assert!((usd - (1_000.0 * 1e-6 + 500.0 * 2e-6)).abs() < 1e-12);
    let cached = Usage {
        cache_read: 10,
        ..plain
    };
    assert!(
        t.exact_cost("acme-1", cached).is_none(),
        "cache rate unknown"
    );
    assert!(
        t.cost("acme-1", cached).is_some(),
        "lenient cost estimates it"
    );
    assert!(
        t.exact("acme-1-20260101").is_none(),
        "no dated-variant guess"
    );
    assert!(
        t.find("acme-1-20260101").is_some(),
        "lenient lookup guesses"
    );
}

#[test]
fn composed_upstream_prices_the_channel_litellm_does_not_carry() {
    let litellm = serde_json::json!({
        "claude-fable-5": {"input_cost_per_token": 6e-6, "output_cost_per_token": 3e-5},
        "xai/grok-4.6": {"input_cost_per_token": 3e-6, "output_cost_per_token": 9e-6}
    })
    .to_string();
    let models_dev = serde_json::json!({
        "opencode-go": {"models": {
            "deepseek-v4.1-flash": {
                "limit": {"context": 1000000, "output": 384000},
                "cost": {"input": 0.15, "output": 0.6, "cache_read": 0.003}
            },
            "grok-4.6": {"limit": {"context": 500000}, "cost": {"input": 2, "output": 6, "cache_read": 0.5}},
            "ox-alpha-free": {"limit": {"context": 1000000}, "cost": {}}
        }},
        "openrouter": {"models": {"claude-fable-5": {"cost": {"input": 9, "output": 9}}}}
    })
    .to_string();
    let tables = compose_upstream(&litellm, &models_dev).expect("compose");
    let t = build_table(embedded_json(), Some(&tables.prices), None);

    let flash = t.exact("deepseek-v4.1-flash").expect("channel row priced");
    assert!(
        (flash.input - 1.5e-7).abs() < 1e-15,
        "per million to per token"
    );
    assert!(flash.cache_read_known);
    assert!(
        !flash.cache_create_known,
        "a rate the source omits stays unknown"
    );
    assert_eq!(
        t.exact("deepseek-flash").map(|p| p.input),
        Some(flash.input)
    );
    assert!(
        t.exact("ox-alpha-free").is_none(),
        "a row without rates is dropped"
    );
    assert_eq!(t.exact("xai/grok-4.6").expect("litellm row").input, 3e-6);
    assert_eq!(t.exact("grok-4.6").expect("channel spelling").input, 2e-6);

    let windows = build_limits("", Some(&tables.limits), None);
    assert_eq!(
        windows.get("deepseek-flash").map(|l| l.context_window),
        Some(1_000_000)
    );
    assert_eq!(
        windows
            .get("deepseek-v4.1-flash")
            .and_then(|l| l.max_output),
        Some(384_000)
    );
    assert_eq!(
        windows.get("grok-4.6").map(|l| l.context_window),
        Some(500_000)
    );
}

#[test]
fn the_upstream_filter_rejects_tables_without_claude() {
    let upstream = serde_json::json!({
        "mistral-large": {"input_cost_per_token": 2e-6, "output_cost_per_token": 6e-6}
    })
    .to_string();
    assert!(filter_upstream(&upstream).is_err());
    assert!(filter_upstream("not json").is_err());
}

#[test]
fn a_host_source_can_replace_the_published_overlays() {
    use fabrials_pricing::{PricingLayers, PricingMap};
    let embedded = r#"{"acme-1": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6}}"#;
    let overlays = r#"{"override": {"acme-1": {"input_cost_per_token": 3e-6, "output_cost_per_token": 4e-6}},
        "fill_missing": {"acme-1": {"input_cost_per_token": 9e-6, "output_cost_per_token": 9e-6},
                         "acme-2": {"input_cost_per_token": 5e-6, "output_cost_per_token": 6e-6}}}"#;
    let map = PricingMap::from_source(&PricingLayers {
        overlays,
        ..PricingLayers::new(embedded)
    });
    assert_eq!(map.exact("acme-1").unwrap().input, 3e-6);
    assert_eq!(map.exact("acme-2").unwrap().input, 5e-6);
    assert!(map.exact("gpt-6-astra").is_none());
}
