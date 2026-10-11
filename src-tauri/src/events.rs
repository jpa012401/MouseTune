//! Events emitted from Rust to the React UI. See spec §10.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

use crate::permission_monitor::PermissionObserver;

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct PermissionChanged {
    pub accessibility_trusted: bool,
}

/// Forwards permission changes to the UI.
pub struct EmitPermissionChanged(pub AppHandle);

impl PermissionObserver for EmitPermissionChanged {
    fn permission_changed(&self, trusted: bool) {
        let event = PermissionChanged {
            accessibility_trusted: trusted,
        };
        if let Err(err) = event.emit(&self.0) {
            tracing::warn!(%err, "failed to emit permission-changed");
        }
    }
}
