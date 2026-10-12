//! Port to the settings file (design §2).

use std::io;

pub trait ConfigFile: Send {
    /// Contents of the config file, or `None` if it doesn't exist.
    fn read(&self) -> io::Result<Option<String>>;
    /// Replaces the file atomically (temp file, fsync, rename).
    fn write_atomic(&self, contents: &str) -> io::Result<()>;
    /// Renames the current file to a timestamped backup; returns the backup's file name.
    fn backup(&self) -> io::Result<String>;
}
