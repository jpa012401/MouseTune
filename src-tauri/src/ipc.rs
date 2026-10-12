//! Tauri commands exposed to the React UI. See spec §10.
//!
//! Commands are thin adapters: they pull dependencies from managed state and
//! delegate to tested use cases.

use std::sync::{Arc, Mutex, PoisonError, atomic::Ordering};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::{
    accessibility::{self, Accessibility},
    actions::ActionStats,
    config::{
        schema::Config,
        store::{ConfigError, ConfigStore, Recovery},
    },
    input::{
        engine::{EngineSnapshot, ScrollStats},
        taps::TapSet,
        thread::{Control, InputHandle},
    },
    platform::os_version,
};

type AccessibilityState<'a> = State<'a, Arc<dyn Accessibility>>;
// Commands using this must stay synchronous: observers (tray `set_checked`,
// `emit`) dispatch to the main thread while the store lock is held, which is
// only safe because sync commands and menu events already run there.
type ConfigState<'a> = State<'a, Mutex<ConfigStore>>;

#[derive(Debug, Clone, Serialize, Type)]
pub struct Status {
    pub app_version: String,
    pub os_version: os_version::OsVersion,
    pub accessibility_trusted: bool,
    pub input: InputStatus,
}

#[tauri::command]
#[specta::specta]
pub fn get_status(
    app: AppHandle,
    ax: AccessibilityState<'_>,
    input: State<'_, Arc<InputHandle>>,
) -> Status {
    let shared = input.shared();
    Status {
        app_version: app.package_info().version.to_string(),
        os_version: os_version::current(),
        accessibility_trusted: ax.is_trusted(),
        input: input_status(
            &shared.snapshot.load(),
            shared.counters.stats(),
            shared.actions.stats(),
            shared.degraded.load(Ordering::Relaxed),
        ),
    }
}

/// A tracked mouse as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Device {
    /// IORegistry entry ID in hex; stable while the device stays connected.
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub button_count: u8,
    pub has_wheel: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct InputStatus {
    pub taps: TapSet,
    pub devices: Vec<Device>,
    pub scroll_stats: ScrollStats,
    pub actions: ActionStats,
    pub degraded: bool,
}

pub fn input_status(
    snapshot: &EngineSnapshot,
    stats: ScrollStats,
    actions: ActionStats,
    degraded: bool,
) -> InputStatus {
    InputStatus {
        taps: snapshot.taps,
        devices: snapshot
            .devices
            .iter()
            .map(|d| Device {
                id: format!("{:x}", d.id),
                name: d.name.clone(),
                manufacturer: d.manufacturer.clone(),
                button_count: d.button_count,
                has_wheel: d.has_wheel,
            })
            .collect(),
        scroll_stats: stats,
        actions,
        degraded,
    }
}

/// Settings plus any startup recovery the UI should mention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ConfigView {
    pub config: Config,
    pub recovery: Option<RecoveryNotice>,
}

/// The settings file was unusable at startup and defaults were loaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RecoveryNotice {
    /// File name of the kept copy of the unusable file; absent when no copy could be kept.
    pub backup_file: Option<String>,
}

pub fn config_view(config: &Config, recovery: Option<&Recovery>) -> ConfigView {
    ConfigView {
        config: config.clone(),
        recovery: recovery.map(|r| RecoveryNotice {
            backup_file: match r {
                Recovery::BackedUp { file_name } => Some(file_name.clone()),
                Recovery::BackupFailed => None,
            },
        }),
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_config(store: ConfigState<'_>) -> ConfigView {
    let store = store.lock().unwrap_or_else(PoisonError::into_inner);
    config_view(store.current(), store.recovery())
}

/// Saves the settings and returns what was saved.
#[tauri::command]
#[specta::specta]
pub fn set_config(store: ConfigState<'_>, config: Config) -> Result<Config, ConfigError> {
    store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .update(config)
}

/// Saves the default settings and returns them.
#[tauri::command]
#[specta::specta]
pub fn reset_config(store: ConfigState<'_>) -> Result<Config, ConfigError> {
    store.lock().unwrap_or_else(PoisonError::into_inner).reset()
}

/// The user has seen the startup recovery notice.
#[tauri::command]
#[specta::specta]
pub fn dismiss_config_recovery(store: ConfigState<'_>) {
    store
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .dismiss_recovery();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        input::{
            engine::{EngineSnapshot, ScrollStats},
            taps::TapSet,
        },
        test_support::mouse,
    };

    #[test]
    fn config_view_reports_a_kept_backup() {
        let view = config_view(
            &Config::default(),
            Some(&Recovery::BackedUp {
                file_name: "config.1.bak.json".into(),
            }),
        );
        assert_eq!(
            view.recovery,
            Some(RecoveryNotice {
                backup_file: Some("config.1.bak.json".into())
            })
        );
        assert_eq!(view.config, Config::default());
    }

    #[test]
    fn config_view_reports_a_failed_backup() {
        let view = config_view(&Config::default(), Some(&Recovery::BackupFailed));
        assert_eq!(view.recovery, Some(RecoveryNotice { backup_file: None }));
    }

    #[test]
    fn config_view_without_recovery_has_no_notice() {
        assert_eq!(config_view(&Config::default(), None).recovery, None);
    }

    #[test]
    fn input_status_carries_the_action_counters() {
        let stats = ActionStats {
            performed: 3,
            dropped: 1,
            failed: 2,
        };
        let status = input_status(
            &EngineSnapshot::default(),
            ScrollStats::default(),
            stats,
            false,
        );
        assert_eq!(status.actions, stats);
    }

    #[test]
    fn input_status_exposes_devices_without_internal_ids() {
        let snapshot = EngineSnapshot {
            trusted: true,
            taps: TapSet {
                buttons: true,
                scroll: true,
                flags: false,
                keys: false,
            },
            devices: vec![mouse(255)],
        };
        let status = input_status(
            &snapshot,
            ScrollStats {
                mouse_wheel: 3,
                passed_through: 9,
            },
            ActionStats::default(),
            true,
        );
        assert_eq!(
            status.taps,
            TapSet {
                buttons: true,
                scroll: true,
                flags: false,
                keys: false
            }
        );
        assert_eq!(
            status.scroll_stats,
            ScrollStats {
                mouse_wheel: 3,
                passed_through: 9
            }
        );
        assert!(status.degraded);
        assert_eq!(status.devices.len(), 1);
        let d = &status.devices[0];
        assert_eq!(
            (
                d.id.as_str(),
                d.name.as_str(),
                d.manufacturer.as_str(),
                d.button_count,
                d.has_wheel
            ),
            ("ff", "MX Master 3", "Logitech", 5, true)
        );
    }
}

/// Report the next press of button 3 or higher as a `button-captured` event.
#[tauri::command]
#[specta::specta]
pub fn start_capture(input: State<'_, Arc<InputHandle>>) {
    input.send(Control::StartCapture);
}

#[tauri::command]
#[specta::specta]
pub fn stop_capture(input: State<'_, Arc<InputHandle>>) {
    input.send(Control::StopCapture);
}

/// Record the next key-down as a `key-captured` event.
#[tauri::command]
#[specta::specta]
pub fn start_key_capture(input: State<'_, Arc<InputHandle>>) {
    input.send(Control::StartKeyCapture);
}

#[tauri::command]
#[specta::specta]
pub fn stop_key_capture(input: State<'_, Arc<InputHandle>>) {
    input.send(Control::StopKeyCapture);
}
