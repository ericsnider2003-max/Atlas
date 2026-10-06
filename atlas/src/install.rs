//! Getting everything Atlas needs, in one go.
//!
//! Setting this up meant visiting four sites, picking the right build out of a
//! release page, unzipping into the right folder, and downloading two models
//! whose filenames you had to already know. That's a lot of chances to get one
//! step wrong and not find out until something silently doesn't work.
//!
//! So: one list, one command, and it can be run again safely. Anything already
//! present is left alone, anything half-downloaded is replaced, and at the end
//! it says what's there and what isn't rather than assuming success.

use serde::{Deserialize, Serialize};

/// Something Atlas needs on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    pub name: &'static str,
    /// What stops working without it.
    pub without_it: &'static str,
    /// Where it lands.
    pub path: &'static str,
    /// Roughly how big.
    pub mb: u32,
    /// Where it comes from.
    pub from: Source,
    /// Atlas works, less well, without this.
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A release asset on a public repository.
    Release,
    /// A model file.
    Model,
    /// Already on most machines, or installable by the system's own manager.
    SystemPackage,
}

/// Everything, in the order it should be fetched.
///
/// Ordered by what unblocks the most: whisper first, because five capabilities
/// are waiting on it and nothing else unblocks more than one.
pub fn pieces() -> Vec<Piece> {
    vec![
        Piece {
            name: "whisper",
            without_it: "hearing you at all — this is the one that unblocks the most",
            path: "tools/whisper/whisper-cli.exe",
            mb: 30,
            from: Source::Release,
            optional: false,
        },
        Piece {
            name: "the listening model",
            without_it: "hearing you",
            path: "models/ggml-base.en.bin",
            mb: 148,
            from: Source::Model,
            optional: false,
        },
        Piece {
            name: "piper",
            without_it: "talking back",
            path: "tools/piper/piper.exe",
            mb: 20,
            from: Source::Release,
            optional: false,
        },
        Piece {
            name: "a voice",
            without_it: "talking back",
            path: "models/en_US-amy-medium.onnx",
            mb: 63,
            from: Source::Model,
            optional: false,
        },
        Piece {
            name: "ffmpeg",
            without_it: "recording, and anything to do with audio or video",
            path: "tools/ffmpeg/ffmpeg.exe",
            mb: 80,
            from: Source::SystemPackage,
            optional: false,
        },
        Piece {
            name: "tesseract",
            without_it: "reading text off an image",
            path: "tools/tesseract/tesseract.exe",
            mb: 60,
            from: Source::SystemPackage,
            optional: true,
        },
        Piece {
            name: "a thinking model",
            without_it: "reasoning — Atlas falls back to rules, which is narrower",
            path: "models/llm.gguf",
            mb: 4400,
            from: Source::Model,
            optional: true,
        },
    ]
}

/// What's actually needed before anything works.
pub fn required() -> Vec<Piece> {
    pieces().into_iter().filter(|p| !p.optional).collect()
}

/// Total download, in megabytes.
///
/// Named `download_mb` rather than `total_mb`: `retention` has a `total_mb`
/// method, and the deadness scans read bare names -- a call to this one made
/// that one look as though something reached it.
pub fn download_mb(include_optional: bool) -> u32 {
    pieces()
        .iter()
        .filter(|p| include_optional || !p.optional)
        .map(|p| p.mb)
        .sum()
}

/// How a piece is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Missing,
    /// There, and the right size.
    Present,
    /// There and too small — a download that stopped partway. Worse than
    /// missing, because everything downstream fails confusingly instead of
    /// clearly.
    HalfDownloaded,
}

/// Judge a file by its size.
///
/// Not a checksum, deliberately: pinning hashes means the installer breaks
/// every time an upstream release is rebuilt, and a truncated download is the
/// failure that actually happens.
pub fn state_of(p: &Piece, found_bytes: Option<u64>) -> State {
    match found_bytes {
        None => State::Missing,
        Some(b) => {
            let expected = p.mb as u64 * 1_000_000;
            if b < expected / 2 {
                State::HalfDownloaded
            } else {
                State::Present
            }
        }
    }
}

/// What to do about each.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Skip { name: &'static str },
    Fetch { name: &'static str, mb: u32 },
    Replace { name: &'static str, why: &'static str },
}

/// The plan for the pieces you actually asked for.
///
/// This was `plan(found)`, which walked every piece. That is right when
/// `include_optional` is on and wrong the moment it is off: `atlas install`
/// printed "4801MB to fetch" beside a 341MB download, because the list you
/// were shown and the number under it were counting different sets. The list
/// you are shown and the number under it have to be about the same things.
///
/// Taking the pieces as an argument rather than keeping a second `plan()`
/// that means "all of them": two ways to ask the same question is how one of
/// them goes unused and then wrong, and this one had just been wrong.
///
/// Named `what_to_fetch` rather than `plan_for`: `fit::plan_for` works out
/// what fits on a machine, the deadness scans read bare names, and the two
/// would have been one word to every guard in the suite. That is the same
/// mistake this file made once already with `total_mb` -- and that collision
/// held `retention::total_mb` up as alive for weeks.
pub fn what_to_fetch(want: &[Piece], found: &[(&'static str, Option<u64>)]) -> Vec<Step> {
    want
        .iter()
        .map(|p| {
            let bytes = found
                .iter()
                .find(|(n, _)| *n == p.name)
                .and_then(|(_, b)| *b);
            match state_of(p, bytes) {
                State::Present => Step::Skip { name: p.name },
                State::Missing => Step::Fetch { name: p.name, mb: p.mb },
                State::HalfDownloaded => Step::Replace {
                    name: p.name,
                    why: "it stopped partway last time",
                },
            }
        })
        .collect()
}

/// What Atlas says before starting.
///
/// The number people want is how long, and the honest version of that is how
/// much to download rather than a fabricated time.
pub fn before(plan: &[Step]) -> String {
    let to_get: u32 = plan
        .iter()
        .filter_map(|s| match s {
            Step::Fetch { mb, .. } => Some(*mb),
            Step::Replace { name, .. } => {
                pieces().iter().find(|p| p.name == *name).map(|p| p.mb)
            }
            Step::Skip { .. } => None,
        })
        .sum();
    let have = plan.iter().filter(|s| matches!(s, Step::Skip { .. })).count();

    if to_get == 0 {
        return "Everything's already here.".into();
    }
    let mut s = format!("{to_get}MB to fetch");
    if have > 0 {
        s.push_str(&format!(", {have} things already here"));
    }
    s.push_str(". You can close this and run it again — it picks up where it stopped.");
    s
}

/// And after.
///
/// Reports per piece, because "install complete" when the model is missing is
/// how you find out an hour later.
pub fn after(results: &[(&'static str, bool)]) -> String {
    let failed: Vec<&str> = results
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(n, _)| *n)
        .collect();

    if failed.is_empty() {
        return "All there. Run the doctor if you want it checked properly.".into();
    }

    let blocking: Vec<&&str> = failed
        .iter()
        .filter(|n| {
            pieces()
                .iter()
                .any(|p| p.name == **n && !p.optional)
        })
        .collect();

    let mut s = format!("{} didn't come down: {}.", failed.len(), failed.join(", "));
    if blocking.is_empty() {
        s.push_str(" All optional — Atlas works without them.");
    } else {
        let p = pieces();
        if let Some(first) = p.iter().find(|p| p.name == *blocking[0]) {
            s.push_str(&format!(" Without {}, {}.", first.name, first.without_it));
        }
    }
    s.push_str(" Run it again — it only fetches what's missing.");
    s
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct InstallConfig {
    /// Fetch the optional pieces too.
    pub include_optional: bool,
    // Not settable since 19 Sep 2026, and the reason is that **nothing here
    // downloads anything**. `plan`, `before` and `after` describe the work
    // and report on it; the fetching is done by the setup script or by you.
    // A switch for how a downloader behaves, in a tree with no downloader,
    // reads as a behaviour you have chosen.
    //
    // Kept rather than deleted because it records the decision made before
    // that fetcher is written, and `PROMISES_ABOUT_WHAT_IS_NOT_BUILT` in
    // `tests/dead_config.rs` carries it with what is missing.
    //
    /// Keep going when one fails rather than stopping.
    ///
    /// On by default: one flaky download shouldn't cost you the other five.
    #[serde(skip, default = "carry_on")]
    pub carry_on_after_failure: bool,
    /// Where things land.
    pub tools_dir: String,
    pub models_dir: String,
}

fn carry_on() -> bool {
    true
}

impl Default for InstallConfig {
    fn default() -> Self {
        InstallConfig {
            include_optional: false,
            carry_on_after_failure: true,
            tools_dir: "tools".into(),
            models_dir: "models".into(),
        }
    }
}

/// Nothing here needs a key, an account or a payment.
pub const COSTS_NOTHING: &str =
    "Every one of these is free and public. No account, no key, no card. If anything ever asks \
     for one, something is wrong and you should stop.";

// ============ where things land ============
//
// `tools_dir` and `models_dir` are for the machine where the models live on
// another drive, and nothing read either — so `atlas install` reported every
// piece as missing on exactly the machine the settings existed for. That is
// the shape of dead setting that costs you something: the feature looks
// broken rather than unconfigured.
//
// `InstallConfig` also had no field anywhere in `ToolsConfig` and no block in
// `tools.yaml`, so none of it could be set in the first place.

/// Where a piece actually lands, given where you keep things.
///
/// `Piece.path` is written with `tools/` and `models/` prefixes because that
/// is where they go by default. Anything else in the path is left alone: a
/// piece that is neither is not a piece these two settings are about, and
/// silently rewriting it would be worse than ignoring it.
///
/// An absolute `models_dir` wins outright — that is the case the setting is
/// for. A relative one stays relative, so `roots::under_install` anchors it
/// the way every other install-relative path is anchored.
pub fn where_it_lands(p: &Piece, cfg: &InstallConfig) -> String {
    for (prefix, replacement) in
        [("tools/", cfg.tools_dir.trim()), ("models/", cfg.models_dir.trim())]
    {
        if let Some(rest) = p.path.strip_prefix(prefix) {
            if replacement.is_empty() {
                return p.path.to_string();
            }
            let sep = if replacement.ends_with('/') || replacement.ends_with('\\') { "" } else { "/" };
            return format!("{replacement}{sep}{rest}");
        }
    }
    p.path.to_string()
}

/// The pieces you have actually asked for.
///
/// `include_optional` ships off: the optional ones are three quarters of the
/// download and Atlas works without them. Listing them as `need` on a machine
/// that never asked for them is how a first run looks like a failure.
pub fn wanted(cfg: &InstallConfig) -> Vec<Piece> {
    pieces().into_iter().filter(|p| cfg.include_optional || !p.optional).collect()
}
