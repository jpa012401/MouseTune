//! Dock-swipe spike (Phase 6 design §3, plan Task 8). **Probe only**: nothing
//! here is wired into the app. Task 9 builds the adapter, or ships the
//! Discrete fallback, from what the maintainer sees when the probe runs.
//!
//! # macOS 27 encoding (what the probe sends)
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
//! | Getter for a CGEvent's HID event | `IOHIDEventRef CGEventCopyIOHIDEvent(CGEventRef)` | [F] `CoreGraphicsSPI.h`; exported per the SDK's `CoreGraphics.tbd` |
//! | Setter for a CGEvent's HID event | `SLEventSetIOHIDEvent` is exported by SkyLight; **there is no `CGEventSetHIDEvent`/`CGEventSetIOHIDEvent` export** on the macOS 27 SDK | SDK `SkyLight.tbd`, `CoreGraphics.tbd` |
//! | Posting | `kCGSessionEventTap`, stamped with `MOUSETUNE_EVENT_TAG` like every other MouseTune event | AGENTS.md footgun 3 |
//!
//! By default the probe sends only that. With
//! `MOUSETUNE_PROBE_ENCODING=legacy-mix` it also writes the same values into
//! the legacy gesture fields of the type-30 `CGEvent` (table below), and
//! attaches the HID event last so it is the authoritative copy.
//!
//! ## Unknown (no allowed source; the probe makes a documented guess)
//!
//! - **`SLEventSetIOHIDEvent`'s signature.** Assumed
//!   `void (CGEventRef, IOHIDEventRef)`, mirroring the sourced getter. The
//!   non-posting, ignored test `attached_dock_swipe_reads_back` checks the
//!   round trip (set, then `CGEventCopyIOHIDEvent`); it passes on macOS 27.0.1. That
//!   proves the event carries the HID event, not that the Dock acts on it.
//! - **`IOHIDEventCreateDockSwipeEvent`'s signature.** It is exported by
//!   IOKit (SDK `IOKit.tbd`), but no allowed source declares it, so the probe
//!   creates the event generically with `initWithType:` instead.
//! - **Progress sign and scale.** The probe uses screen-coordinate signs
//!   (right +, up −, down +) and 1.0 = one full Space / a fully open Mission
//!   Control. Which sign the Dock wants is what the run shows.
//! - **Velocity units.** The probe sends progress per second (±2.0 for 1.0
//!   over 0.5 s) in a velocity child event on the end event only.
//! - **Whether the Dock needs the swipe mask, a `senderID`, or a type-29
//!   companion event on macOS 27.** The probe sends the mask, sender 0, and
//!   no companion.
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
//! carry over from the HID encoding but are unverified on hardware.
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

#[cfg(test)]
mod ffi {
    use std::ffi::{CStr, c_char, c_int, c_void};

    use objc2::{
        msg_send,
        rc::{Allocated, Retained},
        runtime::{AnyClass, AnyObject, Sel},
    };
    use objc2_core_foundation::CFRetained;
    use objc2_core_graphics::{CGEvent, CGEventField, CGEventTapLocation, CGEventType};

    use crate::{input::filter::MOUSETUNE_EVENT_TAG, platform::symbolic_hotkeys::resolve};

    const fn field(hid_type: u32, index: u32) -> u32 {
        (hid_type << 16) | index
    }

    pub const HID_TYPE_VELOCITY: u32 = 9;
    pub const HID_TYPE_DOCK_SWIPE: u32 = 23;
    pub const DOCK_SWIPE_MASK: u32 = field(HID_TYPE_DOCK_SWIPE, 0);
    pub const DOCK_SWIPE_MOTION: u32 = field(HID_TYPE_DOCK_SWIPE, 1);
    pub const DOCK_SWIPE_PROGRESS: u32 = field(HID_TYPE_DOCK_SWIPE, 2);
    pub const DOCK_SWIPE_FLAVOR: u32 = field(HID_TYPE_DOCK_SWIPE, 5);
    pub const VELOCITY_X: u32 = field(HID_TYPE_VELOCITY, 0);
    pub const VELOCITY_Y: u32 = field(HID_TYPE_VELOCITY, 1);
    pub const FLAVOR_DOCK_PRIMARY: isize = 3;
    pub const PHASE_SHIFT: u32 = 24;

    pub const CG_DOCK_CONTROL: u32 = 30;
    const CG_GESTURE_HID_TYPE: u32 = 110;
    const CG_SWIPE_MOTION: u32 = 123;
    const CG_SWIPE_PROGRESS: u32 = 124;
    const CG_SWIPE_VELOCITY_X: u32 = 129;
    const CG_SWIPE_VELOCITY_Y: u32 = 130;
    const CG_GESTURE_PHASE: u32 = 132;
    const CG_SWIPE_MASK: u32 = 134;
    const CG_GESTURE_FLAVOR: u32 = 138;

    const RTLD_LAZY: c_int = 1;
    const HID_FRAMEWORK: &CStr = c"/System/Library/PrivateFrameworks/HID.framework/Versions/A/HID";
    const SKYLIGHT: &CStr =
        c"/System/Library/PrivateFrameworks/SkyLight.framework/Versions/A/SkyLight";

    unsafe extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn mach_absolute_time() -> u64;
        fn CFRelease(cf: *const c_void);
    }

    type SetHidEvent = unsafe extern "C" fn(*mut c_void, *mut c_void);
    type CopyHidEvent = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
    type GetHidType = unsafe extern "C" fn(*mut c_void) -> u32;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Direction {
        Right,
        Up,
        Down,
    }

    impl Direction {
        fn motion(self) -> isize {
            match self {
                Self::Right => 1,
                Self::Up | Self::Down => 2,
            }
        }

        fn mask(self) -> isize {
            match self {
                Self::Up => 1,
                Self::Down => 2,
                Self::Right => 8,
            }
        }

        /// Screen coordinates: x grows right, y grows down. A guess (module doc).
        pub fn sign(self) -> f64 {
            match self {
                Self::Right | Self::Down => 1.0,
                Self::Up => -1.0,
            }
        }

        fn horizontal(self) -> bool {
            self == Self::Right
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Phase {
        Began,
        Changed,
        Ended,
    }

    impl Phase {
        pub fn bits(self) -> u32 {
            match self {
                Self::Began => 1,
                Self::Changed => 2,
                Self::Ended => 4,
            }
        }
    }

    /// Every selector the probe and the read-back test send to a `HIDEvent`
    /// ([B]). `Api::load` refuses to start unless the class answers all of
    /// them, so a missing method fails before anything is posted instead of
    /// raising an Objective-C exception mid-gesture.
    const SELECTORS: [&CStr; 9] = [
        c"initWithType:timestamp:senderID:",
        c"setIntegerValue:forField:",
        c"setDoubleValue:forField:",
        c"setOptions:",
        c"appendEvent:",
        c"integerValueForField:",
        c"doubleValueForField:",
        c"options",
        c"children",
    ];

    /// One built, unposted dock-swipe step. The HID event is kept alive
    /// next to the CGEvent until both drop, after posting, so the step
    /// stays valid whether or not the setter retains it (unknown).
    pub struct Step {
        pub event: CFRetained<CGEvent>,
        _hid: Retained<AnyObject>,
    }

    /// The private functions and class the probe needs, resolved at runtime
    /// so a missing symbol is a readable error rather than a link failure.
    pub struct Api {
        hid_event: &'static AnyClass,
        set_hid_event: SetHidEvent,
        copy_hid_event: CopyHidEvent,
        get_hid_type: GetHidType,
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
                // "Unknown"); `attached_dock_swipe_reads_back` checks it.
                set_hid_event: unsafe { resolve::<SetHidEvent>(c"SLEventSetIOHIDEvent") }
                    .ok_or("SLEventSetIOHIDEvent not found")?,
                // SAFETY: signature from WebKit's CoreGraphicsSPI.h (module doc [F]).
                copy_hid_event: unsafe { resolve::<CopyHidEvent>(c"CGEventCopyIOHIDEvent") }
                    .ok_or("CGEventCopyIOHIDEvent not found")?,
                // SAFETY: signature from WebKit's IOKitSPIIOS.h (module doc [C]).
                get_hid_type: unsafe { resolve::<GetHidType>(c"IOHIDEventGetType") }
                    .ok_or("IOHIDEventGetType not found")?,
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
        pub fn build(
            &self,
            direction: Direction,
            phase: Phase,
            progress: f64,
            velocity: Option<f64>,
            legacy_fields: bool,
        ) -> Option<Step> {
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
            set_int(&swipe, direction.motion(), DOCK_SWIPE_MOTION);
            set_int(&swipe, FLAVOR_DOCK_PRIMARY, DOCK_SWIPE_FLAVOR);
            set_int(&swipe, direction.mask(), DOCK_SWIPE_MASK);
            set_double(&swipe, progress, DOCK_SWIPE_PROGRESS);
            let options = phase.bits() << PHASE_SHIFT;
            // SAFETY: `@property uint32_t options`, [B].
            let _: () = unsafe { msg_send![&*swipe, setOptions: options] };
            if let Some(v) = velocity {
                let child = self.new_hid_event(HID_TYPE_VELOCITY)?;
                let axis = if direction.horizontal() {
                    VELOCITY_X
                } else {
                    VELOCITY_Y
                };
                set_double(&child, v, axis);
                // SAFETY: `-appendEvent:(HIDEvent *)`, [B]; `child` is a live HIDEvent.
                let _: () = unsafe { msg_send![&*swipe, appendEvent: &*child] };
            }

            let event = CGEvent::new(None)?;
            CGEvent::set_type(Some(&event), CGEventType(CG_DOCK_CONTROL));
            let set =
                |f: u32, v: i64| CGEvent::set_integer_value_field(Some(&event), CGEventField(f), v);
            set(CGEventField::EventSourceUserData.0, MOUSETUNE_EVENT_TAG);
            if legacy_fields {
                let set_fine = |f: u32, v: f64| {
                    CGEvent::set_double_value_field(Some(&event), CGEventField(f), v)
                };
                set(CG_GESTURE_HID_TYPE, i64::from(HID_TYPE_DOCK_SWIPE));
                set(CG_SWIPE_MOTION, direction.motion() as i64);
                set(CG_GESTURE_FLAVOR, FLAVOR_DOCK_PRIMARY as i64);
                set(CG_SWIPE_MASK, direction.mask() as i64);
                set(CG_GESTURE_PHASE, i64::from(phase.bits()));
                set_fine(CG_SWIPE_PROGRESS, progress);
                if let Some(v) = velocity {
                    let axis = if direction.horizontal() {
                        CG_SWIPE_VELOCITY_X
                    } else {
                        CG_SWIPE_VELOCITY_Y
                    };
                    set_fine(axis, v);
                }
            }
            let cg_ptr = CFRetained::as_ptr(&event).as_ptr().cast::<c_void>();
            let hid_ptr = Retained::as_ptr(&swipe).cast_mut().cast::<c_void>();
            // SAFETY: both pointers are live objects held by the returned
            // `Step`, so they outlive the call and the post that follows.
            // The signature `void (CGEventRef, IOHIDEventRef)` is an
            // assumption (module doc, "Unknown"), checked by the read-back
            // test. Whether the setter retains or copies the HID event is
            // unknown; `Step` keeps our reference either way.
            unsafe { (self.set_hid_event)(cg_ptr, hid_ptr) };
            Some(Step { event, _hid: swipe })
        }

        pub fn post(step: &Step) {
            CGEvent::post(CGEventTapLocation::SessionEventTap, Some(&step.event));
        }

        /// What the CGEvent carries, read back through the HID event.
        pub fn read_back(&self, step: &Step) -> Option<ReadBack> {
            let event: &CGEvent = &step.event;
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

    #[derive(Debug, PartialEq)]
    pub struct Velocity {
        pub child_type: u32,
        pub x: f64,
        pub y: f64,
    }

    #[derive(Debug, PartialEq)]
    pub struct ReadBack {
        pub hid_type: u32,
        pub motion: isize,
        pub flavor: isize,
        pub progress: f64,
        pub phase: u32,
        pub velocity: Option<Velocity>,
        pub cg_type: u32,
        pub tag: i64,
    }
}

#[cfg(test)]
mod tests {
    use super::ffi::{Api, Direction, Phase, ReadBack, Velocity};
    use crate::input::filter::MOUSETUNE_EVENT_TAG;

    /// Builds (never posts) dock-swipe steps and reads them back, which
    /// checks the assumed `SLEventSetIOHIDEvent` signature, the field
    /// numbers and the end path with its velocity child. Expected values are
    /// the module doc's literals. Run it before the probe:
    /// `cargo test attached_dock_swipe_reads_back -- --ignored --nocapture`.
    #[test]
    #[ignore = "private SkyLight/HID APIs; verified on macOS 27 only"]
    fn attached_dock_swipe_reads_back() {
        let api = Api::load().expect("the private dock-swipe symbols resolve on this macOS");
        for legacy_fields in [false, true] {
            let change = api
                .build(Direction::Right, Phase::Changed, 0.5, None, legacy_fields)
                .expect("a change step builds");
            assert_eq!(
                api.read_back(&change),
                Some(ReadBack {
                    hid_type: 23,
                    motion: 1,
                    flavor: 3,
                    progress: 0.5,
                    phase: 2,
                    velocity: None,
                    cg_type: 30,
                    tag: MOUSETUNE_EVENT_TAG,
                }),
                "change, legacy_fields = {legacy_fields}"
            );
            let end = api
                .build(Direction::Up, Phase::Ended, -1.0, Some(-2.0), legacy_fields)
                .expect("an end step with velocity builds");
            assert_eq!(
                api.read_back(&end),
                Some(ReadBack {
                    hid_type: 23,
                    motion: 2,
                    flavor: 3,
                    progress: -1.0,
                    phase: 4,
                    velocity: Some(Velocity {
                        child_type: 9,
                        x: 0.0,
                        y: -2.0,
                    }),
                    cg_type: 30,
                    tag: MOUSETUNE_EVENT_TAG,
                }),
                "end, legacy_fields = {legacy_fields}"
            );
        }
    }
}

#[cfg(test)]
mod probe {
    use std::{thread::sleep, time::Duration};

    use super::ffi::{Api, Direction, Phase, Step};

    const STEPS: u32 = 30;
    const SWIPE_TIME: Duration = Duration::from_millis(500);
    /// 1.0 of progress over 0.5 s, in progress per second (units unknown).
    const END_SPEED: f64 = 2.0;

    /// A built step, the pause before posting it, and what to print.
    struct Planned {
        wait: Duration,
        step: Step,
        line: String,
    }

    /// Builds every event of one swipe before anything is posted, so a
    /// failed build can't leave the Dock mid-gesture. `Err` says why.
    fn plan(api: &Api, direction: Direction, legacy: bool) -> Result<Vec<Planned>, String> {
        let sign = direction.sign();
        let mut planned = Vec::new();
        let mut add = |wait, phase, progress, velocity: Option<f64>, note: &str| {
            let step = api
                .build(direction, phase, progress, velocity, legacy)
                .ok_or_else(|| format!("{direction:?} {phase:?} {progress:+.3} didn't build"))?;
            let velocity = velocity.map_or(String::new(), |v| format!(", velocity {v:+.1}"));
            planned.push(Planned {
                wait,
                step,
                line: format!("{direction:?} {phase:?} progress {progress:+.3}{velocity}{note}"),
            });
            Ok::<(), String>(())
        };
        add(Duration::ZERO, Phase::Began, 0.0, None, "")?;
        for i in 1..=STEPS {
            let progress = sign * f64::from(i) / f64::from(STEPS);
            add(SWIPE_TIME / STEPS, Phase::Changed, progress, None, "")?;
        }
        // An end without velocity beats a swipe left open (fix round 1).
        let mut velocity = Some(sign * END_SPEED);
        if api
            .build(direction, Phase::Ended, sign, velocity, legacy)
            .is_none()
        {
            println!("{direction:?}: the end with velocity didn't build; ending without velocity.");
            velocity = None;
        }
        add(Duration::ZERO, Phase::Ended, sign, velocity, " (end)")?;
        let resend_200 = Duration::from_millis(200);
        add(
            resend_200,
            Phase::Ended,
            sign,
            velocity,
            " (end re-sent, +200 ms)",
        )?;
        let resend_500 = Duration::from_millis(300);
        add(
            resend_500,
            Phase::Ended,
            sign,
            velocity,
            " (end re-sent, +500 ms)",
        )?;
        Ok(planned)
    }

    fn swipe(api: &Api, direction: Direction, legacy: bool) {
        match plan(api, direction, legacy) {
            Ok(planned) => {
                for p in planned {
                    sleep(p.wait);
                    Api::post(&p.step);
                    println!("{}", p.line);
                }
            }
            Err(why) => println!("{direction:?}: posted nothing, because {why}."),
        }
    }

    /// Run by the maintainer only, after `attached_dock_swipe_reads_back`
    /// passes: `cargo test dock_swipe_probe -- --ignored --nocapture`.
    /// Posts real events. Watch the screen.
    #[test]
    #[ignore = "posts real dock-swipe events; the maintainer runs and watches it"]
    fn dock_swipe_probe() {
        let api = Api::load().expect("the private dock-swipe symbols resolve on this macOS");
        if !crate::platform::permissions::is_accessibility_trusted() {
            println!(
                "WARNING: this terminal isn't trusted for Accessibility; macOS will drop the events."
            );
        }
        let legacy = std::env::var("MOUSETUNE_PROBE_ENCODING").as_deref() == Ok("legacy-mix");
        println!(
            "Encoding: {}.",
            if legacy {
                "legacy-mix (HID dock swipe + legacy gesture fields)"
            } else {
                "hid (HID dock swipe only)"
            }
        );
        println!(
            "In 3 s: a slow swipe to the RIGHT (expect the next Space), then UP (expect Mission Control), then DOWN (expect it to close)."
        );
        sleep(Duration::from_secs(3));
        swipe(&api, Direction::Right, legacy);
        sleep(Duration::from_secs(2));
        swipe(&api, Direction::Up, legacy);
        sleep(Duration::from_secs(2));
        swipe(&api, Direction::Down, legacy);
    }
}
