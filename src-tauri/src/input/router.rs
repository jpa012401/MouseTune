//! Trigger resolution and suppression (spec §5.2, design §2). Pure and
//! allocation-free: called from the button tap callback and the click
//! timers on `mt-input`.
//!
//! Invariant: an up is swallowed iff its down was swallowed. Swallowing an
//! up whose down reached an app leaves that app with a stuck button.

use std::time::Duration;

use crate::{
    actions::CompiledAction,
    config::schema::{ScrollEffect, TriggerDuration},
    input::{
        click_cycle::{ClickCycle, Press},
        filter::MOUSETUNE_EVENT_TAG,
        remaps::{CompiledDrag, CompiledRemaps, DiscreteActions, MAX_BUTTONS, SwipeKind},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Down,
    Up,
}

/// Fields the Drag tap copies out of a mouse-moved or -dragged event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionEvent {
    pub dx: i32,
    pub dy: i32,
    pub user_data: i64,
}

/// Fields the button tap copies out of a CGEvent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonEvent {
    pub kind: ButtonKind,
    /// CG button number.
    pub button: u8,
    pub sender_id: u64,
    pub user_data: i64,
    /// `kCGMouseEventSubtype`: tells tablet events from mouse events.
    pub mouse_subtype: i64,
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
    Fire(CompiledAction),
    Click {
        button: u8,
    },
    Arm(TimerKind, Duration),
    Disarm(TimerKind),
    DragBegin {
        axis: DragAxis,
        swipe: Option<SwipeKind>,
    },
    DragMove {
        dx: i32,
        dy: i32,
    },
    DragEnd,
    HoldThrough {
        cg_button: u8,
        down: bool,
    },
    WantsMotion(bool),
    ModifierReleased,
}

pub const DRAG_THRESHOLD_PX: i32 = 7;
pub const DISCRETE_STEP_PX: i32 = 40;

/// `release_all` pushes at most 5 other outcomes (DragEnd, ModifierReleased,
/// WantsMotion, two Disarms), so 5 hold-through ups keep it within `CAPACITY`.
const MAX_THROUGH_UPS_PER_CALL: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragAxis {
    Horizontal,
    Vertical,
}

/// The drag progress of the press currently down (Phase 6 design §2).
#[derive(Debug, Clone, Copy, Default)]
struct DragTrack {
    dx: i32,
    dy: i32,
    axis: Option<DragAxis>,
    /// Discrete directions already fired this drag: bit 0 left, 1 right, 2 up, 3 down.
    fired: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub verdict: Verdict,
    pub outcomes: Outcomes,
}

/// Port: two reusable one-shot timers on `mt-input`.
///
/// The router tracks one armed flag per kind and ignores fires it didn't
/// arm, so at most one fire per kind may ever be pending.
pub trait ClickTimers {
    /// Schedules `kind` to fire once after `after`, replacing any pending
    /// fire of that timer. Never queues a second fire.
    fn arm(&mut self, kind: TimerKind, after: Duration);
    /// Cancels the pending fire of `kind`, if any.
    fn disarm(&mut self, kind: TimerKind);
}

const CAPACITY: usize = 10;

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
        // Capacity covers the worst case (8); never panic on the hot path.
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
    track: DragTrack,
    /// The current press drove a drag or a scroll binding: its release clicks nothing.
    acted: bool,
    /// The hold-through target posted for each held CG button.
    through: [Option<u8>; MAX_BUTTONS],
}

impl ButtonRouter {
    pub fn new(remaps: CompiledRemaps) -> Self {
        Self {
            remaps,
            cycle: ClickCycle::default(),
            hold_armed: false,
            expiry_armed: false,
            swallowed: 0,
            track: DragTrack::default(),
            acted: false,
            through: [None; MAX_BUTTONS],
        }
    }

    /// Swaps the bindings and ends the cycle. A drag or modifier in
    /// progress is ended first, under the old bindings it was started with.
    pub fn set_remaps(&mut self, remaps: CompiledRemaps) -> Decision {
        let mut d = Decision::pass();
        self.let_go(&mut d.outcomes);
        self.remaps = remaps;
        self.end(&mut d.outcomes);
        d
    }

    /// Ends the click cycle, and any drag or modifier it drives. Keeps the
    /// swallowed bits: the button tap is still running, so the ups of
    /// swallowed downs will arrive.
    pub fn reset(&mut self) -> Decision {
        let mut d = Decision::pass();
        self.let_go(&mut d.outcomes);
        self.end(&mut d.outcomes);
        d
    }

    /// Drops every swallowed bit. Call when the button tap stops seeing
    /// events: ups (and fresh downs) went to the OS meanwhile, so a kept bit
    /// could swallow an up whose down reached an app. An unpaired up passing
    /// is harmless; that one being swallowed leaves the app's button stuck.
    /// Call `release_all` first: a held-through slot isn't cleared here.
    pub fn forget_swallowed(&mut self) {
        self.swallowed = 0;
    }

    pub fn on_button(&mut self, event: &ButtonEvent) -> Decision {
        if event.user_data == MOUSETUNE_EVENT_TAG || usize::from(event.button) >= MAX_BUTTONS {
            return Decision::pass();
        }
        match event.kind {
            ButtonKind::Down => self.down(event.button, event.sender_id),
            ButtonKind::Up => self.up(event.button, event.sender_id),
        }
    }

    /// A down from a device MouseTune leaves alone (an excluded device or a
    /// tablet). It passes untouched, and like an unbound down it proves any
    /// earlier up of this button was missed, so that stale bit is dropped.
    pub fn on_foreign_down(&mut self, button: u8) -> Decision {
        if usize::from(button) < MAX_BUTTONS {
            self.swallowed &= !(1 << button);
        }
        Decision::pass()
    }

    /// A down taken by capture mode: it ends any cycle without firing and
    /// is swallowed, so its up is too. The caller guarantees `button < 32`.
    pub fn on_captured_down(&mut self, button: u8) -> Decision {
        let mut d = Decision::swallow();
        self.let_go(&mut d.outcomes);
        self.end(&mut d.outcomes);
        self.swallowed |= 1 << button;
        d
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

    /// Pointer movement while a button is down (Drag tap). Hot path.
    pub fn on_motion(&mut self, dx: i32, dy: i32) -> Decision {
        let mut d = Decision::pass();
        let Some(c) = self.cycle.current() else {
            return d;
        };
        if !c.down || c.hold_fired {
            return d;
        }
        let Some(drag) = self.remaps.button(c.press.button).drag else {
            return d;
        };
        self.track.dx = self.track.dx.saturating_add(dx);
        self.track.dy = self.track.dy.saturating_add(dy);
        let swipe = match drag {
            CompiledDrag::Swipe(kind) => Some(kind),
            CompiledDrag::Discrete(_) => None,
        };
        if self.track.axis.is_none() {
            let (ax, ay) = (
                self.track.dx.saturating_abs(),
                self.track.dy.saturating_abs(),
            );
            if ax.max(ay) < DRAG_THRESHOLD_PX {
                return d;
            }
            let axis = if ax > ay {
                DragAxis::Horizontal
            } else {
                DragAxis::Vertical
            };
            self.track.axis = Some(axis);
            self.acted = true;
            self.disarm(TimerKind::Hold, &mut d.outcomes);
            self.disarm(TimerKind::Expiry, &mut d.outcomes);
            d.outcomes.push(Outcome::DragBegin { axis, swipe });
            if swipe.is_some() {
                d.outcomes.push(Outcome::DragMove {
                    dx: self.track.dx,
                    dy: self.track.dy,
                });
            }
        } else if swipe.is_some() {
            d.outcomes.push(Outcome::DragMove { dx, dy });
        }
        if let (CompiledDrag::Discrete(actions), Some(axis)) = (drag, self.track.axis) {
            self.fire_discrete(actions, axis, &mut d.outcomes);
        }
        d
    }

    fn fire_discrete(&mut self, a: DiscreteActions, axis: DragAxis, out: &mut Outcomes) {
        let (offset, negative, positive, neg_bit, pos_bit) = match axis {
            DragAxis::Horizontal => (self.track.dx, a.left, a.right, 1u8, 2u8),
            // Screen y grows downwards: negative dy is up.
            DragAxis::Vertical => (self.track.dy, a.up, a.down, 4u8, 8u8),
        };
        let (action, bit) = if offset <= -DISCRETE_STEP_PX {
            (negative, neg_bit)
        } else if offset >= DISCRETE_STEP_PX {
            (positive, pos_bit)
        } else {
            return;
        };
        if self.track.fired & bit == 0 {
            self.track.fired |= bit;
            if let Some(action) = action {
                out.push(Outcome::Fire(action));
            }
        }
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the engine uses it from Phase 6 Task 7")
    )]
    /// A scroll tick was consumed by the held button's binding: it's a
    /// modifier now, so its release clicks nothing.
    pub fn acted_as_modifier(&mut self) -> Decision {
        let mut d = Decision::pass();
        if self.cycle.current().is_some_and(|c| c.down) && !self.acted {
            self.acted = true;
            self.disarm(TimerKind::Hold, &mut d.outcomes);
            self.disarm(TimerKind::Expiry, &mut d.outcomes);
        }
        d
    }

    pub fn held_scroll_effect(&self) -> Option<ScrollEffect> {
        let c = self.cycle.current().filter(|c| c.down)?;
        self.remaps.button(c.press.button).scroll
    }

    /// The Drag tap is needed: a drag-bound button is down.
    pub fn wants_motion(&self) -> bool {
        self.cycle
            .current()
            .is_some_and(|c| c.down && self.remaps.button(c.press.button).drag.is_some())
    }

    pub fn freeze_pointer(&self) -> bool {
        self.remaps.freeze_pointer
    }

    /// Lets go of everything this router is holding open, for `refresh`:
    /// ends a drag, releases a modifier, posts hold-through ups. Posts at
    /// most `MAX_THROUGH_UPS_PER_CALL` ups so the outcomes fit; the caller
    /// repeats until a call returns no outcomes.
    pub fn release_all(&mut self) -> Decision {
        let mut d = Decision::pass();
        self.let_go(&mut d.outcomes);
        let mut released = 0;
        for (button, target) in self.through.iter_mut().enumerate() {
            if released == MAX_THROUGH_UPS_PER_CALL {
                break;
            }
            if let Some(cg_button) = target.take() {
                released += 1;
                self.swallowed &= !(1 << button);
                d.outcomes.push(Outcome::HoldThrough {
                    cg_button,
                    down: false,
                });
            }
        }
        // The press is over for MouseTune: its up is still swallowed (bit kept).
        self.end(&mut d.outcomes);
        d
    }

    /// Ends whatever the press still down is driving (a drag, a scroll
    /// modifier, the Drag tap) before its cycle ends early, so no swipe stays
    /// open and the pointer doesn't stay frozen.
    fn let_go(&mut self, out: &mut Outcomes) {
        if self.track.axis.is_some() {
            out.push(Outcome::DragEnd);
        }
        if self.acted && self.held_scroll_effect().is_some() {
            out.push(Outcome::ModifierReleased);
        }
        if self.wants_motion() {
            out.push(Outcome::WantsMotion(false));
        }
        self.track = DragTrack::default();
        self.acted = false;
    }

    fn down(&mut self, button: u8, sender_id: u64) -> Decision {
        let remaps = self.remaps.button(button);
        if let Some(target) = remaps.acts_as {
            self.swallowed |= 1 << button;
            if let Some(slot) = self.through.get_mut(usize::from(button)) {
                *slot = Some(target);
            }
            let mut d = Decision::swallow();
            d.outcomes.push(Outcome::HoldThrough {
                cg_button: target,
                down: true,
            });
            return d;
        }
        if !remaps.is_bound() {
            // A fresh down proves the previous up was missed (it went to the
            // OS while the tap was off), so its stale bit must not eat this up.
            self.swallowed &= !(1 << button);
            return Decision::pass();
        }
        let mut d = Decision::swallow();
        let continues = self
            .cycle
            .current()
            .is_some_and(|c| c.press.button == button && c.press.sender_id == sender_id && !c.down);
        if !continues {
            self.let_go(&mut d.outcomes);
            self.finish(&mut d.outcomes);
        }
        let press = self
            .cycle
            .press(button, sender_id, remaps.max_click_level().max(1));
        self.swallowed |= 1 << button;
        self.track = DragTrack::default();
        self.acted = false;
        if remaps.drag.is_some() {
            d.outcomes.push(Outcome::WantsMotion(true));
        }
        self.arm(TimerKind::Expiry, &mut d.outcomes);
        let has_hold = remaps.has_hold(press.level);
        if has_hold {
            self.arm(TimerKind::Hold, &mut d.outcomes);
        }
        if !has_hold && !remaps.has_higher_level(press.level) && !remaps.waits_for_release() {
            self.emit_click(press, &mut d.outcomes);
            self.cycle.mark_click_fired();
        }
        d
    }

    fn up(&mut self, button: u8, sender_id: u64) -> Decision {
        if let Some(target) = self
            .through
            .get_mut(usize::from(button))
            .and_then(Option::take)
        {
            self.swallowed &= !(1 << button);
            let mut d = Decision::swallow();
            d.outcomes.push(Outcome::HoldThrough {
                cg_button: target,
                down: false,
            });
            return d;
        }
        let bit = 1 << button;
        if self.swallowed & bit == 0 {
            return Decision::pass();
        }
        self.swallowed &= !bit;
        let mut d = Decision::swallow();
        let Some(c) = self.cycle.current() else {
            return d;
        };
        // Another mouse's up for the same button number: still swallowed (its
        // down was), but it must not release this mouse's press.
        if c.press.button != button || c.press.sender_id != sender_id || !c.down {
            return d;
        }
        self.cycle.release();
        self.disarm(TimerKind::Hold, &mut d.outcomes);
        let remaps = self.remaps.button(button);
        if remaps.drag.is_some() {
            d.outcomes.push(Outcome::WantsMotion(false));
        }
        if self.track.axis.is_some() || self.acted {
            if self.track.axis.is_some() {
                d.outcomes.push(Outcome::DragEnd);
            }
            if self.acted && remaps.scroll.is_some() {
                d.outcomes.push(Outcome::ModifierReleased);
            }
            self.track = DragTrack::default();
            self.acted = false;
            self.end(&mut d.outcomes);
            return d;
        }
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
        test_support::{config_action, phase4_buttons},
    };

    const MC: CompiledAction = CompiledAction::SymbolicHotkey(SymbolicHotkeyId::MissionControl);
    const DESKTOP: CompiledAction = CompiledAction::SymbolicHotkey(SymbolicHotkeyId::ShowDesktop);
    const BACK: CompiledAction = CompiledAction::Navigate(NavDirection::Back);
    const HOLD_250: Outcome = Outcome::Arm(TimerKind::Hold, Duration::from_millis(250));
    const EXPIRY_260: Outcome = Outcome::Arm(TimerKind::Expiry, Duration::from_millis(260));
    const MIDDLE: u8 = 2;
    const SENDER: u64 = 70;

    fn router(remaps: Vec<Remap>) -> ButtonRouter {
        ButtonRouter::new(compile(&Config {
            buttons: Buttons {
                remaps,
                drags: vec![],
                scroll_bindings: vec![],
                hold_through: vec![],
            },
            ..Config::default()
        }))
    }
    #[test]
    fn a_press_arms_the_configured_double_click_and_hold_windows() {
        let mut r = ButtonRouter::new(compile(&Config {
            timing: crate::config::schema::Timing {
                hold_ms: 400,
                double_click_ms: 300,
            },
            buttons: phase4_buttons(),
            ..Config::default()
        }));
        assert_eq!(
            down(&mut r, MIDDLE),
            (
                Verdict::Swallow,
                vec![
                    Outcome::Arm(TimerKind::Expiry, Duration::from_millis(300)),
                    Outcome::Arm(TimerKind::Hold, Duration::from_millis(400)),
                ]
            )
        );
    }

    fn defaults() -> ButtonRouter {
        ButtonRouter::new(compile(&Config {
            buttons: phase4_buttons(),
            ..Config::default()
        }))
    }
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
    fn event(kind: ButtonKind, button: u8) -> ButtonEvent {
        event_from(kind, button, SENDER)
    }
    fn event_from(kind: ButtonKind, button: u8, sender_id: u64) -> ButtonEvent {
        ButtonEvent {
            kind,
            button,
            sender_id,
            user_data: 0,
            mouse_subtype: 0,
        }
    }
    fn down(r: &mut ButtonRouter, button: u8) -> (Verdict, Vec<Outcome>) {
        down_from(r, button, SENDER)
    }
    fn up(r: &mut ButtonRouter, button: u8) -> (Verdict, Vec<Outcome>) {
        up_from(r, button, SENDER)
    }
    fn down_from(r: &mut ButtonRouter, button: u8, sender_id: u64) -> (Verdict, Vec<Outcome>) {
        split(r.on_button(&event_from(ButtonKind::Down, button, sender_id)))
    }
    fn up_from(r: &mut ButtonRouter, button: u8, sender_id: u64) -> (Verdict, Vec<Outcome>) {
        split(r.on_button(&event_from(ButtonKind::Up, button, sender_id)))
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
    fn an_up_from_another_mouse_doesnt_release_this_mouses_press() {
        let mut r = defaults();
        down_from(&mut r, MIDDLE, 70);
        down_from(&mut r, MIDDLE, 71); // ends 70's cycle, starts 71's
        assert_eq!(up_from(&mut r, MIDDLE, 70), (Verdict::Swallow, vec![]));
        assert_eq!(timer(&mut r, TimerKind::Hold), vec![Outcome::Fire(DESKTOP)]);
    }

    #[test]
    fn a_second_mouse_flushes_a_pending_click_of_the_first() {
        let mut r = router(vec![
            remap(3, 1, TriggerDuration::Click, BACK),
            remap(3, 2, TriggerDuration::Click, MC),
        ]);
        down_from(&mut r, MIDDLE, 70);
        up_from(&mut r, MIDDLE, 70);
        assert_eq!(
            down_from(&mut r, MIDDLE, 71),
            (
                Verdict::Swallow,
                vec![
                    Outcome::Fire(BACK),
                    Outcome::Disarm(TimerKind::Expiry),
                    EXPIRY_260
                ]
            )
        );
    }

    #[test]
    fn a_fresh_passing_down_clears_a_stale_swallowed_bit() {
        let mut r = defaults();
        down(&mut r, MIDDLE); // swallowed; its up never reaches us
        r.set_remaps(compile(&Config {
            buttons: Buttons {
                remaps: vec![],
                drags: vec![],
                scroll_bindings: vec![],
                hold_through: vec![],
            },
            ..Config::default()
        }));
        assert_eq!(down(&mut r, MIDDLE), (Verdict::Pass, vec![]));
        assert_eq!(up(&mut r, MIDDLE), (Verdict::Pass, vec![]));
    }

    #[test]
    fn forgetting_swallowed_bits_lets_the_next_up_pass() {
        let mut r = defaults();
        assert_eq!(down(&mut r, MIDDLE).0, Verdict::Swallow);
        r.forget_swallowed();
        assert_eq!(up(&mut r, MIDDLE).0, Verdict::Pass);
    }

    #[test]
    fn a_foreign_down_passes_and_its_up_passes() {
        let mut r = defaults();
        assert_eq!(split(r.on_foreign_down(MIDDLE)), (Verdict::Pass, vec![]));
        assert_eq!(up(&mut r, MIDDLE), (Verdict::Pass, vec![]));
    }

    #[test]
    fn a_foreign_down_clears_a_stale_swallowed_bit() {
        let mut r = defaults();
        down(&mut r, MIDDLE); // swallowed; its up never reaches us
        r.on_foreign_down(MIDDLE);
        assert_eq!(up(&mut r, MIDDLE), (Verdict::Pass, vec![]));
    }

    #[test]
    fn an_out_of_range_foreign_down_passes() {
        let mut r = defaults();
        assert_eq!(split(r.on_foreign_down(40)), (Verdict::Pass, vec![]));
    }

    #[test]
    fn a_captured_down_is_swallowed_and_so_is_its_up() {
        let mut r = defaults();
        assert_eq!(
            split(r.on_captured_down(MIDDLE)),
            (Verdict::Swallow, vec![])
        );
        assert_eq!(up(&mut r, MIDDLE), (Verdict::Swallow, vec![]));
    }

    #[test]
    fn capturing_mid_cycle_ends_that_cycle_without_firing() {
        let mut r = defaults();
        down(&mut r, MIDDLE); // arms expiry + hold
        assert_eq!(
            split(r.on_captured_down(3)),
            (
                Verdict::Swallow,
                vec![
                    Outcome::Disarm(TimerKind::Hold),
                    Outcome::Disarm(TimerKind::Expiry)
                ]
            )
        );
        assert_eq!(timer(&mut r, TimerKind::Hold), vec![]);
        assert_eq!(up(&mut r, MIDDLE), (Verdict::Swallow, vec![]));
    }

    use crate::config::schema::{
        DragBinding, DragEffect, HoldThrough, MouseButton, ScrollBinding, ScrollEffect,
    };
    use crate::input::remaps::{OutputSupport, SwipeKind, compile_with};

    fn router_with(
        f: impl FnOnce(&mut crate::config::schema::Buttons),
        support: OutputSupport,
    ) -> ButtonRouter {
        let mut c = Config {
            buttons: crate::test_support::phase4_buttons(),
            ..Config::default()
        };
        c.buttons
            .remaps
            .retain(|r| r.trigger.button != 3 || r.trigger.duration == TriggerDuration::Click);
        f(&mut c.buttons);
        ButtonRouter::new(compile_with(&c, support))
    }

    /// B3: click → Mission Control, drag → dock swipe.
    fn swipe_router() -> ButtonRouter {
        router_with(
            |b| {
                b.drags = vec![DragBinding {
                    button: 3,
                    effect: DragEffect::SpacesAndMissionControl,
                }]
            },
            OutputSupport {
                dock_swipe: true,
                magnify: false,
            },
        )
    }

    fn outcomes(d: Decision) -> Vec<Outcome> {
        d.outcomes.iter().collect()
    }

    #[test]
    fn a_still_press_of_a_drag_button_clicks_on_release() {
        let mut r = swipe_router();
        let (verdict, on_down) = down(&mut r, MIDDLE);
        assert_eq!(verdict, Verdict::Swallow);
        assert!(
            !on_down.contains(&Outcome::Fire(MC)),
            "a drag button doesn't click on down"
        );
        assert!(on_down.contains(&Outcome::WantsMotion(true)));
        let (_, on_up) = up(&mut r, MIDDLE);
        assert!(on_up.contains(&Outcome::Fire(MC)));
        assert!(on_up.contains(&Outcome::WantsMotion(false)));
    }

    #[test]
    fn six_pixels_is_still_a_click() {
        let mut r = swipe_router();
        down(&mut r, MIDDLE);
        assert_eq!(outcomes(r.on_motion(4, 2)), vec![]);
        assert_eq!(outcomes(r.on_motion(2, 0)), vec![], "6 px total");
        assert!(up(&mut r, MIDDLE).1.contains(&Outcome::Fire(MC)));
    }

    #[test]
    fn seven_pixels_starts_a_horizontal_drag_and_its_release_does_not_click() {
        let mut r = swipe_router();
        down(&mut r, MIDDLE);
        // down armed only the expiry timer (B3 has no hold here), so the drag
        // disarms just that one.
        assert_eq!(
            outcomes(r.on_motion(7, -2)),
            vec![
                Outcome::Disarm(TimerKind::Expiry),
                Outcome::DragBegin {
                    axis: DragAxis::Horizontal,
                    swipe: Some(SwipeKind::SpacesAndMissionControl)
                },
                Outcome::DragMove { dx: 7, dy: -2 },
            ]
        );
        assert_eq!(
            outcomes(r.on_motion(5, 1)),
            vec![Outcome::DragMove { dx: 5, dy: 1 }]
        );
        let (_, on_up) = up(&mut r, MIDDLE);
        assert!(on_up.contains(&Outcome::DragEnd));
        assert!(
            !on_up
                .iter()
                .any(|o| matches!(o, Outcome::Fire(_) | Outcome::Click { .. }))
        );
    }

    #[test]
    fn the_axis_is_vertical_when_dy_wins() {
        let mut r = swipe_router();
        down(&mut r, MIDDLE);
        assert!(outcomes(r.on_motion(3, -8)).contains(&Outcome::DragBegin {
            axis: DragAxis::Vertical,
            swipe: Some(SwipeKind::SpacesAndMissionControl)
        }));
    }

    #[test]
    fn movement_after_a_fired_hold_does_not_drag() {
        let mut r = router_with(
            |b| {
                b.remaps.push(Remap {
                    trigger: ClickTrigger {
                        button: 3,
                        level: 1,
                        duration: TriggerDuration::Hold,
                    },
                    effect: Effect {
                        action: config_action(DESKTOP),
                    },
                });
                b.drags = vec![DragBinding {
                    button: 3,
                    effect: DragEffect::ScrollSwipe,
                }];
            },
            OutputSupport::NONE,
        );
        down(&mut r, MIDDLE);
        assert!(outcomes(r.on_timer(TimerKind::Hold)).contains(&Outcome::Fire(DESKTOP)));
        assert_eq!(outcomes(r.on_motion(30, 0)), vec![]);
    }

    #[test]
    fn separate_actions_fire_once_per_direction_past_40_px() {
        // Unverified dock swipe → SpaceLeft/SpaceRight/MissionControl/AppExpose.
        let mut r = router_with(
            |b| {
                b.drags = vec![DragBinding {
                    button: 3,
                    effect: DragEffect::SpacesAndMissionControl,
                }]
            },
            OutputSupport::NONE,
        );
        down(&mut r, MIDDLE);
        let right = CompiledAction::SymbolicHotkey(SymbolicHotkeyId::SpaceRight);
        let begin = outcomes(r.on_motion(10, 0));
        assert!(begin.contains(&Outcome::DragBegin {
            axis: DragAxis::Horizontal,
            swipe: None
        }));
        assert!(!begin.contains(&Outcome::Fire(right)));
        assert_eq!(outcomes(r.on_motion(29, 0)), vec![], "39 px");
        assert_eq!(
            outcomes(r.on_motion(1, 0)),
            vec![Outcome::Fire(right)],
            "40 px"
        );
        assert_eq!(outcomes(r.on_motion(50, 0)), vec![], "once per drag");
        assert_eq!(
            outcomes(r.on_motion(-130, 0)),
            vec![Outcome::Fire(CompiledAction::SymbolicHotkey(
                SymbolicHotkeyId::SpaceLeft
            ))],
            "back past −40 px: left fires once too"
        );
    }

    #[test]
    fn a_scroll_binding_used_while_held_means_no_click() {
        let mut r = router_with(
            |b| {
                b.scroll_bindings = vec![ScrollBinding {
                    button: 4,
                    effect: ScrollEffect::Zoom,
                }]
            },
            OutputSupport::NONE,
        );
        // B4 (CG 3) clicks → Back when no scroll happens.
        down(&mut r, 3);
        assert_eq!(r.held_scroll_effect(), Some(ScrollEffect::Zoom));
        r.acted_as_modifier();
        let (_, on_up) = up(&mut r, 3);
        assert!(!on_up.contains(&Outcome::Fire(BACK)));
        assert!(on_up.contains(&Outcome::ModifierReleased));
        assert_eq!(r.held_scroll_effect(), None);
    }

    #[test]
    fn hold_through_bypasses_the_click_cycle() {
        let mut r = router_with(
            |b| {
                b.hold_through = vec![HoldThrough {
                    button: 6,
                    acts_as: MouseButton::Middle,
                }]
            },
            OutputSupport::NONE,
        );
        assert_eq!(
            down(&mut r, 5),
            (
                Verdict::Swallow,
                vec![Outcome::HoldThrough {
                    cg_button: 2,
                    down: true
                }]
            )
        );
        assert_eq!(
            up(&mut r, 5),
            (
                Verdict::Swallow,
                vec![Outcome::HoldThrough {
                    cg_button: 2,
                    down: false
                }]
            )
        );
    }

    #[test]
    fn release_all_ends_a_drag_and_lets_go_of_held_through_buttons() {
        let mut r = router_with(
            |b| {
                b.drags = vec![DragBinding {
                    button: 3,
                    effect: DragEffect::ScrollSwipe,
                }];
                b.hold_through = vec![HoldThrough {
                    button: 6,
                    acts_as: MouseButton::Left,
                }];
            },
            OutputSupport::NONE,
        );
        down(&mut r, 5);
        down(&mut r, MIDDLE);
        r.on_motion(0, 9);
        let released = outcomes(r.release_all());
        assert!(released.contains(&Outcome::DragEnd));
        assert!(released.contains(&Outcome::HoldThrough {
            cg_button: 0,
            down: false
        }));
        assert!(released.contains(&Outcome::WantsMotion(false)));
        assert_eq!(outcomes(r.release_all()), vec![], "nothing left to release");
    }

    #[test]
    fn release_all_lets_go_of_at_most_five_held_through_buttons_per_call() {
        let mut r = router_with(
            |b| {
                b.remaps.clear();
                b.hold_through = (3..=9)
                    .map(|button| HoldThrough {
                        button,
                        acts_as: MouseButton::Left,
                    })
                    .collect();
            },
            OutputSupport::NONE,
        );
        for cg in 2..=8 {
            down(&mut r, cg);
        }
        let left_up = Outcome::HoldThrough {
            cg_button: 0,
            down: false,
        };
        assert_eq!(outcomes(r.release_all()), vec![left_up; 5]);
        assert_eq!(outcomes(r.release_all()), vec![left_up; 2]);
        assert_eq!(outcomes(r.release_all()), vec![], "all 7 released");
    }

    #[test]
    fn a_hold_through_up_is_swallowed_even_after_the_bindings_change() {
        let mut r = router_with(
            |b| {
                b.hold_through = vec![HoldThrough {
                    button: 6,
                    acts_as: MouseButton::Middle,
                }]
            },
            OutputSupport::NONE,
        );
        down(&mut r, 5);
        r.set_remaps(compile(&Config::default()));
        assert_eq!(
            up(&mut r, 5),
            (
                Verdict::Swallow,
                vec![Outcome::HoldThrough {
                    cg_button: 2,
                    down: false
                }]
            )
        );
    }

    #[test]
    fn another_button_pressed_mid_drag_ends_the_drag_first() {
        let mut r = swipe_router();
        down(&mut r, MIDDLE);
        r.on_motion(9, 0);
        let (_, on_down) = down(&mut r, 3);
        assert_eq!(
            &on_down[..2],
            &[Outcome::DragEnd, Outcome::WantsMotion(false)]
        );
        assert!(on_down.contains(&Outcome::Fire(BACK)), "B4 still clicks");
        assert!(
            !on_down.contains(&Outcome::Fire(MC)),
            "an interrupted drag never clicks"
        );
        assert_eq!(
            outcomes(r.release_all()),
            vec![Outcome::Disarm(TimerKind::Expiry)],
            "the drag is already over; only B4's expiry timer is left"
        );
    }

    #[test]
    fn changing_the_bindings_mid_drag_ends_the_drag() {
        let mut r = swipe_router();
        down(&mut r, MIDDLE);
        r.on_motion(9, 0);
        let changed = outcomes(r.set_remaps(compile(&Config::default())));
        assert!(changed.contains(&Outcome::DragEnd));
        assert!(changed.contains(&Outcome::WantsMotion(false)));
        assert!(!r.wants_motion());
        assert_eq!(outcomes(r.release_all()), vec![], "nothing left open");
    }

    #[test]
    fn changing_the_bindings_releases_a_used_scroll_modifier() {
        let mut r = router_with(
            |b| {
                b.scroll_bindings = vec![ScrollBinding {
                    button: 4,
                    effect: ScrollEffect::Zoom,
                }]
            },
            OutputSupport::NONE,
        );
        down(&mut r, 3);
        r.acted_as_modifier();
        assert!(
            outcomes(r.set_remaps(compile(&Config::default())))
                .contains(&Outcome::ModifierReleased)
        );
    }

    #[test]
    fn a_reset_mid_drag_ends_the_drag() {
        let mut r = swipe_router();
        down(&mut r, MIDDLE);
        r.on_motion(9, 0);
        assert_eq!(
            outcomes(r.reset()),
            vec![Outcome::DragEnd, Outcome::WantsMotion(false)]
        );
    }

    #[test]
    fn a_capture_mid_drag_ends_the_drag() {
        let mut r = swipe_router();
        down(&mut r, MIDDLE);
        r.on_motion(9, 0);
        assert_eq!(
            split(r.on_captured_down(3)),
            (
                Verdict::Swallow,
                vec![Outcome::DragEnd, Outcome::WantsMotion(false)]
            )
        );
    }

    #[test]
    fn the_router_reports_whether_drags_freeze_the_pointer() {
        let mut config = Config::default();
        config.general.freeze_pointer_during_drag = false;
        assert!(!ButtonRouter::new(compile(&config)).freeze_pointer());
        assert!(ButtonRouter::new(compile(&Config::default())).freeze_pointer());
    }

    #[test]
    fn out_of_range_buttons_pass() {
        let mut r = defaults();
        assert_eq!(down(&mut r, 40), (Verdict::Pass, vec![]));
    }
}
