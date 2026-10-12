//! Compiles the remap list into fixed-size lookup tables the hot path can
//! index without allocating (spec §5.2, design §1). Converts 1-based config
//! button numbers to CG button numbers (middle = 2).

use std::time::Duration;

use crate::{
    actions::CompiledAction,
    config::{
        schema::{Config, TIMING_MS, Timing as TimingConfig, TriggerDuration},
        validate::check_remap,
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

/// One button's bindings, indexed by `level - 1`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ButtonRemaps {
    click: [Option<CompiledAction>; MAX_LEVEL as usize],
    hold: [Option<CompiledAction>; MAX_LEVEL as usize],
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
}

impl CompiledRemaps {
    pub fn empty() -> Self {
        Self {
            buttons: [ButtonRemaps::default(); MAX_BUTTONS],
            timing: Timing::from(&TimingConfig::default()),
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
pub fn compile(config: &Config) -> CompiledRemaps {
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
    compiled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{
        Buttons, ClickTrigger, Effect, NavDirection, Remap, SymbolicHotkeyId,
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
    fn defaults_map_the_middle_button_to_cg_button_2() {
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
}
