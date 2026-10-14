//! Compiles the remap list into fixed-size lookup tables the hot path can
//! index without allocating (spec §5.2, design §1). Converts 1-based config
//! button numbers to CG button numbers (middle = 2).

use std::time::Duration;

use crate::{
    actions::CompiledAction,
    config::{
        schema::{
            Action, Config, DragEffect, ScrollEffect, SymbolicHotkeyId, TIMING_MS,
            Timing as TimingConfig, TriggerDuration,
        },
        validate::{check_action, check_remap},
    },
};

pub const MAX_BUTTONS: usize = 32;
pub const MAX_LEVEL: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub hold: Duration,
    pub level_expiry: Duration,
}

impl From<&TimingConfig> for Timing {
    /// Clamped, so a hand-edited value outside the range still works
    /// (Phase 4c design §1).
    fn from(t: &TimingConfig) -> Self {
        let ms = |v: u32| {
            Duration::from_millis(u64::from(v.clamp(*TIMING_MS.start(), *TIMING_MS.end())))
        };
        Self {
            hold: ms(t.hold_ms),
            level_expiry: ms(t.double_click_ms),
        }
    }
}

/// Which private outputs are verified on this macOS (Phase 6 design §3).
/// Unverified ones compile to their documented fallbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSupport {
    pub dock_swipe: bool,
    pub magnify: bool,
}

impl OutputSupport {
    /// Every output on its fallback. Production always asks
    /// `platform::output_support`; tests start from this.
    #[cfg(test)]
    pub const NONE: Self = Self {
        dock_swipe: false,
        magnify: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeKind {
    SpacesAndMissionControl,
    ScrollSwipe,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiscreteActions {
    pub left: Option<CompiledAction>,
    pub right: Option<CompiledAction>,
    pub up: Option<CompiledAction>,
    pub down: Option<CompiledAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledDrag {
    Swipe(SwipeKind),
    Discrete(DiscreteActions),
}

/// What `SpacesAndMissionControl` becomes when dock swipes aren't verified.
/// The Space follows the hand like a three-finger swipe: dragging left pulls
/// the current Space away to the left and brings in the one on the right.
const SPACES_FALLBACK: DiscreteActions = DiscreteActions {
    left: Some(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::SpaceRight)),
    right: Some(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::SpaceLeft)),
    up: Some(CompiledAction::SymbolicHotkey(
        SymbolicHotkeyId::MissionControl,
    )),
    down: Some(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::AppExpose)),
};

/// One button's bindings, indexed by `level - 1`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonRemaps {
    click: [Option<CompiledAction>; MAX_LEVEL as usize],
    hold: [Option<CompiledAction>; MAX_LEVEL as usize],
    pub drag: Option<CompiledDrag>,
    pub scroll: Option<ScrollEffect>,
    /// The CG button this button presses while held (hold-through).
    pub acts_as: Option<u8>,
}

impl ButtonRemaps {
    fn slot(&mut self, level: u8, duration: TriggerDuration) -> &mut Option<CompiledAction> {
        let i = usize::from(level - 1);
        match duration {
            TriggerDuration::Click => &mut self.click[i],
            TriggerDuration::Hold => &mut self.hold[i],
        }
    }

    pub fn action(&self, level: u8, duration: TriggerDuration) -> Option<CompiledAction> {
        let i = usize::from(level.checked_sub(1)?);
        match duration {
            TriggerDuration::Click => *self.click.get(i)?,
            TriggerDuration::Hold => *self.hold.get(i)?,
        }
    }

    /// The highest level with any binding; 0 means the button is unused.
    pub fn max_click_level(&self) -> u8 {
        (1..=MAX_LEVEL)
            .rev()
            .find(|&level| {
                self.action(level, TriggerDuration::Click).is_some()
                    || self.action(level, TriggerDuration::Hold).is_some()
            })
            .unwrap_or(0)
    }

    /// Swallowed and tracked by the router: anything bound.
    pub fn is_bound(&self) -> bool {
        self.max_click_level() > 0 || self.drag.is_some() || self.scroll.is_some()
    }

    /// The click must wait for the release: the press may turn into a
    /// drag or a scroll modifier first.
    pub fn waits_for_release(&self) -> bool {
        self.drag.is_some() || self.scroll.is_some()
    }

    pub fn has_hold(&self, level: u8) -> bool {
        self.action(level, TriggerDuration::Hold).is_some()
    }

    pub fn has_higher_level(&self, level: u8) -> bool {
        level < self.max_click_level()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledRemaps {
    buttons: [ButtonRemaps; MAX_BUTTONS],
    pub timing: Timing,
    pub freeze_pointer: bool,
}

impl CompiledRemaps {
    pub fn empty() -> Self {
        Self {
            buttons: [ButtonRemaps::default(); MAX_BUTTONS],
            timing: Timing::from(&TimingConfig::default()),
            freeze_pointer: true,
        }
    }

    pub fn button(&self, cg_button: u8) -> ButtonRemaps {
        self.buttons
            .get(usize::from(cg_button))
            .copied()
            .unwrap_or_default()
    }
}

/// Bad hand-edited entries are skipped with a warning; one bad remap never
/// resets the user's settings (design §1).
#[cfg(test)]
pub fn compile(config: &Config) -> CompiledRemaps {
    compile_with(config, OutputSupport::NONE)
}

pub fn compile_with(config: &Config, support: OutputSupport) -> CompiledRemaps {
    let mut compiled = CompiledRemaps::empty();
    for remap in &config.buttons.remaps {
        let t = remap.trigger;
        if let Err(problem) = check_remap(remap) {
            tracing::warn!(?t, ?problem, "skipping an invalid remap");
            continue;
        }
        let slot = compiled.buttons[usize::from(t.button - 1)].slot(t.level, t.duration);
        if slot.is_some() {
            tracing::warn!(?t, "skipping a duplicate remap; the first one wins");
            continue;
        }
        *slot = Some(CompiledAction::from(&remap.effect.action));
    }
    compiled.timing = Timing::from(&config.timing);
    compiled.freeze_pointer = config.general.freeze_pointer_during_drag;
    compile_bindings(config, support, &mut compiled);
    compiled
}

fn valid_binding_button(button: u8, seen: &mut u64) -> bool {
    if !(3..=MAX_BUTTONS as u8).contains(&button) || *seen & (1 << button) != 0 {
        tracing::warn!(button, "skipping an invalid or duplicate binding");
        return false;
    }
    *seen |= 1 << button;
    true
}

/// Same rules as the save path; a bad hand-edited entry is skipped. The
/// whole-list check (`validate_bindings`) finds the first problem only, so
/// entries are checked one by one here.
fn compile_bindings(config: &Config, support: OutputSupport, compiled: &mut CompiledRemaps) {
    let b = &config.buttons;
    let mut seen_drag = 0u64;
    for d in &b.drags {
        if !valid_binding_button(d.button, &mut seen_drag) {
            continue;
        }
        let drag = match &d.effect {
            DragEffect::SpacesAndMissionControl if support.dock_swipe => {
                CompiledDrag::Swipe(SwipeKind::SpacesAndMissionControl)
            }
            DragEffect::SpacesAndMissionControl => CompiledDrag::Discrete(SPACES_FALLBACK),
            DragEffect::ScrollSwipe => CompiledDrag::Swipe(SwipeKind::ScrollSwipe),
            DragEffect::Discrete {
                left,
                right,
                up,
                down,
            } => {
                let one = |a: &Option<Action>| {
                    a.as_ref()
                        .filter(|a| check_action(a).is_ok())
                        .map(CompiledAction::from)
                };
                let actions = DiscreteActions {
                    left: one(left),
                    right: one(right),
                    up: one(up),
                    down: one(down),
                };
                if actions == DiscreteActions::default() {
                    continue;
                }
                CompiledDrag::Discrete(actions)
            }
        };
        compiled.buttons[usize::from(d.button - 1)].drag = Some(drag);
    }

    let mut seen_scroll = 0u64;
    for s in &b.scroll_bindings {
        if valid_binding_button(s.button, &mut seen_scroll) {
            compiled.buttons[usize::from(s.button - 1)].scroll = Some(s.effect);
        }
    }

    let mut seen_hold = 0u64;
    for h in &b.hold_through {
        if !valid_binding_button(h.button, &mut seen_hold) {
            continue;
        }
        let slot = &mut compiled.buttons[usize::from(h.button - 1)];
        if slot.is_bound() {
            tracing::warn!(
                button = h.button,
                "skipping a hold-through on a bound button"
            );
            continue;
        }
        slot.acts_as = Some(h.acts_as.cg_button());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        Buttons, ClickTrigger, DragEffect, Effect, NavDirection, Remap, SymbolicHotkeyId,
    };
    use crate::test_support::{config_action, phase4_buttons};

    const MC: CompiledAction = CompiledAction::SymbolicHotkey(SymbolicHotkeyId::MissionControl);
    const DESKTOP: CompiledAction = CompiledAction::SymbolicHotkey(SymbolicHotkeyId::ShowDesktop);
    const BACK: CompiledAction = CompiledAction::Navigate(NavDirection::Back);

    fn remap(button: u8, level: u8, duration: TriggerDuration, action: CompiledAction) -> Remap {
        Remap {
            trigger: ClickTrigger {
                button,
                level,
                duration,
            },
            effect: Effect {
                action: config_action(action),
            },
        }
    }

    fn with(remaps: Vec<Remap>) -> Config {
        Config {
            buttons: Buttons {
                remaps,
                drags: vec![],
                scroll_bindings: vec![],
                hold_through: vec![],
            },
            ..Config::default()
        }
    }

    #[test]
    fn phase4_buttons_map_the_middle_button_to_cg_button_2() {
        let compiled = compile(&Config {
            buttons: phase4_buttons(),
            ..Config::default()
        });
        let middle = compiled.button(2);
        assert_eq!(middle.action(1, TriggerDuration::Click), Some(MC));
        assert_eq!(middle.action(1, TriggerDuration::Hold), Some(DESKTOP));
        assert_eq!(
            compiled.button(3).action(1, TriggerDuration::Click),
            Some(BACK)
        );
        assert_eq!(compiled.button(4).max_click_level(), 1);
    }

    #[test]
    fn defaults_leave_other_buttons_unused() {
        let compiled = compile(&Config::default());
        assert_eq!(compiled.button(5).max_click_level(), 0);
        assert_eq!(compiled.button(200).max_click_level(), 0);
    }

    #[test]
    fn default_timing_is_250_and_260_ms() {
        let t = compile(&Config::default()).timing;
        assert_eq!(t.hold, Duration::from_millis(250));
        assert_eq!(t.level_expiry, Duration::from_millis(260));
    }

    fn timing_of(hold_ms: u32, double_click_ms: u32) -> Timing {
        compile(&Config {
            timing: crate::config::schema::Timing {
                hold_ms,
                double_click_ms,
            },
            ..Config::default()
        })
        .timing
    }

    #[test]
    fn timing_comes_from_the_config() {
        assert_eq!(
            timing_of(400, 300),
            Timing {
                hold: Duration::from_millis(400),
                level_expiry: Duration::from_millis(300)
            }
        );
    }

    #[test]
    fn out_of_range_timing_is_clamped_when_compiled() {
        assert_eq!(
            timing_of(50, 9999),
            Timing {
                hold: Duration::from_millis(150),
                level_expiry: Duration::from_millis(600)
            }
        );
    }

    #[test]
    fn empty_remaps_use_the_default_timing() {
        assert_eq!(
            CompiledRemaps::empty().timing,
            Timing {
                hold: Duration::from_millis(250),
                level_expiry: Duration::from_millis(260)
            }
        );
    }

    #[test]
    fn hold_and_higher_level_flags_follow_the_bindings() {
        let compiled = compile(&with(vec![
            remap(3, 1, TriggerDuration::Hold, DESKTOP),
            remap(3, 2, TriggerDuration::Click, MC),
        ]));
        let b = compiled.button(2);
        assert_eq!(b.max_click_level(), 2);
        assert!(b.has_hold(1));
        assert!(!b.has_hold(2));
        assert!(b.has_higher_level(1));
        assert!(!b.has_higher_level(2));
        assert_eq!(b.action(1, TriggerDuration::Click), None);
    }

    #[test]
    fn left_and_right_buttons_are_never_remapped() {
        let compiled = compile(&with(vec![
            remap(1, 1, TriggerDuration::Click, MC),
            remap(2, 1, TriggerDuration::Click, MC),
        ]));
        assert_eq!(compiled.button(0).max_click_level(), 0);
        assert_eq!(compiled.button(1).max_click_level(), 0);
    }

    #[test]
    fn out_of_range_levels_and_buttons_are_skipped_and_the_rest_load() {
        let compiled = compile(&with(vec![
            remap(3, 0, TriggerDuration::Click, MC),
            remap(3, 4, TriggerDuration::Click, MC),
            remap(33, 1, TriggerDuration::Click, MC),
            remap(32, 1, TriggerDuration::Click, BACK),
        ]));
        assert_eq!(compiled.button(2).max_click_level(), 0);
        assert_eq!(
            compiled.button(31).action(1, TriggerDuration::Click),
            Some(BACK)
        );
    }

    #[test]
    fn the_first_of_two_identical_triggers_wins() {
        let compiled = compile(&with(vec![
            remap(3, 1, TriggerDuration::Click, MC),
            remap(3, 1, TriggerDuration::Click, BACK),
        ]));
        assert_eq!(
            compiled.button(2).action(1, TriggerDuration::Click),
            Some(MC)
        );
    }

    #[test]
    fn triple_click_and_triple_hold_are_supported() {
        let compiled = compile(&with(vec![
            remap(3, 3, TriggerDuration::Click, MC),
            remap(3, 3, TriggerDuration::Hold, DESKTOP),
        ]));
        let b = compiled.button(2);
        assert_eq!(b.action(3, TriggerDuration::Click), Some(MC));
        assert_eq!(b.action(3, TriggerDuration::Hold), Some(DESKTOP));
        assert_eq!(b.max_click_level(), 3);
        assert!(!b.has_higher_level(3));
        assert!(b.has_higher_level(2));
    }

    #[test]
    fn a_hold_only_at_level_2_makes_level_1_wait() {
        let compiled = compile(&with(vec![remap(3, 2, TriggerDuration::Hold, DESKTOP)]));
        let b = compiled.button(2);
        assert_eq!(b.max_click_level(), 2);
        assert!(b.has_higher_level(1));
    }

    #[test]
    fn mouse_clicks_and_media_keys_compile_to_copy_actions() {
        use crate::config::schema::{Action, MouseButton, SystemKey};
        let mut config = Config::default();
        config.buttons.remaps = vec![
            Remap {
                trigger: ClickTrigger {
                    button: 4,
                    level: 1,
                    duration: TriggerDuration::Click,
                },
                effect: Effect {
                    action: Action::MouseClick {
                        button: MouseButton::Back,
                        clicks: 2,
                    },
                },
            },
            Remap {
                trigger: ClickTrigger {
                    button: 5,
                    level: 1,
                    duration: TriggerDuration::Click,
                },
                effect: Effect {
                    action: Action::SystemKey {
                        key: SystemKey::Mute,
                    },
                },
            },
        ];
        let c = compile(&config);
        // Config buttons are 1-based: B4 is CG 3, B5 is CG 4. Back is CG button 3.
        assert_eq!(
            c.button(3).action(1, TriggerDuration::Click),
            Some(CompiledAction::MouseClick {
                cg_button: 3,
                clicks: 2
            })
        );
        assert_eq!(
            c.button(4).action(1, TriggerDuration::Click),
            Some(CompiledAction::SystemKey(SystemKey::Mute))
        );
    }

    use crate::config::schema::{
        DragBinding, HoldThrough, MouseButton as Mb, ScrollBinding, ScrollEffect as Se,
    };

    fn with_buttons(f: impl FnOnce(&mut Buttons)) -> Config {
        let mut c = Config {
            buttons: phase4_buttons(),
            ..Config::default()
        };
        f(&mut c.buttons);
        c
    }

    #[test]
    fn a_verified_dock_swipe_compiles_to_the_swipe() {
        let c = compile_with(
            &with_buttons(|b| {
                b.drags = vec![DragBinding {
                    button: 3,
                    effect: DragEffect::SpacesAndMissionControl,
                }]
            }),
            OutputSupport {
                dock_swipe: true,
                magnify: false,
            },
        );
        assert_eq!(
            c.button(2).drag,
            Some(CompiledDrag::Swipe(SwipeKind::SpacesAndMissionControl))
        );
    }

    #[test]
    fn an_unverified_dock_swipe_falls_back_to_separate_actions() {
        let c = compile_with(
            &with_buttons(|b| {
                b.drags = vec![DragBinding {
                    button: 3,
                    effect: DragEffect::SpacesAndMissionControl,
                }]
            }),
            OutputSupport::NONE,
        );
        assert_eq!(
            c.button(2).drag,
            Some(CompiledDrag::Discrete(DiscreteActions {
                left: Some(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::SpaceRight)),
                right: Some(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::SpaceLeft)),
                up: Some(MC),
                down: Some(CompiledAction::SymbolicHotkey(SymbolicHotkeyId::AppExpose)),
            }))
        );
    }

    #[test]
    fn scroll_swipe_needs_no_private_api() {
        let c = compile_with(
            &with_buttons(|b| {
                b.drags = vec![DragBinding {
                    button: 5,
                    effect: DragEffect::ScrollSwipe,
                }]
            }),
            OutputSupport::NONE,
        );
        assert_eq!(
            c.button(4).drag,
            Some(CompiledDrag::Swipe(SwipeKind::ScrollSwipe))
        );
    }

    #[test]
    fn scroll_bindings_and_hold_through_compile_per_button() {
        let c = compile(&with_buttons(|b| {
            b.scroll_bindings = vec![ScrollBinding {
                button: 4,
                effect: Se::Zoom,
            }];
            b.hold_through = vec![HoldThrough {
                button: 6,
                acts_as: Mb::Middle,
            }];
        }));
        assert_eq!(c.button(3).scroll, Some(Se::Zoom));
        assert_eq!(c.button(5).acts_as, Some(2), "B6 acts as middle (CG 2)");
    }

    #[test]
    fn a_drag_only_button_is_bound_and_waits_for_release() {
        let c = compile(&with_buttons(|b| {
            b.drags = vec![DragBinding {
                button: 7,
                effect: DragEffect::ScrollSwipe,
            }]
        }));
        let b7 = c.button(6);
        assert_eq!(b7.max_click_level(), 0);
        assert!(b7.is_bound());
        assert!(b7.waits_for_release());
        assert!(!c.button(7).is_bound());
    }

    #[test]
    fn a_hand_edited_conflicting_hold_through_is_skipped() {
        let c = compile(&with_buttons(|b| {
            b.hold_through = vec![HoldThrough {
                button: 4,
                acts_as: Mb::Left,
            }];
        }));
        assert_eq!(c.button(3).acts_as, None, "B4 already has a click remap");
    }

    #[test]
    fn the_freeze_setting_is_compiled() {
        let mut config = Config::default();
        config.general.freeze_pointer_during_drag = false;
        assert!(!compile(&config).freeze_pointer);
        assert!(compile(&Config::default()).freeze_pointer);
    }
}
