//! IOHIDManager adapter (spec §4.3): reports connected mice as
//! `DeviceEvent`s. `device_info` is the only logic, and it is pure.

use std::{
    ffi::{CStr, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
};

use objc2_core_foundation::{
    CFArray, CFDictionary, CFNumber, CFRetained, CFRunLoop, CFString, Type, kCFRunLoopDefaultMode,
};
use objc2_io_kit::{
    IOHIDDevice, IOHIDElement, IOHIDManager, IOIteratorNext, IOObjectRelease,
    IORegistryEntryCreateIterator, IORegistryEntryGetRegistryEntryID, IOReturn, io_iterator_t,
    io_registry_entry_t, kIOHIDDeviceUsageKey, kIOHIDDeviceUsagePageKey, kIOHIDManufacturerKey,
    kIOHIDOptionsTypeNone, kIOHIDProductIDKey, kIOHIDProductKey, kIOHIDVendorIDKey,
    kIORegistryIterateRecursively, kIOReturnSuccess, kIOServicePlane,
};

use crate::input::devices::{DeviceEvent, DeviceInfo};

const GENERIC_DESKTOP_PAGE: u32 = 0x01;
const WHEEL_USAGE: u32 = 0x38;
const BUTTON_PAGE: u32 = 0x09;
const MOUSE_USAGE: i32 = 0x02;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HidProps {
    pub registry_id: u64,
    pub sender_ids: Vec<u64>,
    pub vendor_id: Option<u32>,
    pub product_id: Option<u32>,
    pub product: Option<String>,
    pub manufacturer: Option<String>,
    /// (usage page, usage) of the device's input elements.
    pub usages: Vec<(u32, u32)>,
}

pub fn device_info(props: HidProps) -> DeviceInfo {
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
        button_count: u8::try_from(buttons.len()).unwrap_or(u8::MAX),
        has_wheel,
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
        // SAFETY: `run_loop` is retained by the monitor for as long as the
        // manager stays scheduled on it.
        unsafe { manager.schedule_with_run_loop(run_loop, default_mode()) };
        let result: IOReturn = manager.open(kIOHIDOptionsTypeNone);
        if result != kIOReturnSuccess {
            tracing::warn!(
                result,
                "IOHIDManagerOpen failed; device list may be incomplete"
            );
        }

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
        let result = self.manager.close(kIOHIDOptionsTypeNone);
        if result != kIOReturnSuccess {
            tracing::warn!(result, "IOHIDManagerClose failed");
        }
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
            sink(DeviceEvent::Added(device_info(props)));
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
        usages: input_usages(device),
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

/// (usage page, usage) of the device's input elements.
fn input_usages(device: &IOHIDDevice) -> Vec<(u32, u32)> {
    // SAFETY: a `None` matching dictionary means "all elements".
    let Some(elements) = (unsafe { device.matching_elements(None, kIOHIDOptionsTypeNone) }) else {
        return Vec::new();
    };
    // SAFETY: IOHIDDeviceCopyMatchingElements returns an array of IOHIDElements.
    let elements = unsafe { CFRetained::cast_unchecked::<CFArray<IOHIDElement>>(elements) };
    elements
        .iter()
        // kIOHIDElementTypeInput_Misc (1) through Input_NULL (5).
        .filter(|element| (1..=5).contains(&element.r#type().0))
        .map(|element| (element.usage_page(), element.usage()))
        .collect()
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
