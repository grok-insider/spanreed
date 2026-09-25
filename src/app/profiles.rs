//! Built-in status-bar fragments (Waybar, Eww, SketchyBar) and the panel.

pub use crate::profiles::{files, install};

pub fn panel() -> serde_json::Value {
    crate::panel::snapshot()
}
