//! The settings file's shape. Every field is required so the UI can't send
//! partial settings; the loader fills what a file leaves out from the
//! defaults (`load::merge_defaults`), so later phases can add fields without
//! a version bump (design §1).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::input::taps::TapDemand;

/// Bump only for breaking changes, together with a step in `migrate.rs`.
pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Config {
    pub version: u32,
    pub general: General,
    pub buttons: Buttons,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            general: General::default(),
            buttons: Buttons::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct General {
    /// Kill switch for button features.
    pub buttons_enabled: bool,
    /// Kill switch for scroll features.
    pub scroll_enabled: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            buttons_enabled: true,
            scroll_enabled: true,
        }
    }
}

/// Button remaps (design §1). Button numbers are 1-based as users count
/// them: 3 is the middle button. `input::remaps::compile` turns this into
/// the engine's lookup tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Buttons {
    pub remaps: Vec<Remap>,
}

impl Default for Buttons {
    fn default() -> Self {
        use NavDirection::{Back, Forward};
        use SymbolicHotkeyId::{MissionControl, ShowDesktop};
        let remap = |button, duration, action| Remap {
            trigger: ClickTrigger {
                button,
                level: 1,
                duration,
            },
            effect: Effect { action },
        };
        Self {
            remaps: vec![
                remap(
                    3,
                    TriggerDuration::Click,
                    Action::SymbolicHotkey { id: MissionControl },
                ),
                remap(
                    3,
                    TriggerDuration::Hold,
                    Action::SymbolicHotkey { id: ShowDesktop },
                ),
                remap(
                    4,
                    TriggerDuration::Click,
                    Action::Navigate { direction: Back },
                ),
                remap(
                    5,
                    TriggerDuration::Click,
                    Action::Navigate { direction: Forward },
                ),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Remap {
    pub trigger: ClickTrigger,
    pub effect: Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ClickTrigger {
    /// 1-based: 3 = middle button.
    pub button: u8,
    /// 1 = single click, 2 = double, 3 = triple.
    pub level: u8,
    pub duration: TriggerDuration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum TriggerDuration {
    Click,
    Hold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Effect {
    pub action: Action,
}

/// What a remap does (spec §7.1, §9.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
pub enum Action {
    SymbolicHotkey {
        id: SymbolicHotkeyId,
    },
    Navigate {
        direction: NavDirection,
    },
    /// A keyboard shortcut to post. `key_label` is only for display.
    KeyCombo {
        key_code: u16,
        modifiers: Vec<KeyModifier>,
        key_label: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum KeyModifier {
    Command,
    Shift,
    Option,
    Control,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SymbolicHotkeyId {
    MissionControl,
    AppExpose,
    ShowDesktop,
    Launchpad,
    LookUp,
    Spotlight,
    NotificationCenter,
    SpaceLeft,
    SpaceRight,
    Desktop1,
    Desktop2,
    Desktop3,
    Desktop4,
    Desktop5,
    Desktop6,
    Desktop7,
    Desktop8,
    Desktop9,
    Desktop10,
    Desktop11,
    Desktop12,
    Desktop13,
    Desktop14,
    Desktop15,
    Desktop16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum NavDirection {
    Back,
    Forward,
}

/// Which event taps the settings ask for.
pub fn tap_demand(config: &Config) -> TapDemand {
    let g = &config.general;
    TapDemand {
        buttons: g.buttons_enabled,
        scroll: g.scroll_enabled,
        flags: g.buttons_enabled || g.scroll_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_from_the_ui_must_be_complete() {
        assert!(serde_json::from_value::<Config>(serde_json::json!({ "version": 1 })).is_err());
        assert!(
            serde_json::from_value::<Config>(
                serde_json::json!({ "version": 1, "general": { "scroll_enabled": false } })
            )
            .is_err()
        );
    }

    fn config(buttons: bool, scroll: bool) -> Config {
        Config {
            version: 1,
            general: General {
                buttons_enabled: buttons,
                scroll_enabled: scroll,
            },
            buttons: Buttons::default(),
        }
    }

    #[test]
    fn both_on_asks_for_every_tap() {
        assert_eq!(
            tap_demand(&config(true, true)),
            TapDemand {
                buttons: true,
                scroll: true,
                flags: true
            }
        );
    }

    #[test]
    fn scroll_off_drops_only_the_scroll_tap() {
        assert_eq!(
            tap_demand(&config(true, false)),
            TapDemand {
                buttons: true,
                scroll: false,
                flags: true
            }
        );
    }

    #[test]
    fn buttons_off_drops_only_the_button_tap() {
        assert_eq!(
            tap_demand(&config(false, true)),
            TapDemand {
                buttons: false,
                scroll: true,
                flags: true
            }
        );
    }

    #[test]
    fn both_off_asks_for_no_taps_at_all() {
        assert_eq!(
            tap_demand(&config(false, false)),
            TapDemand {
                buttons: false,
                scroll: false,
                flags: false
            }
        );
    }

    #[test]
    fn defaults_are_version_1_with_both_switches_on() {
        assert_eq!(Config::default(), config(true, true));
    }

    #[test]
    fn serializes_to_the_documented_shape() {
        let json = serde_json::to_value(config(true, false)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "version": 1,
                "general": { "buttons_enabled": true, "scroll_enabled": false },
                "buttons": serde_json::to_value(Buttons::default()).unwrap()
            })
        );
    }

    #[test]
    fn default_buttons_are_the_four_documented_remaps() {
        assert_eq!(
            serde_json::to_value(Buttons::default()).unwrap(),
            serde_json::json!({ "remaps": [
                { "trigger": { "button": 3, "level": 1, "duration": "click" },
                  "effect": { "action": { "type": "SymbolicHotkey", "id": "MissionControl" } } },
                { "trigger": { "button": 3, "level": 1, "duration": "hold" },
                  "effect": { "action": { "type": "SymbolicHotkey", "id": "ShowDesktop" } } },
                { "trigger": { "button": 4, "level": 1, "duration": "click" },
                  "effect": { "action": { "type": "Navigate", "direction": "Back" } } },
                { "trigger": { "button": 5, "level": 1, "duration": "click" },
                  "effect": { "action": { "type": "Navigate", "direction": "Forward" } } }
            ] })
        );
    }

    #[test]
    fn a_remap_from_the_ui_must_be_complete() {
        let missing_effect = serde_json::json!({
            "version": 1,
            "general": { "buttons_enabled": true, "scroll_enabled": true },
            "buttons": { "remaps": [ { "trigger": { "button": 3, "level": 1, "duration": "click" } } ] }
        });
        assert!(serde_json::from_value::<Config>(missing_effect).is_err());
    }

    #[test]
    fn a_key_combo_action_has_the_documented_shape() {
        let action = Action::KeyCombo {
            key_code: 1,
            modifiers: vec![KeyModifier::Command, KeyModifier::Shift],
            key_label: "S".into(),
        };
        assert_eq!(
            serde_json::to_value(&action).unwrap(),
            serde_json::json!({ "type": "KeyCombo", "key_code": 1, "modifiers": ["Command", "Shift"], "key_label": "S" })
        );
    }
}
