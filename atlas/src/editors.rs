//! Using editing software you already own.
//!
//! Everything Atlas does to video works with ffmpeg alone, and that's
//! deliberate: your friends won't have Premiere, and a system whose basic
//! functions need a £50-a-month subscription isn't one you can hand to
//! anyone.
//!
//! But if *you* have it, not using it is silly. So professional tools are an
//! optional better path, never a requirement, and Atlas says which one it
//! used so you always know whether the result is reproducible on a plain
//! machine.
//!
//! ## What's actually possible
//!
//! Honesty matters here, because "can Atlas use Adobe" has a more interesting
//! answer than yes or no:
//!
//! * **Premiere** — scriptable through ExtendScript. Atlas can build a whole
//!   sequence: cuts, captions, colour, and hand it to you open and editable.
//!   This is the good case.
//! * **After Effects** — same, plus `aerender` for headless output. Templates
//!   with editable text are the useful part.
//! * **Photoshop** — scriptable, and genuinely useful for thumbnails at
//!   volume.
//! * **DaVinci Resolve** — has a proper Python API, and the free version is
//!   fully capable. For colour work this is the best option for most people,
//!   because it costs nothing.
//! * **Final Cut** — no useful scripting. Atlas can prepare and hand off, not
//!   drive.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Editor {
    /// Always available. The floor everything is built on.
    Ffmpeg,
    Resolve,
    Premiere,
    AfterEffects,
    Photoshop,
    FinalCut,
}

impl Editor {
    pub fn name(&self) -> &'static str {
        match self {
            Editor::Ffmpeg => "ffmpeg",
            Editor::Resolve => "DaVinci Resolve",
            Editor::Premiere => "Premiere Pro",
            Editor::AfterEffects => "After Effects",
            Editor::Photoshop => "Photoshop",
            Editor::FinalCut => "Final Cut Pro",
        }
    }

    /// Can Atlas actually drive it, or only prepare work for it?
    pub fn drivable(&self) -> bool {
        !matches!(self, Editor::FinalCut)
    }

    pub fn how(&self) -> &'static str {
        match self {
            Editor::Ffmpeg => "directly",
            Editor::Resolve => "its Python API — the free version does everything needed",
            Editor::Premiere => "an ExtendScript file it writes and Premiere runs",
            Editor::AfterEffects => "ExtendScript, and aerender for output without opening it",
            Editor::Photoshop => "a script file, useful for thumbnails at volume",
            Editor::FinalCut => "nothing scriptable — Atlas prepares, you finish",
        }
    }

    /// Costs money.
    fn paid(&self) -> bool {
        matches!(self, Editor::Premiere | Editor::AfterEffects | Editor::Photoshop | Editor::FinalCut)
    }
}

/// What a job needs, and what would do it best.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Job {
    Trim,
    CutSilences,
    Captions,
    Loudness,
    ColourCorrect,
    /// Proper grading with nodes and scopes.
    ColourGrade,
    Crop,
    Thumbnail,
    /// Text that moves.
    MotionGraphics,
    /// A sequence you can then edit by hand.
    HandOff,
}

impl Job {
    /// Said, rather than debug-printed.
    pub fn plain(&self) -> &'static str {
        match self {
            Job::Trim => "trimming",
            Job::CutSilences => "cutting silences",
            Job::Captions => "captions",
            Job::Loudness => "loudness",
            Job::ColourCorrect => "colour correction",
            Job::ColourGrade => "colour grading",
            Job::Crop => "cropping",
            Job::Thumbnail => "thumbnails",
            Job::MotionGraphics => "motion graphics",
            Job::HandOff => "handing a sequence over",
        }
    }

    /// Everything ffmpeg alone can do properly.
    ///
    /// This list is the answer to "what do my friends get" — and it's most of
    /// it.
    fn ffmpeg_does_it_well(&self) -> bool {
        matches!(
            self,
            Job::Trim | Job::CutSilences | Job::Captions | Job::Loudness
                | Job::ColourCorrect | Job::Crop | Job::Thumbnail
        )
    }
}

/// Pick a tool for a job, from what's installed.
///
/// The rule: use the better tool when it's there, and never need it.
pub fn best_for(job: Job, installed: &[Editor]) -> (Editor, String) {
    let has = |e: Editor| installed.contains(&e);

    match job {
        // Grading is the one place a real tool is genuinely better rather
        // than merely nicer.
        Job::ColourGrade => {
            if has(Editor::Resolve) {
                (Editor::Resolve, "proper grading, and Resolve is free".into())
            } else if has(Editor::Premiere) {
                (Editor::Premiere, "Lumetri will do it".into())
            } else {
                (
                    Editor::Ffmpeg,
                    "ffmpeg can correct but not really grade — install Resolve if this matters, \
                     it's free"
                        .into(),
                )
            }
        }
        Job::MotionGraphics => {
            if has(Editor::AfterEffects) {
                (Editor::AfterEffects, "templates with editable text".into())
            } else {
                (Editor::Ffmpeg, "simple animated text only — nothing fancy".into())
            }
        }
        Job::Thumbnail => {
            if has(Editor::Photoshop) {
                (Editor::Photoshop, "batched from a template".into())
            } else {
                (Editor::Ffmpeg, "frame pull and text — fine for most".into())
            }
        }
        Job::HandOff => {
            if has(Editor::Premiere) {
                (Editor::Premiere, "a sequence, cut and captioned, for you to finish".into())
            } else if has(Editor::Resolve) {
                (Editor::Resolve, "a timeline for you to finish".into())
            } else {
                (Editor::Ffmpeg, "a finished file rather than a project".into())
            }
        }
        // Everything else: ffmpeg does it properly, so use it. Opening
        // Premiere to normalise audio is slower and no better.
        _ => (Editor::Ffmpeg, "ffmpeg does this properly — no reason to open anything".into()),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    /// Use paid software when it's there.
    pub use_what_you_have: bool,
    /// Never rely on anything but ffmpeg for the basics.
    #[serde(skip, default = "always")]
    pub ffmpeg_is_the_floor: bool,
    /// Found on this machine.
    pub installed: Vec<String>,
}

fn always() -> bool {
    true
}

impl Default for EditorConfig {
    fn default() -> Self {
        EditorConfig {
            use_what_you_have: true,
            ffmpeg_is_the_floor: true,
            installed: Vec::new(),
        }
    }
}

/// Where these usually live, so setup can find them without asking.
pub fn where_to_look(e: Editor) -> &'static str {
    match e {
        Editor::Ffmpeg => "tools/ffmpeg.exe",
        Editor::Resolve => "%PROGRAMFILES%/Blackmagic Design/DaVinci Resolve",
        Editor::Premiere => "%PROGRAMFILES%/Adobe/Adobe Premiere Pro *",
        Editor::AfterEffects => "%PROGRAMFILES%/Adobe/Adobe After Effects *",
        Editor::Photoshop => "%PROGRAMFILES%/Adobe/Adobe Photoshop *",
        Editor::FinalCut => "/Applications/Final Cut Pro.app",
    }
}

/// What Atlas says about which tool it used.
///
/// Saying so matters when you're sharing this: a result that needed Premiere
/// isn't a result your friend can reproduce.
pub fn used(e: Editor, job: Job, cfg: &EditorConfig) -> String {
    let _ = job;
    if e == Editor::Ffmpeg {
        return String::new();
    }
    let mut s = format!("Used {} for that.", e.name());
    if e.paid() && cfg.ffmpeg_is_the_floor {
        s.push_str(" On a machine without it I'd have done it with ffmpeg — slightly less well.");
    }
    s
}

/// What someone with nothing installed still gets.
pub fn without_anything() -> Vec<Job> {
    [
        Job::Trim, Job::CutSilences, Job::Captions, Job::Loudness,
        Job::ColourCorrect, Job::Crop, Job::Thumbnail,
    ]
    .into_iter()
    .filter(|j| j.ffmpeg_does_it_well())
    .collect()
}
