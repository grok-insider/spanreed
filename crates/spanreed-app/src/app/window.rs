//! Hand-off between front ends and the desktop window (routes and alerts).

pub use crate::desktop_open::{
    PendingAlert, ack_alert, hand_off_alert, mark_running, peek, request as open,
    route_location_script, take, take_alert, unmark_running,
};
