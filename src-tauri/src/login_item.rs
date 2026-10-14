//! Launch at login: the port, and the use cases built on it (spec §3.4,
//! Phase 7a design §2). macOS holds the on/off state; nothing is stored in
//! the config, so a change made in System Settings can't go stale here.
//! Adapters live in `platform::macos::login_item`.

use serde::Serialize;
use specta::Type;

/// Whether MouseTune starts at login, as macOS sees it.
#[allow(dead_code)] // wired in Task 6
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub enum LoginItemStatus {
    Off,
    On,
    /// Registered, but the user must allow it in Login Items (macOS 13+).
    RequiresApproval,
    /// macOS can't register this copy, e.g. a dev build that isn't an app bundle.
    Unavailable,
}

#[allow(dead_code)] // wired in Task 6
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind")]
pub enum LoginItemError {
    #[error("macOS refused to add MouseTune to the login items: {message}")]
    Register { message: String },
    #[error("macOS refused to remove MouseTune from the login items: {message}")]
    Unregister { message: String },
    #[error("couldn’t write the launch agent: {message}")]
    WriteAgent { message: String },
    #[error("couldn’t remove the launch agent: {message}")]
    RemoveAgent { message: String },
}

/// OS operations for starting the app at login.
#[allow(dead_code)] // wired in Task 6
pub trait LoginItem: Send + Sync {
    fn status(&self) -> LoginItemStatus;
    fn enable(&self) -> Result<(), LoginItemError>;
    fn disable(&self) -> Result<(), LoginItemError>;
}

/// Turns launch at login on or off and returns what macOS has afterwards,
/// so the UI shows what happened rather than what was asked.
#[allow(dead_code)] // wired in Task 6
pub fn set_launch_at_login(
    item: &dyn LoginItem,
    enabled: bool,
) -> Result<LoginItemStatus, LoginItemError> {
    if enabled {
        item.enable()?;
    } else {
        item.disable()?;
    }
    Ok(item.status())
}

/// After an upgrade from macOS 12, moves a LaunchAgent login item to
/// `SMAppService` so the app starts once per login (spec §3.4). The legacy
/// item is removed only once the modern one is registered, so a failure
/// still leaves the app starting at login.
#[allow(dead_code)] // wired in Task 6
pub fn migrate_legacy(
    legacy: &dyn LoginItem,
    modern: &dyn LoginItem,
) -> Result<(), LoginItemError> {
    if legacy.status() != LoginItemStatus::On {
        return Ok(());
    }
    modern.enable()?;
    legacy.disable()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeLoginItem;
    use LoginItemStatus::{Off, On, RequiresApproval};

    #[test]
    fn turning_on_reports_what_macos_has_afterwards() {
        let item = FakeLoginItem::with_status(Off);
        assert_eq!(set_launch_at_login(&item, true), Ok(On));
    }

    #[test]
    fn turning_on_can_leave_it_waiting_for_approval() {
        let item = FakeLoginItem::with_status(Off).needing_approval();
        assert_eq!(set_launch_at_login(&item, true), Ok(RequiresApproval));
    }

    #[test]
    fn turning_off_unregisters() {
        let item = FakeLoginItem::with_status(On);
        assert_eq!(set_launch_at_login(&item, false), Ok(Off));
        assert_eq!(item.calls(), ["disable"]);
    }

    #[test]
    fn a_refused_change_is_reported_and_changes_nothing() {
        let item = FakeLoginItem::with_status(Off).failing();
        assert_eq!(
            set_launch_at_login(&item, true),
            Err(LoginItemError::Register {
                message: "denied".into()
            })
        );
        assert_eq!(item.status(), Off);
    }

    #[test]
    fn migration_moves_a_legacy_login_item_to_the_modern_one() {
        let legacy = FakeLoginItem::with_status(On);
        let modern = FakeLoginItem::with_status(Off);
        assert_eq!(migrate_legacy(&legacy, &modern), Ok(()));
        assert_eq!(modern.status(), On);
        assert_eq!(legacy.status(), Off);
    }

    #[test]
    fn migration_keeps_the_legacy_item_when_the_modern_one_is_refused() {
        let legacy = FakeLoginItem::with_status(On);
        let modern = FakeLoginItem::with_status(Off).failing();
        assert!(migrate_legacy(&legacy, &modern).is_err());
        assert_eq!(legacy.status(), On);
        assert_eq!(legacy.calls(), Vec::<&str>::new());
    }

    #[test]
    fn migration_does_nothing_without_a_legacy_item() {
        let legacy = FakeLoginItem::with_status(Off);
        let modern = FakeLoginItem::with_status(Off);
        assert_eq!(migrate_legacy(&legacy, &modern), Ok(()));
        assert_eq!(modern.calls(), Vec::<&str>::new());
        assert_eq!(modern.status(), Off);
    }
}
