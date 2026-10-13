use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub host_id: String,
    pub public_key: String,
    pub private_key: Vec<u8>,
    pub label: String,
}

impl Drop for Config {
    fn drop(&mut self) {
        self.private_key.zeroize();
    }
}

pub fn load(directory: &Path) -> Result<Config, String> {
    read_config(directory, "remote.json", false)
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.trim() == label
        && label.encode_utf16().count() <= 100
        && !label.chars().any(char::is_control)
}

fn read_config(directory: &Path, name: &str, pending: bool) -> Result<Config, String> {
    let path = directory.join(name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| "remote_not_enabled")?;
    if !metadata.is_file() || metadata.len() > 8192 {
        return Err("invalid_remote_state".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("remote_state_not_private".into());
        }
    }
    let bytes =
        zeroize::Zeroizing::new(std::fs::read(path).map_err(|_| "remote_state_unreadable")?);
    let value: Config = serde_json::from_slice(&bytes).map_err(|_| "invalid_remote_state")?;
    if value.private_key.len() != 32
        || !(super::opaque(&value.host_id) || pending && value.host_id.is_empty())
        || value.public_key.len() != 64
        || !value.public_key.bytes().all(|b| b.is_ascii_hexdigit())
        || !valid_label(&value.label)
    {
        return Err("invalid_remote_state".into());
    }
    Ok(value)
}

pub fn save(directory: &Path, value: &Config) -> Result<(), String> {
    save_as(directory, "remote.json", value)
}

fn save_as(directory: &Path, name: &str, value: &Config) -> Result<(), String> {
    write_private_json(directory, name, value)
}

pub(crate) fn write_private_json(
    directory: &Path,
    name: &str,
    value: &impl Serialize,
) -> Result<(), String> {
    fabrials_agent_host::instance::ensure_private_directory(directory)
        .map_err(|_| "remote_state_not_private")?;
    let bytes =
        zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(|_| "invalid_remote_state")?);
    let path = directory.join(name);
    static NEXT_WRITE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let counter = NEXT_WRITE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "clock_unavailable")?
        .as_nanos();
    let temporary = directory.join(format!(
        ".{name}.{}.{time}.{counter}.new",
        std::process::id()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| "remote_state_write_failed")?;
    let result = (|| {
        #[cfg(windows)]
        fabrials_agent_host::win_acl::set_owner_only(&temporary)
            .map_err(|_| "remote_state_not_private")?;
        file.write_all(&bytes)
            .map_err(|_| "remote_state_write_failed")?;
        file.sync_all().map_err(|_| "remote_state_write_failed")?;
        drop(file);
        std::fs::rename(&temporary, &path).map_err(|_| "remote_state_write_failed")?;
        #[cfg(unix)]
        std::fs::File::open(directory)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| "remote_state_write_failed")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|value| format!("{value:02x}")).collect()
}

fn enrollment_lock(directory: &Path) -> Result<std::fs::File, String> {
    fabrials_agent_host::instance::ensure_private_directory(directory)
        .map_err(|_| "remote_state_not_private")?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let path = directory.join("remote-enrollment.lock");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() => return Err("enrollment_lock_unavailable".into()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("enrollment_lock_unavailable".into()),
    }
    let file = options
        .open(&path)
        .map_err(|_| "enrollment_lock_unavailable")?;
    #[cfg(windows)]
    fabrials_agent_host::win_acl::set_owner_only(&path).map_err(|_| "remote_state_not_private")?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| "enrollment_in_progress")?;
    Ok(file)
}

fn prepare_enrollment(directory: &Path, label: &str) -> Result<Config, String> {
    if directory.join("remote-enrollment.json").exists() {
        return read_config(directory, "remote-enrollment.json", true);
    }
    let key = fabrials_remote_noise::identity().map_err(|_| "identity_unavailable")?;
    let pending = Config {
        enabled: false,
        host_id: String::new(),
        public_key: hex(&key.public),
        private_key: key.private.to_vec(),
        label: label.into(),
    };
    save_as(directory, "remote-enrollment.json", &pending)?;
    Ok(pending)
}

pub async fn enable(
    directory: &Path,
    label: &str,
    credentials: &dyn super::Credentials,
) -> Result<Config, String> {
    let label = label.trim();
    if !valid_label(label) {
        return Err("invalid_host_label".into());
    }
    let _lock = enrollment_lock(directory)?;
    if directory.join("remote.json").exists() {
        return Err("remote_already_registered".into());
    }
    let mut config = prepare_enrollment(directory, label)?;
    let token = credentials.access_token().await?;
    let response = super::http_client()?
        .post(format!("{}/hosts", super::API))
        .bearer_auth(&*token)
        .json(&serde_json::json!({"label":config.label,"publicKey":config.public_key}))
        .send()
        .await
        .map_err(|_| "registration_unavailable")?;
    if !response.status().is_success() {
        return Err("registration_rejected_link_installation".into());
    }
    let body = super::bounded_json(response, 8192).await?;
    let id = body
        .pointer("/host/id")
        .and_then(serde_json::Value::as_str)
        .filter(|v| super::opaque(v))
        .ok_or("invalid_registration")?;
    if body
        .pointer("/host/publicKey")
        .and_then(serde_json::Value::as_str)
        != Some(config.public_key.as_str())
    {
        return Err("invalid_registration_identity".into());
    }
    config.enabled = true;
    config.host_id = id.into();
    save(directory, &config)?;
    let _ = std::fs::remove_file(directory.join("remote-enrollment.json"));
    Ok(config)
}

pub async fn disable(directory: &Path, credentials: &dyn super::Credentials) -> Result<(), String> {
    let _lock = enrollment_lock(directory)?;
    disable_locked(directory, credentials).await
}

async fn disable_locked(
    directory: &Path,
    credentials: &dyn super::Credentials,
) -> Result<(), String> {
    let mut config = load(directory)?;
    config.enabled = false;
    save(directory, &config)?;
    let token = credentials.access_token().await?;
    let response = super::http_client()?
        .delete(format!("{}/hosts", super::API))
        .bearer_auth(&*token)
        .json(&serde_json::json!({"hostId":config.host_id}))
        .send()
        .await
        .map_err(|_| "disabled_locally_retry_cloud_revocation")?;
    if !response.status().is_success() {
        return Err("disabled_locally_retry_cloud_revocation".into());
    }
    Ok(())
}

pub async fn reset(directory: &Path, credentials: &dyn super::Credentials) -> Result<(), String> {
    let _lock = enrollment_lock(directory)?;
    if load(directory)?.enabled {
        return Err("disable_remote_before_reset".into());
    }
    disable_locked(directory, credentials).await?;
    std::fs::remove_file(directory.join("remote.json")).map_err(|_| "remote_state_write_failed")?;
    match std::fs::remove_file(directory.join("remote-enrollment.json")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("remote_state_write_failed".into()),
    }
    #[cfg(unix)]
    std::fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| "remote_state_write_failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enrollment_operations_are_exclusive() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        let first = enrollment_lock(&directory).unwrap();
        assert_eq!(
            enrollment_lock(&directory).unwrap_err(),
            "enrollment_in_progress"
        );
        drop(first);
        assert!(enrollment_lock(&directory).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn enrollment_refuses_symlink_lock() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        fabrials_agent_host::instance::ensure_private_directory(&directory).unwrap();
        let outside = root.path().join("outside");
        std::fs::write(&outside, b"untouched").unwrap();
        std::os::unix::fs::symlink(&outside, directory.join("remote-enrollment.lock")).unwrap();
        assert_eq!(
            enrollment_lock(&directory).unwrap_err(),
            "enrollment_lock_unavailable"
        );
        assert_eq!(std::fs::read(outside).unwrap(), b"untouched");
    }

    #[test]
    fn private_state_roundtrip_and_corruption_closed() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        let value = Config {
            enabled: true,
            host_id: "host-1".into(),
            public_key: "00".repeat(32),
            private_key: vec![1; 32],
            label: "Station".into(),
        };
        save(&directory, &value).unwrap();
        assert_eq!(load(&directory).unwrap().host_id, "host-1");
        save(&directory, &value).unwrap();
        std::fs::write(directory.join("remote.json"), b"{}").unwrap();
        assert!(load(&directory).is_err());
    }

    #[test]
    fn pending_registration_preserves_key_after_lost_ack_and_stale_temp() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        let first = prepare_enrollment(&directory, "Station").unwrap();
        std::fs::write(directory.join("remote.json.new"), b"old interrupted write").unwrap();
        let second = prepare_enrollment(&directory, "Another label").unwrap();
        assert_eq!(first.private_key, second.private_key);
        assert_eq!(first.public_key, second.public_key);
        assert_eq!(second.label, "Station");
        let mut ready = second;
        ready.host_id = "host-1".into();
        ready.enabled = true;
        save(&directory, &ready).unwrap();
        assert_eq!(load(&directory).unwrap().private_key, first.private_key);
    }

    #[test]
    fn label_limit_matches_web_utf16_including_multibyte_and_astral() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        let mut config = prepare_enrollment(&directory, &"é".repeat(100)).unwrap();
        config.host_id = "host-1".into();
        save(&directory, &config).unwrap();
        assert_eq!(load(&directory).unwrap().label, "é".repeat(100));
        assert!(valid_label(&"🦀".repeat(50)));
        assert!(!valid_label(&"🦀".repeat(51)));
        assert!(!valid_label("line\nbreak"));
    }
}
