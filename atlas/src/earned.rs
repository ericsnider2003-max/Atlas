//! Confidence Atlas has earned, per kind of work.
//!
//! The complaint this answers, in Eric's words: when Atlas isn't confident
//! enough, the fix must not be "raise the bar until it acts less". That is not
//! smarter, it is more restrictive, and it ends with him doing everything
//! himself again.
//!
//! ## The distinction the old system missed
//!
//! `certainty.rs` asks *how sure am I about this answer* — from grounding,
//! hedging words, whether a source was read. That is about one answer, and it
//! is the same question every time regardless of whether Atlas has done this
//! kind of thing five hundred times correctly or never once.
//!
//! This asks a different question: **how often has Atlas been right about this
//! kind of thing before?** A ceiling that is the same for tidying a folder and
//! for sending a message to a business partner is not calibration, it is a
//! single global setting wearing calibration's clothes.
//!
//! ## Why per category, and why it can go down
//!
//! Globally, Atlas's record is dominated by whatever it does most. Being
//! excellent at reading files would raise its licence to act on money, which
//! is exactly backwards. So the record is kept per `Kind`, and a `Kind` earns
//! its own ceiling.
//!
//! It falls faster than it rises, and one correction on something already
//! trusted costs more than one on something new. A track record that only ever
//! improves is not a record, it is a counter.
//!
//! ## What it never does
//!
//! It does not decide. It answers "may Atlas do this alone", and something
//! else decides what to do with the answer. Nothing here can grant reach that
//! the permission settings have not already given — earning trust widens what
//! Atlas may do *within* what you allowed, never past it.

use serde::{Deserialize, Serialize};


/// Whose work this was.
///
/// The record is kept per space as well as per kind, because a track record
/// built on Eric's own tasks is not evidence about a business partner's. He
/// was explicit: Atlas starts on personal work, builds a record there, and
/// only then graduates to acting on business tasks — and the business bar is
/// visibly higher, because a wrong confident guess there costs somebody who
/// never agreed to the risk.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Space {
    /// Eric's own work.
    #[default]
    Personal,
    /// A named business, shared with somebody.
    Business(String),
}


impl Space {
    pub fn title(&self) -> String {
        match self {
            Space::Personal => "Your own work".to_string(),
            Space::Business(name) => name.clone(),
        }
    }

    pub fn is_business(&self) -> bool {
        matches!(self, Space::Business(_))
    }
}

/// A kind of work, for the purpose of keeping score.
///
/// Coarse on purpose. Fifty categories would each hold three data points, and
/// three data points is a rumour rather than a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Answering from what it already knows or has read.
    Answering,
    /// Finding something — a file, a note, a page.
    Finding,
    /// Tidying, moving, renaming: reversible things on this machine.
    Housekeeping,
    /// Writing something you will read before it goes anywhere.
    Drafting,
    /// Changing how the machine itself is set up.
    ChangingTheMachine,
    /// Anything that leaves this machine, or that someone else sees.
    ReachingOut,
    /// Anything touching money, accounts or secrets.
    Sensitive,
}

impl Kind {
    pub fn all() -> [Kind; 7] {
        [
            Kind::Answering,
            Kind::Finding,
            Kind::Housekeeping,
            Kind::Drafting,
            Kind::ChangingTheMachine,
            Kind::ReachingOut,
            Kind::Sensitive,
        ]
    }

    pub fn title(self) -> &'static str {
        match self {
            Kind::Answering => "Answering",
            Kind::Finding => "Finding things",
            Kind::Housekeeping => "Housekeeping",
            Kind::Drafting => "Drafting",
            Kind::ChangingTheMachine => "Machine changes",
            Kind::ReachingOut => "Reaching out",
            Kind::Sensitive => "Money and secrets",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            Kind::Answering => "Questions answered from what Atlas already knows",
            Kind::Finding => "Looking something up on this machine",
            Kind::Housekeeping => "Reversible tidying — moving, renaming, sorting",
            Kind::Drafting => "Writing you will read before it goes anywhere",
            Kind::ChangingTheMachine => "Settings and software on this machine",
            Kind::ReachingOut => "Anything that leaves here, or that someone else sees",
            Kind::Sensitive => "Money, accounts, and anything the vault holds",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Answering => "answering",
            Kind::Finding => "finding",
            Kind::Housekeeping => "housekeeping",
            Kind::Drafting => "drafting",
            Kind::ChangingTheMachine => "machine",
            Kind::ReachingOut => "reaching_out",
            Kind::Sensitive => "sensitive",
        }
    }

    /// How bad it is to be wrong here, before any track record is considered.
    ///
    /// A record cannot argue this away. Being right ninety-nine times about
    /// money does not make the hundredth mistake cheap — the cost of a wrong
    /// confident guess is a property of the act, not of the history.
    pub fn cost_of_being_wrong(self) -> u8 {
        match self {
            Kind::Answering | Kind::Finding => 1,
            Kind::Housekeeping | Kind::Drafting => 2,
            Kind::ChangingTheMachine => 3,
            Kind::ReachingOut => 4,
            Kind::Sensitive => 5,
        }
    }
}

/// How much rope a kind of work has earned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rope {
    /// Ask first, every time.
    AskFirst,
    /// Do it and say so immediately, so a mistake is caught while it is still
    /// one action old.
    DoAndSay,
    /// Do it. It appears in the day's account like everything else.
    JustDo,
}

impl Rope {
    pub fn plain(self) -> &'static str {
        match self {
            Rope::AskFirst => "asks you first",
            Rope::DoAndSay => "does it and tells you straight away",
            Rope::JustDo => "gets on with it",
        }
    }
}

/// One thing Atlas did, and whether it turned out right.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub kind: Kind,
    /// Whose work it was. Defaults to personal so a record written before
    /// businesses existed still reads — those outcomes genuinely were his own.
    #[serde(default)]
    pub space: Space,
    /// Did it turn out to be right?
    pub good: bool,
    /// When, so an old record can be told from a current one.
    pub at: u64,
    /// What it was, short, so a bad run can be recognised rather than
    /// only counted.
    pub about: String,
}

/// How many outcomes a kind keeps.
///
/// A window, not a total. What Atlas was like in June should not be why it is
/// trusted in September, and a total means a bad week can never be recovered
/// from either.
pub const WINDOW: usize = 30;

/// Fewest outcomes before a record means anything.
///
/// Below this there is no record, only a coincidence. Three right answers in a
/// row happens by chance often enough that acting on it would be acting on
/// nothing.
pub const ENOUGH: usize = 8;

/// The record, per kind.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub outcomes: Vec<Outcome>,
}

/// Where it is kept.
pub const FILE: &str = "confidence";

impl Record {
    pub fn load(store: &crate::store::Store) -> Record {
        store.load::<Record>(FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    /// Write down how something turned out.
    pub fn note(&mut self, kind: Kind, good: bool, about: &str, at: u64) {
        self.note_in(&Space::Personal, kind, good, about, at)
    }

    /// The same, for work done inside a named business.
    pub fn note_in(&mut self, space: &Space, kind: Kind, good: bool, about: &str, at: u64) {
        self.outcomes.push(Outcome {
            kind,
            space: space.clone(),
            good,
            at,
            about: about.chars().take(60).collect(),
        });
        // Trim per kind, not globally: a busy category would otherwise push a
        // quiet one's entire history out and reset it to untrusted.
        let mut seen = 0;
        let mut keep = vec![true; self.outcomes.len()];
        for i in (0..self.outcomes.len()).rev() {
            if self.outcomes[i].kind == kind && self.outcomes[i].space == *space {
                seen += 1;
                if seen > WINDOW {
                    keep[i] = false;
                }
            }
        }
        let mut i = 0;
        self.outcomes.retain(|_| {
            let k = keep[i];
            i += 1;
            k
        });
    }

    pub fn of(&self, kind: Kind) -> Vec<&Outcome> {
        self.of_in(&Space::Personal, kind)
    }

    fn of_in(&self, space: &Space, kind: Kind) -> Vec<&Outcome> {
        self.outcomes
            .iter()
            .filter(|o| o.kind == kind && o.space == *space)
            .collect()
    }

    /// Every business Atlas has done work for.
    pub fn businesses(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .outcomes
            .iter()
            .filter_map(|o| match &o.space {
                Space::Business(n) => Some(n.clone()),
                Space::Personal => None,
            })
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// How many times Atlas has been right about this, out of how many.
    pub fn tally(&self, kind: Kind) -> (usize, usize) {
        self.tally_in(&Space::Personal, kind)
    }

    fn tally_in(&self, space: &Space, kind: Kind) -> (usize, usize) {
        let all = self.of_in(space, kind);
        (all.iter().filter(|o| o.good).count(), all.len())
    }

    /// The most recent run of wrong answers, which is what a person actually
    /// notices — three in a row today matters more than three spread over a
    /// month.
    pub fn wrong_lately(&self, kind: Kind) -> usize {
        self.wrong_lately_in(&Space::Personal, kind)
    }

    fn wrong_lately_in(&self, space: &Space, kind: Kind) -> usize {
        self.of_in(space, kind)
            .iter()
            .rev()
            .take_while(|o| !o.good)
            .count()
    }

    /// How much rope this kind of work has earned.
    ///
    /// Deliberately asymmetric. Rising takes a long clean run; falling takes
    /// two mistakes. The cost of being wrong is a floor the record cannot
    /// argue past — `Sensitive` never leaves `AskFirst`, however good the run,
    /// because that is a decision for Eric to make and not one for Atlas to
    /// earn.
    pub fn rope(&self, kind: Kind) -> Rope {
        self.rope_in(&Space::Personal, kind)
    }

    /// The same, in a named space.
    ///
    /// **Graduation.** A business kind can never outrank the same kind on
    /// Eric's own work. Atlas has to be trusted to draft for him before it is
    /// trusted to draft for a partner, and no amount of good business work
    /// substitutes for that — a record built entirely inside a business is a
    /// record with nobody checking it, since the partner is not the one who
    /// would notice a wrong tone.
    ///
    /// **And a higher bar on top.** Business work is capped at `DoAndSay`
    /// however good both records are. Something a partner sees always gets
    /// said out loud. That cap is not earnable, for the same reason
    /// `Sensitive` is not: the risk lands on somebody who never agreed to it.
    pub fn rope_in(&self, space: &Space, kind: Kind) -> Rope {
        if !space.is_business() {
            return self.rope_alone(space, kind);
        }
        let personally = self.rope_alone(&Space::Personal, kind);
        if personally == Rope::AskFirst {
            // Not yet trusted here at all. Nothing a business record can say
            // changes that, so it is not even consulted -- which also stops a
            // brand-new business being where Atlas quietly earns its first
            // licence.
            return Rope::AskFirst;
        }
        self.rope_alone(space, kind)
            .min(personally)
            .min(Rope::DoAndSay)
    }

    fn rope_alone(&self, space: &Space, kind: Kind) -> Rope {
        // Money and secrets are not on the table. Anything that reaches
        // another person is capped one step below the top for the same
        // reason: a wrong confident guess there costs somebody who never
        // agreed to the risk.
        let ceiling = match kind.cost_of_being_wrong() {
            5 => Rope::AskFirst,
            4 => Rope::DoAndSay,
            _ => Rope::JustDo,
        };

        let (good, total) = self.tally_in(space, kind);
        if total < ENOUGH {
            return Rope::AskFirst;
        }
        // Two recent mistakes drop it back regardless of the long record.
        if self.wrong_lately_in(space, kind) >= 2 {
            return Rope::AskFirst;
        }
        let rate = good as f32 / total as f32;
        let earned = if rate >= 0.95 {
            Rope::JustDo
        } else if rate >= 0.8 {
            Rope::DoAndSay
        } else {
            Rope::AskFirst
        };
        earned.min(ceiling)
    }

    /// May Atlas do this on its own?
    ///
    /// The question every caller actually has. It answers only this; what to
    /// do about the answer belongs to the caller.
    pub fn may_act_alone(&self, kind: Kind) -> bool {
        self.may_act_alone_in(&Space::Personal, kind)
    }

    pub fn may_act_alone_in(&self, space: &Space, kind: Kind) -> bool {
        self.rope_in(space, kind) != Rope::AskFirst
    }

    /// What is standing between this kind of work and more rope.
    ///
    /// The point of the whole module. "Not confident enough" with no reason is
    /// what makes a system feel arbitrary; this says exactly what would change
    /// it, so getting better is something Atlas can actually work at.
    pub fn what_would_earn_more(&self, kind: Kind) -> String {
        self.what_would_earn_more_in(&Space::Personal, kind)
    }

    pub fn what_would_earn_more_in(&self, space: &Space, kind: Kind) -> String {
        // The gate comes first, because when it is closed nothing about the
        // business's own record is the reason.
        if space.is_business() && self.rope_alone(&Space::Personal, kind) == Rope::AskFirst {
            return format!(
                "I still ask you first about {} on your own work, so I won't do \
                 it on my own for {} either.",
                kind.title().to_lowercase(),
                space.title()
            );
        }
        if space.is_business() && self.rope_in(space, kind) == Rope::DoAndSay {
            return format!(
                "I'll do this for {} and tell you straight away. Anything a \
                 partner sees gets said out loud — that part isn't something I \
                 can earn my way out of.",
                space.title()
            );
        }
        let (good, total) = self.tally_in(space, kind);
        if kind.cost_of_being_wrong() == 5 {
            return format!(
                "{} always asks first. That isn't something I can earn — it's \
                 yours to decide.",
                kind.title()
            );
        }
        if total < ENOUGH {
            return format!(
                "I've only done this {total} time{} — {} before there's a record \
                 rather than a coincidence.",
                if total == 1 { "" } else { "s" },
                ENOUGH - total
            );
        }
        let wrong = self.wrong_lately_in(space, kind);
        if wrong >= 2 {
            return format!(
                "I got the last {wrong} wrong. I'll ask first until that's not \
                 true any more."
            );
        }
        let rate = good as f32 / total as f32;
        match self.rope_in(space, kind) {
            Rope::JustDo => "Nothing — I get on with these.".into(),
            _ if rate < 0.8 => format!(
                "I'm right {} times in {total} here. Eight in ten is where I \
                 stop asking every time.",
                good
            ),
            _ => format!(
                "I'm right {} times in {total}. Nineteen in twenty is where I \
                 stop mentioning each one.",
                good
            ),
        }
    }

    /// The whole picture, for the hub.
    pub fn standing(&self) -> Vec<(Kind, Rope, usize, usize)> {
        self.standing_in(&Space::Personal)
    }

    pub fn standing_in(&self, space: &Space) -> Vec<(Kind, Rope, usize, usize)> {
        Kind::all()
            .into_iter()
            .map(|k| {
                let (good, total) = self.tally_in(space, k);
                (k, self.rope_in(space, k), good, total)
            })
            .collect()
    }
}

/// Which kind of work an intent is.
///
/// Built on `categories::category_of` rather than beside it, so there is one
/// answer to "what kind of thing is this" — a second, independent
/// classification is a pair that agrees on the day it is written and drifts
/// from there. The extra distinctions here are only the ones a track record
/// needs and consent does not: consent cares whether something leaves the
/// machine, this cares how expensive being wrong is.
pub fn kind_of(intent: &crate::intent::Intent) -> Kind {
    use crate::categories::Category;
    use crate::intent::Intent;

    // Money, accounts and secrets first, whatever category they fall in.
    match intent {
        Intent::Unlock(_) | Intent::CreateAccount(_) | Intent::SignIn(_) => {
            return Kind::Sensitive
        }
        Intent::Ask(_) | Intent::Why(_) => return Kind::Answering,
        Intent::WhatsThere | Intent::WhatsThis => return Kind::Answering,
        Intent::Files(_) | Intent::Show(_) => return Kind::Finding,
        Intent::Research(_) => return Kind::Answering,
        Intent::DraftPost(_) | Intent::ReviewPost(_) => return Kind::Drafting,
        Intent::WorkOnYourself(_) => return Kind::ChangingTheMachine,
        _ => {}
    }

    match crate::categories::category_of(intent) {
        Category::AgreementExternal => Kind::Sensitive,
        Category::ExternalAiCreative => Kind::ReachingOut,
        Category::StandardExternal => Kind::ReachingOut,
        Category::SelfModification => Kind::ChangingTheMachine,
        Category::LocalCreative => Kind::Drafting,
        Category::LocalOperational => Kind::Housekeeping,
    }
}

impl Record {
    /// The last thing recorded, taken back.
    ///
    /// Undo is the clearest statement that something was wrong, and the only
    /// one available without asking Eric to grade every action. It rewrites
    /// the outcome rather than adding a second one — otherwise a single
    /// mistake would appear twice, once as the act and once as the correction,
    /// and two entries out of a window of thirty is a real distortion.
    pub fn taken_back(&mut self) -> bool {
        match self.outcomes.last_mut() {
            Some(o) if o.good => {
                o.good = false;
                true
            }
            _ => false,
        }
    }
}
