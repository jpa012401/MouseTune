use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
    thread::{self, JoinHandle},
};

use crossbeam_channel::Receiver;

use super::{
    ActionCounters, Job,
    perform::{Outputs, perform},
};

/// Blocks until every sender is gone. A panicking job counts as failed and
/// never stops the thread.
pub fn run(jobs: Receiver<Job>, out: Outputs, counters: Arc<ActionCounters>) {
    if out.hotkeys.is_none() {
        tracing::warn!("symbolic hotkey functions unavailable; using default shortcuts (degraded)");
    }
    for job in jobs {
        match catch_unwind(AssertUnwindSafe(|| perform(job, &out))) {
            Ok(Ok(())) => counters.record_performed(),
            Ok(Err(err)) => {
                counters.record_failed();
                tracing::warn!(?job, %err, "action failed");
            }
            Err(_) => {
                counters.record_failed();
                tracing::error!(?job, "action panicked");
            }
        }
    }
}

pub fn spawn(jobs: Receiver<Job>, out: Outputs, counters: Arc<ActionCounters>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("mt-exec".into())
        .spawn(move || run(jobs, out, counters))
        .expect("failed to spawn the mt-exec thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        actions::{
            ActionSink, ActionStats,
            keys::{KeyBinding, ModifierMask},
            perform::HotkeyState,
            queue::queue,
        },
        config::schema::{Action, NavDirection, SymbolicHotkeyId},
        test_support::{FakeFrontmost, FakeHotkeys, FakePoster, OutputLog},
    };

    fn outputs(log: &OutputLog, panics: bool) -> Outputs {
        Outputs {
            hotkeys: None,
            poster: Box::new(FakePoster {
                log: log.clone(),
                panics,
            }),
            frontmost: Box::new(FakeFrontmost(None)),
        }
    }

    #[test]
    fn performs_each_job_counts_it_and_stops_when_the_sender_goes() {
        let log = OutputLog::default();
        let (sink, rx) = queue();
        sink.submit(Job::Click { button: 2 });
        sink.submit(Job::Click { button: 3 });
        drop(sink);
        let counters = Arc::new(ActionCounters::default());
        run(rx, outputs(&log, false), Arc::clone(&counters));
        assert_eq!(log.all(), ["click 2", "click 3"]);
        assert_eq!(
            counters.stats(),
            ActionStats {
                performed: 2,
                dropped: 0,
                failed: 0
            }
        );
    }

    #[test]
    fn a_job_that_returns_an_error_is_counted_as_failed() {
        let log = OutputLog::default();
        let (sink, rx) = queue();
        sink.submit(Job::Perform(Action::SymbolicHotkey {
            id: SymbolicHotkeyId::MissionControl,
        }));
        drop(sink);
        let state = HotkeyState {
            enabled: false,
            binding: KeyBinding {
                key_code: 0xFFFF,
                modifiers: ModifierMask(0),
            },
        };
        let out = Outputs {
            hotkeys: Some(Box::new(FakeHotkeys {
                log: log.clone(),
                state: Some(state),
                install_fails: true,
            })),
            poster: Box::new(FakePoster {
                log: log.clone(),
                panics: false,
            }),
            frontmost: Box::new(FakeFrontmost(None)),
        };
        let counters = Arc::new(ActionCounters::default());
        run(rx, out, Arc::clone(&counters));
        assert_eq!(
            counters.stats(),
            ActionStats {
                performed: 0,
                dropped: 0,
                failed: 1
            }
        );
    }

    #[test]
    fn a_panicking_job_is_counted_as_failed_and_the_next_job_still_runs() {
        let log = OutputLog::default();
        let (sink, rx) = queue();
        let nav = Job::Perform(Action::Navigate {
            direction: NavDirection::Back,
        });
        sink.submit(Job::Perform(Action::SymbolicHotkey {
            id: SymbolicHotkeyId::MissionControl,
        })); // posts keys → panics
        sink.submit(nav); // no frontmost app → click 3, no key posting
        drop(sink);
        let counters = Arc::new(ActionCounters::default());
        run(rx, outputs(&log, true), Arc::clone(&counters));
        assert_eq!(log.all(), ["click 3"]);
        assert_eq!(
            counters.stats(),
            ActionStats {
                performed: 1,
                dropped: 0,
                failed: 1
            }
        );
    }
}
