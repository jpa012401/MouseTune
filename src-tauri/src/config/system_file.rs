//! The settings file on disk (design §2). The only config code that touches `std::fs`.

use std::{
    fs, io,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::config::file::ConfigFile;

pub struct SystemConfigFile {
    path: PathBuf,
}

impl SystemConfigFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn sibling(&self, name: &str) -> PathBuf {
        self.path.with_file_name(name)
    }
}

impl ConfigFile for SystemConfigFile {
    fn read(&self) -> io::Result<Option<String>> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn write_atomic(&self, contents: &str) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = self.sibling("config.json.tmp");
        let mut file = fs::File::create(&tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, &self.path)
    }

    fn backup(&self) -> io::Result<String> {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut name = format!("config.{secs}.bak.json");
        let mut n = 1;
        while self.sibling(&name).exists() {
            name = format!("config.{secs}-{n}.bak.json");
            n += 1;
        }
        fs::rename(&self.path, self.sibling(&name))?;
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU32, Ordering},
    };

    use super::*;

    /// A fresh directory under the OS temp dir, removed when dropped.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "mousetune-config-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self) -> SystemConfigFile {
            SystemConfigFile::new(self.0.join("config.json"))
        }
        fn names(&self) -> Vec<String> {
            let mut names: Vec<_> = fs::read_dir(&self.0)
                .unwrap()
                .map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_file_reads_as_none() {
        assert_eq!(TempDir::new().file().read().unwrap(), None);
    }

    #[test]
    fn write_then_read_round_trips_and_leaves_no_temp_file() {
        let dir = TempDir::new();
        dir.file().write_atomic("{\"version\":1}").unwrap();
        assert_eq!(
            dir.file().read().unwrap().as_deref(),
            Some("{\"version\":1}")
        );
        assert_eq!(dir.names(), ["config.json"]);
    }

    #[test]
    fn write_creates_missing_parent_directories() {
        let dir = TempDir::new();
        let file = SystemConfigFile::new(dir.0.join("nested/deeper/config.json"));
        file.write_atomic("x").unwrap();
        assert_eq!(file.read().unwrap().as_deref(), Some("x"));
    }

    #[test]
    fn write_replaces_existing_contents() {
        let dir = TempDir::new();
        dir.file().write_atomic("old").unwrap();
        dir.file().write_atomic("new").unwrap();
        assert_eq!(dir.file().read().unwrap().as_deref(), Some("new"));
    }

    #[test]
    fn backup_moves_the_file_aside_and_returns_its_name() {
        let dir = TempDir::new();
        dir.file().write_atomic("broken").unwrap();
        let name = dir.file().backup().unwrap();
        assert!(
            name.starts_with("config.") && name.ends_with(".bak.json"),
            "{name}"
        );
        assert_eq!(dir.names(), vec![name.clone()]);
        assert_eq!(fs::read_to_string(dir.0.join(&name)).unwrap(), "broken");
        assert_eq!(dir.file().read().unwrap(), None);
    }

    #[test]
    fn two_backups_in_the_same_second_get_different_names() {
        let dir = TempDir::new();
        dir.file().write_atomic("first").unwrap();
        let a = dir.file().backup().unwrap();
        dir.file().write_atomic("second").unwrap();
        let b = dir.file().backup().unwrap();
        assert_ne!(a, b);
        assert_eq!(dir.names().len(), 2);
    }
}
