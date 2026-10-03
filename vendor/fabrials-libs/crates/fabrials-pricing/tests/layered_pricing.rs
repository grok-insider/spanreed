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

#[test]
fn sol_and_luna_price_the_full_request_only_above_the_prompt_threshold() {
    use fabrials_pricing::cost::list_cost_usd_with;
    use fabrials_types::HopRecord;

    let table = build_table(embedded_json(), None, None);
    for (model, input, read, write, output) in [
        ("gpt-6.1-sol", 2e-6, 1e-7, 2.5e-6, 1e-5),
        ("gpt-6-sol", 2e-6, 2e-7, 2.5e-6, 1e-5),
        ("gpt-6-luna", 1e-7, 1e-8, 1.25e-7, 5e-7),
    ] {
        let price = table.exact(model).unwrap();
        assert_eq!(price.long_context_threshold_tokens, Some(272_000));
        assert_eq!(price.input, input);
        assert_eq!(price.cache_read, read);
        assert_eq!(price.cache_create, write);
        assert_eq!(price.output, output);
        assert_eq!(
            table.exact(&format!("openai/{model}")).unwrap().input,
            input
        );
        assert_eq!(
            table.find(&format!("{model}-20260930")).unwrap().input,
            input
        );
        assert!(table.exact(&format!("{model}-unknown")).is_none());
        for prompt in [200_000, 271_999, 272_000, 272_001] {
            let factor = if prompt > 272_000 { 2.0 } else { 1.0 };
            let output_factor = if prompt > 272_000 { 1.5 } else { 1.0 };
            for (cache_read, cache_create) in [(0, 0), (100_000, 0), (0, 100_000), (50_000, 50_000)]
            {
                let usage = Usage {
                    input: prompt - cache_read - cache_create,
                    cache_read,
                    cache_create,
                    output: 300_000,
                };
                let expected = (usage.input as f64 * input
                    + cache_read as f64 * read
                    + cache_create as f64 * write)
                    * factor
                    + usage.output as f64 * output * output_factor;
                let strict = table.exact_cost(model, usage).unwrap().0;
                assert!((strict - expected).abs() < 1e-12, "{model} {prompt}");
                assert!((table.cost(model, usage).unwrap() - expected).abs() < 1e-12);
                if cache_create == 0 {
                    let record = HopRecord {
                        model: Some(model.into()),
                        input_tokens: prompt,
                        cached_input_tokens: cache_read,
                        output_tokens: usage.output,
                        ..Default::default()
                    };
                    assert!(
                        (list_cost_usd_with(&record, &table).unwrap() - expected).abs() < 1e-12
                    );
                }
            }
        }
    }
    assert!(table
        .exact_cost("unpublished-model", Usage::default())
        .is_none());
}

#[test]
fn model_threshold_survives_filtering_and_price_layer_precedence() {
    let row = serde_json::json!({
        "input_cost_per_token": 9e-6,
        "output_cost_per_token": 9e-6,
        "long_context_threshold_tokens": 123,
        "input_cost_per_token_above_200k_tokens": 18e-6
    });
    let remote = serde_json::json!({"claude-test": row, "gpt-6-sol": row}).to_string();
    let filtered = filter_upstream(&remote).unwrap();
    let value: serde_json::Value = serde_json::from_str(&filtered).unwrap();
    assert_eq!(value["gpt-6-sol"]["long_context_threshold_tokens"], 123);
    let published = build_table(embedded_json(), Some(&filtered), None);
    assert_eq!(
        published
            .exact("gpt-6-sol")
            .unwrap()
            .long_context_threshold_tokens,
        Some(272_000)
    );
    let user = build_table(embedded_json(), Some(&filtered), Some(&filtered));
    assert_eq!(
        user.exact("gpt-6-sol")
            .unwrap()
            .long_context_threshold_tokens,
        Some(123)
    );
    let legacy = build_table(
        r#"{"legacy": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6, "input_cost_per_token_above_200k_tokens": 3e-6}}"#,
        None,
        None,
    );
    assert_eq!(
        legacy
            .exact("legacy")
            .unwrap()
            .long_context_threshold_tokens,
        None
    );
    let usage = Usage {
        input: 200_001,
        ..Default::default()
    };
    assert!((legacy.cost("legacy", usage).unwrap() - (200_000.0 * 1e-6 + 3e-6)).abs() < 1e-12);
}

#[test]
fn unknown_prefixes_and_suffixes_never_inherit_a_price() {
    use fabrials_pricing::cost::list_cost_usd_with;
    use fabrials_types::HopRecord;

    let table = build_table(embedded_json(), None, None);
    for model in [
        "gpt",
        "gpt-6",
        "gpt-6.1",
        "gpt-6.1-sol-unknown",
        "gpt-6.1-sol-fast-unpublished",
        "gpt-6.1-sol-20260930-extra",
        "gpt-6.1-sol-2026-9-30",
        "gpt-6.1-sol-20261301",
        "gpt-6.1-sol-2026-02-30",
        "gpt-6.1-sol-2026093",
        "gpt-6.1-sol-202609300",
        "gpt-6.1-sol20260930",
        "openai/gpt-6.1-sol:unknown",
        "gpt-6.1-sol-２０２６０９３０",
    ] {
        assert!(table.find(model).is_none(), "{model}");
        assert!(table.find(model).is_none(), "memoized {model}");
        assert!(
            table
                .cost(
                    model,
                    Usage {
                        input: 100,
                        ..Default::default()
                    }
                )
                .is_none(),
            "{model}"
        );
        let record = HopRecord {
            model: Some(model.into()),
            input_tokens: 100,
            ..Default::default()
        };
        assert!(list_cost_usd_with(&record, &table).is_none(), "{model}");
    }
    for model in ["gpt-6.1-sol-20260930", "openai/GPT-6.1-SOL@2026-09-30"] {
        assert_eq!(table.find(model).unwrap().input, 2e-6);
    }
}

#[test]
fn aliases_require_explicit_entries_and_dated_rows_do_not_price_their_base() {
    let rows = r#"{
        "acme-model-2026-09-30": {"input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6},
        "acme-model-fast": {"input_cost_per_token": 3e-6, "output_cost_per_token": 4e-6},
        "acme-alias": {"input_cost_per_token": 3e-6, "output_cost_per_token": 4e-6}
    }"#;
    let table = build_table(rows, None, None);
    for model in ["acme", "acme-model", "acme-model-f", "acme-alias-extra"] {
        assert!(table.find(model).is_none(), "{model}");
    }
    assert_eq!(table.find("provider/ACME-ALIAS").unwrap().input, 3e-6);
    assert_eq!(table.find("acme-model-fast").unwrap().input, 3e-6);
    assert_eq!(table.find("acme-model-2026-09-30").unwrap().input, 1e-6);
}
