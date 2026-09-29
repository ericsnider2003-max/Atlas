//! Reading, converting and joining whatever you point at.
//!
//! "Atlas needs to handle every file type" is really three jobs: get the text
//! and structure out of anything, turn one thing into another, and join or
//! split. All three are ffmpeg, a PDF library, an unzipper and OCR — nothing
//! exotic, all local, all free.
//!
//! The one that matters most in practice is the scan: a photo of a page is the
//! most common way a document arrives now, and it's the format that's least
//! useful until something reads it.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// Plain text, markdown, code, csv.
    Text,
    /// Word, Pages, ODT.
    Document,
    /// Excel, Numbers, ODS.
    Sheet,
    Pdf,
    /// PNG, JPEG, HEIC.
    Picture,
    /// A photo or scan of a page. A picture, but wanted as words.
    Scan,
    Audio,
    Video,
    /// Zip, tar, 7z, rar.
    Archive,
    Email,
    /// Calendar, contacts, structured exports.
    Data,
    Unknown,
}

impl Sort {
    pub fn of(filename: &str) -> Sort {
        let f = filename.to_lowercase();
        let ext = f.rsplit('.').next().unwrap_or("");
        match ext {
            "txt" | "md" | "csv" | "tsv" | "json" | "yaml" | "yml" | "rs" | "py" | "js" | "log" => Sort::Text,
            "docx" | "doc" | "odt" | "rtf" | "pages" => Sort::Document,
            "xlsx" | "xls" | "ods" | "numbers" => Sort::Sheet,
            "pdf" => Sort::Pdf,
            "png" | "jpg" | "jpeg" | "heic" | "webp" | "gif" | "tiff" | "bmp" => Sort::Picture,
            "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" => Sort::Audio,
            "mp4" | "mov" | "mkv" | "avi" | "webm" => Sort::Video,
            "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" => Sort::Archive,
            "eml" | "msg" | "mbox" => Sort::Email,
            "ics" | "vcf" | "xml" | "sqlite" | "db" => Sort::Data,
            _ => Sort::Unknown,
        }
    }

    /// What it takes to get the content out.
    pub fn needs(&self) -> &'static str {
        match self {
            Sort::Text | Sort::Data => "nothing",
            Sort::Document | Sort::Sheet => "a reader for the format",
            Sort::Pdf => "a PDF library, and OCR if it's a scan rather than text",
            Sort::Picture => "nothing, unless you want the words in it",
            Sort::Scan => "OCR",
            Sort::Audio | Sort::Video => "whisper for the words, ffmpeg for the rest",
            Sort::Archive => "an unzipper, then whatever's inside",
            Sort::Email => "nothing",
            Sort::Unknown => "a look at the first few bytes",
        }
    }

    /// Can Atlas get words out of it offline?
    pub fn readable_offline(&self) -> bool {
        !matches!(self, Sort::Unknown)
    }

    /// What it can turn into.
    fn converts_to(&self) -> &'static [Sort] {
        match self {
            Sort::Text => &[Sort::Document, Sort::Pdf],
            Sort::Document => &[Sort::Pdf, Sort::Text],
            Sort::Sheet => &[Sort::Text, Sort::Pdf],
            Sort::Pdf => &[Sort::Text, Sort::Picture],
            Sort::Picture | Sort::Scan => &[Sort::Text, Sort::Pdf, Sort::Document],
            Sort::Audio => &[Sort::Text],
            Sort::Video => &[Sort::Audio, Sort::Text],
            Sort::Archive => &[],
            Sort::Email => &[Sort::Text, Sort::Pdf],
            Sort::Data => &[Sort::Text],
            Sort::Unknown => &[],
        }
    }
}

/// A PDF that's a photo of pages rather than text is the common trap: it looks
/// like a document and behaves like a picture.
pub fn pdf_is_really_a_scan(text_chars: usize, pages: usize) -> bool {
    pages > 0 && text_chars / pages < 50
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FilesConfig {
    pub enabled: bool,
    // `look_inside_archives` was here, shipped `true`: "read inside archives
    // rather than just listing them". Deleted 19 Sep 2026, because nothing in
    // this tree opens an archive. `index::AssetClass` classifies a `.zip` and
    // stops there; `safe_to_unpack` below is the guard for an unpacker that
    // does not exist yet and has no caller.
    //
    // A switch shipped on, for a behaviour nothing performs, reads as a
    // behaviour you have — and the belief that Atlas has looked inside your
    // archives is exactly the belief that makes you stop checking. The same
    // shape as `cloudsync.encrypt_before_writing`, deleted a day earlier for
    // the same reason.
    //
    // It comes back the day something unpacks. `config::NO_FIELD_TO_LAND_IN`
    // names it meanwhile, so a file still setting it is told.
    /// Nested archives deeper than this are left alone — a zip of zips of
    /// zips is either a mistake or an attack.
    pub max_archive_depth: u32,
    /// Don't unpack anything that would expand beyond this.
    pub max_unpacked_mb: u64,
    /// Straighten and clean up a photographed page before reading it.
    pub clean_up_scans: bool,
    /// The virus scanner run on a file before Atlas opens it, and on
    /// everything a zip unpacks to (Eric, H3). Windows Defender by default.
    pub virus_scan: crate::unpack::ScanConfig,
}

impl Default for FilesConfig {
    fn default() -> Self {
        FilesConfig {
            enabled: true,
            max_archive_depth: 3,
            // A zip bomb is a small file that becomes a full disk.
            max_unpacked_mb: 2000,
            clean_up_scans: true,
            virus_scan: crate::unpack::ScanConfig::default(),
        }
    }
}

/// Turning one thing into another.
#[derive(Debug, Clone, PartialEq)]
pub enum Convert {
    Can { how: String, loses: Option<String> },
    /// Possible, but you'd lose something worth knowing about.
    CanWithLoss { how: String, loses: String },
    Cannot(String),
}

pub fn convert(from: Sort, to: Sort) -> Convert {
    if from == to {
        return Convert::Cannot("it already is that".into());
    }
    if !from.converts_to().contains(&to) {
        return Convert::Cannot(format!("{from:?} doesn't become {to:?} in any useful way"));
    }
    match (from, to) {
        (Sort::Pdf, Sort::Text) => Convert::CanWithLoss {
            how: "pull the text out".into(),
            loses: "layout, tables and anything that was a picture".into(),
        },
        (Sort::Document, Sort::Text) => Convert::CanWithLoss {
            how: "strip to plain text".into(),
            loses: "formatting, images and comments".into(),
        },
        (Sort::Sheet, Sort::Text) => Convert::CanWithLoss {
            how: "export as CSV".into(),
            loses: "formulas, formatting and every sheet but the first".into(),
        },
        (Sort::Scan, Sort::Text) | (Sort::Picture, Sort::Text) => Convert::CanWithLoss {
            how: "read the words off it".into(),
            loses: "anything handwritten badly, and the layout".into(),
        },
        (Sort::Video, Sort::Audio) => Convert::Can {
            how: "take the sound out".into(),
            loses: None,
        },
        _ => Convert::Can { how: format!("{from:?} to {to:?}"), loses: None },
    }
}

/// Joining things.
#[derive(Debug, Clone, PartialEq)]
pub enum Join {
    Can(String),
    /// They aren't the same kind of thing.
    Mixed { kinds: Vec<Sort>, suggestion: String },
    Cannot(String),
}

pub fn join(files: &[String]) -> Join {
    if files.len() < 2 {
        return Join::Cannot("that's one file".into());
    }
    let mut kinds: Vec<Sort> = files.iter().map(|f| Sort::of(f)).collect();
    kinds.sort_by_key(|k| format!("{k:?}"));
    kinds.dedup();

    if kinds.len() == 1 {
        return match kinds[0] {
            Sort::Pdf => Join::Can("one PDF, in the order you gave them".into()),
            Sort::Picture => Join::Can("one PDF, a page each".into()),
            Sort::Text => Join::Can("one file, with a line between each".into()),
            Sort::Audio => Join::Can("end to end, levels matched".into()),
            Sort::Video => Join::Can("end to end — same size and frame rate or it re-encodes".into()),
            Sort::Sheet => Join::Can("one workbook, a sheet each".into()),
            _ => Join::Cannot(format!("{:?} files don't join into anything useful", kinds[0])),
        };
    }

    // The common mixed case is worth handling rather than refusing: photos and
    // PDFs together is how a set of documents actually arrives.
    let all_paperish = kinds
        .iter()
        .all(|k| matches!(k, Sort::Pdf | Sort::Picture | Sort::Scan | Sort::Document));
    if all_paperish {
        return Join::Mixed {
            kinds,
            suggestion: "I'd make them all pages of one PDF — that's usually what's wanted".into(),
        };
    }
    Join::Mixed {
        kinds,
        suggestion: "those aren't the same sort of thing. Which did you want them to become?".into(),
    }
}

// ---------- the scan, which is the one that matters ----------

/// What was photographed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scanned {
    /// A page of text.
    Page,
    /// A receipt.
    Receipt,
    /// A form with fields.
    Form,
    /// A whiteboard.
    Whiteboard,
    /// A screen.
    Screen,
    /// A card — business card, ID.
    Card,
    Unclear,
}

impl Scanned {
    /// What you almost certainly want out of it.
    pub fn probably_wanted(&self) -> &'static str {
        match self {
            Scanned::Page => "the text, as a document",
            Scanned::Receipt => "the total, the date and the merchant, into your ledger",
            Scanned::Form => "the fields and what's in them",
            Scanned::Whiteboard => "the text, straightened and cleaned up",
            Scanned::Screen => "the text — and there's usually a better way to get it",
            Scanned::Card => "the name and details, as a contact",
            Scanned::Unclear => "the text",
        }
    }

    /// Worth cleaning up before reading?
    fn needs_straightening(&self) -> bool {
        matches!(self, Scanned::Page | Scanned::Receipt | Scanned::Form | Scanned::Whiteboard)
    }
}

/// The steps for a photographed page.
///
/// The order matters: straightening before reading is most of the accuracy,
/// and people skip it because the photo looks fine to a human eye.
pub fn scan_steps(what: Scanned, cfg: &FilesConfig) -> Vec<&'static str> {
    let mut steps = Vec::new();
    if cfg.clean_up_scans && what.needs_straightening() {
        steps.push("find the edges of the page and straighten it");
        steps.push("flatten the lighting so one side isn't darker");
        steps.push("raise the contrast between ink and paper");
    }
    steps.push("read the words");
    match what {
        Scanned::Receipt => steps.push("pull out the total, the date and who it was"),
        Scanned::Form => steps.push("pair each label with its value"),
        Scanned::Card => steps.push("pull out the name, number and email"),
        _ => steps.push("keep the paragraphs and headings as they were"),
    }
    steps
}

/// What a photographed thing is, from the words read off it. A guess, and
/// `after_scan` says it as one so a wrong guess is caught.
pub fn what_was_scanned(text: &str) -> Scanned {
    let t = text.to_lowercase();
    let words = t.split_whitespace().count();
    if words < 5 {
        return Scanned::Unclear;
    }
    let money = t.matches(|c| c == '$' || c == '£' || c == '€').count();
    if (t.contains("total") || t.contains("subtotal")) && (money > 0 || t.contains("tax") || t.contains("change")) {
        return Scanned::Receipt;
    }
    if t.contains('@') && words < 40 && t.chars().filter(|c| c.is_ascii_digit()).count() >= 7 {
        return Scanned::Card;
    }
    let labelled = text.lines().filter(|l| {
        let l = l.trim();
        l.ends_with(':') || l.contains(": ") && l.len() < 60 || l.contains("____")
    }).count();
    if labelled >= 4 {
        return Scanned::Form;
    }
    Scanned::Page
}

/// A file path in what was said: quoted, or a word ending in one of `exts`.
pub fn path_in(said: &str, exts: &[&str]) -> Option<String> {
    for q in ['"', '\'', '“'] {
        let close = if q == '“' { '”' } else { q };
        if let Some(a) = said.find(q) {
            if let Some(b) = said[a + q.len_utf8()..].find(close) {
                let p = &said[a + q.len_utf8()..a + q.len_utf8() + b];
                if p.len() > 2 {
                    return Some(p.to_string());
                }
            }
        }
    }
    said.split_whitespace()
        .map(|w| w.trim_matches(|c: char| c == ',' || c == '?' || c == '!' || c == ':'))
        .map(|w| w.trim_end_matches('.'))
        .find(|w| {
            let l = w.to_lowercase();
            exts.iter().any(|e| l.ends_with(&format!(".{e}")) && l.len() > e.len() + 1)
        })
        .map(String::from)
}

/// What Atlas offers after a scan.
///
/// It says what it thinks the thing is, so a wrong guess is caught before it
/// files a receipt as a page of notes.
pub fn after_scan(what: Scanned, words: usize) -> String {
    if words < 5 {
        return "I couldn't get much off that — try again with more light, or flatter."
            .to_string();
    }
    format!(
        "Looks like {}. I'd get you {}. Want it as a document, a PDF, or just the text?",
        match what {
            Scanned::Page => "a page",
            Scanned::Receipt => "a receipt",
            Scanned::Form => "a form",
            Scanned::Whiteboard => "a whiteboard",
            Scanned::Screen => "a screen",
            Scanned::Card => "a card",
            Scanned::Unclear => "text of some kind",
        },
        what.probably_wanted()
    )
}

/// Things worth refusing to unpack.
pub fn safe_to_unpack(compressed_mb: u64, claims_uncompressed_mb: u64, depth: u32, cfg: &FilesConfig) -> Result<(), String> {
    if depth > cfg.max_archive_depth {
        return Err(format!(
            "that's {depth} archives deep — either a mistake or something trying to be clever"
        ));
    }
    if claims_uncompressed_mb > cfg.max_unpacked_mb {
        return Err(format!(
            "{compressed_mb}MB that becomes {claims_uncompressed_mb}MB. I'll list what's in it \
             rather than unpacking it"
        ));
    }
    Ok(())
}
