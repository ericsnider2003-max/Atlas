//! Permission to touch an app.
//!
//! Three rules, from how you actually described it:
//!
//! 1. If Atlas doesn't know an app, it asks before using it.
//! 2. If you *tell* it to use something — "use Excel to build that sheet" —
//!    naming the app is the permission. It doesn't ask again for that.
//! 3. Some apps are confirm-every-time regardless. Discord can be interacted
//!    with, but each send is confirmed, because a wrong message there is
//!    public and permanent.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Span {
    /// This one action only.
    Once,
    /// Until Atlas restarts.
    Session,
    /// Remembered across restarts. Only ever from an explicit "always".
    Always,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Granted {
    pub app: String,
    /// None means any action on this app.
    pub action: Option<String>,
    pub span: Span,
    pub at: u64,
    #[serde(default)]
    pub used: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Go ahead, with the reason it was allowed.
    Allowed(String),
    /// Ask this question first.
    Ask(String),
}

impl Verdict {
    pub fn allowed(&self) -> bool {
        matches!(self, Verdict::Allowed(_))
    }
    pub fn message(&self) -> &str {
        match self {
            Verdict::Allowed(m) | Verdict::Ask(m) => m,
        }
    }
}

/// What Atlas already knows about an app, from config.
#[derive(Debug, Clone, Default)]
pub struct AppFacts {
    pub known: bool,
    /// Confirm every single interaction, however often approved.
    pub confirm_each_time: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Permissions {
    pub grants: Vec<Granted>,
}

impl Permissions {
    /// May Atlas do `action` in `app` right now?
    pub fn check(&self, app: &str, action: &str, facts: &AppFacts) -> Verdict {
        // Rule 3 first: some apps are always confirmed, and no accumulated
        // history changes that. A wrong Discord message is public.
        if facts.confirm_each_time {
            return Verdict::Ask(format!("Send that in {app}?"));
        }
        if let Some(g) = self.find(app, action) {
            let why = match g.span {
                Span::Once => format!("you asked for this specifically"),
                Span::Session => format!("you approved {app} earlier"),
                Span::Always => format!("{app} is always allowed"),
            };
            return Verdict::Allowed(why);
        }
        if !facts.known {
            return Verdict::Ask(format!("I don't know {app}. Use it for this?"));
        }
        Verdict::Allowed(format!("{app} is a configured app"))
    }

    fn find(&self, app: &str, action: &str) -> Option<&Granted> {
        self.grants.iter().find(|g| {
            g.app.eq_ignore_ascii_case(app)
                && g.action.as_ref().map(|a| a.eq_ignore_ascii_case(action)).unwrap_or(true)
                && !(g.span == Span::Once && g.used)
        })
    }

    pub fn grant(&mut self, app: &str, action: Option<&str>, span: Span, t: u64) {
        self.grants.retain(|g| !(g.app.eq_ignore_ascii_case(app) && g.span == Span::Once && g.used));
        self.grants.push(Granted {
            app: app.to_string(),
            action: action.map(str::to_string),
            span,
            at: t,
            used: false,
        });
    }

    /// A one-off grant is spent after use.
    pub fn consume(&mut self, app: &str, action: &str) {
        if let Some(g) = self.grants.iter_mut().find(|g| {
            g.app.eq_ignore_ascii_case(app)
                && g.span == Span::Once
                && g.action.as_ref().map(|a| a.eq_ignore_ascii_case(action)).unwrap_or(true)
        }) {
            g.used = true;
        }
    }

    pub fn revoke(&mut self, app: &str) {
        self.grants.retain(|g| !g.app.eq_ignore_ascii_case(app));
    }

    /// Drop session grants on restart. Only "always" survives.
    pub fn new_session(&mut self) {
        self.grants.retain(|g| g.span == Span::Always);
    }

    pub fn granted_apps(&self) -> Vec<String> {
        let mut v: Vec<String> = self.grants.iter().map(|g| g.app.clone()).collect();
        v.sort();
        v.dedup();
        v
    }
}

/// "Use Excel to build that sheet" — naming the tool *is* the permission.
///
/// Only counts when you name it as the instrument of the task. Merely
/// mentioning an app ("chrome keeps crashing") grants nothing.
pub fn grant_in_instruction(said: &str, known_apps: &[String]) -> Option<(String, Span)> {
    let t = said.to_lowercase();
    const LEADS: &[&str] = &["use ", "using ", "with ", "open up ", "go into ", "in "];

    // A refusal is not a grant, however the app is named. "don't use excel"
    // used to grant Excel, because nothing looked for the negation.
    if t.contains("don't ") || t.contains("dont ") || t.contains("do not ") || t.contains("never ") {
        return None;
    }

    for lead in LEADS {
        let mut from = 0usize;
        while let Some(i) = t[from..].find(lead) {
            let at = from + i;
            // The lead has to be a WORD, not a substring.
            //
            // `"because "` contains `"use "` at index 4, so
            // `grant_in_instruction("because chrome keeps crashing", &["chrome"])`
            // returned a grant for Chrome -- and this function's own doc two
            // lines up says *"Merely mentioning an app (\"chrome keeps
            // crashing\") grants nothing."* `"in "` is worse: it is inside
            // "certain ", "main ", "again "; and "misuse ", "cause " and
            // "paused " all contain `"use "`.
            let boundary = at == 0
                || !t[..at]
                    .chars()
                    .next_back()
                    .map(|c| c.is_alphanumeric())
                    .unwrap_or(false);
            let start = at + lead.len();
            if !boundary {
                from = start;
                continue;
            }
            let rest = &t[start..];
            let candidate: String = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .collect();
            if !candidate.is_empty() {
                // A named app in an instruction is a grant for this task.
                if known_apps.iter().any(|a| a.eq_ignore_ascii_case(&candidate)) {
                    return Some((candidate, Span::Once));
                }
                // Something Atlas doesn't know, but you named it deliberately.
                if *lead == "use " || *lead == "using " {
                    return Some((candidate, Span::Once));
                }
            }
            from = start;
        }
    }
    None
}

/// Did you say yes to a permission question, and how broadly?
///
/// ## This was upside down, and a refusal granted permanent permission
///
/// It read:
///
/// ```text
/// if !session::is_yes(&t) {
///     if t.contains("always")      { return Some(Span::Always) }
///     if t.contains("this session"){ return Some(Span::Session) }
///     return None;
/// }
/// Some(Span::Once)
/// ```
///
/// The intent is clear -- "yes, always" is a broad yes -- and the structure
/// is inverted. `session::is_yes` is a strict whitelist over the whole
/// answer, so "yes, always" is **not** a yes and falls into the *negative*
/// branch, which is what made the intended case work at all and is why the
/// existing test passed.
///
/// But every refusal fell there too:
///
/// * `"no, don't always do that"` -> `Some(Span::Always)`
/// * `"never, not this session"` -> `Some(Span::Session)`
///
/// `Span::Always` is the one `new_session` deliberately preserves across
/// restarts, so "no, don't always use Discord" would have granted Discord
/// permanent standing permission. Latent only because `Permissions::check`
/// has no production caller yet -- which makes it a trap set for whoever
/// wires it.
///
/// Now: a refusal is looked for **first**, and breadth is only read out of
/// an answer that is actually affirmative.
pub fn span_from_answer(said: &str) -> Option<Span> {
    let t = said.to_lowercase();

    // First, because a refusal that contains a breadth word is still a
    // refusal. Both an exact "no" and a negation anywhere in a longer
    // sentence -- "don't", "never", "do not", "stop" -- end it here.
    if crate::session::is_no(&t)
        || t.contains("don't")
        || t.contains("dont")
        || t.contains("do not")
        || t.contains("never")
        || t.contains("no thanks")
        // "not this session", "not always". A leading or space-separated
        // "not" only, so "another" and "cannot" as part of a longer word do
        // not trip it.
        || t.starts_with("not ")
        || t.contains(" not ")
    {
        return None;
    }

    let broad = t.contains("always") || t.contains("from now on") || t.contains("every time");
    let session = t.contains("this session") || t.contains("for now") || t.contains("just today");

    // A breadth word decides the span even inside a yes ("yes, always"):
    // `is_yes` takes "yes please" too since 30 Sep 2026, so it can't be
    // asked first.
    if crate::session::is_yes(&t) && !broad && !session {
        // A bare yes is the narrowest grant there is.
        return Some(Span::Once);
    }

    // Not a bare yes, not a refusal. Either something affirmative, or a
    // bare statement of scope -- because "for this session" is only a
    // sensible answer to "may I?" if the answer is yes, and answering with
    // the boundary is how people actually say a bounded yes.
    //
    // A bare "always" counts for the same reason. What does not count is
    // anything with a negation in it, which is why the refusal check above
    // comes first rather than being folded in here.
    let affirmative = t.starts_with("yes")
        || t.starts_with("yeah")
        || t.starts_with("sure")
        || t.starts_with("ok")
        || t.starts_with("okay")
        || t.contains("go ahead")
        || t.contains("please do")
        || broad
        || session;
    if !affirmative {
        return None;
    }
    if broad {
        return Some(Span::Always);
    }
    if session {
        return Some(Span::Session);
    }
    Some(Span::Once)
}
