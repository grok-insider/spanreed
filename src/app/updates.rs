//! Self-update from GitHub Releases.

pub use crate::self_update::{
    CheckResult, apply_blocked_reason, can_apply_self_update, check_for_update, current_version,
};

pub fn offline() -> bool {
    crate::product::env_offline()
}
