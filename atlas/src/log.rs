//! Bounded local logging — a Milestone 1 acceptance criterion.
//!
//! Rotates by size and keeps exactly one previous file. An assistant running
//! all day will otherwise quietly fill a disk.

use std::io::Write;
use std::path::PathBuf;

pub struct Log {
    path: PathBuf,
    max_bytes: u64,
}

impl Log {
    pub fn new(dir: impl Into<PathBuf>, max_bytes: u64) -> Log {
        let dir: PathBuf = dir.into();
        let _ = std::fs::create_dir_all(&dir);
        Log { path: dir.join("atlas.log"), max_bytes }
    }

    pub fn write(&self, level: &str, msg: &str) {
        self.rotate_if_needed();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(f, "{} {level} {msg}", crate::store::now());
        }
    }

    pub fn info(&self, msg: &str) {
        self.write("INFO", msg);
    }
    pub fn warn(&self, msg: &str) {
        self.write("WARN", msg);
    }

    fn rotate_if_needed(&self) {
        let Ok(md) = std::fs::metadata(&self.path) else { return };
        if md.len() < self.max_bytes {
            return;
        }
        let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
    }

    pub fn size(&self) -> u64 {
        std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }
}
