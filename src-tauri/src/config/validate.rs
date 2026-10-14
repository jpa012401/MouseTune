//! Remap rules shared by the save path (rejects) and `compile` (skips) (spec §10).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
    actions::keys::is_modifier_key,
    config::schema::{
        Action, Buttons, DragEffect, KeyModifier, Remap, ScrollModifiers, TIMING_MS, Timing,
        TriggerDuration,
    },
    input::remaps::{MAX_BUTTONS, MAX_LEVEL},
};

/// The first key bound to two scroll effects, if any (spec §8.5).
pub fn duplicate_scroll_modifier(m: &ScrollModifiers) -> Option<KeyModifier> {
    let keys = [m.horizontal, m.precise, m.swift, m.zoom];
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
    check_action(&remap.effect.action)
}

/// The rules for an action wherever it appears (remaps, separate drag actions).
pub fn check_action(action: &Action) -> Result<(), RemapProblem> {
    if let Action::KeyCombo { key_code, .. } = action
        && (is_modifier_key(*key_code) || *key_code >= 128)
    {
        return Err(RemapProblem::EmptyKeyCombo);
    }
    if let Action::MouseClick { clicks, .. } = action
        && !(1..=2).contains(clicks)
    {
        return Err(RemapProblem::ClicksOutOfRange);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum BindingList {
    Drags,
    ScrollBindings,
    HoldThrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum BindingProblem {
    ButtonNotRemappable,
    ButtonOutOfRange,
    Duplicate,
    HoldThroughConflict,
    EmptyDiscrete,
    InvalidAction,
}

fn check_button(button: u8) -> Result<(), BindingProblem> {
    if button < FIRST_REMAPPABLE_BUTTON {
        Err(BindingProblem::ButtonNotRemappable)
    } else if usize::from(button) > MAX_BUTTONS {
        Err(BindingProblem::ButtonOutOfRange)
    } else {
        Ok(())
    }
}

fn check_drag(effect: &DragEffect) -> Result<(), BindingProblem> {
    if let DragEffect::Discrete {
        left,
        right,
        up,
        down,
    } = effect
    {
        let actions = [left, right, up, down];
        if actions.iter().all(|a| a.is_none()) {
            return Err(BindingProblem::EmptyDiscrete);
        }
        if actions
            .iter()
            .flat_map(|a| a.as_ref())
            .any(|a| check_action(a).is_err())
        {
            return Err(BindingProblem::InvalidAction);
        }
    }
    Ok(())
}

/// The first problem in the drag, scroll and hold-through lists.
/// `compile` skips the same entries instead.
pub fn validate_bindings(b: &Buttons) -> Result<(), (BindingList, usize, BindingProblem)> {
    fn each<T>(
        list: BindingList,
        items: &[T],
        button: impl Fn(&T) -> u8,
        extra: impl Fn(&T) -> Result<(), BindingProblem>,
    ) -> Result<(), (BindingList, usize, BindingProblem)> {
        let mut seen = HashSet::new();
        for (i, item) in items.iter().enumerate() {
            let b = button(item);
            check_button(b)
                .and_then(|()| extra(item))
                .map_err(|p| (list, i, p))?;
            if !seen.insert(b) {
                return Err((list, i, BindingProblem::Duplicate));
            }
        }
        Ok(())
    }
    each(
        BindingList::Drags,
        &b.drags,
        |d| d.button,
        |d| check_drag(&d.effect),
    )?;
    each(
        BindingList::ScrollBindings,
        &b.scroll_bindings,
        |s| s.button,
        |_| Ok(()),
    )?;
    each(
        BindingList::HoldThrough,
        &b.hold_through,
        |h| h.button,
        |h| {
            let taken = b.remaps.iter().any(|r| r.trigger.button == h.button)
                || b.drags.iter().any(|d| d.button == h.button)
                || b.scroll_bindings.iter().any(|s| s.button == h.button);
            if taken {
                Err(BindingProblem::HoldThroughConflict)
            } else {
                Ok(())
            }
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum TimingField {
    Hold,
    DoubleClick,
}

/// The first value that differs from `old` and lies outside `TIMING_MS`, if
/// any. An unchanged value isn't checked, so a
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
            zoom: None,
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
    #[test]
    fn the_zoom_key_counts_as_a_scroll_effect_binding() {
        use crate::config::schema::KeyModifier::{Command, Shift};
        let m = ScrollModifiers {
            horizontal: Some(Shift),
            precise: None,
            swift: None,
            zoom: Some(Shift),
        };
        assert_eq!(duplicate_scroll_modifier(&m), Some(Shift));
        let free = ScrollModifiers {
            zoom: Some(Command),
            ..m
        };
        assert_eq!(duplicate_scroll_modifier(&free), None);
    }

    use crate::config::schema::{
        Buttons, ClickTrigger, DragBinding, DragEffect, Effect, HoldThrough, MouseButton,
        ScrollBinding, ScrollEffect, SymbolicHotkeyId,
    };

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

    fn buttons(f: impl FnOnce(&mut Buttons)) -> Buttons {
        let mut b = Buttons {
            remaps: vec![],
            drags: vec![],
            scroll_bindings: vec![],
            hold_through: vec![],
        };
        f(&mut b);
        b
    }

    fn drag(button: u8) -> DragBinding {
        DragBinding {
            button,
            effect: DragEffect::SpacesAndMissionControl,
        }
    }

    #[test]
    fn binding_buttons_must_be_3_to_32() {
        assert_eq!(
            validate_bindings(&buttons(|b| b.drags = vec![drag(2)])),
            Err((BindingList::Drags, 0, BindingProblem::ButtonNotRemappable))
        );
        assert_eq!(
            validate_bindings(&buttons(|b| b.drags = vec![drag(33)])),
            Err((BindingList::Drags, 0, BindingProblem::ButtonOutOfRange))
        );
        assert_eq!(
            validate_bindings(&buttons(|b| b.drags = vec![drag(3), drag(32)])),
            Ok(())
        );
    }

    #[test]
    fn one_binding_per_button_per_list() {
        let twice = buttons(|b| {
            b.scroll_bindings = vec![
                ScrollBinding {
                    button: 4,
                    effect: ScrollEffect::Zoom,
                },
                ScrollBinding {
                    button: 4,
                    effect: ScrollEffect::Horizontal,
                },
            ]
        });
        assert_eq!(
            validate_bindings(&twice),
            Err((BindingList::ScrollBindings, 1, BindingProblem::Duplicate))
        );
    }

    #[test]
    fn a_hold_through_button_can_have_no_other_binding() {
        let clash = buttons(|b| {
            b.drags = vec![drag(6)];
            b.hold_through = vec![HoldThrough {
                button: 6,
                acts_as: MouseButton::Middle,
            }];
        });
        assert_eq!(
            validate_bindings(&clash),
            Err((
                BindingList::HoldThrough,
                0,
                BindingProblem::HoldThroughConflict
            ))
        );
        let with_remap = buttons(|b| {
            b.remaps = vec![remap(6, 1, TriggerDuration::Click)];
            b.hold_through = vec![HoldThrough {
                button: 6,
                acts_as: MouseButton::Left,
            }];
        });
        assert_eq!(
            validate_bindings(&with_remap),
            Err((
                BindingList::HoldThrough,
                0,
                BindingProblem::HoldThroughConflict
            ))
        );
    }

    #[test]
    fn separate_actions_need_at_least_one_valid_direction() {
        let empty = buttons(|b| {
            b.drags = vec![DragBinding {
                button: 5,
                effect: DragEffect::Discrete {
                    left: None,
                    right: None,
                    up: None,
                    down: None,
                },
            }]
        });
        assert_eq!(
            validate_bindings(&empty),
            Err((BindingList::Drags, 0, BindingProblem::EmptyDiscrete))
        );
        let bad = buttons(|b| {
            b.drags = vec![DragBinding {
                button: 5,
                effect: DragEffect::Discrete {
                    left: Some(Action::MouseClick {
                        button: MouseButton::Left,
                        clicks: 3,
                    }),
                    right: None,
                    up: None,
                    down: None,
                },
            }]
        });
        assert_eq!(
            validate_bindings(&bad),
            Err((BindingList::Drags, 0, BindingProblem::InvalidAction))
        );
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
