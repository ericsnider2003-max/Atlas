//! If something happens to you.
//!
//! ## What the envelope actually is
//!
//! Physical. A piece of paper with the passphrase on it, in a sealed envelope,
//! in a place you control — your safe, a lockbox, a fire tin. Not given to
//! anyone. Not a file, not a cloud folder, not a photo on your phone.
//!
//! It's paper for a specific reason: it can't be phished, can't be
//! brute-forced, can't be copied remotely, and doesn't quietly sync to five
//! devices the way anything digital does. It also can't be accessed by
//! coincidence — nobody stumbles into a sealed envelope in your safe the way
//! they stumble into an unlocked laptop.
//!
//! ## Nobody gets your information
//!
//! What another person is given is **where it is and when to open it**. Not
//! the passphrase, not a copy, not access. A sentence: *"if I'm out of contact
//! for ninety days, there's an envelope in the safe."*
//!
//! That's the whole design. In your hands first, reachable by you at any time,
//! and reachable by someone else only after a condition you set has actually
//! happened.

use serde::{Deserialize, Serialize};

/// Where the paper lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Where {
    /// Your safe, your lockbox, your house.
    YoursAlone,
    /// A bank box in your name.
    BankBox,
    /// Someone holds the sealed envelope without knowing what's in it.
    SealedWithSomeone,
    /// Split so no single person's copy is useful.
    SplitPhysically,
}

impl Where {
    /// Can someone open it without you knowing?
    pub fn openable_without_you(&self) -> bool {
        matches!(self, Where::SealedWithSomeone | Where::SplitPhysically)
    }

    /// Can it be reached if you're not available?
    fn reachable_without_you(&self) -> bool {
        !matches!(self, Where::BankBox)
    }

    /// What is wrong with keeping it there.
    ///
    /// Named `the_catch` rather than `honest`: `mesh` and `messaging` each
    /// have a method called `honest` that nothing calls, and a caller here
    /// would have made both look alive. The name is also the better one —
    /// every arm of this says what the drawback is.
    pub fn the_catch(&self) -> &'static str {
        match self {
            Where::YoursAlone => {
                "yours entirely, and useless to anyone if they can't get into your house"
            }
            Where::BankBox => {
                "very safe, and the hardest for anyone else to reach — a bank box in one name \
                 usually needs probate, which takes months"
            }
            Where::SealedWithSomeone => {
                "reachable quickly, and that person could open it any evening they felt like it"
            }
            Where::SplitPhysically => {
                "no single person's piece is worth anything, and it needs two of them to be \
                 reachable at once"
            }
        }
    }
}

/// What has to be true before anyone else is told where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    /// You've been out of contact this long.
    OutOfContact { days: u32 },
    /// Never automatic — someone has to ask you and you say yes.
    OnlyIfYouSaySo,
}

/// The instruction someone is given.
///
/// Deliberately not the passphrase. A location and a condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instruction {
    /// Who.
    pub person: String,
    /// Where the envelope is, in words they'd understand.
    pub location: String,
    pub when: When,
    /// What they should do with it.
    pub then_what: String,
    /// They've confirmed they know and agreed.
    pub they_know: bool,
}

impl Instruction {
    /// What the person is actually told. One sentence, and no secrets in it.
    pub fn as_told(&self) -> String {
        let condition = match self.when {
            When::OutOfContact { days } => {
                format!("if I'm out of contact for {days} days")
            }
            When::OnlyIfYouSaySo => "only if I ask you to".to_string(),
        };
        format!(
            "{condition}, there's a sealed envelope {}. {}",
            self.location, self.then_what
        )
    }

    // `gives_anything_away_now` was here, returning `false` for every
    // instruction ever built. Deleted 19 Sep 2026: a bool nobody reads is not
    // a boundary, and this one could not have been anything else — the answer
    // was a constant. The guarantee is real and it is held by the type, which
    // has nowhere to put a secret: a person, a place, a condition and what to
    // do. `an_instruction_has_nowhere_to_put_a_secret` in
    // `tests/the_envelope.rs` is that invariant asserted where it is
    // enforced, rather than restated as a method.
}

/// The thing that makes an out-of-contact condition work at all.
///
/// Something has to be counting, and your laptop is off for six months at a
/// time. That rules out Atlas being the timer, which is worth saying plainly
/// rather than building something that silently never fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timer {
    /// Google's Inactive Account Manager, Apple's Legacy Contact, and the
    /// equivalents. Free, runs on their servers, works whether your devices
    /// are on or not.
    ThePlatforms,
    /// Your phone, which is on more than the laptop is.
    YourPhone,
    /// A person who checks in with you.
    APerson,
    /// Atlas. Only honest if the laptop is actually on.
    Atlas,
}

impl Timer {
    /// Does it keep counting while your laptop is off for months?
    pub fn survives_your_absence(&self) -> bool {
        !matches!(self, Timer::Atlas)
    }

    pub fn why(&self) -> &'static str {
        match self {
            Timer::ThePlatforms => {
                "runs on their servers, costs nothing, and keeps counting whether any device of \
                 yours is on — this is what it's for"
            }
            Timer::YourPhone => "on more than the laptop, but it still has to be on",
            Timer::APerson => "the only one that notices something is wrong rather than just late",
            Timer::Atlas => {
                "no use here — your laptop is off while you're away, so the count would never \
                 run and nothing would ever fire"
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AfterMeConfig {
    pub enabled: bool,
    /// Where the paper is.
    pub location: String,
    /// Days out of contact.
    pub after_days: u32,
    /// Nudge you to check the arrangement still holds.
    pub review_every_days: u64,
    // `atlas_holds_it` was here: `#[serde(skip)]`, pinned false, read by
    // nothing. Deleted 19 Sep 2026 when this module was wired, because a bool
    // nobody reads is not a boundary — and this one could not have become
    // one, since nothing would have consulted it before deciding.
    //
    // The guarantee is real and the types hold it: there is no field
    // anywhere in `AfterMeConfig`, `Arrangement` or `Instruction` that a
    // passphrase could be put in, so Atlas cannot hold one whatever any
    // config says. `what_is_written_down_contains_no_secret_and_could_not`
    // in `tests/the_envelope.rs` asserts that where it is enforced.
}

impl Default for AfterMeConfig {
    fn default() -> Self {
        AfterMeConfig {
            enabled: false,
            location: String::new(),
            // Long enough that a deployment doesn't trigger it, short enough
            // to be useful.
            after_days: 180,
            review_every_days: 365,
        }
    }
}

/// What's missing from the arrangement.
#[derive(Debug, Clone, PartialEq)]
pub struct Gap {
    pub what: String,
    pub why: String,
    pub urgency: f32,
}

/// What's missing from the arrangement.
///
/// Private since 19 Sep 2026. `Arrangement::gaps` is the way in: the inputs
/// now live in one place that something actually holds, and two public ways
/// to ask the same question is how one of them goes unused and then wrong.
fn gaps(
    where_: Option<Where>,
    instructions: &[Instruction],
    timer: Option<Timer>,
    cfg: &AfterMeConfig,
) -> Vec<Gap> {
    let mut out = Vec::new();

    if where_.is_none() {
        out.push(Gap {
            what: "there's no envelope".into(),
            why: "everything in the vault goes with you, including the recovery codes".into(),
            urgency: 1.0,
        });
        return out;
    }
    let w = where_.unwrap();

    if instructions.is_empty() {
        out.push(Gap {
            what: "nobody knows where it is".into(),
            why: "an envelope nobody can find is the same as no envelope".into(),
            urgency: 0.9,
        });
    }

    for i in instructions {
        if !i.they_know {
            out.push(Gap {
                what: format!("{} hasn't been told", i.person),
                why: "an arrangement someone doesn't know about isn't an arrangement".into(),
                urgency: 0.85,
            });
        }
    }

    match timer {
        None => out.push(Gap {
            what: "nothing is counting the days".into(),
            why: "an out-of-contact condition needs something that keeps running while you're \
                  away, and your laptop is off"
                .into(),
            urgency: 0.8,
        }),
        Some(t) if !t.survives_your_absence() => out.push(Gap {
            what: "the count runs on the laptop".into(),
            why: t.why().into(),
            urgency: 0.9,
        }),
        _ => {}
    }

    // `!w.reachable_without_you()`, not `w == Where::BankBox`.
    //
    // These are the same test today -- `reachable_without_you` is
    // `!matches!(self, BankBox)` -- and they are the same fact written twice,
    // with the named one having no caller. The compiler found it the moment
    // this function stopped being `pub`: nothing in the program asked the
    // question the predicate exists to answer.
    //
    // It matters because the arrangement list grows. Add a `Where` that also
    // cannot be reached while you are unavailable -- a box abroad, a solicitor
    // holding it -- and the hand-written comparison silently stops warning,
    // which in this module means Atlas quietly stops telling you that the
    // people who need the envelope cannot get it.
    if !w.reachable_without_you() && cfg.after_days < 365 {
        out.push(Gap {
            what: "a bank box may take longer to open than your condition allows".into(),
            why: "probate can run months — worth a second copy somewhere reachable".into(),
            urgency: 0.5,
        });
    }

    out.sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// What Atlas suggests, given that you want it yours first.
pub fn suggest_for_you() -> Vec<(Where, &'static str)> {
    vec![
        (
            Where::YoursAlone,
            "the envelope in your own safe, and one person told only where it is and when to \
             open it — nothing changes hands, and nobody can reach it by coincidence",
        ),
        (
            Where::SplitPhysically,
            "if you'd rather no single person could ever open it, split the paper in two and \
             give the halves to different people — neither piece is worth anything alone",
        ),
    ]
}

/// The recommendation, in one line.
pub const THE_SHAPE: &str =
    "Paper, in your safe, and one person who knows only where it is and when to open it. You can \
     reach it any day you like. Nobody else can reach it by accident, and nobody has been handed \
     anything.";

/// Why it isn't digital.
pub const WHY_PAPER: &str =
    "Anything digital syncs. A file gets backed up, a photo goes to the cloud, a note ends up on \
     three devices — and every one of those is a copy you didn't decide to make. Paper stays \
     where you put it and you can tell if the seal is broken.";

/// What Atlas never holds.
pub const NOT_ATLAS: &str =
    "I don't hold the passphrase or a copy of it. If I did, the envelope would be pointless and \
     so would the vault — the whole arrangement rests on the way back in being somewhere I'm not.";

/// The bit worth saying about timers.
pub const USE_THE_PLATFORMS: &str =
    "For the out-of-contact part, use Google's Inactive Account Manager or Apple's Legacy \
     Contact. They're free, they run on someone else's servers, and they keep counting while your \
     laptop is in a drawer — which mine can't. Set the trigger there and have it release nothing \
     but a note saying where the envelope is.";

// ============ the arrangement, written down ============
//
// This module was complete, tested and unreachable: `gaps` takes a `Where`,
// a list of `Instruction`s and a `Timer`, and nothing in the tree had any of
// those, so nothing could call it. `after_me:` sat in `tools.yaml` being
// parsed into a field nobody read — `config::PARSED_AND_NEVER_READ` recorded
// exactly that.
//
// The missing piece was never the logic. It was a place to keep what you have
// actually arranged, and a way to say it.

/// What Atlas keeps about the arrangement. Never the passphrase.
///
/// There is nothing secret in here by construction: a kind of place, who has
/// been told, what they were told, and what is counting the days. Somebody
/// reading this file learns that an envelope exists and not one thing about
/// what is in it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Arrangement {
    pub kind: Option<Where>,
    pub instructions: Vec<Instruction>,
    pub timer: Option<Timer>,
    /// When you last confirmed it still holds. Zero means never.
    pub reviewed_at: u64,
}

/// Where that is kept.
pub const RECORD: &str = "after_me";

impl Arrangement {
    /// What's missing, against the settings you chose.
    pub fn gaps(&self, cfg: &AfterMeConfig) -> Vec<Gap> {
        gaps(self.kind, &self.instructions, self.timer, cfg)
    }

    /// Tell someone. Replaces what that person was told rather than adding a
    /// second instruction to the same name — two different answers about
    /// where the envelope is, held for one person, is worse than none:
    /// whichever they act on, half the time it is the wrong one.
    ///
    /// Changing what they would hear un-agrees them. They agreed to a
    /// sentence, and a different sentence is a different agreement — an
    /// arrangement that quietly kept somebody's yes across a move of the
    /// envelope would be recording consent to something nobody gave.
    pub fn tell(&mut self, mut i: Instruction) {
        if let Some(old) = self
            .instructions
            .iter()
            .find(|x| x.person.eq_ignore_ascii_case(&i.person))
        {
            i.they_know = if old.as_told() == i.as_told() { old.they_know } else { false };
        }
        self.instructions.retain(|x| !x.person.eq_ignore_ascii_case(&i.person));
        self.instructions.push(i);
    }

    /// They've confirmed they know and agreed.
    ///
    /// Named `they_agreed` rather than `confirmed`: `delegate` has a
    /// `confirmed` that nothing calls, and a caller here would have made it
    /// look alive.
    pub fn they_agreed(&mut self, person: &str) -> bool {
        match self
            .instructions
            .iter_mut()
            .find(|i| i.person.eq_ignore_ascii_case(person))
        {
            Some(i) => {
                i.they_know = true;
                true
            }
            None => false,
        }
    }

    /// Is it time to check the arrangement still holds?
    ///
    /// `review_every_days` is the setting this reads, and until 19 Sep 2026
    /// nothing read it. A review interval is the only part of an arrangement
    /// like this that has to run on a clock: everything else is decided once,
    /// and the failure mode is a plan that was true three years ago.
    pub fn due_for_review(&self, cfg: &AfterMeConfig, now: u64) -> bool {
        if !cfg.enabled || cfg.review_every_days == 0 {
            return false;
        }
        // Nothing arranged is not something to review; it is a gap, and
        // `gaps` is what says so. Nagging about reviewing an arrangement that
        // does not exist is how a nudge gets ignored.
        if self.kind.is_none() {
            return false;
        }
        if self.reviewed_at == 0 || self.reviewed_at > now {
            return true;
        }
        now.saturating_sub(self.reviewed_at) >= cfg.review_every_days * 86_400
    }

    /// The nudge, when one is due.
    pub fn nudge(&self, cfg: &AfterMeConfig, now: u64) -> Option<String> {
        if !self.due_for_review(cfg, now) {
            return None;
        }
        let years = cfg.review_every_days / 365;
        let how_long = if years >= 1 {
            format!("{years} year{}", if years == 1 { "" } else { "s" })
        } else {
            format!("{} days", cfg.review_every_days)
        };
        let who: Vec<&str> = self.instructions.iter().map(|i| i.person.as_str()).collect();
        let named = if who.is_empty() {
            "nobody has been told".to_string()
        } else {
            format!("{} knows", who.join(" and "))
        };
        Some(format!(
            "It's been {how_long} since you checked the envelope arrangement — {named}. Still \
             right? `atlas afterme reviewed` when you've looked."
        ))
    }

    /// The whole arrangement, for reading.
    pub fn spoken(&self, cfg: &AfterMeConfig, now: u64) -> String {
        let mut s = String::new();
        match self.kind {
            None => s.push_str("Nothing arranged.\n"),
            Some(w) => {
                s.push_str(&format!("The envelope: {}\n", w.the_catch()));
                if !cfg.location.is_empty() {
                    s.push_str(&format!("Where you said: {}\n", cfg.location));
                }
                if w.openable_without_you() {
                    s.push_str(
                        "Worth knowing: somebody could open that without you ever finding out.\n",
                    );
                }
            }
        }
        if self.instructions.is_empty() {
            s.push_str("Nobody has been told.\n");
        } else {
            for i in &self.instructions {
                s.push_str(&format!(
                    "{}{}: \"{}\"\n",
                    i.person,
                    if i.they_know { "" } else { " (hasn't agreed yet)" },
                    i.as_told()
                ));
            }
        }
        match self.timer {
            None => s.push_str("Nothing is counting the days.\n"),
            Some(t) => s.push_str(&format!("Counting: {}\n", t.why())),
        }

        let gaps = self.gaps(cfg);
        if gaps.is_empty() {
            s.push_str("\nNothing missing that I can see.\n");
        } else {
            s.push_str("\nWhat's missing:\n");
            for g in gaps {
                s.push_str(&format!("  {} — {}\n", g.what, g.why));
            }
        }
        if let Some(n) = self.nudge(cfg, now) {
            s.push_str(&format!("\n{n}\n"));
        }
        s
    }
}

/// Read a place from what somebody typed.
pub fn where_from(word: &str) -> Option<Where> {
    match word.trim().to_lowercase().as_str() {
        "yours" | "mine" | "safe" | "yours_alone" => Some(Where::YoursAlone),
        "bank" | "bankbox" | "bank_box" => Some(Where::BankBox),
        "sealed" | "someone" | "sealed_with_someone" => Some(Where::SealedWithSomeone),
        "split" | "split_physically" => Some(Where::SplitPhysically),
        _ => None,
    }
}

/// Read a timer from what somebody typed.
pub fn timer_from(word: &str) -> Option<Timer> {
    match word.trim().to_lowercase().as_str() {
        "platforms" | "google" | "apple" => Some(Timer::ThePlatforms),
        "phone" => Some(Timer::YourPhone),
        "person" | "someone" => Some(Timer::APerson),
        "atlas" | "laptop" => Some(Timer::Atlas),
        _ => None,
    }
}
