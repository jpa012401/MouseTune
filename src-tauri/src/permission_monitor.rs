//! Watches Accessibility trust and broadcasts changes (spec §3.1).

use std::{sync::Arc, thread, time::Duration};

use crate::accessibility::Accessibility;

const UNTRUSTED_INTERVAL: Duration = Duration::from_millis(500);
const TRUSTED_INTERVAL: Duration = Duration::from_secs(5);

/// Anything that reacts to the permission being granted or revoked.
pub trait PermissionObserver: Send {
    fn permission_changed(&self, trusted: bool);
}

/// Poll timing and change detection. Polls quickly while untrusted so
/// onboarding reacts as soon as the user flips the switch, and slowly while
/// trusted to notice revocation.
pub struct PermissionWatch {
    trusted: bool,
}

impl PermissionWatch {
    pub fn new(trusted: bool) -> Self {
        Self { trusted }
    }

    pub fn poll_interval(&self) -> Duration {
        if self.trusted {
            TRUSTED_INTERVAL
        } else {
            UNTRUSTED_INTERVAL
        }
    }

    /// Returns the new state if it differs from the last one observed.
    pub fn observe(&mut self, trusted: bool) -> Option<bool> {
        if trusted == self.trusted {
            return None;
        }
        self.trusted = trusted;
        Some(trusted)
    }
}

pub struct PermissionMonitor {
    watch: PermissionWatch,
    accessibility: Arc<dyn Accessibility>,
    observers: Vec<Box<dyn PermissionObserver>>,
}

impl PermissionMonitor {
    pub fn new(
        accessibility: Arc<dyn Accessibility>,
        observers: Vec<Box<dyn PermissionObserver>>,
    ) -> Self {
        Self {
            watch: PermissionWatch::new(accessibility.is_trusted()),
            accessibility,
            observers,
        }
    }

    /// Reads the current trust once and notifies observers if it changed.
    pub fn check(&mut self) {
        let Some(trusted) = self.watch.observe(self.accessibility.is_trusted()) else {
            return;
        };
        tracing::info!(trusted, "Accessibility permission changed");
        for observer in &self.observers {
            observer.permission_changed(trusted);
        }
    }

    pub fn spawn(mut self) {
        thread::Builder::new()
            .name("mt-permissions".into())
            .spawn(move || {
                loop {
                    thread::sleep(self.watch.poll_interval());
                    self.check();
                }
            })
            .expect("failed to spawn permission monitor thread");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::test_support::FakeAccessibility;

    #[test]
    fn watch_reports_only_flips() {
        let mut watch = PermissionWatch::new(false);
        assert_eq!(watch.observe(false), None);
        assert_eq!(watch.observe(true), Some(true));
        assert_eq!(watch.observe(true), None);
        assert_eq!(watch.observe(false), Some(false));
    }

    #[test]
    fn onboarding_reacts_within_half_a_second() {
        assert!(PermissionWatch::new(false).poll_interval() <= Duration::from_millis(500));
    }

    #[test]
    fn revocation_is_noticed_within_five_seconds_but_polled_less_often() {
        let mut watch = PermissionWatch::new(false);
        let waiting = watch.poll_interval();
        watch.observe(true);
        let trusted = watch.poll_interval();
        assert!(trusted <= Duration::from_secs(5));
        assert!(trusted > waiting);
    }

    struct Recorder(Arc<Mutex<Vec<bool>>>);

    impl PermissionObserver for Recorder {
        fn permission_changed(&self, trusted: bool) {
            self.0.lock().unwrap().push(trusted);
        }
    }

    #[test]
    fn monitor_notifies_every_observer_once_per_change() {
        let ax = Arc::new(FakeAccessibility::default());
        let (a, b) = (Arc::default(), Arc::default());
        let mut monitor = PermissionMonitor::new(
            ax.clone(),
            vec![
                Box::new(Recorder(Arc::clone(&a))),
                Box::new(Recorder(Arc::clone(&b))),
            ],
        );

        monitor.check(); // still untrusted: no notification
        ax.set_trusted(true);
        monitor.check();
        monitor.check(); // unchanged
        ax.set_trusted(false);
        monitor.check();

        assert_eq!(*a.lock().unwrap(), [true, false]);
        assert_eq!(*b.lock().unwrap(), [true, false]);
    }

    #[test]
    fn monitor_starts_from_current_state() {
        let ax = Arc::new(FakeAccessibility::default());
        ax.set_trusted(true);
        let seen = Arc::default();
        let mut monitor =
            PermissionMonitor::new(ax.clone(), vec![Box::new(Recorder(Arc::clone(&seen)))]);

        monitor.check();

        assert!(
            seen.lock().unwrap().is_empty(),
            "already trusted at start is not a change"
        );
    }
}
