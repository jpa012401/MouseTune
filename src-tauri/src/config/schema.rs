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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            general: General::default(),
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
            serde_json::json!({ "version": 1, "general": { "buttons_enabled": true, "scroll_enabled": false } })
        );
    }
}
