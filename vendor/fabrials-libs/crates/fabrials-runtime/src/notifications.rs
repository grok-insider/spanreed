//! Durable delivery claims. Transport acknowledgement is separate from claiming.
use rusqlite::params;
use std::path::Path;

pub struct DeliveryStore(rusqlite::Connection);
pub struct Claim {
    namespace: String,
    key: String,
    token: String,
}
impl DeliveryStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = crate::database::open(path)?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS notification_deliveries(namespace TEXT NOT NULL, event_key TEXT NOT NULL, expires_at_ms INTEGER NOT NULL, lease_until_ms INTEGER NOT NULL, claim_id TEXT NOT NULL, delivered INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(namespace,event_key));")
            .map_err(|_| "Notification history unavailable")?;
        Ok(Self(connection))
    }
    pub fn claim(
        &self,
        namespace: &str,
        key: &str,
        now: i64,
        expires: i64,
    ) -> Result<Option<Claim>, String> {
        if namespace.is_empty()
            || namespace.len() > 256
            || key.is_empty()
            || key.len() > 512
            || expires <= now
            || expires.saturating_sub(now) > 86_400_000
        {
            return Err("Invalid notification event".into());
        }
        self.0
            .execute(
                "DELETE FROM notification_deliveries WHERE expires_at_ms<=?1",
                [now],
            )
            .map_err(|_| "Notification history cleanup failed")?;
        let token = crate::accounting::new_request_id();
        let changed = self.0.execute("INSERT INTO notification_deliveries(namespace,event_key,expires_at_ms,lease_until_ms,claim_id) VALUES(?1,?2,?3,?4,?6) ON CONFLICT(namespace,event_key) DO UPDATE SET lease_until_ms=excluded.lease_until_ms,claim_id=excluded.claim_id WHERE notification_deliveries.delivered=0 AND notification_deliveries.lease_until_ms<=?5", params![namespace,key,expires,now.saturating_add(60_000),now,token])
            .map_err(|_| "Notification claim failed")?;
        Ok((changed == 1).then(|| Claim {
            namespace: namespace.into(),
            key: key.into(),
            token,
        }))
    }
    pub fn acknowledge(&self, claim: &Claim) -> Result<(), String> {
        let changed = self.0.execute("UPDATE notification_deliveries SET delivered=1 WHERE namespace=?1 AND event_key=?2 AND claim_id=?3", params![claim.namespace,claim.key,claim.token])
            .map_err(|_| "Notification acknowledgement failed")?;
        if changed != 1 {
            return Err("Notification claim was superseded".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn claims_survive_restart_and_retry_only_without_acknowledgement() {
        let root = std::env::temp_dir().join(format!(
            "fabrials-notify-{}",
            crate::accounting::new_request_id()
        ));
        let path = root.join("runtime.sqlite3");
        let first = DeliveryStore::open(&path).unwrap();
        let second = DeliveryStore::open(&path).unwrap();
        let original = first.claim("one", "credit", 0, 100_000).unwrap().unwrap();
        assert!(second.claim("one", "credit", 1, 100_000).unwrap().is_none());
        assert!(second.claim("two", "credit", 1, 100_000).unwrap().is_some());
        drop(first);
        drop(second);
        let reopened = DeliveryStore::open(&path).unwrap();
        let retry = reopened
            .claim("one", "credit", 60_000, 100_000)
            .unwrap()
            .unwrap();
        assert!(reopened.acknowledge(&original).is_err());
        reopened.acknowledge(&retry).unwrap();
        assert!(reopened
            .claim("one", "credit", 90_000, 100_000)
            .unwrap()
            .is_none());
        assert!(reopened.claim("one", "expired", 100_000, 100_000).is_err());
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
