//! Accessibility (TCC) trust. See spec §3.1.

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// Returns a `Boolean` (`unsigned char`).
    fn AXIsProcessTrusted() -> u8;
}

pub fn is_accessibility_trusted() -> bool {
    // SAFETY: no arguments, no preconditions; safe to call from any thread.
    unsafe { AXIsProcessTrusted() != 0 }
}
