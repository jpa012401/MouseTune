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
    let key_flags = binding.modifiers | implied_flags(binding.key_code);
    for down in [true, false] {
        strokes.push(KeyStroke {
            key_code: binding.key_code,
            down,
            flags: key_flags,
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

/// Flags hardware sends with arrow keys.
const ARROW: ModifierMask = ModifierMask(0x200000 | 0x800000);
/// Flags hardware sends with function and navigation keys.
const FN: ModifierMask = ModifierMask::SECONDARY_FN;
const NONE: ModifierMask = ModifierMask(0);

/// What we know about a macOS virtual key code: the fixed label for keys whose
/// typed characters are blank, invisible or layout-independent, and the flags
/// hardware always sends with the key. The recorder strips those flags, so
/// replay adds them back; WindowServer matches its ⌃-arrow hotkeys with them.
const KEY_FACTS: [(u16, Option<&str>, ModifierMask); 36] = [
    (0x24, Some("↩"), NONE),
    (0x30, Some("⇥"), NONE),
    (0x31, Some("Space"), NONE),
    (0x33, Some("⌫"), NONE),
    (0x35, Some("Esc"), NONE),
    (0x4C, Some("⌤"), NONE),
    (0x75, Some("⌦"), FN),
    (0x72, None, FN), // Help
    (0x73, Some("Home"), FN),
    (0x77, Some("End"), FN),
    (0x74, Some("Page Up"), FN),
    (0x79, Some("Page Down"), FN),
    (0x7B, Some("←"), ARROW),
    (0x7C, Some("→"), ARROW),
    (0x7D, Some("↓"), ARROW),
    (0x7E, Some("↑"), ARROW),
    (0x7A, Some("F1"), FN),
    (0x78, Some("F2"), FN),
    (0x63, Some("F3"), FN),
    (0x76, Some("F4"), FN),
    (0x60, Some("F5"), FN),
    (0x61, Some("F6"), FN),
    (0x62, Some("F7"), FN),
    (0x64, Some("F8"), FN),
    (0x65, Some("F9"), FN),
    (0x6D, Some("F10"), FN),
    (0x67, Some("F11"), FN),
    (0x6F, Some("F12"), FN),
    (0x69, Some("F13"), FN),
    (0x6B, Some("F14"), FN),
    (0x71, Some("F15"), FN),
    (0x6A, Some("F16"), FN),
    (0x40, Some("F17"), FN),
    (0x4F, Some("F18"), FN),
    (0x50, Some("F19"), FN),
    (0x5A, Some("F20"), FN),
];

fn key_fact(key_code: u16) -> Option<&'static (u16, Option<&'static str>, ModifierMask)> {
    KEY_FACTS.iter().find(|(code, _, _)| *code == key_code)
}

/// The flags hardware sends with this key on its own (none for unknown codes).
pub fn implied_flags(key_code: u16) -> ModifierMask {
    key_fact(key_code).map_or(NONE, |(_, _, flags)| *flags)
}

/// The label the UI shows for a captured key: a fixed name for special keys,
/// otherwise the typed characters upper-cased, or `Key {code}` when they
/// aren't printable.
pub fn key_label(key_code: u16, chars: &[u16]) -> String {
    if let Some((_, Some(label), _)) = key_fact(key_code) {
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

    fn strokes(key_code: u16, modifiers: ModifierMask) -> Vec<(u16, bool, u64)> {
        key_combo_strokes(KeyBinding {
            key_code,
            modifiers,
        })
        .iter()
        .map(|s| (s.key_code, s.down, s.flags.0))
        .collect()
    }

    #[test]
    fn an_arrow_key_carries_the_numeric_pad_and_fn_flags_but_its_modifiers_do_not() {
        assert_eq!(
            strokes(0x7C, ModifierMask::CONTROL),
            vec![
                (0x3B, true, 0x40000),
                (0x7C, true, 0xA40000),
                (0x7C, false, 0xA40000),
                (0x3B, false, 0x0),
            ]
        );
    }

    #[test]
    fn a_function_key_carries_the_fn_flag() {
        assert_eq!(
            strokes(0x60, ModifierMask(0)),
            vec![(0x60, true, 0x800000), (0x60, false, 0x800000)]
        );
    }

    #[test]
    fn navigation_keys_carry_the_fn_flag() {
        for code in [0x73, 0x77, 0x74, 0x79, 0x75, 0x72] {
            assert_eq!(
                strokes(code, ModifierMask(0)),
                vec![(code, true, 0x800000), (code, false, 0x800000)],
                "{code:#x}"
            );
        }
    }

    #[test]
    fn a_letter_key_gets_no_extra_flags() {
        assert_eq!(
            strokes(0x01, ModifierMask(0)),
            vec![(0x01, true, 0x0), (0x01, false, 0x0)]
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
