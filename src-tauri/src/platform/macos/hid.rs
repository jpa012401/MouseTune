//! IOHIDManager adapter (spec §4.3): reports connected mice as
//! `DeviceEvent`s. `input_usages` and `device_info` are the only logic, and pure.

use std::{
    ffi::{CStr, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
};

use objc2_core_foundation::{
    CFArray, CFDictionary, CFMutableDictionary, CFNumber, CFRetained, CFRunLoop, CFString, CFType,
    Type, kCFRunLoopDefaultMode,
};
use objc2_io_kit::{
    IOHIDDevice, IOHIDManager, IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperties,
    IORegistryEntryCreateIterator, IORegistryEntryGetRegistryEntryID, IOReturn, io_iterator_t,
    io_registry_entry_t, kIOHIDDeviceUsageKey, kIOHIDDeviceUsagePageKey, kIOHIDElementKey,
    kIOHIDManufacturerKey, kIOHIDOptionsTypeNone, kIOHIDProductIDKey, kIOHIDProductKey,
    kIOHIDVendorIDKey, kIORegistryIterateRecursively, kIOReturnSuccess, kIOServicePlane,
};

use crate::input::devices::{DeviceEvent, DeviceInfo};

const GENERIC_DESKTOP_PAGE: u32 = 0x01;
const WHEEL_USAGE: u32 = 0x38;
const BUTTON_PAGE: u32 = 0x09;
const UNDEFINED_PAGE: u32 = 0x00;
const MOUSE_USAGE: i32 = 0x02;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HidProps {
    pub registry_id: u64,
    pub sender_ids: Vec<u64>,
    pub vendor_id: Option<u32>,
    pub product_id: Option<u32>,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    /// (usage page, usage) of the device's input elements. Empty means the
    /// elements couldn't be read, and `device_info` fails open.
    pub usages: Vec<(u32, u32)>,
}

/// One HID element as the IORegistry `Elements` property describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawElement {
    pub usage_page: u32,
    pub usage: u32,
    /// `IOHIDElementType`: 1–5 input, 129 output, 257 feature, 513 collection.
    pub kind: u32,
    pub children: Vec<RawElement>,
}

/// (usage page, usage) of every input element in the tree.
pub fn input_usages(elements: &[RawElement]) -> Vec<(u32, u32)> {
    let mut usages = Vec::new();
    collect_input_usages(elements, &mut usages);
    usages
}

fn collect_input_usages(elements: &[RawElement], usages: &mut Vec<(u32, u32)>) {
    for element in elements {
        // kIOHIDElementTypeInput_Misc (1) through Input_NULL (5). Usage
        // page 0 (Undefined) marks padding and placeholder elements.
        if (1..=5).contains(&element.kind) && element.usage_page != UNDEFINED_PAGE {
            usages.push((element.usage_page, element.usage));
        }
        collect_input_usages(&element.children, usages);
    }
}

/// What `device_info` assumes when a mouse's elements can't be read: enough
/// for the Buttons and Scroll taps. A tap with nothing to do passes events
/// through, which is harmless; a tap kept off breaks the mouse's features.
const UNKNOWN_BUTTON_COUNT: u8 = 3;

pub fn device_info(props: HidProps) -> DeviceInfo {
    let elements_known = !props.usages.is_empty();
    let mut buttons: Vec<u32> = props
        .usages
        .iter()
        .filter(|(page, _)| *page == BUTTON_PAGE)
        .map(|(_, usage)| *usage)
        .collect();
    buttons.sort_unstable();
    buttons.dedup();
    let has_wheel = props.usages.contains(&(GENERIC_DESKTOP_PAGE, WHEEL_USAGE));
    let name = props
        .product
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Unknown mouse".to_owned());
    DeviceInfo {
        id: props.registry_id,
        sender_ids: props.sender_ids,
        name,
        manufacturer: props.manufacturer.unwrap_or_default(),
        vendor_id: props.vendor_id.unwrap_or(0),
        product_id: props.product_id.unwrap_or(0),
        button_count: if elements_known {
            u8::try_from(buttons.len()).unwrap_or(u8::MAX)
        } else {
            UNKNOWN_BUTTON_COUNT
        },
        has_wheel: has_wheel || !elements_known,
    }
}

type Sink = Box<dyn FnMut(DeviceEvent)>;

/// Watches HID mice on a run loop. Keep it alive: dropping it stops
/// monitoring, and no callback fires after that.
pub struct DeviceMonitor {
    manager: CFRetained<IOHIDManager>,
    run_loop: CFRetained<CFRunLoop>,
    /// Boxed twice so the callbacks get a thin, stable pointer to it.
    _sink: Box<Sink>,
}

impl DeviceMonitor {
    /// Starts matching mice (Generic Desktop / Mouse) on `run_loop`. Existing
    /// devices arrive as `Added` once the run loop runs. Devices are never
    /// seized, so their events keep reaching the system.
    pub fn start(run_loop: &CFRunLoop, sink: impl FnMut(DeviceEvent) + 'static) -> DeviceMonitor {
        let manager = IOHIDManager::new(None, kIOHIDOptionsTypeNone);
        let mut sink: Box<Sink> = Box::new(Box::new(sink));
        let context: *mut Sink = &mut *sink;

        let matching = CFDictionary::<CFString, CFNumber>::from_slices(
            &[
                &cf_string(kIOHIDDeviceUsagePageKey),
                &cf_string(kIOHIDDeviceUsageKey),
            ],
            &[
                &CFNumber::new_i32(GENERIC_DESKTOP_PAGE as i32),
                &CFNumber::new_i32(MOUSE_USAGE),
            ],
        );
        // SAFETY: the dictionary has CFString keys and CFNumber values, as
        // IOHIDManager matching expects.
        unsafe { manager.set_device_matching(Some(matching.as_opaque())) };
        // SAFETY: both callbacks match `IOHIDDeviceCallback`. `context` points
        // into `sink`, a heap allocation owned by the returned monitor, which
        // unschedules the manager before freeing it.
        unsafe {
            manager.register_device_matching_callback(Some(device_added), context.cast());
            manager.register_device_removal_callback(Some(device_removed), context.cast());
        }
        // Matching and property reads come from the IORegistry, so the
        // manager is never opened: IOHIDManagerOpen would add an Input
        // Monitoring permission dependency for nothing.
        // SAFETY: `run_loop` is retained by the monitor for as long as the
        // manager stays scheduled on it.
        unsafe { manager.schedule_with_run_loop(run_loop, default_mode()) };

        DeviceMonitor {
            manager,
            run_loop: run_loop.retain(),
            _sink: sink,
        }
    }
}

// Verified by the ignored hardware test, not CI: it needs real devices.
impl Drop for DeviceMonitor {
    fn drop(&mut self) {
        // SAFETY: unscheduling from the run loop and mode used in `start`.
        // After this no callback can fire into `_sink`, which drops next.
        unsafe {
            self.manager
                .unschedule_from_run_loop(&self.run_loop, default_mode())
        };
    }
}

fn default_mode() -> &'static CFString {
    // SAFETY: reading an immutable CoreFoundation constant.
    unsafe { kCFRunLoopDefaultMode }.expect("kCFRunLoopDefaultMode is always defined")
}

fn cf_string(key: &CStr) -> CFRetained<CFString> {
    CFString::from_str(key.to_str().expect("IOKit keys are ASCII"))
}

/// Runs a callback body against the sink, catching panics so they never
/// unwind into IOKit.
fn with_sink(context: *mut c_void, name: &str, body: impl FnOnce(&mut Sink)) {
    // SAFETY: `context` is the `Sink` pointer registered in `start`; the
    // monitor keeps it alive while the manager is scheduled. Callbacks run on
    // the monitor's run loop thread one at a time, so the borrow is unique.
    let sink = unsafe { &mut *context.cast::<Sink>() };
    if catch_unwind(AssertUnwindSafe(|| body(sink))).is_err() {
        tracing::error!(callback = name, "HID callback panicked; event dropped");
    }
}

unsafe extern "C-unwind" fn device_added(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    device: NonNull<IOHIDDevice>,
) {
    with_sink(context, "device_added", |sink| {
        // SAFETY: IOKit passes a valid device for the call's duration.
        let device = unsafe { device.as_ref() };
        if let Some(props) = hid_props(device) {
            let info = device_info(props);
            tracing::debug!(
                id = info.id,
                name = %info.name,
                vendor_id = info.vendor_id,
                sender_ids = info.sender_ids.len(),
                button_count = info.button_count,
                has_wheel = info.has_wheel,
                "HID device added"
            );
            sink(DeviceEvent::Added(info));
        }
    });
}

unsafe extern "C-unwind" fn device_removed(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    device: NonNull<IOHIDDevice>,
) {
    with_sink(context, "device_removed", |sink| {
        // SAFETY: IOKit passes a valid device for the call's duration.
        let device = unsafe { device.as_ref() };
        if let Some(id) = registry_id(device.service()) {
            tracing::debug!(id, "HID device removed");
            sink(DeviceEvent::Removed { id });
        }
    });
}

/// Reads everything `device_info` needs. `None` if the device has no
/// IORegistry entry, since it then can't be identified or removed.
fn hid_props(device: &IOHIDDevice) -> Option<HidProps> {
    let service = device.service();
    let Some(registry_id) = registry_id(service) else {
        tracing::warn!("HID device without a registry ID; ignoring it");
        return None;
    };
    Some(HidProps {
        registry_id,
        sender_ids: service_and_descendant_ids(service, registry_id),
        vendor_id: number_property(device, kIOHIDVendorIDKey),
        product_id: number_property(device, kIOHIDProductIDKey),
        product: string_property(device, kIOHIDProductKey),
        manufacturer: string_property(device, kIOHIDManufacturerKey),
        usages: registry_input_usages(service, registry_id),
    })
}

fn registry_id(entry: io_registry_entry_t) -> Option<u64> {
    if entry == 0 {
        return None;
    }
    let mut id = 0u64;
    // SAFETY: `entry` is a live registry handle and `id` a valid out-pointer.
    let result = unsafe { IORegistryEntryGetRegistryEntryID(entry, &mut id) };
    (result == kIOReturnSuccess).then_some(id)
}

/// The service's own ID plus every descendant's in the IOService plane.
/// CGEvent field 87 carries a descendant event service's ID.
fn service_and_descendant_ids(service: io_registry_entry_t, own_id: u64) -> Vec<u64> {
    let mut ids = vec![own_id];
    let mut iterator: io_iterator_t = 0;
    // SAFETY: `service` is a live registry handle, the plane name is a
    // NUL-terminated IOKit constant that is only read, and `iterator` is a
    // valid out-pointer.
    let result = unsafe {
        IORegistryEntryCreateIterator(
            service,
            kIOServicePlane.as_ptr().cast_mut().cast(),
            kIORegistryIterateRecursively,
            &mut iterator,
        )
    };
    if result != kIOReturnSuccess {
        tracing::warn!(result, "could not walk HID service children");
        return ids;
    }
    loop {
        let entry = IOIteratorNext(iterator);
        if entry == 0 {
            break;
        }
        ids.extend(registry_id(entry));
        IOObjectRelease(entry);
    }
    IOObjectRelease(iterator);
    ids
}

fn number_property(device: &IOHIDDevice, key: &CStr) -> Option<u32> {
    let number = device
        .property(&cf_string(key))?
        .downcast::<CFNumber>()
        .ok()?;
    u32::try_from(number.as_i64()?).ok()
}

fn string_property(device: &IOHIDDevice, key: &CStr) -> Option<String> {
    let string = device
        .property(&cf_string(key))?
        .downcast::<CFString>()
        .ok()?;
    Some(string.to_string())
}

/// Input usages from the IORegistry `Elements` property of the IOHIDDevice
/// entry. It's read from the registry because
/// `IOHIDDeviceCopyMatchingElements` returns nothing until the device is
/// opened, and opening it would add an Input Monitoring permission
/// dependency.
fn registry_input_usages(service: io_registry_entry_t, registry_id: u64) -> Vec<(u32, u32)> {
    let usages = registry_properties(service)
        .and_then(|properties| properties.get(&cf_string(kIOHIDElementKey)))
        .map(|elements| input_usages(&raw_elements(&elements)))
        .unwrap_or_default();
    if usages.is_empty() {
        tracing::debug!(
            id = registry_id,
            "HID elements unreadable; assuming 3 buttons and a wheel"
        );
    }
    usages
}

/// A snapshot of the entry's whole property table. Reading the single
/// `Elements` property (`IORegistryEntryCreateCFProperty`, or
/// `IORegistryEntrySearchCFProperty`) returns only the top-level elements,
/// with every collection's children missing; the whole-table copy keeps the
/// nesting (seen on macOS 27 with the built-in trackpad).
fn registry_properties(
    service: io_registry_entry_t,
) -> Option<CFRetained<CFDictionary<CFString, CFType>>> {
    let mut properties: *mut CFMutableDictionary = std::ptr::null_mut();
    // SAFETY: `service` is a live registry handle, `properties` a valid
    // out-pointer, and `None` selects the default allocator.
    let result = unsafe { IORegistryEntryCreateCFProperties(service, &mut properties, None, 0) };
    if result != kIOReturnSuccess {
        return None;
    }
    let properties = NonNull::new(properties)?;
    // SAFETY: on success the call returns a +1 retained dictionary (Create
    // rule) whose keys are property names (CFString) and whose values are
    // CoreFoundation objects.
    Some(unsafe { CFRetained::from_raw(properties.cast()) })
}

/// Converts an `Elements` array into `RawElement`s. Anything that isn't the
/// expected array of dictionaries is skipped.
fn raw_elements(elements: &CFType) -> Vec<RawElement> {
    let Some(array) = elements.downcast_ref::<CFArray>() else {
        return Vec::new();
    };
    // SAFETY: every value in a CFArray is a CoreFoundation object.
    let array = unsafe { array.cast_unchecked::<CFType>() };
    array
        .iter()
        .filter_map(|item| {
            let dictionary = item.downcast_ref::<CFDictionary>()?;
            // SAFETY: IORegistry property dictionaries have CFString keys
            // (OSSymbol) and CoreFoundation object values.
            let dictionary = unsafe { dictionary.cast_unchecked::<CFString, CFType>() };
            Some(raw_element(dictionary))
        })
        .collect()
}

fn raw_element(dictionary: &CFDictionary<CFString, CFType>) -> RawElement {
    let number = |key: &str| {
        dictionary
            .get(&CFString::from_str(key))
            .and_then(|value| value.downcast::<CFNumber>().ok())
            .and_then(|number| number.as_i64())
            // Some elements report usage -1; no real usage is that large.
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0)
    };
    RawElement {
        usage_page: number("UsagePage"),
        usage: number("Usage"),
        kind: number("Type"),
        children: dictionary
            .get(&cf_string(kIOHIDElementKey))
            .map(|children| raw_elements(&children))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props() -> HidProps {
        HidProps {
            registry_id: 7,
            sender_ids: vec![70, 71],
            vendor_id: Some(0x046D),
            product_id: Some(0x4082),
            product: Some("MX Master 3".into()),
            manufacturer: Some("Logitech".into()),
            usages: vec![
                (0x09, 1),
                (0x09, 2),
                (0x09, 3),
                (0x09, 4),
                (0x09, 5),
                (0x01, 0x30),
                (0x01, 0x31),
                (0x01, 0x38),
            ],
        }
    }

    #[test]
    fn maps_identity_and_names() {
        let info = device_info(props());
        assert_eq!(info.id, 7);
        assert_eq!(info.sender_ids, vec![70, 71]);
        assert_eq!(info.vendor_id, 0x046D);
        assert_eq!(info.product_id, 0x4082);
        assert_eq!(info.name, "MX Master 3");
        assert_eq!(info.manufacturer, "Logitech");
    }

    #[test]
    fn counts_distinct_buttons() {
        let mut p = props();
        p.usages.push((0x09, 3)); // duplicate element for button 3
        assert_eq!(device_info(p).button_count, 5);
    }

    #[test]
    fn detects_the_wheel() {
        assert!(device_info(props()).has_wheel);
        let no_wheel = HidProps {
            usages: vec![(0x09, 1), (0x09, 2)],
            ..props()
        };
        assert!(!device_info(no_wheel).has_wheel);
    }

    #[test]
    fn missing_or_blank_names_fall_back() {
        let p = HidProps {
            product: Some("  ".into()),
            manufacturer: None,
            vendor_id: None,
            ..props()
        };
        let info = device_info(p);
        assert_eq!(info.name, "Unknown mouse");
        assert_eq!(info.manufacturer, "");
        assert_eq!(info.vendor_id, 0);
    }

    fn element(usage_page: u32, usage: u32, kind: u32) -> RawElement {
        RawElement {
            usage_page,
            usage,
            kind,
            children: Vec::new(),
        }
    }

    fn collection(children: Vec<RawElement>) -> RawElement {
        RawElement {
            usage_page: 0x01,
            usage: 0x02,
            kind: 513,
            children,
        }
    }

    #[test]
    fn finds_buttons_and_axes_inside_the_application_collection() {
        let mouse = [collection(vec![
            element(0x09, 1, 2),
            element(0x09, 2, 2),
            element(0x09, 3, 2),
            element(0x01, 0x30, 1),
            element(0x01, 0x31, 1),
            element(0x01, 0x38, 1),
        ])];
        assert_eq!(
            input_usages(&mouse),
            [
                (0x09, 1),
                (0x09, 2),
                (0x09, 3),
                (0x01, 0x30),
                (0x01, 0x31),
                (0x01, 0x38)
            ]
        );
    }

    #[test]
    fn walks_nested_collections() {
        let mouse = [collection(vec![
            element(0x09, 1, 2),
            collection(vec![collection(vec![element(0x01, 0x38, 1)])]),
        ])];
        assert_eq!(input_usages(&mouse), [(0x09, 1), (0x01, 0x38)]);
    }

    #[test]
    fn ignores_output_feature_and_collection_elements() {
        let device = [collection(vec![
            element(0x08, 1, 129),
            element(0xFF00, 5, 257),
            element(0x09, 4, 3),
        ])];
        assert_eq!(input_usages(&device), [(0x09, 4)]);
    }

    /// The shape a truncated `Elements` read had on macOS 27: the
    /// collection's children gone, only the undefined-page NULL elements left.
    #[test]
    fn undefined_page_placeholders_are_not_usages() {
        let truncated = [collection(Vec::new()), element(0, 0, 5), element(0, 0, 5)];
        assert_eq!(input_usages(&truncated), []);
    }

    #[test]
    fn unknown_elements_fail_open_to_three_buttons_and_a_wheel() {
        let info = device_info(HidProps {
            usages: Vec::new(),
            ..props()
        });
        assert_eq!(info.button_count, 3);
        assert!(info.has_wheel);
    }

    #[test]
    fn button_count_saturates_at_u8_max() {
        let usages = (1..=300).map(|u| (0x09, u)).collect();
        assert_eq!(
            device_info(HidProps { usages, ..props() }).button_count,
            255
        );
    }

    /// Lists real devices. Run manually:
    /// `cargo test -q lists_connected_mice -- --ignored --nocapture`
    #[test]
    #[ignore = "needs real hardware"]
    fn lists_connected_mice() {
        use objc2_core_foundation::{CFRunLoop, kCFRunLoopDefaultMode};
        let run_loop = CFRunLoop::current().unwrap();
        let _monitor = DeviceMonitor::start(&run_loop, |event| println!("{event:?}"));
        // SAFETY: reading an immutable CoreFoundation constant.
        let mode = unsafe { kCFRunLoopDefaultMode }.unwrap();
        CFRunLoop::run_in_mode(Some(mode), 1.0, false);
    }
}
