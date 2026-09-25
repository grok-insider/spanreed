//! The account registry as one JSON file (`<data>/accounts/index.json`).
use fabrials_accounts::Registry;
use std::fs;
use std::path::{Path, PathBuf};

pub fn load(path: &Path) -> Registry {
    let Ok(raw) = fs::read_to_string(path) else {
        return Registry::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(path: &Path, reg: &Registry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(reg).map_err(|e| e.to_string())?;
    fs::write(path, body).map_err(|e| e.to_string())
}

pub fn index_path(data_dir: &Path) -> PathBuf {
    data_dir.join("accounts").join("index.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_registry_loads_back_and_a_missing_one_is_empty() {
        let dir = std::env::temp_dir().join(format!(
            "fabrials-accounts-{}",
            fabrials_fabric::accounting::new_request_id()
        ));
        let path = index_path(&dir);
        assert_eq!(load(&path), Registry::default());
        let mut registry = Registry::default();
        registry.upsert(fabrials_accounts::Account::new("grok", "work").unwrap());
        save(&path, &registry).unwrap();
        assert_eq!(load(&path), registry);
        let _ = std::fs::remove_dir_all(dir);
    }
}
