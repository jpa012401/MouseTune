//! Pointing devices MouseTune tracks (spec §4.3). Pure model; the IOKit
//! adapter (`platform::macos::hid`) produces `DeviceEvent`s.

/// USB vendor ID of Apple's own peripherals.
pub const APPLE_USB_VENDOR_ID: u32 = 0x05AC;
/// Bluetooth SIG company ID for Apple; some Magic Mice report this instead.
pub const APPLE_BLUETOOTH_VENDOR_ID: u32 = 0x004C;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// IORegistry entry ID of the IOHIDDevice.
    pub id: u64,
    /// Registry IDs of the device's event services. CGEvent field 87
    /// carries one of these.
    pub sender_ids: Vec<u64>,
    pub name: String,
    pub manufacturer: String,
    pub vendor_id: u32,
    pub product_id: u32,
    pub button_count: u8,
    pub has_wheel: bool,
}

impl DeviceInfo {
    /// Apple pointing devices and trackpads already emit trackpad-style
    /// input; MouseTune leaves them alone. Called from the scroll tap on
    /// every wheel tick, so it must not allocate (AGENTS.md invariant 1).
    pub fn is_excluded(&self) -> bool {
        self.vendor_id == APPLE_USB_VENDOR_ID
            || self.vendor_id == APPLE_BLUETOOTH_VENDOR_ID
            || contains_ignore_ascii_case(&self.name, "trackpad")
    }

    /// Has buttons beyond left and right (spec: buttons 3+).
    pub fn has_extra_buttons(&self) -> bool {
        self.button_count >= 3
    }
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    Added(DeviceInfo),
    Removed { id: u64 },
}

#[derive(Debug, Default)]
pub struct DeviceRegistry {
    devices: Vec<DeviceInfo>,
}

impl DeviceRegistry {
    /// Adds a device, replacing any existing entry with the same `id`.
    pub fn insert(&mut self, device: DeviceInfo) {
        self.remove(device.id);
        self.devices.push(device);
    }

    pub fn remove(&mut self, id: u64) -> Option<DeviceInfo> {
        let index = self.devices.iter().position(|d| d.id == id)?;
        Some(self.devices.remove(index))
    }

    /// The device that produced an event, by CGEvent field 87.
    pub fn by_sender(&self, sender_id: u64) -> Option<&DeviceInfo> {
        if sender_id == 0 {
            return None;
        }
        self.devices
            .iter()
            .find(|d| d.sender_ids.contains(&sender_id))
    }

    /// Devices MouseTune acts on (excluded devices filtered out).
    pub fn tracked(&self) -> impl Iterator<Item = &DeviceInfo> {
        self.devices.iter().filter(|d| !d.is_excluded())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{apple_trackpad, mouse};

    #[test]
    fn third_party_mouse_is_tracked() {
        assert!(!mouse(1).is_excluded());
    }

    #[test]
    fn apple_usb_devices_are_excluded() {
        assert!(apple_trackpad(1).is_excluded());
    }

    #[test]
    fn apple_bluetooth_vendor_id_is_excluded() {
        let magic_mouse = DeviceInfo {
            vendor_id: 0x004C,
            name: "Magic Mouse".into(),
            ..mouse(1)
        };
        assert!(magic_mouse.is_excluded());
    }

    #[test]
    fn third_party_trackpads_are_excluded_by_name() {
        let trackpad = DeviceInfo {
            name: "Wireless TRACKPAD T650".into(),
            ..mouse(1)
        };
        assert!(trackpad.is_excluded());
    }

    #[test]
    fn mixed_case_trackpad_names_are_excluded() {
        let trackpad = DeviceInfo {
            name: "Perixx TrackPad".into(),
            ..mouse(1)
        };
        assert!(trackpad.is_excluded());
    }

    #[test]
    fn names_merely_close_to_trackpad_are_tracked() {
        let mouse_named = DeviceInfo {
            name: "Track Pad Mouse".into(),
            ..mouse(1)
        };
        assert!(!mouse_named.is_excluded());
    }

    #[test]
    fn three_buttons_count_as_extra_two_do_not() {
        assert!(
            DeviceInfo {
                button_count: 3,
                ..mouse(1)
            }
            .has_extra_buttons()
        );
        assert!(
            !DeviceInfo {
                button_count: 2,
                ..mouse(1)
            }
            .has_extra_buttons()
        );
    }

    #[test]
    fn insert_replaces_same_id() {
        let mut registry = DeviceRegistry::default();
        registry.insert(mouse(1));
        registry.insert(DeviceInfo {
            name: "Renamed".into(),
            ..mouse(1)
        });
        let names: Vec<_> = registry.tracked().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Renamed"]);
    }

    #[test]
    fn remove_returns_the_device_once() {
        let mut registry = DeviceRegistry::default();
        registry.insert(mouse(1));
        assert_eq!(registry.remove(1).map(|d| d.id), Some(1));
        assert_eq!(registry.remove(1), None);
        assert_eq!(registry.tracked().count(), 0);
    }

    #[test]
    fn by_sender_matches_any_of_a_devices_sender_ids() {
        let mut registry = DeviceRegistry::default();
        registry.insert(mouse(1));
        registry.insert(mouse(2));
        assert_eq!(registry.by_sender(21).map(|d| d.id), Some(2));
        assert_eq!(registry.by_sender(10).map(|d| d.id), Some(1));
    }

    #[test]
    fn by_sender_unknown_or_zero_is_none() {
        let mut registry = DeviceRegistry::default();
        registry.insert(mouse(1));
        assert_eq!(registry.by_sender(0), None);
        assert_eq!(registry.by_sender(999), None);
    }

    #[test]
    fn tracked_skips_excluded_devices() {
        let mut registry = DeviceRegistry::default();
        registry.insert(mouse(1));
        registry.insert(apple_trackpad(2));
        let ids: Vec<_> = registry.tracked().map(|d| d.id).collect();
        assert_eq!(ids, [1]);
    }
}
