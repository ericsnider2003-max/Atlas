//! What Atlas may fix on its own.
//!
//! Asking before every change sounds safe and isn't — a system that needs
//! permission to fix a typo in a phrase list will never fix one, and you end
//! up with the recommendations piling up unread. That's the failure mode of
//! "always ask": it doesn't make anything safer, it makes everything stop.
//!
//! So permission is granted by **what a change touches**, once, rather than by
//! confidence in each change. Confidence is self-assessed, which makes it
//! useless as a safety property — Atlas being sure about something is not
//! evidence.

use serde::{Deserialize, Serialize};

/// How far the blast radius reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    /// Words Atlas says, phrase lists, wording. Wrong is embarrassing and
    /// nothing else.
    WhatItSays,
    /// How it ranks, sorts or chooses. Wrong means worse answers.
    HowItDecides,
    /// A whole new module. Additive and reversible, but it's a new thing in
    /// the system rather than a change to an existing one — which is a
    /// different decision and deserves to be asked as one.
    SomethingNew,
    /// What it does to your machine or your accounts.
    WhatItTouches,
    /// What it's allowed to do at all.
    ItsOwnLimits,
}

impl Reach {
    pub fn plain(&self) -> &'static str {
        match self {
            Reach::WhatItSays => "what it says",
            Reach::HowItDecides => "how it decides things",
            Reach::SomethingNew => "adding something new",
            Reach::WhatItTouches => "what it does to your machine",
            Reach::ItsOwnLimits => "what it's allowed to do",
        }
    }

    /// A number that doesn't move when the enum is reordered.
    ///
    /// `PartialOrd` on a variant order is a comparison that silently changes
    /// meaning the day someone inserts a level in the middle — and what it
    /// decides here is what Atlas may do to itself.
    pub fn severity(&self) -> u8 {
        match self {
            Reach::WhatItSays => 1,
            Reach::HowItDecides => 2,
            Reach::SomethingNew => 3,
            Reach::WhatItTouches => 4,
            Reach::ItsOwnLimits => 5,
        }
    }

    /// Can this ever be granted?
    ///
    /// The last one never. A system that can widen its own limits has no
    /// limits — everything else in this file is a preference, and this one
    /// isn't.
    pub fn ever_grantable(&self) -> bool {
        *self != Reach::ItsOwnLimits
    }

    /// What goes wrong if a change here is wrong.
    pub fn if_wrong(&self) -> &'static str {
        match self {
            Reach::WhatItSays => "it says something clumsy until you notice",
            Reach::HowItDecides => "it gives worse answers, quietly, until you notice",
            Reach::SomethingNew => "there's a thing in the system you didn't ask for",
            Reach::WhatItTouches => "it does something to your files or your accounts",
            Reach::ItsOwnLimits => "it can do more than you agreed to, and can widen that again",
        }
    }
}

/// Files whose whole purpose is to say what Atlas may not do.
const ITS_OWN_LIMITS: &[&str] = &[
    "policy.rs", "consent.rs", "finance.rs", "grants.rs", "selfgrant.rs",
    "categories.rs", "confirmed.rs", "pipeline.rs", "cargo.toml", "policy.yaml",
    // tools.yaml holds `self_grant:` and `financial_domains:` — the settings
    // that decide what Atlas may do to itself and which sites it must never
    // act on. It looks like ordinary config and isn't.
    "tools.yaml",
    // And the struct those settings parse into. Adding a field with a
    // permissive default changes a limit without touching the YAML.
    "voice.rs",
    // What judges its own fixes: `mend.rs` refuses a paper-over (a fix that
    // hides the symptom), `selfwork.rs` holds `may_edit` and the shipped
    // never-touch list, `sandbox.rs` is where the proof runs and its time
    // limit, `selftest.rs` is what reports how it's doing. A self-fix that
    // could edit any of these could weaken the check on itself (research
    // report, 30 Sep 2026, Stage 1 item 2).
    "mend.rs", "selfwork.rs", "sandbox.rs", "selftest.rs",
    // The rest of the fences (1 Oct 2026 security pass): `uia.rs` decides
    // what can't be undone, `research.rs` what may be fetched, `untrusted.rs`
    // how outside text is kept from being obeyed, `operate.rs` the app
    // worker's guard, `smtp.rs` the send limit, `unsub.rs` which links are
    // followed, `transport.rs` and `server.rs` who is answered, `lookalike.rs`
    // which mail is trusted, `vault.rs` the passwords.
    "uia.rs", "research.rs", "untrusted.rs", "operate.rs", "smtp.rs", "unsub.rs",
    "transport.rs", "server.rs", "lookalike.rs", "vault.rs",
];

/// Tests Atlas may edit.
///
/// An allowlist rather than a blocklist, because the failure directions are
/// not symmetric: over-restricting costs a question, under-restricting costs
/// the limit. Anything not on this list is treated as a guard.
const ORDINARY_TESTS: &[&str] = &[
    "prose.rs", "look.rs", "grading.rs", "layout.rs", "metrics.rs",
    "language.rs", "voiceover.rs", "plainchange.rs",
];

/// The modules that limits are enforced by.
///
/// A test that mentions one of these is a guard, and must never appear in
/// `ORDINARY_TESTS`. There's a test asserting exactly that, so a guard can't
/// be allowlisted by accident.
pub const LIMIT_MODULES: &[&str] = &[
    "policy", "selfgrant", "categories", "consent", "finance", "confirmed",
    "vault", "signin", "enrol", "pipeline",
];

/// Which files sit at which reach.
///
/// Deliberately by path rather than by judgement, so it can't drift.
///
/// The hole this closes: an unknown file used to fall through to
/// `HowItDecides`, which meant `tests/wiring.rs`, `tests/retrospective.rs` and
/// `config/policy.yaml` were all editable under an ordinary grant. Atlas could
/// have edited the ratchet that catches unreachable code, the audit that
/// catches bad tests, or the policy config itself — each of which hides the
/// next mistake rather than causing one.
pub fn reach_of(path: &str) -> Reach {
    let p = path.replace('\\', "/").to_lowercase();
    let file = p.rsplit('/').next().unwrap_or(&p);

    if ITS_OWN_LIMITS.iter().any(|f| file == *f) {
        return Reach::ItsOwnLimits;
    }
    // Anything that runs before, or instead of, the tests. A build script or
    // a CI config can make the whole suite vacuous without touching a test.
    if p.starts_with(".cargo/") || p.starts_with(".github/") || p.contains("/.cargo/")
        || file == "build.rs" || file == "rust-toolchain.toml"
    {
        return Reach::ItsOwnLimits;
    }

    if p.starts_with("tests/") || p.contains("/tests/") {
        if ORDINARY_TESTS.iter().any(|f| file == *f) {
            // Still shown to you: a test says what correct means, and that's
            // worth seeing even when it's an ordinary one.
            return Reach::WhatItTouches;
        }
        return Reach::ItsOwnLimits;
    }

    if ["system.rs", "workspace.rs", "publish.rs", "signin.rs", "vault.rs", "mail.rs",
        "browser.rs", "enrol.rs", "cdp.rs", "server.rs", "install.rs", "selfwork.rs",
        "selfaudit.rs"]
        .iter()
        .any(|f| file == *f)
    {
        return Reach::WhatItTouches;
    }

    if ["commands.yaml", "persona.rs", "plainchange.rs", "voiceover.rs", "language.rs"]
        .iter()
        .any(|f| file == *f)
    {
        return Reach::WhatItSays;
    }

    // Remaining config parameterises behaviour without being a limit — but a
    // config change is still a change to what Atlas does on your machine.
    if p.ends_with(".yaml") || p.ends_with(".yml") || p.ends_with(".toml") {
        return Reach::WhatItTouches;
    }

    // Everything else decides something. The safe default is the middle,
    // not the bottom.
    Reach::HowItDecides
}

/// A change's reach is the furthest thing it touches.
///
/// Not the average, and not the one it's mostly about — one file at a higher
/// reach makes the whole change that reach.
pub fn reach_of_change(paths: &[String]) -> Reach {
    paths
        .iter()
        .map(|p| reach_of(p))
        .max_by_key(|r| r.severity())
        .unwrap_or(Reach::WhatItSays)
}

/// What you've said Atlas may do alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Granted {
    pub up_to: Reach,
    /// When you said so.
    pub at: u64,
}

// A grant used to run out after 90 days you were around for. Eric, 25 Sep
// 2026 (B3): "I have to step away from my computer for long periods of time,
// sometimes. I don't need Atlas to break in that time." A grant now stands
// until you take it back (Settings, or changing `self_grant.may_change`). The
// counter it ran out on had no writer anyway, so it never ran out; now that's
// the rule rather than an accident.

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SelfGrantConfig {
    /// The furthest Atlas may go alone, as a name. Stands until you change
    /// it: grants don't lapse (Eric, B3).
    pub may_change: String,
    /// Everything done alone is still recorded and undoable.
    #[serde(skip, default = "always")]
    pub always_reversible: bool,
}

fn always() -> bool {
    true
}

impl Default for SelfGrantConfig {
    fn default() -> Self {
        SelfGrantConfig {
            // Fixing how it decides things, on its own. Adding something new
            // or touching your machine always comes to you.
            //
            // The old default of nothing was the wrong way round: it made a
            // self-improving system that couldn't improve anything.
            may_change: "how_it_decides".into(),
            always_reversible: true,
        }
    }
}

impl SelfGrantConfig {
    pub fn granted(&self) -> Option<Reach> {
        match self.may_change.as_str() {
            "what_it_says" => Some(Reach::WhatItSays),
            "how_it_decides" => Some(Reach::HowItDecides),
            // Refused rather than parsed. Writing it in the file doesn't make
            // it true.
            "something_new" | "what_it_touches" | "its_own_limits" => None,
            _ => None,
        }
    }
}

/// May Atlas land this without asking?
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Go ahead, and say what changed afterwards.
    GoAhead { say_after: String },
    /// Ask first, with the reason.
    AskFirst(String),
    /// Never, whatever is granted.
    Never(String),
}

pub fn may_land(
    paths: &[String],
    tests_passed: bool,
    reversible: bool,
    granted: Option<&Granted>,
    is_a_new_module: bool,
    cfg: &SelfGrantConfig,
) -> Verdict {
    let reach = if is_a_new_module {
        reach_of_change(paths).max(Reach::SomethingNew)
    } else {
        reach_of_change(paths)
    };

    if !reach.ever_grantable() {
        return Verdict::Never(format!(
            "that changes {}, which I can't do on my own however this is set — a system that \
             widens its own limits doesn't have any",
            reach.plain()
        ));
    }
    // A change that can't be taken back is one you should have seen.
    if !reversible && cfg.always_reversible {
        return Verdict::AskFirst("I couldn't undo that one, so you should see it first".into());
    }
    if !tests_passed {
        return Verdict::AskFirst("the tests don't pass".into());
    }

    match granted {
        None => Verdict::AskFirst(format!("you haven't said I can change {}", reach.plain())),
        Some(g) if reach.severity() > g.up_to.severity() => Verdict::AskFirst(format!(
            "that reaches {} and you've only said {}",
            reach.plain(),
            g.up_to.plain()
        )),
        Some(_) => Verdict::GoAhead {
            // Said after, never asked before — but never silent either.
            say_after: format!("Changed {} on my own. {}", reach.plain(), "Say undo if not."),
        },
    }
}

/// How Atlas raises something it wants to change.
///
/// Low-pressure on purpose. A system that interrupts with "I need a decision"
/// about its own internals is one you learn to dread, and the thing it wants
/// is almost never urgent — it has waited this long and can wait longer.
pub fn raise_it(what: &str, reach: Reach) -> String {
    format!(
        "I've got a system recommendation when you've got a minute — {what}. It'd change {}, so \
         it's yours rather than mine.",
        reach.plain()
    )
}

/// Something raised and not answered.
///
/// Said once more, then left alone. Asking twice is a reminder; asking three
/// times is nagging about something that was never urgent.
pub fn raised_again(what: &str, times_asked: u32) -> Option<String> {
    match times_asked {
        0 | 1 => Some(format!("Still got that recommendation about {what}, whenever suits.")),
        _ => None,
    }
}

/// What Atlas says when asking for a standing grant.
///
/// Names what goes wrong if it gets one wrong, because that's the thing you're
/// actually deciding about.
pub fn asking_for(reach: Reach, _cfg: &SelfGrantConfig) -> String {
    format!(
        "Want me to fix {} on my own? If I get one wrong, {}. Everything's recorded and undoable, \
         and it stands until you take it back.",
        reach.plain(),
        reach.if_wrong(),
    )
}

/// Why this rather than asking every time.
pub const WHY_STANDING: &str =
    "Asking before every change sounds safe and isn't. A system that needs permission to fix a \
     typo in a phrase list never fixes one, and the recommendations pile up unread — that doesn't \
     make anything safer, it makes everything stop. Granting by what a change touches, once, \
     keeps the check where it's worth having.";

/// Why not by confidence.
pub const NOT_BY_CONFIDENCE: &str =
    "I could tell you how sure I am about each change and let that decide. That would be worse: \
     being sure is something I assess about myself, and a mistake I'm confident about is exactly \
     the one that gets through. What a change touches is a fact about the change.";
