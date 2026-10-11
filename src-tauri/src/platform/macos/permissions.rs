//! Accessibility (TCC) trust. See spec §3.1.

use std::{ffi::c_void, process::Command};

use objc2_foundation::{NSDictionary, NSNumber, NSString};

const ACCESSIBILITY_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// Returns a `Boolean` (`unsigned char`).
    fn AXIsProcessTrusted() -> u8;
    /// `options` is a `CFDictionaryRef`; returns a `Boolean`.
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    /// A `CFStringRef`, toll-free bridged to `NSString`.
    static kAXTrustedCheckOptionPrompt: *const NSString;
}

pub fn is_accessibility_trusted() -> bool {
    // SAFETY: no arguments, no preconditions; safe to call from any thread.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Shows the system "would like to control this computer" prompt if the app
/// isn't trusted yet. macOS only shows it once per app identity.
pub fn prompt_for_accessibility() -> bool {
    // SAFETY: `kAXTrustedCheckOptionPrompt` is a valid, immortal CFString
    // constant; NSDictionary is toll-free bridged to CFDictionary and
    // outlives the call.
    unsafe {
        let key = &*kAXTrustedCheckOptionPrompt;
        let yes = NSNumber::new_bool(true);
        let options = NSDictionary::from_slices(&[key], &[&*yes]);
        AXIsProcessTrustedWithOptions(&*options as *const _ as *const c_void) != 0
    }
}

pub fn open_accessibility_settings() -> std::io::Result<()> {
    Command::new("/usr/bin/open")
        .arg(ACCESSIBILITY_PANE)
        .status()?;
    Ok(())
}

/// Clears this app's Accessibility entry, e.g. a stale grant left by a build
/// with a different code signature.
pub fn reset_accessibility(bundle_id: &str) -> std::io::Result<()> {
    let status = Command::new("/usr/bin/tccutil")
        .args(["reset", "Accessibility", bundle_id])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "tccutil exited with {status}"
        )))
    }
}
