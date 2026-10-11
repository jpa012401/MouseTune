//! Test doubles shared across unit tests.

use std::{
    io,
    sync::{
        Arc, Mutex,
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

use crate::input::devices::DeviceInfo;

/// A third-party 5-button wheel mouse. Sender IDs are `id * 10` and `id * 10 + 1`.
pub fn mouse(id: u64) -> DeviceInfo {
    DeviceInfo {
        id,
        sender_ids: vec![id * 10, id * 10 + 1],
        name: "MX Master 3".into(),
        manufacturer: "Logitech".into(),
        vendor_id: 0x046D,
        product_id: 0x4082,
        button_count: 5,
        has_wheel: true,
    }
}

/// Apple's Magic Trackpad (excluded).
pub fn apple_trackpad(id: u64) -> DeviceInfo {
    DeviceInfo {
        id,
        sender_ids: vec![id * 10],
        name: "Magic Trackpad".into(),
        manufacturer: "Apple Inc.".into(),
        vendor_id: 0x05AC,
        product_id: 0x0265,
        button_count: 1,
        has_wheel: false,
    }
}

use crate::input::taps::{TapController, TapKind, TapSet};

#[derive(Default)]
struct FakeTapState {
    enabled: TapSet,
    failing: TapSet,
    calls: Vec<(TapKind, bool)>,
}

/// In-memory taps. Clones share state, so a test keeps a handle after
/// moving one into the code under test.
#[derive(Clone, Default)]
pub struct FakeTapController(Arc<Mutex<FakeTapState>>);

impl FakeTapController {
    /// Tap creation for `kind` fails until `succeed` is called.
    pub fn fail(&self, kind: TapKind) {
        self.0.lock().unwrap().failing.set(kind, true);
    }
    pub fn succeed(&self, kind: TapKind) {
        self.0.lock().unwrap().failing.set(kind, false);
    }
    /// Simulates macOS disabling a tap behind our back.
    pub fn os_disables(&self, kind: TapKind) {
        self.0.lock().unwrap().enabled.set(kind, false);
    }
    pub fn calls(&self) -> Vec<(TapKind, bool)> {
        self.0.lock().unwrap().calls.clone()
    }
}

impl TapController for FakeTapController {
    fn set_enabled(&mut self, kind: TapKind, enabled: bool) -> bool {
        let mut state = self.0.lock().unwrap();
        state.calls.push((kind, enabled));
        if enabled && state.failing.get(kind) {
            return false;
        }
        state.enabled.set(kind, enabled);
        true
    }
    fn is_enabled(&self, kind: TapKind) -> bool {
        self.0.lock().unwrap().enabled.get(kind)
    }
}
