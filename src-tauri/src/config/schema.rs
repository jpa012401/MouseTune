//! The settings file's shape. Every field is required so the UI can't send
//! partial settings; the loader fills what a file leaves out from the
//! defaults (`load::merge_defaults`), so later phases can add fields without
//! a version bump (design §1).

use std::collections::BTreeMap;

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
    pub scroll: Scroll,
    pub timing: Timing,
    /// Per-app Back/Forward strategy, keyed by bundle ID (spec §7.3).
    pub nav_overrides: BTreeMap<String, NavStrategy>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            general: General::default(),
            buttons: Buttons::default(),
            scroll: Scroll::default(),
            timing: Timing::default(),
            nav_overrides: default_nav_overrides(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct General {
    /// Kill switch for button features.
    pub buttons_enabled: bool,
    /// Kill switch for scroll features.
    pub scroll_enabled: bool,
    /// Keep the pointer still during drag gestures.
    pub freeze_pointer_during_drag: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            buttons_enabled: true,
            scroll_enabled: true,
            freeze_pointer_during_drag: true,
        }
    }
}

/// Button remaps (design §1). Button numbers are 1-based as users count
/// them: 3 is the middle button. `input::remaps::compile` turns this into
/// the engine's lookup tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Buttons {
    pub remaps: Vec<Remap>,
    pub drags: Vec<DragBinding>,
    pub scroll_bindings: Vec<ScrollBinding>,
    pub hold_through: Vec<HoldThrough>,
}

impl Default for Buttons {
    /// B3 hold is not a default: with B3 drag bound, a slow-starting drag would fire it (Phase 6 design, Decisions).
    fn default() -> Self {
        use NavDirection::{Back, Forward};
        use SymbolicHotkeyId::MissionControl;
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
            drags: vec![DragBinding {
                button: 3,
                effect: DragEffect::SpacesAndMissionControl,
            }],
            scroll_bindings: vec![],
            hold_through: vec![],
        }
    }
}

/// A drag of a held button (Phase 6 design §1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DragBinding {
    /// 1-based, 3 = middle.
    pub button: u8,
    pub effect: DragEffect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type")]
#[expect(
    clippy::large_enum_variant,
    reason = "settings data, a few entries at most, never on the hot path; boxing would only complicate the wire type"
)]
pub enum DragEffect {
    /// Interactive dock swipe: left/right switches Space, up Mission
    /// Control, down App Exposé.
    SpacesAndMissionControl,
    /// Two-finger-style scroll swipe.
    ScrollSwipe,
    /// One action per direction once the drag passes 40 px.
    Discrete {
        left: Option<Action>,
        right: Option<Action>,
        up: Option<Action>,
        down: Option<Action>,
    },
}

/// Scrolling while a button is held (spec §8.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ScrollBinding {
    pub button: u8,
    pub effect: ScrollEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum ScrollEffect {
    Zoom,
    Horizontal,
    AppSwitcher,
}

/// A button that behaves exactly like another while held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct HoldThrough {
    pub button: u8,
    pub acts_as: MouseButton,
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
    /// A full click of another button, 1 or 2 clicks.
    MouseClick {
        button: MouseButton,
        clicks: u8,
    },
    SystemKey {
        key: SystemKey,
    },
}

/// Clicks a remap can post (Phase 4c design §1). Named, not numbered, so the
/// file reads clearly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    /// Button 4.
    Back,
    /// Button 5.
    Forward,
}

impl MouseButton {
    /// The CoreGraphics button number (left = 0).
    pub fn cg_button(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Middle => 2,
            Self::Back => 3,
            Self::Forward => 4,
        }
    }
}

/// Media and brightness keys (spec §7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum SystemKey {
    PlayPause,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    Mute,
    BrightnessUp,
    BrightnessDown,
}

/// The click-cycle timers (spec §5.1). Both within `TIMING_MS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Timing {
    /// How long a press must last to fire a Hold action.
    pub hold_ms: u32,
    /// How soon the next press must come to count as a double or triple click.
    pub double_click_ms: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            hold_ms: 250,
            double_click_ms: 260,
        }
    }
}

/// Allowed `timing` values, in ms.
pub const TIMING_MS: std::ops::RangeInclusive<u32> = 150..=600;

/// How Back/Forward reach an app (spec §7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum NavStrategy {
    /// ⌘[ / ⌘].
    CmdBracket,
    /// Synthesized buttons 4 / 5.
    MouseButtons,
}

/// Apps where ⌘[ / ⌘] means back / forward. Xcode and Notes stay off the
/// list: there ⌘[ edits text.
fn default_nav_overrides() -> BTreeMap<String, NavStrategy> {
    [
        "com.apple.Safari",
        "com.apple.finder",
        "com.apple.Music",
        "com.apple.AppStore",
        "com.apple.systempreferences",
        "com.apple.Preview",
    ]
    .into_iter()
    .map(|id| (id.to_owned(), NavStrategy::CmdBracket))
    .collect()
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

/// Smooth scrolling (Phase 5a design §1). Every field is `Copy`, so the
/// input engine takes the whole section as its settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Scroll {
    /// Animate wheel ticks. Off: inversion only, done in place.
    pub smooth: bool,
    pub smoothness: Smoothness,
    pub speed: ScrollSpeed,
    /// Send the coasting tail as momentum events instead of `Changed`.
    pub trackpad_simulation: bool,
    /// Flip that axis relative to what macOS delivers (which already
    /// includes the Natural Scrolling setting).
    pub invert_vertical: bool,
    pub invert_horizontal: bool,
    /// A slow wheel scrolls in fine steps (Phase 5b design §3).
    pub precise: bool,
    pub keyboard_modifiers: ScrollModifiers,
}

/// Which held key applies each scroll effect (Phase 5b design §1): one key
/// per effect, each key at most once (`validate::duplicate_scroll_modifier`).
/// A key that isn't bound passes the tick through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ScrollModifiers {
    pub horizontal: Option<KeyModifier>,
    pub precise: Option<KeyModifier>,
    pub swift: Option<KeyModifier>,
    pub zoom: Option<KeyModifier>,
}

impl Default for ScrollModifiers {
    /// Ctrl stays free for Accessibility Zoom; Cmd for apps (and Phase 6 zoom).
    fn default() -> Self {
        Self {
            horizontal: Some(KeyModifier::Shift),
            precise: Some(KeyModifier::Option),
            swift: None,
            zoom: None,
        }
    }
}

impl Default for Scroll {
    fn default() -> Self {
        Self {
            smooth: true,
            smoothness: Smoothness::Regular,
            speed: ScrollSpeed::Medium,
            trackpad_simulation: true,
            invert_vertical: false,
            invert_horizontal: false,
            precise: false,
            keyboard_modifiers: ScrollModifiers::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Smoothness {
    Low,
    Regular,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ScrollSpeed {
    /// The distance macOS would have scrolled, just smoothed.
    System,
    Low,
    Medium,
    High,
}

/// Which event taps the settings ask for.
pub fn tap_demand(config: &Config) -> TapDemand {
    let g = &config.general;
    let s = &config.scroll;
    TapDemand {
        buttons: g.buttons_enabled,
        // Off when no setting would change a wheel event (spec §4.1).
        scroll: g.scroll_enabled && (s.smooth || s.invert_vertical || s.invert_horizontal),
        flags: g.buttons_enabled || g.scroll_enabled,
        keys: false,
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
                freeze_pointer_during_drag: true,
            },
            buttons: Buttons::default(),
            scroll: Scroll::default(),
            timing: Timing::default(),
            nav_overrides: default_nav_overrides(),
        }
    }

    #[test]
    fn both_on_asks_for_every_tap() {
        assert_eq!(
            tap_demand(&config(true, true)),
            TapDemand {
                buttons: true,
                scroll: true,
                flags: true,
                keys: false
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
                flags: true,
                keys: false
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
                flags: true,
                keys: false
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
                flags: false,
                keys: false
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
                "general": { "buttons_enabled": true, "scroll_enabled": false, "freeze_pointer_during_drag": true },
                "buttons": serde_json::to_value(Buttons::default()).unwrap(),
                "scroll": serde_json::to_value(Scroll::default()).unwrap(),
                "timing": { "hold_ms": 250, "double_click_ms": 260 },
                "nav_overrides": serde_json::to_value(default_nav_overrides()).unwrap()
            })
        );
    }

    #[test]
    fn default_buttons_are_the_documented_remaps_and_b3_drag() {
        assert_eq!(
            serde_json::to_value(Buttons::default()).unwrap(),
            serde_json::json!({ "remaps": [
                { "trigger": { "button": 3, "level": 1, "duration": "click" },
                  "effect": { "action": { "type": "SymbolicHotkey", "id": "MissionControl" } } },
                { "trigger": { "button": 4, "level": 1, "duration": "click" },
                  "effect": { "action": { "type": "Navigate", "direction": "Back" } } },
                { "trigger": { "button": 5, "level": 1, "duration": "click" },
                  "effect": { "action": { "type": "Navigate", "direction": "Forward" } } }
            ],
            "drags": [ { "button": 3, "effect": { "type": "SpacesAndMissionControl" } } ],
            "scroll_bindings": [],
            "hold_through": [] })
        );
    }

    #[test]
    fn default_buttons_drag_b3_through_spaces_and_drop_the_b3_hold() {
        let b = Buttons::default();
        assert_eq!(
            serde_json::to_value(&b.drags).unwrap(),
            serde_json::json!([{ "button": 3, "effect": { "type": "SpacesAndMissionControl" } }])
        );
        assert!(
            !b.remaps
                .iter()
                .any(|r| r.trigger.button == 3 && r.trigger.duration == TriggerDuration::Hold),
            "B3 hold is no longer a default"
        );
        assert_eq!(b.remaps.len(), 3);
        assert!(b.scroll_bindings.is_empty() && b.hold_through.is_empty());
    }

    #[test]
    fn binding_wire_shapes_match_the_design() {
        let discrete = DragEffect::Discrete {
            left: Some(Action::Navigate {
                direction: NavDirection::Back,
            }),
            right: None,
            up: None,
            down: None,
        };
        assert_eq!(
            serde_json::to_value(&discrete).unwrap(),
            serde_json::json!({ "type": "Discrete", "left": { "type": "Navigate", "direction": "Back" }, "right": null, "up": null, "down": null })
        );
        assert_eq!(
            serde_json::to_value(ScrollBinding {
                button: 4,
                effect: ScrollEffect::Zoom
            })
            .unwrap(),
            serde_json::json!({ "button": 4, "effect": "Zoom" })
        );
        assert_eq!(
            serde_json::to_value(HoldThrough {
                button: 6,
                acts_as: MouseButton::Middle
            })
            .unwrap(),
            serde_json::json!({ "button": 6, "acts_as": "Middle" })
        );
    }

    #[test]
    fn freeze_and_zoom_slot_defaults() {
        let c = Config::default();
        assert!(c.general.freeze_pointer_during_drag);
        assert_eq!(c.scroll.keyboard_modifiers.zoom, None);
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

    #[test]
    fn the_new_actions_have_the_design_wire_shape() {
        let click = Action::MouseClick {
            button: MouseButton::Middle,
            clicks: 1,
        };
        assert_eq!(
            serde_json::to_value(&click).unwrap(),
            serde_json::json!({ "type": "MouseClick", "button": "Middle", "clicks": 1 })
        );
        let key = Action::SystemKey {
            key: SystemKey::PlayPause,
        };
        assert_eq!(
            serde_json::to_value(&key).unwrap(),
            serde_json::json!({ "type": "SystemKey", "key": "PlayPause" })
        );
    }

    #[test]
    fn mouse_buttons_map_to_cg_numbers() {
        use MouseButton::*;
        let cg: Vec<u8> = [Left, Right, Middle, Back, Forward]
            .map(MouseButton::cg_button)
            .to_vec();
        assert_eq!(cg, [0, 1, 2, 3, 4]);
    }

    #[test]
    fn default_timing_and_nav_overrides_are_the_design_shape() {
        let c = Config::default();
        assert_eq!(
            serde_json::to_value(c.timing).unwrap(),
            serde_json::json!({ "hold_ms": 250, "double_click_ms": 260 })
        );
        assert_eq!(
            serde_json::to_value(&c.nav_overrides).unwrap(),
            serde_json::json!({
                "com.apple.AppStore": "CmdBracket",
                "com.apple.Music": "CmdBracket",
                "com.apple.Preview": "CmdBracket",
                "com.apple.Safari": "CmdBracket",
                "com.apple.finder": "CmdBracket",
                "com.apple.systempreferences": "CmdBracket"
            })
        );
    }

    #[test]
    fn default_scroll_section_is_the_design_shape() {
        assert_eq!(
            serde_json::to_value(Config::default().scroll).unwrap(),
            serde_json::json!({
                "smooth": true,
                "smoothness": "regular",
                "speed": "medium",
                "trackpad_simulation": true,
                "invert_vertical": false,
                "invert_horizontal": false,
                "precise": false,
                "keyboard_modifiers": { "horizontal": "Shift", "precise": "Option", "swift": null, "zoom": null }
            })
        );
    }

    #[test]
    fn the_scroll_tap_runs_only_when_a_setting_would_change_wheel_events() {
        let demand = |enabled, smooth, invert_vertical, invert_horizontal| {
            let mut c = Config::default();
            c.general.scroll_enabled = enabled;
            c.scroll.smooth = smooth;
            c.scroll.invert_vertical = invert_vertical;
            c.scroll.invert_horizontal = invert_horizontal;
            tap_demand(&c).scroll
        };
        assert!(demand(true, true, false, false));
        assert!(demand(true, false, true, false));
        assert!(demand(true, false, false, true));
        assert!(!demand(true, false, false, false), "nothing would change");
        assert!(!demand(false, true, true, true), "kill switch wins");
    }
}
