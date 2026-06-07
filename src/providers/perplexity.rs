//! Perplexity provider.
//!
//! Upstream reads the macOS Perplexity app's CFNetwork `Cache.db` to extract a
//! bearer token. There is no Perplexity desktop app on Linux and therefore no
//! local session cache to read, so this provider never detects on Linux. It is
//! kept registered for parity and to give a clear message if force-probed.

use crate::model::ProviderOutput;
use crate::providers::Provider;

const ID: &str = "perplexity";
const NAME: &str = "Perplexity";

pub struct Perplexity;

impl Provider for Perplexity {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }

    fn detect(&self) -> bool {
        // No Linux desktop app / local session cache.
        false
    }

    fn probe(&self) -> ProviderOutput {
        ProviderOutput::error(
            ID,
            NAME,
            "Perplexity is macOS-only (reads the desktop app cache); not available on Linux.",
        )
    }
}
