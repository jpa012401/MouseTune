//! Test doubles shared across unit tests.

use std::{
    io,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::accessibility::Accessibility;

/// Records every OS call, in order, with its argument.
#[derive(Default)]
pub struct FakeAccessibility {
    pub trusted: AtomicBool,
    pub trusted_after_prompt: bool,
    pub open_settings_fails: bool,
    pub reset_fails: bool,
    pub(crate) calls: Mutex<Vec<String>>,
}

impl FakeAccessibility {
    pub fn set_trusted(&self, trusted: bool) {
        self.trusted.store(trusted, Ordering::SeqCst);
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, call: impl Into<String>) {
        self.calls.lock().unwrap().push(call.into());
    }
}

impl Accessibility for FakeAccessibility {
    fn is_trusted(&self) -> bool {
        self.trusted.load(Ordering::SeqCst)
    }
    fn prompt(&self) -> bool {
        self.record("prompt");
        self.trusted_after_prompt
    }
    fn open_settings(&self) -> io::Result<()> {
        self.record("open_settings");
        if self.open_settings_fails {
            Err(io::Error::other("open failed"))
        } else {
            Ok(())
        }
    }
    fn reset(&self, bundle_id: &str) -> io::Result<()> {
        self.record(format!("reset:{bundle_id}"));
        if self.reset_fails {
            Err(io::Error::other("tccutil exited with 1"))
        } else {
            Ok(())
        }
    }
}
