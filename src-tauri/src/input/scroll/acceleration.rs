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

/// How far one tick scrolls, in pixels, at spin fraction `x`.
pub fn tick_px(speed: ScrollSpeed, x: f64, tick: &Tick) -> f64 {
    let (min, max) = match speed {
        ScrollSpeed::System if tick.points != 0 => return tick.points as f64,
        ScrollSpeed::System => return tick.lines as f64 * PX_PER_LINE,
        ScrollSpeed::Low => (60.0, 120.0),
        ScrollSpeed::Medium => (90.0, 180.0),
        ScrollSpeed::High => (150.0, 240.0),
    };
    // s(x) = x²: a monotone ease-in, the tuning knob (design §3).
    min + (max - min) * x * x
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
        tick_px(speed, x, &down())
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
        assert_eq!(tick_px(System, 1.0, &tick), 37.0);
    }

    #[test]
    fn system_speed_without_a_point_delta_uses_ten_px_per_line() {
        let tick = Tick {
            lines: 3,
            points: 0,
            ..down()
        };
        assert_eq!(tick_px(System, 0.0, &tick), 30.0);
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
                points: 24
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
                points: 12
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
}
