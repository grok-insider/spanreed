//! Single-flight refresh coordinator. The host supplies I/O and persistence.
use std::sync::Mutex;
use std::time::{Duration, Instant};
pub mod history;
pub mod local_usage;

pub struct Snapshot<T> {
    state: Mutex<Option<(Instant, T)>>,
    ttl: Duration,
}
impl<T: Clone> Snapshot<T> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            state: Mutex::new(None),
            ttl,
        }
    }
    pub fn read(&self) -> Option<T> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|(_, value)| value.clone())
    }
    pub fn refresh(&self, force: bool, fetch: impl FnOnce() -> T) -> T {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if !force {
            if let Some((time, value)) = &*state {
                if time.elapsed() < self.ttl {
                    return value.clone();
                }
            }
        }
        let value = fetch();
        *state = Some((Instant::now(), value.clone()));
        value
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simultaneous_readers_fetch_once() {
        let cache = std::sync::Arc::new(Snapshot::new(Duration::from_secs(60)));
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let cache = cache.clone();
                let calls = calls.clone();
                scope.spawn(move || {
                    assert_eq!(
                        cache.refresh(false, || {
                            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            42
                        }),
                        42
                    );
                });
            }
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}

pub mod accounting;
pub mod files;
pub mod forward;
pub mod http;
pub mod listener;
pub mod models;
pub mod provider;
pub mod routes;
pub mod upstreams;
pub mod ws_tunnel;

mod database;
pub mod hops;

pub mod credential_journal;

pub mod file_set;

pub mod migration;
pub mod notifications;
