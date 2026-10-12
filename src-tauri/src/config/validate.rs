//! Remap rules shared by the save path (rejects) and `compile` (skips) (spec §10).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    actions::keys::is_modifier_key,
    config::schema::{
        Action, KeyModifier, Remap, ScrollModifiers, TIMING_MS, Timing, TriggerDuration,
    },
    input::remaps::{MAX_BUTTONS, MAX_LEVEL},
};

/// The first key bound to two scroll effects, if any (Phase 5b design §1).
pub fn duplicate_scroll_modifier(m: &ScrollModifiers) -> Option<KeyModifier> {
    let keys = [m.horizontal, m.precise, m.swift];
    keys.iter()
        .enumerate()
        .find_map(|(i, key)| key.filter(|k| keys[i + 1..].contains(&Some(*k))))
}

/// 1-based: left (1) and right (2) stay with macOS.
pub const FIRST_REMAPPABLE_BUTTON: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum RemapProblem {
    DuplicateTrigger,
    ButtonNotRemappable,
    ButtonOutOfRange,
    LevelOutOfRange,
    EmptyKeyCombo,
    ClicksOutOfRange,
}

/// The per-entry rules shared with `compile` (which skips what fails).
pub fn check_remap(remap: &Remap) -> Result<(), RemapProblem> {
    let t = remap.trigger;
    if t.button < FIRST_REMAPPABLE_BUTTON {
        return Err(RemapProblem::ButtonNotRemappable);
    }
    if usize::from(t.button) > MAX_BUTTONS {
        return Err(RemapProblem::ButtonOutOfRange);
    }
    if !(1..=MAX_LEVEL).contains(&t.level) {
        return Err(RemapProblem::LevelOutOfRange);
    }
    if let Action::KeyCombo { key_code, .. } = remap.effect.action
        && (is_modifier_key(key_code) || key_code >= 128)
    {
        return Err(RemapProblem::EmptyKeyCombo);
    }
    if let Action::MouseClick { clicks, .. } = remap.effect.action
        && !(1..=2).contains(&clicks)
    {
        return Err(RemapProblem::ClicksOutOfRange);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TimingField {
    Hold,
    DoubleClick,
}

/// The first value that differs from `old` and lies outside `TIMING_MS`, if
/// any (Phase 4c design §1). An unchanged value isn't checked, so a
/// hand-edited out-of-range one doesn't block saving the other.
pub fn check_timing(new: &Timing, old: &Timing) -> Result<(), (TimingField, u32)> {
    for (field, ms, was) in [
        (TimingField::Hold, new.hold_ms, old.hold_ms),
        (
            TimingField::DoubleClick,
            new.double_click_ms,
            old.double_click_ms,
        ),
    ] {
        if ms != was && !TIMING_MS.contains(&ms) {
            return Err((field, ms));
        }
    }
    Ok(())
}

/// All rules, including duplicates; the first problem wins.
pub fn validate_remaps(remaps: &[Remap]) -> Result<(), (usize, RemapProblem)> {
    let mut seen: HashSet<(u8, u8, TriggerDuration)> = HashSet::new();
    for (index, remap) in remaps.iter().enumerate() {
        check_remap(remap).map_err(|problem| (index, problem))?;
        let t = remap.trigger;
        if !seen.insert((t.button, t.level, t.duration)) {
            return Err((index, RemapProblem::DuplicateTrigger));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_bound_to_two_scroll_effects_is_found() {
        use crate::config::schema::{KeyModifier::*, ScrollModifiers};
        let m = |horizontal, precise, swift| ScrollModifiers {
            horizontal,
            precise,
            swift,
        };
        assert_eq!(
            duplicate_scroll_modifier(&m(Some(Shift), Some(Option), None)),
            None
        );
        assert_eq!(duplicate_scroll_modifier(&m(None, None, None)), None);
        assert_eq!(
            duplicate_scroll_modifier(&m(Some(Option), Some(Option), None)),
            Some(Option)
        );
        assert_eq!(
            duplicate_scroll_modifier(&m(Some(Control), None, Some(Control))),
            Some(Control)
        );
        assert_eq!(
            duplicate_scroll_modifier(&m(None, Some(Command), Some(Command))),
            Some(Command)
        );
    }
    use crate::config::schema::{Buttons, ClickTrigger, Effect, SymbolicHotkeyId};

    fn remap(button: u8, level: u8, duration: TriggerDuration) -> Remap {
        Remap {
            trigger: ClickTrigger {
                button,
                level,
                duration,
            },
            effect: Effect {
                action: Action::SymbolicHotkey {
                    id: SymbolicHotkeyId::MissionControl,
                },
            },
        }
    }

    fn combo(key_code: u16) -> Remap {
        Remap {
            trigger: remap(4, 1, TriggerDuration::Click).trigger,
            effect: Effect {
                action: Action::KeyCombo {
                    key_code,
                    modifiers: Vec::new(),
                    key_label: String::new(),
                },
            },
        }
    }

    #[test]
    fn a_mouse_click_of_three_clicks_is_invalid() {
        use crate::config::schema::MouseButton;
        let mut r = remap(5, 1, TriggerDuration::Click);
        r.effect.action = Action::MouseClick {
            button: MouseButton::Left,
            clicks: 3,
        };
        assert_eq!(check_remap(&r), Err(RemapProblem::ClicksOutOfRange));
        r.effect.action = Action::MouseClick {
            button: MouseButton::Left,
            clicks: 2,
        };
        assert_eq!(check_remap(&r), Ok(()));
    }

    #[test]
    fn timing_outside_150_to_600_ms_is_named() {
        use crate::config::schema::Timing;
        let defaults = Timing {
            hold_ms: 250,
            double_click_ms: 260,
        };
        assert_eq!(
            check_timing(
                &Timing {
                    hold_ms: 150,
                    double_click_ms: 600
                },
                &defaults
            ),
            Ok(())
        );
        assert_eq!(
            check_timing(
                &Timing {
                    hold_ms: 149,
                    double_click_ms: 260
                },
                &defaults
            ),
            Err((TimingField::Hold, 149))
        );
        assert_eq!(
            check_timing(
                &Timing {
                    hold_ms: 250,
                    double_click_ms: 601
                },
                &defaults
            ),
            Err((TimingField::DoubleClick, 601))
        );
    }

    #[test]
    fn an_unchanged_out_of_range_timing_is_not_checked() {
        use crate::config::schema::Timing;
        let hand_edited = Timing {
            hold_ms: 50,
            double_click_ms: 260,
        };
        assert_eq!(
            check_timing(
                &Timing {
                    hold_ms: 50,
                    double_click_ms: 300
                },
                &hand_edited
            ),
            Ok(())
        );
    }

    #[test]
    fn the_defaults_are_valid() {
        assert_eq!(validate_remaps(&Buttons::default().remaps), Ok(()));
    }
    #[test]
    fn left_and_right_buttons_are_not_remappable() {
        assert_eq!(
            check_remap(&remap(2, 1, TriggerDuration::Click)),
            Err(RemapProblem::ButtonNotRemappable)
        );
    }
    #[test]
    fn buttons_above_32_are_out_of_range() {
        assert_eq!(
            check_remap(&remap(33, 1, TriggerDuration::Click)),
            Err(RemapProblem::ButtonOutOfRange)
        );
        assert_eq!(check_remap(&remap(32, 1, TriggerDuration::Click)), Ok(()));
    }
    #[test]
    fn levels_must_be_1_to_3() {
        assert_eq!(
            check_remap(&remap(3, 0, TriggerDuration::Click)),
            Err(RemapProblem::LevelOutOfRange)
        );
        assert_eq!(
            check_remap(&remap(3, 4, TriggerDuration::Hold)),
            Err(RemapProblem::LevelOutOfRange)
        );
        assert_eq!(check_remap(&remap(3, 3, TriggerDuration::Hold)), Ok(()));
    }
    #[test]
    fn a_shortcut_needs_a_real_key() {
        assert_eq!(check_remap(&combo(0x37)), Err(RemapProblem::EmptyKeyCombo)); // Command alone
        assert_eq!(check_remap(&combo(200)), Err(RemapProblem::EmptyKeyCombo));
        assert_eq!(check_remap(&combo(0x01)), Ok(()));
    }
    #[test]
    fn a_repeated_trigger_is_reported_at_its_second_index() {
        let remaps = vec![
            remap(4, 1, TriggerDuration::Click),
            remap(3, 1, TriggerDuration::Click),
            remap(4, 1, TriggerDuration::Click),
        ];
        assert_eq!(
            validate_remaps(&remaps),
            Err((2, RemapProblem::DuplicateTrigger))
        );
    }
    #[test]
    fn click_and_hold_on_the_same_level_are_different_triggers() {
        assert_eq!(
            validate_remaps(&[
                remap(3, 1, TriggerDuration::Click),
                remap(3, 1, TriggerDuration::Hold)
            ]),
            Ok(())
        );
    }
}
