//! The pieces a card is built from.
//!
//! Moved out of `hub.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// A card that genuinely has nothing to show, saying so like a person.
///
/// Never an empty frame. An empty frame and a card whose data failed to load
/// look identical, and one of them is fine while the other is a bug.
pub fn nothing(said: &str) -> String {
    format!("<p class=nothing>{}</p>", esc(said))
}

/// A short list inside a card.
pub fn lines(items: &[String]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = String::from("<ul class=tight>");
    for i in items {
        out.push_str(&format!("<li>{}</li>", esc(i)));
    }
    out.push_str("</ul>");
    out
}

/// A command-deck list: a dot, the thing, and a quiet figure on the right.
pub fn rows(items: &[(Dot, String, String)]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = String::from("<ul class=rows>");
    for (dot, what, meta) in items {
        let class = match dot {
            Dot::Act => "dot",
            Dot::Proposed => "dot cool",
            Dot::Record => "dot mute",
        };
        out.push_str(&format!(
            "<li><span class='{class}'></span><span class=t>{}</span><span class=meta>{}</span></li>",
            esc(what),
            esc(meta)
        ));
    }
    out.push_str("</ul>");
    out
}

/// The big numbers at the top of Waiting on you: ember for what's yours to
/// act on, blue for what's proposed.
pub fn deck_figures(pairs: &[(&str, usize, Dot)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let mut out = String::from("<div class=figures>");
    for (label, n, dot) in pairs {
        let tone = match dot {
            Dot::Act => " warm",
            Dot::Proposed => " cool",
            Dot::Record => "",
        };
        out.push_str(&format!(
            "<div class='figure{tone}'><b>{n}</b><span>{}</span></div>",
            esc(label)
        ));
    }
    out.push_str("</div>");
    out
}

/// One measured thing as a ring: how much is used, the figure, and what it
/// is. The ring turns ember past three quarters and red past nine tenths.
pub fn gauge(figure: &str, what: &str, used: f32) -> String {
    let used = used.clamp(0.0, 1.0);
    // Circumference of r=15 is 94.2; the dash is the used share of it.
    let dash = (used * 94.2).round() as u32;
    let colour = if used >= 0.9 {
        "var(--hot)"
    } else if used >= 0.75 {
        "var(--accent)"
    } else {
        "var(--good)"
    };
    format!(
        "<div class=gauge><svg class=dial viewBox='0 0 36 36' aria-hidden=true>\
         <circle cx=18 cy=18 r=15 fill=none stroke='var(--edge)' stroke-width=3 />\
         <circle cx=18 cy=18 r=15 fill=none stroke='{colour}' stroke-width=3 stroke-linecap=round \
         stroke-dasharray='{dash} 95' transform='rotate(-90 18 18)' /></svg>\
         <div class=lab><b>{}</b><span>{}</span></div></div>",
        esc(figure),
        esc(what)
    )
}

/// The way out of a card into the full page behind it.
///
/// Summary first, detail on request. The card answers the question; the link
/// is there for the times the answer raises another one.
pub fn more(href: &str, label: &str) -> String {
    format!("<a class=more href='{}'>{}</a>", esc(href), esc(label))
}

/// Settings must be reachable when Atlas isn't working.
///
/// The whole point of a voice assistant is that you talk to it — which is no
/// help at all when the thing that's broken is the listening. So the hub runs
/// on its own: no microphone, no model, no daemon loop. Just the config, a
/// page, and the switches.
pub fn works_without_voice(page: Page) -> bool {
    // Access has to be reachable when Atlas is broken. If something's wrong,
    // taking access away is exactly what you'd want to do, and needing a
    // working assistant to do it would be the wrong way round.
    matches!(
        page,
        Page::Settings
            | Page::Permissions
            // Both are files on disk, and taking an add-on's permission away
            // is exactly what you'd want when something is misbehaving.
            | Page::AddOns
            | Page::Edits
            // Files on disk too; changes are sent to members when Atlas runs.
            | Page::Groups
            | Page::Status
            | Page::Accounts
            | Page::Access
            | Page::Connections
            // The day sync stops working is a day you may not be able to ask
            // out loud, and "make a new key" is the fix for the worst of it.
            | Page::Sync
            // Help has to open when nothing else works: it's where the
            // accessibility statement and the way to report a barrier live.
            | Page::Help
            | Page::Offline
    )
}

/// The page listing what Atlas can sign into.
///
/// One row per site, one button each, and one that takes everything at once.
pub fn access_page(rows: &[(String, String, String, bool)]) -> String {
    access_page_full(&[], &[], rows)
}

/// What Atlas can reach, what it deliberately cannot, and how to take each away.
///
/// The old version of this page took a list of sites and was never given one,
/// so it rendered "Nothing yet." on a machine where Atlas had a browser
/// session and a vault. Worse than empty: an access page that says nothing
/// reads as "nothing to worry about", which is the one thing an access page
/// must never do by accident.
///
/// `credentials.rs` had the honest answer written down the whole time —
/// what is held, what it opens, where it lives, and the exact words for
/// taking it back — and nothing ever called it.
pub fn access_page_full(
    held: &[&crate::credentials::Credential],
    misplaced: &[&crate::credentials::Credential],
    sites: &[(String, String, String, bool)],
) -> String {
    let mut body = String::from(
        "<p class=note>Everything Atlas can reach, and how you take it back. \
         Taking access away is immediate and doesn't change your password — \
         Atlas simply stops having it.</p>",
    );

    // Anything kept somewhere weaker than what it opens. First, because it is
    // the only thing on this page that is a problem rather than a fact.
    if !misplaced.is_empty() {
        body.push_str("<h2>Kept somewhere it shouldn't be</h2>");
        for c in misplaced {
            body.push_str(&format!(
                "<div class='rec'><h3>{}</h3><p class=cause>{}</p>\
                 <p class=evidence>{}</p></div>",
                esc(c.name),
                esc(&format!(
                    "This opens {}, and it is kept {}.",
                    opens_word(c.opens),
                    kept_word(c.kept)
                )),
                esc(c.revoke)
            ));
        }
    }

    body.push_str("<h2>What Atlas can reach</h2>");
    if held.is_empty() {
        body.push_str(&nothing(
            "Nothing. Atlas is holding no credentials and has no session it can use.",
        ));
    }
    for c in held {
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div>\
             <div class=what>For {}. Opens {}, kept {}.</div>\
             <div class=cost>To take it back: {}</div>{}</div>",
            esc(c.name),
            esc(c.used_for),
            esc(opens_word(c.opens)),
            esc(kept_word(c.kept)),
            esc(c.revoke),
            if c.needed_overnight {
                "<div class=what>Needed while you're asleep, so it can't sit \
                 behind a passphrase you have to type.</div>"
            } else {
                ""
            }
        ));
    }

    // Sites Atlas is actually signed into, when there are any. Separate from
    // the inventory above: that is what kind of thing it holds, this is which
    // particular doors are currently open.
    if !sites.is_empty() {
        body.push_str("<h2>Signed in right now</h2><table class=access>");
        for (name, detail, domain, stale) in sites {
            body.push_str(&format!(
                "<tr{}><td><b>{}</b><br><span class=note>{}</span></td>\
                 <td class=right><form method=post action=/hub/access/revoke>\
                 <input type=hidden name=domain value='{}'>\
                 <button class=revoke>Take it away</button></form></td></tr>",
                if *stale { " class=stale" } else { "" },
                esc(name),
                esc(detail),
                esc(domain)
            ));
        }
        body.push_str("</table>");
        body.push_str(
            "<form method=post action=/hub/access/revoke-all style='margin-top:18px'>\
             <button class=revoke>Take all of it away</button></form>",
        );
    }

    // The other half of the answer, and the half nobody writes down. "What
    // can it reach" is only reassuring next to "and what can it never".
    body.push_str("<h2>What Atlas never holds</h2>");
    for (what, why) in crate::credentials::never_held() {
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div><div class=what>{}</div></div>",
            esc(what),
            esc(why)
        ));
    }

    shell_at(Some(Page::Access), "Sites I can sign into", &body)
}

/// What a credential gets someone, said rather than named.
pub(super) fn opens_word(o: crate::credentials::Opens) -> &'static str {
    match o {
        crate::credentials::Opens::Nothing => "nothing on its own",
        crate::credentials::Opens::OneThing => "one narrow thing",
        crate::credentials::Opens::AService => "a whole service",
        crate::credentials::Opens::Everything => "the account and everything that resets through it",
    }
}

/// Where it lives, said rather than named.
pub(super) fn kept_word(k: crate::credentials::Kept) -> &'static str {
    match k {
        crate::credentials::Kept::NotHeld => "nowhere — you type it when it's needed",
        crate::credentials::Kept::YourBrowserSession => {
            "in the browser, as a session you created by signing in yourself"
        }
        crate::credentials::Kept::Vault => "in the locked vault",
        crate::credentials::Kept::PlainConfig => "in a config file, in plain text",
    }
}

pub fn route(path: &str) -> Option<Page> {
    route_any(path).filter(|p| p.here())
}

/// Every page's address, whatever device this is.
pub(super) fn route_any(path: &str) -> Option<Page> {
    // An anchor is the browser's business, not the server's. It arrives here
    // only from a link Atlas wrote itself — the palette's jumps into a
    // settings category — and letting it fall through to "no such page" would
    // make those links dead ends.
    let path = path.split('#').next().unwrap_or(path);
    match path.trim_end_matches('/') {
        "" | "/hub" => Some(Page::Dashboard),
        "/hub/status" => Some(Page::Status),
        "/hub/now" => Some(Page::Now),
        "/hub/gestures" => Some(Page::Gestures),
        "/hub/access" => Some(Page::Access),
        "/hub/connections" => Some(Page::Connections),
        "/hub/workspace" => Some(Page::Workspace),
        "/hub/workshop" => Some(Page::Workshop),
        "/hub/calendar" => Some(Page::Calendar),
        "/hub/back" => Some(Page::LookingBack),
        "/hub/recommendations" => Some(Page::Recommendations),
        "/hub/settings" => Some(Page::Settings),
        "/hub/permissions" => Some(Page::Permissions),
        "/hub/addons" => Some(Page::AddOns),
        "/hub/edits" => Some(Page::Edits),
        "/hub/friends" => Some(Page::Friends),
        "/hub/groups" => Some(Page::Groups),
        "/hub/accounts" => Some(Page::Accounts),
        "/hub/activity" => Some(Page::Activity),
        "/hub/outstanding" => Some(Page::Outstanding),
        "/hub/sync" => Some(Page::Sync),
        "/hub/messages" => Some(Page::Messages),
        "/hub/documents" => Some(Page::Documents),
        "/hub/business" => Some(Page::Business),
        "/hub/tasks" => Some(Page::SharedTasks),
        "/hub/clients" => Some(Page::Clients),
        "/hub/partners" => Some(Page::Partners),
        "/hub/sound" => Some(Page::Sound),
        "/hub/trusted" => Some(Page::Trusted),
        "/hub/give" => Some(Page::Give),
        "/hub/social" => Some(Page::Social),
        "/hub/offline" => Some(Page::Offline),
        "/hub/talk" => Some(Page::Talk),
        "/hub/help" => Some(Page::Help),
        "/hub/updates" => Some(Page::Updates),
        "/hub/phone" => Some(Page::Phone),
        "/hub/feedback" => Some(Page::Feedback),
        "/hub/opportunities" => Some(Page::Opportunities),
        _ => None,
    }
}
