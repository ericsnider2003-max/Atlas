//! The room's seats, signing in and up, mail hosts, undo.
//!
//! Moved out of `daemon.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// Ask every seat in one round and read back what each said. A seat that
/// couldn't be reached is left out; every call is noted for the record.
pub(super) fn ask_seats(
    llm: &dyn crate::brain::Llm,
    prompts: &[(String, String)],
    ctl: &crew::Control,
    calls: &mut Vec<SeatCall>,
    retests: bool,
) -> Vec<crate::council::Opinion> {
    let mut opinions = Vec::new();
    for (seat, prompt) in prompts {
        if ctl.checkpoint() {
            break;
        }
        let started = std::time::Instant::now();
        let answer = llm.complete("Answer as the seat you are given. Be brief.", prompt);
        // Graded from what the seat did with its brief: committed, or not;
        // in a retesting room, a no that says what would make it a yes.
        let grade = answer.as_ref().ok().map(|reply| {
            let o = crate::council::parse_opinion(seat, reply);
            if o.lean == crate::council::Lean::Depends && o.would_change_my_mind.is_none() {
                (false, "wouldn't commit".to_string())
            } else if retests && o.lean == crate::council::Lean::Against && o.would_change_my_mind.is_none() {
                (false, "said no without saying what would change it".to_string())
            } else {
                (true, String::new())
            }
        });
        calls.push(SeatCall {
            took_ms: started.elapsed().as_millis() as u64,
            prompt_chars: prompt.len(),
            reply_chars: answer.as_ref().map(|r| r.len()).unwrap_or(0),
            failed: answer.as_ref().err().map(|e| e.to_string()),
            grade,
            words: answer.as_ref().ok().map(|reply| crate::trace::Words {
                system: "Answer as the seat you are given. Be brief.".into(),
                user: prompt.clone(),
                reply: reply.clone(),
            }),
        });
        if let Ok(reply) = answer {
            opinions.push(crate::council::parse_opinion(seat, &reply));
        }
    }
    opinions
}

impl crate::brain::Llm for CountedLlm<'_> {
    fn complete(&self, system: &str, user: &str) -> crate::error::Result<String> {
        let started = std::time::Instant::now();
        let r = self.inner.complete(system, user);
        if let Ok(mut c) = self.calls.lock().or_else(crate::crash::unpoison) {
            c.push(SeatCall {
                took_ms: started.elapsed().as_millis() as u64,
                prompt_chars: system.len() + user.len(),
                reply_chars: r.as_ref().map(|t| t.len()).unwrap_or(0),
                failed: r.as_ref().err().map(|e| e.to_string()),
                grade: None,
                words: r.as_ref().ok().map(|reply| crate::trace::Words {
                    system: system.to_string(),
                    user: user.to_string(),
                    reply: reply.clone(),
                }),
            });
        }
        r
    }
}

/// The site in "turn off two-factor on github" / "…for my google account".
pub(super) fn site_named_in(said: &str) -> Option<String> {
    if let Some(d) = crate::enrol::domain_from(said) {
        return Some(d);
    }
    let words: Vec<&str> = said.split_whitespace().collect();
    let at = words.iter().rposition(|w| matches!(*w, "on" | "for" | "at"))?;
    // "turn on two factor" — the "on" is the phrase, not the site.
    let rest: Vec<&str> = words[at + 1..]
        .iter()
        .copied()
        .filter(|w| !matches!(*w, "my" | "the" | "account" | "accounts" | "please" | "two" | "factor" | "2fa" | "step"))
        .collect();
    let site = rest.join(" ");
    (!site.is_empty()).then_some(site)
}

/// The IMAP host for an account: the one set on it, or the provider's.
pub(super) fn mail_host(account: &crate::mail::Account) -> Option<String> {
    // Sorting moves and labels over IMAP itself; through Himalaya it isn't
    // offered (yet), so such an account is passed over here.
    if crate::himalaya::route(&account.imap_host).is_some() {
        return None;
    }
    if !account.imap_host.is_empty() {
        return Some(account.imap_host.clone());
    }
    crate::mail::Provider::from_address(&account.address).imap_host().map(str::to_string)
}

/// The action that takes back something Atlas did, from what was recorded.
pub fn undo_intent(what: &str) -> Option<Intent> {
    if what == "brought the workspace up" {
        return Some(Intent::WorkspaceOff);
    }
    if what == "put the workspace away" {
        return Some(Intent::WorkspaceOn);
    }
    if let Some(app) = what.strip_prefix("opened ") {
        return Some(Intent::CloseApp(app.to_string()));
    }
    if let Some(app) = what.strip_prefix("closed ") {
        return Some(Intent::OpenApp(app.to_string()));
    }
    if what.starts_with("switched to ") {
        return Some(Intent::SetMode("off".into()));
    }
    None
}
