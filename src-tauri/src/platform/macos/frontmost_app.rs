//! The frontmost app's bundle ID, for Navigate's per-app strategy (spec §7.3).
//! Read on demand from `mt-exec`; NSWorkspace keeps it current via the main
//! run loop, which Tauri runs.

use objc2_app_kit::NSWorkspace;

use crate::actions::perform::FrontmostApp;

pub struct MacFrontmostApp;

impl FrontmostApp for MacFrontmostApp {
    fn bundle_id(&self) -> Option<String> {
        objc2::rc::autoreleasepool(|_| {
            let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
            Some(app.bundleIdentifier()?.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "depends on the desktop session; run by hand"]
    fn reads_a_frontmost_bundle_id() {
        let id = MacFrontmostApp.bundle_id();
        println!("frontmost: {id:?}");
        assert!(id.is_some());
    }
}
