//! Host-neutral device authorization state. Provider credentials never enter its view.
use serde_json::Value;

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub verification_uri: String,
    pub user_code: String,
    pub expires_in: u64,
    pub interval: u64,
}
pub struct DeviceAuthorization {
    pub view: DeviceView,
    pub device_code: String,
}
pub enum PollResult {
    Pending,
    SlowDown,
    Authorized(Value),
}

pub struct DeviceFlow {
    authorization: DeviceAuthorization,
    deadline_ms: i64,
    next_poll_ms: i64,
    interval: u64,
    authorized: Option<Value>,
    connected: bool,
}

#[cfg_attr(feature = "contracts", derive(ts_rs::TS))]
#[derive(serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Progress {
    Pending {
        #[serde(rename = "retryAfterSecs")]
        retry_after_secs: u64,
    },
    Connected,
}

impl DeviceFlow {
    pub fn new(authorization: DeviceAuthorization, now_ms: i64) -> Self {
        let interval = authorization.view.interval.clamp(1, 60);
        Self {
            deadline_ms: now_ms
                .saturating_add(authorization.view.expires_in.min(3600) as i64 * 1000),
            next_poll_ms: now_ms.saturating_add(interval as i64 * 1000),
            interval,
            authorization,
            authorized: None,
            connected: false,
        }
    }

    pub fn view(&self) -> DeviceView {
        self.authorization.view.clone()
    }

    pub fn expired(&self, now_ms: i64) -> bool {
        !self.connected && self.authorized.is_none() && now_ms >= self.deadline_ms
    }

    pub fn advance(
        &mut self,
        now_ms: i64,
        poll: impl FnOnce(&str) -> Result<PollResult, String>,
        persist: impl FnOnce(&Value) -> Result<(), String>,
    ) -> Result<Progress, String> {
        if self.connected {
            return Ok(Progress::Connected);
        }
        if self.expired(now_ms) {
            return Err("Authorization expired; start again".into());
        }
        if self.authorized.is_none() {
            if now_ms < self.next_poll_ms {
                return Ok(Progress::Pending {
                    retry_after_secs: ((self.next_poll_ms - now_ms) as u64).div_ceil(1000),
                });
            }
            self.next_poll_ms = now_ms.saturating_add(self.interval as i64 * 1000);
            match poll(&self.authorization.device_code)? {
                PollResult::Pending => {}
                PollResult::SlowDown => {
                    self.interval = (self.interval + 5).min(60);
                    self.next_poll_ms = now_ms.saturating_add(self.interval as i64 * 1000);
                }
                PollResult::Authorized(document) => self.authorized = Some(document),
            }
        }
        if let Some(document) = &self.authorized {
            persist(document)?;
            self.authorized = None;
            self.authorization.device_code.clear();
            self.connected = true;
            return Ok(Progress::Connected);
        }
        Ok(Progress::Pending {
            retry_after_secs: self.interval,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn flow() -> DeviceFlow {
        DeviceFlow::new(
            DeviceAuthorization {
                device_code: "private-device-fixture".into(),
                view: DeviceView {
                    verification_uri: "https://portal.nousresearch.com".into(),
                    user_code: "PUBLIC-CODE".into(),
                    expires_in: 60,
                    interval: 5,
                },
            },
            0,
        )
    }
    #[test]
    fn persistence_retry_does_not_redeem_the_device_grant_again() {
        let mut flow = flow();
        assert!(flow
            .advance(
                5000,
                |_| Ok(PollResult::Authorized(
                    serde_json::json!({"access_token":"private-fixture"})
                )),
                |_| Err("disk unavailable".into())
            )
            .is_err());
        assert!(matches!(
            flow.advance(
                70_000,
                |_| panic!("must not redeem again"),
                |document| {
                    assert_eq!(document["access_token"], "private-fixture");
                    Ok(())
                }
            )
            .unwrap(),
            Progress::Connected
        ));
        assert!(matches!(
            flow.advance(80_000, |_| panic!(), |_| panic!()).unwrap(),
            Progress::Connected
        ));
        assert!(!serde_json::to_string(&flow.view())
            .unwrap()
            .contains("private"));
    }
    #[test]
    fn polling_obeys_provider_interval_slowdown_and_expiry() {
        let mut flow = flow();
        assert!(matches!(
            flow.advance(0, |_| panic!(), |_| panic!()).unwrap(),
            Progress::Pending {
                retry_after_secs: 5
            }
        ));
        assert!(matches!(
            flow.advance(5000, |_| Ok(PollResult::SlowDown), |_| panic!())
                .unwrap(),
            Progress::Pending {
                retry_after_secs: 10
            }
        ));
        assert!(matches!(
            flow.advance(10_000, |_| panic!(), |_| panic!()).unwrap(),
            Progress::Pending {
                retry_after_secs: 5
            }
        ));
        assert!(flow.advance(60_000, |_| panic!(), |_| panic!()).is_err());
    }
}
