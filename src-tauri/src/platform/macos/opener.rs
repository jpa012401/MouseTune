//! `NSWorkspace` openers for links and folders.

use std::path::Path;

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};

use crate::opener::Opener;

pub struct MacOpener;

impl Opener for MacOpener {
    fn open_url(&self, url: &str) -> bool {
        objc2::rc::autoreleasepool(|_| {
            let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
                return false;
            };
            NSWorkspace::sharedWorkspace().openURL(&url)
        })
    }

    fn open_path(&self, path: &Path) -> bool {
        objc2::rc::autoreleasepool(|_| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
            NSWorkspace::sharedWorkspace().openURL(&url)
        })
    }
}
