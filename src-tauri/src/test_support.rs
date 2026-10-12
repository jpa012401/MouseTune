//! Test doubles shared across unit tests.

use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use crate::{
    accessibility::Accessibility,
    actions::{
        ActionSink, CompiledAction, Job, KeyCapture, NoticeObserver, OutputError,
        keys::{KeyBinding, KeyStroke},
        perform::{EventPoster, FrontmostApp, HotkeyState, SymbolicHotkeys},
    },
    config::schema::{Action, Buttons, MouseButton, SystemKey},
    input::{
        capture::Clock,
        router::{ClickTimers, TimerKind},
    },
};

/// The Phase 4 default buttons: B3 click → Mission Control, B3 hold → Show
/// Desktop, B4/B5 → Back/Forward, and no drags. For tests written against
/// those defaults, which Phase 6 changed.
pub fn phase4_buttons() -> Buttons {
    use crate::config::schema::{
        ClickTrigger, Effect, NavDirection, Remap, SymbolicHotkeyId, TriggerDuration,
    };
    let remap = |button, duration, action| Remap {
        trigger: ClickTrigger {
            button,
            level: 1,
            duration,
        },
        effect: Effect { action },
    };
    Buttons {
        remaps: vec![
            remap(
                3,
                TriggerDuration::Click,
                Action::SymbolicHotkey {
                    id: SymbolicHotkeyId::MissionControl,
                },
            ),
            remap(
                3,
                TriggerDuration::Hold,
                Action::SymbolicHotkey {
                    id: SymbolicHotkeyId::ShowDesktop,
                },
            ),
            remap(
                4,
                TriggerDuration::Click,
                Action::Navigate {
                    direction: NavDirection::Back,
                },
            ),
            remap(
                5,
                TriggerDuration::Click,
                Action::Navigate {
                    direction: NavDirection::Forward,
                },
            ),
        ],
        drags: vec![],
        scroll_bindings: vec![],
        hold_through: vec![],
    }
}

/// The config form of a compiled action, for building `Remap`s in tests.
pub fn config_action(action: CompiledAction) -> Action {
    match action {
        CompiledAction::SymbolicHotkey(id) => Action::SymbolicHotkey { id },
        CompiledAction::Navigate(direction) => Action::Navigate { direction },
        // The label never reaches execution, so any fixed text will do.
        CompiledAction::KeyCombo(binding) => Action::KeyCombo {
            key_code: binding.key_code,
            modifiers: Vec::new(),
            key_label: format!("{:X}", binding.key_code),
        },
        CompiledAction::MouseClick { cg_button, clicks } => Action::MouseClick {
            button: match cg_button {
                0 => MouseButton::Left,
                1 => MouseButton::Right,
                3 => MouseButton::Back,
                4 => MouseButton::Forward,
                _ => MouseButton::Middle,
            },
            clicks,
        },
        CompiledAction::SystemKey(key) => Action::SystemKey { key },
    }
}

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
    fn click(&self, b: u8, clicks: u8) -> Result<(), OutputError> {
        self.log.push(if clicks == 1 {
            format!("click {b}")
        } else {
            format!("click {b} x{clicks}")
        });
        Ok(())
    }
    fn post_system_key(&self, key: SystemKey) -> Result<(), OutputError> {
        self.log.push(format!("system key {key:?}"));
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

/// A clock that moves only when told. Clones share the time.
#[derive(Clone)]
pub struct FakeClock(Arc<Mutex<Instant>>);
impl FakeClock {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }
    pub fn advance(&self, by: std::time::Duration) {
        *self.0.lock().unwrap() += by;
    }
}
impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

/// Logs each notice: "button 3", "key 1 [Shift, Command] S", "cancelled".
pub struct FakeNotices(pub OutputLog);
impl NoticeObserver for FakeNotices {
    fn button_captured(&self, button: u8) {
        self.0.push(format!("button {button}"));
    }
    fn key_captured(&self, c: KeyCapture) {
        self.0.push(format!(
            "key {} {:?} {}",
            c.key_code, c.modifiers, c.key_label
        ));
    }
    fn key_capture_cancelled(&self) {
        self.0.push("cancelled".into());
    }
}

use crate::input::scroll::animator::{MomentumPhase, ScrollOut};

/// One scroll event as text: its phase ("Began", "momentum End"), then the
/// axis that moved, if any ("Changed dy 79", "Began dx -38").
pub fn describe_scroll(e: &ScrollOut) -> String {
    let phase = match e.momentum {
        MomentumPhase::None => format!("{:?}", e.phase),
        m => format!("momentum {m:?}"),
    };
    match (e.dy, e.dx) {
        (0, 0) => phase,
        (dy, 0) => format!("{phase} dy {dy}"),
        (0, dx) => format!("{phase} dx {dx}"),
        (dy, dx) => format!("{phase} dy {dy} dx {dx}"),
    }
}

use crate::input::scroll::{FrameDriver, ScrollPoster};

/// Records "request" and "stop" in order. Clones share the log.
#[derive(Clone, Default)]
pub struct FakeFrames(Arc<Mutex<Vec<&'static str>>>);
impl FakeFrames {
    pub fn calls(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().clone()
    }
}
impl FrameDriver for FakeFrames {
    fn request(&mut self) {
        self.0.lock().unwrap().push("request");
    }
    fn stop(&mut self) {
        self.0.lock().unwrap().push("stop");
    }
}

/// Records posted scroll events as `describe_scroll` text. Clones share the log.
#[derive(Clone, Default)]
pub struct FakeScrollPoster(Arc<Mutex<Vec<String>>>);
impl FakeScrollPoster {
    pub fn log(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}
impl ScrollPoster for FakeScrollPoster {
    fn post(&mut self, event: &ScrollOut) {
        self.0.lock().unwrap().push(describe_scroll(event));
    }
}

use crate::input::intents::{InputPoster, IntentSignal, PointerFreeze};

/// Records "freeze" and "unfreeze" in order. Clones share the log.
#[derive(Clone, Default)]
pub struct FakeFreeze(Arc<Mutex<Vec<&'static str>>>);
impl FakeFreeze {
    pub fn calls(&self) -> Vec<&'static str> {
        self.0.lock().unwrap().clone()
    }
}
impl PointerFreeze for FakeFreeze {
    fn set_frozen(&mut self, frozen: bool) {
        self.0
            .lock()
            .unwrap()
            .push(if frozen { "freeze" } else { "unfreeze" });
    }
}

/// Records "down 2", "up 2" and "key 0x30 down 0x100000". Clones share the log.
#[derive(Clone, Default)]
pub struct FakeInputPoster(Arc<Mutex<Vec<String>>>);
impl FakeInputPoster {
    pub fn log(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}
impl InputPoster for FakeInputPoster {
    fn button(&mut self, cg_button: u8, down: bool) {
        self.0.lock().unwrap().push(if down {
            format!("down {cg_button}")
        } else {
            format!("up {cg_button}")
        });
    }
    fn keys(&mut self, strokes: &[KeyStroke]) {
        let mut log = self.0.lock().unwrap();
        for s in strokes {
            log.push(format!(
                "key {:#x} {} {:#x}",
                s.key_code,
                if s.down { "down" } else { "up" },
                s.flags.0
            ));
        }
    }
}

/// Counts signals. Clones share the count.
#[derive(Clone, Default)]
pub struct FakeSignal(Arc<std::sync::atomic::AtomicUsize>);
impl FakeSignal {
    pub fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}
impl IntentSignal for FakeSignal {
    fn signal(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Fake gesture ports for tests that don't look at them.
pub fn fake_gestures() -> crate::input::intents::GestureSetup {
    crate::input::intents::GestureSetup {
        freeze: Box::new(FakeFreeze::default()),
        poster: Box::new(FakeInputPoster::default()),
        signal: Box::new(FakeSignal::default()),
    }
}
