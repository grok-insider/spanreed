use crate::{accounts, model::ProviderOutput};

pub fn probe_accounts() -> Vec<ProviderOutput> {
    accounts::list_provider("codex")
        .into_iter()
        .map(|account| {
            probe(&account).unwrap_or_else(|error| {
                ProviderOutput::error(&account.id, &format!("Codex ({})", account.alias), error)
            })
        })
        .collect()
}
fn probe(account: &accounts::Account) -> Result<ProviderOutput, String> {
    super::oauth::token("codex", &account.alias)?;
    let document = accounts::read_secret_document("codex", &account.alias)?
        .ok_or("Codex authorization missing")?;
    let client = fabrials_providers::codex::auth::Client::new()?;
    let usage = client.get("/backend-api/wham/usage", &document)?;
    let now = crate::util::now_ms();
    let mut output = fabrials_providers::codex::quota_output(&usage, &account.id, now);
    output.display_name = format!("Codex ({})", account.alias);
    let resets = client
        .get("/backend-api/wham/rate-limit-reset-credits", &document)
        .and_then(|value| {
            fabrials_providers::codex::parse_resets(&value, now).map_err(str::to_owned)
        });
    output.reset_inventory = Some(fabrials_core::Observation {
        availability: if resets.is_ok() {
            fabrials_core::Availability::Available
        } else {
            fabrials_core::Availability::Unavailable
        },
        freshness: fabrials_core::Freshness::Fresh,
        observed_at_ms: Some(now),
        source: "codex-reset-inventory".into(),
        value: resets.as_ref().ok().cloned(),
        error: resets
            .err()
            .map(|_| "Could not refresh reset inventory".into()),
    });
    accounts::apply_codex_snapshot(account, &document, &output, now)?;
    Ok(output)
}
