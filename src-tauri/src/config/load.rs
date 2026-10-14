//! Loading rules (spec §9). Pure: the store does the I/O.

use serde_json::Value;

use crate::config::{migrate::migrate, schema::Config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadOutcome {
    pub config: Config,
    /// The file was unusable: back it up and tell the user.
    pub recover: bool,
}

/// `contents` is `None` when the file doesn't exist.
pub fn parse_config(contents: Option<&str>) -> LoadOutcome {
    let Some(text) = contents else {
        return LoadOutcome {
            config: Config::default(),
            recover: false,
        };
    };
    match try_parse(text) {
        Some(config) => LoadOutcome {
            config,
            recover: false,
        },
        None => LoadOutcome {
            config: Config::default(),
            recover: true,
        },
    }
}

fn try_parse(text: &str) -> Option<Config> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let version = u32::try_from(value.get("version")?.as_u64()?).ok()?;
    if version == 0 || version > crate::config::schema::CURRENT_VERSION {
        return None;
    }
    let defaults = serde_json::to_value(Config::default()).ok()?;
    let merged = merge_defaults(defaults, migrate(value, version));
    let mut config: Config = serde_json::from_value(merged).ok()?;
    config.version = crate::config::schema::CURRENT_VERSION;
    Some(config)
}

/// Fills what the file leaves out from `defaults`, so the file can omit any
/// section or field while the types stay strict. Objects merge key by key;
/// any other file value wins. Unknown file keys are kept (serde ignores them).
fn merge_defaults(defaults: Value, file: Value) -> Value {
    match (defaults, file) {
        (Value::Object(mut merged), Value::Object(file)) => {
            for (key, file_value) in file {
                let value = match merged.remove(&key) {
                    Some(default_value) => merge_defaults(default_value, file_value),
                    None => file_value,
                };
                merged.insert(key, value);
            }
            Value::Object(merged)
        }
        (_, file) => file,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::General;
    use serde_json::json;

    #[test]
    fn an_older_file_keeps_its_remaps_and_gains_the_default_drag() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "buttons": { "remaps": [
                { "trigger": { "button": 3, "level": 1, "duration": "hold" }, "effect": { "action": { "type": "SymbolicHotkey", "id": "ShowDesktop" } } }
            ] } }"#,
        ));
        assert!(!outcome.recover);
        let b = &outcome.config.buttons;
        assert_eq!(b.remaps.len(), 1, "the file's remaps are kept as they are");
        assert_eq!(
            b.drags,
            crate::config::schema::Buttons::default().drags,
            "the new list comes from the defaults"
        );
        assert!(outcome.config.general.freeze_pointer_during_drag);
    }

    #[test]
    fn a_phase_5a_file_gets_the_modifier_defaults() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "scroll": { "smooth": true, "smoothness": "high", "speed": "low", "trackpad_simulation": false, "invert_vertical": true, "invert_horizontal": false } }"#,
        ));
        assert!(!outcome.recover);
        let s = outcome.config.scroll;
        assert!(!s.precise);
        assert_eq!(
            s.keyboard_modifiers,
            crate::config::schema::ScrollModifiers::default()
        );
        assert!(s.invert_vertical, "the file's own values stay");
    }

    #[test]
    fn a_phase_5b_file_gets_the_timing_and_nav_defaults() {
        let outcome = parse_config(Some(r#"{ "version": 1, "scroll": { "precise": true } }"#));
        assert!(!outcome.recover);
        assert_eq!(
            outcome.config.timing,
            crate::config::schema::Timing::default()
        );
        assert_eq!(
            outcome.config.nav_overrides,
            Config::default().nav_overrides
        );
    }

    #[test]
    fn a_phase_6_file_shows_the_menu_bar_item() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "general": { "buttons_enabled": true, "scroll_enabled": false, "freeze_pointer_during_drag": true } }"#,
        ));
        assert!(!outcome.recover);
        assert!(outcome.config.general.show_menu_bar_item);
        assert!(!outcome.config.general.scroll_enabled);
    }

    #[test]
    fn a_hidden_menu_bar_item_stays_hidden() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "general": { "show_menu_bar_item": false } }"#,
        ));
        assert!(!outcome.config.general.show_menu_bar_item);
    }

    #[test]
    fn a_file_nav_entry_adds_an_app_or_overrides_a_default() {
        use crate::config::schema::NavStrategy::{CmdBracket, MouseButtons};
        let outcome = parse_config(Some(
            r#"{ "version": 1, "nav_overrides": { "com.apple.dt.Xcode": "CmdBracket", "com.apple.Safari": "MouseButtons" } }"#,
        ));
        assert!(!outcome.recover);
        let nav = &outcome.config.nav_overrides;
        assert_eq!(nav.get("com.apple.dt.Xcode"), Some(&CmdBracket));
        assert_eq!(nav.get("com.apple.Safari"), Some(&MouseButtons));
        assert_eq!(
            nav.get("com.apple.finder"),
            Some(&CmdBracket),
            "defaults stay"
        );
        assert_eq!(nav.len(), 7);
    }

    #[test]
    fn a_null_binding_in_the_file_stays_unbound() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "scroll": { "keyboard_modifiers": { "horizontal": null } } }"#,
        ));
        assert!(!outcome.recover);
        let m = outcome.config.scroll.keyboard_modifiers;
        assert_eq!(m.horizontal, None);
        assert_eq!(m.precise, Some(crate::config::schema::KeyModifier::Option));
    }

    #[test]
    fn merge_fills_missing_keys_at_every_depth_and_keeps_file_values() {
        let defaults =
            json!({ "a": 1, "outer": { "x": true, "inner": { "y": 2, "z": 3 } }, "list": [1] });
        let file =
            json!({ "outer": { "inner": { "z": 9 }, "extra": "kept" }, "list": [], "unknown": 5 });
        assert_eq!(
            merge_defaults(defaults, file),
            json!({ "a": 1, "outer": { "x": true, "inner": { "y": 2, "z": 9 }, "extra": "kept" }, "list": [], "unknown": 5 })
        );
    }

    #[test]
    fn merge_lets_a_non_object_file_value_replace_a_default_object() {
        assert_eq!(
            merge_defaults(
                json!({ "general": { "x": true } }),
                json!({ "general": null })
            ),
            json!({ "general": null })
        );
    }

    fn ok(config: Config) -> LoadOutcome {
        LoadOutcome {
            config,
            recover: false,
        }
    }

    fn recovered() -> LoadOutcome {
        LoadOutcome {
            config: Config::default(),
            recover: true,
        }
    }

    #[test]
    fn missing_file_gives_defaults_without_recovery() {
        assert_eq!(parse_config(None), ok(Config::default()));
    }

    #[test]
    fn valid_file_is_used() {
        let text =
            r#"{ "version": 1, "general": { "buttons_enabled": false, "scroll_enabled": true } }"#;
        let want = Config {
            version: 1,
            general: General {
                buttons_enabled: false,
                scroll_enabled: true,
                freeze_pointer_during_drag: true,
                show_menu_bar_item: true,
            },
            ..Config::default()
        };
        assert_eq!(parse_config(Some(text)), ok(want));
    }

    #[test]
    fn missing_fields_take_defaults() {
        let text = r#"{ "version": 1, "general": { "scroll_enabled": false } }"#;
        let want = Config {
            version: 1,
            general: General {
                buttons_enabled: true,
                scroll_enabled: false,
                freeze_pointer_during_drag: true,
                show_menu_bar_item: true,
            },
            ..Config::default()
        };
        assert_eq!(parse_config(Some(text)), ok(want));
    }

    #[test]
    fn missing_section_takes_defaults() {
        assert_eq!(
            parse_config(Some(r#"{ "version": 1 }"#)),
            ok(Config::default())
        );
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let text = r#"{ "version": 1, "general": { "scroll_enabled": false, "future": 3 }, "remaps": [] }"#;
        let want = Config {
            version: 1,
            general: General {
                buttons_enabled: true,
                scroll_enabled: false,
                freeze_pointer_during_drag: true,
                show_menu_bar_item: true,
            },
            ..Config::default()
        };
        assert_eq!(parse_config(Some(text)), ok(want));
    }

    #[test]
    fn unparseable_json_is_recovered() {
        assert_eq!(parse_config(Some("{ not json")), recovered());
    }

    #[test]
    fn wrong_field_types_are_recovered() {
        assert_eq!(
            parse_config(Some(
                r#"{ "version": 1, "general": { "scroll_enabled": "no" } }"#
            )),
            recovered()
        );
    }

    #[test]
    fn missing_version_is_recovered() {
        assert_eq!(parse_config(Some(r#"{ "general": {} }"#)), recovered());
    }

    #[test]
    fn non_integer_version_is_recovered() {
        assert_eq!(parse_config(Some(r#"{ "version": "1" }"#)), recovered());
        assert_eq!(parse_config(Some(r#"{ "version": 1.5 }"#)), recovered());
    }

    #[test]
    fn version_zero_is_recovered() {
        assert_eq!(parse_config(Some(r#"{ "version": 0 }"#)), recovered());
    }

    #[test]
    fn newer_version_is_recovered_not_edited() {
        assert_eq!(
            parse_config(Some(r#"{ "version": 2, "general": {} }"#)),
            recovered()
        );
    }

    #[test]
    fn a_phase_3_file_without_buttons_gets_the_default_remaps() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "general": { "buttons_enabled": false, "scroll_enabled": true } }"#,
        ));
        assert!(!outcome.recover);
        assert!(!outcome.config.general.buttons_enabled);
        assert_eq!(
            outcome.config.buttons,
            crate::config::schema::Buttons::default()
        );
    }

    #[test]
    fn an_empty_remap_list_stays_empty() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "general": { "buttons_enabled": true, "scroll_enabled": true }, "buttons": { "remaps": [] } }"#,
        ));
        assert!(!outcome.recover);
        assert!(outcome.config.buttons.remaps.is_empty());
    }

    #[test]
    fn an_unknown_action_type_recovers_to_defaults() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "general": { "buttons_enabled": true, "scroll_enabled": true },
                 "buttons": { "remaps": [ { "trigger": { "button": 3, "level": 1, "duration": "click" },
                                            "effect": { "action": { "type": "Teleport" } } } ] } }"#,
        ));
        assert!(outcome.recover);
    }

    #[test]
    fn a_phase_4b_file_without_scroll_gets_the_scroll_defaults() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "general": { "buttons_enabled": true, "scroll_enabled": false }, "buttons": { "remaps": [] } }"#,
        ));
        assert!(!outcome.recover);
        assert!(!outcome.config.general.scroll_enabled);
        assert_eq!(
            outcome.config.scroll,
            crate::config::schema::Scroll::default()
        );
    }

    #[test]
    fn a_partial_scroll_section_keeps_its_values_and_fills_the_rest() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "scroll": { "smooth": false, "speed": "high" } }"#,
        ));
        assert!(!outcome.recover);
        let s = outcome.config.scroll;
        assert!(!s.smooth);
        assert_eq!(s.speed, crate::config::schema::ScrollSpeed::High);
        assert_eq!(s.smoothness, crate::config::schema::Smoothness::Regular);
        assert!(s.trackpad_simulation);
    }

    #[test]
    fn an_unknown_smoothness_recovers_to_defaults() {
        let outcome = parse_config(Some(
            r#"{ "version": 1, "scroll": { "smoothness": "silky" } }"#,
        ));
        assert!(outcome.recover);
    }
}
