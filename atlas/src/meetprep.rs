//! A short brief before a meeting: who's in it, what you last wrote to each
//! other, what you noted about them, and what's still open between you.
//!
//! Built from what's already on this machine -- the mail cache (`mailbook`),
//! your captured notes (`capture`) and the waiting-for list (`waitingfor`) --
//! so it costs nothing to keep up and nothing leaves the machine. Said once,
//! a quarter of an hour before, at a natural break (the same deferral every
//! offer goes through), and on request: "prep me for my next meeting".
//!
//! **Who's in it.** Calendar events here don't carry an attendee list (the
//! `.ics` import keeps summary, place and notes), so the people are read
//! from the title and notes: "call with Sam", "Sam / Priya sync", an email
//! address in the notes. A name the mail cache has never seen is still
//! listed, with "no mail with them in the last N days" -- an honest blank
//! rather than a guess.

use crate::mailbook::MailBook;

/// The people a meeting names, from its title and notes: whatever follows
/// "with" (split on "and", "&", ","), "A / B" and "A <> B" titles, and email
/// addresses anywhere.
pub fn people_in(title: &str, note: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '@' && c != '.').to_string();
        let low = s.to_lowercase();
        let filler = ["me", "the team", "team", "everyone", "all", "you", "us"];
        if s.len() >= 2 && !filler.contains(&low.as_str()) && !out.iter().any(|o| o.eq_ignore_ascii_case(&s)) {
            out.push(s);
        }
    };
    let low = title.to_ascii_lowercase();
    if let Some(i) = low.find(" with ") {
        let rest = &title[i + 6..];
        // Stop at what isn't a name: "about", "re", "for", "to discuss".
        let cut = ["about", " re ", " re:", " for ", " to ", " on ", "(", " - "]
            .iter()
            .filter_map(|m| rest.to_ascii_lowercase().find(m))
            .min()
            .unwrap_or(rest.len());
        for part in rest[..cut].split([',', '&']).flat_map(|p| p.split(" and ")) {
            push(part);
        }
    } else if title.contains(" / ") || title.contains(" <> ") || title.contains(" x ") {
        let sep = if title.contains(" / ") { " / " } else if title.contains(" <> ") { " <> " } else { " x " };
        for part in title.split(sep) {
            // "Sam / Priya sync": drop the trailing meeting word.
            let words: Vec<&str> = part.split_whitespace().collect();
            let keep: Vec<&str> = words.into_iter().filter(|w| !["sync", "1:1", "catch-up", "catchup", "call", "meeting", "chat"].contains(&w.to_lowercase().as_str())).collect();
            push(&keep.join(" "));
        }
    }
    for w in note.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '<' || c == '>') {
        if w.contains('@') && w.contains('.') {
            push(w);
        }
    }
    out
}

/// The brief's lines for one meeting.
pub fn prepare(
    title: &str,
    note: &str,
    book: &MailBook,
    notes: &[crate::capture::Note],
    waiting: &[crate::waitingfor::Waiting],
    look_back_days: u64,
    now: u64,
) -> Vec<String> {
    let people = people_in(title, note);
    let mut lines = Vec::new();
    if people.is_empty() {
        lines.push("I can't tell who's in it from the title -- \"call with Sam\" would tell me.".to_string());
    }
    for p in &people {
        let key = p.split('@').next().unwrap_or(p).split_whitespace().next().unwrap_or(p).to_lowercase();
        let recent = book.with(&key, 3);
        if recent.is_empty() {
            lines.push(format!("{p}: no mail with them in the last {look_back_days} days."));
        } else {
            let said: Vec<String> = recent
                .iter()
                .map(|l| {
                    let who = if l.mine { "you wrote" } else { "they wrote" };
                    let days = now.saturating_sub(l.at) / 86_400;
                    let when = match days {
                        0 => "today".to_string(),
                        1 => "yesterday".to_string(),
                        d => format!("{d} days ago"),
                    };
                    format!("{who} \"{}\" {when}", l.subject)
                })
                .collect();
            lines.push(format!("{p}: {}.", said.join("; ")));
        }
        for w in waiting.iter().filter(|w| w.with.to_lowercase().contains(&key)) {
            let side = match w.side {
                crate::waitingfor::Side::Owed => "they still owe you a reply on",
                crate::waitingfor::Side::Promised => "you said you'd",
            };
            lines.push(format!("  Open: {side} \"{}\" ({}).", w.subject, w.said));
        }
        let mentioned: Vec<&crate::capture::Note> = notes.iter().filter(|n| n.text.to_lowercase().contains(&key)).collect();
        for n in mentioned.iter().rev().take(2) {
            let flat: String = n.text.split_whitespace().collect::<Vec<_>>().join(" ");
            lines.push(format!("  Your note: {}", flat.chars().take(100).collect::<String>()));
        }
    }
    lines
}

/// The brief, said: "In 15 minutes: call with Sam. …".
pub fn said(title: &str, minutes_away: u64, lines: &[String]) -> String {
    let mut out = if minutes_away == 0 {
        format!("Now: {title}.")
    } else {
        format!("In {minutes_away} minute{}: {title}.", if minutes_away == 1 { "" } else { "s" })
    };
    for l in lines {
        out.push('\n');
        out.push_str(l);
    }
    out
}
