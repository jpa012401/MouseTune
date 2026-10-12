//! Wheel ticks and how far each one scrolls (Phase 5a design §3, spec §8.2):
//! a faster-spinning wheel moves further per tick. Pure; runs in the scroll
//! tap callback, so nothing here allocates or panics.

use std::time::{Duration, Instant};

use crate::{config::schema::ScrollSpeed, input::filter::ScrollFields};

/// Tick rates (ticks/s) the speed curve maps from.
const R_MIN: f64 = 6.25;
const R_MAX: f64 = 66.7;
/// A longer pause counts as the slowest rate again (1 / R_MIN).
const MAX_GAP: Duration = Duration::from_millis(160);
/// macOS's own line height, for line deltas and for `speed = system`.
pub const PX_PER_LINE: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Axis 1.
    Vertical,
    /// Axis 2.
    Horizontal,
}

/// One wheel tick, as the animator needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub axis: Axis,
    /// +1 or −1: the sign of the delta macOS delivered.
    pub sign: i8,
    /// |line delta|.
    pub lines: u64,
    /// |point delta|: what macOS would have scrolled.
    pub points: u64,
    /// The precise key is held: 1–20 px, and no swift factor.
    pub precise: bool,
    /// The swift key is held: at least 2× (`swift_factor`).
    pub swift: bool,
}

impl Tick {
    /// `None` when neither axis moved. The filter has already passed
    /// diagonal events, so at most one axis is set.
    pub fn from_fields(f: &ScrollFields) -> Option<Self> {
        let (axis, lines, points) = if f.delta_axis1 != 0 {
            (Axis::Vertical, f.delta_axis1, f.point_delta_axis1)
        } else if f.delta_axis2 != 0 {
            (Axis::Horizontal, f.delta_axis2, f.point_delta_axis2)
        } else {
            return None;
        };
        Some(Self {
            axis,
            sign: if lines > 0 { 1 } else { -1 },
            lines: lines.unsigned_abs(),
            points: points.unsigned_abs(),
            precise: false,
            swift: false,
        })
    }
}

/// How fast the wheel spins, measured between consecutive ticks on the
/// same axis in the same direction.
#[derive(Debug, Default)]
pub struct TickRate {
    last: Option<(Instant, Axis, i8)>,
}

impl TickRate {
    /// The spin fraction for this tick: 0 at or below `R_MIN`, 1 at or above
    /// `R_MAX`. A first tick, a reversal, an axis change or a long pause is 0.
    pub fn observe(&mut self, tick: &Tick, now: Instant) -> f64 {
        let previous = self.last.replace((now, tick.axis, tick.sign));
        let Some((at, axis, sign)) = previous else {
            return 0.0;
        };
        if axis != tick.axis || sign != tick.sign {
            return 0.0;
        }
        let gap = now.saturating_duration_since(at);
        if gap > MAX_GAP {
            return 0.0;
        }
        if gap.is_zero() {
            return 1.0;
        }
        ((1.0 / gap.as_secs_f64() - R_MIN) / (R_MAX - R_MIN)).clamp(0.0, 1.0)
    }

    pub fn reset(&mut self) {
        self.last = None;
    }
}

/// Precise key: this range whatever the speed (Phase 5b design §3).
const PRECISE_PX: (f64, f64) = (1.0, 20.0);
/// A run of this many ticks, each ≤ `MAX_GAP` apart, is a swipe.
const SWIPE_TICKS: u32 = 3;
/// Swipes no further apart than this form a streak.
const STREAK_GAP: Duration = Duration::from_millis(600);
/// Each further swipe in a streak multiplies the distance by this…
const SWIPE_GROWTH: f64 = 1.33;
/// …up to this.
const MAX_SWIFT: f64 = 4.0;
/// A held swift key gives at least this.
const SWIFT_KEY_FACTOR: f64 = 2.0;

impl Tick {
    /// The same tick on the other axis (the horizontal-scroll key).
    pub fn swapped(self) -> Self {
        let axis = match self.axis {
            Axis::Vertical => Axis::Horizontal,
            Axis::Horizontal => Axis::Vertical,
        };
        Self { axis, ..self }
    }
}

/// How far one tick scrolls, in pixels, at spin fraction `x`. The precise
/// key wins over the speed; the precise setting lowers a curve's slow end
/// to 1 px (design §3).
pub fn tick_px(speed: ScrollSpeed, precise_setting: bool, x: f64, tick: &Tick) -> f64 {
    let (min, max) = if tick.precise {
        PRECISE_PX
    } else {
        let (min, max) = match speed {
            ScrollSpeed::System if tick.points != 0 => return tick.points as f64,
            ScrollSpeed::System => return tick.lines as f64 * PX_PER_LINE,
            ScrollSpeed::Low => (60.0, 120.0),
            ScrollSpeed::Medium => (90.0, 180.0),
            ScrollSpeed::High => (150.0, 240.0),
        };
        (if precise_setting { 1.0 } else { min }, max)
    };
    // s(x) = x²: a monotone ease-in, the tuning knob (design §3).
    min + (max - min) * x * x
}

/// Counts swipes in a streak (design §3). A run of ticks ≤ `MAX_GAP` apart
/// is a swipe once it has `SWIPE_TICKS`. A new run starting ≤ `STREAK_GAP`
/// after the last tick continues the streak (a run too short to be a swipe
/// keeps the streak but doesn't add to it). A reversal, an axis change or a
/// longer pause ends it.
#[derive(Debug, Default)]
pub struct SwipeTracker {
    last: Option<(Instant, Axis, i8)>,
    run: u32,
    swipes: u32,
}

impl SwipeTracker {
    /// Swipes completed earlier in this streak, before this tick's run.
    pub fn observe(&mut self, tick: &Tick, now: Instant) -> u32 {
        let previous = self.last.replace((now, tick.axis, tick.sign));
        let gap = previous
            .filter(|&(_, axis, sign)| axis == tick.axis && sign == tick.sign)
            .map(|(at, ..)| now.saturating_duration_since(at));
        match gap {
            Some(gap) if gap <= MAX_GAP => self.run = self.run.saturating_add(1),
            Some(gap) if gap <= STREAK_GAP => {
                if self.run >= SWIPE_TICKS {
                    self.swipes = self.swipes.saturating_add(1);
                }
                self.run = 1;
            }
            _ => {
                self.swipes = 0;
                self.run = 1;
            }
        }
        self.swipes
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The distance multiplier for a tick after `swipes` swipes in its streak.
pub fn swift_factor(swipes: u32, tick: &Tick) -> f64 {
    if tick.precise {
        return 1.0;
    }
    // min(16): 1.33^16 is already far past the cap, and powi takes an i32.
    let streak = SWIPE_GROWTH.powi(swipes.min(16) as i32).min(MAX_SWIFT);
    if tick.swift {
        streak.max(SWIFT_KEY_FACTOR)
    } else {
        streak
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::config::schema::ScrollSpeed::{High, Low, Medium, System};

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

    /// Ticks `gaps` ms apart; the pixels of the last one.
    fn px_after(speed: ScrollSpeed, gaps: &[u64]) -> f64 {
        let mut at = Instant::now();
        let mut rate = TickRate::default();
        let mut x = rate.observe(&down(), at);
        for gap in gaps {
            at += Duration::from_millis(*gap);
            x = rate.observe(&down(), at);
        }
        tick_px(speed, false, x, &down())
    }

    #[test]
    fn a_first_tick_moves_the_slowest_distance() {
        assert_eq!(px_after(Low, &[]), 60.0);
        assert_eq!(px_after(Medium, &[]), 90.0);
        assert_eq!(px_after(High, &[]), 150.0);
    }

    #[test]
    fn ticks_25_ms_apart_on_medium_move_about_118_px() {
        // r = 1/0.025 = 40 ticks/s; x = (40 − 6.25) / (66.7 − 6.25) = 0.558313;
        // px = 90 + 90 · x² = 118.0542
        assert!((px_after(Medium, &[25]) - 118.0542).abs() < 1e-3);
    }

    #[test]
    fn ticks_10_ms_apart_hit_the_maximum() {
        // r = 100 ticks/s, above r_max = 66.7: x = 1
        assert_eq!(px_after(Low, &[10]), 120.0);
        assert_eq!(px_after(Medium, &[10]), 180.0);
        assert_eq!(px_after(High, &[10]), 240.0);
    }

    #[test]
    fn a_pause_over_160_ms_starts_over() {
        assert_eq!(px_after(Medium, &[10, 161]), 90.0);
    }

    #[test]
    fn a_reversal_starts_over() {
        let at = Instant::now();
        let mut rate = TickRate::default();
        rate.observe(&down(), at);
        let up = Tick { sign: -1, ..down() };
        assert_eq!(rate.observe(&up, at + Duration::from_millis(10)), 0.0);
    }

    #[test]
    fn an_axis_change_starts_over() {
        let at = Instant::now();
        let mut rate = TickRate::default();
        rate.observe(&down(), at);
        let right = Tick {
            axis: Axis::Horizontal,
            ..down()
        };
        assert_eq!(rate.observe(&right, at + Duration::from_millis(10)), 0.0);
    }

    #[test]
    fn reset_forgets_the_last_tick() {
        let at = Instant::now();
        let mut rate = TickRate::default();
        rate.observe(&down(), at);
        rate.reset();
        assert_eq!(rate.observe(&down(), at + Duration::from_millis(10)), 0.0);
    }

    #[test]
    fn system_speed_uses_the_point_delta_whatever_the_spin() {
        let tick = Tick {
            points: 37,
            ..down()
        };
        assert_eq!(tick_px(System, false, 1.0, &tick), 37.0);
    }

    #[test]
    fn system_speed_without_a_point_delta_uses_ten_px_per_line() {
        let tick = Tick {
            lines: 3,
            points: 0,
            ..down()
        };
        assert_eq!(tick_px(System, false, 0.0, &tick), 30.0);
    }

    #[test]
    fn a_vertical_tick_is_read_from_axis_1() {
        let f = ScrollFields {
            delta_axis1: -2,
            point_delta_axis1: -24,
            ..Default::default()
        };
        assert_eq!(
            Tick::from_fields(&f),
            Some(Tick {
                axis: Axis::Vertical,
                sign: -1,
                lines: 2,
                points: 24,
                precise: false,
                swift: false
            })
        );
    }

    #[test]
    fn a_horizontal_tick_is_read_from_axis_2() {
        let f = ScrollFields {
            delta_axis2: 1,
            point_delta_axis2: 12,
            ..Default::default()
        };
        assert_eq!(
            Tick::from_fields(&f),
            Some(Tick {
                axis: Axis::Horizontal,
                sign: 1,
                lines: 1,
                points: 12,
                precise: false,
                swift: false
            })
        );
    }

    #[test]
    fn a_tick_without_deltas_is_no_tick() {
        assert_eq!(Tick::from_fields(&ScrollFields::default()), None);
    }

    #[test]
    fn extreme_deltas_do_not_panic() {
        let f = ScrollFields {
            delta_axis1: i64::MIN,
            point_delta_axis1: i64::MIN,
            ..Default::default()
        };
        let tick = Tick::from_fields(&f).expect("a tick");
        assert_eq!(tick.lines, 1u64 << 63);
    }

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn the_precise_key_scrolls_1_to_20_px_whatever_the_speed() {
        let tick = Tick {
            precise: true,
            points: 37,
            ..down()
        };
        // 1 + 19·x²: x=0 → 1, x=0.5 → 5.75, x=1 → 20
        assert_eq!(tick_px(Medium, false, 0.0, &tick), 1.0);
        assert_eq!(tick_px(High, false, 0.5, &tick), 5.75);
        assert_eq!(tick_px(System, false, 1.0, &tick), 20.0);
    }

    #[test]
    fn the_precise_setting_drops_the_slow_end_to_one_pixel() {
        // 1 + (180 − 1)·x²: x=0 → 1, x=0.5 → 45.75, x=1 → 180
        assert_eq!(tick_px(Medium, true, 0.0, &down()), 1.0);
        assert_eq!(tick_px(Medium, true, 0.5, &down()), 45.75);
        assert_eq!(tick_px(Medium, true, 1.0, &down()), 180.0);
    }

    #[test]
    fn the_precise_setting_leaves_system_speed_alone() {
        let tick = Tick {
            points: 37,
            ..down()
        };
        assert_eq!(tick_px(System, true, 0.0, &tick), 37.0);
    }

    #[test]
    fn swapping_turns_vertical_into_horizontal_and_back() {
        assert_eq!(down().swapped().axis, Axis::Horizontal);
        assert_eq!(down().swapped().swapped().axis, Axis::Vertical);
    }

    /// Ticks at `ms` (from one start), all downward; the swipes count after the last.
    fn swipes_after(ms: &[u64]) -> u32 {
        let t0 = Instant::now();
        let mut swipes = SwipeTracker::default();
        ms.iter()
            .map(|m| swipes.observe(&down(), at(t0, *m)))
            .last()
            .unwrap_or(0)
    }

    #[test]
    fn a_second_swipe_within_600_ms_continues_the_streak() {
        // Swipe 1: 0, 20, 40 (3 ticks ≤ 160 ms apart). 300 ms later a new run starts.
        assert_eq!(swipes_after(&[0, 20, 40, 340]), 1);
        // Swipe 2: 340, 360, 380; a third run starts 300 ms later.
        assert_eq!(swipes_after(&[0, 20, 40, 340, 360, 380, 680]), 2);
    }

    #[test]
    fn a_run_shorter_than_three_ticks_is_not_a_swipe() {
        assert_eq!(swipes_after(&[0, 20, 320]), 0);
    }

    #[test]
    fn a_pause_over_600_ms_ends_the_streak() {
        assert_eq!(swipes_after(&[0, 20, 40, 641]), 0);
    }

    #[test]
    fn a_reversal_ends_the_streak() {
        let t0 = Instant::now();
        let mut swipes = SwipeTracker::default();
        for ms in [0, 20, 40, 340] {
            swipes.observe(&down(), at(t0, ms));
        }
        let up = Tick { sign: -1, ..down() };
        assert_eq!(swipes.observe(&up, at(t0, 360)), 0);
    }

    #[test]
    fn reset_forgets_the_streak() {
        let t0 = Instant::now();
        let mut swipes = SwipeTracker::default();
        for ms in [0, 20, 40] {
            swipes.observe(&down(), at(t0, ms));
        }
        swipes.reset();
        assert_eq!(swipes.observe(&down(), at(t0, 340)), 0);
    }

    #[test]
    fn the_swift_factor_grows_by_a_third_per_swipe_up_to_four() {
        // 1.33^0 = 1, 1.33^1 = 1.33, 1.33^2 = 1.7689, 1.33^5 = 4.16 → capped at 4
        assert_eq!(swift_factor(0, &down()), 1.0);
        assert!((swift_factor(1, &down()) - 1.33).abs() < 1e-12);
        assert!((swift_factor(2, &down()) - 1.7689).abs() < 1e-12);
        assert_eq!(swift_factor(5, &down()), 4.0);
        assert_eq!(swift_factor(u32::MAX, &down()), 4.0);
    }

    #[test]
    fn a_held_swift_key_doubles_at_least() {
        let swift = Tick {
            swift: true,
            ..down()
        };
        assert_eq!(swift_factor(0, &swift), 2.0);
        // 1.33^3 = 2.352637 beats 2
        assert!((swift_factor(3, &swift) - 2.352637).abs() < 1e-6);
    }

    #[test]
    fn the_precise_key_overrides_swift() {
        let both = Tick {
            swift: true,
            precise: true,
            ..down()
        };
        assert_eq!(swift_factor(3, &both), 1.0);
    }
}
