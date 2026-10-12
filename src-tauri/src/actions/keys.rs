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

#[cfg(test)]
mod tests {
    use super::*;

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
