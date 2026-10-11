//! Accessibility permission: the port the rest of the app depends on, and the
//! use cases built on it (spec §3.1). The macOS adapter lives in
//! `platform::macos::permissions`; tests use a fake.

use std::io;

/// OS operations for the Accessibility (TCC) permission.
pub trait Accessibility: Send + Sync {
    fn is_trusted(&self) -> bool;
    /// Shows the system prompt if not yet trusted. Returns current trust.
    fn prompt(&self) -> bool;
    fn open_settings(&self) -> io::Result<()>;
    fn reset(&self, bundle_id: &str) -> io::Result<()>;
}

/// Production adapter backed by the macOS APIs.
pub struct SystemAccessibility;

impl Accessibility for SystemAccessibility {
    fn is_trusted(&self) -> bool {
        crate::platform::permissions::is_accessibility_trusted()
    }
    fn prompt(&self) -> bool {
        crate::platform::permissions::prompt_for_accessibility()
    }
    fn open_settings(&self) -> io::Result<()> {
        crate::platform::permissions::open_accessibility_settings()
    }
    fn reset(&self, bundle_id: &str) -> io::Result<()> {
        crate::platform::permissions::reset_accessibility(bundle_id)
    }
}

/// Asks for permission: prompt, and if still untrusted, open the settings pane.
pub fn request_access(ax: &dyn Accessibility) -> Result<(), String> {
    if ax.prompt() {
        return Ok(());
    }
    ax.open_settings().map_err(|err| err.to_string())
}

/// Clears a stale TCC entry, then asks again.
pub fn reset_access(ax: &dyn Accessibility, bundle_id: &str) -> Result<(), String> {
    ax.reset(bundle_id).map_err(|err| err.to_string())?;
    request_access(ax)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeAccessibility;

    #[test]
    fn request_opens_settings_when_prompt_leaves_app_untrusted() {
        let ax = FakeAccessibility::default();
        assert_eq!(request_access(&ax), Ok(()));
        assert_eq!(ax.calls(), ["prompt", "open_settings"]);
    }

    #[test]
    fn request_skips_settings_when_already_trusted() {
        let ax = FakeAccessibility {
            trusted_after_prompt: true,
            ..Default::default()
        };
        assert_eq!(request_access(&ax), Ok(()));
        assert_eq!(ax.calls(), ["prompt"]);
    }

    #[test]
    fn request_reports_settings_failure() {
        let ax = FakeAccessibility {
            open_settings_fails: true,
            ..Default::default()
        };
        assert_eq!(request_access(&ax), Err("open failed".to_string()));
    }

    #[test]
    fn reset_clears_this_apps_entry_then_requests_again() {
        let ax = FakeAccessibility::default();
        assert_eq!(reset_access(&ax, "com.example.app"), Ok(()));
        assert_eq!(
            ax.calls(),
            ["reset:com.example.app", "prompt", "open_settings"]
        );
    }

    #[test]
    fn reset_failure_is_reported_without_prompting() {
        let ax = FakeAccessibility {
            reset_fails: true,
            ..Default::default()
        };
        assert_eq!(
            reset_access(&ax, "com.example.app"),
            Err("tccutil exited with 1".to_string())
        );
        assert_eq!(ax.calls(), ["reset:com.example.app"]);
    }
}
