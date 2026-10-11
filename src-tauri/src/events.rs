//! Events emitted from Rust to the React UI. See spec §10.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct PermissionChanged {
    pub accessibility_trusted: bool,
}
