//! Long work in phases, each written down as it finishes, so a restart picks
//! up after the last finished phase instead of starting over.
//!
//! From wshobson/agents' `comprehensive-review/full-review`: each phase
//! writes its result to a file and a state file says how far the work got.
//! Rewritten small for Atlas: a folder per piece of work under the state
//! folder, one JSON file per finished phase, keyed by what the work *is*
//! (your words and where it applies) so asking again — or `resume` redoing it
//! after a restart — finds the phases already done.
//!
//! What it doesn't do: a phase that was halfway through when Atlas stopped
//! is run again from its start. Only a finished phase is kept.

use serde::{de::DeserializeOwned, Serialize};
use std::path::{Path, PathBuf};

/// One piece of work's phases on disk.
#[derive(Debug, Clone)]
pub struct Phases {
    dir: PathBuf,
}

impl Phases {
    /// The folder for this piece of work: `root/phases/<kind>-<key hash>`.
    pub fn for_work(root: &Path, kind: &str, key: &str) -> Phases {
        let hash = crate::digest::sha256_hex(key.as_bytes());
        Phases { dir: root.join("phases").join(format!("{kind}-{}", &hash[..16])) }
    }

    /// A finished phase's result, if it finished before.
    pub fn done<T: DeserializeOwned>(&self, phase: &str) -> Option<T> {
        let text = std::fs::read_to_string(self.dir.join(format!("{phase}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Write a phase down as finished. Written to a side file and renamed,
    /// so a stop mid-write leaves the phase unfinished rather than half a file.
    pub fn finished<T: Serialize>(&self, phase: &str, result: &T) -> bool {
        let Some(root) = self.dir.parent().and_then(Path::parent) else { return false };
        let Ok(_state) = crate::store::state_transaction(root) else { return false };
        if std::fs::create_dir_all(&self.dir).is_err() {
            return false;
        }
        let Ok(text) = serde_json::to_string(result) else { return false };
        let tmp = self.dir.join(format!("{phase}.json.part"));
        std::fs::write(&tmp, text).is_ok() && crate::store::rename_patiently(&tmp, &self.dir.join(format!("{phase}.json"))).is_ok()
    }

    /// Which phases are written down, for "what's queued".
    pub fn finished_phases(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// The work is filed: its phases aren't needed any more.
    pub fn close(&self) {
        let Some(root) = self.dir.parent().and_then(Path::parent) else { return };
        let Ok(_state) = crate::store::state_transaction(root) else { return };
        crate::heard!(std::fs::remove_dir_all(&self.dir));
    }
}
