//! Loading rules (design §1 table). Pure: the store does the I/O.

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
    let mut config: Config = serde_json::from_value(migrate(value, version)).ok()?;
    config.version = crate::config::schema::CURRENT_VERSION;
    Some(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::General;

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
            },
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
            },
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
            },
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
}
