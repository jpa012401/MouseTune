//! Action execution (spec §7, design §3). The input engine submits `Job`s
//! through `ActionSink`; `mt-exec` performs them against OS ports.

pub mod executor;
pub mod keys;
pub mod perform;
pub mod queue;

use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;
use specta::Type;

use crate::config::schema::{Action, NavDirection, SymbolicHotkeyId};

/// The engine's copy of an action: plain data the tap callback can copy
/// without touching the heap. Built by `input::remaps::compile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledAction {
    SymbolicHotkey(SymbolicHotkeyId),
    Navigate(NavDirection),
}

impl From<&Action> for CompiledAction {
    fn from(action: &Action) -> Self {
        match *action {
            Action::SymbolicHotkey { id } => Self::SymbolicHotkey(id),
            Action::Navigate { direction } => Self::Navigate(direction),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    Perform(CompiledAction),
    /// A plain click of this CG button number.
    Click {
        button: u8,
    },
}

/// Port the input engine submits through. `false` = dropped (queue full or closed).
pub trait ActionSink {
    fn submit(&self, job: Job) -> bool;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Type)]
pub struct ActionStats {
    pub performed: u32,
    pub dropped: u32,
    pub failed: u32,
}

#[derive(Debug, Default)]
pub struct ActionCounters {
    performed: AtomicU32,
    dropped: AtomicU32,
    failed: AtomicU32,
}

fn bump(counter: &AtomicU32) {
    // fetch_update with saturating_add: never wraps to 0.
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        Some(n.saturating_add(1))
    });
}

impl ActionCounters {
    pub fn record_performed(&self) {
        bump(&self.performed);
    }
    pub fn record_dropped(&self) {
        bump(&self.dropped);
    }
    pub fn record_failed(&self) {
        bump(&self.failed);
    }
    pub fn stats(&self) -> ActionStats {
        ActionStats {
            performed: self.performed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum OutputError {
    #[error("{0} failed")]
    Os(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_actions_compile_to_plain_copies() {
        use crate::config::schema::{Action, NavDirection, SymbolicHotkeyId};
        assert_eq!(
            CompiledAction::from(&Action::SymbolicHotkey {
                id: SymbolicHotkeyId::MissionControl
            }),
            CompiledAction::SymbolicHotkey(SymbolicHotkeyId::MissionControl)
        );
        assert_eq!(
            CompiledAction::from(&Action::Navigate {
                direction: NavDirection::Back
            }),
            CompiledAction::Navigate(NavDirection::Back)
        );
    }

    #[test]
    fn counters_report_each_kind_separately() {
        let c = ActionCounters::default();
        c.record_performed();
        c.record_performed();
        c.record_dropped();
        c.record_failed();
        assert_eq!(
            c.stats(),
            ActionStats {
                performed: 2,
                dropped: 1,
                failed: 1
            }
        );
    }
}
