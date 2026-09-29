//! Turning text into a meaning vector, so `recall` can search by what a note
//! is *about* rather than only the words it happens to use.
//!
//! `recall.rs` was built two-handed from the start — word search that needs no
//! model, and a meaning path behind `RecallConfig::semantic` that merges a
//! cosine score into the ranking. The meaning half had everything except a
//! source of vectors: `Piece.embedding` was only ever `None`,
//! `set_embedding`/`unembedded` sat on the dead-methods list, and
//! `semantic: false` shipped with a comment saying "turn on once an embedding
//! model is installed". This module is the missing source, and it is
//! deliberately the smaller half — the same shape as `speaker.rs`, for the
//! same reason: the encoder itself is an external program, like speech-to-text
//! and text-to-speech, so a model is swapped by editing the config, never by
//! recompiling. `fit.rs` already names the model this machine should run
//! (`all-MiniLM-L6-v2`, ~90MB); what was missing was the seam.
//!
//! Two jobs live here:
//!
//! * **The seam.** Run the configured encoder over a piece of text and read
//!   the numbers back. Text goes in on stdin (and as a `{text}` var, for an
//!   encoder that takes it as an argument instead); parsing reuses
//!   `speaker::parse_embedding`, because "prints numbers, somehow" is the same
//!   loose contract voice encoders have and requiring one exact format would
//!   mean the first encoder you try fails for a reason that has nothing to do
//!   with meaning search.
//!
//! * **The memory.** `Daemon::reload_library` rebuilds the library from the
//!   notes folder on every load, with every `embedding: None`. Without a
//!   remembered copy, each restart would re-run the encoder over every note —
//!   slow, and pointless for notes that have not changed. `Remembered` keeps
//!   vectors keyed by a stable hash of the note's content, so an unchanged
//!   note is rehydrated for free and an edited one (new content, new key)
//!   is re-embedded exactly as it should be. Keys use FNV-1a written out
//!   here rather than `DefaultHasher`, because `DefaultHasher` is seeded per
//!   process and a key that changes every run is a cache that never hits.

use crate::error::{AtlasError, Result};
use crate::tools::{ExternalTool, Vars};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct MeaningConfig {
    /// The program that turns text into a meaning vector.
    ///
    /// Absent means Atlas searches by words alone on this machine. That is
    /// not a failure — word search is complete without it — but with
    /// `recall.semantic` switched on it is a gap worth naming, and `doctor`
    /// does. See `NO_ENCODER`.
    pub encoder: Option<ExternalTool>,
}

/// Run the encoder over a piece of text.
///
/// The text is piped to stdin when the tool asks for it (`stdin_text: true`,
/// the normal shape) and is also available as `{text}` for an encoder that
/// takes its input as an argument. Both, rather than picking one, because
/// every embedding wrapper reads its input a slightly different way and this
/// seam exists so that any of them can be dropped in.
pub fn embed(cfg: &MeaningConfig, vars: &Vars, text: &str) -> Result<Vec<f32>> {
    let Some(tool) = cfg.encoder.as_ref() else {
        return Err(AtlasError::Platform(NO_ENCODER.into()));
    };
    let mut v = vars.clone();
    v.insert("text".into(), text.to_string());
    crate::speaker::parse_embedding(&tool.run(&v, Some(text))?)
}

/// Can this machine make meaning vectors at all?
///
/// Checked by looking for the program, not at the config — a configured
/// encoder that is not installed is the same as none, and reporting it as
/// present would be a check that cannot fail.
pub fn available(cfg: &MeaningConfig, vars: &Vars) -> bool {
    cfg.encoder.as_ref().map(|t| t.available(vars)).unwrap_or(false)
}

/// A stable key for a note's content.
///
/// FNV-1a over title and text, spelled out because it has to give the same
/// answer next run: `std`'s `DefaultHasher` is randomly seeded per process,
/// which for a persisted cache means a key that never matches what was saved.
pub fn key(title: &str, text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in title.as_bytes().iter().chain(b"\n").chain(text.as_bytes()) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Meaning vectors already made, keyed by note content.
///
/// The library is derived state — rebuilt from the notes folder on every
/// load — so the vectors have to live somewhere that survives the rebuild.
/// Content-keyed rather than id-keyed because ids are assigned by directory
/// order and change when a note is added; content does not lie about whether
/// the vector still describes it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Remembered {
    vectors: BTreeMap<String, Vec<f32>>,
    /// Which encoder made these (`fingerprint`). Vectors from two models
    /// live in two different spaces: comparing a question embedded by one
    /// with notes embedded by the other gives numbers that look like
    /// similarity and mean nothing. So a new encoder clears them all.
    #[serde(default)]
    model: String,
}

impl Remembered {
    pub fn load(store: &crate::store::Store) -> Remembered {
        store.load("meaning")
    }

    pub fn save(&self, store: &crate::store::Store) -> Result<()> {
        store.save("meaning", self)
    }

    pub fn get(&self, title: &str, text: &str) -> Option<&Vec<f32>> {
        self.vectors.get(&key(title, text))
    }

    pub fn put(&mut self, title: &str, text: &str, v: Vec<f32>) {
        self.vectors.insert(key(title, text), v);
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Keep these vectors only if `fp` made them; otherwise forget them all
    /// and remember `fp`. Returns true when they were dropped.
    pub fn for_model(&mut self, fp: &str) -> bool {
        if self.model == fp {
            return false;
        }
        let had = !self.vectors.is_empty();
        self.vectors.clear();
        self.model = fp.to_string();
        had
    }

    /// Which encoder made the vectors held now.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Drop vectors for notes that no longer exist.
    ///
    /// Without this the cache only ever grows: every edit of a note leaves
    /// its old content's vector behind forever. Called with the keys the
    /// library currently holds; anything else is a note that was deleted or
    /// rewritten, and its vector describes nothing.
    pub fn keep_only(&mut self, keys: &[String]) {
        self.vectors.retain(|k, _| keys.contains(k));
    }
}

/// Said by `doctor` when meaning search is asked for and cannot run.
///
/// The wording matters the same way `speaker::NO_ENCODER`'s does: an absent
/// encoder must never read as search being broken. Word search is complete
/// without it — this names what is missing, not what failed.
pub const NO_ENCODER: &str =
    "Meaning search is switched on but no encoder is installed, so I search by words alone. \
     Word search works fine — this only costs finding notes by what they're about when they \
     share no words with the question. Install an embedding model and name it under \
     tools.meaning.encoder to close the gap.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_is_stable_and_content_sensitive() {
        let a = key("Harbour", "silts to four metres");
        assert_eq!(a, key("Harbour", "silts to four metres"), "same content, same key");
        assert_ne!(a, key("Harbour", "silts to five metres"), "changed text, changed key");
        assert_ne!(a, key("Harbor", "silts to four metres"), "changed title, changed key");
    }

    #[test]
    fn remembered_round_trips_and_prunes() {
        let mut r = Remembered::default();
        r.put("a", "one", vec![1.0; 16]);
        r.put("b", "two", vec![2.0; 16]);
        assert_eq!(r.get("a", "one").map(|v| v.len()), Some(16));
        assert!(r.get("a", "edited").is_none(), "edited content is a different note");
        r.keep_only(&[key("a", "one")]);
        assert_eq!(r.len(), 1, "the deleted note's vector went with it");
    }

    #[test]
    fn no_encoder_is_an_error_that_names_itself() {
        let err = embed(&MeaningConfig::default(), &Vars::new(), "anything").unwrap_err();
        assert!(format!("{err}").contains("no encoder"), "{err}");
    }
}

/// Which encoder this is, for telling vectors apart: the program and its
/// arguments, and the size and date of any argument that is a file (the
/// model). Change the model file and the fingerprint changes with it.
/// Empty when there is no encoder.
pub fn fingerprint(cfg: &MeaningConfig, vars: &Vars) -> String {
    let Some(tool) = cfg.encoder.as_ref() else { return String::new() };
    let (cmd, args) = tool.resolved(vars);
    let mut parts = vec![cmd];
    for a in args {
        let p = std::path::Path::new(&a);
        match std::fs::metadata(p) {
            Ok(m) if m.is_file() => {
                let when = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                parts.push(format!("{a}@{}:{when}", m.len()));
            }
            _ => parts.push(a),
        }
    }
    key("encoder", &parts.join("\u{1f}"))
}
