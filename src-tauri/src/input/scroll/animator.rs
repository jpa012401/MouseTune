//! Turns wheel ticks into per-frame scroll events with the trackpad phase
//! lifecycle (Phase 5a design §3). Pure: time comes in as `Instant`.
//! `tick` and `end` run in the scroll tap callback, so nothing here
//! allocates; `frame` runs on display-link frames.

use std::time::Instant;

use super::{
    acceleration::{Axis, PX_PER_LINE, SwipeTracker, Tick, TickRate, swift_factor, tick_px},
    curves::{Curve, params},
};
use crate::config::schema::Scroll;

/// `kCGScrollWheelEventScrollPhase` values the animator sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GesturePhase {
    None,
    Began,
    Changed,
    Ended,
}

/// `kCGScrollWheelEventMomentumPhase` values the animator sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MomentumPhase {
    None,
    Begin,
    Continue,
    End,
}

/// One synthetic scroll event. `dy` is axis 1 (vertical) and `dx` axis 2,
/// already inverted where the settings say so.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollOut {
    /// Whole pixels; a gesture's whole pixels add up to its rounded distance.
    pub dy: i64,
    pub dx: i64,
    /// Unrounded pixels.
    pub dy_fine: f64,
    pub dx_fine: f64,
    /// Whole lines, at `PX_PER_LINE`, for apps that read only line deltas.
    pub dy_lines: i64,
    pub dx_lines: i64,
    pub phase: GesturePhase,
    pub momentum: MomentumPhase,
}

/// One frame's output. There are up to three events: the previous
/// gesture's close, this frame's movement, and a phase change.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameOut {
    events: [Option<ScrollOut>; 3],
    pub wants_frames: bool,
}

impl FrameOut {
    pub fn events(&self) -> impl Iterator<Item = &ScrollOut> {
        self.events.iter().flatten()
    }

    fn push(&mut self, event: ScrollOut) {
        if let Some(slot) = self.events.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(event);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Scroll phase `Began`/`Changed` along the base curve.
    Base,
    /// The tail, sent as momentum events.
    Momentum,
    /// The tail, sent as `Changed` (trackpad simulation off).
    TailAsChanged,
}

#[derive(Debug, Clone, Copy)]
struct Gesture {
    axis: Axis,
    /// The ticks' sign, which every continuing tick must share.
    sign: i8,
    /// Output sign: the ticks' sign, flipped if this axis is inverted.
    out_sign: i64,
    curve: Curve,
    start: Instant,
    /// Gesture position where `curve` starts (what earlier curves covered).
    origin: f64,
    /// Gesture position already sent.
    sent: f64,
    /// Pixels not yet sent as a whole line.
    line_carry: f64,
    began: bool,
    momentum_begun: bool,
    stage: Stage,
}

pub struct Animator {
    settings: Scroll,
    rate: TickRate,
    swipes: SwipeTracker,
    gesture: Option<Gesture>,
    /// The replaced or ended gesture's closing event, sent on the next frame.
    closing: Option<ScrollOut>,
}

impl Animator {
    pub fn new(settings: Scroll) -> Self {
        Self {
            settings,
            rate: TickRate::default(),
            swipes: SwipeTracker::default(),
            gesture: None,
            closing: None,
        }
    }

    /// Callers cancel first; the new settings apply from the next tick.
    pub fn set_settings(&mut self, settings: Scroll) {
        self.settings = settings;
    }

    pub fn is_active(&self) -> bool {
        self.gesture.is_some() || self.closing.is_some()
    }

    /// A swallowed wheel tick. Same axis and direction during the base (or
    /// a tail sent as `Changed`) extends the gesture; anything else closes
    /// it and begins a new one (design §3).
    pub fn tick(&mut self, tick: Tick, now: Instant) {
        let x = self.rate.observe(&tick, now);
        // A precise tick ends any streak and starts none, so letting go of
        // the key after quick precise flicks doesn't jump to a swipe factor.
        let swipes = if tick.precise {
            self.swipes.reset();
            0
        } else {
            self.swipes.observe(&tick, now)
        };
        let px = tick_px(self.settings.speed, self.settings.precise, x, &tick)
            * swift_factor(swipes, &tick);
        let params = params(self.settings.smoothness, x);
        if let Some(g) = self.gesture.as_mut()
            && g.axis == tick.axis
            && g.sign == tick.sign
            && g.stage != Stage::Momentum
        {
            let at = g.curve.position(secs(now, g.start));
            let remaining = g.curve.distance() - at;
            g.origin += at;
            g.curve = Curve::new(remaining + px, params);
            g.start = now;
            g.stage = Stage::Base;
            return;
        }
        self.end();
        self.gesture = Some(Gesture {
            axis: tick.axis,
            sign: tick.sign,
            out_sign: self.out_sign(&tick),
            curve: Curve::new(px, params),
            start: now,
            origin: 0.0,
            sent: 0.0,
            line_carry: 0.0,
            began: false,
            momentum_begun: false,
            stage: Stage::Base,
        });
    }

    /// Ends the gesture in flight. True when a closing event waits for the
    /// next frame. A gesture that never sent `Began` closes silently.
    pub fn end(&mut self) -> bool {
        if let Some(g) = self.gesture.take()
            && g.began
        {
            self.closing = g.closing();
        }
        self.closing.is_some()
    }

    /// Ends everything now and returns the closing events to post. Not for
    /// a tap callout: the caller posts them.
    pub fn cancel(&mut self) -> FrameOut {
        self.end();
        let mut out = FrameOut::default();
        if let Some(closing) = self.closing.take() {
            out.push(closing);
        }
        self.rate.reset();
        self.swipes.reset();
        out
    }

    pub fn frame(&mut self, now: Instant) -> FrameOut {
        let mut out = FrameOut::default();
        if let Some(closing) = self.closing.take() {
            out.push(closing);
        }
        let trackpad = self.settings.trackpad_simulation;
        if let Some(g) = self.gesture.as_mut()
            && g.advance(now, trackpad, &mut out)
        {
            self.gesture = None;
        }
        out.wants_frames = self.gesture.is_some();
        out
    }

    fn out_sign(&self, tick: &Tick) -> i64 {
        let inverted = match tick.axis {
            Axis::Vertical => self.settings.invert_vertical,
            Axis::Horizontal => self.settings.invert_horizontal,
        };
        let sign = i64::from(tick.sign);
        if inverted { -sign } else { sign }
    }
}

impl Gesture {
    /// Sends this frame's movement and any phase change. True once over.
    fn advance(&mut self, now: Instant, trackpad: bool, out: &mut FrameOut) -> bool {
        let t = secs(now, self.start);
        match self.stage {
            Stage::Base => {
                let base_end = self.curve.base_secs();
                let phase = if self.began {
                    GesturePhase::Changed
                } else {
                    GesturePhase::Began
                };
                self.began = true;
                out.push(self.step(t.min(base_end), phase, MomentumPhase::None));
                if t < base_end {
                    return false;
                }
                if !self.curve.has_tail() {
                    out.push(self.marker(GesturePhase::Ended, MomentumPhase::None));
                    return true;
                }
                if trackpad {
                    out.push(self.marker(GesturePhase::Ended, MomentumPhase::None));
                    self.stage = Stage::Momentum;
                } else {
                    self.stage = Stage::TailAsChanged;
                }
                false
            }
            Stage::Momentum => {
                let momentum = if self.momentum_begun {
                    MomentumPhase::Continue
                } else {
                    MomentumPhase::Begin
                };
                self.momentum_begun = true;
                out.push(self.step(t, GesturePhase::None, momentum));
                if t < self.curve.duration() {
                    return false;
                }
                out.push(self.marker(GesturePhase::None, MomentumPhase::End));
                true
            }
            Stage::TailAsChanged => {
                out.push(self.step(t, GesturePhase::Changed, MomentumPhase::None));
                if t < self.curve.duration() {
                    return false;
                }
                out.push(self.marker(GesturePhase::Ended, MomentumPhase::None));
                true
            }
        }
    }

    /// Moves the sent position to the curve at `t` seconds.
    fn step(&mut self, t: f64, phase: GesturePhase, momentum: MomentumPhase) -> ScrollOut {
        let target = self.origin + self.curve.position(t);
        let fine = target - self.sent;
        let whole = (target.round() - self.sent.round()) as i64;
        self.sent = target;
        self.line_carry += fine;
        let lines = (self.line_carry / PX_PER_LINE).trunc();
        self.line_carry -= lines * PX_PER_LINE;
        self.event(whole, fine, lines as i64, phase, momentum)
    }

    /// A phase change with no movement.
    fn marker(&self, phase: GesturePhase, momentum: MomentumPhase) -> ScrollOut {
        self.event(0, 0.0, 0, phase, momentum)
    }

    fn event(
        &self,
        whole: i64,
        fine: f64,
        lines: i64,
        phase: GesturePhase,
        momentum: MomentumPhase,
    ) -> ScrollOut {
        let s = self.out_sign;
        let (whole, fine, lines) = (whole * s, fine * s as f64, lines * s);
        match self.axis {
            Axis::Vertical => ScrollOut {
                dy: whole,
                dx: 0,
                dy_fine: fine,
                dx_fine: 0.0,
                dy_lines: lines,
                dx_lines: 0,
                phase,
                momentum,
            },
            Axis::Horizontal => ScrollOut {
                dy: 0,
                dx: whole,
                dy_fine: 0.0,
                dx_fine: fine,
                dy_lines: 0,
                dx_lines: lines,
                phase,
                momentum,
            },
        }
    }

    /// What tells apps this gesture is over, if anything is still open.
    fn closing(&self) -> Option<ScrollOut> {
        match self.stage {
            Stage::Base | Stage::TailAsChanged => {
                Some(self.marker(GesturePhase::Ended, MomentumPhase::None))
            }
            Stage::Momentum if self.momentum_begun => {
                Some(self.marker(GesturePhase::None, MomentumPhase::End))
            }
            // `Ended` already went out when the base finished.
            Stage::Momentum => None,
        }
    }
}

fn secs(now: Instant, since: Instant) -> f64 {
    now.saturating_duration_since(since).as_secs_f64()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        config::schema::{
            ScrollSpeed,
            Smoothness::{self, High, Low, Regular},
        },
        test_support::describe_scroll,
    };

    fn settings(smoothness: Smoothness, trackpad_simulation: bool) -> Scroll {
        Scroll {
            smoothness,
            trackpad_simulation,
            ..Scroll::default()
        }
    }

    fn low() -> Animator {
        Animator::new(settings(Low, true))
    }

    fn regular(trackpad_simulation: bool) -> Animator {
        Animator::new(settings(Regular, trackpad_simulation))
    }

    fn system_tick() -> Tick {
        Tick {
            axis: Axis::Vertical,
            sign: 1,
            lines: 1,
            points: 10,
            precise: false,
            swift: false,
        }
    }

    fn system_low() -> Animator {
        Animator::new(Scroll {
            speed: ScrollSpeed::System,
            ..settings(Low, true)
        })
    }

    #[test]
    fn a_second_flick_goes_a_third_further() {
        // speed = system: every tick is its own 10 px point delta, so only the
        // swipe factor changes the distance. Flick 1 (0, 20, 40 ms) is one
        // gesture of 3 × 10 = 30 px. Flick 2 starts 300 ms after it: 10 × 1.33 = 13.3 → 13.
        let at = Clock(Instant::now());
        let mut a = system_low();
        for ms in [0, 20, 40] {
            a.tick(system_tick(), at.ms(ms));
            if ms == 0 {
                a.frame(at.ms(0));
            }
        }
        assert_eq!(frame(&mut a, at.ms(130)), ["Changed dy 30", "Ended"]);
        a.tick(system_tick(), at.ms(340));
        assert_eq!(frame(&mut a, at.ms(340)), ["Began"]);
        assert_eq!(frame(&mut a, at.ms(430)), ["Changed dy 13", "Ended"]);
    }

    #[test]
    fn cancel_also_ends_the_flick_streak() {
        let at = Clock(Instant::now());
        let mut a = system_low();
        for ms in [0, 20, 40] {
            a.tick(system_tick(), at.ms(ms));
        }
        a.cancel();
        a.tick(system_tick(), at.ms(340));
        a.frame(at.ms(340));
        assert_eq!(frame(&mut a, at.ms(430)), ["Changed dy 10", "Ended"]);
    }

    #[test]
    fn precise_ticks_build_no_flick_streak() {
        // Three quick precise ticks (0, 20, 40 ms) would be a swipe if they
        // counted. A plain tick 300 ms later must scroll as a first tick:
        // speed = system → its own 10 px, × 1.0, not × 1.33 = 13.
        let at = Clock(Instant::now());
        let mut a = system_low();
        let precise = Tick {
            precise: true,
            ..system_tick()
        };
        for ms in [0, 20, 40] {
            a.tick(precise, at.ms(ms));
            if ms == 0 {
                a.frame(at.ms(0));
            }
        }
        a.frame(at.ms(130));
        a.tick(system_tick(), at.ms(340));
        assert_eq!(frame(&mut a, at.ms(340)), ["Began"]);
        assert_eq!(frame(&mut a, at.ms(430)), ["Changed dy 10", "Ended"]);
    }

    fn down() -> Tick {
        Tick {
            axis: Axis::Vertical,
            sign: 1,
            lines: 1,
            points: 0,
            precise: false,
            swift: false,
        }
    }

    fn up() -> Tick {
        Tick { sign: -1, ..down() }
    }

    fn right() -> Tick {
        Tick {
            axis: Axis::Horizontal,
            ..down()
        }
    }

    struct Clock(Instant);
    impl Clock {
        fn ms(&self, ms: u64) -> Instant {
            self.0 + Duration::from_millis(ms)
        }
    }

    fn frame(a: &mut Animator, now: Instant) -> Vec<String> {
        a.frame(now).events().map(describe_scroll).collect()
    }

    fn first(out: FrameOut) -> ScrollOut {
        *out.events().next().expect("an event")
    }

    // Low smoothness, 90 px first tick: p(t) = 90 · (1 − (1 − t/90 ms)³).

    #[test]
    fn a_low_smoothness_tick_begins_changes_and_ends() {
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        assert_eq!(frame(&mut a, at.ms(0)), ["Began"]);
        // 90 · (1 − 0.5³) = 78.75 → 79
        assert_eq!(frame(&mut a, at.ms(45)), ["Changed dy 79"]);
        let last = a.frame(at.ms(90));
        assert_eq!(
            last.events().map(describe_scroll).collect::<Vec<_>>(),
            ["Changed dy 11", "Ended"]
        );
        assert!(!last.wants_frames);
        assert!(!a.is_active());
    }

    #[test]
    fn line_and_fine_deltas_follow_the_pixels() {
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        let e = first(a.frame(at.ms(45)));
        assert!((e.dy_fine - 78.75).abs() < 1e-9);
        assert_eq!(e.dy_lines, 7, "78.75 px = 7 lines, 8.75 px carried");
        let e = first(a.frame(at.ms(90)));
        assert_eq!(e.dy_lines, 2, "8.75 + 11.25 = 20 px");
    }

    #[test]
    fn inverting_vertical_flips_every_delta() {
        let at = Clock(Instant::now());
        let mut a = Animator::new(Scroll {
            invert_vertical: true,
            ..settings(Low, true)
        });
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        let e = first(a.frame(at.ms(45)));
        assert_eq!((e.dy, e.dy_lines), (-79, -7));
        assert!((e.dy_fine + 78.75).abs() < 1e-9);
    }

    #[test]
    fn a_horizontal_tick_moves_dx_and_ignores_invert_vertical() {
        let at = Clock(Instant::now());
        let mut a = Animator::new(Scroll {
            invert_vertical: true,
            ..settings(Low, true)
        });
        a.tick(right(), at.ms(0));
        a.frame(at.ms(0));
        assert_eq!(frame(&mut a, at.ms(45)), ["Changed dx 79"]);
    }

    // Regular, 90 px first tick (closed form, see curves tests): the base
    // covers 80.38 px in 160 ms; at 200 ms the tail is at
    // 80.38 + (251.196 − 251.196·e^(−23·0.04)) / 23 = 86.95 → 87; it ends at
    // 252.39 ms on 90.

    #[test]
    fn regular_coasts_as_momentum_with_trackpad_simulation() {
        let at = Clock(Instant::now());
        let mut a = regular(true);
        a.tick(down(), at.ms(0));
        assert_eq!(frame(&mut a, at.ms(0)), ["Began"]);
        assert_eq!(frame(&mut a, at.ms(160)), ["Changed dy 80", "Ended"]);
        assert_eq!(frame(&mut a, at.ms(200)), ["momentum Begin dy 7"]);
        let last = a.frame(at.ms(253));
        assert_eq!(
            last.events().map(describe_scroll).collect::<Vec<_>>(),
            ["momentum Continue dy 3", "momentum End"]
        );
        assert!(!last.wants_frames);
    }

    #[test]
    fn without_trackpad_simulation_the_tail_is_sent_as_changed() {
        let at = Clock(Instant::now());
        let mut a = regular(false);
        a.tick(down(), at.ms(0));
        assert_eq!(frame(&mut a, at.ms(0)), ["Began"]);
        assert_eq!(frame(&mut a, at.ms(160)), ["Changed dy 80"]);
        assert_eq!(frame(&mut a, at.ms(200)), ["Changed dy 7"]);
        assert_eq!(frame(&mut a, at.ms(253)), ["Changed dy 3", "Ended"]);
    }

    #[test]
    fn whole_pixels_add_up_to_the_distance_at_60_and_120_hz() {
        for (smoothness, trackpad) in [(Low, true), (Regular, true), (Regular, false), (High, true)]
        {
            for period_us in [16_667, 8_333] {
                let at = Clock(Instant::now());
                let mut a = Animator::new(settings(smoothness, trackpad));
                a.tick(down(), at.ms(0));
                let (mut now, mut total, mut frames) = (at.ms(0), 0, 0);
                loop {
                    let out = a.frame(now);
                    total += out.events().map(|e| e.dy).sum::<i64>();
                    if !out.wants_frames {
                        break;
                    }
                    now += Duration::from_micros(period_us);
                    frames += 1;
                    assert!(frames < 1000, "{smoothness:?} never finished");
                }
                assert_eq!(
                    total, 90,
                    "{smoothness:?} trackpad={trackpad} at {period_us} µs"
                );
            }
        }
    }

    #[test]
    fn a_second_tick_in_the_same_direction_extends_the_gesture() {
        // Second tick 45 ms in: r = 22.2/s, x = 0.26422, px = 90 + 90·x² = 96.283.
        // 11.25 px were still to go, so the new curve covers 107.533 px from
        // 78.75 and lands on 186.283 → 186 whole px, 79 of them already sent.
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        assert_eq!(frame(&mut a, at.ms(45)), ["Changed dy 79"]);
        a.tick(down(), at.ms(45));
        assert_eq!(frame(&mut a, at.ms(135)), ["Changed dy 107", "Ended"]);
    }

    #[test]
    fn a_reversal_closes_the_gesture_and_begins_a_new_one() {
        // The new gesture starts over at 90 px; 15 ms in: 90·(1 − (5/6)³) = 37.92 → 38, upward.
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        a.frame(at.ms(45));
        a.tick(up(), at.ms(45));
        assert_eq!(frame(&mut a, at.ms(60)), ["Ended", "Began dy -38"]);
    }

    #[test]
    fn an_axis_change_closes_the_gesture_and_begins_a_new_one() {
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        a.frame(at.ms(45));
        a.tick(right(), at.ms(45));
        assert_eq!(frame(&mut a, at.ms(60)), ["Ended", "Began dx 38"]);
    }

    #[test]
    fn a_tick_during_momentum_ends_it_and_begins_again() {
        // 200 ms after the first tick the wheel counts as slow again (x = 0, 90 px);
        // 16 ms into the new base: 80.383 · h(0.1) = 80.383 · 0.1855 = 14.91 → 15.
        let at = Clock(Instant::now());
        let mut a = regular(true);
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        a.frame(at.ms(160));
        assert_eq!(frame(&mut a, at.ms(200)), ["momentum Begin dy 7"]);
        a.tick(down(), at.ms(200));
        assert_eq!(frame(&mut a, at.ms(216)), ["momentum End", "Began dy 15"]);
    }

    #[test]
    fn two_opposite_ticks_before_any_frame_send_no_orphan_end() {
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.tick(up(), at.ms(5));
        assert_eq!(frame(&mut a, at.ms(5)), ["Began"]);
    }

    #[test]
    fn ending_mid_base_closes_on_the_next_frame() {
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        assert!(a.end());
        let out = a.frame(at.ms(20));
        assert_eq!(
            out.events().map(describe_scroll).collect::<Vec<_>>(),
            ["Ended"]
        );
        assert!(!out.wants_frames);
    }

    #[test]
    fn ending_mid_momentum_sends_momentum_end() {
        let at = Clock(Instant::now());
        let mut a = regular(true);
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        a.frame(at.ms(160));
        a.frame(at.ms(200));
        assert!(a.end());
        assert_eq!(frame(&mut a, at.ms(210)), ["momentum End"]);
    }

    #[test]
    fn ending_between_base_and_momentum_sends_nothing_more() {
        let at = Clock(Instant::now());
        let mut a = regular(true);
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        a.frame(at.ms(160)); // "Ended" already sent
        assert!(!a.end());
        assert!(frame(&mut a, at.ms(170)).is_empty());
    }

    #[test]
    fn ending_before_the_first_frame_sends_nothing() {
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        assert!(!a.end());
        let out = a.frame(at.ms(5));
        assert_eq!(out.events().count(), 0);
        assert!(!out.wants_frames);
    }

    #[test]
    fn cancel_returns_the_close_at_once() {
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        let out = a.cancel();
        assert_eq!(
            out.events().map(describe_scroll).collect::<Vec<_>>(),
            ["Ended"]
        );
        assert!(!a.is_active());
        assert!(frame(&mut a, at.ms(10)).is_empty());
    }

    #[test]
    fn cancel_resets_the_wheel_speed() {
        // Without the reset, a tick 10 ms later would count as fast (180 px).
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.cancel();
        a.tick(down(), at.ms(10));
        a.frame(at.ms(10));
        assert_eq!(frame(&mut a, at.ms(100)), ["Changed dy 90", "Ended"]);
    }

    #[test]
    fn a_late_frame_finishes_the_gesture_in_one_go() {
        let at = Clock(Instant::now());
        let mut a = low();
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        assert_eq!(frame(&mut a, at.ms(5000)), ["Changed dy 90", "Ended"]);

        let mut a = regular(true);
        a.tick(down(), at.ms(0));
        a.frame(at.ms(0));
        assert_eq!(frame(&mut a, at.ms(5000)), ["Changed dy 80", "Ended"]);
        assert_eq!(
            frame(&mut a, at.ms(5001)),
            ["momentum Begin dy 10", "momentum End"]
        );
    }
}
