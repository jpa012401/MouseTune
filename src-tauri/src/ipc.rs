//! Tauri commands exposed to the React UI. See spec §10.

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;

use crate::platform::{os_version, permissions};

#[derive(Debug, Clone, Serialize, Type)]
pub struct Status {
    pub app_version: String,
    pub os_version: os_version::OsVersion,
    pub accessibility_trusted: bool,
}

#[tauri::command]
#[specta::specta]
pub fn get_status(app: AppHandle) -> Status {
    Status {
        app_version: app.package_info().version.to_string(),
        os_version: os_version::current(),
        accessibility_trusted: permissions::is_accessibility_trusted(),
    }
}

/// Shows the system prompt (first time only) and opens the Accessibility pane.
#[tauri::command]
#[specta::specta]
pub fn request_accessibility() -> Result<(), String> {
    if permissions::prompt_for_accessibility() {
        return Ok(());
    }
    permissions::open_accessibility_settings().map_err(|err| err.to_string())
}

/// Clears a stale TCC entry, then asks again.
#[tauri::command]
#[specta::specta]
pub fn reset_accessibility(app: AppHandle) -> Result<(), String> {
    let bundle_id = &app.config().identifier;
    permissions::reset_accessibility(bundle_id).map_err(|err| err.to_string())?;
    tracing::info!(bundle_id, "reset Accessibility permission");
    request_accessibility()
}
