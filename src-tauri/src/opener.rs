//! Opening links and folders in other apps (spec §11 About, General).
//! The webview never opens arbitrary URLs: About passes an `AboutLink`.
//! The macOS adapter is `platform::macos::opener`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

pub trait Opener: Send + Sync {
    /// True when macOS accepted the URL.
    fn open_url(&self, url: &str) -> bool;
    /// Opens a file or folder; a folder opens in Finder. True when accepted.
    fn open_path(&self, path: &Path) -> bool;
}

/// Links the About page may open.
#[allow(dead_code)] // wired in Task 6
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum AboutLink {
    MacMouseFix,
}

impl AboutLink {
    #[allow(dead_code)] // wired in Task 6
    fn url(self) -> &'static str {
        match self {
            Self::MacMouseFix => "https://github.com/noah-nuebling/mac-mouse-fix",
        }
    }
}

#[allow(dead_code)] // wired in Task 6
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind")]
pub enum OpenError {
    #[error("macOS couldn’t open {target}")]
    Refused { target: String },
    #[error("the logs folder couldn’t be found: {message}")]
    NoLogsFolder { message: String },
}

#[allow(dead_code)] // wired in Task 6
pub fn open_about_link(opener: &dyn Opener, link: AboutLink) -> Result<(), OpenError> {
    let url = link.url();
    if opener.open_url(url) {
        Ok(())
    } else {
        Err(OpenError::Refused {
            target: url.to_owned(),
        })
    }
}

/// `dir` is where Tauri says the logs are, or why it couldn't say.
#[allow(dead_code)] // wired in Task 6
pub fn open_logs_folder(
    opener: &dyn Opener,
    dir: Result<PathBuf, String>,
) -> Result<(), OpenError> {
    let dir = dir.map_err(|message| OpenError::NoLogsFolder { message })?;
    if opener.open_path(&dir) {
        Ok(())
    } else {
        Err(OpenError::Refused {
            target: dir.display().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::FakeOpener;

    #[test]
    fn the_mac_mouse_fix_link_opens_its_github_page() {
        let opener = FakeOpener::default();
        assert_eq!(open_about_link(&opener, AboutLink::MacMouseFix), Ok(()));
        assert_eq!(
            opener.opened(),
            ["https://github.com/noah-nuebling/mac-mouse-fix"]
        );
    }

    #[test]
    fn a_refused_link_says_what_couldnt_open() {
        assert_eq!(
            open_about_link(&FakeOpener::refusing(), AboutLink::MacMouseFix),
            Err(OpenError::Refused {
                target: "https://github.com/noah-nuebling/mac-mouse-fix".into()
            })
        );
    }

    #[test]
    fn the_logs_folder_opens_in_finder() {
        let opener = FakeOpener::default();
        assert_eq!(
            open_logs_folder(&opener, Ok(PathBuf::from("/Logs/m"))),
            Ok(())
        );
        assert_eq!(opener.opened(), ["/Logs/m"]);
    }

    #[test]
    fn an_unknown_logs_folder_is_reported_without_opening_anything() {
        let opener = FakeOpener::default();
        assert_eq!(
            open_logs_folder(&opener, Err("no home directory".into())),
            Err(OpenError::NoLogsFolder {
                message: "no home directory".into()
            })
        );
        assert!(opener.opened().is_empty());
    }

    #[test]
    fn a_refused_logs_folder_names_the_folder() {
        assert_eq!(
            open_logs_folder(&FakeOpener::refusing(), Ok(PathBuf::from("/Logs/m"))),
            Err(OpenError::Refused {
                target: "/Logs/m".into()
            })
        );
    }
}
