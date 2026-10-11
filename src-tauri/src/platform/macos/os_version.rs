//! Running macOS version. All version-gated code paths branch on this (spec §3.4).

use std::fmt;

use objc2_foundation::NSProcessInfo;
use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Type)]
pub struct OsVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl OsVersion {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    // First callers arrive with the version-gated paths in Phase 2 (spec §3.4).
    #[allow(dead_code)]
    pub fn at_least(self, major: u32, minor: u32) -> bool {
        self >= Self::new(major, minor, 0)
    }
}

impl fmt::Display for OsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub fn current() -> OsVersion {
    let v = NSProcessInfo::processInfo().operatingSystemVersion();
    let part = |n: isize| u32::try_from(n).unwrap_or(0);
    OsVersion::new(
        part(v.majorVersion),
        part(v.minorVersion),
        part(v.patchVersion),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_compares_major_then_minor() {
        assert!(OsVersion::new(27, 0, 1).at_least(14, 0));
        assert!(OsVersion::new(13, 6, 0).at_least(13, 0));
        assert!(!OsVersion::new(12, 7, 6).at_least(13, 0));
        assert!(!OsVersion::new(14, 0, 0).at_least(14, 1));
    }

    #[test]
    fn current_is_supported() {
        assert!(current().at_least(12, 0), "running on {}", current());
    }
}
