//! Trigger resolution and suppression (spec §5.2, design §2). Pure and
//! allocation-free: called from the button tap callback and the click
//! timers on `mt-input`.
//!
//! Invariant: an up is swallowed iff its down was swallowed. Swallowing an
//! up whose down reached an app leaves that app with a stuck button.

use std::time::Duration;

use crate::{
    config::schema::{Action, TriggerDuration},
    input::{
        click_cycle::{ClickCycle, Press},
        filter::MOUSETUNE_EVENT_TAG,
        remaps::{CompiledRemaps, MAX_BUTTONS},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Down,
    Up,
}

/// Fields the button tap copies out of a CGEvent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonEvent {
    pub kind: ButtonKind,
    /// CG button number.
    pub button: u8,
    pub sender_id: u64,
    pub user_data: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Swallow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerKind {
    Hold,
    Expiry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Fire(Action),
    Click { button: u8 },
    Arm(TimerKind, Duration),
    Disarm(TimerKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub verdict: Verdict,
    pub outcomes: Outcomes,
}

/// Port: two reusable one-shot timers on `mt-input`.
pub trait ClickTimers {
    fn arm(&mut self, kind: TimerKind, after: Duration);
    fn disarm(&mut self, kind: TimerKind);
}

const CAPACITY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcomes {
    items: [Option<Outcome>; CAPACITY],
    len: usize,
}

impl Outcomes {
    const NONE: Self = Self {
        items: [None; CAPACITY],
        len: 0,
    };

    fn push(&mut self, outcome: Outcome) {
        // Capacity covers the worst case (6); never panic on the hot path.
        if let Some(slot) = self.items.get_mut(self.len) {
            *slot = Some(outcome);
            self.len += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = Outcome> + '_ {
        self.items.iter().take(self.len).flatten().copied()
    }
}

impl Decision {
    fn pass() -> Self {
        Self {
            verdict: Verdict::Pass,
            outcomes: Outcomes::NONE,
        }
    }
    fn swallow() -> Self {
        Self {
            verdict: Verdict::Swallow,
            outcomes: Outcomes::NONE,
        }
    }
}

pub struct ButtonRouter {
    remaps: CompiledRemaps,
    cycle: ClickCycle,
    hold_armed: bool,
    expiry_armed: bool,
    /// Bit `n` set: CG button `n`'s down was swallowed, so its up must be.
    swallowed: u32,
}

impl ButtonRouter {
    pub fn new(remaps: CompiledRemaps) -> Self {
        Self {
            remaps,
            cycle: ClickCycle::default(),
            hold_armed: false,
            expiry_armed: false,
            swallowed: 0,
        }
    }

    pub fn set_remaps(&mut self, remaps: CompiledRemaps) -> Decision {
        self.remaps = remaps;
        self.reset()
    }

    pub fn reset(&mut self) -> Decision {
        let mut d = Decision::pass();
        self.end(&mut d.outcomes);
        d
    }

    pub fn on_button(&mut self, event: &ButtonEvent) -> Decision {
        if event.user_data == MOUSETUNE_EVENT_TAG || usize::from(event.button) >= MAX_BUTTONS {
            return Decision::pass();
        }
        match event.kind {
            ButtonKind::Down => self.down(event.button, event.sender_id),
            ButtonKind::Up => self.up(event.button),
        }
    }

    pub fn on_timer(&mut self, kind: TimerKind) -> Decision {
        let mut d = Decision::pass();
        let armed = match kind {
            TimerKind::Hold => &mut self.hold_armed,
            TimerKind::Expiry => &mut self.expiry_armed,
        };
        if !std::mem::replace(armed, false) {
            return d;
        }
        let Some(c) = self.cycle.current() else {
            return d;
        };
        let remaps = self.remaps.button(c.press.button);
        match kind {
            TimerKind::Hold => {
                if c.down && !c.hold_fired {
                    self.cycle.mark_hold_fired();
                    if let Some(action) = remaps.action(c.press.level, TriggerDuration::Hold) {
                        d.outcomes.push(Outcome::Fire(action));
                    }
                }
            }
            TimerKind::Expiry => {
                if c.down {
                    self.cycle.mark_expired();
                } else {
                    self.finish(&mut d.outcomes);
                }
            }
        }
        d
    }

    fn down(&mut self, button: u8, sender_id: u64) -> Decision {
        let remaps = self.remaps.button(button);
        let max = remaps.max_click_level();
        if max == 0 {
            return Decision::pass();
        }
        let mut d = Decision::swallow();
        let continues = self
            .cycle
            .current()
            .is_some_and(|c| c.press.button == button && c.press.sender_id == sender_id && !c.down);
        if !continues {
            self.finish(&mut d.outcomes);
        }
        let press = self.cycle.press(button, sender_id, max);
        self.swallowed |= 1 << button;
        self.arm(TimerKind::Expiry, &mut d.outcomes);
        let has_hold = remaps.has_hold(press.level);
        if has_hold {
            self.arm(TimerKind::Hold, &mut d.outcomes);
        }
        if !has_hold && !remaps.has_higher_level(press.level) {
            self.emit_click(press, &mut d.outcomes);
            self.cycle.mark_click_fired();
        }
        d
    }

    fn up(&mut self, button: u8) -> Decision {
        let bit = 1 << button;
        if self.swallowed & bit == 0 {
            return Decision::pass();
        }
        self.swallowed &= !bit;
        let mut d = Decision::swallow();
        let Some(c) = self.cycle.current() else {
            return d;
        };
        if c.press.button != button || !c.down {
            return d;
        }
        self.cycle.release();
        self.disarm(TimerKind::Hold, &mut d.outcomes);
        let remaps = self.remaps.button(button);
        if c.hold_fired || c.click_fired {
            self.end(&mut d.outcomes);
        } else if remaps.has_higher_level(c.press.level) && !c.expired {
            // Wait: the expiry timer fires the click, or the next press continues the cycle.
        } else {
            self.emit_click(c.press, &mut d.outcomes);
            self.end(&mut d.outcomes);
        }
        d
    }

    /// Ends the cycle, first firing a released press that was waiting for expiry.
    fn finish(&mut self, out: &mut Outcomes) {
        if let Some(c) = self.cycle.current()
            && !c.down
            && !c.hold_fired
            && !c.click_fired
        {
            self.emit_click(c.press, out);
        }
        self.end(out);
    }

    /// Ends the cycle without firing.
    fn end(&mut self, out: &mut Outcomes) {
        self.cycle.end();
        self.disarm(TimerKind::Hold, out);
        self.disarm(TimerKind::Expiry, out);
    }

    fn emit_click(&self, press: Press, out: &mut Outcomes) {
        match self
            .remaps
            .button(press.button)
            .action(press.level, TriggerDuration::Click)
        {
            Some(action) => out.push(Outcome::Fire(action)),
            None => out.push(Outcome::Click {
                button: press.button,
            }),
        }
    }

    fn arm(&mut self, kind: TimerKind, out: &mut Outcomes) {
        let after = match kind {
            TimerKind::Hold => {
                self.hold_armed = true;
                self.remaps.timing.hold
            }
            TimerKind::Expiry => {
                self.expiry_armed = true;
                self.remaps.timing.level_expiry
            }
        };
        out.push(Outcome::Arm(kind, after));
    }

    fn disarm(&mut self, kind: TimerKind, out: &mut Outcomes) {
        let armed = match kind {
            TimerKind::Hold => &mut self.hold_armed,
            TimerKind::Expiry => &mut self.expiry_armed,
        };
        if std::mem::replace(armed, false) {
            out.push(Outcome::Disarm(kind));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::schema::{
            Buttons, ClickTrigger, Config, Effect, NavDirection, Remap, SymbolicHotkeyId,
        },
        input::remaps::compile,
    };

    const MC: Action = Action::SymbolicHotkey {
        id: SymbolicHotkeyId::MissionControl,
    };
    const DESKTOP: Action = Action::SymbolicHotkey {
        id: SymbolicHotkeyId::ShowDesktop,
    };
    const BACK: Action = Action::Navigate {
        direction: NavDirection::Back,
    };
    const HOLD_250: Outcome = Outcome::Arm(TimerKind::Hold, Duration::from_millis(250));
    const EXPIRY_260: Outcome = Outcome::Arm(TimerKind::Expiry, Duration::from_millis(260));
    const MIDDLE: u8 = 2;
    const SENDER: u64 = 70;

    fn router(remaps: Vec<Remap>) -> ButtonRouter {
        ButtonRouter::new(compile(&Config {
            buttons: Buttons { remaps },
            ..Config::default()
        }))
    }
    fn defaults() -> ButtonRouter {
        ButtonRouter::new(compile(&Config::default()))
    }
    fn remap(button: u8, level: u8, duration: TriggerDuration, action: Action) -> Remap {
        Remap {
            trigger: ClickTrigger {
                button,
                level,
                duration,
            },
            effect: Effect { action },
        }
    }
    fn event(kind: ButtonKind, button: u8) -> ButtonEvent {
        ButtonEvent {
            kind,
            button,
            sender_id: SENDER,
            user_data: 0,
        }
    }
    fn down(r: &mut ButtonRouter, button: u8) -> (Verdict, Vec<Outcome>) {
        split(r.on_button(&event(ButtonKind::Down, button)))
    }
    fn up(r: &mut ButtonRouter, button: u8) -> (Verdict, Vec<Outcome>) {
        split(r.on_button(&event(ButtonKind::Up, button)))
    }
    fn timer(r: &mut ButtonRouter, kind: TimerKind) -> Vec<Outcome> {
        split(r.on_timer(kind)).1
    }
    fn split(d: Decision) -> (Verdict, Vec<Outcome>) {
        (d.verdict, d.outcomes.iter().collect())
    }

    #[test]
    fn unbound_button_passes_down_and_up() {
        let mut r = defaults();
        assert_eq!(down(&mut r, 5), (Verdict::Pass, vec![]));
        assert_eq!(up(&mut r, 5), (Verdict::Pass, vec![]));
    }

    #[test]
    fn a_click_with_nothing_else_bound_fires_on_down() {
        let mut r = defaults();
        assert_eq!(
            down(&mut r, 3),
            (Verdict::Swallow, vec![EXPIRY_260, Outcome::Fire(BACK)])
        );
        assert_eq!(
            up(&mut r, 3),
            (Verdict::Swallow, vec![Outcome::Disarm(TimerKind::Expiry)])
        );
    }

    #[test]
    fn middle_click_released_before_the_hold_fires_mission_control_on_up() {
        let mut r = defaults();
        assert_eq!(
            down(&mut r, MIDDLE),
            (Verdict::Swallow, vec![EXPIRY_260, HOLD_250])
        );
        assert_eq!(
            up(&mut r, MIDDLE),
            (
                Verdict::Swallow,
                vec![
                    Outcome::Disarm(TimerKind::Hold),
                    Outcome::Fire(MC),
                    Outcome::Disarm(TimerKind::Expiry)
                ]
            )
        );
    }

    #[test]
    fn middle_hold_fires_show_desktop_and_no_click_on_release() {
        let mut r = defaults();
        down(&mut r, MIDDLE);
        assert_eq!(timer(&mut r, TimerKind::Hold), vec![Outcome::Fire(DESKTOP)]);
        assert_eq!(
            up(&mut r, MIDDLE),
            (Verdict::Swallow, vec![Outcome::Disarm(TimerKind::Expiry)])
        );
    }

    #[test]
    fn two_quick_clicks_without_a_double_binding_fire_twice() {
        let mut r = defaults();
        down(&mut r, 3);
        up(&mut r, 3);
        assert_eq!(
            down(&mut r, 3),
            (Verdict::Swallow, vec![EXPIRY_260, Outcome::Fire(BACK)])
        );
    }

    #[test]
    fn with_a_double_binding_a_single_click_waits_for_expiry() {
        let mut r = router(vec![
            remap(3, 1, TriggerDuration::Click, BACK),
            remap(3, 2, TriggerDuration::Click, MC),
        ]);
        assert_eq!(down(&mut r, MIDDLE), (Verdict::Swallow, vec![EXPIRY_260]));
        assert_eq!(up(&mut r, MIDDLE), (Verdict::Swallow, vec![]));
        assert_eq!(timer(&mut r, TimerKind::Expiry), vec![Outcome::Fire(BACK)]);
    }

    #[test]
    fn a_double_click_fires_the_level_2_action_on_the_second_down() {
        let mut r = router(vec![
            remap(3, 1, TriggerDuration::Click, BACK),
            remap(3, 2, TriggerDuration::Click, MC),
        ]);
        down(&mut r, MIDDLE);
        up(&mut r, MIDDLE);
        assert_eq!(
            down(&mut r, MIDDLE),
            (Verdict::Swallow, vec![EXPIRY_260, Outcome::Fire(MC)])
        );
    }

    #[test]
    fn an_unbound_level_sends_a_plain_click() {
        let mut r = router(vec![remap(3, 2, TriggerDuration::Click, MC)]);
        down(&mut r, MIDDLE);
        up(&mut r, MIDDLE);
        assert_eq!(
            timer(&mut r, TimerKind::Expiry),
            vec![Outcome::Click { button: MIDDLE }]
        );
    }

    #[test]
    fn held_past_expiry_with_a_double_binding_fires_level_1_on_release() {
        let mut r = router(vec![
            remap(3, 1, TriggerDuration::Click, BACK),
            remap(3, 2, TriggerDuration::Click, MC),
        ]);
        down(&mut r, MIDDLE);
        assert_eq!(timer(&mut r, TimerKind::Expiry), vec![]);
        assert_eq!(
            up(&mut r, MIDDLE),
            (Verdict::Swallow, vec![Outcome::Fire(BACK)])
        );
    }

    #[test]
    fn a_different_button_flushes_a_pending_click_first() {
        let mut r = router(vec![
            remap(3, 1, TriggerDuration::Click, MC),
            remap(3, 2, TriggerDuration::Click, DESKTOP),
            remap(4, 1, TriggerDuration::Click, BACK),
        ]);
        down(&mut r, MIDDLE);
        up(&mut r, MIDDLE);
        assert_eq!(
            down(&mut r, 3),
            (
                Verdict::Swallow,
                vec![
                    Outcome::Fire(MC),
                    Outcome::Disarm(TimerKind::Expiry),
                    EXPIRY_260,
                    Outcome::Fire(BACK)
                ]
            )
        );
    }

    #[test]
    fn our_own_synthetic_events_pass_untouched() {
        let mut r = defaults();
        let tagged = ButtonEvent {
            user_data: crate::input::filter::MOUSETUNE_EVENT_TAG,
            ..event(ButtonKind::Down, 3)
        };
        assert_eq!(split(r.on_button(&tagged)), (Verdict::Pass, vec![]));
    }

    #[test]
    fn a_stray_timer_fire_does_nothing() {
        let mut r = defaults();
        assert_eq!(timer(&mut r, TimerKind::Hold), vec![]);
        assert_eq!(timer(&mut r, TimerKind::Expiry), vec![]);
    }

    #[test]
    fn reset_keeps_the_up_swallowed_and_cancels_the_hold() {
        let mut r = defaults();
        down(&mut r, MIDDLE);
        assert_eq!(
            split(r.reset()).1,
            vec![
                Outcome::Disarm(TimerKind::Hold),
                Outcome::Disarm(TimerKind::Expiry)
            ]
        );
        assert_eq!(timer(&mut r, TimerKind::Hold), vec![]);
        assert_eq!(up(&mut r, MIDDLE), (Verdict::Swallow, vec![]));
    }

    #[test]
    fn an_up_whose_down_passed_is_never_swallowed() {
        let mut r = router(vec![]);
        down(&mut r, MIDDLE); // unbound: passes
        r.set_remaps(compile(&Config::default())); // now bound
        assert_eq!(up(&mut r, MIDDLE), (Verdict::Pass, vec![]));
    }

    #[test]
    fn out_of_range_buttons_pass() {
        let mut r = defaults();
        assert_eq!(down(&mut r, 40), (Verdict::Pass, vec![]));
    }
}
