//! Handing your laptop to someone else.
//!
//! The question this answers is not "who is at the machine" — Atlas cannot
//! know that, and the honest reasons are written down elsewhere in this tree:
//! a camera reading never stands in for you saying who you are, because a
//! photograph held to a webcam defeats face recognition, and `voiceid`'s own
//! doc says a recording of you sounds like you.
//!
//! So this answers a different question: **have you said you are handing it
//! over?** That is knowable, because you said it.
//!
//! # The asymmetry, which is the whole design
//!
//! Entering handover only ever *narrows* what Atlas will do. Leaving it
//! *grants*. Those are not the same act and must not be guarded the same way:
//!
//! * **Anyone may enter.** Your friend can say "guest mode" themselves. The
//!   worst a false entry costs is that Atlas is briefly less useful to you,
//!   and you can undo it in one sentence.
//! * **Only the passphrase leaves.** `vault.rs` has real crypto on every
//!   platform — argon2 over a passphrase, XChaCha20-Poly1305 over the
//!   contents. That is the one thing in this codebase that genuinely proves
//!   it is you, because it is something you know rather than something you
//!   sound or look like.
//!
//! The previous arrangement had this exactly backwards. Nothing could put
//! Atlas into a guest state at all, and `atlas profiles switch eric` — which
//! passes through no gate of any kind — took anyone straight out of one.
//!
//! # Why no sensor may do either
//!
//! A voice or a face reading is a **hint**, and hints are allowed to *ask*.
//! They are not allowed to decide, in either direction:
//!
//! * A sensor that could *enter* handover can lock you out of your own
//!   assistant. `voiceid::handle` returning `Ignore` on `NotYou` was already
//!   this: a cold, a new headset, or sitting further from the microphone and
//!   Atlas silently stops answering, with no error and nothing said. That is
//!   fixed at the source; nothing here reintroduces it.
//! * A sensor that could *leave* handover is worth spoofing. A photograph is
//!   cheap. A passphrase is not.
//!
//! So `Hint` exists, and the only thing it can produce is an offer.

use serde::{Deserialize, Serialize};

/// What Atlas will do for whoever is at the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    /// Normal. This is you.
    #[default]
    Yours,
    /// Somebody else has it, and you said so.
    HandedOver,
}

impl Stance {
    pub fn handed_over(&self) -> bool {
        matches!(self, Stance::HandedOver)
    }
}

/// The state, and how it got here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Handover {
    pub stance: Stance,
    /// When it was handed over. Kept so Atlas can say how long, which is the
    /// difference between "you're in guest mode" and "you've been in guest
    /// mode since Tuesday".
    pub since: Option<u64>,
    /// What you said it was for, when you said. Never required — a handover
    /// that insists on a reason is one you skip in the moment you need it.
    pub note: String,
    /// How many times the passphrase has been refused since the handover.
    /// Not a lockout: a count Atlas can mention to you afterwards.
    pub refused: u32,
}

/// What a sensor is allowed to contribute: a reason to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// A voice that did not match the enrolment.
    VoiceUnfamiliar,
    /// A face that is not yours, or faces with yours not among them.
    FaceUnfamiliar,
    /// Nothing to say.
    None,
}

impl Hint {
    /// The offer, in words. Never an action — see this module's doc.
    ///
    /// Phrased as a question with the benign reading first, because the
    /// benign reading is the likely one: far more often than a stranger, it
    /// is you with a cold, you on a different headset, or you sitting further
    /// back than usual.
    pub fn offer(&self) -> Option<&'static str> {
        match self {
            Hint::VoiceUnfamiliar => Some(
                "You sound different — a cold or a different microphone would do that, \
                 and so would this not being you. Say \"hand over\" if someone else has \
                 the laptop and I'll keep your things to myself.",
            ),
            Hint::FaceUnfamiliar => Some(
                "I don't recognise who's at the screen. If you've handed this over, say \
                 \"hand over\" and I'll keep your things to myself.",
            ),
            Hint::None => None,
        }
    }
}

/// Where the state is kept.
///
/// In the *install's* state, not the active person's: a handover has to
/// survive whatever else changes, and it is a fact about the machine rather
/// than about a person.
pub const FILE: &str = "handover";

impl Handover {
    pub fn load(install_state: &crate::store::Store) -> Handover {
        install_state.load(FILE)
    }

    pub fn save(&self, install_state: &crate::store::Store) -> crate::error::Result<()> {
        install_state.save(FILE, self)
    }

    /// Hand it over. Always allowed, from anywhere, by anyone.
    ///
    /// Returns what to say. Idempotent on purpose: saying it twice is a thing
    /// people do when they are unsure whether it took, and the second one
    /// must not read as a failure.
    pub fn hand_over(&mut self, note: &str, now: u64) -> String {
        if self.stance.handed_over() {
            return "Already handed over — I'm keeping the owner's things to myself.".into();
        }
        self.stance = Stance::HandedOver;
        self.since = Some(now);
        self.note = note.trim().to_string();
        self.refused = 0;
        "Handed over. I'll help with the ordinary things and keep the owner's to \
         myself. Say \"I'm back\" when it's yours again and I'll ask for the vault \
         passphrase in writing -- saying it out loud in front of somebody is the \
         one thing I won't take for it."
            .into()
    }

    /// Take it back. The passphrase is the only way, and the vault is what
    /// checks it.
    ///
    /// Takes an already-opened vault rather than a passphrase, so that this
    /// module never sees one and there is exactly one place in the codebase
    /// that verifies a secret.
    pub fn take_back(&mut self, vault: &crate::vault::Vault, now: u64) -> Result<String, String> {
        if !self.stance.handed_over() {
            return Ok("It was never handed over — this is yours already.".into());
        }
        // Two separate things, and conflating them is how this design would
        // have been decoration.
        //
        // `Vault::open` used to return `Ok` for *any* passphrase of twelve
        // characters or more -- it derived a key and never checked it. So
        // "the vault is open" meant "a key was derived", and resting a
        // security boundary on it would have meant a stranger typing twelve
        // characters took the laptop back. The vault verifies now, against a
        // sealed check value; this is what that fix was for.
        //
        // And a vault with no passphrase set cannot prove anything at all:
        // the first unlock *chooses* the passphrase rather than checking one.
        // Said plainly rather than silently accepted, because the silent
        // version is a lock that opens for everyone.
        //
        // `has_a_passphrase` is the wrong question to ask a vault that is
        // already open, and asking it was a bug of exactly the kind this
        // module is about: the first unlock writes the check value, so by the
        // time anyone looks, a vault that had no passphrase a moment ago
        // reports that it has one. `proved_it` is a fact about *this* unlock
        // and cannot be established by the unlock itself.
        if !vault.proved_it() {
            self.refused = self.refused.saturating_add(1);
            if vault.state() != crate::vault::State::Open {
                return Err(
                    "That's the one thing I can't take your word for. Unlock the vault and \
                     I'll know it's you."
                        .into(),
                );
            }
            return Err(
                "There's no passphrase on this vault yet, so unlocking it didn't prove \
                 anything -- the first unlock sets one. Set one \
                 while this is yours, and then a handover can be taken back."
                    .into(),
            );
        }
        let held = self.since.map(|s| now.saturating_sub(s));
        self.stance = Stance::Yours;
        self.since = None;
        let refused = self.refused;
        self.refused = 0;
        let mut said = match held {
            Some(secs) if secs >= 60 => format!("Yours again — that was {}.", plainly(secs)),
            _ => "Yours again.".to_string(),
        };
        if refused > 0 {
            // Said, not buried in a log. Somebody tried.
            said.push_str(&format!(
                " While it was handed over, the passphrase was tried and refused {refused} \
                 time{}.",
                if refused == 1 { "" } else { "s" }
            ));
        }
        self.note.clear();
        Ok(said)
    }

    /// How long it has been handed over, for a person.
    pub fn spoken(&self, now: u64) -> String {
        match (self.stance, self.since) {
            (Stance::Yours, _) => "This is yours — nothing is held back.".into(),
            (Stance::HandedOver, Some(s)) => {
                let what = if self.note.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", self.note)
                };
                format!(
                    "Handed over {} ago{what}. Say \"I'm back\"; \
                     it costs the vault passphrase, typed.",
                    plainly(now.saturating_sub(s))
                )
            }
            (Stance::HandedOver, None) => {
                "Handed over. Say \"I'm back\"; \
                 it costs the vault passphrase, typed."
                    .into()
            }
        }
    }
}

fn plainly(secs: u64) -> String {
    match secs {
        0..=90 => "a moment".to_string(),
        s if s < 3600 => format!("{} minutes", s / 60),
        s if s < 7200 => "an hour".to_string(),
        s if s < 86_400 => format!("{} hours", s / 3600),
        s if s < 172_800 => "a day".to_string(),
        s => format!("{} days", s / 86_400),
    }
}

/// What a handed-over Atlas will not do.
///
/// Two lists, both in `profiles.rs` so that the names live beside the role
/// that also uses one of them:
///
/// * `NEVER_AS_A_GUEST` — acting as you. Posting, mailing, signing in,
///   pairing, changing Atlas itself.
/// * `THE_OWNERS_OWN` — reading out or writing into what Atlas holds for
///   you. Your brief, your outstanding list, your notes, your history.
///
/// This was one list until it was tested against the situation it is for. A
/// guest *profile* has its own empty state, so reading their own outstanding
/// list back to them is the feature; a handed-over install has *your* state
/// live underneath, and the same action is a stranger reading your day. The
/// second list is that difference, and `profiles::THE_OWNERS_OWN` carries the
/// long version of the argument.
///
/// Every name in both is checked against `session::kind_of` by a test,
/// because a restriction naming an action that never arrives is not a weak
/// protection, it is none.
pub fn refuses(action: &str) -> bool {
    crate::profiles::NEVER_AS_A_GUEST.contains(&action)
        || crate::profiles::THE_OWNERS_OWN.contains(&action)
}

/// Would this `atlas vault` command hand the way back to whoever is holding
/// the laptop?
///
/// # The two-step escape this exists to stop
///
/// `take_back` refuses a vault with no passphrase, because on such a vault
/// the first unlock *chooses* a passphrase rather than checking one — so "the
/// vault is open" proves nothing about who opened it. That refusal was
/// reversible in one command. Whoever is holding the machine runs
/// `atlas vault passphrase`, sets the first passphrase, and then takes the
/// handover back using the thing they just invented. Both steps allowed,
/// protection gone.
///
/// Only the *first* passphrase is the problem. Changing one already costs the
/// current one, and blocking that would take away the thing you most want
/// available right after somebody has watched you type it.
///
/// This lives here, in the library, rather than as a condition inside
/// `main.rs`. That is not tidiness: a decision made inside the binary can
/// only be tested by reading the source as text, and a text guard cannot tell
/// a live condition from one somebody has turned off. This was found exactly
/// that way — the check was written in `run_vault`, a test asserted its
/// presence and position, and disabling the condition left every test green.
pub fn would_hand_out_the_way_back(
    handed_over: bool,
    has_a_passphrase: bool,
    command: Option<&str>,
) -> bool {
    if !handed_over || has_a_passphrase {
        return false;
    }
    matches!(
        command,
        Some("passphrase") | Some("set") | Some("change") | Some("recovery")
            | Some("recovery-key") | Some("newkey")
    )
}

/// What Atlas says instead. Addressed to whoever is holding the laptop, who
/// may have no idea there is an owner in the picture at all.
pub fn not_yours_to_set() -> String {
    "Not while this is handed over. Setting the first passphrase here would hand \
     out the one thing that takes the handover back, to whoever is holding this. \
     The owner can set it once this is theirs again."
        .to_string()
}

/// What Atlas says when it will not do something, while handed over.
///
/// Names the way back, because a refusal that does not is one you have to go
/// and look up — and the person holding the laptop is often not the person
/// who knows how.
pub fn refusal(action: &str) -> String {
    format!(
        "Not while this is handed over — {} is the owner's. Saying \"I'm back\", \
         with the vault passphrase, is the way out.",
        action.replace('_', " ")
    )
}

/// A sentence, with a failure to write the handover down said after it
/// rather than swallowed.
fn with_save(said: String, saved: crate::error::Result<()>) -> String {
    match saved {
        Ok(()) => said,
        Err(e) => format!("{said} (I couldn't write the handover down: {e})"),
    }
}

/// What is said when there is no passphrase to take a handover back with.
/// Where to set one is the caller's to add: a terminal and a page point to
/// different places.
pub const NO_PASSPHRASE_YET: &str =
    "There's no passphrase on this vault yet, so unlocking it wouldn't prove anything -- the \
     first unlock sets one.";

/// Take a handover back with a typed passphrase: the one sequence the
/// command line, "I'm back" and the hub's Accounts page all go through
/// (27 Sep 2026 — it was written out twice before, in `run_handover` and
/// `Daemon::take_it_back`, and a third copy for the hub is how the three
/// would have come to disagree).
///
/// A wrong passphrase is refused by the vault and counted by the handover,
/// and neither of them locks anyone out: being told "that isn't the
/// passphrase" twice is recoverable, and a lockout on your own machine is not.
///
/// The vault is locked **before** anything is checked, as well as after. An
/// owner who unlocked it earlier and then handed the machine over has left
/// it open; without the first lock a wrong passphrase would fail to open a
/// vault that was already open and proven, and `take_back` would read that
/// earlier proof as this one.
pub fn take_back_with(
    state: &crate::store::Store,
    vault: &mut crate::vault::Vault,
    phrase: &str,
    cfg: &crate::vault::VaultConfig,
    now: u64,
) -> String {
    let mut h = Handover::load(state);
    if !h.stance.handed_over() {
        return h.spoken(now);
    }
    if !vault.has_a_passphrase() {
        return format!("{NO_PASSPHRASE_YET} Set one while this is yours, and then a handover can be taken back.");
    }
    if phrase.is_empty() {
        return "Nothing typed — it's still handed over.".into();
    }
    vault.lock();
    if let Err(why) = vault.open(phrase, now, cfg) {
        vault.lock();
        crate::heard!(h.take_back(vault, now));
        return with_save(why, h.save(state));
    }
    let said = match h.take_back(vault, now) {
        Ok(said) => said,
        Err(why) => why,
    };
    let said = with_save(said, h.save(state));
    // Open only for as long as it took to prove who was typing.
    vault.lock();
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vault with a passphrase actually set, then re-opened with it.
    ///
    /// Two steps on purpose: the first `open` establishes the passphrase and
    /// the second verifies against it. A test that only did the first would
    /// be testing the case this module explicitly refuses.
    fn vault_open() -> crate::vault::Vault {
        let cfg = crate::vault::VaultConfig::default();
        let mut v = crate::vault::Vault::default();
        v.open("a long enough passphrase", 100, &cfg).expect("should set the passphrase");
        assert!(v.has_a_passphrase());
        v.lock();
        v.open("a long enough passphrase", 100, &cfg).expect("should verify");
        assert!(v.proved_it(), "the second open verified nothing");
        v
    }

    /// The right passphrase exists; the vault is simply shut.
    ///
    /// Not `Vault::default()` -- that is a vault with no passphrase at all,
    /// which is a different refusal with a different sentence, and using it
    /// here would have tested the wrong branch while reading as though it
    /// tested this one.
    fn vault_locked() -> crate::vault::Vault {
        let mut v = vault_open();
        v.lock();
        assert!(v.has_a_passphrase(), "the passphrase did not survive the lock");
        v
    }

    #[test]
    fn anyone_may_hand_it_over() {
        // No check of any kind. Entering only narrows, so guarding it would
        // buy nothing and would cost you the one moment you need it.
        let mut h = Handover::default();
        let said = h.hand_over("lending it to Sam", 100);
        assert!(h.stance.handed_over());
        assert!(said.contains("Handed over"));
        assert_eq!(h.note, "lending it to Sam");
    }

    #[test]
    fn saying_it_twice_is_not_a_failure() {
        let mut h = Handover::default();
        h.hand_over("", 100);
        let again = h.hand_over("", 110);
        assert!(again.contains("Already"), "the second one read as an error: {again}");
        assert_eq!(h.since, Some(100), "the clock restarted on a repeat");
    }

    #[test]
    fn only_the_passphrase_takes_it_back() {
        let mut h = Handover::default();
        h.hand_over("", 100);

        // A locked vault is a refusal, and it says what to do.
        let locked = vault_locked();
        let why = h.take_back(&locked, 200).unwrap_err();
        assert!(why.contains("Unlock the vault"), "the refusal does not say the way back: {why}");
        assert!(h.stance.handed_over(), "it was given back without the passphrase");

        // An open vault is proof.
        let said = h.take_back(&vault_open(), 200).unwrap();
        assert!(!h.stance.handed_over());
        assert!(said.contains("Yours again"), "{said}");
    }

    #[test]
    fn a_refused_attempt_is_counted_and_mentioned_afterwards() {
        // Not a lockout -- counting attempts and then locking is how you get
        // locked out of your own machine. It is told to you instead.
        let mut h = Handover::default();
        h.hand_over("", 100);
        let locked = vault_locked();
        assert!(h.take_back(&locked, 110).is_err());
        assert!(h.take_back(&locked, 120).is_err());
        assert_eq!(h.refused, 2);

        let said = h.take_back(&vault_open(), 130).unwrap();
        assert!(said.contains("refused 2 times"), "it never mentioned the attempts: {said}");
        assert_eq!(h.refused, 0, "the count was not cleared on the way out");
    }

    #[test]
    fn a_vault_with_no_passphrase_cannot_take_it_back() {
        // The edge that would have made this decoration. On a vault where no
        // passphrase has ever been set, the first unlock *chooses* one -- so
        // an "open" vault proves nothing, and a stranger would have taken the
        // laptop back by typing twelve characters.
        let mut h = Handover::default();
        h.hand_over("", 100);
        let mut fresh = crate::vault::Vault::default();
        fresh.open("twelve characters at least", 100, &crate::vault::VaultConfig::default()).unwrap();
        assert_eq!(fresh.state(), crate::vault::State::Open);

        let why = h.take_back(&fresh, 200).unwrap_err();
        assert!(why.contains("no passphrase"), "an unverifiable unlock was accepted: {why}");
        assert!(h.stance.handed_over());
    }

    #[test]
    fn taking_back_something_never_handed_over_is_not_an_error() {
        let mut h = Handover::default();
        let said = h.take_back(&crate::vault::Vault::default(), 100).unwrap();
        assert!(said.contains("never handed over"), "{said}");
    }

    #[test]
    fn a_hint_can_only_ever_offer() {
        // The rule the whole module turns on, asserted rather than trusted:
        // there is no method on `Hint` that changes a `Handover`, and the
        // only thing it produces is words.
        let mut h = Handover::default();
        for hint in [Hint::VoiceUnfamiliar, Hint::FaceUnfamiliar] {
            let offer = hint.offer().expect("a hint with nothing to say is not a hint");
            assert!(offer.contains("hand over"), "the offer does not name the way in: {offer}");
            assert!(!h.stance.handed_over(), "a sensor changed the stance");
        }
        assert!(Hint::None.offer().is_none());
        // And nothing above touched it.
        assert_eq!(h, Handover::default());
        h.hand_over("", 1);
        assert!(h.stance.handed_over(), "the only way in is saying so");
    }

    #[test]
    fn an_unfamiliar_voice_leads_with_the_likely_explanation() {
        // Far more often than a stranger, it is you with a cold. An offer
        // that opens by accusing the room is one you turn off.
        let offer = Hint::VoiceUnfamiliar.offer().unwrap();
        let cold = offer.find("cold").expect("it does not mention the ordinary reason");
        let stranger = offer.find("not being you").expect("it never says the other reason");
        assert!(cold < stranger, "it leads with the alarming reading: {offer}");
    }

    #[test]
    fn what_it_refuses_is_the_guest_list_and_not_a_second_one() {
        // Two lists of what a stranger may not do would drift, and the one
        // that drifted would be the one nobody read.
        for action in crate::profiles::NEVER_AS_A_GUEST {
            assert!(refuses(action), "{action} is guest-blocked and handover allows it");
        }
        assert!(!refuses("say"), "it refuses ordinary conversation");
        assert!(!refuses("research"), "it refuses looking something up");
    }

    #[test]
    fn a_refusal_names_the_way_back() {
        let said = refusal("draft_post");
        assert!(said.contains("draft post"), "it says the action in code: {said}");
        assert!(said.contains("vault"), "it does not say how to take it back: {said}");
        assert!(!said.contains('_'), "an identifier reached the sentence: {said}");
        // Said to whoever is holding the laptop, who may well not be the
        // owner and may not know anything about them.
        // Whole words -- "the owner" contains "he " and a substring check
        // here would be the tree's own standing caution all over again.
        let words: Vec<String> = said
            .split(|c: char| !c.is_alphanumeric())
            .map(|w| w.to_lowercase())
            .collect();
        for guess in ["his", "her", "hers", "he", "she", "him"] {
            assert!(
                !words.iter().any(|w| w == guess),
                "the refusal guesses at the owner: {said}"
            );
        }
    }

    #[test]
    fn it_says_how_long_rather_than_only_that_it_is_on() {
        let mut h = Handover::default();
        h.hand_over("Sam is borrowing it", 0);
        let said = h.spoken(3 * 3600);
        assert!(said.contains("3 hours"), "{said}");
        assert!(said.contains("Sam is borrowing it"), "the reason was dropped: {said}");
        assert!(Handover::default().spoken(100).contains("yours"));
    }
}
