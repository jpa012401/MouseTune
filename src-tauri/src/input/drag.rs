//! Continuous drag gestures (spec §6): the interactive dock swipe
//! through a `SwipeOutput` port, and the two-finger swipe as scroll events
//! with gesture phases. Pure; runs from `run_intents`, never in a callout.

use std::time::{Duration, Instant};

use crate::input::{
    remaps::SwipeKind,
    router::DragAxis,
    scroll::{
        ScrollPoster,
        animator::{GesturePhase, MomentumPhase, ScrollOut},
    },
};

/// Pixels of drag for one whole swipe unit (one Space, or Mission Control
/// fully open). A tuning knob.
pub const SWIPE_UNIT_PX: f64 = 400.0;

/// A release this long after the last motion is a stop, not a flick: the
/// swipe ends with no velocity instead of the last measured one.
const FLICK_WINDOW: Duration = Duration::from_millis(50);

/// Port: the Dock's swipe gesture. `progress` is in swipe units along the
/// locked axis, signed in screen coordinates (left and up are negative).
pub trait SwipeOutput {
    fn begin(&mut self, axis: DragAxis);
    fn change(&mut self, progress: f64);
    fn end(&mut self, velocity: f64);
    fn cancel(&mut self);
}

/// Used where dock swipes aren't verified: `compile` never produces a
/// dock-swipe drag then, so this is never called.
pub struct NoSwipe;

impl SwipeOutput for NoSwipe {
    fn begin(&mut self, _: DragAxis) {}
    fn change(&mut self, _: f64) {}
    fn end(&mut self, _: f64) {}
    fn cancel(&mut self) {}
}

#[derive(Debug, Clone, Copy)]
struct Active {
    kind: SwipeKind,
    axis: DragAxis,
    progress: f64,
    last: Instant,
    velocity: f64,
}

pub struct DragSession {
    swipe: Box<dyn SwipeOutput>,
    scroll: Box<dyn ScrollPoster>,
    active: Option<Active>,
}

impl DragSession {
    pub fn new(swipe: Box<dyn SwipeOutput>, scroll: Box<dyn ScrollPoster>) -> Self {
        Self {
            swipe,
            scroll,
            active: None,
        }
    }

    pub fn begin(&mut self, axis: DragAxis, kind: SwipeKind, now: Instant) {
        self.cancel();
        match kind {
            SwipeKind::SpacesAndMissionControl => self.swipe.begin(axis),
            SwipeKind::ScrollSwipe => self.scroll.post(&marker(GesturePhase::Began)),
        }
        self.active = Some(Active {
            kind,
            axis,
            progress: 0.0,
            last: now,
            velocity: 0.0,
        });
    }

    pub fn motion(&mut self, dx: i32, dy: i32, now: Instant) {
        let Some(a) = self.active.as_mut() else {
            return;
        };
        let along = match a.axis {
            DragAxis::Horizontal => dx,
            DragAxis::Vertical => dy,
        };
        match a.kind {
            SwipeKind::SpacesAndMissionControl => {
                let step = f64::from(along) / SWIPE_UNIT_PX;
                a.progress += step;
                let dt = now.saturating_duration_since(a.last).as_secs_f64();
                if dt > 0.0 {
                    a.velocity = step / dt;
                }
                a.last = now;
                self.swipe.change(a.progress);
            }
            SwipeKind::ScrollSwipe => {
                let px = i64::from(along);
                let (dx, dy) = match a.axis {
                    DragAxis::Horizontal => (px, 0),
                    DragAxis::Vertical => (0, px),
                };
                self.scroll.post(&ScrollOut {
                    dy,
                    dx,
                    dy_fine: dy as f64,
                    dx_fine: dx as f64,
                    dy_lines: dy / 10,
                    dx_lines: dx / 10,
                    phase: GesturePhase::Changed,
                    momentum: MomentumPhase::None,
                });
            }
        }
    }

    pub fn end(&mut self, now: Instant) {
        let Some(a) = self.active.take() else {
            return;
        };
        match a.kind {
            SwipeKind::SpacesAndMissionControl => {
                let paused = now.saturating_duration_since(a.last) > FLICK_WINDOW;
                self.swipe.end(if paused { 0.0 } else { a.velocity });
            }
            SwipeKind::ScrollSwipe => self.scroll.post(&marker(GesturePhase::Ended)),
        }
    }

    pub fn cancel(&mut self) {
        let Some(a) = self.active.take() else {
            return;
        };
        match a.kind {
            SwipeKind::SpacesAndMissionControl => self.swipe.cancel(),
            SwipeKind::ScrollSwipe => self.scroll.post(&marker(GesturePhase::Ended)),
        }
    }
}

fn marker(phase: GesturePhase) -> ScrollOut {
    ScrollOut {
        dy: 0,
        dx: 0,
        dy_fine: 0.0,
        dx_fine: 0.0,
        dy_lines: 0,
        dx_lines: 0,
        phase,
        momentum: MomentumPhase::None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::test_support::{FakeScrollPoster, FakeSwipe};

    fn session() -> (DragSession, FakeSwipe, FakeScrollPoster) {
        let (swipe, scroll) = (FakeSwipe::default(), FakeScrollPoster::default());
        (
            DragSession::new(Box::new(swipe.clone()), Box::new(scroll.clone())),
            swipe,
            scroll,
        )
    }

    #[test]
    fn a_horizontal_dock_swipe_reports_progress_along_its_axis_only() {
        // SWIPE_UNIT_PX = 400: 100 px is a quarter swipe; dy is ignored.
        let (mut s, swipe, _) = session();
        let t0 = Instant::now();
        s.begin(DragAxis::Horizontal, SwipeKind::SpacesAndMissionControl, t0);
        s.motion(100, 30, t0 + Duration::from_millis(10));
        s.motion(100, 0, t0 + Duration::from_millis(20));
        // velocity: the last 100 px (0.25 units) over 10 ms = 25 units/s
        s.end(t0 + Duration::from_millis(20));
        assert_eq!(
            swipe.log(),
            [
                "begin Horizontal",
                "change 0.25",
                "change 0.50",
                "end 25.00"
            ]
        );
    }

    #[test]
    fn a_pause_before_release_ends_with_no_velocity() {
        // The last motion was 190 ms before the release: the flick is over.
        let (mut s, swipe, _) = session();
        let t0 = Instant::now();
        s.begin(DragAxis::Horizontal, SwipeKind::SpacesAndMissionControl, t0);
        s.motion(100, 0, t0 + Duration::from_millis(10));
        s.end(t0 + Duration::from_millis(200));
        assert_eq!(swipe.log(), ["begin Horizontal", "change 0.25", "end 0.00"]);
    }

    #[test]
    fn a_vertical_dock_swipe_counts_up_as_negative_dy() {
        let (mut s, swipe, _) = session();
        let t0 = Instant::now();
        s.begin(DragAxis::Vertical, SwipeKind::SpacesAndMissionControl, t0);
        s.motion(5, -200, t0 + Duration::from_millis(10));
        assert_eq!(swipe.log(), ["begin Vertical", "change -0.50"]);
    }

    #[test]
    fn a_two_finger_swipe_posts_scroll_phases_along_its_axis() {
        let (mut s, _, scroll) = session();
        let t0 = Instant::now();
        s.begin(DragAxis::Horizontal, SwipeKind::ScrollSwipe, t0);
        s.motion(12, 4, t0);
        s.motion(-5, 0, t0);
        s.end(t0);
        assert_eq!(
            scroll.log(),
            ["Began", "Changed dx 12", "Changed dx -5", "Ended"]
        );
    }

    #[test]
    fn cancel_ends_whatever_is_running_and_then_nothing() {
        let (mut s, swipe, scroll) = session();
        let t0 = Instant::now();
        s.begin(DragAxis::Horizontal, SwipeKind::SpacesAndMissionControl, t0);
        s.cancel();
        s.cancel();
        s.motion(50, 0, t0);
        assert_eq!(swipe.log(), ["begin Horizontal", "cancel"]);
        s.begin(DragAxis::Vertical, SwipeKind::ScrollSwipe, t0);
        s.cancel();
        assert_eq!(scroll.log(), ["Began", "Ended"]);
    }
}
