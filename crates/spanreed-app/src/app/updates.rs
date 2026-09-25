//! Self-update from GitHub Releases.

pub use crate::self_update::{
    CheckResult, DEFAULT_REPO, apply_blocked_reason, apply_update, can_apply_self_update,
    check_for_update, current_version,
};

pub fn offline() -> bool {
    crate::product::env_offline()
}
