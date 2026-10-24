//! The models that ship inside the phone app (24 Oct 2026).
//!
//! Eric, 22 Oct 2026: "phone Atlas is supposed to be able to function as a
//! standalone system then connect when it can." Until now the phone had no
//! model until it downloaded one (`phonemodel::fetch`), so a phone with no
//! signal on its first day could only answer what the command floor knows.
//! Now the app carries a small model and the fast meaning layer, and the
//! phone works with no laptop and no internet from the first minute.
//!
//! The phone shell (`AtlasCore.kt`, `AtlasCore.swift`) copies the files it
//! was built with into `<home>/bundled/` on first run. [`adopt`] then checks
//! each one's SHA-256 against the pin here and moves it to where Atlas reads
//! it: a file whose hash is wrong is never used and is said so, never quietly
//! loaded. Moving rather than copying keeps one copy on the phone.
//!
//! Pinned 23 Oct 2026 from the files' own Hugging Face listings (both
//! Apache-2.0); `mobile/bundled-models.json` names the same files for the
//! app builds.

use std::path::{Path, PathBuf};

/// Where the phone shell leaves the files it was built with, under the home.
pub const INBOX: &str = "bundled";

/// Where one bundled file belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Into {
    /// The language models' folder (`models::Registry::dir_for`).
    Models,
    /// Under the home, as `meaningnative` reads it.
    Home,
}

/// One file the app carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// Its name in the inbox, and its path where it belongs.
    pub file: &'static str,
    pub into: Into,
    pub sha256: &'static str,
    pub bytes: u64,
    pub what: &'static str,
    /// Where it came from, for the record (`mobile/bundled-models.json`).
    pub source: &'static str,
}

/// Everything the app carries: about 420 MB.
pub const PIECES: [Piece; 3] = [
    Piece {
        file: crate::meaningnative::MODEL_INT8,
        into: Into::Home,
        sha256: "afdb6f1a0e45b715d0bb9b11772f032c399babd23bfc31fed1c170afc848bdb1",
        bytes: 22_972_370,
        what: "the meaning layer (all-MiniLM-L6-v2, int8)",
        source: "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/751bff37182d3f1213fa05d7196b954e230abad9/onnx/model_int8.onnx",
    },
    Piece {
        file: crate::meaningnative::VOCAB,
        into: Into::Home,
        sha256: "07eced375cec144d27c900241f3e339478dec958f92fddbc551f295c992038a3",
        bytes: 231_508,
        what: "the meaning layer's word list",
        source: "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/751bff37182d3f1213fa05d7196b954e230abad9/vocab.txt",
    },
    Piece {
        file: "Qwen3-0.6B-Q4_K_M.gguf",
        into: Into::Models,
        sha256: "ac2d97712095a558e31573f62f466a3f9d93990898b0ec79d7c974c1780d524a",
        bytes: 396_705_472,
        what: "the phone's own language model (Qwen3 0.6B, 4-bit)",
        source: "https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/resolve/50968a4468ef4233ed78cd7c3de230dd1d61a56b/Qwen3-0.6B-Q4_K_M.gguf",
    },
];

/// What happened to one piece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adopted {
    /// Already where it belongs, the right size.
    Present,
    /// Checked and moved into place.
    Moved,
    /// Not in the inbox and not in place: this app was built without it.
    Missing,
    /// In the inbox but not the file that was pinned: left unused.
    Wrong { why: String },
}

/// Where a piece belongs.
pub(crate) fn place(p: &Piece, home: &Path, models_dir: &Path) -> PathBuf {
    match p.into {
        Into::Models => models_dir.join(p.file),
        Into::Home => home.join(p.file),
    }
}

/// Take in what the app was built with: check each file in `<home>/bundled/`
/// against its pin and move it into place. Safe to call on every start: a
/// piece already in place is left alone, and an empty inbox is the usual case
/// after the first.
pub(crate) fn adopt(home: &Path, models_dir: &Path) -> Vec<(&'static Piece, Adopted)> {
    adopt_from(&home.join(INBOX), home, models_dir)
}

fn adopt_from(inbox: &Path, home: &Path, models_dir: &Path) -> Vec<(&'static Piece, Adopted)> {
    PIECES
        .iter()
        .map(|p| (p, adopt_one(p, inbox, &place(p, home, models_dir))))
        .collect()
}

fn adopt_one(p: &Piece, inbox: &Path, dest: &Path) -> Adopted {
    let from = inbox.join(file_name(p.file));
    let in_place = std::fs::metadata(dest)
        .map(|m| m.len() == p.bytes)
        .unwrap_or(false);
    if !from.exists() {
        return if in_place {
            Adopted::Present
        } else {
            Adopted::Missing
        };
    }
    let size = std::fs::metadata(&from).map(|m| m.len()).unwrap_or(0);
    if size != p.bytes {
        return Adopted::Wrong {
            why: format!(
                "{} is {size} bytes in the app, not the {} it should be.",
                p.what, p.bytes
            ),
        };
    }
    match crate::digest::sha256_file_hex(&from) {
        Ok(h) if h == p.sha256 => {}
        Ok(h) => {
            return Adopted::Wrong {
                why: format!(
                    "{} in the app is not the file that was checked (its fingerprint is {}…).",
                    p.what,
                    &h[..12.min(h.len())]
                ),
            }
        }
        Err(e) => {
            return Adopted::Wrong {
                why: format!("{} couldn't be read to check it: {e}", p.what),
            }
        }
    }
    if let Some(dir) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return Adopted::Wrong {
                why: format!("{} couldn't be put in place: {e}", p.what),
            };
        }
    }
    // A move keeps one copy on the phone; across file systems, a copy and
    // then the inbox's is let go.
    let moved = std::fs::rename(&from, dest)
        .or_else(|_| std::fs::copy(&from, dest).and_then(|_| std::fs::remove_file(&from)));
    match moved {
        Ok(()) => Adopted::Moved,
        Err(e) => Adopted::Wrong {
            why: format!("{} couldn't be put in place: {e}", p.what),
        },
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Lines worth saying about what [`adopt`] found: only trouble, in words.
pub(crate) fn said(outcome: &[(&'static Piece, Adopted)]) -> Vec<String> {
    outcome
        .iter()
        .filter_map(|(_, a)| match a {
            Adopted::Wrong { why } => Some(format!("{why} I won't use it.")),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("atlas-bundled-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A stand-in piece the size and hash of `bytes`.
    fn piece_for(bytes: &[u8], file: &'static str, into: Into) -> Piece {
        use sha2::{Digest, Sha256};
        let hex: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Piece {
            file,
            into,
            sha256: Box::leak(hex.into_boxed_str()),
            bytes: bytes.len() as u64,
            what: "a test file",
            source: "test",
        }
    }

    #[test]
    fn a_checked_file_moves_into_place_and_leaves_no_second_copy() {
        let home = tmp("move");
        let models = home.join("models");
        let p = piece_for(b"weights", "w.gguf", Into::Models);
        std::fs::create_dir_all(home.join(INBOX)).unwrap();
        std::fs::write(home.join(INBOX).join("w.gguf"), b"weights").unwrap();
        assert_eq!(
            adopt_one(&p, &home.join(INBOX), &place(&p, &home, &models)),
            Adopted::Moved
        );
        assert_eq!(std::fs::read(models.join("w.gguf")).unwrap(), b"weights");
        assert!(
            !home.join(INBOX).join("w.gguf").exists(),
            "one copy on the phone"
        );
        // The next start finds it in place.
        assert_eq!(
            adopt_one(&p, &home.join(INBOX), &place(&p, &home, &models)),
            Adopted::Present
        );
    }

    #[test]
    fn a_file_that_is_not_the_pinned_one_is_never_used() {
        let home = tmp("wrong");
        let models = home.join("models");
        let p = piece_for(b"weights", "w.gguf", Into::Models);
        std::fs::create_dir_all(home.join(INBOX)).unwrap();
        // Same size, different bytes.
        std::fs::write(home.join(INBOX).join("w.gguf"), b"WEIGHTS").unwrap();
        let got = adopt_one(&p, &home.join(INBOX), &place(&p, &home, &models));
        assert!(matches!(got, Adopted::Wrong { .. }), "{got:?}");
        assert!(
            !models.join("w.gguf").exists(),
            "a wrong file is not put where it is loaded from"
        );
        let said = said(&[(&PIECES[2], got)]);
        assert!(said[0].contains("I won't use it"), "{said:?}");
    }

    #[test]
    fn a_short_file_is_refused_before_it_is_hashed() {
        let home = tmp("short");
        let p = piece_for(b"weights", "w.gguf", Into::Models);
        std::fs::create_dir_all(home.join(INBOX)).unwrap();
        std::fs::write(home.join(INBOX).join("w.gguf"), b"wei").unwrap();
        let got = adopt_one(&p, &home.join(INBOX), &place(&p, &home, &home.join("m")));
        assert!(
            matches!(&got, Adopted::Wrong { why } if why.contains("3 bytes")),
            "{got:?}"
        );
    }

    #[test]
    fn an_app_built_without_a_piece_says_missing_not_present() {
        let home = tmp("missing");
        let got = adopt(&home, &home.join("models"));
        assert_eq!(got.len(), PIECES.len());
        assert!(got.iter().all(|(_, a)| *a == Adopted::Missing), "{got:?}");
        assert!(
            said(&got).is_empty(),
            "missing is not trouble: the app may download it"
        );
    }

    #[test]
    fn the_meaning_layer_lands_where_the_meaning_encoder_reads_it() {
        let home = Path::new("/h");
        let models = Path::new("/m");
        assert_eq!(
            place(&PIECES[0], home, models),
            home.join(crate::meaningnative::MODEL_INT8)
        );
        assert_eq!(
            place(&PIECES[1], home, models),
            home.join(crate::meaningnative::VOCAB)
        );
        assert_eq!(
            place(&PIECES[2], home, models),
            models.join("Qwen3-0.6B-Q4_K_M.gguf")
        );
    }

    #[test]
    fn the_bundle_is_the_size_the_plan_says() {
        let total: u64 = PIECES.iter().map(|p| p.bytes).sum();
        assert_eq!(total, 419_909_350, "about 420 MB inside the app");
        assert!(PIECES.iter().all(|p| p.sha256.len() == 64));
    }
}

/// The real bundle, end to end (24 Oct 2026): the files `fetch-bundled-models.sh`
/// checked go into a fresh phone home's inbox, are adopted, and are what the
/// phone then loads. Run by hand: `ATLAS_BUNDLE_DIR=atlas/mobile/bundled-cache`.
#[cfg(test)]
mod the_real_bundle {
    use super::*;

    #[test]
    #[ignore = "needs the 420 MB bundle; run with ATLAS_BUNDLE_DIR set"]
    fn a_fresh_phone_takes_in_the_bundle_and_has_its_own_model() {
        let src = PathBuf::from(std::env::var("ATLAS_BUNDLE_DIR").expect("ATLAS_BUNDLE_DIR"));
        let home = std::env::temp_dir().join(format!("atlas-real-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let inbox = home.join(INBOX);
        std::fs::create_dir_all(&inbox).unwrap();
        for p in &PIECES {
            let name = file_name(p.file);
            std::fs::copy(src.join(name), inbox.join(name)).unwrap();
        }
        let models = home.join("models");
        let t = std::time::Instant::now();
        let got = adopt(&home, &models);
        let took = t.elapsed();
        assert!(got.iter().all(|(_, a)| *a == Adopted::Moved), "{got:?}");
        let (path, m) = crate::phonemodel::present(&models).expect("the phone has its own model");
        assert_eq!(m.file, "Qwen3-0.6B-Q4_K_M.gguf");
        assert!(path.exists());
        assert!(
            std::fs::read_dir(&inbox).unwrap().next().is_none(),
            "one copy on the phone"
        );
        assert!(adopt(&home, &models)
            .iter()
            .all(|(_, a)| *a == Adopted::Present));
        println!(
            "BUNDLE adopted {} files, {} bytes, checked and moved in {:?}",
            PIECES.len(),
            PIECES.iter().map(|p| p.bytes).sum::<u64>(),
            took
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
