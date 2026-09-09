use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const REFERENCE_REVISION: &str = "0d621cae343af9b057fb4a38c84d089524ac4378";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientDefinition {
    pub id: String,
    pub name: String,
    pub root: String,
    pub environment: Option<String>,
    pub fallback: Option<String>,
    pub relative: String,
    pub pattern: String,
}

impl ClientDefinition {
    pub fn remote_collection(&self) -> bool {
        matches!(self.id.as_str(), "cursor" | "antigravity" | "trae" | "warp")
    }
    pub fn aggregate_only(&self) -> bool {
        matches!(
            self.id.as_str(),
            "droid" | "crush" | "mux" | "warp" | "fx" | "trae"
        )
    }
}

pub fn clients() -> &'static [ClientDefinition] {
    static CLIENTS: OnceLock<Vec<ClientDefinition>> = OnceLock::new();
    CLIENTS.get_or_init(|| {
        serde_json::from_str(include_str!("catalog.json")).expect("embedded usage catalog")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_catalog_has_unique_reader_entries() {
        let ids: std::collections::HashSet<_> = clients().iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids.len(), 53);
        assert_eq!(ids.len(), clients().len());
        for client in clients() {
            assert!(
                super::super::files::has_reader(&client.id),
                "missing {}",
                client.id
            );
        }
    }
}
