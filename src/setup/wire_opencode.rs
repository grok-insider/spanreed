//! Patch OpenCode global config so xAI traffic hits the capture proxy.

use std::path::Path;

use super::detect::{self, Detection};
use super::paths;
use super::state::{OpenCodeWireState, SetupState};
use super::OPENCODE_XAI_CAPTURE_BASE_URL;

/// True when OpenCode xAI baseURL points at the local capture proxy.
pub fn is_wired_to_capture(det: &Detection, state: &SetupState) -> bool {
    let path = det.opencode.config_path.clone().or_else(|| {
        state
            .wired
            .opencode_xai
            .as_ref()
            .map(|w| std::path::PathBuf::from(&w.path))
    });
    let Some(path) = path else {
        return state.wired.opencode_xai.is_some();
    };
    match read_xai_base_url(&path) {
        Some(url) => is_canonical_xai_capture_url(&url),
        None => false,
    }
}

pub fn status_line(det: &Detection, state: &SetupState) -> String {
    let path = det.opencode.config_path.clone().or_else(|| {
        state
            .wired
            .opencode_xai
            .as_ref()
            .map(|w| std::path::PathBuf::from(&w.path))
    });
    let Some(path) = path else {
        return "no config".into();
    };
    match read_xai_base_url(&path) {
        Some(url) if is_canonical_xai_capture_url(&url) => {
            format!("wired → {url}")
        }
        Some(url) if url.contains("127.0.0.1:18737") => {
            format!("legacy :18737 ({url}) — rewire to {OPENCODE_XAI_CAPTURE_BASE_URL}")
        }
        Some(url) => format!("baseURL={url} ({})", path.display()),
        None if path.exists() => format!("no xai baseURL ({})", path.display()),
        None => "config missing".into(),
    }
}

pub fn wire(dry_run: bool, state: &mut SetupState, det: &Detection) -> Result<String, String> {
    let path = detect::opencode_config_write_path(&det.opencode);
    let previous = read_xai_base_url(&path);

    if previous.as_deref() == Some(OPENCODE_XAI_CAPTURE_BASE_URL) {
        // Already pointing at capture. Keep prior previous_base_url if we have one;
        // otherwise mark as pre-existing so uninstall will not strip the URL.
        let prev = state
            .wired
            .opencode_xai
            .as_ref()
            .and_then(|w| w.previous_base_url.clone());
        state.wired.opencode_xai = Some(OpenCodeWireState {
            path: path.display().to_string(),
            // Sentinel: same as current means "was already capture before us".
            previous_base_url: prev.or_else(|| Some(OPENCODE_XAI_CAPTURE_BASE_URL.into())),
        });
        return Ok(format!("already wired ({})", path.display()));
    }

    if dry_run {
        return Ok(format!(
            "would set provider.xai.options.baseURL → {OPENCODE_XAI_CAPTURE_BASE_URL} in {}",
            path.display()
        ));
    }

    if path.exists() {
        let _ = paths::backup_file(&path)?;
    }

    let mut root = if path.exists() {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("read opencode.json: {e}"))?;
        serde_json::from_str::<serde_json::Value>(text.trim())
            .map_err(|e| format!("parse opencode.json: {e}"))?
    } else {
        serde_json::json!({
            "$schema": "https://opencode.ai/config.json"
        })
    };

    if !root.is_object() {
        return Err("opencode.json root is not an object".into());
    }

    let obj = root.as_object_mut().unwrap();
    let provider = obj
        .entry("provider")
        .or_insert_with(|| serde_json::json!({}));
    if !provider.is_object() {
        *provider = serde_json::json!({});
    }
    let provider_obj = provider.as_object_mut().unwrap();
    let xai = provider_obj
        .entry("xai")
        .or_insert_with(|| serde_json::json!({}));
    if !xai.is_object() {
        *xai = serde_json::json!({});
    }
    let xai_obj = xai.as_object_mut().unwrap();
    let options = xai_obj
        .entry("options")
        .or_insert_with(|| serde_json::json!({}));
    if !options.is_object() {
        *options = serde_json::json!({});
    }
    options.as_object_mut().unwrap().insert(
        "baseURL".into(),
        serde_json::Value::String(OPENCODE_XAI_CAPTURE_BASE_URL.into()),
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir opencode config: {e}"))?;
    }
    let out = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&path, out + "\n").map_err(|e| format!("write opencode.json: {e}"))?;

    state.wired.opencode_xai = Some(OpenCodeWireState {
        path: path.display().to_string(),
        previous_base_url: previous,
    });

    Ok(format!(
        "set baseURL → {OPENCODE_XAI_CAPTURE_BASE_URL} ({})",
        path.display()
    ))
}

pub fn unwire(dry_run: bool, state: &mut SetupState) -> Result<String, String> {
    let Some(wire) = state.wired.opencode_xai.clone() else {
        return Ok("nothing to unwire".into());
    };
    let path = std::path::PathBuf::from(&wire.path);
    if !path.exists() {
        state.wired.opencode_xai = None;
        return Ok("config already gone".into());
    }

    let current = read_xai_base_url(&path);
    if current.as_deref() != Some(OPENCODE_XAI_CAPTURE_BASE_URL) {
        state.wired.opencode_xai = None;
        return Ok(format!(
            "left baseURL unchanged ({})",
            current.unwrap_or_else(|| "unset".into())
        ));
    }

    // If the user already had capture URL before we ran setup, do not strip it.
    if wire.previous_base_url.as_deref() == Some(OPENCODE_XAI_CAPTURE_BASE_URL) {
        state.wired.opencode_xai = None;
        return Ok("left pre-existing capture baseURL in place".into());
    }

    if dry_run {
        return Ok(match &wire.previous_base_url {
            Some(p) => format!("would restore baseURL → {p}"),
            None => "would remove capture baseURL".into(),
        });
    }

    let _ = paths::backup_file(&path)?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?;
    let mut root: serde_json::Value =
        serde_json::from_str(text.trim()).map_err(|e| format!("parse: {e}"))?;

    if let Some(prev) = &wire.previous_base_url {
        set_xai_base_url(&mut root, Some(prev))?;
    } else {
        set_xai_base_url(&mut root, None)?;
    }

    let out = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&path, out + "\n").map_err(|e| format!("write: {e}"))?;
    state.wired.opencode_xai = None;
    Ok(match &wire.previous_base_url {
        Some(p) => format!("restored baseURL → {p}"),
        None => "removed capture baseURL".into(),
    })
}

fn is_canonical_xai_capture_url(url: &str) -> bool {
    url == OPENCODE_XAI_CAPTURE_BASE_URL || url.contains("127.0.0.1:18736/xai")
}

pub fn read_xai_base_url(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    v.pointer("/provider/xai/options/baseURL")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

fn set_xai_base_url(root: &mut serde_json::Value, url: Option<&str>) -> Result<(), String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "root not object".to_string())?;
    let provider = obj
        .entry("provider")
        .or_insert_with(|| serde_json::json!({}));
    let provider_obj = provider
        .as_object_mut()
        .ok_or_else(|| "provider not object".to_string())?;
    let xai = provider_obj
        .entry("xai")
        .or_insert_with(|| serde_json::json!({}));
    let xai_obj = xai
        .as_object_mut()
        .ok_or_else(|| "xai not object".to_string())?;
    let options = xai_obj
        .entry("options")
        .or_insert_with(|| serde_json::json!({}));
    let options_obj = options
        .as_object_mut()
        .ok_or_else(|| "options not object".to_string())?;
    match url {
        Some(u) => {
            options_obj.insert("baseURL".into(), serde_json::Value::String(u.into()));
        }
        None => {
            options_obj.remove("baseURL");
        }
    }
    Ok(())
}

/// Pure merge helper for tests.
#[cfg(test)]
pub fn merge_base_url_for_test(existing: &str) -> String {
    let mut root: serde_json::Value = serde_json::from_str(existing).unwrap();
    set_xai_base_url(&mut root, Some(OPENCODE_XAI_CAPTURE_BASE_URL)).unwrap();
    serde_json::to_string(&root).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_plugins() {
        let existing = r#"{
            "$schema": "https://opencode.ai/config.json",
            "plugin": ["foo"],
            "provider": {
                "xai": {
                    "models": { "grok-4": {} },
                    "options": { "baseURL": "https://api.x.ai/v1" }
                }
            }
        }"#;
        let out = merge_base_url_for_test(existing);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v.pointer("/provider/xai/options/baseURL")
                .and_then(|x| x.as_str()),
            Some(OPENCODE_XAI_CAPTURE_BASE_URL)
        );
        assert!(v.pointer("/provider/xai/models/grok-4").is_some());
        assert_eq!(v["plugin"][0], "foo");
    }

    #[test]
    fn canonical_url_is_fabric_xai_not_legacy_port() {
        assert!(is_canonical_xai_capture_url(OPENCODE_XAI_CAPTURE_BASE_URL));
        assert!(is_canonical_xai_capture_url(
            "http://127.0.0.1:18736/xai/v1/"
        ));
        assert!(!is_canonical_xai_capture_url("http://127.0.0.1:18737/v1"));
        assert!(!is_canonical_xai_capture_url("https://api.x.ai/v1"));
    }
}
