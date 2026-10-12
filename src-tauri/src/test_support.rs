//! Test doubles shared across unit tests.

use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::{
    accessibility::Accessibility,
    actions::{
        ActionSink, Job, OutputError,
        keys::{KeyBinding, KeyStroke},
        perform::{EventPoster, FrontmostApp, HotkeyState, SymbolicHotkeys},
    },
    input::router::{ClickTimers, TimerKind},
};

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

use crate::config::file::ConfigFile;
use crate::input::taps::{TapController, TapKind, TapSet};

#[derive(Default)]
struct FakeTapState {
    enabled: TapSet,
    failing: TapSet,
    wedged: TapSet,
    calls: Vec<(TapKind, bool)>,
    revives: Vec<TapKind>,
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
    /// Simulates a tap macOS disabled whose port is still valid but won't re-enable.
    pub fn wedge(&self, kind: TapKind) {
        let mut state = self.0.lock().unwrap();
        state.enabled.set(kind, false);
        state.wedged.set(kind, true);
    }
    pub fn calls(&self) -> Vec<(TapKind, bool)> {
        self.0.lock().unwrap().calls.clone()
    }
    pub fn revives(&self) -> Vec<TapKind> {
        self.0.lock().unwrap().revives.clone()
    }
}

impl TapController for FakeTapController {
    fn set_enabled(&mut self, kind: TapKind, enabled: bool) -> bool {
        let mut state = self.0.lock().unwrap();
        state.calls.push((kind, enabled));
        if enabled && (state.failing.get(kind) || state.wedged.get(kind)) {
            return false;
        }
        state.enabled.set(kind, enabled);
        true
    }
    fn revive(&mut self, kind: TapKind) -> bool {
        let mut state = self.0.lock().unwrap();
        state.revives.push(kind);
        if state.failing.get(kind) {
            return false;
        }
        state.wedged.set(kind, false);
        state.enabled.set(kind, true);
        true
    }
    fn is_enabled(&self, kind: TapKind) -> bool {
        self.0.lock().unwrap().enabled.get(kind)
    }
}

#[derive(Default)]
struct FakeConfigState {
    contents: Option<String>,
    read_fails: bool,
    write_fails: bool,
    backup_fails: bool,
    writes: Vec<String>,
    backups: usize,
}

/// In-memory settings file. Clones share state.
#[derive(Clone, Default)]
pub struct FakeConfigFile(Arc<Mutex<FakeConfigState>>);

impl FakeConfigFile {
    pub fn missing() -> Self {
        Self::default()
    }
    pub fn with(contents: &str) -> Self {
        let fake = Self::default();
        fake.0.lock().unwrap().contents = Some(contents.to_owned());
        fake
    }
    pub fn fail_reads(&self) {
        self.0.lock().unwrap().read_fails = true;
    }
    pub fn fail_writes(&self) {
        self.0.lock().unwrap().write_fails = true;
    }
    pub fn fail_backups(&self) {
        self.0.lock().unwrap().backup_fails = true;
    }
    pub fn writes(&self) -> Vec<String> {
        self.0.lock().unwrap().writes.clone()
    }
    pub fn backups(&self) -> usize {
        self.0.lock().unwrap().backups
    }
}

impl ConfigFile for FakeConfigFile {
    fn read(&self) -> io::Result<Option<String>> {
        let state = self.0.lock().unwrap();
        if state.read_fails {
            return Err(io::Error::other("read failed"));
        }
        Ok(state.contents.clone())
    }
    fn write_atomic(&self, contents: &str) -> io::Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.write_fails {
            return Err(io::Error::other("disk full"));
        }
        state.contents = Some(contents.to_owned());
        state.writes.push(contents.to_owned());
        Ok(())
    }
    fn backup(&self) -> io::Result<String> {
        let mut state = self.0.lock().unwrap();
        if state.backup_fails {
            return Err(io::Error::other("backup failed"));
        }
        state.backups += 1;
        state.contents = None;
        Ok("config.1760000000.bak.json".to_owned())
    }
}

/// Records submitted jobs; `full` makes every submit fail like a full queue.
#[derive(Clone, Default)]
pub struct FakeSink {
    pub jobs: Arc<Mutex<Vec<Job>>>,
    pub full: Arc<AtomicBool>,
}
impl ActionSink for FakeSink {
    fn submit(&self, job: Job) -> bool {
        if self.full.load(Ordering::SeqCst) {
            return false;
        }
        self.jobs.lock().unwrap().push(job);
        true
    }
}

/// Every OS output in order, as readable strings.
#[derive(Clone, Default)]
pub struct OutputLog(pub Arc<Mutex<Vec<String>>>);
impl OutputLog {
    pub fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    fn push(&self, s: String) {
        self.0.lock().unwrap().push(s)
    }
}

pub struct FakeHotkeys {
    pub log: OutputLog,
    pub state: Option<HotkeyState>,
    pub install_fails: bool,
}
impl SymbolicHotkeys for FakeHotkeys {
    fn read(&self, id: u16) -> Option<HotkeyState> {
        self.log.push(format!("read {id}"));
        self.state
    }
    fn install(&self, id: u16, b: KeyBinding) -> Result<(), OutputError> {
        self.log.push(format!(
            "install {id} key {} mods {:#x}",
            b.key_code, b.modifiers.0
        ));
        if self.install_fails {
            Err(OutputError::Os("CGSSetSymbolicHotKeyValue"))
        } else {
            Ok(())
        }
    }
}

pub struct FakePoster {
    pub log: OutputLog,
    pub panics: bool,
}
impl EventPoster for FakePoster {
    fn post_keys(&self, strokes: &[KeyStroke]) -> Result<(), OutputError> {
        if self.panics {
            panic!("poster bug");
        }
        for s in strokes {
            self.log.push(format!(
                "key {:#x} {} {:#x}",
                s.key_code,
                if s.down { "down" } else { "up" },
                s.flags.0
            ));
        }
        Ok(())
    }
    fn click(&self, b: u8) -> Result<(), OutputError> {
        self.log.push(format!("click {b}"));
        Ok(())
    }
}

pub struct FakeFrontmost(pub Option<&'static str>);
impl FrontmostApp for FakeFrontmost {
    fn bundle_id(&self) -> Option<String> {
        self.0.map(String::from)
    }
}

/// Records arm/disarm calls as strings ("arm Hold 250ms", "disarm Expiry").
#[derive(Clone, Default)]
pub struct FakeTimers(pub Arc<Mutex<Vec<String>>>);
impl FakeTimers {
    pub fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}
impl ClickTimers for FakeTimers {
    fn arm(&mut self, kind: TimerKind, after: std::time::Duration) {
        self.0
            .lock()
            .unwrap()
            .push(format!("arm {kind:?} {}ms", after.as_millis()));
    }
    fn disarm(&mut self, kind: TimerKind) {
        self.0.lock().unwrap().push(format!("disarm {kind:?}"));
    }
}
