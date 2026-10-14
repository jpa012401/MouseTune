//! Capture mode (spec §10): while armed, the next press of mouse
//! button 3 or higher is reported instead of acting. Pure and
//! allocation-free: `take` runs in the button tap callback.

use std::time::{Duration, Instant};

/// How long capture waits for a press before ending by itself.
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);

/// Port: the current time, so tests can move it.
pub trait Clock {
    fn now(&self) -> Instant;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Debug, Default)]
pub struct ButtonCapture {
    until: Option<Instant>,
}

impl ButtonCapture {
    pub fn start(&mut self, now: Instant) {
        self.until = now.checked_add(CAPTURE_TIMEOUT);
    }

    pub fn stop(&mut self) {
        self.until = None;
    }

    pub fn is_armed(&self) -> bool {
        self.until.is_some()
    }

    /// For an untagged, non-foreign down: true if this press is captured.
    /// Capture ends either way once used or past its deadline.
    pub fn take(&mut self, cg_button: u8, now: Instant) -> bool {
        let Some(until) = self.until else {
            return false;
        };
        if now > until {
            self.until = None;
            return false;
        }
        // Left and right clicks keep working while capture waits; 32 and up
        // don't fit the router's swallowed bitmask.
        if (2..32).contains(&cg_button) {
            self.until = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_takes_the_next_other_button_and_then_ends() {
        let t = Instant::now();
        let mut c = ButtonCapture::default();
        c.start(t);
        assert!(c.take(2, t));
        assert!(!c.is_armed());
        assert!(!c.take(2, t));
    }

    #[test]
    fn capture_ends_by_itself_after_ten_seconds() {
        let t = Instant::now();
        let mut c = ButtonCapture::default();
        c.start(t);
        assert!(!c.take(2, t + Duration::from_millis(10_001)));
        assert!(!c.is_armed());
    }

    #[test]
    fn at_exactly_ten_seconds_capture_still_takes_the_press() {
        let t = Instant::now();
        let mut c = ButtonCapture::default();
        c.start(t);
        assert!(c.take(3, t + Duration::from_secs(10)));
    }

    #[test]
    fn stop_ends_capture() {
        let t = Instant::now();
        let mut c = ButtonCapture::default();
        c.start(t);
        c.stop();
        assert!(!c.take(2, t));
    }

    #[test]
    fn left_right_and_out_of_range_buttons_are_never_captured() {
        let t = Instant::now();
        let mut c = ButtonCapture::default();
        c.start(t);
        assert!(!c.take(1, t));
        assert!(!c.take(32, t));
        assert!(c.is_armed());
    }
}
