//! Events emitted from Rust to the React UI. See spec §10.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

use crate::{
    actions::{ActionStats, KeyCapture, NoticeObserver},
    config::{
        schema::{Config, KeyModifier},
        store::ConfigObserver,
    },
    input::{
        engine::{EngineObserver, EngineSnapshot, ScrollStats},
        taps::TapSet,
    },
    ipc::{Device, input_status},
    permission_monitor::PermissionObserver,
};

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

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct InputStatusChanged {
    pub taps: TapSet,
    pub devices: Vec<Device>,
}

/// Forwards input-engine changes to the UI.
pub struct EmitInputStatus(pub AppHandle);

impl EngineObserver for EmitInputStatus {
    fn snapshot_changed(&self, snapshot: &EngineSnapshot) {
        let status = input_status(
            snapshot,
            ScrollStats::default(),
            ActionStats::default(),
            false,
        );
        let event = InputStatusChanged {
            taps: status.taps,
            devices: status.devices,
        };
        if let Err(err) = event.emit(&self.0) {
            tracing::warn!(%err, "failed to emit input-status-changed");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct ConfigChanged(pub Config);

/// Forwards saved settings to the UI.
pub struct EmitConfigChanged(pub AppHandle);

impl ConfigObserver for EmitConfigChanged {
    fn config_changed(&self, config: &Config) {
        if let Err(err) = ConfigChanged(config.clone()).emit(&self.0) {
            tracing::warn!(%err, "failed to emit config-changed");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct ButtonCaptured {
    pub button: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct KeyCaptured {
    pub key_code: u16,
    pub modifiers: Vec<KeyModifier>,
    pub key_label: String,
}

impl From<KeyCapture> for KeyCaptured {
    fn from(capture: KeyCapture) -> Self {
        Self {
            key_code: capture.key_code,
            modifiers: capture.modifiers,
            key_label: capture.key_label,
        }
    }
}

/// No fields, so it serializes as `{}`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct KeyCaptureCancelled {}

/// Forwards capture results to the UI.
pub struct EmitNotices(pub AppHandle);

impl NoticeObserver for EmitNotices {
    fn button_captured(&self, button: u8) {
        if let Err(err) = (ButtonCaptured { button }).emit(&self.0) {
            tracing::warn!(%err, "failed to emit button-captured");
        }
    }
    fn key_captured(&self, capture: KeyCapture) {
        if let Err(err) = KeyCaptured::from(capture).emit(&self.0) {
            tracing::warn!(%err, "failed to emit key-captured");
        }
    }
    fn key_capture_cancelled(&self) {
        if let Err(err) = (KeyCaptureCancelled {}).emit(&self.0) {
            tracing::warn!(%err, "failed to emit key-capture-cancelled");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_capture_becomes_the_event_unchanged() {
        let capture = KeyCapture {
            key_code: 21,
            modifiers: vec![KeyModifier::Shift, KeyModifier::Command],
            key_label: "4".into(),
        };
        let event = KeyCaptured::from(capture);
        assert_eq!(
            (event.key_code, event.modifiers, event.key_label.as_str()),
            (21, vec![KeyModifier::Shift, KeyModifier::Command], "4")
        );
    }
}
