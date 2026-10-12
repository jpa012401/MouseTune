use super::nav::{SharedNavOverrides, nav_strategy};
use super::{
    CompiledAction, Job, NoticeObserver, OutputError, deliver,
    keys::{KeyBinding, KeyStroke, ModifierMask, key_combo_strokes},
};
use crate::config::schema::{NavDirection, NavStrategy, SymbolicHotkeyId, SystemKey};

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
    /// A full click at the cursor; `clicks` = 2 is a double click.
    fn click(&self, cg_button: u8, clicks: u8) -> Result<(), OutputError>;
    /// A media or brightness key, down then up.
    fn post_system_key(&self, key: SystemKey) -> Result<(), OutputError>;
}

pub trait FrontmostApp: Send {
    fn bundle_id(&self) -> Option<String>;
}

pub struct Outputs {
    pub hotkeys: Option<Box<dyn SymbolicHotkeys>>,
    pub poster: Box<dyn EventPoster>,
    pub frontmost: Box<dyn FrontmostApp>,
    pub notices: Box<dyn NoticeObserver>,
    pub nav: SharedNavOverrides,
}

const NO_KEY: u16 = 0xFFFF;

/// Every hotkey the picker can offer, in picker order. Tests walk it so a
/// new variant must get a table row.
#[cfg(test)]
pub const ALL_HOTKEYS: [SymbolicHotkeyId; 25] = {
    use SymbolicHotkeyId::*;
    [
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
    ]
};

const fn key(key_code: u16, modifiers: ModifierMask) -> Option<KeyBinding> {
    Some(KeyBinding {
        key_code,
        modifiers,
    })
}

/// A hotkey's macOS symbolic hotkey ID (spec §7.1) and the shortcut macOS
/// ships for it, posted only when the CGS functions are missing or fail. The
/// defaults are unverified on hardware, apart from Mission Control and Show
/// Desktop (Phase 4a). `None` = no default shortcut, so the action needs CGS.
pub fn hotkey(id: SymbolicHotkeyId) -> (u16, Option<KeyBinding>) {
    use SymbolicHotkeyId::*;
    let none = ModifierMask(0);
    match id {
        MissionControl => (32, key(0x7E, ModifierMask::CONTROL)),
        AppExpose => (33, key(0x7D, ModifierMask::CONTROL)),
        ShowDesktop => (36, key(0x67, none)),
        Launchpad => (160, None),
        LookUp => (70, key(0x02, ModifierMask::CONTROL | ModifierMask::COMMAND)),
        Spotlight => (64, key(0x31, ModifierMask::COMMAND)),
        NotificationCenter => (163, None),
        SpaceLeft => (79, key(0x7B, ModifierMask::CONTROL)),
        SpaceRight => (81, key(0x7C, ModifierMask::CONTROL)),
        Desktop1 => (118, None),
        Desktop2 => (119, None),
        Desktop3 => (120, None),
        Desktop4 => (121, None),
        Desktop5 => (122, None),
        Desktop6 => (123, None),
        Desktop7 => (124, None),
        Desktop8 => (125, None),
        Desktop9 => (126, None),
        Desktop10 => (127, None),
        Desktop11 => (128, None),
        Desktop12 => (129, None),
        Desktop13 => (130, None),
        Desktop14 => (131, None),
        Desktop15 => (132, None),
        Desktop16 => (133, None),
    }
}

pub fn perform(job: Job, out: &Outputs) -> Result<(), OutputError> {
    match job {
        Job::Click { button } => out.poster.click(button, 1),
        Job::Perform(CompiledAction::SymbolicHotkey(id)) => symbolic_hotkey(id, out),
        Job::Perform(CompiledAction::Navigate(direction)) => navigate(direction, out),
        Job::Perform(CompiledAction::KeyCombo(binding)) => post(binding, out),
        Job::Perform(CompiledAction::MouseClick { cg_button, clicks }) => {
            out.poster.click(cg_button, clicks)
        }
        Job::Perform(CompiledAction::SystemKey(key)) => out.poster.post_system_key(key),
        Job::Notice(notice) => {
            deliver(notice, out.notices.as_ref());
            Ok(())
        }
    }
}

/// A disabled or key-less hotkey is given a binding no keyboard can type, so
/// posting it triggers the action without a visible shortcut (spec §7.2).
fn symbolic_hotkey(id: SymbolicHotkeyId, out: &Outputs) -> Result<(), OutputError> {
    let (n, fallback) = hotkey(id);
    let post_fallback = || fallback.map_or(Err(OutputError::NoShortcut), |b| post(b, out));
    let Some(hotkeys) = &out.hotkeys else {
        return post_fallback();
    };
    // A failed read says nothing about the user's binding; installing over it
    // could replace a working shortcut, so fall back to the default instead.
    let binding = match hotkeys.read(n) {
        None => return post_fallback(),
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
    let bundle_id = out.frontmost.bundle_id();
    let strategy = nav_strategy(&out.nav.load(), bundle_id.as_deref());
    match (strategy, direction) {
        (NavStrategy::CmdBracket, NavDirection::Back) => post(
            KeyBinding {
                key_code: 0x21,
                modifiers: ModifierMask::COMMAND,
            },
            out,
        ),
        (NavStrategy::CmdBracket, NavDirection::Forward) => post(
            KeyBinding {
                key_code: 0x1E,
                modifiers: ModifierMask::COMMAND,
            },
            out,
        ),
        // Mouse buttons 4 and 5 are CG buttons 3 and 4.
        (NavStrategy::MouseButtons, NavDirection::Back) => out.poster.click(3, 1),
        (NavStrategy::MouseButtons, NavDirection::Forward) => out.poster.click(4, 1),
    }
}

fn post(binding: KeyBinding, out: &Outputs) -> Result<(), OutputError> {
    out.poster.post_keys(&key_combo_strokes(binding))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::nav::shared_nav_overrides;
    use crate::config::schema::Config;
    use crate::test_support::{FakeFrontmost, FakeHotkeys, FakeNotices, FakePoster, OutputLog};

    fn rig(hotkeys: Option<FakeHotkeys>, app: Option<&'static str>, log: &OutputLog) -> Outputs {
        Outputs {
            hotkeys: hotkeys.map(|h| Box::new(h) as Box<dyn SymbolicHotkeys>),
            poster: Box::new(FakePoster {
                log: log.clone(),
                panics: false,
            }),
            frontmost: Box::new(FakeFrontmost(app)),
            notices: Box::new(FakeNotices(log.clone())),
            nav: shared_nav_overrides(&Config::default()),
        }
    }
    fn rig_with(nav: &[(&str, NavStrategy)], app: &'static str, log: &OutputLog) -> Outputs {
        let mut config = Config::default();
        for (id, strategy) in nav {
            config.nav_overrides.insert((*id).to_owned(), *strategy);
        }
        Outputs {
            nav: shared_nav_overrides(&config),
            ..rig(None, Some(app), log)
        }
    }
    fn hotkeys(log: &OutputLog, state: Option<HotkeyState>) -> FakeHotkeys {
        FakeHotkeys {
            log: log.clone(),
            state,
            install_fails: false,
        }
    }
    const MC: Job = Job::Perform(CompiledAction::SymbolicHotkey(
        SymbolicHotkeyId::MissionControl,
    ));

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
                "key 0x7e down 0xa40000",
                "key 0x7e up 0xa40000",
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
        let job = Job::Perform(CompiledAction::SymbolicHotkey(
            SymbolicHotkeyId::ShowDesktop,
        ));
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
                "key 0x7e down 0xa40000",
                "key 0x7e up 0xa40000",
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
                "key 0x7e down 0xa40000",
                "key 0x7e up 0xa40000",
                "key 0x3b up 0x0"
            ]
        );
    }

    #[test]
    fn show_desktop_without_cgs_symbols_posts_f11() {
        let log = OutputLog::default();
        let job = Job::Perform(CompiledAction::SymbolicHotkey(
            SymbolicHotkeyId::ShowDesktop,
        ));
        perform(job, &rig(None, None, &log)).unwrap();
        assert_eq!(
            log.all(),
            ["key 0x67 down 0x800000", "key 0x67 up 0x800000"]
        );
    }

    #[test]
    fn space_right_without_cgs_symbols_posts_ctrl_right_arrow_with_its_key_flags() {
        let log = OutputLog::default();
        let job = Job::Perform(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::SpaceRight));
        perform(job, &rig(None, None, &log)).unwrap();
        assert_eq!(
            log.all(),
            [
                "key 0x3b down 0x40000",
                "key 0x7c down 0xa40000",
                "key 0x7c up 0xa40000",
                "key 0x3b up 0x0"
            ]
        );
    }

    #[test]
    fn forward_in_finder_is_cmd_right_bracket() {
        let log = OutputLog::default();
        perform(
            Job::Perform(CompiledAction::Navigate(NavDirection::Forward)),
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
            Job::Perform(CompiledAction::Navigate(NavDirection::Back)),
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
    fn an_override_makes_back_a_key_combo_in_xcode() {
        let log = OutputLog::default();
        perform(
            Job::Perform(CompiledAction::Navigate(NavDirection::Back)),
            &rig_with(
                &[("com.apple.dt.Xcode", NavStrategy::CmdBracket)],
                "com.apple.dt.Xcode",
                &log,
            ),
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
    fn a_default_app_set_to_mouse_buttons_uses_the_buttons() {
        let log = OutputLog::default();
        perform(
            Job::Perform(CompiledAction::Navigate(NavDirection::Back)),
            &rig_with(
                &[("com.apple.Safari", NavStrategy::MouseButtons)],
                "com.apple.Safari",
                &log,
            ),
        )
        .unwrap();
        assert_eq!(log.all(), ["click 3"]);
    }

    #[test]
    fn forward_in_chrome_is_mouse_button_5() {
        let log = OutputLog::default();
        perform(
            Job::Perform(CompiledAction::Navigate(NavDirection::Forward)),
            &rig(None, Some("com.google.Chrome"), &log),
        )
        .unwrap();
        assert_eq!(log.all(), ["click 4"]);
    }

    #[test]
    fn back_in_xcode_and_notes_is_mouse_button_4_not_a_key_combo() {
        for app in ["com.apple.dt.Xcode", "com.apple.Notes"] {
            let log = OutputLog::default();
            perform(
                Job::Perform(CompiledAction::Navigate(NavDirection::Back)),
                &rig(None, Some(app), &log),
            )
            .unwrap();
            assert_eq!(log.all(), ["click 3"], "{app}");
        }
    }

    #[test]
    fn unknown_frontmost_app_uses_the_mouse_button() {
        let log = OutputLog::default();
        perform(
            Job::Perform(CompiledAction::Navigate(NavDirection::Back)),
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

    #[test]
    fn hotkey_ids_match_the_spec_table() {
        use SymbolicHotkeyId::*;
        let ids: Vec<(SymbolicHotkeyId, u16)> =
            ALL_HOTKEYS.iter().map(|&h| (h, hotkey(h).0)).collect();
        assert_eq!(
            ids,
            vec![
                (MissionControl, 32),
                (AppExpose, 33),
                (ShowDesktop, 36),
                (Launchpad, 160),
                (LookUp, 70),
                (Spotlight, 64),
                (NotificationCenter, 163),
                (SpaceLeft, 79),
                (SpaceRight, 81),
                (Desktop1, 118),
                (Desktop2, 119),
                (Desktop3, 120),
                (Desktop4, 121),
                (Desktop5, 122),
                (Desktop6, 123),
                (Desktop7, 124),
                (Desktop8, 125),
                (Desktop9, 126),
                (Desktop10, 127),
                (Desktop11, 128),
                (Desktop12, 129),
                (Desktop13, 130),
                (Desktop14, 131),
                (Desktop15, 132),
                (Desktop16, 133),
            ]
        );
    }

    #[test]
    fn spotlight_without_cgs_falls_back_to_command_space() {
        let log = OutputLog::default();
        perform(
            Job::Perform(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::Spotlight)),
            &rig(None, None, &log),
        )
        .unwrap();
        assert_eq!(
            log.all(),
            [
                "key 0x37 down 0x100000",
                "key 0x31 down 0x100000",
                "key 0x31 up 0x100000",
                "key 0x37 up 0x0"
            ]
        );
    }

    #[test]
    fn launchpad_without_cgs_has_no_fallback_and_posts_nothing() {
        let log = OutputLog::default();
        let job = Job::Perform(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::Launchpad));
        assert_eq!(
            perform(job, &rig(None, None, &log)),
            Err(OutputError::NoShortcut)
        );
        assert!(log.all().is_empty());
    }

    #[test]
    fn launchpad_with_cgs_uses_its_own_id() {
        let log = OutputLog::default();
        let state = HotkeyState {
            enabled: false,
            binding: KeyBinding {
                key_code: 0xFFFF,
                modifiers: ModifierMask(0),
            },
        };
        perform(
            Job::Perform(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::Launchpad)),
            &rig(Some(hotkeys(&log, Some(state))), None, &log),
        )
        .unwrap();
        assert_eq!(
            log.all()[..2],
            ["read 160", "install 160 key 560 mods 0xa00000"]
        );
    }

    #[test]
    fn a_failed_read_of_a_hotkey_without_fallback_posts_nothing() {
        let log = OutputLog::default();
        let job = Job::Perform(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::Desktop3));
        assert_eq!(
            perform(job, &rig(Some(hotkeys(&log, None)), None, &log)),
            Err(OutputError::NoShortcut)
        );
        assert_eq!(log.all(), ["read 120"]);
    }

    #[test]
    fn a_key_combo_posts_its_modifiers_around_the_key() {
        let log = OutputLog::default();
        let job = Job::Perform(CompiledAction::KeyCombo(KeyBinding {
            key_code: 0x15,
            modifiers: ModifierMask(0x120000),
        }));
        perform(job, &rig(None, None, &log)).unwrap();
        assert_eq!(
            log.all(),
            [
                "key 0x38 down 0x20000",
                "key 0x37 down 0x120000",
                "key 0x15 down 0x120000",
                "key 0x15 up 0x120000",
                "key 0x37 up 0x20000",
                "key 0x38 up 0x0",
            ]
        );
    }

    #[test]
    fn a_double_middle_click_remap_clicks_twice() {
        let log = OutputLog::default();
        perform(
            Job::Perform(CompiledAction::MouseClick {
                cg_button: 2,
                clicks: 2,
            }),
            &rig(None, None, &log),
        )
        .unwrap();
        assert_eq!(log.all(), ["click 2 x2"]);
    }

    #[test]
    fn a_media_key_remap_posts_the_system_key() {
        let log = OutputLog::default();
        perform(
            Job::Perform(CompiledAction::SystemKey(
                crate::config::schema::SystemKey::PlayPause,
            )),
            &rig(None, None, &log),
        )
        .unwrap();
        assert_eq!(log.all(), ["system key PlayPause"]);
    }
}
