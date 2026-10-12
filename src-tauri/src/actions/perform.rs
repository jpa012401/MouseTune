use super::{
    Job, OutputError,
    keys::{KeyBinding, KeyStroke, ModifierMask, key_combo_strokes},
};
use crate::config::schema::{Action, NavDirection, SymbolicHotkeyId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyState {
    pub enabled: bool,
    pub binding: KeyBinding,
}

pub trait SymbolicHotkeys: Send {
    /// `None` = the call failed.
    fn read(&self, id: u16) -> Option<HotkeyState>;
    fn install(&self, id: u16, binding: KeyBinding) -> Result<(), OutputError>;
}

pub trait EventPoster: Send {
    fn post_keys(&self, strokes: &[KeyStroke]) -> Result<(), OutputError>;
    fn click(&self, cg_button: u8) -> Result<(), OutputError>;
}

pub trait FrontmostApp: Send {
    fn bundle_id(&self) -> Option<String>;
}

pub struct Outputs {
    pub hotkeys: Option<Box<dyn SymbolicHotkeys>>,
    pub poster: Box<dyn EventPoster>,
    pub frontmost: Box<dyn FrontmostApp>,
}

const NO_KEY: u16 = 0xFFFF;

/// Apps where Cmd+[ / Cmd+] means back / forward (spec §7.4).
const APPLE_NAV_APPS: [&str; 8] = [
    "com.apple.Safari",
    "com.apple.finder",
    "com.apple.dt.Xcode",
    "com.apple.Music",
    "com.apple.Notes",
    "com.apple.AppStore",
    "com.apple.systempreferences",
    "com.apple.Preview",
];

/// macOS symbolic hotkey IDs (spec §7.2).
fn hotkey_id(id: SymbolicHotkeyId) -> u16 {
    match id {
        SymbolicHotkeyId::MissionControl => 32,
        SymbolicHotkeyId::ShowDesktop => 36,
    }
}

/// macOS default shortcuts, used only when the CGS functions are missing.
/// Unverified on hardware.
fn default_shortcut(id: SymbolicHotkeyId) -> KeyBinding {
    match id {
        SymbolicHotkeyId::MissionControl => KeyBinding {
            key_code: 0x7E,
            modifiers: ModifierMask::CONTROL,
        },
        SymbolicHotkeyId::ShowDesktop => KeyBinding {
            key_code: 0x67,
            modifiers: ModifierMask(0),
        },
    }
}

pub fn perform(job: Job, out: &Outputs) -> Result<(), OutputError> {
    match job {
        Job::Click { button } => out.poster.click(button),
        Job::Perform(Action::SymbolicHotkey { id }) => symbolic_hotkey(id, out),
        Job::Perform(Action::Navigate { direction }) => navigate(direction, out),
    }
}

/// A disabled or key-less hotkey is given a binding no keyboard can type, so
/// posting it triggers the action without a visible shortcut (spec §7.2).
fn symbolic_hotkey(id: SymbolicHotkeyId, out: &Outputs) -> Result<(), OutputError> {
    let Some(hotkeys) = &out.hotkeys else {
        return post(default_shortcut(id), out);
    };
    let n = hotkey_id(id);
    // A failed read says nothing about the user's binding; installing over it
    // could replace a working shortcut, so fall back to the default instead.
    let binding = match hotkeys.read(n) {
        None => return post(default_shortcut(id), out),
        Some(s) if s.enabled && s.binding.key_code != NO_KEY => s.binding,
        Some(_) => {
            let unreachable = KeyBinding {
                key_code: 400 + n,
                modifiers: ModifierMask::NUMERIC_PAD | ModifierMask::SECONDARY_FN,
            };
            hotkeys.install(n, unreachable)?;
            unreachable
        }
    };
    post(binding, out)
}

fn navigate(direction: NavDirection, out: &Outputs) -> Result<(), OutputError> {
    let apple = out
        .frontmost
        .bundle_id()
        .is_some_and(|id| APPLE_NAV_APPS.contains(&id.as_str()));
    match (apple, direction) {
        (true, NavDirection::Back) => post(
            KeyBinding {
                key_code: 0x21,
                modifiers: ModifierMask::COMMAND,
            },
            out,
        ),
        (true, NavDirection::Forward) => post(
            KeyBinding {
                key_code: 0x1E,
                modifiers: ModifierMask::COMMAND,
            },
            out,
        ),
        // Mouse buttons 4 and 5 are CG buttons 3 and 4.
        (false, NavDirection::Back) => out.poster.click(3),
        (false, NavDirection::Forward) => out.poster.click(4),
    }
}

fn post(binding: KeyBinding, out: &Outputs) -> Result<(), OutputError> {
    out.poster.post_keys(&key_combo_strokes(binding))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeFrontmost, FakeHotkeys, FakePoster, OutputLog};

    fn rig(hotkeys: Option<FakeHotkeys>, app: Option<&'static str>, log: &OutputLog) -> Outputs {
        Outputs {
            hotkeys: hotkeys.map(|h| Box::new(h) as Box<dyn SymbolicHotkeys>),
            poster: Box::new(FakePoster {
                log: log.clone(),
                panics: false,
            }),
            frontmost: Box::new(FakeFrontmost(app)),
        }
    }
    fn hotkeys(log: &OutputLog, state: Option<HotkeyState>) -> FakeHotkeys {
        FakeHotkeys {
            log: log.clone(),
            state,
            install_fails: false,
        }
    }
    const MC: Job = Job::Perform(Action::SymbolicHotkey {
        id: SymbolicHotkeyId::MissionControl,
    });

    #[test]
    fn an_enabled_hotkey_posts_its_own_shortcut() {
        let log = OutputLog::default();
        let state = HotkeyState {
            enabled: true,
            binding: KeyBinding {
                key_code: 0x7E,
                modifiers: ModifierMask::CONTROL,
            },
        };
        perform(MC, &rig(Some(hotkeys(&log, Some(state))), None, &log)).unwrap();
        assert_eq!(
            log.all(),
            [
                "read 32",
                "key 0x3b down 0x40000",
                "key 0x7e down 0x40000",
                "key 0x7e up 0x40000",
                "key 0x3b up 0x0"
            ]
        );
    }

    #[test]
    fn a_disabled_hotkey_gets_the_unreachable_binding_then_posts_it() {
        let log = OutputLog::default();
        let state = HotkeyState {
            enabled: false,
            binding: KeyBinding {
                key_code: 0x7E,
                modifiers: ModifierMask::CONTROL,
            },
        };
        perform(MC, &rig(Some(hotkeys(&log, Some(state))), None, &log)).unwrap();
        assert_eq!(
            log.all(),
            [
                "read 32",
                "install 32 key 432 mods 0xa00000",
                "key 0x1b0 down 0xa00000",
                "key 0x1b0 up 0xa00000"
            ]
        );
    }

    #[test]
    fn a_hotkey_with_no_key_gets_the_unreachable_binding() {
        let log = OutputLog::default();
        let state = HotkeyState {
            enabled: true,
            binding: KeyBinding {
                key_code: 0xFFFF,
                modifiers: ModifierMask(0),
            },
        };
        let job = Job::Perform(Action::SymbolicHotkey {
            id: SymbolicHotkeyId::ShowDesktop,
        });
        perform(job, &rig(Some(hotkeys(&log, Some(state))), None, &log)).unwrap();
        assert_eq!(log.all()[1], "install 36 key 436 mods 0xa00000");
    }

    #[test]
    fn a_failed_install_is_an_error_and_posts_nothing() {
        let log = OutputLog::default();
        let mut h = hotkeys(
            &log,
            Some(HotkeyState {
                enabled: false,
                binding: KeyBinding {
                    key_code: 0xFFFF,
                    modifiers: ModifierMask(0),
                },
            }),
        );
        h.install_fails = true;
        assert_eq!(
            perform(MC, &rig(Some(h), None, &log)),
            Err(OutputError::Os("CGSSetSymbolicHotKeyValue"))
        );
        assert_eq!(log.all(), ["read 32", "install 32 key 432 mods 0xa00000"]);
    }

    #[test]
    fn missing_cgs_symbols_fall_back_to_the_default_shortcut() {
        let log = OutputLog::default();
        perform(MC, &rig(None, None, &log)).unwrap();
        assert_eq!(
            log.all(),
            [
                "key 0x3b down 0x40000",
                "key 0x7e down 0x40000",
                "key 0x7e up 0x40000",
                "key 0x3b up 0x0"
            ]
        );
    }

    #[test]
    fn a_failed_read_falls_back_to_the_default_shortcut() {
        let log = OutputLog::default();
        perform(MC, &rig(Some(hotkeys(&log, None)), None, &log)).unwrap();
        assert_eq!(
            log.all(),
            [
                "read 32",
                "key 0x3b down 0x40000",
                "key 0x7e down 0x40000",
                "key 0x7e up 0x40000",
                "key 0x3b up 0x0"
            ]
        );
    }

    #[test]
    fn show_desktop_without_cgs_symbols_posts_f11() {
        let log = OutputLog::default();
        let job = Job::Perform(Action::SymbolicHotkey {
            id: SymbolicHotkeyId::ShowDesktop,
        });
        perform(job, &rig(None, None, &log)).unwrap();
        assert_eq!(log.all(), ["key 0x67 down 0x0", "key 0x67 up 0x0"]);
    }

    #[test]
    fn forward_in_finder_is_cmd_right_bracket() {
        let log = OutputLog::default();
        perform(
            Job::Perform(Action::Navigate {
                direction: NavDirection::Forward,
            }),
            &rig(None, Some("com.apple.finder"), &log),
        )
        .unwrap();
        assert_eq!(
            log.all(),
            [
                "key 0x37 down 0x100000",
                "key 0x1e down 0x100000",
                "key 0x1e up 0x100000",
                "key 0x37 up 0x0"
            ]
        );
    }

    #[test]
    fn back_in_safari_is_cmd_left_bracket() {
        let log = OutputLog::default();
        perform(
            Job::Perform(Action::Navigate {
                direction: NavDirection::Back,
            }),
            &rig(None, Some("com.apple.Safari"), &log),
        )
        .unwrap();
        assert_eq!(
            log.all(),
            [
                "key 0x37 down 0x100000",
                "key 0x21 down 0x100000",
                "key 0x21 up 0x100000",
                "key 0x37 up 0x0"
            ]
        );
    }

    #[test]
    fn forward_in_chrome_is_mouse_button_5() {
        let log = OutputLog::default();
        perform(
            Job::Perform(Action::Navigate {
                direction: NavDirection::Forward,
            }),
            &rig(None, Some("com.google.Chrome"), &log),
        )
        .unwrap();
        assert_eq!(log.all(), ["click 4"]);
    }

    #[test]
    fn unknown_frontmost_app_uses_the_mouse_button() {
        let log = OutputLog::default();
        perform(
            Job::Perform(Action::Navigate {
                direction: NavDirection::Back,
            }),
            &rig(None, None, &log),
        )
        .unwrap();
        assert_eq!(log.all(), ["click 3"]);
    }

    #[test]
    fn a_click_job_posts_that_button() {
        let log = OutputLog::default();
        perform(Job::Click { button: 2 }, &rig(None, None, &log)).unwrap();
        assert_eq!(log.all(), ["click 2"]);
    }
}
