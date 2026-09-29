//! Reading text off the screen and off paper — the outside program.
//!
//! Where this sits among the other options, cheapest first: the accessibility
//! tree (exact, instant, needs a cooperating app), then reading the pixels
//! (exact enough, a second or two, works on anything visible), then a vision
//! model (understands layout and pictures, and on this hardware genuinely
//! slow).
//!
//! ## This is no longer the way Atlas reads a screen
//!
//! `words` is. It runs two model files inside `atlas.exe` — no second
//! program, no install, nothing to go missing. This module drives
//! `tesseract.exe`, which has **never run**, because nothing in `ATLAS.bat`
//! ever downloaded it: the capability page said *"waiting on tesseract"* from
//! the day it was written until the day `words` replaced it.
//!
//! It is kept rather than deleted for one reason: Tesseract reads capitals and
//! punctuation, and the English recogniser in `words` reads thirty-six
//! lower-case characters and nothing else. If Eric ever installs Tesseract by
//! hand, this is a better reader for a scanned page than `words` is. It is not
//! the default and nothing reaches for it on its own.
//!
//! See `words::NO_CAPITALS` for the limit that makes this worth keeping.

use crate::error::{AtlasError, Result};
use crate::tools::{ExternalTool, Vars};
use serde::Deserialize;

/// Said out loud, so a reader of this module knows where the real one is.
pub const NOT_THE_DEFAULT: &str =
    "Atlas reads screens with `words`, in its own code — this drives tesseract.exe, which is \
     not installed and is not fetched by anything";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OcrConfig {
    /// Off. Turning it on is a deliberate act and needs tesseract on the
    /// machine; `words` needs neither.
    pub enabled: bool,
    /// The OCR engine. Tesseract — free, local, no account, not installed.
    pub engine: Option<ExternalTool>,
    /// Discard words the engine is less sure about than this.
    pub min_confidence: f32,
    /// Language pack.
    pub language: String,
}

impl Default for OcrConfig {
    fn default() -> Self {
        OcrConfig { enabled: false, engine: None, min_confidence: 0.55, language: "eng".into() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub confidence: f32,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub text: String,
    /// How sure the engine was overall, 0 to 1.
    pub confidence: f32,
    pub words: usize,
    /// Words dropped for being below the threshold.
    pub discarded: usize,
}

impl Reading {
    /// Is this worth acting on?
    ///
    /// OCR fails by producing plausible nonsense rather than an error, so a
    /// low-confidence reading has to be treated as "I couldn't read it" rather
    /// than handed on as text.
    pub fn trustworthy(&self) -> bool {
        self.confidence >= 0.7 && self.words >= 2
    }

    pub fn summary(&self) -> String {
        if self.words == 0 {
            return "I couldn't read any text there.".into();
        }
        if !self.trustworthy() {
            return format!(
                "I could only half-read that — {} words at {:.0}% confidence. Worth a screenshot instead.",
                self.words,
                self.confidence * 100.0
            );
        }
        format!("Read {} words.", self.words)
    }
}

/// Parse Tesseract's TSV output.
///
/// TSV rather than plain text because it carries per-word confidence, and
/// confidence is the only thing standing between a bad scan and Atlas
/// confidently repeating gibberish.
pub fn parse_tsv(tsv: &str, min_confidence: f32) -> Reading {
    let mut words = Vec::new();
    let mut discarded = 0;

    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        let text = cols[11].trim();
        if text.is_empty() {
            continue;
        }
        // Tesseract reports -1 for structural rows, and 0-100 for words.
        let Ok(conf) = cols[10].trim().parse::<f32>() else { continue };
        if conf < 0.0 {
            continue;
        }
        let c = conf / 100.0;
        let line_no: u32 = cols[4].trim().parse().unwrap_or(0);
        if c < min_confidence {
            discarded += 1;
            continue;
        }
        words.push(Word { text: text.to_string(), confidence: c, line: line_no });
    }

    let confidence = if words.is_empty() {
        0.0
    } else {
        words.iter().map(|w| w.confidence).sum::<f32>() / words.len() as f32
    };

    Reading {
        text: join_lines(&words),
        confidence,
        words: words.len(),
        discarded,
    }
}

/// Rebuild lines, so a paragraph reads as a paragraph rather than a word list.
fn join_lines(words: &[Word]) -> String {
    let mut out = String::new();
    let mut current_line = None;
    for w in words {
        match current_line {
            Some(l) if l == w.line => out.push(' '),
            Some(_) => out.push('\n'),
            None => {}
        }
        out.push_str(&w.text);
        current_line = Some(w.line);
    }
    out
}

/// Arguments for reading an image file. `-` sends the result to stdout.
pub fn args(image: &str, language: &str) -> Vec<String> {
    vec![image.into(), "-".into(), "-l".into(), language.into(), "tsv".into()]
}

pub fn read_image(cfg: &OcrConfig, image: &str, vars: &Vars) -> Result<Reading> {
    let engine = cfg
        .engine
        .as_ref()
        .ok_or_else(|| AtlasError::Config("no OCR engine configured — install tesseract".into()))?;
    let (cmd, mut a) = engine.resolved(vars);
    a.extend(args(image, &cfg.language));

    let out = crate::tools::command(&cmd)
        .args(&a)
        .output()
        .map_err(|e| AtlasError::Platform(format!("could not start {cmd}: {e}")))?;
    if !out.status.success() {
        return Err(AtlasError::Platform(format!(
            "OCR failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(parse_tsv(&String::from_utf8_lossy(&out.stdout), cfg.min_confidence))
}

/// Tidy up what OCR produces. Engines split words oddly and hallucinate
/// punctuation at edges.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let l = line.trim().trim_matches(|c: char| "|_~`".contains(c)).trim();
        if l.is_empty() {
            continue;
        }
        // A line of one or two stray characters is nearly always an artefact
        // of a border or a cursor.
        if l.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(l);
    }
    out
}
