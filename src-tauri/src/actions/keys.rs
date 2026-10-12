use std::ops::BitOr;

/// Values equal CGEventFlags, so adapters pass them straight through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModifierMask(pub u64);

impl ModifierMask {
    pub const SHIFT: Self = Self(0x20000);
    pub const CONTROL: Self = Self(0x40000);
    pub const OPTION: Self = Self(0x80000);
    pub const COMMAND: Self = Self(0x100000);
    pub const NUMERIC_PAD: Self = Self(0x200000);
    pub const SECONDARY_FN: Self = Self(0x800000);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for ModifierMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyBinding {
    pub key_code: u16,
    pub modifiers: ModifierMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    pub key_code: u16,
    pub down: bool,
    pub flags: ModifierMask,
}

/// Held in this order going down, released in reverse. Flag-only modifiers
/// (numeric pad, fn) have no key of their own; they ride on the key's flags.
const MODIFIER_KEYS: [(ModifierMask, u16); 4] = [
    (ModifierMask::CONTROL, 0x3B),
    (ModifierMask::OPTION, 0x3A),
    (ModifierMask::SHIFT, 0x38),
    (ModifierMask::COMMAND, 0x37),
];

/// macOS virtual key codes of modifier keys (Command, Shift, Option, Control, both sides; Caps Lock; Fn).
pub fn is_modifier_key(key_code: u16) -> bool {
    (0x36..=0x3F).contains(&key_code)
}

pub fn key_combo_strokes(binding: KeyBinding) -> Vec<KeyStroke> {
    let mods: Vec<(ModifierMask, u16)> = MODIFIER_KEYS
        .into_iter()
        .filter(|(m, _)| binding.modifiers.contains(*m))
        .collect();
    let mut strokes = Vec::new();
    let mut held = ModifierMask(0);
    for &(mask, code) in &mods {
        held = held | mask;
        strokes.push(KeyStroke {
            key_code: code,
            down: true,
            flags: held,
        });
    }
    for down in [true, false] {
        strokes.push(KeyStroke {
            key_code: binding.key_code,
            down,
            flags: binding.modifiers,
        });
    }
    for &(mask, code) in mods.iter().rev() {
        held = ModifierMask(held.0 & !mask.0);
        strokes.push(KeyStroke {
            key_code: code,
            down: false,
            flags: held,
        });
    }
    strokes
}

/// Keys whose typed characters are blank, invisible or layout-independent,
/// by macOS virtual key code.
const SPECIAL_KEYS: [(u16, &str); 35] = [
    (0x24, "↩"),
    (0x30, "⇥"),
    (0x31, "Space"),
    (0x33, "⌫"),
    (0x35, "Esc"),
    (0x4C, "⌤"),
    (0x75, "⌦"),
    (0x73, "Home"),
    (0x77, "End"),
    (0x74, "Page Up"),
    (0x79, "Page Down"),
    (0x7B, "←"),
    (0x7C, "→"),
    (0x7D, "↓"),
    (0x7E, "↑"),
    (0x7A, "F1"),
    (0x78, "F2"),
    (0x63, "F3"),
    (0x76, "F4"),
    (0x60, "F5"),
    (0x61, "F6"),
    (0x62, "F7"),
    (0x64, "F8"),
    (0x65, "F9"),
    (0x6D, "F10"),
    (0x67, "F11"),
    (0x6F, "F12"),
    (0x69, "F13"),
    (0x6B, "F14"),
    (0x71, "F15"),
    (0x6A, "F16"),
    (0x40, "F17"),
    (0x4F, "F18"),
    (0x50, "F19"),
    (0x5A, "F20"),
];

/// The label the UI shows for a captured key: a fixed name for special keys,
/// otherwise the typed characters upper-cased, or `Key {code}` when they
/// aren't printable.
pub fn key_label(key_code: u16, chars: &[u16]) -> String {
    if let Some((_, label)) = SPECIAL_KEYS.iter().find(|(code, _)| *code == key_code) {
        return (*label).to_string();
    }
    let typed = String::from_utf16_lossy(chars).trim().to_uppercase();
    if typed.is_empty() || typed.chars().any(char::is_control) {
        format!("Key {key_code}")
    } else {
        typed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printable_key_is_labelled_by_its_upper_cased_character() {
        assert_eq!(key_label(1, &[0x73]), "S");
    }

    #[test]
    fn special_keys_have_fixed_labels() {
        assert_eq!(key_label(0x7E, &[]), "↑");
        assert_eq!(key_label(0x31, &[0x20]), "Space");
        assert_eq!(key_label(0x60, &[]), "F5");
        assert_eq!(key_label(0x24, &[0x0D]), "↩");
    }

    #[test]
    fn printable_keys_use_their_character_uppercased() {
        assert_eq!(key_label(0x01, &[0x73]), "S");
        assert_eq!(key_label(0x15, &[0x34]), "4");
    }

    #[test]
    fn unknown_unprintable_keys_fall_back_to_their_code() {
        assert_eq!(key_label(0x47, &[0x10]), "Key 71");
        assert_eq!(key_label(0x47, &[]), "Key 71");
    }

    #[test]
    fn modifier_key_codes_are_recognised() {
        for code in 0x36..=0x3F {
            assert!(is_modifier_key(code), "{code:#x}");
        }
        assert!(!is_modifier_key(0x35));
    }

    #[test]
    fn cmd_bracket_presses_command_around_the_key() {
        let strokes = key_combo_strokes(KeyBinding {
            key_code: 0x21,
            modifiers: ModifierMask::COMMAND,
        });
        assert_eq!(
            strokes,
            vec![
                KeyStroke {
                    key_code: 0x37,
                    down: true,
                    flags: ModifierMask(0x100000)
                },
                KeyStroke {
                    key_code: 0x21,
                    down: true,
                    flags: ModifierMask(0x100000)
                },
                KeyStroke {
                    key_code: 0x21,
                    down: false,
                    flags: ModifierMask(0x100000)
                },
                KeyStroke {
                    key_code: 0x37,
                    down: false,
                    flags: ModifierMask(0)
                },
            ]
        );
    }

    #[test]
    fn flag_only_modifiers_ride_on_the_key_without_their_own_strokes() {
        let mods = ModifierMask::NUMERIC_PAD | ModifierMask::SECONDARY_FN;
        let strokes = key_combo_strokes(KeyBinding {
            key_code: 432,
            modifiers: mods,
        });
        assert_eq!(
            strokes,
            vec![
                KeyStroke {
                    key_code: 432,
                    down: true,
                    flags: ModifierMask(0xA00000)
                },
                KeyStroke {
                    key_code: 432,
                    down: false,
                    flags: ModifierMask(0xA00000)
                },
            ]
        );
    }

    #[test]
    fn several_modifiers_go_down_in_order_and_up_in_reverse() {
        let strokes = key_combo_strokes(KeyBinding {
            key_code: 0x11,
            modifiers: ModifierMask::COMMAND | ModifierMask::SHIFT,
        });
        let codes: Vec<(u16, bool, u64)> = strokes
            .iter()
            .map(|s| (s.key_code, s.down, s.flags.0))
            .collect();
        assert_eq!(
            codes,
            vec![
                (0x38, true, 0x20000),
                (0x37, true, 0x120000),
                (0x11, true, 0x120000),
                (0x11, false, 0x120000),
                (0x37, false, 0x20000),
                (0x38, false, 0),
            ]
        );
    }
}
