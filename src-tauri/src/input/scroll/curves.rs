//! The scroll animation curve (Phase 5a design §3, spec §8.3). It has an
//! ease-out "base" over a fixed duration. For regular and high smoothness,
//! a drag-decay tail `v' = −c·vᵉ` follows and stops below `v_stop`.
//! Pure. Times are in seconds, distances in pixels. `Curve::new` runs in
//! the scroll tap callback: bounded work, no allocation.

use crate::config::schema::Smoothness;

/// Slopes of the hybrid base's Hermite ease-out, in units of D_b / T.
/// Tuning knobs (design §3). With these the base is monotone.
const BASE_START_SLOPE: f64 = 2.0;
const BASE_END_SLOPE: f64 = 0.5;
/// Fixed, so `Curve::new` is O(1).
const BISECTION_STEPS: u32 = 32;
/// The tail stops below this speed (px/s).
const STOP_SPEED: f64 = 30.0;

/// The tail `v' = −c·vᵉ`, stopping below `v_stop`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub c: f64,
    pub e: f64,
    pub v_stop: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveParams {
    pub base_secs: f64,
    /// `None`: base only.
    pub drag: Option<Drag>,
}

/// The spec §8.3 table. `x` is the wheel's spin fraction (0..=1).
pub fn params(smoothness: Smoothness, x: f64) -> CurveParams {
    match smoothness {
        Smoothness::Low => CurveParams {
            base_secs: 0.090,
            drag: None,
        },
        Smoothness::Regular => CurveParams {
            base_secs: 0.160 - 0.070 * x,
            drag: Some(Drag {
                c: 23.0,
                e: 1.0,
                v_stop: STOP_SPEED,
            }),
        },
        Smoothness::High => CurveParams {
            base_secs: 0.220,
            drag: Some(Drag {
                c: 40.0,
                e: 0.7,
                v_stop: STOP_SPEED,
            }),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    distance: f64,
    base_secs: f64,
    shape: Shape,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    /// `D · (1 − (1 − u)³)`: ends at rest.
    EaseOut,
    /// A Hermite ease-out over `base_distance` that ends at speed `v0`,
    /// then the drag tail from `v0`.
    Hybrid {
        base_distance: f64,
        v0: f64,
        drag: Drag,
        tail_secs: f64,
    },
}

impl Curve {
    pub fn new(distance: f64, params: CurveParams) -> Self {
        let distance = distance.max(0.0);
        let t = params.base_secs;
        let shape = params
            .drag
            .and_then(|drag| hybrid(distance, t, drag))
            .unwrap_or(Shape::EaseOut);
        Self {
            distance,
            base_secs: t,
            shape,
        }
    }

    pub fn distance(&self) -> f64 {
        self.distance
    }

    pub fn base_secs(&self) -> f64 {
        self.base_secs
    }

    pub fn has_tail(&self) -> bool {
        matches!(self.shape, Shape::Hybrid { .. })
    }

    pub fn duration(&self) -> f64 {
        match self.shape {
            Shape::EaseOut => self.base_secs,
            Shape::Hybrid { tail_secs, .. } => self.base_secs + tail_secs,
        }
    }

    /// Position at `t` seconds, clamped to the curve's start and end.
    pub fn position(&self, t: f64) -> f64 {
        // Not `clamp`, which panics if the duration is NaN or negative.
        let t = t.max(0.0).min(self.duration());
        let u = (t / self.base_secs).min(1.0);
        match self.shape {
            Shape::EaseOut => self.distance * (1.0 - (1.0 - u).powi(3)),
            Shape::Hybrid {
                base_distance,
                v0,
                drag,
                ..
            } => {
                if t <= self.base_secs {
                    base_distance * hermite(u)
                } else {
                    base_distance + tail_position(v0, drag, t - self.base_secs)
                }
            }
        }
    }

    #[cfg(test)]
    pub fn velocity(&self, t: f64) -> f64 {
        let u = (t / self.base_secs).clamp(0.0, 1.0);
        match self.shape {
            Shape::EaseOut => 3.0 * self.distance * (1.0 - u).powi(2) / self.base_secs,
            Shape::Hybrid {
                base_distance,
                v0,
                drag,
                ..
            } => {
                if t <= self.base_secs {
                    base_distance * hermite_slope(u) / self.base_secs
                } else {
                    tail_speed(v0, drag, t - self.base_secs)
                }
            }
        }
    }
}

/// Splits `distance` so the base ends at the speed the tail starts from:
/// `D_b + tail_distance(v₀(D_b)) = D`, with `v₀ = D_b · BASE_END_SLOPE / T`.
/// The left side grows with `D_b`, so a fixed bisection finds it. `None`
/// when even the whole distance is too slow for a tail.
fn hybrid(distance: f64, t: f64, drag: Drag) -> Option<Shape> {
    let v0_at = |base: f64| base * BASE_END_SLOPE / t;
    if v0_at(distance) <= drag.v_stop {
        return None;
    }
    let (mut lo, mut hi) = (0.0, distance);
    for _ in 0..BISECTION_STEPS {
        let mid = (lo + hi) / 2.0;
        if mid + tail_distance(v0_at(mid), drag) < distance {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let base_distance = (lo + hi) / 2.0;
    let v0 = v0_at(base_distance);
    Some(Shape::Hybrid {
        base_distance,
        v0,
        drag,
        tail_secs: tail_secs(v0, drag),
    })
}

/// `h(0) = 0`, `h(1) = 1`, `h′(0) = BASE_START_SLOPE`, `h′(1) = BASE_END_SLOPE`.
fn hermite(u: f64) -> f64 {
    let (a, b) = (BASE_START_SLOPE, BASE_END_SLOPE);
    (a + b - 2.0) * u.powi(3) + (3.0 - 2.0 * a - b) * u * u + a * u
}

#[cfg(test)]
fn hermite_slope(u: f64) -> f64 {
    let (a, b) = (BASE_START_SLOPE, BASE_END_SLOPE);
    3.0 * (a + b - 2.0) * u * u + 2.0 * (3.0 - 2.0 * a - b) * u + a
}

/// Distance the tail covers from `v0` down to `v_stop` (closed form).
fn tail_distance(v0: f64, d: Drag) -> f64 {
    if v0 <= d.v_stop {
        0.0
    } else if d.e == 1.0 {
        (v0 - d.v_stop) / d.c
    } else {
        (v0.powf(2.0 - d.e) - d.v_stop.powf(2.0 - d.e)) / (d.c * (2.0 - d.e))
    }
}

/// How long the tail takes from `v0` down to `v_stop`.
fn tail_secs(v0: f64, d: Drag) -> f64 {
    if v0 <= d.v_stop {
        0.0
    } else if d.e == 1.0 {
        (v0 / d.v_stop).ln() / d.c
    } else {
        (v0.powf(1.0 - d.e) - d.v_stop.powf(1.0 - d.e)) / ((1.0 - d.e) * d.c)
    }
}

/// Speed `tau` seconds into the tail.
fn tail_speed(v0: f64, d: Drag, tau: f64) -> f64 {
    if d.e == 1.0 {
        v0 * (-d.c * tau).exp()
    } else {
        (v0.powf(1.0 - d.e) - (1.0 - d.e) * d.c * tau)
            .max(0.0)
            .powf(1.0 / (1.0 - d.e))
    }
}

/// Distance covered `tau` seconds into the tail.
fn tail_position(v0: f64, d: Drag, tau: f64) -> f64 {
    let v = tail_speed(v0, d, tau);
    if d.e == 1.0 {
        (v0 - v) / d.c
    } else {
        (v0.powf(2.0 - d.e) - v.powf(2.0 - d.e)) / (d.c * (2.0 - d.e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::Smoothness::{High, Low, Regular};

    fn close(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() < tolerance
    }

    #[test]
    fn a_degenerate_duration_never_panics_on_the_frame_path() {
        // Unreachable from the §8.3 table, but `position` runs on every frame.
        let negative = Curve::new(
            10.0,
            CurveParams {
                base_secs: -0.01,
                drag: None,
            },
        );
        // t = max(0.05, 0) then min(−0.01) = −0.01; u = −0.01 / −0.01 = 1,
        // so 10 · (1 − 0³) = 10: it lands on its distance.
        assert_eq!(negative.position(0.05), 10.0);
        let nan = Curve::new(
            10.0,
            CurveParams {
                base_secs: f64::NAN,
                drag: None,
            },
        );
        let _ = nan.position(0.05);
    }

    #[test]
    fn low_is_a_cubic_ease_out_over_90_ms() {
        let c = Curve::new(90.0, params(Low, 0.0));
        assert!(!c.has_tail());
        assert!(close(c.duration(), 0.090, 1e-12));
        // u = 0.5: 90 · (1 − 0.5³) = 78.75
        assert!(close(c.position(0.045), 78.75, 1e-9));
        assert!(close(c.position(0.090), 90.0, 1e-9));
        assert!(close(c.position(5.0), 90.0, 1e-9), "clamped after the end");
        assert_eq!(c.position(-1.0), 0.0, "clamped before the start");
    }

    #[test]
    fn regular_splits_90_px_into_a_base_and_a_drag_tail() {
        // With e = 1 the split has a closed form:
        // D_b = (D + v_s/c) / (1 + 0.5/(T·c)) = (90 + 30/23) / (1 + 0.5/3.68) = 80.38278
        // v₀ = D_b · 0.5 / T = 251.19617 px/s; tail = ln(v₀/30) / 23 = 0.092393 s
        let c = Curve::new(90.0, params(Regular, 0.0));
        assert!(c.has_tail());
        assert!(close(c.base_secs(), 0.160, 1e-12));
        assert!(close(c.position(0.160), 80.38278, 1e-4));
        assert!(close(c.duration(), 0.252393, 1e-5));
        assert!(close(c.position(c.duration()), 90.0, 1e-6));
        // Hermite at u = 0.5: 0.5/8 − 1.5/4 + 2/2 = 0.6875
        assert!(close(c.position(0.080), 80.38278 * 0.6875, 1e-3));
    }

    #[test]
    fn a_faster_wheel_shortens_the_regular_base() {
        // T = 160 ms − 70 ms · x
        assert!(close(params(Regular, 1.0).base_secs, 0.090, 1e-12));
        assert!(close(params(Regular, 0.5).base_secs, 0.125, 1e-12));
    }

    #[test]
    fn high_coasts_longer_than_regular() {
        // Solved independently (bisection, e = 0.7, c = 40, T = 220 ms):
        // D_b = 76.0162, v₀ = 172.764 px/s, tail 0.159708 s.
        let c = Curve::new(90.0, params(High, 0.0));
        assert!(close(c.duration(), 0.379708, 1e-4));
        assert!(close(c.position(0.220), 76.0162, 1e-3));
    }

    #[test]
    fn a_short_distance_has_no_tail() {
        // 9 px: v₀ at D_b = D is 9 · 0.5 / 0.16 = 28.1 px/s, below the 30 px/s stop speed.
        let c = Curve::new(9.0, params(Regular, 0.0));
        assert!(!c.has_tail());
        assert!(close(c.duration(), 0.160, 1e-12));
        assert!(close(c.position(0.160), 9.0, 1e-9));
    }

    #[test]
    fn zero_and_negative_distances_stay_put() {
        assert_eq!(Curve::new(0.0, params(Regular, 0.0)).position(0.1), 0.0);
        assert_eq!(Curve::new(-5.0, params(High, 0.0)).position(0.1), 0.0);
    }

    #[test]
    fn every_curve_is_monotone_continuous_finite_and_lands_on_its_distance() {
        for smoothness in [Low, Regular, High] {
            for x in [0.0, 1.0] {
                for d in [1.0, 20.0, 90.0, 240.0, 2000.0] {
                    let c = Curve::new(d, params(smoothness, x));
                    let label = format!("{smoothness:?} x={x} d={d}");
                    assert!(c.duration() < 2.0, "{label}: ends in finite time");
                    let mut last = 0.0;
                    let mut t = 0.0;
                    while t <= c.duration() + 0.001 {
                        let p = c.position(t);
                        assert!(p >= last - 1e-9, "{label}: monotone at {t}");
                        last = p;
                        t += 0.0005;
                    }
                    assert!(
                        close(c.position(c.duration()), d, 1.0),
                        "{label}: lands on d"
                    );
                    if c.has_tail() {
                        let b = c.base_secs();
                        let (before, after) = (c.velocity(b - 1e-7), c.velocity(b + 1e-7));
                        assert!(
                            close(before, after, before * 0.01),
                            "{label}: {before} vs {after}"
                        );
                    }
                }
            }
        }
    }
}
