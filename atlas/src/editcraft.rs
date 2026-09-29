//! Editing knowledge, taken from people who actually do it.
//!
//! Transcribed from the videos rather than guessed at, which matters: most
//! "editing tips" available to a machine are about software, and the ones that
//! change how a piece feels are about where the viewer's eye is and where the
//! camera was.
//!
//! Two ideas here are worth more than everything else combined.

use serde::{Deserialize, Serialize};

/// Where the subject sits in frame, as a fraction across.
///
/// 0.0 is hard left, 1.0 hard right.
pub type EyeLine = f32;

/// The idea most editors never articulate: **you are steering the viewer's
/// eye, and a cut that moves it across the frame breaks the illusion.**
///
/// Grid lines are usually used for safe zones. Used properly they tell you
/// where the eye is at the end of a shot, so the next shot can put its subject
/// in the same place and the cut disappears.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Cut {
    /// Where the eye is on the last frame before the cut.
    pub leaving_at: EyeLine,
    /// Where the subject is on the first frame after it.
    pub arriving_at: EyeLine,
}

impl Cut {
    /// How far the eye is asked to jump.
    fn jump(&self) -> f32 {
        (self.arriving_at - self.leaving_at).abs()
    }

    /// Does the cut disappear, or does the viewer notice it?
    ///
    /// Under about a fifth of the frame and the eye follows without noticing.
    /// Past a third it reads as a jump, however good the footage is.
    pub fn seamless(&self) -> bool {
        self.jump() < 0.2
    }

    pub fn note(&self) -> Option<String> {
        if self.seamless() {
            return None;
        }
        Some(format!(
            "the eye is at {:.0}% across when you cut and the next shot puts it at {:.0}% — \
             that's the jump you can feel. Reframe one of them and the cut vanishes.",
            self.leaving_at * 100.0,
            self.arriving_at * 100.0
        ))
    }
}

/// Check a whole sequence against your own limit (`editcraft.eye_jump_limit`)
/// rather than the built-in fifth of the frame.
pub fn check_cuts_within(cuts: &[Cut], limit: f32) -> Vec<(usize, String)> {
    cuts.iter()
        .enumerate()
        .filter(|(_, c)| c.jump() >= limit)
        .map(|(i, c)| {
            (i, format!(
                "the eye is at {:.0}% across when you cut and the next shot puts it at {:.0}% — a jump of {:.0}% of the frame",
                c.leaving_at * 100.0,
                c.arriving_at * 100.0,
                c.jump() * 100.0
            ))
        })
        .collect()
}

/// Check a whole sequence.
pub fn check_cuts(cuts: &[Cut]) -> Vec<(usize, String)> {
    cuts.iter()
        .enumerate()
        .filter_map(|(i, c)| c.note().map(|n| (i, n)))
        .collect()
}

/// Transitions done with the camera rather than in the edit.
///
/// The insight: the easiest way to level up editing isn't editing. Moving the
/// camera between two directions and turning your head into the second one
/// gives you a transition that costs nothing in post and doesn't date the way
/// a software effect does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    /// Camera moves A to B, you turn your head into B.
    Directional,
    /// Something passes close to the lens and hides the cut.
    Whip,
    /// A hand or object crosses the lens.
    Wipe,
    /// A software effect.
    InPost,
    /// A straight cut.
    Hard,
}

impl Transition {

    pub fn how(&self) -> &'static str {
        match self {
            Transition::Directional => {
                "set up facing one way, say the first half, turn your head the way you're going \
                 and move the camera to match. Cut on the turn"
            }
            Transition::Whip => "move something past the lens quickly and cut mid-blur",
            Transition::Wipe => "pass a hand or object across the lens and cut behind it",
            Transition::InPost => "a software effect — fine occasionally, dates fast",
            Transition::Hard => "just cut. Most cuts should be this",
        }
    }
}

/// Too many effects is the commonest sign of an editor who's learning.
pub fn too_many_effects(transitions: &[Transition], seconds: f32) -> Option<String> {
    let in_post = transitions.iter().filter(|t| **t == Transition::InPost).count();
    if in_post == 0 || seconds <= 0.0 {
        return None;
    }
    let per_minute = in_post as f32 / (seconds / 60.0);
    if per_minute > 6.0 {
        return Some(format!(
            "{in_post} software transitions in {:.0} seconds. Most of them could be a straight \
             cut, and two could be done in camera — which costs nothing and doesn't date.",
            seconds
        ));
    }
    None
}

// ---------- what a brand says, and what to say back ----------

/// The four things brands say, and the answer that holds your rate.
///
/// Transcribed rather than invented. The pattern underneath all four: never
/// move on the rate, move on the scope — because a rate you dropped once is
/// your rate forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrandAsks {
    /// "Send over your rate card."
    RateCard,
    /// "Can you do it for less?"
    Cheaper,
    /// "Our payment terms are net 60."
    LatePayment,
    /// "Full perpetual worldwide usage, budget is small."
    HugeRightsSmallBudget,
    /// Something else.
    Other,
}

pub fn what_they_asked(text: &str) -> BrandAsks {
    let t = text.to_lowercase();
    if t.contains("rate card") || t.contains("your rates") || t.contains("send over your rate") {
        return BrandAsks::RateCard;
    }
    if t.contains("perpetual") || t.contains("in perpetuity") || t.contains("worldwide usage") {
        return BrandAsks::HugeRightsSmallBudget;
    }
    if t.contains("net 30") || t.contains("net 60") || t.contains("net 90") || t.contains("payment terms") {
        return BrandAsks::LatePayment;
    }
    if t.contains("smaller brand") || t.contains("bit less") || t.contains("our budget is")
        || t.contains("lower rate")
    {
        return BrandAsks::Cheaper;
    }
    BrandAsks::Other
}

/// A reply that holds the line without closing the door.
///
/// Atlas drafts it. You send it — this is your name and your money.
pub fn reply_to(asked: BrandAsks) -> Option<&'static str> {
    Some(match asked {
        // Never quote before you know what they want. A number given without
        // scope is a number you'll be held to.
        BrandAsks::RateCard => {
            "Thanks for reaching out — I'd love to discuss this further so I can give you an \
             accurate quote. Could you send over usage rights, deliverables, budget and \
             timeline? Once I have those I'll come back to you straight away."
        }
        // Move the scope, never the rate.
        BrandAsks::Cheaper => {
            "I understand. I can't move on the rate, but I can adjust the scope — if we drop to \
             one deliverable instead of two and keep usage rights to six months, I can make that \
             work."
        }
        BrandAsks::LatePayment => {
            "I work on a 50/50 split: 50% up front before I start, and 50% on delivery."
        }
        // Perpetual worldwide rights are the expensive part, and the one
        // people give away without noticing.
        BrandAsks::HugeRightsSmallBudget => {
            "Would you be open to defining a usage term instead? That way you're fully covered \
             for the period you need, and you can always renew when it comes up."
        }
        BrandAsks::Other => return None,
    })
}

/// The rule underneath all of them.
pub const THE_PRINCIPLE: &str =
    "Move the scope, not the rate. A rate you dropped once is your rate forever, with that brand \
     and with anyone they talk to. Dropping a deliverable or shortening the usage term gets them \
     the number they wanted without doing that.";

/// The part that's usually given away for nothing.
pub const RIGHTS_ARE_THE_PRICE: &str =
    "Perpetual worldwide usage across every platform is the expensive part of any deal and the \
     part people hand over without noticing. A defined term costs them almost nothing and is \
     worth a great deal to you.";

/// Affiliate links and partnership deals are not the same thing.
///
/// Worth Atlas knowing because the words are used interchangeably and the
/// obligations are completely different.
pub const NOT_THE_SAME: &str =
    "An affiliate link is you recommending something you already use and taking a cut. A \
     partnership is a contract with deliverables. Being paid only in a commission code while \
     owing someone deliverables is the bad version of both.";

// ---------- spotting an affiliate deal that isn't one ----------

/// What a deal actually asks of you.
///
/// A real affiliate arrangement asks for nothing: you already use the thing,
/// you already recommend it, and a link earns you a cut. The moment a "deal"
/// starts adding obligations without adding money up front, it has stopped
/// being an affiliate arrangement and become a partnership you're doing for
/// free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DealTerms {
    /// You can't promote competitors.
    pub exclusive: bool,
    /// You owe them posts.
    pub deliverables: u32,
    /// Something has to live in your bio.
    pub bio_placement: bool,
    /// A minimum posting rate.
    pub posting_quota: bool,
    /// Money before you do anything.
    pub paid_up_front: bool,
    /// Commission rate, as a percentage.
    pub commission_pct: f32,
    /// You already own and use the thing.
    pub you_already_use_it: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhatItIs {
    /// No obligations, you get a cut. Take it.
    RealAffiliate,
    /// Obligations and money. A normal partnership — judge it on the money.
    RealPartnership,
    /// Obligations with no money. The one to walk away from.
    PartnershipDressedAsAffiliate,
}

/// Which of the three it is.
///
/// The line is simple and worth stating plainly: **obligations without money
/// up front is the bad one**, however the email describes it.
pub fn what_it_is(d: &DealTerms) -> WhatItIs {
    let obligations = d.exclusive || d.deliverables > 0 || d.bio_placement || d.posting_quota;
    match (obligations, d.paid_up_front) {
        (false, _) => WhatItIs::RealAffiliate,
        (true, true) => WhatItIs::RealPartnership,
        (true, false) => WhatItIs::PartnershipDressedAsAffiliate,
    }
}

/// What Atlas says about a deal.
pub fn judge_deal(d: &DealTerms) -> String {
    match what_it_is(d) {
        WhatItIs::RealAffiliate => {
            let mut s = format!("That's a genuine affiliate arrangement — {}% and nothing owed.", d.commission_pct);
            if d.you_already_use_it {
                // The whole argument for affiliates, and the answer to
                // "isn't that free labour".
                s.push_str(
                    " You already bought it and you'd recommend it anyway, so you're taking a cut \
                     of something you were doing for nothing.",
                );
            }
            s
        }
        WhatItIs::RealPartnership => {
            "That's a partnership, not an affiliate deal — there are deliverables and there's \
             money up front. Judge it on the money."
                .into()
        }
        WhatItIs::PartnershipDressedAsAffiliate => {
            let mut owed: Vec<&str> = Vec::new();
            if d.exclusive {
                owed.push("exclusivity");
            }
            if d.deliverables > 0 {
                owed.push("deliverables");
            }
            if d.bio_placement {
                owed.push("a bio placement");
            }
            if d.posting_quota {
                owed.push("a posting quota");
            }
            format!(
                "This is called an affiliate deal and isn't one. It wants {} and pays nothing up \
                 front — just {}% on sales. That's a partnership you'd be doing for free.",
                owed.join(", "),
                d.commission_pct
            )
        }
    }
}

/// The distinction, in one line.
pub const AFFILIATE_IS_NOT_EXCLUSIVE: &str =
    "An affiliate arrangement is not an exclusivity. If someone tells you that you can't promote \
     anything else in that space, or that you owe them a post a month, or that something has to \
     stay in your bio — that isn't an affiliate deal whatever they call it, and there's no money \
     up front to make it a partnership either.";

// ---------- the profile ladder ----------

/// Five things, in order, and each one is a level.
///
/// Ordered deliberately: a good bio under a bad name does less than a decent
/// name and no bio, because the name is what people see in a feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rung {
    /// A clean photo where your face is visible.
    Photo,
    /// Your name plus what you do — "Adam · fitness".
    NameAndNiche,
    /// First line says who you help.
    WhoYouHelp,
    /// Second line shows why they'd trust you. Results, proof, numbers.
    Proof,
    /// Last line says what to do next.
    OneThingToDo,
}

impl Rung {
    pub fn what(&self) -> &'static str {
        match self {
            Rung::Photo => "a clear photo where your face is actually visible",
            Rung::NameAndNiche => "your name plus what you do, so a feed tells people something",
            Rung::WhoYouHelp => "a first line that says exactly who this is for",
            Rung::Proof => "a second line showing why they'd trust you — results, numbers, proof",
            Rung::OneThingToDo => "a last line with one clear thing to do",
        }
    }
}

pub fn ladder() -> Vec<Rung> {
    vec![Rung::Photo, Rung::NameAndNiche, Rung::WhoYouHelp, Rung::Proof, Rung::OneThingToDo]
}

/// How far up you are, and the next rung.
///
/// Returns the first thing missing rather than a list, because a profile
/// review that hands you five changes gets none of them done.
pub fn next_rung(has: &[Rung]) -> Option<Rung> {
    ladder().into_iter().find(|r| !has.contains(r))
}

pub fn profile_note(has: &[Rung]) -> String {
    match next_rung(has) {
        None => "Your profile is doing everything it can.".into(),
        Some(r) => format!(
            "{} of 5. The next one is {}.",
            has.len(),
            r.what()
        ),
    }
}

// ---------- capturing rather than scheduling ----------

/// The difference between creators who grow and creators who don't isn't
/// quality — it's whether recording is a habit or an event.
///
/// Scheduling a shoot is friction, and friction is slow. Atlas can see this
/// in your own pattern: everything captured on two days a week is a schedule,
/// not a habit.
pub fn is_scheduling_rather_than_capturing(capture_days: &[u32]) -> Option<String> {
    if capture_days.len() < 14 {
        return None;
    }
    let active = capture_days.iter().filter(|n| **n > 0).count();
    let total: u32 = capture_days.iter().sum();
    if total < 10 {
        return None;
    }
    // Everything arriving on a couple of days means you sat down to make
    // content rather than caught it.
    if active * 4 <= capture_days.len() {
        return Some(format!(
            "Everything you've captured came on {active} of the last {} days. That's a shoot, \
             not a habit — the people who grow fastest record the moment they have the idea.",
            capture_days.len()
        ));
    }
    None
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EditCraftConfig {
    pub enabled: bool,
    /// Flag cuts where the eye jumps more than this fraction of the frame.
    pub eye_jump_limit: f32,
    /// Software transitions per minute above which it says something.
    pub effects_per_minute: f32,
    /// Atlas drafts brand replies; it never sends them.
    #[serde(skip, default = "never")]
    pub may_send_replies: bool,
}

fn never() -> bool {
    false
}

impl Default for EditCraftConfig {
    fn default() -> Self {
        EditCraftConfig {
            enabled: false,
            eye_jump_limit: 0.2,
            effects_per_minute: 6.0,
            may_send_replies: false,
        }
    }
}

/// A deal's terms, read from the email or what you said about it. Whatever
/// it doesn't say is taken as not asked for, and "you already use it" is only
/// what you said.
pub fn terms_from(text: &str) -> DealTerms {
    let l = text.to_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| l.contains(w));
    let pct = l
        .split(|c: char| c.is_whitespace() || c == '(' || c == ',')
        .find_map(|w| w.strip_suffix('%').and_then(|n| n.parse::<f32>().ok()))
        .unwrap_or(0.0);
    let deliverables = l
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|w| if w[1].starts_with("post") || w[1].starts_with("video") || w[1].starts_with("reel") { w[0].parse::<u32>().ok() } else { None })
        .unwrap_or(if has(&["deliverable", "a post a", "one post", "a video a"]) { 1 } else { 0 });
    DealTerms {
        exclusive: has(&["exclusive", "can't promote", "cannot promote", "not promote any other", "only promote", "competitor"]),
        deliverables,
        bio_placement: has(&["in your bio", "link in bio", "bio link"]),
        posting_quota: has(&["per month", "a month", "per week", "a week", "minimum of", "at least"]),
        paid_up_front: has(&["flat fee", "up front", "upfront", "paid in advance", "per post fee", "we'll pay you", "we will pay you"]),
        commission_pct: pct,
        you_already_use_it: has(&["i already use", "i use it", "i own it", "i already have"]),
    }
}

/// Which rungs of the profile ladder you said you have.
pub fn rungs_from(text: &str) -> Vec<Rung> {
    let l = text.to_lowercase();
    let mut has = Vec::new();
    if l.contains("photo") || l.contains("picture") {
        has.push(Rung::Photo);
    }
    if l.contains("name") || l.contains("niche") {
        has.push(Rung::NameAndNiche);
    }
    if l.contains("who i help") || l.contains("who it's for") || l.contains("who its for") {
        has.push(Rung::WhoYouHelp);
    }
    if l.contains("proof") || l.contains("results") {
        has.push(Rung::Proof);
    }
    if l.contains("call to action") || l.contains("one thing to do") || l.contains("link") {
        has.push(Rung::OneThingToDo);
    }
    has
}
