//! Which held keys apply each scroll effect, as `CGEventFlags` bitmasks the
//! tap callback can test (Phase 5b design §1–§2). Compiled whenever the
//! settings change, never inside a callout.

use crate::{
    actions::modifier_mask,
    config::schema::{KeyModifier, ScrollModifiers},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModifierKeys {
    pub horizontal: u64,
    pub precise: u64,
    pub swift: u64,
    /// Every bound key; a held key outside this passes the tick through.
    pub bound: u64,
}

impl ModifierKeys {
    /// A key bound twice (a hand-edited file) counts for its first effect
    /// only: horizontal, then precise, then swift.
    pub fn compile(m: &ScrollModifiers) -> Self {
        let mut bound = 0;
        let mut take = |key: Option<KeyModifier>| {
            let mask = key.map_or(0, |k| modifier_mask(&[k]).0);
            if mask & bound != 0 {
                return 0;
            }
            bound |= mask;
            mask
        };
        let horizontal = take(m.horizontal);
        let precise = take(m.precise);
        let swift = take(m.swift);
        Self {
            horizontal,
            precise,
            swift,
            bound,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::KeyModifier::{Control, Option, Shift};

    // CGEventFlags: Shift 0x2_0000, Control 0x4_0000, Option 0x8_0000, Command 0x10_0000

    #[test]
    fn the_defaults_bind_shift_and_option() {
        assert_eq!(
            ModifierKeys::compile(&ScrollModifiers::default()),
            ModifierKeys {
                horizontal: 0x2_0000,
                precise: 0x8_0000,
                swift: 0,
                bound: 0xA_0000
            }
        );
    }

    #[test]
    fn nothing_bound_compiles_to_no_bits() {
        let none = ScrollModifiers {
            horizontal: None,
            precise: None,
            swift: None,
            zoom: None,
        };
        assert_eq!(ModifierKeys::compile(&none), ModifierKeys::default());
    }

    #[test]
    fn a_key_bound_twice_counts_for_the_first_effect_only() {
        let clash = ScrollModifiers {
            horizontal: Some(Option),
            precise: Some(Option),
            swift: Some(Control),
            zoom: None,
        };
        assert_eq!(
            ModifierKeys::compile(&clash),
            ModifierKeys {
                horizontal: 0x8_0000,
                precise: 0,
                swift: 0x4_0000,
                bound: 0xC_0000
            }
        );
        let late = ScrollModifiers {
            horizontal: None,
            precise: Some(Shift),
            swift: Some(Shift),
            zoom: None,
        };
        assert_eq!(ModifierKeys::compile(&late).swift, 0);
    }
}
