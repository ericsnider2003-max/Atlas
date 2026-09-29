//! A flight recorder for every model call.
//!
//! The hosted tools that do this — Helicone, Phoenix, Langfuse — are proxies
//! built to watch cloud API traffic across a team. Atlas is one process, on
//! one machine, calling a model on the same disk. There is no network hop to
//! proxy and no team to share a dashboard with, so the whole thing is an
//! append-only file and a reader.
//!
//! It earns its place for four reasons, in the order the pain arrives:
//!
//! 1. **You cannot debug what you cannot see.** When an answer is wrong next
//!    month, the question is what Atlas actually sent and with what loaded.
//!    Without a record that is unanswerable.
//! 2. **It closes the correction loop.** `mending`'s scoreboard is "does it
//!    make the same mistake twice", and counting that needs a record of the
//!    first time.
//! 3. **It is how a local model earns trust.** Whether the 7B is good enough
//!    or a question needs the 14B is a measurement, not an opinion.
//! 4. **Without it every prompt change is a vibe.**
//!
//! One line per call, newest last, never rewritten. A log that gets rewritten
//! is a log you cannot trust, and the entire value here is trust.

use serde::{Deserialize, Serialize};

/// One model call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub at: u64,
    /// Which module asked. The most useful field, and the one a proxy cannot
    /// know: a proxy sees traffic, this sees *who wanted it*.
    pub asked_by: String,
    pub model: String,
    /// How long it took, wall clock.
    pub took_ms: u64,
    pub prompt_chars: usize,
    pub reply_chars: usize,
    /// Set when the call failed, with why.
    pub failed: Option<String>,
    /// Filled in later by `grading`, once there is an outcome to attach.
    #[serde(default)]
    pub graded: Option<bool>,
    /// Ties a call to the correction it later caused, so a bad answer can be
    /// traced to the exact request that produced it.
    #[serde(default)]
    pub correction: Option<String>,
    /// Its number in this log, so a grade written later can find it.
    #[serde(default)]
    pub id: u64,
    /// When graded bad: what kind of bad, in a few words. The failure
    /// buckets are these, counted.
    #[serde(default)]
    pub why: Option<String>,
}

impl Call {
    pub fn new(asked_by: &str, model: &str, at: u64) -> Call {
        Call {
            at,
            asked_by: asked_by.into(),
            model: model.into(),
            took_ms: 0,
            prompt_chars: 0,
            reply_chars: 0,
            failed: None,
            graded: None,
            correction: None,
            id: 0,
            why: None,
        }
    }

    pub fn finished(mut self, took_ms: u64, prompt: &str, reply: &str) -> Call {
        self.took_ms = took_ms;
        self.prompt_chars = prompt.len();
        self.reply_chars = reply.len();
        self
    }

    pub fn broke(mut self, why: &str) -> Call {
        self.failed = Some(why.into());
        self
    }

    pub fn ok(&self) -> bool {
        self.failed.is_none()
    }
}

/// Prompts and replies are **not** stored.
///
/// A verbatim log of everything Atlas has ever been asked is the single most
/// sensitive file it could keep, and it would sit unencrypted next to the
/// notes. Lengths, timings and who asked answer every question in the header
/// comment above without holding the content.
///
/// `mending` already keeps the corrections, which is the part worth keeping in
/// words, and keeps them because you said them rather than because they passed
/// through.
pub const STORES_NO_CONTENT: &str =
    "lengths and timings only; the words of a prompt are never written to disk";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trace {
    pub calls: Vec<Call>,
    /// Oldest kept. Below this they are counted and dropped, so the file does
    /// not grow forever on a machine that runs every day.
    #[serde(default)]
    pub dropped: u64,
}

/// Calls kept in full before the oldest start rolling off.
pub const KEEP: usize = 5_000;

impl Trace {
    pub fn record(&mut self, c: Call) {
        self.calls.push(c);
        while self.calls.len() > KEEP {
            self.calls.remove(0);
            self.dropped += 1;
        }
    }

    /// How many calls were ever recorded, rolled off or not: only ever
    /// grows, so "the calls since" is a difference of two of these. Counting
    /// `calls.len()` stopped working once the oldest began rolling off at
    /// `KEEP` (28 Sep 2026): the length stayed at 5,000 and every turn
    /// seemed to have made no model calls.
    pub fn recorded(&self) -> u64 {
        self.dropped + self.calls.len() as u64
    }

    /// The calls recorded since `recorded()` was `before`.
    pub fn recorded_since(&self, before: u64) -> &[Call] {
        let new = self.recorded().saturating_sub(before).min(self.calls.len() as u64) as usize;
        &self.calls[self.calls.len() - new..]
    }

    /// The number the next call gets.
    pub fn next_id(&self) -> u64 {
        self.calls.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }

    /// Grade a call by number. `false` when it has rolled off.
    pub fn grade(&mut self, id: u64, good: bool, why: Option<&str>) -> bool {
        match self.calls.iter_mut().rev().find(|c| c.id == id && id != 0) {
            Some(c) => {
                c.graded = Some(good);
                c.why = if good { None } else { why.map(str::to_string) };
                true
            }
            None => false,
        }
    }

    /// How each kind of call is doing, from its grades: how many were
    /// graded, how many were good, a 95% range for the true rate (Wilson,
    /// so twelve grades don't get read as a percentage to trust), and the
    /// ways the bad ones went wrong, commonest first.
    pub fn scorecard(&self) -> Vec<Score> {
        let mut by: std::collections::BTreeMap<&str, Vec<&Call>> = Default::default();
        for c in self.calls.iter().filter(|c| c.graded.is_some()) {
            by.entry(c.asked_by.as_str()).or_default().push(c);
        }
        by.into_iter()
            .map(|(who, calls)| {
                let graded = calls.len();
                let good = calls.iter().filter(|c| c.graded == Some(true)).count();
                let mut reasons: std::collections::BTreeMap<String, usize> = Default::default();
                for c in calls.iter().filter(|c| c.graded == Some(false)) {
                    *reasons.entry(c.why.clone().unwrap_or_else(|| "no reason given".into())).or_default() += 1;
                }
                let mut reasons: Vec<(String, usize)> = reasons.into_iter().collect();
                reasons.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                let (low, high) = wilson(good, graded);
                Score { asked_by: who.to_string(), graded, good, low, high, reasons }
            })
            .collect()
    }

    /// Attach an outcome to the most recent call from a module.
    pub fn grade_last(&mut self, asked_by: &str, good: bool) -> bool {
        match self.calls.iter_mut().rev().find(|c| c.asked_by == asked_by) {
            Some(c) => {
                c.graded = Some(good);
                true
            }
            None => false,
        }
    }

    /// Tie the most recent call from a module to a correction you made.
    pub fn blame(&mut self, asked_by: &str, correction: &str) -> bool {
        match self.calls.iter_mut().rev().find(|c| c.asked_by == asked_by) {
            Some(c) => {
                c.correction = Some(correction.into());
                true
            }
            None => false,
        }
    }

    pub fn since(&self, t: u64) -> Vec<&Call> {
        self.calls.iter().filter(|c| c.at >= t).collect()
    }

    /// How often calls from here fail.
    pub fn failure_rate(&self, asked_by: &str) -> f32 {
        let mine: Vec<&Call> = self.calls.iter().filter(|c| c.asked_by == asked_by).collect();
        if mine.is_empty() {
            return 0.0;
        }
        mine.iter().filter(|c| !c.ok()).count() as f32 / mine.len() as f32
    }

    /// Median, not mean. One thirty-second stall should not make a fast model
    /// look slow, and with a local model on a laptop that stall happens.
    pub fn typical_ms(&self, model: &str) -> u64 {
        let mut times: Vec<u64> =
            self.calls.iter().filter(|c| c.model == model && c.ok()).map(|c| c.took_ms).collect();
        if times.is_empty() {
            return 0;
        }
        times.sort_unstable();
        times[times.len() / 2]
    }

    /// Which module asks the most. Answers "what is actually using the model",
    /// which is never what you expect.
    pub fn busiest(&self) -> Option<(String, usize)> {
        let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
        for c in &self.calls {
            *counts.entry(c.asked_by.as_str()).or_default() += 1;
        }
        counts.into_iter().max_by_key(|(_, n)| *n).map(|(k, n)| (k.to_string(), n))
    }

    /// Calls that led to you correcting Atlas. The list worth reading.
    pub fn caused_corrections(&self) -> Vec<&Call> {
        self.calls.iter().filter(|c| c.correction.is_some()).collect()
    }

    /// One line, for `atlas trace`.
    pub fn spoken(&self) -> String {
        if self.calls.is_empty() {
            return "I haven't asked the model anything yet.".into();
        }
        let failed = self.calls.iter().filter(|c| !c.ok()).count();
        let mut out = format!("{} calls", self.calls.len());
        if self.dropped > 0 {
            out.push_str(&format!(" kept, {} rolled off", self.dropped));
        }
        if let Some((who, n)) = self.busiest() {
            out.push_str(&format!(", most from {who} ({n})"));
        }
        if failed > 0 {
            out.push_str(&format!(", {failed} failed"));
        }
        let corrections = self.caused_corrections().len();
        if corrections > 0 {
            out.push_str(&format!(", {corrections} you had to correct"));
        }
        out.push('.');
        out
    }
}

/// One line of the log file. Deliberately JSON so it can be read by anything,
/// including you with a text editor.
pub fn to_line(c: &Call) -> String {
    serde_json::to_string(c).unwrap_or_else(|_| String::from("{}"))
}

/// Read a log back. Bad lines are skipped rather than failing the whole read —
/// a truncated final line after a crash must not cost you the other 4,999.
pub fn from_lines(text: &str) -> Trace {
    let mut t = Trace::default();
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        if let Ok(c) = serde_json::from_str::<Call>(l) {
            t.calls.push(c);
        }
    }
    t
}

// ---------------------------------------------------------------------------
// The half that was missing.
//
// `to_line` and `from_lines` describe a file. Nothing ever wrote one, nothing
// ever read one, and no model call was ever recorded — so `spoken()`,
// `failure_rate`, `typical_ms`, `busiest` and `caused_corrections` all
// computed over an empty `Vec` that only a test had ever filled.
//
// The module header lists four reasons this earns its place, and every one of
// them is about having a record. Below is the recording.
// ---------------------------------------------------------------------------

use std::path::{Path, PathBuf};

/// Where the log lives, given the directory that holds it.
///
/// `.jsonl` and not `.json` is the whole design: one call per line means a
/// crash costs you the last line, not the file. It is still meant to be
/// readable with a text editor and handed to someone when an answer went
/// wrong — see `to_line`'s note on why the format is JSON.
pub fn log_path(dir: &Path) -> PathBuf {
    dir.join("model-calls.jsonl")
}

/// Append one call. Never rewrites, never reads first.
///
/// Failing to write must not fail the call it is recording: a flight recorder
/// that can take the aircraft down is worse than no flight recorder. The
/// caller gets a `bool` so it can say so once rather than silently.
pub fn append(path: &Path, c: &Call) -> bool {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) else {
        return false;
    };
    writeln!(f, "{}", to_line(c)).is_ok()
}

/// Read the log back, or an empty trace when there is not one yet.
///
/// No log is the ordinary case on a fresh install and is not a fault.
pub fn load(path: &Path) -> Trace {
    let mut t = match std::fs::read_to_string(path) {
        Ok(text) => from_lines(&text),
        Err(_) => Trace::default(),
    };
    // Grades written since, laid over the calls they're about. The last
    // grade for a call is the one that stands.
    if let Ok(text) = std::fs::read_to_string(grades_path(path)) {
        for l in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(l) else { continue };
            let (Some(id), Some(good)) = (v.get("id").and_then(|x| x.as_u64()), v.get("good").and_then(|x| x.as_bool())) else {
                continue;
            };
            t.grade(id, good, v.get("why").and_then(|x| x.as_str()));
        }
    }
    t
}

/// Rewrite the file keeping only the newest `keep` calls.
///
/// The in-memory `Trace` rolls at `KEEP`; without this the file would not, and
/// a machine that runs every day would carry a log that only grows. Done as a
/// write-then-rename so a crash halfway through leaves the old file intact
/// rather than a half-written one.
pub fn compact(path: &Path, keep: usize) -> std::io::Result<usize> {
    let t = load(path);
    if t.calls.len() <= keep {
        return Ok(t.calls.len());
    }
    let kept: Vec<&Call> = t.calls.iter().skip(t.calls.len() - keep).collect();
    let body: String = kept.iter().map(|c| format!("{}\n", to_line(c))).collect();
    let tmp = path.with_extension("jsonl.new");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    // The grades beside it go with the calls they're about: the grades file
    // is append-only too, and grades of calls no longer kept grade nothing
    // (28 Sep 2026: it only grew).
    let oldest = kept.first().map(|c| c.id).unwrap_or(0);
    let grades = grades_path(path);
    if let Ok(text) = std::fs::read_to_string(&grades) {
        let still: String = text
            .lines()
            .filter(|l| {
                serde_json::from_str::<serde_json::Value>(l)
                    .ok()
                    .and_then(|v| v.get("id").and_then(|x| x.as_u64()))
                    .is_some_and(|id| id >= oldest)
            })
            .map(|l| format!("{l}\n"))
            .collect();
        if still.len() < text.len() {
            let tmp = grades.with_extension("jsonl.new");
            if std::fs::write(&tmp, still).is_ok() {
                let _ = std::fs::rename(&tmp, &grades);
            }
        }
    }
    Ok(kept.len())
}

/// The log as the running Atlas opens it: cut back to the newest `KEEP` on
/// disk first when it has grown past twice that.
///
/// `compact` had exactly one caller, `atlas trace compact`, which nobody
/// running Atlas in the background ever types. So `model-calls.jsonl` grew by
/// a line for every model call for as long as Atlas was installed, and every
/// start read all of it into memory (28 Sep 2026).
pub fn open(path: &Path) -> Trace {
    let t = load(path);
    if t.calls.len() > 2 * KEEP && compact(path, KEEP).is_ok() {
        return load(path);
    }
    t
}

/// After a call is recorded: each time another `KEEP` calls have rolled off
/// in memory, the file is cut back too, so a daemon that runs for weeks
/// without a restart keeps it between `KEEP` and twice that. Returns whether
/// it was cut.
pub fn keep_bounded(path: &Path, t: &Trace) -> bool {
    t.dropped > 0 && t.dropped % KEEP as u64 == 0 && compact(path, KEEP).is_ok()
}

/// The model's name, taken from the request body it is actually sent in.
///
/// There is no `name` field on `LlmConfig` and adding one would let the config
/// disagree with what is on the wire — the exact two-declarations-of-one-fact
/// problem this codebase removed from `settings.rs`. The name lives in the
/// JSON body (`{"model":"llama3.1", ...}`), so that is where it is read from.
///
/// `unnamed` when the body does not carry one, which is honest: some backends
/// take the model from the URL or the binary, and inventing a label would make
/// `typical_ms` compare two different models under one name.
pub fn model_name(request: &str) -> String {
    let Some(at) = request.find("\"model\"") else {
        return "unnamed".into();
    };
    let rest = &request[at + "\"model\"".len()..];
    let Some(colon) = rest.find(':') else { return "unnamed".into() };
    let after = rest[colon + 1..].trim_start();
    // The value may be a placeholder like {model} rather than a literal.
    let Some(stripped) = after.strip_prefix('"') else { return "unnamed".into() };
    match stripped.find('"') {
        Some(end) if end > 0 => stripped[..end].to_string(),
        _ => "unnamed".into(),
    }
}

/// One kind of call's grades.
#[derive(Debug, Clone, PartialEq)]
pub struct Score {
    pub asked_by: String,
    pub graded: usize,
    pub good: usize,
    /// The 95% range the true good-rate is in, given how few grades there are.
    pub low: f32,
    pub high: f32,
    pub reasons: Vec<(String, usize)>,
}

/// Fewer than this many grades and a rate isn't worth saying as a number.
pub const ENOUGH_TO_MEASURE: usize = 30;

impl Score {
    pub fn said(&self) -> String {
        let mut s = if self.graded < ENOUGH_TO_MEASURE {
            format!("{}: {} of {} good so far — too few to call a rate", self.asked_by, self.good, self.graded)
        } else {
            format!(
                "{}: {:.0}% good ({:.0}–{:.0}%, {} graded)",
                self.asked_by,
                100.0 * self.good as f32 / self.graded as f32,
                100.0 * self.low,
                100.0 * self.high,
                self.graded
            )
        };
        if !self.reasons.is_empty() {
            let top: Vec<String> = self.reasons.iter().take(3).map(|(r, n)| format!("{r} ({n})")).collect();
            s.push_str(&format!("; went wrong by: {}", top.join(", ")));
        }
        s
    }
}

/// The Wilson score interval for `good` out of `n` at 95%.
pub fn wilson(good: usize, n: usize) -> (f32, f32) {
    if n == 0 {
        return (0.0, 1.0);
    }
    let z = 1.96f64;
    let n_f = n as f64;
    let p = good as f64 / n_f;
    let denom = 1.0 + z * z / n_f;
    let centre = (p + z * z / (2.0 * n_f)) / denom;
    let half = z * ((p * (1.0 - p) / n_f) + z * z / (4.0 * n_f * n_f)).sqrt() / denom;
    (((centre - half).max(0.0)) as f32, ((centre + half).min(1.0)) as f32)
}

/// Where grades are written: beside the log, one line each, never rewritten.
/// Grades arrive after the call (a correction a minute later, a rewrite, a
/// figure found missing), and the log itself is never rewritten, so they
/// live in their own file and are laid over the calls on loading.
pub fn grades_path(log: &Path) -> PathBuf {
    log.with_file_name("model-grades.jsonl")
}

/// Write a grade down, and apply it to the calls in memory.
pub fn grade_and_keep(log: &Path, t: &mut Trace, id: u64, good: bool, why: Option<&str>) -> bool {
    use std::io::Write;
    t.grade(id, good, why);
    let line = serde_json::json!({ "id": id, "good": good, "why": why }).to_string();
    let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(grades_path(log)) else {
        return false;
    };
    writeln!(f, "{line}").is_ok()
}

// ------------------------------------------------------------------ examples
//
// Eric, 25 Sep 2026: "If the prompts add value yes." The call log above keeps
// no words, and still doesn't. What's kept here is narrower: the words of a
// call only when that call was *graded* — good, or bad and why — because a
// graded call is a labelled example (something to test a new model against,
// and one day to tune one with) and an ungraded prompt adds nothing the
// lengths don't already say. Only kinds whose grade comes from something that
// actually happened: a window reply (read back, rewritten, kept or not), a
// council seat (committed, or not), and a turn you corrected. Research isn't
// kept: its prompt is the pages it read, tens of thousands of characters a
// time, and its one grade (a figure not in its sources) is checked without
// the words. Emails, phone numbers, long numbers, web-address queries and
// anything after "password" are taken out before a line is written; names
// and addresses are not, and can't reliably be.

/// Keeping graded examples.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TraceConfig {
    /// Keep the words of graded calls (scrubbed) in `model-examples.jsonl`.
    pub keep_examples: bool,
    /// The most kept; the oldest go first.
    pub examples_max: usize,
}

impl Default for TraceConfig {
    fn default() -> Self {
        TraceConfig { keep_examples: true, examples_max: 2_000 }
    }
}

/// The words of one call, carried back from an errand so they can be kept if
/// the call is graded. Never written anywhere on their own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Words {
    pub system: String,
    pub user: String,
    pub reply: String,
}

/// A graded call with its words, scrubbed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Example {
    pub id: u64,
    pub at: u64,
    pub asked_by: String,
    pub good: bool,
    #[serde(default)]
    pub why: Option<String>,
    pub system: String,
    pub user: String,
    pub reply: String,
}

/// Where examples are kept: beside the call log.
pub fn examples_path(log: &Path) -> PathBuf {
    log.with_file_name("model-examples.jsonl")
}

/// Keep one example, dropping the oldest past `max`.
pub fn keep_example(log: &Path, ex: &Example, max: usize) -> bool {
    use std::io::Write;
    let path = examples_path(log);
    let Ok(line) = serde_json::to_string(ex) else { return false };
    let ok = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| writeln!(f, "{line}"))
        .is_ok();
    if ok {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        if lines.len() > max.max(1) {
            let kept = lines[lines.len() - max.max(1)..].join("\n") + "\n";
            let _ = std::fs::write(&path, kept);
        }
    }
    ok
}

/// Every kept example, oldest first.
pub fn examples(log: &Path) -> Vec<Example> {
    std::fs::read_to_string(examples_path(log))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Personal details that can be found by their shape, taken out.
pub fn scrub(text: &str) -> String {
    let digits = |t: &str| t.chars().filter(|c| c.is_ascii_digit()).count();
    // A run of tokens that is only digits and phone punctuation.
    let numberish =
        |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || "+-().".contains(c)) && digits(t) > 0;
    let mut out: Vec<String> = Vec::new();
    let mut after_secret_word = false;
    let lines: Vec<&str> = text.split('\n').collect();
    let mut result_lines = Vec::new();
    for line in lines {
        out.clear();
        let toks: Vec<&str> = line.split(' ').collect();
        let mut i = 0;
        while i < toks.len() {
            let t = toks[i];
            let bare = t.trim_matches(|c: char| ",;:!?\"'<>[]".contains(c));
            if after_secret_word && ["is", "was", "=", "-", "it's"].contains(&bare.to_lowercase().as_str()) {
                out.push(t.to_string());
                i += 1;
                continue;
            }
            if after_secret_word && !bare.is_empty() {
                out.push("[secret]".into());
                after_secret_word = false;
                i += 1;
                continue;
            }
            let low = bare.to_lowercase();
            if ["password", "passcode", "passphrase", "pin", "password:", "pin:"].contains(&low.trim_end_matches(':')) {
                out.push(t.to_string());
                after_secret_word = true;
                i += 1;
                continue;
            }
            // Emails.
            if let Some(at) = bare.find('@') {
                if at > 0 && bare[at + 1..].contains('.') {
                    out.push("[email]".into());
                    i += 1;
                    continue;
                }
            }
            // A web address's query string often carries a token.
            if (bare.starts_with("http://") || bare.starts_with("https://")) && bare.contains('?') {
                let base = &bare[..bare.find('?').unwrap_or(bare.len())];
                out.push(format!("{base}?[…]"));
                i += 1;
                continue;
            }
            // Phone numbers and account numbers, possibly spread over tokens.
            if numberish(bare) {
                let mut j = i;
                let mut n = 0;
                while j < toks.len() {
                    let b = toks[j].trim_matches(|c: char| ",;:!?\"'".contains(c));
                    if !numberish(b) {
                        break;
                    }
                    n += digits(b);
                    j += 1;
                }
                if n >= 7 {
                    out.push("[number]".into());
                    i = j;
                    continue;
                }
            }
            // A long code: letters and digits mixed, the shape of a key.
            if bare.len() >= 24
                && bare.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && bare.chars().any(|c| c.is_ascii_digit())
                && bare.chars().any(|c| c.is_ascii_alphabetic())
            {
                out.push("[key]".into());
                i += 1;
                continue;
            }
            out.push(t.to_string());
            i += 1;
        }
        result_lines.push(out.join(" "));
    }
    result_lines.join("\n")
}

/// A graded call and its words as an example, scrubbed — `None` when the
/// call isn't graded or isn't found.
pub fn example_of(t: &Trace, id: u64, words: &Words) -> Option<Example> {
    let c = t.calls.iter().rev().find(|c| c.id == id && id != 0)?;
    let good = c.graded?;
    Some(Example {
        id,
        at: c.at,
        asked_by: c.asked_by.clone(),
        good,
        why: c.why.clone(),
        system: scrub(&words.system),
        user: scrub(&words.user),
        reply: scrub(&words.reply),
    })
}
