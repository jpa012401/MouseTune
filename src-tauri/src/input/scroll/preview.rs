//! The Scroll screen's curve plot (Phase 5b design §4): the engine's own
//! curve for one slow and one fast tick, sampled for drawing. Pure.

use serde::Serialize;
use specta::Type;

use super::{
    acceleration::{Axis, Tick, tick_px},
    curves::{Curve, params},
};
use crate::config::schema::Scroll;

const SAMPLE_MS: f64 = 4.0;

/// `[milliseconds, pixels]` points for a tick at the slowest and the
/// fastest wheel rate.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct CurvePreview {
    pub slow: Vec<[f64; 2]>,
    pub fast: Vec<[f64; 2]>,
}

pub fn curve_preview(scroll: &Scroll) -> CurvePreview {
    CurvePreview {
        slow: sampled(scroll, 0.0),
        fast: sampled(scroll, 1.0),
    }
}

/// A plain one-line tick (no key held). Under `speed = system` that's
/// macOS's 10 px line, since there's no real point delta to plot.
fn sampled(scroll: &Scroll, x: f64) -> Vec<[f64; 2]> {
    let tick = Tick {
        axis: Axis::Vertical,
        sign: 1,
        lines: 1,
        points: 0,
        precise: false,
        swift: false,
    };
    let curve = Curve::new(
        tick_px(scroll.speed, scroll.precise, x, &tick),
        params(scroll.smoothness, x),
    );
    let end_ms = curve.duration() * 1000.0;
    let steps = (end_ms / SAMPLE_MS).ceil() as usize;
    (0..=steps)
        .map(|i| {
            let ms = (i as f64 * SAMPLE_MS).min(end_ms);
            [ms, curve.position(ms / 1000.0)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{ScrollSpeed, Smoothness};

    fn last(points: &[[f64; 2]]) -> [f64; 2] {
        *points.last().expect("at least one point")
    }

    #[test]
    fn a_slow_regular_medium_tick_moves_90_px_over_252_ms() {
        // Phase 5a curve: 90 px, duration 252.393 ms (closed form, see curves tests).
        let p = curve_preview(&Scroll::default());
        assert_eq!(p.slow[0], [0.0, 0.0]);
        let [ms, px] = last(&p.slow);
        assert!((ms - 252.393).abs() < 1e-3, "{ms}");
        assert!((px - 90.0).abs() < 1e-6, "{px}");
        // ceil(252.393 / 4) = 64 steps → 65 points
        assert_eq!(p.slow.len(), 65);
    }

    #[test]
    fn a_fast_tick_moves_the_speed_maximum() {
        let [_, px] = last(&curve_preview(&Scroll::default()).fast);
        assert!((px - 180.0).abs() < 1e-6, "{px}");
    }

    #[test]
    fn points_are_4_ms_apart_and_never_move_backwards() {
        let p = curve_preview(&Scroll {
            smoothness: Smoothness::High,
            ..Scroll::default()
        });
        for line in [&p.slow, &p.fast] {
            for pair in line.windows(2) {
                let ([t0, x0], [t1, x1]) = (pair[0], pair[1]);
                assert!(t1 - t0 <= 4.0 + 1e-9 && t1 > t0, "{t0} → {t1}");
                assert!(x1 >= x0 - 1e-9, "{x0} → {x1}");
            }
        }
    }

    #[test]
    fn system_speed_plots_one_line_of_ten_pixels() {
        let p = curve_preview(&Scroll {
            speed: ScrollSpeed::System,
            ..Scroll::default()
        });
        assert!((last(&p.slow)[1] - 10.0).abs() < 1e-6);
        assert!((last(&p.fast)[1] - 10.0).abs() < 1e-6);
    }

    #[test]
    fn the_precise_setting_shrinks_the_slow_tick_to_one_pixel() {
        let p = curve_preview(&Scroll {
            precise: true,
            ..Scroll::default()
        });
        assert!((last(&p.slow)[1] - 1.0).abs() < 1e-6);
        assert!((last(&p.fast)[1] - 180.0).abs() < 1e-6);
    }
}
