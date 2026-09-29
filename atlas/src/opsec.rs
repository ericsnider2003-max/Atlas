//! Checking content before it goes out, while the rules apply to you.
//!
//! Two things this has to get right. The checks have to be about **what's
//! visible in the frame**, not about what you're allowed to think — Atlas is
//! looking for a patch and a tail number, not vetting your opinions. And it
//! has to know there is a date after which none of this is its business.
//!
//! That second part matters more than it sounds. A system that keeps applying
//! rules you're no longer under is a system you start ignoring, and then it's
//! useless on the day it's right.

use serde::{Deserialize, Serialize};

/// What might be in the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Rank, unit, name tape, anything worn.
    Insignia,
    /// A building, gate, sign, or a view that places you.
    Location,
    /// Tail numbers, bumper numbers, hull numbers.
    Markings,
    /// A badge, CAC, or anything with a number on it.
    Credential,
    /// Screens, whiteboards, paperwork in shot.
    Documents,
    /// When you're leaving, where you're going, how long.
    Movement,
    /// Names or faces of other people.
    Others,
    /// Coordinates in the file itself.
    Metadata,
    /// Uniform plus something the uniform shouldn't be next to.
    UniformContext,
}

impl Risk {
    pub fn what(&self) -> &'static str {
        match self {
            Risk::Insignia => "rank, unit or name tape readable",
            Risk::Location => "something that places where you are",
            Risk::Markings => "vehicle or aircraft markings",
            Risk::Credential => "a badge or ID in shot",
            Risk::Documents => "a screen, whiteboard or paperwork readable",
            Risk::Movement => "when you're moving, or where to",
            Risk::Others => "someone else identifiable",
            Risk::Metadata => "coordinates in the file",
            Risk::UniformContext => "uniform alongside something it shouldn't be",
        }
    }

    /// Can it be fixed in the edit, or does it need a reshoot?
    pub fn fixable_in_edit(&self) -> bool {
        matches!(
            self,
            Risk::Insignia | Risk::Markings | Risk::Credential | Risk::Documents
                | Risk::Metadata | Risk::Others
        )
    }

    pub fn fix(&self) -> &'static str {
        match self {
            Risk::Insignia => "blur it, or reframe above the chest",
            Risk::Location => "this one needs a different backdrop — a blur still leaves the shape",
            Risk::Markings => "blur the numbers",
            Risk::Credential => "cut the frame or blur it",
            Risk::Documents => "blur, or cut those seconds",
            Risk::Movement => "say it after, not before — the timing is the whole problem",
            Risk::Others => "blur the face, or ask them",
            Risk::Metadata => "strip it on export, which I do by default",
            Risk::UniformContext => "either the uniform comes out or the content does",
        }
    }
}

/// Something spotted, with where.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spotted {
    pub risk: Risk,
    /// Where in the video, in seconds. None for the whole thing.
    pub at: Option<f32>,
    /// What was actually seen or heard.
    pub detail: String,
    /// Higher means say it first.
    pub weight: f32,
}

/// Phrases that say when you're moving. The timing is the leak, not the fact.
/// Specific enough to rarely appear in ordinary personal speech — checked
/// regardless of context, the same as `UNIFORM_TROUBLE` always is.
const MOVEMENT_UNAMBIGUOUS: &[&str] =
    &["deployment", "shipping out", "wheels up", "port call", "tdy"];

/// Common enough in ordinary life that alone they say nothing — "my last
/// day" and "next week i" are how anyone talks about a new job or a
/// holiday. Only worth flagging alongside something that actually places
/// this in the affiliation, not on their own.
const MOVEMENT_AMBIGUOUS: &[&str] = &[
    "deploy", "deploying", "i leave", "i'm leaving", "im leaving", "flying out",
    "next week i", "in two weeks", "back in", "my last day", "rotation", "pcs",
];

/// Places names that identify almost nothing else.
const PLACES_UNAMBIGUOUS: &[&str] =
    &["naval station", "air force base", "afb", "barracks", "flight line", "motor pool"];

/// "Base", "post" and "range" are three of the most overloaded words in
/// ordinary English — a baseball base, a blog post, a shooting range or a
/// product range. Flagging these alone would mean opsec firing on
/// completely personal writing that happens to use a common word.
const PLACES_AMBIGUOUS: &[&str] = &["base", "post", "fort ", "camp ", "the gate", "hangar", "range"];

/// Uniform next to things it shouldn't be next to.
const UNIFORM_TROUBLE: &[&str] = &[
    "sponsored", "use my code", "affiliate", "link in bio", "buy now",
    "discount", "brand deal", "partnership", "vote", "campaign", "candidate",
    "endorse",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OpsecConfig {
    pub enabled: bool,
    /// The date it stops applying, as a plain year-month-day.
    ///
    /// After this Atlas says nothing about any of it, without being asked
    /// twice.
    pub applies_until: String,
    /// You're in uniform in this one.
    pub in_uniform: bool,
    /// Strip location data from every export regardless.
    pub always_strip_metadata: bool,
    /// Words of your own that aren't leaks.
    pub not_a_leak: Vec<String>,
}

impl Default for OpsecConfig {
    fn default() -> Self {
        OpsecConfig {
            enabled: false,
            applies_until: String::new(),
            in_uniform: false,
            // Cheap, invisible, and the one that catches people who did
            // everything else right.
            always_strip_metadata: true,
            not_a_leak: Vec::new(),
        }
    }
}

impl OpsecConfig {
    /// Are you still under this?
    ///
    /// Dates compared as text, which works for year-month-day and needs no
    /// date library.
    pub fn still_applies(&self, today: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if self.applies_until.trim().is_empty() {
            return true;
        }
        today.as_bytes() <= self.applies_until.as_bytes()
    }
}

/// Check a piece.
pub fn check(
    transcript: &str,
    visible: &[(Risk, f32, String)],
    cfg: &OpsecConfig,
    today: &str,
) -> Vec<Spotted> {
    let mut out = Vec::new();
    if !cfg.still_applies(today) {
        return out;
    }

    let t = transcript.to_lowercase();
    let mine: Vec<String> = cfg.not_a_leak.iter().map(|w| w.to_lowercase()).collect();
    let is_mine = |w: &str| mine.iter().any(|m| m == w);

    // Something unambiguous already found says this piece is actually about
    // the affiliation, not just a personal sentence that happens to share a
    // word with one. Computed once, from the unambiguous lists only, so an
    // ambiguous term can never corroborate another ambiguous term.
    let corroborated = cfg.in_uniform
        || MOVEMENT_UNAMBIGUOUS.iter().any(|m| t.contains(m) && !is_mine(m))
        || PLACES_UNAMBIGUOUS.iter().any(|p| t.contains(p) && !is_mine(p));

    // Movement is the one that matters most and the one people say without
    // thinking, because it feels like ordinary conversation.
    for m in MOVEMENT_UNAMBIGUOUS {
        if t.contains(m) && !is_mine(m) {
            out.push(Spotted {
                risk: Risk::Movement,
                at: None,
                detail: format!("\"{m}\" — when you're moving is the thing worth not saying"),
                weight: 1.0,
            });
            break;
        }
    }
    if corroborated {
        for m in MOVEMENT_AMBIGUOUS {
            if t.contains(m) && !is_mine(m) {
                out.push(Spotted {
                    risk: Risk::Movement,
                    at: None,
                    detail: format!(
                        "\"{m}\" — ordinary on its own, but alongside the rest of this it reads \
                         as when you're moving"
                    ),
                    weight: 0.7,
                });
                break;
            }
        }
    }

    for p in PLACES_UNAMBIGUOUS {
        if t.contains(p) && !is_mine(p) {
            out.push(Spotted {
                risk: Risk::Location,
                at: None,
                detail: format!("\"{}\" places you", p.trim()),
                weight: 0.8,
            });
            break;
        }
    }
    if corroborated {
        for p in PLACES_AMBIGUOUS {
            if t.contains(p) && !is_mine(p) {
                out.push(Spotted {
                    risk: Risk::Location,
                    at: None,
                    detail: format!(
                        "\"{}\" — a word with a dozen ordinary meanings, but this reads like the \
                         military one",
                        p.trim()
                    ),
                    weight: 0.5,
                });
                break;
            }
        }
    }

    if cfg.in_uniform {
        for u in UNIFORM_TROUBLE {
            if t.contains(u) {
                out.push(Spotted {
                    risk: Risk::UniformContext,
                    at: None,
                    detail: format!("\"{u}\" while you're in uniform"),
                    weight: 0.95,
                });
                break;
            }
        }
    }

    // Whatever was actually seen in the frame.
    for (risk, at, detail) in visible {
        out.push(Spotted {
            risk: *risk,
            at: Some(*at),
            detail: detail.clone(),
            weight: match risk {
                Risk::Credential | Risk::Documents => 0.95,
                Risk::Insignia | Risk::Markings => 0.85,
                Risk::Location => 0.8,
                Risk::Others => 0.6,
                _ => 0.5,
            },
        });
    }

    out.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// What Atlas says. The one that would actually cost you, first.
pub fn spoken(found: &[Spotted], cfg: &OpsecConfig, today: &str) -> String {
    if !cfg.still_applies(today) {
        return String::new();
    }
    match found.split_first() {
        None => String::new(),
        Some((first, rest)) => {
            let where_ = first
                .at
                .map(|s| format!(" at {:.0}s", s))
                .unwrap_or_default();
            let mut s = format!("{}{where_} — {}. {}", first.risk.what(), first.detail, first.risk.fix());
            if !rest.is_empty() {
                s.push_str(&format!(" {} other thing{}.", rest.len(), if rest.len() == 1 { "" } else { "s" }));
            }
            let reshoot = found.iter().filter(|f| !f.risk.fixable_in_edit()).count();
            if reshoot > 0 {
                s.push_str(" Some of that can't be fixed in the edit.");
            }
            // And what was NOT checked, said in the same breath.
            //
            // Everything above comes from the transcript. A warning about a
            // word, delivered on its own, reads as "I looked and this is what
            // I found" — and the picture is the half this module is named
            // for. See `frame_unchecked`.
            if let Some(why) = frame_unchecked() {
                s.push(' ');
                s.push_str(why);
            }
            s
        }
    }
}

// The date arriving, and a countdown to it, were two lines Atlas could say.
// Eric, 25 Sep 2026 (F5): "I don't want Atlas to remind me of the end date."
// Both were removed with the day count behind them. After the date, checking
// simply stops (`still_applies`), and nothing is said about it.

/// What Atlas will never do here, because it isn't its job.
///
/// ## This said the opposite of what happens
///
/// It read: *"I check what's in the frame, not what you're saying."*
///
/// The frame half is the `visible` parameter of `check`, and **nothing in
/// `src/` produces a `Risk`** — the only call site passes `&[]`, so the loop
/// under the comment "Whatever was actually seen in the frame" has never run.
/// What does run is the transcript scan: the words. So the one sentence that
/// tells a person which half of this Atlas looks at named the half it does
/// not look at, and disclaimed the half it does.
///
/// It also had no caller, which is how it stayed wrong — a promise nobody
/// reads is a promise nothing checks.
///
/// `vision::Scene` exists and detects objects and faces, so the frame half is
/// buildable; what it needs is a decision about which detector labels amount
/// to `Risk::Insignia` or `Risk::Documents`, and that is a ruling rather than
/// a wiring job. `frame_unchecked` below is what says so until then, and
/// `tests/it_says_which_half_it_looks_at.rs` fails if the note and the
/// wiring ever disagree.
pub const NOT_ITS_BUSINESS: &str =
    "I read what you said, for the things people say without thinking — a date you're \
     moving, a unit, a base by name. What you think about anything is yours, and I'm not \
     reading your posts for opinions.";

/// Why the frame is not being checked, while it is not.
///
/// `None` the moment something hands `check` a non-empty `visible` list.
/// Until then this is the honest answer to "did you look at the picture", and
/// `spoken` says it alongside anything it found so that a transcript-only
/// warning is not mistaken for a clean frame.
pub fn frame_unchecked() -> Option<&'static str> {
    Some(
        "I haven't looked at the picture itself — rank on a collar, a gate sign, a tail \
         number, a screen in the background. Nothing feeds me the frame yet, so that half \
         is on you.",
    )
}
