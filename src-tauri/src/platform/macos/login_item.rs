//! Launch at login (spec §3.4). `MacAppService` is `SMAppService.mainApp`
//! (macOS 13+); `MacAgentFile` is the macOS 12 LaunchAgent plist on disk.
//! Decisions (which to use, migration, refresh) are in `crate::login_item`.

use std::{fs, io, path::PathBuf};

use objc2_service_management::{SMAppService, SMAppServiceStatus};

use crate::login_item::{AgentFile, LoginItem, LoginItemError, LoginItemStatus};

/// `SMAppService.mainApp`. Build this only on macOS 13+.
#[allow(dead_code)] // wired in Task 6
pub struct MacAppService;

impl LoginItem for MacAppService {
    fn status(&self) -> LoginItemStatus {
        // SAFETY: class method and getter with no arguments; SMAppService is
        // thread-safe and exists on macOS 13+, the only versions that build this.
        let status = unsafe { SMAppService::mainAppService().status() };
        match status {
            SMAppServiceStatus::Enabled => LoginItemStatus::On,
            SMAppServiceStatus::RequiresApproval => LoginItemStatus::RequiresApproval,
            SMAppServiceStatus::NotRegistered => LoginItemStatus::Off,
            _ => LoginItemStatus::Unavailable,
        }
    }

    fn enable(&self) -> Result<(), LoginItemError> {
        // SAFETY: as in `status`; the error out-parameter is handled by objc2.
        unsafe { SMAppService::mainAppService().registerAndReturnError() }.map_err(|err| {
            LoginItemError::Register {
                message: err.localizedDescription().to_string(),
            }
        })
    }

    fn disable(&self) -> Result<(), LoginItemError> {
        // SAFETY: as in `enable`.
        unsafe { SMAppService::mainAppService().unregisterAndReturnError() }.map_err(|err| {
            LoginItemError::Unregister {
                message: err.localizedDescription().to_string(),
            }
        })
    }
}

/// `~/Library/LaunchAgents/<bundle id>.plist`.
#[allow(dead_code)] // wired in Task 6
pub struct MacAgentFile {
    path: PathBuf,
}

#[allow(dead_code)] // wired in Task 6
impl MacAgentFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl AgentFile for MacAgentFile {
    fn read(&self) -> Option<String> {
        fs::read_to_string(&self.path).ok()
    }

    fn write(&self, contents: &str) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.path, contents)
    }

    fn remove(&self) -> io::Result<()> {
        fs::remove_file(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "asks macOS about this binary's login item; run by hand"]
    fn sm_app_service_reports_a_status() {
        // Read-only: never registers, so the maintainer's Login Items stay as they are.
        let status = MacAppService.status();
        println!("SMAppService.mainApp status: {status:?}");
    }

    #[test]
    fn agent_file_round_trips_and_reports_a_missing_file() {
        let dir = std::env::temp_dir().join(format!("mousetune-agent-test-{}", std::process::id()));
        let file = MacAgentFile::new(dir.join("LaunchAgents").join("x.plist"));
        assert_eq!(file.read(), None);
        file.write("plist").unwrap();
        assert_eq!(file.read().as_deref(), Some("plist"));
        file.remove().unwrap();
        assert_eq!(
            file.remove().unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
