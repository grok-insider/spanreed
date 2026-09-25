//! Keeps rotated credential documents until the original generation can be replaced.
//! The caller owns locking and durable storage. Secrets deliberately have no Debug/Serialize.
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub struct PendingCredential<D> {
    pub expected: D,
    pub replacement: D,
    pub expires_at_ms: i64,
}

pub struct RecoveryQueue<K, D> {
    entries: HashMap<K, PendingCredential<D>>,
    capacity: usize,
    slots: HashSet<K>,
}

impl<K: Eq + Hash + Clone, D> RecoveryQueue<K, D> {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity,
            slots: HashSet::new(),
        }
    }
    pub fn insert(
        &mut self,
        key: K,
        value: PendingCredential<D>,
        _now_ms: i64,
    ) -> Result<(), &'static str> {
        if !self.reserve(key.clone()) {
            return Err("credential recovery queue full");
        }
        self.entries.insert(key, value);
        Ok(())
    }
    pub fn remove(&mut self, key: &K) -> Option<PendingCredential<D>> {
        self.slots.remove(key);
        self.entries.remove(key)
    }

    /// Reserve recovery capacity before redeeming a rotating grant.
    pub fn reserve(&mut self, key: K) -> bool {
        if self.slots.contains(&key) {
            return true;
        }
        if self.slots.len() >= self.capacity {
            return false;
        }
        self.slots.insert(key);
        true
    }

    pub fn take_reserved(&mut self, key: &K) -> Option<PendingCredential<D>> {
        self.entries.remove(key)
    }

    pub fn release_reservation(&mut self, key: &K) {
        if !self.entries.contains_key(key) {
            self.slots.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn in_flight_rotation_cannot_lose_recovery_capacity() {
        let mut queue = RecoveryQueue::new(1);
        assert!(queue.reserve("account-a"));
        assert!(!queue.reserve("account-b"));
        queue
            .insert(
                "account-a",
                PendingCredential {
                    expected: "old",
                    replacement: "new",
                    expires_at_ms: 10,
                },
                0,
            )
            .unwrap();
        let pending = queue.take_reserved(&"account-a").unwrap();
        assert!(!queue.reserve("account-b"));
        queue.insert("account-a", pending, 20).unwrap();
        queue.release_reservation(&"account-a");
        assert!(!queue.reserve("account-b"));
        queue.remove(&"account-a");
        assert!(queue.reserve("account-b"));
    }
    #[test]
    fn retains_both_generations_and_bounds_queue() {
        let mut queue = RecoveryQueue::new(1);
        queue
            .insert(
                "owner/a",
                PendingCredential {
                    expected: "old",
                    replacement: "rotated",
                    expires_at_ms: 20,
                },
                10,
            )
            .unwrap();
        assert!(queue
            .insert(
                "owner/b",
                PendingCredential {
                    expected: "b",
                    replacement: "new-b",
                    expires_at_ms: 30
                },
                10
            )
            .is_err());
        let pending = queue.remove(&"owner/a").unwrap();
        assert_eq!((pending.expected, pending.replacement), ("old", "rotated"));
        assert!(queue.remove(&"other-owner/a").is_none());
    }
}
