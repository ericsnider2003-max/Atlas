//! Doing something on a security page, with you there.
//!
//! You've scoped this tightly and the scope is what makes it workable: you're
//! at the machine, you say what you want, Atlas repeats it back in your own
//! terms, and nothing happens until you say yes.
//!
//! That's a genuinely different thing from a system that *can* change security
//! settings. The capability here is bounded by your presence, your instruction
//! and your confirmation, in that order, every time — and none of the three
//! can be assumed from the others.
//!
//! The read-back is the part that does the work. Not because you'd forget what
//! you asked, but because it's where a misheard instruction surfaces: "turn it
//! off on Instagram" and "turn it off on Instagram and Facebook" sound similar
//! and Instagram's settings page changes both.

use serde::{Deserialize, Serialize};

/// What you asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asked {
    pub site: String,
    pub account: String,
    pub change: Change,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Change {
    TurnOffTwoFactor,
    TurnOnTwoFactor,
    /// Move from one method to another.
    SwitchMethod { from: String, to: String },
    GenerateRecoveryCodes,
    /// Anything Atlas doesn't have a specific shape for.
    Other(String),
}

impl Change {
    /// In your words, for the read-back.
    pub fn plainly(&self) -> String {
        match self {
            Change::TurnOffTwoFactor => "turn two-factor off".into(),
            Change::TurnOnTwoFactor => "turn two-factor on".into(),
            Change::SwitchMethod { from, to } => format!("move from {from} to {to}"),
            Change::GenerateRecoveryCodes => "generate a new set of recovery codes".into(),
            Change::Other(what) => what.clone(),
        }
    }

    /// Does this leave the account less protected than it was?
    fn weakens(&self) -> bool {
        matches!(self, Change::TurnOffTwoFactor)
    }
}

/// Things worth saying in the read-back, because they're consequences of the
/// change that you might not have in mind.
pub fn consequence(site: &str, change: &Change) -> Option<String> {
    let s = site.to_lowercase();
    match change {
        Change::TurnOffTwoFactor => {
            if s.contains("instagram") || s.contains("facebook") {
                Some("Instagram and Facebook share this if the accounts are linked, so it comes \
                      off both.".into())
            } else if s.contains("github") {
                Some("This can also drop your SSH keys and tokens.".into())
            } else if s.contains("google") || s.contains("gmail") {
                Some("This is the account your others reset through.".into())
            } else {
                None
            }
        }
        Change::GenerateRecoveryCodes => {
            Some("Any codes you already printed stop working the moment these are made.".into())
        }
        Change::SwitchMethod { from, .. } if from.contains("text") => {
            Some("You'll need the new method working before the old one comes off.".into())
        }
        _ => None,
    }
}

/// The words on the control that makes this change, as sites label it.
///
/// Atlas presses a control only when exactly one visible one on the page
/// carries one of these words (`press_the_one`). Two matches is a page it
/// doesn't understand, and it hands the click back to you rather than pick.
pub fn labels_for(change: &Change) -> Vec<&'static str> {
    match change {
        Change::TurnOffTwoFactor => vec!["Turn off", "Turn Off", "Disable", "Turn off 2-Step Verification", "Turn off two-factor authentication"],
        Change::TurnOnTwoFactor => vec!["Turn on", "Turn On", "Enable", "Turn on 2-Step Verification", "Turn on two-factor authentication"],
        Change::GenerateRecoveryCodes => vec!["Get new codes", "Generate new codes", "Regenerate", "Generate new recovery codes"],
        // Switching method is several screens, and "anything else" has no
        // words Atlas can know in advance: those stay yours to click.
        Change::SwitchMethod { .. } | Change::Other(_) => Vec::new(),
    }
}

/// What came of Atlas trying to press the control itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Pressed {
    /// Exactly one control carried the words, and it was pressed.
    Done,
    /// No visible control carries any of the words.
    NotFound,
    /// Several did; Atlas won't choose between them.
    MoreThanOne(usize),
    /// The page wants your password first. That's always yours.
    WantsYourPassword,
    /// Atlas doesn't know the words for this change.
    NoWordsForIt,
    /// The page that loaded is a sign-in page. Signing in is yours.
    WantsYouToSignIn,
    /// The page that loaded belongs to a different site from the one you
    /// said yes to (a redirect, a look-alike). Nothing pressed.
    SomewhereElse(String),
}

impl Pressed {
    /// What to tell you, with where the page is when it's back to you.
    pub fn say(&self, asked: &Asked, url: &str) -> String {
        let yours = format!("It's yours from here: {url}");
        match self {
            Pressed::Done => format!(
                "Done: I pressed it to {} for {}. Have a look at the page — some sites ask you to confirm again. {url}",
                asked.change.plainly(),
                asked.site
            ),
            Pressed::WantsYouToSignIn => format!("The page wants you to sign in first — that's always yours. {yours}"),
            Pressed::SomewhereElse(host) => {
                format!("The page that opened is on {host}, not {}, so I didn't press anything. {yours}", asked.site)
            }
            Pressed::NotFound => format!("I couldn't find the control on that page. {yours}"),
            Pressed::MoreThanOne(n) => format!("There were {n} controls that could be it, and I won't guess. {yours}"),
            Pressed::WantsYourPassword => format!("The page wants your password first — I never type that. {yours}"),
            Pressed::NoWordsForIt => format!("I don't know which control does that on {}. {yours}", asked.site),
        }
    }
}

/// The page script: count the visible controls carrying one of `words`
/// (exactly, ignoring case and spacing), press it if there's exactly one,
/// and say what happened. A visible password box stops it first.
pub fn press_js(words: &[&str]) -> String {
    let list = words
        .iter()
        .map(|w| format!("'{}'", crate::cdp::js_str(&w.to_lowercase())))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "(() => {{
          const seen = e => {{ const r = e.getBoundingClientRect(); const s = getComputedStyle(e);
            return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; }};
          if (Array.from(document.querySelectorAll('input[type=password]')).some(seen)) return 'password';
          const words = [{list}];
          const said = e => [e.innerText, e.value, e.getAttribute('aria-label')]
            .filter(Boolean).map(t => t.replace(/\\s+/g, ' ').trim().toLowerCase());
          const hits = Array.from(document.querySelectorAll(
              'button, a, [role=button], [role=switch], [role=menuitem], input[type=button], input[type=submit], input[type=checkbox]'))
            .filter(seen).filter(e => said(e).some(t => words.includes(t)));
          if (hits.length === 0) return 'none';
          if (hits.length > 1) return 'many:' + hits.length;
          hits[0].scrollIntoView({{block:'center'}}); hits[0].click(); return 'pressed';
        }})()"
    )
}

/// Read what `press_js` returned.
pub fn pressed_from(result: &str) -> Pressed {
    match result {
        "pressed" => Pressed::Done,
        "password" => Pressed::WantsYourPassword,
        r if r.starts_with("many:") => Pressed::MoreThanOne(r[5..].parse().unwrap_or(2)),
        _ => Pressed::NotFound,
    }
}

/// Where a request is up to.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Say this back and wait.
    ReadBack { say: String },
    /// You said yes. Do it.
    Go { asked: Asked },
    /// You said no.
    Dropped,
    /// You said something that isn't yes or no.
    Unclear { say: String },
    /// Can't — and why.
    Cannot(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConfirmConfig {
    pub enabled: bool,
    /// Read back every change, not just the weakening ones.
    ///
    /// On by default: "turn it on" misheard as "turn it off" is the failure
    /// this catches, and it goes both ways.
    pub read_back_everything: bool,
    // Two `#[serde(skip)]` bools were deleted here on 19 Sep 2026, when this
    // module was wired. Both stated something true, and neither was what made
    // it true — a bool nobody reads is not a boundary.
    //
    // `requires_you_present`: `read_back` refuses outright when you are not
    // at the machine, and the walkthrough path is a command you just typed.
    // The refusal is the boundary; the field was a note beside it.
    //
    // `one_at_a_time`: `Run` cannot express a batch. `current` returns one
    // `Asked`, `record` and `skip` each advance by one, and there is no way
    // in to answer for several — so a single yes covering six security
    // changes is not something a caller could ask for and be refused, it is
    // something no caller can say. That is a stronger guarantee than a
    // switch, and `tests/reading_it_back.rs` asserts it where it lives.
}

impl Default for ConfirmConfig {
    fn default() -> Self {
        ConfirmConfig {
            enabled: false,
            read_back_everything: true,
        }
    }
}

/// Does this one need saying back to you before it happens?
///
/// `read_back_everything` ships on — "turn it on" misheard as "turn it off"
/// is the failure this catches, and it goes both ways — and nothing read it
/// until 19 Sep 2026, because nothing in the running program ever built an
/// `Asked`. With it off, only the changes that leave an account less
/// protected than it is now are read back.
pub fn needs_reading_back(a: &Asked, cfg: &ConfirmConfig) -> bool {
    cfg.enabled && (cfg.read_back_everything || a.change.weakens())
}

/// The read-back itself, without the preconditions for Atlas doing it.
///
/// `read_back` also checks that the vault is open, because it was written for
/// Atlas making the change itself, and that needs a credential. A walkthrough
/// is *you* making it, with Atlas saying back what you asked for first — the
/// mishearing it catches is the same one, and the vault has nothing to do
/// with it. Passing `true` for a vault nobody opened would be a lie in the
/// shape of a precondition, which is worse than not checking.
pub fn saying_it_back(asked: &Asked) -> String {
    let account = if asked.account.is_empty() {
        String::new()
    } else {
        format!(" on {}", asked.account)
    };

    let mut say = format!("So: {} for {}{}.", asked.change.plainly(), asked.site, account);
    if let Some(c) = consequence(&asked.site, &asked.change) {
        say.push_str(&format!(" {c}"));
    }
    say.push_str(" Yes or no?");
    say
}

/// Turn a request into a read-back.
pub fn read_back(asked: &Asked, you_are_here: bool, vault_open: bool, cfg: &ConfirmConfig) -> Step {
    if !cfg.enabled {
        return Step::Cannot("changing security settings is switched off".into());
    }
    if !you_are_here {
        return Step::Cannot(
            "this only happens with you at the machine — it's the whole reason it's allowed at \
             all"
                .into(),
        );
    }
    if !vault_open {
        return Step::Cannot("I'd need the vault open to sign in first".into());
    }
    Step::ReadBack { say: saying_it_back(asked) }
}

/// Your answer.
pub fn answer(said: &str, asked: &Asked) -> Step {
    let t = said.trim().to_lowercase();
    let yes = ["yes", "yep", "yeah", "do it", "go ahead", "correct", "right", "confirm", "go"]
        .iter()
        .any(|w| t == *w || t.starts_with(&format!("{w} ")));
    let cleaned = t.trim_end_matches(['.', '!']).to_string();
    let no = ["no", "nope", "stop", "cancel", "don't", "dont", "wait", "hold on", "not that"]
        .iter()
        .any(|w| {
            cleaned == *w
                || cleaned.starts_with(&format!("{w} "))
                // "no, not that one" — the comma is why a plain prefix check
                // misses it.
                || cleaned.starts_with(&format!("{w},"))
        });

    if yes && !no {
        Step::Go { asked: asked.clone() }
    } else if no {
        Step::Dropped
    } else {
        // Anything ambiguous is a no for now, not a yes.
        Step::Unclear {
            say: "I need a yes or a no on that one.".into(),
        }
    }
}

/// A run of several, confirmed one at a time.
///
/// You can pick six accounts in the panel, but you say yes six times. A single
/// yes covering six security changes is how the wrong one gets made — and
/// you'd have no way to tell which.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub items: Vec<Asked>,
    pub at: usize,
    pub done: Vec<(String, bool)>,
}

impl Run {
    pub fn new(items: Vec<Asked>) -> Run {
        Run { items, at: 0, done: Vec::new() }
    }

    pub fn current(&self) -> Option<&Asked> {
        self.items.get(self.at)
    }

    pub fn record(&mut self, worked: bool) {
        if let Some(a) = self.items.get(self.at) {
            self.done.push((format!("{} — {}", a.site, a.change.plainly()), worked));
        }
        self.at += 1;
    }

    pub fn skip(&mut self) {
        self.at += 1;
    }

    pub fn finished(&self) -> bool {
        self.at >= self.items.len()
    }

    /// What Atlas says at the end.
    pub fn summary(&self) -> String {
        let ok = self.done.iter().filter(|(_, w)| *w).count();
        let failed: Vec<&str> = self
            .done
            .iter()
            .filter(|(_, w)| !*w)
            .map(|(n, _)| n.as_str())
            .collect();
        let skipped = self.items.len() - self.done.len();

        let mut s = format!("{ok} done");
        if !failed.is_empty() {
            s.push_str(&format!(", {} didn't work: {}", failed.len(), failed.join(", ")));
        }
        if skipped > 0 {
            s.push_str(&format!(", {skipped} skipped"));
        }
        s.push('.');
        s
    }
}

/// What Atlas says before starting a run of them.
pub fn before_a_run(items: &[Asked]) -> String {
    let weakening = items.iter().filter(|a| a.change.weakens()).count();
    let mut s = format!("{} changes, one at a time — I'll read each one back.", items.len());
    if weakening > 0 {
        s.push_str(&format!(
            " {weakening} of them leave the account less protected than it is now."
        ));
    }
    s
}

/// After the fact, so there's a record of what was changed and when.
///
/// Security changes are the ones you most want a trail of, and the ones people
/// least often have one for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub site: String,
    pub account: String,
    pub what: String,
    pub at: u64,
    pub worked: bool,
    /// What you said to confirm it.
    pub you_said: String,
}

pub fn record(asked: &Asked, worked: bool, you_said: &str, now: u64) -> Record {
    Record {
        site: asked.site.clone(),
        account: asked.account.clone(),
        what: asked.change.plainly(),
        at: now,
        worked,
        you_said: you_said.to_string(),
    }
}

/// Undoing it later, which is the question you'll have and won't remember.
pub fn how_to_undo(change: &Change) -> String {
    match change {
        Change::TurnOffTwoFactor => {
            "Turning it back on is the same page — and you'll need whatever method you pick to \
             hand when you do."
                .into()
        }
        Change::TurnOnTwoFactor => "Same page to turn it off again.".into(),
        Change::SwitchMethod { from, to } => format!("Switching back from {to} to {from} is the same page."),
        Change::GenerateRecoveryCodes => {
            "The old set is gone. Generating again replaces these too.".into()
        }
        Change::Other(_) => "Same page.".into(),
    }
}

/// The last two parts of a host name: "myaccount.google.com" → "google.com".
/// Enough to tell one company's site from another's; a country domain like
/// "bbc.co.uk" keeps three.
pub fn site_of(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_lowercase();
    let host = host.split(':').next().unwrap_or("").to_string();
    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    let keep = if parts.len() >= 3 && parts[parts.len() - 2].len() <= 3 && parts[parts.len() - 1].len() == 2 { 3 } else { 2 };
    parts[parts.len().saturating_sub(keep)..].join(".")
}

/// The host part of a URL.
pub fn host_of(url: &str) -> String {
    // No "://" (a data: or about: page): no host.
    let Some((_, after)) = url.split_once("://") else { return String::new() };
    after.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("").to_lowercase()
}

/// Before pressing: is the page on the site you said yes to, and not a
/// sign-in page? `None` means go ahead. A page with no host (a local test
/// page) is only accepted when the expected URL has none either.
pub fn before_pressing(expected_url: &str, page_host: &str) -> Option<Pressed> {
    let want = host_of(expected_url);
    let got = page_host.trim().to_lowercase();
    if want.is_empty() && got.is_empty() {
        return None;
    }
    const SIGN_IN: [&str; 6] = ["accounts.google.com", "login.microsoftonline.com", "login.live.com", "appleid.apple.com", "idmsa.apple.com", "login.yahoo.com"];
    let first = got.split('.').next().unwrap_or("");
    if SIGN_IN.contains(&got.as_str()) || matches!(first, "login" | "signin" | "sign-in" | "auth" | "sso") {
        return Some(Pressed::WantsYouToSignIn);
    }
    if site_of(&want) != site_of(&got) {
        return Some(Pressed::SomewhereElse(if got.is_empty() { "a page with no address".into() } else { got }));
    }
    None
}
