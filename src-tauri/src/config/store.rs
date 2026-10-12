//! Single owner of the current settings (design §2).

use serde::Serialize;
use specta::Type;

use crate::config::{
    file::ConfigFile,
    load::parse_config,
    schema::{CURRENT_VERSION, Config, KeyModifier},
    validate::{RemapProblem, duplicate_scroll_modifier, validate_remaps},
};

/// Called after a change has been saved. Runs while the store is locked:
/// implementations must never call back into the store.
pub trait ConfigObserver: Send {
    fn config_changed(&self, config: &Config);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind")]
pub enum ConfigError {
    #[error("{message}")]
    Io { message: String },
    #[error("settings version {found} isn't supported (expected {expected})")]
    UnsupportedVersion { found: u32, expected: u32 },
    #[error("remap {index} is invalid: {reason:?}")]
    InvalidRemap { index: u32, reason: RemapProblem },
    #[error("{key:?} is bound to two scroll effects")]
    DuplicateScrollModifier { key: KeyModifier },
}

/// The settings file couldn't be used at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    BackedUp { file_name: String },
    BackupFailed,
}

pub struct ConfigStore {
    file: Box<dyn ConfigFile>,
    current: Config,
    recovery: Option<Recovery>,
    observers: Vec<Box<dyn ConfigObserver>>,
}

impl ConfigStore {
    pub fn load(file: Box<dyn ConfigFile>) -> Self {
        let contents = file.read().unwrap_or_else(|err| {
            tracing::warn!(%err, "couldn't read the settings file; using defaults");
            None
        });
        let outcome = parse_config(contents.as_deref());
        let recovery = outcome.recover.then(|| match file.backup() {
            Ok(file_name) => {
                tracing::warn!(%file_name, "settings file was unusable; kept a backup");
                Recovery::BackedUp { file_name }
            }
            Err(err) => {
                tracing::warn!(%err, "settings file was unusable and couldn't be backed up");
                Recovery::BackupFailed
            }
        });
        Self {
            file,
            current: outcome.config,
            recovery,
            observers: Vec::new(),
        }
    }
    pub fn subscribe(&mut self, observer: Box<dyn ConfigObserver>) {
        self.observers.push(observer);
    }
    pub fn current(&self) -> &Config {
        &self.current
    }
    pub fn update(&mut self, new: Config) -> Result<Config, ConfigError> {
        if new.version != CURRENT_VERSION {
            return Err(ConfigError::UnsupportedVersion {
                found: new.version,
                expected: CURRENT_VERSION,
            });
        }
        // Only a changed list is checked: a bad hand-edited remap still loads
        // (design §1), and it must not stop the kill switches from saving.
        if new.buttons.remaps != self.current.buttons.remaps {
            validate_remaps(&new.buttons.remaps).map_err(|(index, reason)| {
                ConfigError::InvalidRemap {
                    index: index as u32,
                    reason,
                }
            })?;
        }
        // Only a changed binding is checked, like remaps: a bad hand-edited
        // file still loads, and its other settings must still save.
        if new.scroll.keyboard_modifiers != self.current.scroll.keyboard_modifiers
            && let Some(key) = duplicate_scroll_modifier(&new.scroll.keyboard_modifiers)
        {
            return Err(ConfigError::DuplicateScrollModifier { key });
        }
        if new == self.current {
            return Ok(new);
        }
        let text = serde_json::to_string_pretty(&new).expect("Config always serializes");
        self.file
            .write_atomic(&text)
            .map_err(|err| ConfigError::Io {
                message: err.to_string(),
            })?;
        self.current = new;
        // The write replaced the unreadable file, so "left in place" is no longer true.
        // A kept backup still exists, so that notice stays until dismissed.
        if self.recovery == Some(Recovery::BackupFailed) {
            self.recovery = None;
        }
        for observer in &self.observers {
            observer.config_changed(&self.current);
        }
        Ok(self.current.clone())
    }
    pub fn reset(&mut self) -> Result<Config, ConfigError> {
        self.update(Config::default())
    }
    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }
    pub fn dismiss_recovery(&mut self) {
        self.recovery = None;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::config::schema::General;
    use crate::test_support::FakeConfigFile;

    const SCROLL_OFF: &str =
        r#"{ "version": 1, "general": { "buttons_enabled": true, "scroll_enabled": false } }"#;

    fn scroll_off() -> Config {
        Config {
            version: 1,
            general: General {
                buttons_enabled: true,
                scroll_enabled: false,
            },
            ..Config::default()
        }
    }

    /// Records each notification plus how many writes had happened by then.
    #[derive(Clone)]
    struct Seen {
        file: FakeConfigFile,
        log: Arc<Mutex<Vec<(Config, usize)>>>,
    }

    impl Seen {
        fn new(file: &FakeConfigFile) -> Self {
            Self {
                file: file.clone(),
                log: Arc::default(),
            }
        }
        fn log(&self) -> Vec<(Config, usize)> {
            self.log.lock().unwrap().clone()
        }
    }

    impl ConfigObserver for Seen {
        fn config_changed(&self, config: &Config) {
            self.log
                .lock()
                .unwrap()
                .push((config.clone(), self.file.writes().len()));
        }
    }

    #[test]
    fn missing_file_starts_with_defaults_and_writes_nothing() {
        let file = FakeConfigFile::missing();
        let store = ConfigStore::load(Box::new(file.clone()));
        assert_eq!(store.current(), &Config::default());
        assert_eq!(store.recovery(), None);
        assert!(file.writes().is_empty());
    }

    #[test]
    fn saved_settings_are_loaded() {
        let store = ConfigStore::load(Box::new(FakeConfigFile::with(SCROLL_OFF)));
        assert_eq!(store.current(), &scroll_off());
    }

    #[test]
    fn unreadable_file_is_backed_up_and_reported() {
        let file = FakeConfigFile::with("{ broken");
        let store = ConfigStore::load(Box::new(file.clone()));
        assert_eq!(store.current(), &Config::default());
        assert_eq!(file.backups(), 1);
        assert_eq!(
            store.recovery(),
            Some(&Recovery::BackedUp {
                file_name: "config.1760000000.bak.json".into()
            })
        );
    }

    #[test]
    fn failed_backup_is_reported_and_the_file_left_alone() {
        let file = FakeConfigFile::with("{ broken");
        file.fail_backups();
        let store = ConfigStore::load(Box::new(file.clone()));
        assert_eq!(store.current(), &Config::default());
        assert_eq!(store.recovery(), Some(&Recovery::BackupFailed));
        assert!(file.writes().is_empty());
    }

    #[test]
    fn read_error_is_treated_as_missing_without_recovery() {
        let file = FakeConfigFile::with(SCROLL_OFF);
        file.fail_reads();
        let store = ConfigStore::load(Box::new(file.clone()));
        assert_eq!(store.current(), &Config::default());
        assert_eq!(store.recovery(), None);
        assert_eq!(file.backups(), 0);
    }

    #[test]
    fn update_writes_before_notifying_each_observer_once() {
        let file = FakeConfigFile::missing();
        let mut store = ConfigStore::load(Box::new(file.clone()));
        let (a, b) = (Seen::new(&file), Seen::new(&file));
        store.subscribe(Box::new(a.clone()));
        store.subscribe(Box::new(b.clone()));

        assert_eq!(store.update(scroll_off()), Ok(scroll_off()));

        assert_eq!(store.current(), &scroll_off());
        assert_eq!(file.writes().len(), 1);
        assert_eq!(
            serde_json::from_str::<Config>(&file.writes()[0]).unwrap(),
            scroll_off()
        );
        assert_eq!(a.log(), vec![(scroll_off(), 1)], "notified after the write");
        assert_eq!(b.log(), vec![(scroll_off(), 1)]);
    }

    #[test]
    fn failed_write_changes_nothing() {
        let file = FakeConfigFile::missing();
        file.fail_writes();
        let mut store = ConfigStore::load(Box::new(file.clone()));
        let seen = Seen::new(&file);
        store.subscribe(Box::new(seen.clone()));

        assert_eq!(
            store.update(scroll_off()),
            Err(ConfigError::Io {
                message: "disk full".into()
            })
        );
        assert_eq!(store.current(), &Config::default());
        assert!(seen.log().is_empty());
    }

    #[test]
    fn wrong_version_is_rejected_without_writing() {
        let file = FakeConfigFile::missing();
        let mut store = ConfigStore::load(Box::new(file.clone()));
        let bad = Config {
            version: 7,
            ..scroll_off()
        };
        assert_eq!(
            store.update(bad),
            Err(ConfigError::UnsupportedVersion {
                found: 7,
                expected: 1
            })
        );
        assert!(file.writes().is_empty());
        assert_eq!(store.current(), &Config::default());
    }

    #[test]
    fn an_invalid_remap_is_rejected_without_writing() {
        let file = FakeConfigFile::missing();
        let mut store = ConfigStore::load(Box::new(file.clone()));
        let seen = Seen::new(&file);
        store.subscribe(Box::new(seen.clone()));
        let mut bad = Config::default();
        bad.buttons.remaps[0].trigger.button = 1;
        assert_eq!(
            store.update(bad),
            Err(ConfigError::InvalidRemap {
                index: 0,
                reason: RemapProblem::ButtonNotRemappable
            })
        );
        assert!(file.writes().is_empty());
        assert!(seen.log().is_empty());
        assert_eq!(store.current(), &Config::default());
    }

    #[test]
    fn a_key_bound_twice_is_rejected_without_writing() {
        let file = FakeConfigFile::missing();
        let mut store = ConfigStore::load(Box::new(file.clone()));
        let mut next = store.current().clone();
        next.scroll.keyboard_modifiers.swift = Some(KeyModifier::Shift);
        assert_eq!(
            store.update(next),
            Err(ConfigError::DuplicateScrollModifier {
                key: KeyModifier::Shift
            })
        );
        assert!(file.writes().is_empty());
    }

    #[test]
    fn a_bad_hand_edited_scroll_binding_still_loads_and_other_settings_save() {
        let file = FakeConfigFile::with(
            r#"{ "version": 1, "scroll": { "keyboard_modifiers": { "horizontal": "Option", "precise": "Option", "swift": null } } }"#,
        );
        let mut store = ConfigStore::load(Box::new(file.clone()));
        assert_eq!(
            store.current().scroll.keyboard_modifiers.precise,
            Some(KeyModifier::Option)
        );
        let mut next = store.current().clone();
        next.general.buttons_enabled = false;
        assert!(
            store.update(next).is_ok(),
            "an unchanged binding isn't re-checked"
        );
        assert_eq!(file.writes().len(), 1);
    }

    #[test]
    fn a_bad_hand_edited_remap_still_loads() {
        let file = FakeConfigFile::with(
            r#"{ "version": 1, "general": { "buttons_enabled": true, "scroll_enabled": true },
            "buttons": { "remaps": [ { "trigger": { "button": 1, "level": 1, "duration": "click" },
                                       "effect": { "action": { "type": "Navigate", "direction": "Back" } } } ] } }"#,
        );
        let store = ConfigStore::load(Box::new(file));
        assert_eq!(store.current().buttons.remaps.len(), 1);
        assert!(store.recovery().is_none());
    }

    #[test]
    fn a_bad_hand_edited_remap_does_not_block_the_kill_switches() {
        let file = FakeConfigFile::with(
            r#"{ "version": 1, "general": { "buttons_enabled": true, "scroll_enabled": true },
            "buttons": { "remaps": [ { "trigger": { "button": 1, "level": 1, "duration": "click" },
                                       "effect": { "action": { "type": "Navigate", "direction": "Back" } } } ] } }"#,
        );
        let mut store = ConfigStore::load(Box::new(file.clone()));

        let mut next = store.current().clone();
        next.general.buttons_enabled = false;
        let saved = store.update(next).expect("the kill switch saves");

        assert!(!saved.general.buttons_enabled);
        assert_eq!(file.writes().len(), 1);
        let written: Config = serde_json::from_str(&file.writes()[0]).unwrap();
        assert!(!written.general.buttons_enabled);
        assert_eq!(written.buttons.remaps[0].trigger.button, 1);
    }

    #[test]
    fn unchanged_settings_neither_write_nor_notify() {
        let file = FakeConfigFile::missing();
        let mut store = ConfigStore::load(Box::new(file.clone()));
        let seen = Seen::new(&file);
        store.subscribe(Box::new(seen.clone()));
        assert_eq!(store.update(Config::default()), Ok(Config::default()));
        assert!(file.writes().is_empty());
        assert!(seen.log().is_empty());
    }

    #[test]
    fn reset_restores_defaults() {
        let mut store = ConfigStore::load(Box::new(FakeConfigFile::with(SCROLL_OFF)));
        assert_eq!(store.reset(), Ok(Config::default()));
        assert_eq!(store.current(), &Config::default());
    }

    #[test]
    fn dismissing_clears_the_recovery_notice() {
        let mut store = ConfigStore::load(Box::new(FakeConfigFile::with("{ broken")));
        store.dismiss_recovery();
        assert_eq!(store.recovery(), None);
    }

    #[test]
    fn a_saved_change_clears_a_failed_backup_notice() {
        let file = FakeConfigFile::with("{ broken");
        file.fail_backups();
        let mut store = ConfigStore::load(Box::new(file.clone()));
        assert_eq!(store.recovery(), Some(&Recovery::BackupFailed));

        store.update(scroll_off()).unwrap();

        assert_eq!(
            store.recovery(),
            None,
            "the unreadable file was overwritten"
        );
    }

    #[test]
    fn a_failed_save_keeps_the_failed_backup_notice() {
        let file = FakeConfigFile::with("{ broken");
        file.fail_backups();
        file.fail_writes();
        let mut store = ConfigStore::load(Box::new(file.clone()));

        assert!(store.update(scroll_off()).is_err());

        assert_eq!(store.recovery(), Some(&Recovery::BackupFailed));
    }

    #[test]
    fn a_saved_change_keeps_a_kept_backup_notice_until_dismissed() {
        let mut store = ConfigStore::load(Box::new(FakeConfigFile::with("{ broken")));

        store.update(scroll_off()).unwrap();

        assert_eq!(
            store.recovery(),
            Some(&Recovery::BackedUp {
                file_name: "config.1760000000.bak.json".into()
            })
        );
    }

    #[test]
    fn errors_serialize_with_a_kind_tag() {
        assert_eq!(
            serde_json::to_value(ConfigError::Io {
                message: "disk full".into()
            })
            .unwrap(),
            serde_json::json!({ "kind": "Io", "message": "disk full" })
        );
        assert_eq!(
            serde_json::to_value(ConfigError::UnsupportedVersion {
                found: 7,
                expected: 1
            })
            .unwrap(),
            serde_json::json!({ "kind": "UnsupportedVersion", "found": 7, "expected": 1 })
        );
    }
}
