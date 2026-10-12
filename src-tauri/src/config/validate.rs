//! Remap rules shared by the save path (rejects) and `compile` (skips) (spec §10).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    actions::keys::is_modifier_key,
    config::schema::{Action, Remap, TriggerDuration},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum RemapProblem {
    DuplicateTrigger,
    ButtonNotRemappable,
    ButtonOutOfRange,
    LevelOutOfRange,
    EmptyKeyCombo,
}

/// The per-entry rules shared with `compile` (which skips what fails).
pub fn check_remap(remap: &Remap) -> Result<(), RemapProblem> {
    let t = remap.trigger;
    if t.button < 3 {
        return Err(RemapProblem::ButtonNotRemappable);
    }
    if t.button > 32 {
        return Err(RemapProblem::ButtonOutOfRange);
    }
    if !(1..=3).contains(&t.level) {
        return Err(RemapProblem::LevelOutOfRange);
    }
    if let Action::KeyCombo { key_code, .. } = remap.effect.action
        && (is_modifier_key(key_code) || key_code >= 128)
    {
        return Err(RemapProblem::EmptyKeyCombo);
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
