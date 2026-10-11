//! Safety rails for code macOS calls back into (spec §4.5).

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicBool, Ordering},
};

/// Runs `f`, turning a panic into `fallback` and raising `degraded`.
/// A panic must never unwind into CoreGraphics.
pub fn guarded<T>(degraded: &AtomicBool, fallback: T, f: impl FnOnce() -> T) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => {
            degraded.store(true, Ordering::SeqCst);
            fallback
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledReason {
    /// The callback took too long.
    Timeout,
    /// The user deliberately disabled input taps (e.g. secure input).
    UserInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledAction {
    Reenable,
    LeaveDisabled,
}

pub fn on_tap_disabled(reason: DisabledReason) -> DisabledAction {
    match reason {
        DisabledReason::Timeout => DisabledAction::Reenable,
        DisabledReason::UserInput => DisabledAction::LeaveDisabled,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    #[test]
    fn returns_the_closures_value_and_stays_healthy() {
        let degraded = AtomicBool::new(false);
        assert_eq!(guarded(&degraded, 0, || 42), 42);
        assert!(!degraded.load(Ordering::SeqCst));
    }

    #[test]
    fn panic_yields_fallback_and_marks_degraded() {
        let degraded = AtomicBool::new(false);
        let result = guarded(&degraded, 7, || -> i32 { panic!("boom") });
        assert_eq!(result, 7);
        assert!(degraded.load(Ordering::SeqCst));
    }

    #[test]
    fn degraded_is_sticky_across_later_successes() {
        let degraded = AtomicBool::new(false);
        guarded(&degraded, (), || panic!("boom"));
        guarded(&degraded, (), || ());
        assert!(degraded.load(Ordering::SeqCst));
    }

    #[test]
    fn timeout_reenables() {
        assert_eq!(
            on_tap_disabled(DisabledReason::Timeout),
            DisabledAction::Reenable
        );
    }

    #[test]
    fn user_input_is_respected() {
        assert_eq!(
            on_tap_disabled(DisabledReason::UserInput),
            DisabledAction::LeaveDisabled
        );
    }
}
