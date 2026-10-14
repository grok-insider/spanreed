use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Connection {
    Connecting,
    Connected,
    Retrying,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Failure {
    AccountUnavailable,
    RegistrationRejected,
    NetworkUnavailable,
    RelayRejected,
    LocalStateUnavailable,
    PublicationUnavailable,
}

impl Failure {
    pub(crate) fn connection(reason: &str) -> Self {
        match reason {
            "installation_login_unavailable" => Self::AccountUnavailable,
            "host_registration_revoked" => Self::RegistrationRejected,
            "relay_protocol" | "relay_origin_rejected" | "invalid_ticket" => Self::RelayRejected,
            "remote_not_enabled"
            | "invalid_remote_state"
            | "remote_state_unreadable"
            | "remote_state_not_private"
            | "remote_state_write_failed"
            | "remote_status_unavailable"
            | "remote_status_invalid"
            | "remote_status_not_private" => Self::LocalStateUnavailable,
            _ => Self::NetworkUnavailable,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeStatus {
    pub updated_at_ms: u64,
    pub connection: Connection,
    pub failure: Option<Failure>,
    pub publication_failure: Option<Failure>,
    pub last_publication_check_ms: Option<u64>,
}

impl RuntimeStatus {
    pub fn is_fresh(&self) -> bool {
        self.fresh_at(fabrials_agent_host::now_ms())
    }

    fn fresh_at(&self, now: u64) -> bool {
        now >= self.updated_at_ms && now - self.updated_at_ms <= 35_000
    }
}

pub fn read(directory: &Path) -> Result<Option<RuntimeStatus>, String> {
    let path = directory.join("remote-status.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("remote_status_unavailable".into()),
    };
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err("remote_status_invalid".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("remote_status_not_private".into());
        }
    }
    let bytes = std::fs::read(path).map_err(|_| "remote_status_unavailable")?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "remote_status_invalid".into())
}

pub(crate) struct Monitor {
    directory: PathBuf,
    value: Mutex<RuntimeStatus>,
}

impl Monitor {
    pub fn new(directory: &Path) -> Result<Self, String> {
        let value = RuntimeStatus {
            updated_at_ms: fabrials_agent_host::now_ms(),
            connection: Connection::Connecting,
            failure: None,
            publication_failure: None,
            last_publication_check_ms: None,
        };
        super::state::write_private_json(directory, "remote-status.json", &value)?;
        Ok(Self {
            directory: directory.into(),
            value: Mutex::new(value),
        })
    }

    pub async fn connection(
        &self,
        connection: Connection,
        failure: Option<Failure>,
    ) -> Result<(), String> {
        let mut value = self.value.lock().await;
        if value.connection != connection || value.failure != failure {
            eprintln!("spanreed remote connection: {connection:?}; failure: {failure:?}");
        }
        value.connection = connection;
        value.failure = failure;
        value.updated_at_ms = fabrials_agent_host::now_ms();
        super::state::write_private_json(&self.directory, "remote-status.json", &*value)
    }

    pub async fn publication(&self, failed: bool) -> Result<(), String> {
        let mut value = self.value.lock().await;
        let failure = failed.then_some(Failure::PublicationUnavailable);
        if value.publication_failure != failure {
            eprintln!("spanreed remote publication: {failure:?}");
        }
        value.publication_failure = failure;
        value.last_publication_check_ms = Some(fabrials_agent_host::now_ms());
        super::state::write_private_json(&self.directory, "remote-status.json", &*value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn diagnostics_report_connection_and_publication_independently() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        let monitor = Monitor::new(&directory).unwrap();
        monitor
            .connection(Connection::Connected, None)
            .await
            .unwrap();
        monitor.publication(true).await.unwrap();
        let value = read(&directory).unwrap().unwrap();
        assert_eq!(value.connection, Connection::Connected);
        assert_eq!(
            value.publication_failure,
            Some(Failure::PublicationUnavailable)
        );
        assert!(value.is_fresh());
        monitor
            .connection(Connection::Retrying, Some(Failure::AccountUnavailable))
            .await
            .unwrap();
        monitor.publication(false).await.unwrap();
        let value = read(&directory).unwrap().unwrap();
        assert_eq!(value.failure, Some(Failure::AccountUnavailable));
        assert_eq!(value.publication_failure, None);
        assert!(!value.fresh_at(value.updated_at_ms + 35_001));
        assert!(!value.fresh_at(value.updated_at_ms - 1));
    }

    #[test]
    fn diagnostic_codes_never_store_provider_error_text() {
        assert_eq!(
            Failure::connection("credentials with bearer secret"),
            Failure::NetworkUnavailable
        );
        assert_eq!(
            Failure::connection("installation_login_unavailable"),
            Failure::AccountUnavailable
        );
        assert_eq!(
            Failure::connection("remote_state_write_failed"),
            Failure::LocalStateUnavailable
        );
    }

    #[test]
    fn malformed_status_is_not_reported_as_connected() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        assert!(read(&directory).unwrap().is_none());
        super::super::state::write_private_json(
            &directory,
            "remote-status.json",
            &serde_json::json!({"connection":"connected"}),
        )
        .unwrap();
        assert!(read(&directory).is_err());
    }
}
