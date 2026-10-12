//! Interactive dock swipes (Phase 6 design §3): Spaces and
//! Mission Control following the drag, through the `SwipeOutput` port.
//!
//! # Switched off until verified
//!
//! `VERIFIED_ON` lists the macOS major versions where the maintainer saw
//! the probe (`dock_swipe_moves_spaces`) move Spaces **in its default
//! (sequencer) mode**, the one that sends what the adapter sends. It is
//! empty, so
//! `platform::output_support().dock_swipe` is false, `input_loop` passes
//! `NoSwipe`, and `SpacesAndMissionControl` compiles to the Discrete
//! fallback. `MacDockSwipe` is built and tested but never constructed in
//! production until a version is added.
//!
//! The pure parts (`fields`, `Sequencer`, `ResendSchedule`) are CI-tested.
//! Everything that touches the private APIs is resolved with `dlsym` and
//! checked with `respondsToSelector:` (`Api::load`), so MouseTune links and
//! launches on macOS 12–26, where these symbols may be missing; a failed
//! load leaves dock swipes off.
//!
//! # macOS 27 encoding (what the adapter and the probe send)
//!
//! One `CGEvent` of type 30 per step, carrying an `IOHIDEvent` dock swipe.
//!
//! | Fact | Value | Source |
//! |---|---|---|
//! | HID event type "dock swipe" | `kIOHIDEventTypeDockSwipe` = **23** ("just like NavigationSwipe, but intended for consumption by Dock") | [A] `IOHIDEventTypes.h` enum |
//! | HID event type "velocity" | `kIOHIDEventTypeVelocity` = **9** | [A] `IOHIDEventTypes.h` enum |
//! | Field numbering | `IOHIDEventFieldBase(type) = type << 16`; field = base \| index | [A] `IOHIDEventTypes.h` |
//! | Dock-swipe fields | Mask = base\|0, Motion = base\|1, Progress = base\|2, PositionX/Y/Z = base\|3/4/6, Flavor = base\|5 | [A] `IOHIDEventFieldDefs.h` |
//! | Velocity fields | X = base\|0, Y = base\|1, Z = base\|2 | [A] `IOHIDEventFieldDefs.h` |
//! | Motion | `kIOHIDGestureMotionHorizontalX` = **1**, `VerticalY` = **2** | [A] `IOHIDEventTypes.h` `IOHIDGestureMotion` |
//! | Flavor | `kIOHIDGestureFlavorDockPrimary` = **3** | [A] `IOHIDEventTypes.h` `IOHIDGestureFlavor` |
//! | Swipe mask | Up = 1, Down = 2, Left = 4, Right = 8 | [A] `IOHIDEventTypes.h` `IOHIDSwipeMask` |
//! | Phase | Began = 1, Changed = 2, Ended = 4, Cancelled = 8, stored in the event's option bits 24–31 (`kIOHIDEventEventOptionPhaseShift` = 24) | [A] `IOHIDEventTypes.h` |
//! | Progress storage | `IOFixed` in `IOHIDDockSwipeEventData`, set through the double setter | [A] `IOHIDEventStructDefs.h`, `IOHIDEventMacroDefs.h` |
//! | Creating the HID event | `-[HIDEvent initWithType:timestamp:senderID:]` (HID.framework category on IOKit's `HIDEvent` class) | [B] `HID/Headers/HIDEvent.h` |
//! | Setting fields, phase, children | `-setIntegerValue:forField:`, `-setDoubleValue:forField:`, `options` property, `-appendEvent:` | [B] `HID/Headers/HIDEvent.h` |
//! | `HIDEvent` *is* the `IOHIDEventRef` | `IOHIDEventRef` is `CF_BRIDGED_TYPE(id)`; `HIDEvent` holds the event struct as its ivar | [C] `IOKitSPIIOS.h`; [D] `HIDEventBase.h` |
//! | Reading it back (tests) | `IOHIDEventType IOHIDEventGetType(IOHIDEventRef)` | [C] `IOKitSPIIOS.h` |
//! | CGEvent type 30 | `kCGSEventDockControl` = **30** (29 = `kCGSEventGesture`) | [E] `CoreGraphicsTestSPI.h` |
//! | Getter for a CGEvent's HID event (tests) | `IOHIDEventRef CGEventCopyIOHIDEvent(CGEventRef)` | [F] `CoreGraphicsSPI.h`; exported per the SDK's `CoreGraphics.tbd` |
//! | Setter for a CGEvent's HID event | `SLEventSetIOHIDEvent` is exported by SkyLight; **there is no `CGEventSetHIDEvent`/`CGEventSetIOHIDEvent` export** on the macOS 27 SDK | SDK `SkyLight.tbd`, `CoreGraphics.tbd` |
//! | Posting | `kCGSessionEventTap`, stamped with `MOUSETUNE_EVENT_TAG` like every other MouseTune event | AGENTS.md footgun 3 |
//!
//! How the adapter fills it in (`fields`):
//! - **Progress** is `SwipeOutput`'s progress unchanged: swipe units along
//!   the locked axis, screen-coordinate signs (right and down +, left and
//!   up −), 1.0 = one Space or Mission Control fully open.
//! - **Mask** follows the sign of the current progress (Left/Right or
//!   Up/Down), and is 0 while the progress is 0, as at Began, when the
//!   direction isn't known yet.
//! - **Velocity** is `SwipeOutput::end`'s velocity in progress per second,
//!   in one velocity child on the Ended event, on the locked axis.
//! - **Ended and Cancelled are re-sent** at +200 ms and +500 ms
//!   (`ResendSchedule`), so a dropped close can't leave Mission Control
//!   half open. A new Began or a Cancelled withdraws pending re-sends first.
//! - **A Began is posted only if the matching Ended (with its velocity
//!   child) also builds**, and that built Ended is kept as the swipe's
//!   closer. An end that won't build falls back to an Ended without
//!   velocity, then a Cancelled, then the closer (`pick`), so a begun
//!   gesture always gets a closing event. Re-sends re-post the event that
//!   went out.
//! - **No `CGEventSource`**: events are created with `CGEventCreate(NULL)`,
//!   in the adapter and the probe alike.
//!
//! The probe's default mode drives this same `Sequencer`, `pick` and
//! `Api::build`, and re-posts the same closing event at +200/+500 ms, so a
//! "worked" result from that mode verifies the shipped encoding. Two
//! diagnostic modes do not, and can't justify adding to `VERIFIED_ON`:
//! - `MOUSETUNE_PROBE_ENCODING=fixed-mask` sends the earlier fixed-mask
//!   encoding: the direction's mask on every step, including Began.
//! - `MOUSETUNE_PROBE_ENCODING=legacy-mix` does the same and also writes
//!   the values into the legacy gesture fields of the type-30 `CGEvent`
//!   (table below), attaching the HID event last so it is the
//!   authoritative copy.
//!
//! ## Unknown (no allowed source; documented guesses)
//!
//! - **`SLEventSetIOHIDEvent`'s signature.** Assumed
//!   `void (CGEventRef, IOHIDEventRef)`, mirroring the sourced getter. The
//!   non-posting, ignored test `dock_swipe_events_read_back` checks the
//!   round trip (set, then `CGEventCopyIOHIDEvent`); it passes on macOS
//!   27.0.1. That proves the event carries the HID event, not that the
//!   Dock acts on it.
//! - **`IOHIDEventCreateDockSwipeEvent`'s signature.** It is exported by
//!   IOKit (SDK `IOKit.tbd`), but no allowed source declares it, so the
//!   events are created generically with `initWithType:` instead.
//! - **Progress sign and scale, velocity units, and the mask at Began.**
//!   As above; the probe run shows whether the Dock agrees.
//! - **Whether the Dock needs a `senderID` or a type-29 companion event on
//!   macOS 27.** Sender 0, no companion.
//! - **Whether a stale timestamp matters on a re-send.** Re-sends re-post
//!   the event that went out, so its HID timestamp (`mach_absolute_time`
//!   when it was built) is 200 or 500 ms old by the time it is posted
//!   again. Whether the Dock ignores or rejects an old timestamp is
//!   unknown.
//!
//! # macOS 12–26 encoding (documentation only; not testable on this Mac)
//!
//! A pair of `CGEvent`s: a type-29 `kCGSEventGesture` and a type-30
//! `kCGSEventDockControl` [E], both posted at the session tap. CGEvent field
//! numbers [E]:
//!
//! | Field | Number |
//! |---|---|
//! | `kCGSEventTypeField` (event type) | 55 |
//! | `kCGEventGestureHIDType` (IOHIDEventType, so 23 for a dock swipe) | 110 |
//! | `kCGEventGestureSwipeMotion` | 123 |
//! | `kCGEventGestureSwipeProgress` | 124 |
//! | `kCGEventGestureSwipePositionX` / `Y` | 125 / 126 |
//! | `kCGEventGestureSwipeVelocityX` / `Y` / `Z` | 129 / 130 / 131 |
//! | `kCGEventGesturePhase` (1, 2, 4, 8 as above) | 132 |
//! | `kCGEventGestureMask` | 133 |
//! | `kCGEventGestureSwipeMask` | 134 |
//! | `kCGEventGestureFlavor` | 138 |
//! | `kCGEventGestureProgress` (double) | 142 |
//!
//! WebKit's event serializer [G] confirms these fields belong to event types
//! 29, 30 and 31, with the HID type in field 110. **Unknown:** what the
//! type-29 half of the pair must contain, the value of field 110 on each
//! half, and the field types of 124/129/130. The values in the table above
//! carry over from the HID encoding but are unverified on hardware. Not
//! implemented: on 12–26 `SpacesAndMissionControl` runs as Discrete.
//!
//! # Sources
//!
//! - [A] Apple open source, IOHIDFamily-1915.140.3 (`IOHIDFamily/IOHIDEventTypes.h`,
//!   `IOHIDEventFieldDefs.h`, `IOHIDEventStructDefs.h`, `IOHIDEventMacroDefs.h`),
//!   github.com/apple-oss-distributions/IOHIDFamily. The newest tag that ships
//!   these headers; later tags dropped them.
//! - [B] Apple open source, IOHIDFamily `main`, `HID/Headers/HIDEvent.h` (© 2022).
//! - [C] WebKit, `Source/WebCore/PAL/pal/spi/ios/IOKitSPIIOS.h`.
//! - [D] Apple open source, IOKitUser-100231.120.3, `hid.subproj/HIDEventBase.h`.
//! - [E] WebKit, `Tools/TestRunnerShared/spi/CoreGraphicsTestSPI.h`.
//! - [F] WebKit, `Source/WebCore/PAL/pal/spi/cg/CoreGraphicsSPI.h`.
//! - [G] WebKit, `Tools/TestRunnerShared/EventSerialization/mac/EventSerializerMac.mm`.
//! - The macOS 27.0.1 SDK's `.tbd` stubs (`xcrun --show-sdk-path`) for which
//!   symbols exist.
//!
//! Upstream Mac Mouse Fix was not consulted (spec §13).

use std::{
    cell::RefCell,
    ffi::{CStr, c_char, c_int, c_void},
    rc::Rc,
    sync::{Arc, OnceLock, atomic::AtomicBool},
};

use objc2::{
    msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, Sel},
};
use objc2_core_foundation::{CFAbsoluteTimeGetCurrent, CFRetained, CFRunLoop, CFRunLoopTimer};
use objc2_core_graphics::{CGEvent, CGEventField, CGEventTapLocation, CGEventType};

use crate::{
    input::{drag::SwipeOutput, filter::MOUSETUNE_EVENT_TAG, guard::guarded, router::DragAxis},
    platform::{
        click_timers::{NEVER_SECS, parked_timer},
        symbolic_hotkeys::resolve,
    },
};

/// macOS major versions where the maintainer saw dock swipes work, from
/// the probe's **default (sequencer) mode** only: the diagnostic modes
/// don't send what ships. Empty: no result is confirmed yet, so dock swipes
/// stay off everywhere. **Turning them on for macOS 27 is this one line,
/// `&[27]`**, plus the spec §3.4 and §6.1 wording.
pub const VERIFIED_ON: &[u32] = &[];

// ---- Encoding (pure) ----------------------------------------------------

const fn field(hid_type: u32, index: u32) -> u32 {
    (hid_type << 16) | index
}

const HID_TYPE_VELOCITY: u32 = 9;
const HID_TYPE_DOCK_SWIPE: u32 = 23;
const DOCK_SWIPE_MASK: u32 = field(HID_TYPE_DOCK_SWIPE, 0);
const DOCK_SWIPE_MOTION: u32 = field(HID_TYPE_DOCK_SWIPE, 1);
const DOCK_SWIPE_PROGRESS: u32 = field(HID_TYPE_DOCK_SWIPE, 2);
const DOCK_SWIPE_FLAVOR: u32 = field(HID_TYPE_DOCK_SWIPE, 5);
const VELOCITY_X: u32 = field(HID_TYPE_VELOCITY, 0);
const VELOCITY_Y: u32 = field(HID_TYPE_VELOCITY, 1);
const MOTION_HORIZONTAL: isize = 1;
const MOTION_VERTICAL: isize = 2;
const FLAVOR_DOCK_PRIMARY: isize = 3;
const MASK_UP: isize = 1;
const MASK_DOWN: isize = 2;
const MASK_LEFT: isize = 4;
const MASK_RIGHT: isize = 8;
const PHASE_SHIFT: u32 = 24;
const CG_DOCK_CONTROL: u32 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Began,
    Changed,
    Ended,
    Cancelled,
}

/// A velocity child event: which field, and its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VelocityField {
    pub field: u32,
    pub value: f64,
}

/// Every value one dock-swipe step writes into its HID event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SwipeFields {
    pub motion: isize,
    pub mask: isize,
    pub flavor: isize,
    pub progress: f64,
    /// The HID event's `options`: the phase in bits 24–31.
    pub options: u32,
    pub velocity: Option<VelocityField>,
}

/// The field values for one step (module doc, "How the adapter fills it in").
pub(crate) fn fields(
    axis: DragAxis,
    phase: Phase,
    progress: f64,
    velocity: Option<f64>,
) -> SwipeFields {
    let (motion, back, forward, velocity_field) = match axis {
        DragAxis::Horizontal => (MOTION_HORIZONTAL, MASK_LEFT, MASK_RIGHT, VELOCITY_X),
        DragAxis::Vertical => (MOTION_VERTICAL, MASK_UP, MASK_DOWN, VELOCITY_Y),
    };
    let mask = if progress < 0.0 {
        back
    } else if progress > 0.0 {
        forward
    } else {
        0
    };
    let phase_bits: u32 = match phase {
        Phase::Began => 1,
        Phase::Changed => 2,
        Phase::Ended => 4,
        Phase::Cancelled => 8,
    };
    SwipeFields {
        motion,
        mask,
        flavor: FLAVOR_DOCK_PRIMARY,
        progress,
        options: phase_bits << PHASE_SHIFT,
        velocity: velocity.map(|value| VelocityField {
            field: velocity_field,
            value,
        }),
    }
}

/// When an Ended or Cancelled is re-sent, in `CFAbsoluteTime` seconds.
pub(crate) struct ResendSchedule;

impl ResendSchedule {
    /// Re-sends for an end (or cancel) posted at `end`: +200 ms and +500 ms.
    pub fn after_end(end: f64) -> [f64; 2] {
        [end + 0.2, end + 0.5]
    }
}

/// What the adapter does for one `SwipeOutput` call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plan {
    /// Withdraw the re-sends of an earlier end.
    pub disarm: bool,
    /// Tried in order; the first that builds is posted (`pick`).
    pub tries: [Option<SwipeFields>; 3],
    /// A begin: posted only if this builds too. The built event is kept as
    /// the swipe's closer, the last resort of a closing plan.
    pub needs: Option<SwipeFields>,
    /// This plan closes the swipe: if no try builds, the closer is posted.
    pub closes: bool,
    /// Re-send what was posted at these `CFAbsoluteTime`s.
    pub resend_at: Option<[f64; 2]>,
}

impl Plan {
    pub const NOTHING: Self = Self {
        disarm: false,
        tries: [None; 3],
        needs: None,
        closes: false,
        resend_at: None,
    };

    fn posting(fields: SwipeFields) -> Self {
        Self {
            tries: [Some(fields), None, None],
            ..Self::NOTHING
        }
    }
}

/// What a plan posts: the first try that `build` can build, else, for a
/// closing plan, `closer` (the end built at begin). So a begun swipe gets a
/// closing event whenever anything at all could be built.
pub(crate) fn pick<B>(
    plan: &Plan,
    build: impl FnMut(&SwipeFields) -> Option<B>,
    closer: Option<B>,
) -> Option<B> {
    let built = plan.tries.iter().flatten().find_map(build);
    if plan.closes { built.or(closer) } else { built }
}

/// The swipe in progress, as the Dock has been told it. Pure.
#[derive(Debug, Default)]
pub(crate) struct Sequencer {
    active: Option<(DragAxis, f64)>,
}

impl Sequencer {
    pub fn begin(&mut self, axis: DragAxis) -> Plan {
        self.active = Some((axis, 0.0));
        Plan {
            disarm: true,
            needs: Some(fields(axis, Phase::Ended, 0.0, Some(0.0))),
            ..Plan::posting(fields(axis, Phase::Began, 0.0, None))
        }
    }

    pub fn change(&mut self, progress: f64) -> Plan {
        let Some((axis, last)) = self.active.as_mut() else {
            return Plan::NOTHING;
        };
        *last = progress;
        Plan::posting(fields(*axis, Phase::Changed, progress, None))
    }

    /// `now` is the `CFAbsoluteTime` the end is posted at.
    pub fn end(&mut self, velocity: f64, now: f64) -> Plan {
        let Some((axis, progress)) = self.active.take() else {
            return Plan::NOTHING;
        };
        Plan {
            tries: [
                Some(fields(axis, Phase::Ended, progress, Some(velocity))),
                Some(fields(axis, Phase::Ended, progress, None)),
                Some(fields(axis, Phase::Cancelled, progress, None)),
            ],
            closes: true,
            resend_at: Some(ResendSchedule::after_end(now)),
            ..Plan::NOTHING
        }
    }

    /// `now` is the `CFAbsoluteTime` the cancel is posted at.
    pub fn cancel(&mut self, now: f64) -> Plan {
        let Some((axis, progress)) = self.active.take() else {
            return Plan::NOTHING;
        };
        Plan {
            disarm: true,
            closes: true,
            resend_at: Some(ResendSchedule::after_end(now)),
            ..Plan::posting(fields(axis, Phase::Cancelled, progress, None))
        }
    }

    /// The begin couldn't be posted: the Dock knows of no swipe.
    pub fn abandon(&mut self) {
        self.active = None;
    }
}

// ---- Private API (humble) -----------------------------------------------

const RTLD_LAZY: c_int = 1;
const HID_FRAMEWORK: &CStr = c"/System/Library/PrivateFrameworks/HID.framework/Versions/A/HID";
const SKYLIGHT: &CStr = c"/System/Library/PrivateFrameworks/SkyLight.framework/Versions/A/SkyLight";

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn mach_absolute_time() -> u64;
}

type SetHidEvent = unsafe extern "C" fn(*mut c_void, *mut c_void);

/// Every selector `Api::build` sends to a `HIDEvent` ([B]). `Api::load`
/// refuses to start unless the class answers all of them, so a missing
/// method fails at startup instead of raising an Objective-C exception
/// mid-gesture.
const SELECTORS: [&CStr; 5] = [
    c"initWithType:timestamp:senderID:",
    c"setIntegerValue:forField:",
    c"setDoubleValue:forField:",
    c"setOptions:",
    c"appendEvent:",
];

/// The private symbols, loaded on first use and cached for the process:
/// `output_support` and `MacDockSwipe::new` both read this one answer.
pub(crate) fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        Api::load()
            .inspect_err(|why| tracing::warn!(%why, "dock-swipe symbols didn't load"))
            .ok()
    })
    .as_ref()
}

/// Whether the private symbols dock swipes need resolve on this macOS.
pub fn loads() -> bool {
    api().is_some()
}

/// One built, unposted dock-swipe step. The HID event is kept alive next
/// to the CGEvent until both drop, after posting, so the step stays valid
/// whether or not the setter retains it (unknown).
pub(crate) struct Built {
    event: CFRetained<CGEvent>,
    _hid: Retained<AnyObject>,
}

impl Built {
    fn post(&self) {
        CGEvent::post(CGEventTapLocation::SessionEventTap, Some(&self.event));
    }
}

/// The private class and function dock swipes need, resolved at runtime so
/// a missing symbol is an error, not a link failure.
pub(crate) struct Api {
    hid_event: &'static AnyClass,
    set_hid_event: SetHidEvent,
}

impl Api {
    pub fn load() -> Result<Self, String> {
        for path in [HID_FRAMEWORK, SKYLIGHT] {
            // SAFETY: dlopen with a NUL-terminated path; a null result only
            // means the framework isn't there, reported by the lookups below.
            unsafe { dlopen(path.as_ptr(), RTLD_LAZY) };
        }
        let hid_event = AnyClass::get(c"HIDEvent").ok_or("class HIDEvent not found")?;
        for name in SELECTORS {
            if !hid_event.responds_to(Sel::register(name)) {
                return Err(format!("HIDEvent doesn't respond to {name:?}"));
            }
        }
        Ok(Self {
            hid_event,
            // SAFETY: the alias is the assumed signature (module doc,
            // "Unknown"); `dock_swipe_events_read_back` checks it.
            set_hid_event: unsafe { resolve::<SetHidEvent>(c"SLEventSetIOHIDEvent") }
                .ok_or("SLEventSetIOHIDEvent not found")?,
        })
    }

    fn new_hid_event(&self, hid_type: u32) -> Option<Retained<AnyObject>> {
        // SAFETY: `HIDEvent` is an NSObject subclass; `alloc` takes no arguments.
        let alloc: Allocated<AnyObject> = unsafe { msg_send![self.hid_event, alloc] };
        // SAFETY: selector and argument types from Apple's HIDEvent.h
        // (module doc [B]): IOHIDEventType is uint32_t, the rest uint64_t.
        // `load` checked the class responds to it.
        unsafe {
            msg_send![alloc, initWithType: hid_type, timestamp: mach_absolute_time(), senderID: 0u64]
        }
    }

    /// One dock-swipe step as a type-30 CGEvent, built but not posted.
    pub fn build(&self, f: &SwipeFields) -> Option<Built> {
        self.build_with(f, |_| {})
    }

    /// As `build`, with `extra` writing more CGEvent fields before the HID
    /// event is attached (the probe's legacy-mix encoding).
    fn build_with(&self, f: &SwipeFields, extra: impl FnOnce(&CGEvent)) -> Option<Built> {
        // Every selector below was checked by `load`.
        let swipe = self.new_hid_event(HID_TYPE_DOCK_SWIPE)?;
        let set_int = |event: &AnyObject, value: isize, field: u32| {
            // SAFETY: `-setIntegerValue:(NSInteger)forField:(IOHIDEventField)`, [B].
            let _: () = unsafe { msg_send![event, setIntegerValue: value, forField: field] };
        };
        let set_double = |event: &AnyObject, value: f64, field: u32| {
            // SAFETY: `-setDoubleValue:(double)forField:(IOHIDEventField)`, [B].
            let _: () = unsafe { msg_send![event, setDoubleValue: value, forField: field] };
        };
        set_int(&swipe, f.motion, DOCK_SWIPE_MOTION);
        set_int(&swipe, f.flavor, DOCK_SWIPE_FLAVOR);
        set_int(&swipe, f.mask, DOCK_SWIPE_MASK);
        set_double(&swipe, f.progress, DOCK_SWIPE_PROGRESS);
        // SAFETY: `@property uint32_t options`, [B].
        let _: () = unsafe { msg_send![&*swipe, setOptions: f.options] };
        if let Some(v) = f.velocity {
            let child = self.new_hid_event(HID_TYPE_VELOCITY)?;
            set_double(&child, v.value, v.field);
            // SAFETY: `-appendEvent:(HIDEvent *)`, [B]; `child` is a live HIDEvent.
            let _: () = unsafe { msg_send![&*swipe, appendEvent: &*child] };
        }

        let event = CGEvent::new(None)?;
        CGEvent::set_type(Some(&event), CGEventType(CG_DOCK_CONTROL));
        CGEvent::set_integer_value_field(
            Some(&event),
            CGEventField::EventSourceUserData,
            MOUSETUNE_EVENT_TAG,
        );
        extra(&event);
        let cg_ptr = CFRetained::as_ptr(&event).as_ptr().cast::<c_void>();
        let hid_ptr = Retained::as_ptr(&swipe).cast_mut().cast::<c_void>();
        // SAFETY: both pointers are live objects held by the returned
        // `Built`, so they outlive the call and the post that follows.
        // The signature `void (CGEventRef, IOHIDEventRef)` is an
        // assumption (module doc, "Unknown"), checked by the read-back
        // test. Whether the setter retains or copies the HID event is
        // unknown; `Built` keeps our reference either way.
        unsafe { (self.set_hid_event)(cg_ptr, hid_ptr) };
        Some(Built { event, _hid: swipe })
    }
}

// ---- Adapter ------------------------------------------------------------

/// The posted close (Ended or Cancelled), once per re-send timer, reached
/// by the timers through their `info` pointer. Each slot posts at most once.
struct Resends {
    ends: RefCell<[Option<Rc<Built>>; 2]>,
    degraded: Arc<AtomicBool>,
}

/// `SwipeOutput` on macOS 27. Its methods run from `run_intents` on
/// `mt-input`, never inside a tap callout; the re-send timers fire on the
/// same run loop. Never panics: anything that fails to build is not posted.
pub struct MacDockSwipe {
    api: &'static Api,
    sequencer: Sequencer,
    /// The end built at begin: posted if nothing else closing will build.
    closer: Option<Rc<Built>>,
    /// From `Box::into_raw`: the timers' `info`. Never moves, and is freed
    /// in `Drop` after both timers are invalidated (as `input_loop` does
    /// with its `LoopState`). All other access is through `resends()`.
    resends: *mut Resends,
    /// `None` if CoreFoundation couldn't create one: that re-send is lost.
    timers: [Option<CFRetained<CFRunLoopTimer>>; 2],
}

impl MacDockSwipe {
    /// `None` exactly when `loads()` is false, so wherever
    /// `output_support().dock_swipe` is true this succeeds. A timer that
    /// can't be created only loses its re-send.
    pub fn new(run_loop: &CFRunLoop, degraded: Arc<AtomicBool>) -> Option<Self> {
        let api = api()?;
        let resends = Box::into_raw(Box::new(Resends {
            ends: RefCell::new([None, None]),
            degraded,
        }));
        let info = resends.cast::<c_void>();
        Some(Self {
            api,
            sequencer: Sequencer::default(),
            closer: None,
            resends,
            timers: [
                parked_timer(info, run_loop, first_resend_fired),
                parked_timer(info, run_loop, second_resend_fired),
            ],
        })
    }

    fn resends(&self) -> &Resends {
        // SAFETY: `resends` came from `Box::into_raw` in `new` and is freed
        // only in `Drop`; only shared references are ever made to it, and
        // its mutable state sits behind a `RefCell`.
        unsafe { &*self.resends }
    }

    fn disarm(&mut self) {
        for timer in self.timers.iter().flatten() {
            timer.set_next_fire_date(CFAbsoluteTimeGetCurrent() + NEVER_SECS);
        }
        if let Ok(mut ends) = self.resends().ends.try_borrow_mut() {
            *ends = [None, None];
        }
    }

    /// Carries out a plan; `pick` decides what is posted.
    fn run(&mut self, plan: Plan) {
        if plan.disarm {
            self.disarm();
        }
        if let Some(needs) = plan.needs {
            self.closer = self.api.build(&needs).map(Rc::new);
            if self.closer.is_none() {
                self.sequencer.abandon();
                return;
            }
        }
        let closer = if plan.closes {
            self.closer.take()
        } else {
            None
        };
        let api = self.api;
        let Some(built) = pick(&plan, |f| api.build(f).map(Rc::new), closer) else {
            if plan.needs.is_some() {
                self.sequencer.abandon();
                self.closer = None;
            }
            return;
        };
        built.post();
        let Some(dates) = plan.resend_at else {
            return;
        };
        let Ok(mut ends) = self.resends().ends.try_borrow_mut() else {
            return;
        };
        for ((slot, timer), date) in ends.iter_mut().zip(&self.timers).zip(dates) {
            if let Some(timer) = timer {
                *slot = Some(Rc::clone(&built));
                timer.set_next_fire_date(date);
            }
        }
    }
}

impl SwipeOutput for MacDockSwipe {
    fn begin(&mut self, axis: DragAxis) {
        let plan = self.sequencer.begin(axis);
        self.run(plan);
    }

    fn change(&mut self, progress: f64) {
        let plan = self.sequencer.change(progress);
        self.run(plan);
    }

    fn end(&mut self, velocity: f64) {
        let plan = self.sequencer.end(velocity, CFAbsoluteTimeGetCurrent());
        self.run(plan);
    }

    fn cancel(&mut self) {
        let plan = self.sequencer.cancel(CFAbsoluteTimeGetCurrent());
        self.run(plan);
    }
}

impl Drop for MacDockSwipe {
    /// Invalidating removes the timers from every run loop, so no callback
    /// can reach `resends` after this; only then is it freed.
    fn drop(&mut self) {
        for timer in self.timers.iter().flatten() {
            timer.invalidate();
        }
        // SAFETY: `resends` came from `Box::into_raw` in `new`, is freed
        // only here, and both timers that pointed at it are invalidated.
        drop(unsafe { Box::from_raw(self.resends) });
    }
}

unsafe extern "C-unwind" fn first_resend_fired(_timer: *mut CFRunLoopTimer, info: *mut c_void) {
    resend(info, 0)
}

unsafe extern "C-unwind" fn second_resend_fired(_timer: *mut CFRunLoopTimer, info: *mut c_void) {
    resend(info, 1)
}

/// Posts one prebuilt re-send, once. A re-entrant fire is dropped.
fn resend(info: *mut c_void, index: usize) {
    // SAFETY: `info` is the `Resends` from `Box::into_raw` the timer was
    // created with; `MacDockSwipe::drop` invalidates both timers before it
    // frees it. Only shared references are made to it.
    let resends = unsafe { &*info.cast_const().cast::<Resends>() };
    guarded(&resends.degraded, (), || {
        let Ok(mut ends) = resends.ends.try_borrow_mut() else {
            return;
        };
        if let Some(end) = ends.get_mut(index).and_then(Option::take) {
            end.post();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn sf(
        motion: isize,
        mask: isize,
        progress: f64,
        options: u32,
        velocity: Option<VelocityField>,
    ) -> SwipeFields {
        SwipeFields {
            motion,
            mask,
            flavor: 3,
            progress,
            options,
            velocity,
        }
    }

    #[test]
    fn a_horizontal_change_encodes_motion_mask_progress_and_phase() {
        assert_eq!(
            fields(DragAxis::Horizontal, Phase::Changed, 0.5, None),
            sf(1, 8, 0.5, 0x0200_0000, None)
        );
        assert_eq!(
            fields(DragAxis::Horizontal, Phase::Changed, -0.25, None),
            sf(1, 4, -0.25, 0x0200_0000, None)
        );
    }

    #[test]
    fn an_upward_end_carries_its_velocity_on_the_y_axis() {
        assert_eq!(
            fields(DragAxis::Vertical, Phase::Ended, -1.0, Some(-2.0)),
            sf(
                2,
                1,
                -1.0,
                0x0400_0000,
                Some(VelocityField {
                    field: 0x0009_0001,
                    value: -2.0
                })
            )
        );
    }

    #[test]
    fn a_begin_has_no_direction_yet_and_a_downward_cancel_masks_down() {
        assert_eq!(
            fields(DragAxis::Vertical, Phase::Began, 0.0, None),
            sf(2, 0, 0.0, 0x0100_0000, None)
        );
        assert_eq!(
            fields(DragAxis::Vertical, Phase::Cancelled, 0.3, None),
            sf(2, 2, 0.3, 0x0800_0000, None)
        );
    }

    #[test]
    fn an_end_is_re_sent_at_200_and_500_ms() {
        assert_eq!(ResendSchedule::after_end(10.0), [10.2, 10.5]);
    }

    #[test]
    fn a_begin_withdraws_old_re_sends_and_needs_its_end_to_build() {
        let mut s = Sequencer::default();
        assert_eq!(
            s.begin(DragAxis::Horizontal),
            Plan {
                disarm: true,
                tries: [Some(sf(1, 0, 0.0, 0x0100_0000, None)), None, None],
                needs: Some(sf(
                    1,
                    0,
                    0.0,
                    0x0400_0000,
                    Some(VelocityField {
                        field: 0x0009_0000,
                        value: 0.0
                    })
                )),
                closes: false,
                resend_at: None,
            }
        );
    }

    #[test]
    fn an_end_posts_the_last_progress_with_velocity_and_two_re_sends() {
        let mut s = Sequencer::default();
        s.begin(DragAxis::Horizontal);
        assert_eq!(
            s.change(0.5),
            Plan {
                tries: [Some(sf(1, 8, 0.5, 0x0200_0000, None)), None, None],
                ..Plan::NOTHING
            }
        );
        // Falls back to an end without velocity, then a cancel, then the
        // closer built at begin (`pick`).
        assert_eq!(
            s.end(2.0, 10.0),
            Plan {
                tries: [
                    Some(sf(
                        1,
                        8,
                        0.5,
                        0x0400_0000,
                        Some(VelocityField {
                            field: 0x0009_0000,
                            value: 2.0
                        })
                    )),
                    Some(sf(1, 8, 0.5, 0x0400_0000, None)),
                    Some(sf(1, 8, 0.5, 0x0800_0000, None)),
                ],
                closes: true,
                resend_at: Some([10.2, 10.5]),
                ..Plan::NOTHING
            }
        );
        assert_eq!(s.change(0.7), Plan::NOTHING);
        assert_eq!(s.end(1.0, 11.0), Plan::NOTHING);
        assert_eq!(s.cancel(11.0), Plan::NOTHING);
    }

    #[test]
    fn a_cancel_posts_cancelled_at_the_last_progress_and_is_re_sent_like_an_end() {
        let mut s = Sequencer::default();
        s.begin(DragAxis::Vertical);
        s.change(-0.3);
        assert_eq!(
            s.cancel(10.0),
            Plan {
                disarm: true,
                tries: [Some(sf(2, 1, -0.3, 0x0800_0000, None)), None, None],
                closes: true,
                resend_at: Some([10.2, 10.5]),
                ..Plan::NOTHING
            }
        );
        assert_eq!(s.cancel(11.0), Plan::NOTHING);
    }

    /// A builder that fails for every field set in `broken`.
    fn builder(broken: &[SwipeFields]) -> impl FnMut(&SwipeFields) -> Option<SwipeFields> + '_ {
        move |f| (!broken.contains(f)).then_some(*f)
    }

    const VELOCITY_2: Option<VelocityField> = Some(VelocityField {
        field: 0x0009_0000,
        value: 2.0,
    });
    const CLOSER: SwipeFields = sf(
        1,
        0,
        0.0,
        0x0400_0000,
        Some(VelocityField {
            field: 0x0009_0000,
            value: 0.0,
        }),
    );

    #[test]
    fn an_end_that_wont_build_falls_back_to_no_velocity_then_cancel_then_the_closer() {
        let mut s = Sequencer::default();
        s.begin(DragAxis::Horizontal);
        s.change(0.5);
        let end = s.end(2.0, 10.0);
        let with_velocity = sf(1, 8, 0.5, 0x0400_0000, VELOCITY_2);
        let without = sf(1, 8, 0.5, 0x0400_0000, None);
        let cancelled = sf(1, 8, 0.5, 0x0800_0000, None);

        assert_eq!(pick(&end, builder(&[]), Some(CLOSER)), Some(with_velocity));
        assert_eq!(
            pick(&end, builder(&[with_velocity]), Some(CLOSER)),
            Some(without)
        );
        assert_eq!(
            pick(&end, builder(&[with_velocity, without]), Some(CLOSER)),
            Some(cancelled)
        );
        assert_eq!(
            pick(
                &end,
                builder(&[with_velocity, without, cancelled]),
                Some(CLOSER)
            ),
            Some(CLOSER)
        );
    }

    #[test]
    fn only_a_closing_plan_falls_back_to_the_closer() {
        let mut s = Sequencer::default();
        s.begin(DragAxis::Horizontal);
        let change = s.change(0.5);
        let changed = sf(1, 8, 0.5, 0x0200_0000, None);
        assert_eq!(pick(&change, builder(&[changed]), Some(CLOSER)), None);
        let cancel = s.cancel(10.0);
        let cancelled = sf(1, 8, 0.5, 0x0800_0000, None);
        assert_eq!(
            pick(&cancel, builder(&[cancelled]), Some(CLOSER)),
            Some(CLOSER)
        );
        assert_eq!(pick(&Plan::NOTHING, builder(&[]), Some(CLOSER)), None);
    }

    #[test]
    fn nothing_is_posted_without_a_begin_or_after_an_abandoned_one() {
        let mut s = Sequencer::default();
        assert_eq!(s.change(0.5), Plan::NOTHING);
        assert_eq!(s.end(2.0, 10.0), Plan::NOTHING);
        assert_eq!(s.cancel(10.0), Plan::NOTHING);
        s.begin(DragAxis::Horizontal);
        s.abandon();
        assert_eq!(s.change(0.5), Plan::NOTHING);
        assert_eq!(s.end(2.0, 10.0), Plan::NOTHING);
    }
}

/// Reads built events back through the private getters. Posts nothing.
#[cfg(test)]
mod read_back {
    use objc2::msg_send;

    use super::*;

    type CopyHidEvent = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
    type GetHidType = unsafe extern "C" fn(*mut c_void) -> u32;

    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    #[derive(Debug, PartialEq)]
    struct Velocity {
        child_type: u32,
        x: f64,
        y: f64,
    }

    #[derive(Debug, PartialEq)]
    struct ReadBack {
        hid_type: u32,
        motion: isize,
        mask: isize,
        flavor: isize,
        progress: f64,
        phase: u32,
        velocity: Option<Velocity>,
        cg_type: u32,
        tag: i64,
    }

    struct Getters {
        copy_hid_event: CopyHidEvent,
        get_hid_type: GetHidType,
    }

    impl Getters {
        fn load() -> Self {
            Self {
                // SAFETY: signature from WebKit's CoreGraphicsSPI.h (module doc [F]).
                copy_hid_event: unsafe { resolve::<CopyHidEvent>(c"CGEventCopyIOHIDEvent") }
                    .expect("CGEventCopyIOHIDEvent resolves"),
                // SAFETY: signature from WebKit's IOKitSPIIOS.h (module doc [C]).
                get_hid_type: unsafe { resolve::<GetHidType>(c"IOHIDEventGetType") }
                    .expect("IOHIDEventGetType resolves"),
            }
        }

        fn read(&self, built: &Built) -> Option<ReadBack> {
            let event: &CGEvent = &built.event;
            let cg_ptr = (event as *const CGEvent).cast_mut().cast::<c_void>();
            // SAFETY: `event` is a live CGEvent; the getter follows the CF
            // Copy rule, so we release the result below.
            let hid = unsafe { (self.copy_hid_event)(cg_ptr) };
            if hid.is_null() {
                return None;
            }
            // SAFETY: `hid` is a non-null IOHIDEventRef, which is a HIDEvent
            // object (module doc [C], [D]).
            let object: &AnyObject = unsafe { &*hid.cast::<AnyObject>() };
            let int = |field: u32| -> isize {
                // SAFETY: `-integerValueForField:(IOHIDEventField)`, [B].
                unsafe { msg_send![object, integerValueForField: field] }
            };
            // SAFETY: `-doubleValueForField:(IOHIDEventField)`, [B].
            let progress: f64 =
                unsafe { msg_send![object, doubleValueForField: DOCK_SWIPE_PROGRESS] };
            // SAFETY: `@property uint32_t options`, [B].
            let options: u32 = unsafe { msg_send![object, options] };
            // SAFETY: `@property (readonly, nullable) NSArray<HIDEvent *> *children`, [B].
            let children: Option<&AnyObject> = unsafe { msg_send![object, children] };
            let velocity = children.and_then(|array| {
                // SAFETY: `array` is an NSArray; `-count` returns NSUInteger.
                let count: usize = unsafe { msg_send![array, count] };
                if count != 1 {
                    return None;
                }
                // SAFETY: index 0 exists (count is 1); NSArray returns a HIDEvent.
                let child: &AnyObject = unsafe { msg_send![array, objectAtIndex: 0usize] };
                // SAFETY: `child` is a live IOHIDEventRef; signature from [C].
                let child_type = unsafe {
                    (self.get_hid_type)((child as *const AnyObject).cast_mut().cast::<c_void>())
                };
                // SAFETY: `-doubleValueForField:(IOHIDEventField)`, [B].
                let x: f64 = unsafe { msg_send![child, doubleValueForField: VELOCITY_X] };
                // SAFETY: as above.
                let y: f64 = unsafe { msg_send![child, doubleValueForField: VELOCITY_Y] };
                Some(Velocity { child_type, x, y })
            });
            let read = ReadBack {
                // SAFETY: `hid` is a live IOHIDEventRef; signature from [C].
                hid_type: unsafe { (self.get_hid_type)(hid) },
                motion: int(DOCK_SWIPE_MOTION),
                mask: int(DOCK_SWIPE_MASK),
                flavor: int(DOCK_SWIPE_FLAVOR),
                progress,
                phase: options >> PHASE_SHIFT,
                velocity,
                cg_type: CGEvent::r#type(Some(event)).0,
                tag: CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData),
            };
            // SAFETY: balances the +1 from the Copy getter above.
            unsafe { CFRelease(hid) };
            Some(read)
        }
    }

    /// Builds (never posts) the adapter's begin, change(0.5) and end(2.0)
    /// and reads them back, which checks the assumed `SLEventSetIOHIDEvent`
    /// signature, the field numbers and the end's velocity child. Expected
    /// values are the module doc's literals. Run by name:
    /// `cargo test dock_swipe_events_read_back -- --ignored --nocapture`.
    #[test]
    #[ignore = "private SkyLight/HID APIs; verified on macOS 27 only"]
    fn dock_swipe_events_read_back() {
        let api = Api::load().expect("the private dock-swipe symbols resolve on this macOS");
        let getters = Getters::load();
        let mut s = Sequencer::default();
        let read = |plan: Plan| {
            let post = plan.tries[0].expect("the plan posts");
            getters.read(&api.build(&post).expect("the step builds"))
        };
        let step = |mask, progress, phase, velocity| {
            Some(ReadBack {
                hid_type: 23,
                motion: 1,
                mask,
                flavor: 3,
                progress,
                phase,
                velocity,
                cg_type: 30,
                tag: MOUSETUNE_EVENT_TAG,
            })
        };
        assert_eq!(read(s.begin(DragAxis::Horizontal)), step(0, 0.0, 1, None));
        assert_eq!(read(s.change(0.5)), step(8, 0.5, 2, None));
        assert_eq!(
            read(s.end(2.0, 0.0)),
            step(
                8,
                0.5,
                4,
                Some(Velocity {
                    child_type: 9,
                    x: 2.0,
                    y: 0.0
                })
            )
        );
        s.begin(DragAxis::Vertical);
        s.change(-0.25);
        let cancelled = read(s.cancel(0.0)).expect("reads back");
        assert_eq!(
            (
                cancelled.motion,
                cancelled.mask,
                cancelled.progress,
                cancelled.phase
            ),
            (2, 1, -0.25, 8)
        );
    }
}

/// The maintainer's hardware probe. Its default mode
/// drives the adapter's own `Sequencer`, `pick` and `Api::build`, so a
/// "worked" result verifies what `MacDockSwipe` sends. Two diagnostic
/// modes keep the earlier encoding: a fixed mask per direction, optionally
/// with the legacy gesture fields.
#[cfg(test)]
mod probe {
    use std::{thread::sleep, time::Duration};

    use super::*;

    const STEPS: u32 = 30;
    const SWIPE_TIME: Duration = Duration::from_millis(500);
    /// 1.0 of progress over 0.5 s, in progress per second (units unknown).
    const END_SPEED: f64 = 2.0;
    /// The adapter's re-sends: +200 ms, then +500 ms (`ResendSchedule`).
    const RESEND_WAITS: [Duration; 2] = [Duration::from_millis(200), Duration::from_millis(300)];

    const CG_GESTURE_HID_TYPE: u32 = 110;
    const CG_SWIPE_MOTION: u32 = 123;
    const CG_SWIPE_PROGRESS: u32 = 124;
    const CG_SWIPE_VELOCITY_X: u32 = 129;
    const CG_SWIPE_VELOCITY_Y: u32 = 130;
    const CG_GESTURE_PHASE: u32 = 132;
    const CG_SWIPE_MASK: u32 = 134;
    const CG_GESTURE_FLAVOR: u32 = 138;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Mode {
        /// What `MacDockSwipe` sends. The default.
        Sequencer,
        /// The earlier encoding: the direction's mask on every step.
        FixedMask,
        /// `FixedMask` plus the legacy gesture fields.
        LegacyMix,
    }

    impl Mode {
        fn from_env() -> Self {
            match std::env::var("MOUSETUNE_PROBE_ENCODING").as_deref() {
                Ok("fixed-mask") => Self::FixedMask,
                Ok("legacy-mix") => Self::LegacyMix,
                _ => Self::Sequencer,
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Direction {
        Right,
        Up,
        Down,
    }

    impl Direction {
        fn axis(self) -> DragAxis {
            match self {
                Self::Right => DragAxis::Horizontal,
                Self::Up | Self::Down => DragAxis::Vertical,
            }
        }

        fn mask(self) -> isize {
            match self {
                Self::Up => MASK_UP,
                Self::Down => MASK_DOWN,
                Self::Right => MASK_RIGHT,
            }
        }

        /// Screen coordinates: x grows right, y grows down.
        fn sign(self) -> f64 {
            match self {
                Self::Right | Self::Down => 1.0,
                Self::Up => -1.0,
            }
        }
    }

    /// A built step, the pause before posting it, and what to print.
    struct Planned {
        wait: Duration,
        step: Rc<Built>,
        line: String,
    }

    fn describe(direction: Direction, f: &SwipeFields, note: &str) -> String {
        let phase = match f.options >> PHASE_SHIFT {
            1 => "Began",
            2 => "Changed",
            4 => "Ended",
            8 => "Cancelled",
            _ => "?",
        };
        let velocity = f
            .velocity
            .map_or(String::new(), |v| format!(", velocity {:+.1}", v.value));
        format!(
            "{direction:?} {phase} progress {:+.3}, mask {}{velocity}{note}",
            f.progress, f.mask
        )
    }

    /// The default mode: exactly the adapter's plans and fallbacks, built
    /// before anything is posted. Re-sends re-post the same event, as the
    /// adapter's timers do.
    fn plan_as_adapter(api: &Api, direction: Direction) -> Result<Vec<Planned>, String> {
        let build = |f: &SwipeFields| api.build(f).map(|b| (*f, Rc::new(b)));
        let mut seq = Sequencer::default();
        let mut planned = Vec::new();
        let mut push = |wait, (f, step): (SwipeFields, Rc<Built>), note: &str| {
            planned.push(Planned {
                wait,
                step,
                line: describe(direction, &f, note),
            })
        };
        let begin = seq.begin(direction.axis());
        let closer = begin
            .needs
            .and_then(|f| build(&f))
            .ok_or("the end a begin needs didn't build")?;
        push(
            Duration::ZERO,
            pick(&begin, build, None).ok_or("Began didn't build")?,
            "",
        );
        for i in 1..=STEPS {
            let change = seq.change(direction.sign() * f64::from(i) / f64::from(STEPS));
            let built = pick(&change, build, None).ok_or("a Changed didn't build")?;
            push(SWIPE_TIME / STEPS, built, "");
        }
        let end = seq.end(direction.sign() * END_SPEED, 0.0);
        let (f, step) = pick(&end, build, Some(closer)).ok_or("no closing event built")?;
        push(Duration::ZERO, (f, Rc::clone(&step)), " (end)");
        push(
            RESEND_WAITS[0],
            (f, Rc::clone(&step)),
            " (re-sent, +200 ms)",
        );
        push(RESEND_WAITS[1], (f, step), " (re-sent, +500 ms)");
        Ok(planned)
    }

    /// A fixed-mask step: the adapter's fields with the direction's mask on
    /// every phase.
    fn fixed_mask_fields(
        direction: Direction,
        phase: Phase,
        progress: f64,
        velocity: Option<f64>,
    ) -> SwipeFields {
        SwipeFields {
            mask: direction.mask(),
            ..fields(direction.axis(), phase, progress, velocity)
        }
    }

    fn build_fixed(api: &Api, f: &SwipeFields, legacy: bool) -> Option<Built> {
        if !legacy {
            return api.build(f);
        }
        api.build_with(f, |event| {
            let set =
                |n: u32, v: i64| CGEvent::set_integer_value_field(Some(event), CGEventField(n), v);
            let set_fine =
                |n: u32, v: f64| CGEvent::set_double_value_field(Some(event), CGEventField(n), v);
            set(CG_GESTURE_HID_TYPE, i64::from(HID_TYPE_DOCK_SWIPE));
            set(CG_SWIPE_MOTION, f.motion as i64);
            set(CG_GESTURE_FLAVOR, f.flavor as i64);
            set(CG_SWIPE_MASK, f.mask as i64);
            set(CG_GESTURE_PHASE, i64::from(f.options >> PHASE_SHIFT));
            set_fine(CG_SWIPE_PROGRESS, f.progress);
            if let Some(v) = f.velocity {
                let axis = if v.field == VELOCITY_X {
                    CG_SWIPE_VELOCITY_X
                } else {
                    CG_SWIPE_VELOCITY_Y
                };
                set_fine(axis, v.value);
            }
        })
    }

    /// The diagnostic modes' plan, built before anything is posted.
    fn plan_fixed_mask(
        api: &Api,
        direction: Direction,
        legacy: bool,
    ) -> Result<Vec<Planned>, String> {
        let sign = direction.sign();
        let mut planned = Vec::new();
        let mut add = |wait, phase, progress, velocity: Option<f64>, note: &str| {
            let f = fixed_mask_fields(direction, phase, progress, velocity);
            let step = build_fixed(api, &f, legacy)
                .ok_or_else(|| format!("{phase:?} {progress:+.3} didn't build"))?;
            planned.push(Planned {
                wait,
                step: Rc::new(step),
                line: describe(direction, &f, note),
            });
            Ok::<(), String>(())
        };
        add(Duration::ZERO, Phase::Began, 0.0, None, "")?;
        for i in 1..=STEPS {
            let progress = sign * f64::from(i) / f64::from(STEPS);
            add(SWIPE_TIME / STEPS, Phase::Changed, progress, None, "")?;
        }
        // An end without velocity beats a swipe left open.
        let mut velocity = Some(sign * END_SPEED);
        let end = fixed_mask_fields(direction, Phase::Ended, sign, velocity);
        if build_fixed(api, &end, legacy).is_none() {
            println!("{direction:?}: the end with velocity didn't build; ending without velocity.");
            velocity = None;
        }
        add(Duration::ZERO, Phase::Ended, sign, velocity, " (end)")?;
        add(
            RESEND_WAITS[0],
            Phase::Ended,
            sign,
            velocity,
            " (end re-sent, +200 ms)",
        )?;
        add(
            RESEND_WAITS[1],
            Phase::Ended,
            sign,
            velocity,
            " (end re-sent, +500 ms)",
        )?;
        Ok(planned)
    }

    fn swipe(api: &Api, direction: Direction, mode: Mode) {
        let planned = match mode {
            Mode::Sequencer => plan_as_adapter(api, direction),
            Mode::FixedMask => plan_fixed_mask(api, direction, false),
            Mode::LegacyMix => plan_fixed_mask(api, direction, true),
        };
        match planned {
            Ok(planned) => {
                for p in planned {
                    sleep(p.wait);
                    p.step.post();
                    println!("{}", p.line);
                }
            }
            Err(why) => println!("{direction:?}: posted nothing, because {why}."),
        }
    }

    /// Run by the maintainer only, after `dock_swipe_events_read_back`
    /// passes: `cargo test dock_swipe_moves_spaces -- --ignored --nocapture`.
    /// Posts real events. Watch the screen.
    #[test]
    #[ignore = "posts real dock-swipe events; the maintainer runs and watches it"]
    fn dock_swipe_moves_spaces() {
        let api = Api::load().expect("the private dock-swipe symbols resolve on this macOS");
        if !crate::platform::permissions::is_accessibility_trusted() {
            println!(
                "WARNING: this terminal isn't trusted for Accessibility; macOS will drop the events."
            );
        }
        let mode = Mode::from_env();
        println!(
            "Encoding: {}.",
            match mode {
                Mode::Sequencer => "sequencer (what MouseTune ships; the run that verifies it)",
                Mode::FixedMask => "fixed-mask (diagnostic: a fixed mask per direction)",
                Mode::LegacyMix => "legacy-mix (diagnostic: fixed mask + legacy gesture fields)",
            }
        );
        println!(
            "In 3 s: a slow swipe to the RIGHT (expect the next Space), then UP (expect Mission Control), then DOWN (expect it to close)."
        );
        sleep(Duration::from_secs(3));
        swipe(&api, Direction::Right, mode);
        sleep(Duration::from_secs(2));
        swipe(&api, Direction::Up, mode);
        sleep(Duration::from_secs(2));
        swipe(&api, Direction::Down, mode);
    }
}
