//! Tauri commands exposed to the React UI. See spec §10.
//!
//! Commands are thin adapters: they pull dependencies from managed state and
//! delegate to tested use cases.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};

use crate::{
    accessibility::{self, Accessibility},
    platform::os_version,
};

type AccessibilityState<'a> = State<'a, Arc<dyn Accessibility>>;

#[derive(Debug, Clone, Serialize, Type)]
pub struct Status {
    pub app_version: String,
    pub os_version: os_version::OsVersion,
    pub accessibility_trusted: bool,
}

#[tauri::command]
#[specta::specta]
pub fn get_status(app: AppHandle, ax: AccessibilityState<'_>) -> Status {
    Status {
        app_version: app.package_info().version.to_string(),
        os_version: os_version::current(),
        accessibility_trusted: ax.is_trusted(),
    }
}

/// Shows the system prompt (first time only) and opens the Accessibility pane.
#[tauri::command]
#[specta::specta]
pub fn request_accessibility(ax: AccessibilityState<'_>) -> Result<(), String> {
    accessibility::request_access(ax.as_ref())
}

/// Clears a stale TCC entry, then asks again.
#[tauri::command]
#[specta::specta]
pub fn reset_accessibility(app: AppHandle, ax: AccessibilityState<'_>) -> Result<(), String> {
    let bundle_id = &app.config().identifier;
    tracing::info!(bundle_id, "resetting Accessibility permission");
    accessibility::reset_access(ax.as_ref(), bundle_id)
}
