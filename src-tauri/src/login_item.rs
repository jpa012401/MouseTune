//! Launch at login: the port, and the use cases built on it (spec §3.4).
//! macOS holds the on/off state; nothing is stored in the config, so a change made in System Settings can't go stale here.
//! Adapters live in `platform::macos::login_item`.

use std::{io, path::Path, sync::Arc};

use serde::Serialize;
use specta::Type;

/// Whether MouseTune starts at login, as macOS sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub enum LoginItemStatus {
    Off,
    On,
    /// Registered, but the user must allow it in Login Items (macOS 13+).
    RequiresApproval,
    /// macOS can't register this copy, e.g. a dev build that isn't an app bundle.
    Unavailable,
}

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
pub trait LoginItem: Send + Sync {
    fn status(&self) -> LoginItemStatus;
    fn enable(&self) -> Result<(), LoginItemError>;
    fn disable(&self) -> Result<(), LoginItemError>;
}

/// Turns launch at login on or off and returns what macOS has afterwards,
/// so the UI shows what happened rather than what was asked.
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

/// What `SMAppService` reports for the app (macOS 13+), before it is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

/// Reads `SMAppService`'s answer. macOS reports a bundled app it has never
/// registered as `NotFound`, so in a bundle that means off, or the switch
/// could never turn it on. Outside a bundle (`pnpm tauri dev`) it can't work.
pub fn modern_status(service: ServiceStatus, in_app_bundle: bool) -> LoginItemStatus {
    match service {
        ServiceStatus::NotRegistered => LoginItemStatus::Off,
        ServiceStatus::Enabled => LoginItemStatus::On,
        ServiceStatus::RequiresApproval => LoginItemStatus::RequiresApproval,
        ServiceStatus::NotFound if in_app_bundle => LoginItemStatus::Off,
        ServiceStatus::NotFound => LoginItemStatus::Unavailable,
    }
}

/// Whether `program` is the executable inside a `.app` bundle
/// (`<name>.app/Contents/MacOS/<exe>`).
pub fn in_app_bundle(program: &Path) -> bool {
    let macos = program.parent();
    let contents = macos.and_then(Path::parent);
    let app = contents.and_then(Path::parent);
    macos.and_then(Path::file_name) == Some("MacOS".as_ref())
        && contents.and_then(Path::file_name) == Some("Contents".as_ref())
        && app.and_then(Path::extension) == Some("app".as_ref())
}

/// After an upgrade from macOS 12, moves a LaunchAgent login item to
/// `SMAppService` so the app starts once per login (spec §3.4). The legacy
/// item is removed only once the modern one is registered, so a failure
/// still leaves the app starting at login.
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

/// The LaunchAgent plist file in `~/Library/LaunchAgents` (macOS 12).
pub trait AgentFile: Send + Sync {
    /// The plist's text, or `None` when there is none (or it can't be read).
    fn read(&self) -> Option<String>;
    fn write(&self, contents: &str) -> io::Result<()>;
    /// Fails with `ErrorKind::NotFound` when there is no file.
    fn remove(&self) -> io::Result<()>;
}

/// The plist that starts `program` at login. `ProcessType=Interactive` keeps
/// launchd from throttling an app that sits in the input path (spec §3.4).
pub fn launch_agent_plist(label: &str, program: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
  <key>Label</key>\n\
  <string>{label}</string>\n\
  <key>ProgramArguments</key>\n\
  <array>\n\
    <string>{program}</string>\n\
  </array>\n\
  <key>RunAtLoad</key>\n\
  <true/>\n\
  <key>ProcessType</key>\n\
  <string>Interactive</string>\n\
</dict>\n\
</plist>\n",
        label = xml_escape(label),
        program = xml_escape(&program.to_string_lossy()),
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Launch at login on macOS 12: a plist launchd reads at login.
pub struct LaunchAgentLoginItem {
    file: Box<dyn AgentFile>,
    plist: String,
}

impl LaunchAgentLoginItem {
    pub fn new(file: Box<dyn AgentFile>, label: &str, program: &Path) -> Self {
        Self {
            file,
            plist: launch_agent_plist(label, program),
        }
    }

    /// Points an existing plist at this copy of the app, which may have
    /// moved since it was written. Leaves launch at login off if it is off.
    pub fn refresh(&self) -> Result<(), LoginItemError> {
        match self.file.read() {
            Some(current) if current != self.plist => self.enable(),
            _ => Ok(()),
        }
    }
}

impl LoginItem for LaunchAgentLoginItem {
    fn status(&self) -> LoginItemStatus {
        if self.file.read().is_some() {
            LoginItemStatus::On
        } else {
            LoginItemStatus::Off
        }
    }

    fn enable(&self) -> Result<(), LoginItemError> {
        self.file
            .write(&self.plist)
            .map_err(|err| LoginItemError::WriteAgent {
                message: err.to_string(),
            })
    }

    fn disable(&self) -> Result<(), LoginItemError> {
        match self.file.remove() {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(LoginItemError::RemoveAgent {
                message: err.to_string(),
            }),
            _ => Ok(()),
        }
    }
}

/// The login item for this Mac (`modern` is `Some` on macOS 13+), after
/// tidying up what earlier versions or an app move left behind. Failures
/// are logged and never block startup.
pub fn startup(
    modern: Option<Arc<dyn LoginItem>>,
    agent: Arc<LaunchAgentLoginItem>,
) -> Arc<dyn LoginItem> {
    match modern {
        Some(modern) => {
            if let Err(err) = migrate_legacy(agent.as_ref(), modern.as_ref()) {
                tracing::warn!(%err, "couldn’t move launch at login to Login Items");
            }
            modern
        }
        None => {
            if let Err(err) = agent.refresh() {
                tracing::warn!(%err, "couldn’t update the launch agent");
            }
            agent
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakeAgentFile, FakeLoginItem};
    use LoginItemStatus::{Off, On, RequiresApproval, Unavailable};

    #[test]
    fn a_bundled_app_macos_has_never_registered_is_off() {
        assert_eq!(modern_status(ServiceStatus::NotFound, true), Off);
    }

    #[test]
    fn a_copy_outside_an_app_bundle_is_unavailable() {
        assert_eq!(modern_status(ServiceStatus::NotFound, false), Unavailable);
    }

    #[test]
    fn registered_states_read_the_same_in_or_out_of_a_bundle() {
        for in_bundle in [true, false] {
            assert_eq!(modern_status(ServiceStatus::NotRegistered, in_bundle), Off);
            assert_eq!(modern_status(ServiceStatus::Enabled, in_bundle), On);
            assert_eq!(
                modern_status(ServiceStatus::RequiresApproval, in_bundle),
                RequiresApproval
            );
        }
    }

    #[test]
    fn the_executable_inside_an_app_is_in_a_bundle() {
        let program = Path::new("/Applications/MouseTune.app/Contents/MacOS/mousetune");
        assert!(in_app_bundle(program));
    }

    #[test]
    fn a_dev_build_is_not_in_a_bundle() {
        let program = Path::new("/Users/me/MouseTune/src-tauri/target/debug/mousetune");
        assert!(!in_app_bundle(program));
    }

    #[test]
    fn a_macos_folder_outside_an_app_is_not_a_bundle() {
        assert!(!in_app_bundle(Path::new("/tmp/Contents/MacOS/mousetune")));
        assert!(!in_app_bundle(Path::new("/tmp/Thing.app/MacOS/mousetune")));
    }

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

    const LABEL: &str = "com.example.mouse";

    fn agent(file: &FakeAgentFile, program: &str) -> LaunchAgentLoginItem {
        LaunchAgentLoginItem::new(Box::new(file.clone()), LABEL, Path::new(program))
    }

    #[test]
    fn the_plist_starts_the_app_at_login_as_an_interactive_process() {
        assert_eq!(
            launch_agent_plist(
                LABEL,
                Path::new("/Applications/MouseTune.app/Contents/MacOS/mousetune")
            ),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
  <key>Label</key>\n\
  <string>com.example.mouse</string>\n\
  <key>ProgramArguments</key>\n\
  <array>\n\
    <string>/Applications/MouseTune.app/Contents/MacOS/mousetune</string>\n\
  </array>\n\
  <key>RunAtLoad</key>\n\
  <true/>\n\
  <key>ProcessType</key>\n\
  <string>Interactive</string>\n\
</dict>\n\
</plist>\n"
        );
    }

    #[test]
    fn the_plist_escapes_xml_in_the_app_path() {
        let plist = launch_agent_plist(LABEL, Path::new("/Applications/Mice & <Co>/m"));
        assert!(plist.contains("<string>/Applications/Mice &amp; &lt;Co&gt;/m</string>"));
    }

    #[test]
    fn turning_on_writes_the_plist_for_this_app() {
        let file = FakeAgentFile::default();
        let item = agent(&file, "/A/m");
        assert_eq!(set_launch_at_login(&item, true), Ok(On));
        assert_eq!(
            file.contents(),
            Some(launch_agent_plist(LABEL, Path::new("/A/m")))
        );
    }

    #[test]
    fn a_plist_on_disk_means_on() {
        assert_eq!(agent(&FakeAgentFile::holding("x"), "/A/m").status(), On);
        assert_eq!(agent(&FakeAgentFile::default(), "/A/m").status(), Off);
    }

    #[test]
    fn turning_off_removes_the_plist() {
        let file = FakeAgentFile::holding("x");
        assert_eq!(set_launch_at_login(&agent(&file, "/A/m"), false), Ok(Off));
        assert_eq!(file.contents(), None);
    }

    #[test]
    fn turning_off_without_a_plist_is_not_an_error() {
        let item = agent(&FakeAgentFile::default(), "/A/m");
        assert_eq!(set_launch_at_login(&item, false), Ok(Off));
    }

    #[test]
    fn a_plist_that_cant_be_written_is_reported() {
        let item = agent(&FakeAgentFile::default().write_fails(), "/A/m");
        assert_eq!(
            set_launch_at_login(&item, true),
            Err(LoginItemError::WriteAgent {
                message: "disk full".into()
            })
        );
    }

    #[test]
    fn refresh_points_a_moved_app_s_plist_at_its_new_place() {
        let file = FakeAgentFile::holding(&launch_agent_plist(LABEL, Path::new("/Old/m")));
        assert_eq!(agent(&file, "/New/m").refresh(), Ok(()));
        assert_eq!(
            file.contents(),
            Some(launch_agent_plist(LABEL, Path::new("/New/m")))
        );
    }

    #[test]
    fn refresh_leaves_launch_at_login_off() {
        let file = FakeAgentFile::default();
        assert_eq!(agent(&file, "/New/m").refresh(), Ok(()));
        assert_eq!(file.contents(), None);
    }

    #[test]
    fn refresh_doesnt_rewrite_a_current_plist() {
        let current = launch_agent_plist(LABEL, Path::new("/A/m"));
        // A failing disk proves no write was attempted.
        let file = FakeAgentFile::holding(&current).write_fails();
        assert_eq!(agent(&file, "/A/m").refresh(), Ok(()));
    }

    #[test]
    fn on_macos_13_startup_uses_smappservice_and_retires_the_plist() {
        let file = FakeAgentFile::holding("x");
        let modern = FakeLoginItem::with_status(Off);
        let item = startup(
            Some(Arc::new(modern.clone())),
            Arc::new(agent(&file, "/A/m")),
        );
        assert_eq!(item.status(), On);
        assert_eq!(modern.calls(), ["enable"]);
        assert_eq!(file.contents(), None);
    }

    #[test]
    fn on_macos_12_startup_uses_the_plist_and_refreshes_it() {
        let file = FakeAgentFile::holding(&launch_agent_plist(LABEL, Path::new("/Old/m")));
        let item = startup(None, Arc::new(agent(&file, "/New/m")));
        assert_eq!(item.status(), On);
        assert_eq!(
            file.contents(),
            Some(launch_agent_plist(LABEL, Path::new("/New/m")))
        );
    }

    #[test]
    fn a_failed_migration_still_returns_the_modern_item() {
        let file = FakeAgentFile::holding("x");
        let modern = FakeLoginItem::with_status(Off).failing();
        let item = startup(Some(Arc::new(modern)), Arc::new(agent(&file, "/A/m")));
        assert_eq!(item.status(), Off);
        assert_eq!(file.contents(), Some("x".into()));
    }
}
