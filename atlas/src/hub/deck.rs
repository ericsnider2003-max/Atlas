//! The dashboard deck and its cards.
//!
//! Moved out of `hub.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Deck {
    /// What Home says when there is no running Atlas to ask — the
    /// settings-only hub, and pages rendered for a test.
    pub fn not_running() -> Deck {
        Deck {
            greeting: "Hello.".into(),
            status: "Not running".into(),
            tone: "off",
            now: "Atlas isn't running, so nothing is underway.".into(),
            now_sub: "Start it from the Atlas window, and this fills in with your day.".into(),
            spine: Vec::new(),
            brief: "Atlas isn't running, so there's no brief yet. Start it from the Atlas window and this fills in.".into(),
            asks: Vec::new(),
            businesses: Vec::new(),
            first_run: false,
        }
    }
}

/// A status as an icon and a word — the design's rule: never colour alone.
pub(super) fn pill(kind: &str, word: &str) -> String {
    let (class, paths) = match kind {
        "done" => ("ok", "<path d='M5 12l5 5L20 6'/>"),
        "working" => ("wait", "<circle cx=12 cy=12 r=9 /><path d='M12 8v4l2.5 1.5'/>"),
        "waiting" => ("wait", "<path d='M12 8v4M12 16h.01'/><circle cx=12 cy=12 r=9 />"),
        "next" => ("info", "<rect x=3 y=4.5 width=18 height=16.5 rx=2.5 /><path d='M3 9.5h18'/>"),
        "stopped" => ("stop", "<path d='M12 8v4M12 16h.01'/><circle cx=12 cy=12 r=9 />"),
        "online" => ("ok", "<path d='M12 3a9 9 0 1 0 9 9'/><path d='M8 12l3 3 5-6'/>"),
        _ => ("", "<circle cx=12 cy=12 r=9 />"),
    };
    format!(
        "<span class='pill {class}'><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2.4 aria-hidden=true>{paths}</svg>{}</span>",
        esc(word)
    )
}

/// Today, in order, each moment with its state as an icon and a word.
pub(super) fn today_html(d: &Deck) -> String {
    let mut rows = String::new();
    for (time, what, mark) in &d.spine {
        let (class, p) = match mark {
            Mark::Done => ("day done", pill("done", "Done")),
            Mark::Now => ("day now", pill("working", "Working")),
            Mark::Later => ("day", pill("next", "Next")),
        };
        rows.push_str(&format!(
            "<div class='{class}'><time>{}</time>{p}<span class=what>{}</span></div>",
            esc(time),
            esc(what)
        ));
    }
    if rows.is_empty() {
        rows.push_str("<p class=nothing>Nothing on your calendar today, and nothing done yet.</p>");
    }
    format!("<section class=today><h2>Today</h2><div class=days>{rows}</div></section>")
}

/// Right now, beside Today when there's no business to show: what Atlas is
/// doing, and the box to ask or find anything.
pub(super) fn right_now_html(d: &Deck) -> String {
    format!(
        "<section class=rightnow><h2>Right now</h2>\
         <p class=now>{now}</p><p class=nowsub>{sub}</p>\
         <form class=ask method=get action='/hub/find'>\
         <input name=q autocomplete=off placeholder='Find anything, or jump to a page…' aria-label='Find anything'>\
         <button type=submit aria-label=Find><span class=kbd>Enter</span></button></form></section>",
        now = esc(&d.now),
        sub = esc(&d.now_sub),
    )
}

/// Business at a glance: each business, its open work, and its people.
pub(super) fn glance_html(g: &Glance) -> String {
    let mut people = String::new();
    for (name, what) in &g.people {
        let initials: String = name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase();
        people.push_str(&format!(
            "<div class=who><span class=av>{}</span><span>{}</span><span class=meta>{}</span></div>",
            esc(&initials),
            esc(name),
            esc(what)
        ));
    }
    format!(
        "<section class=glance><h2><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>\
         <path d='M3 21V7l9-4 9 4v14'/></svg>{name}</h2>\
         <div class=tiles><div class=tile><b>{open}</b><span>open</span></div>\
         <div class=tile><b>{n}</b><span>people</span></div></div>{people}\
         <a class=more href='{board}'>Shared tasks</a></section>",
        name = esc(&g.name),
        open = g.open,
        n = g.people.len(),
        board = Page::Workspace.href(),
    )
}

/// The Brief: Atlas's summary in its own words, with what's waiting on you
/// inside it and the first thing to do as a button.
pub(super) fn brief_html(d: &Deck) -> String {
    let mut asks = String::new();
    if !d.asks.is_empty() {
        asks.push_str("<ul class=asks>");
        for (what, href) in d.asks.iter().take(5) {
            asks.push_str(&format!("<li><a href='{}'>{}</a></li>", esc(href), esc(what)));
        }
        if d.asks.len() > 5 {
            asks.push_str(&format!(
                "<li><a href='{}'>{} more</a></li>",
                Page::Outstanding.href(),
                d.asks.len() - 5
            ));
        }
        asks.push_str("</ul>");
    }
    let act = match d.asks.first() {
        Some((_, href)) => format!(
            "<div class=acts><a class='btn primary' href='{}'>Start with the first</a>\
             <a class=btn href='{}'>See everything open</a></div>",
            esc(href),
            Page::Outstanding.href()
        ),
        None => String::new(),
    };
    format!(
        "<section class=brief aria-label='Your brief'>{MARK}<div><p>{}</p>{asks}{act}</div></section>",
        esc(&d.brief)
    )
}

/// First run: calm, not blank. A few guided steps, each skippable, all of
/// which work offline.
pub(super) fn first_run_html(d: &Deck) -> String {
    if crate::phonemode::on() {
        return first_run_on_the_phone(d);
    }
    let connect = format!("{}#connect", Page::Accounts.href());
    format!(
        "<section class=brief aria-label='Welcome'>{MARK}<div><p><b>{greet}</b> This is home. It fills in as you go — \
         your brief, your day, what I'm working on. Nothing here yet, so let's give it a little to work with. \
         You can skip any of these and come back; I run fine offline the whole time.</p></div></section>\
         <ol class=firststeps>\
         <li><b>Connect your email</b><span>Type your address and I'll walk you through it -- for most providers that's one app password, tried before it's kept.</span>\
         <a class=btn href='{acc}'>Connect your email</a></li>\
         <li><b>Bring in your calendar</b><span>Paste your Google, Outlook or iCloud calendar's private link and I'll keep reading it.</span>\
         <a class=btn href='{cal}'>Add a calendar</a></li>\
         <li><b>Hand me something to look at</b><span>A file, a link, a photo — any type, any size.</span>\
         <a class=btn href='{give}'>Give Atlas a file</a></li>\
         </ol><p class=note>Or just tell me what you need — say “Atlas”, or type in the bar. I'll take it from there.</p>",
        greet = esc(&d.greeting),
        // Each goes where the thing is actually done (27 Sep 2026: "Connect"
        // opened the calendar, "Add" opened site security, and "Give Atlas a
        // file" opened search).
        cal = connect,
        acc = connect,
        give = Page::Give.href(),
    )
}

/// First run on the phone app: what works here, on this phone, with
/// nothing else set up -- no laptop, no accounts (the TestFlight review
/// audit, 2 Oct 2026: the laptop's steps sent a phone to pages about adding
/// a phone).
pub(super) fn first_run_on_the_phone(d: &Deck) -> String {
    format!(
        "<section class=brief aria-label='Welcome'>{MARK}<div><p><b>{greet}</b> This is home. It fills in as you go -- \
         your reminders, notes and what I'm working on. Everything you tell me is kept on this phone.</p></div></section>\
         <ol class=firststeps>\
         <li><b>Talk to me</b><span>Tap the microphone, or type. Try \u{201c}remind me to call Mum at 6\u{201d} or \u{201c}note that the wifi password is on the fridge\u{201d}.</span>\
         <a class=btn href='{talk}'>Start talking</a></li>\
         <li><b>Hand me something to look at</b><span>A file, a link or a photo.</span>\
         <a class=btn href='{give}'>Give Atlas a file</a></li>\
         <li><b>Bring in your calendar</b><span>So your day here matches the phone's. The phone asks you first; it stays on this phone.</span>\
         <button class=btn type=button onclick='window.AtlasShell&&AtlasShell.calendar&&AtlasShell.calendar()'>Bring in your calendar</button></li>\
         <li><b>Choose what I'm allowed to do</b><span>Every switch, in plain words. Nothing is on that you didn't choose.</span>\
         <a class=btn href='{set}'>Open Settings</a></li>\
         </ol>",
        greet = esc(&d.greeting),
        talk = Page::Talk.href(),
        give = Page::Give.href(),
        set = Page::Settings.href(),
    )
}

/// Home, as the design draws it: the greeting, the Brief with what's waiting
/// on you inside it, Today beside Right now (or your businesses at a glance),
/// then the cards you arranged.
pub fn dashboard_deck(
    layout: &crate::dash::Layout,
    bodies: &[(crate::dash::Card, String)],
    arranging: bool,
    waiting: usize,
    deck: &Deck,
) -> String {
    let mut body = String::new();

    body.push_str(&format!(
        "<div class=homehead><span class=bigmark>{MARK}</span><h1>Home</h1>\
         <span class=hi>{}</span><span class=status><span class='live {}'></span> {}</span></div>",
        esc(&deck.greeting),
        deck.tone,
        esc(&deck.status)
    ));
    if deck.first_run {
        body.push_str(&first_run_html(deck));
    } else {
        body.push_str(&brief_html(deck));
    }
    body.push_str("<div class=living>");
    body.push_str(&today_html(deck));
    if deck.businesses.is_empty() {
        body.push_str(&right_now_html(deck));
    } else {
        body.push_str("<div class=glances>");
        for g in &deck.businesses {
            body.push_str(&glance_html(g));
        }
        body.push_str("</div>");
    }
    body.push_str("</div>");

    body.push_str("<div class=dashtop><h2>Your cards</h2>");
    if arranging {
        body.push_str(
            "<span class=note>Drag a card by its handle, or use the buttons.</span>\
             <span class=grow></span>",
        );
        if !layout.is_default() {
            body.push_str(
                "<form class=inline method=post action=/hub/dash>\
                 <input type=hidden name=what value=reset>\
                 <button class=quiet>Put it back how it was</button></form>",
            );
        }
        body.push_str(
            "<form class=inline method=post action=/hub/dash>\
             <input type=hidden name=what value=done>\
             <button class=primary>Done</button></form>",
        );
    } else {
        body.push_str("<span class=grow></span>");
        body.push_str(
            "<form class=inline method=post action=/hub/dash>\
             <input type=hidden name=what value=arrange>\
             <button>Arrange</button></form>",
        );
    }
    body.push_str("</div>");

    body.push_str(&format!(
        "<div class='cards{}' id=cards>",
        if arranging { " arranging" } else { "" }
    ));
    if layout.is_empty() {
        body.push_str(
            "<p class=nothing>Every card is hidden. Press Arrange to put some back.</p></div>",
        );
        return shell_with(Some(Page::Dashboard), "Home", &body, waiting);
    }
    let visible = layout.visible();
    for (i, placed) in visible.iter().enumerate() {
        let card = placed.card;
        let inner = bodies
            .iter()
            .find(|(c, _)| *c == card)
            .map(|(_, b)| b.clone())
            // Never an empty frame: a blank card and a card whose contents
            // failed to arrive look identical, and one of those is a bug.
            .unwrap_or_else(|| nothing("Nothing to show here yet."));

        body.push_str(&format!(
            // The explanation rides on the heading as its tooltip rather than
            // as a line under it: the design's section headings stand alone.
            "<section class='card {span}' data-card='{key}' data-at='{i}'>\
             <h3 title='{note}'>{title}{grip}</h3>",
            span = match placed.span {
                crate::dash::Span::Full => "full",
                crate::dash::Span::Half => "half",
            },
            key = card.key(),
            i = i,
            title = esc(card.title()),
            note = esc(card.note()),
            grip = if arranging {
                "<span class=grip aria-hidden=true>⠿</span>"
            } else {
                ""
            },
        ));

        if arranging {
            // The buttons and the drag do the same thing through the same
            // route, so there is one way to move a card rather than two that
            // can disagree — and the buttons keep working with no script, on
            // a slow connection, and for anyone who cannot hold and drag.
            body.push_str("<div class=handles>");
            for (what, label, on) in [
                ("up", "Up", i > 0),
                ("down", "Down", i + 1 < visible.len()),
                (
                    "widen",
                    match placed.span {
                        crate::dash::Span::Full => "Narrower",
                        crate::dash::Span::Half => "Wider",
                    },
                    true,
                ),
                ("hide", "Hide", true),
            ] {
                if !on {
                    continue;
                }
                body.push_str(&format!(
                    "<form class=inline method=post action=/hub/dash>\
                     <input type=hidden name=what value='{what}'>\
                     <input type=hidden name=card value='{}'>\
                     <button class=go>{label}</button></form>",
                    card.key()
                ));
            }
            body.push_str("</div>");
        } else {
            body.push_str(&inner);
        }
        body.push_str("</section>");
    }
    body.push_str("</div>");

    if arranging {
        let hidden: Vec<&crate::dash::Placed> =
            layout.cards.iter().filter(|p| p.hidden).collect();
        if !hidden.is_empty() {
            body.push_str("<h2>Hidden</h2><div class=handles>");
            for p in hidden {
                body.push_str(&format!(
                    "<form class=inline method=post action=/hub/dash>\
                     <input type=hidden name=what value=show>\
                     <input type=hidden name=card value='{}'>\
                     <button class=go>Show {}</button></form>",
                    p.card.key(),
                    esc(p.card.title())
                ));
            }
            body.push_str("</div>");
        }
        body.push_str(DRAG_SCRIPT);
    }

    shell_with(Some(Page::Dashboard), "Home", &body, waiting)
}

/// Stored logins, and what actually protects each account.
///
/// Two different things on one page on purpose: what Atlas is holding for you,
/// and how exposed each site is. Split across two pages you would check one
/// and not the other, and the interesting answer is always the join — a
/// keystone account with nothing but a password.
///
/// Atlas never changes a security setting. It can take you to the page.
pub fn accounts_page(
    accounts: &[crate::accounts::Account],
    advice: &[crate::accounts::Advice],
    undescribed: &[&crate::accounts::Account],
    stored: &[(String, String)],
    vault_open: bool,
    // What would lock you out, from `goingaway`, `codes` and `recovery`.
    safety: &[(String, String, f32)],
) -> String {
    let mut body = String::new();

    // What would lock you out, before what is merely weak. Being unable to
    // get back in is worse than being easy to get into, and it is the thing
    // nobody checks until the day it matters.
    if !safety.is_empty() {
        body.push_str("<h2>What would lock you out</h2>");
        for (what, why, _) in safety.iter().take(6) {
            body.push_str(&format!(
                "<div class=rec><h3>{}</h3><p class=cause>{}</p></div>",
                esc(what),
                esc(why)
            ));
        }
    }

    // What to fix, before the inventory. An inventory is a list; this is the
    // answer.
    if advice.is_empty() && !accounts.is_empty() {
        body.push_str(&nothing(
            "Nothing here needs changing. Every account you've told me about is \
             protected by more than a password.",
        ));
    }
    for a in advice.iter().take(6) {
        body.push_str(&format!(
            "<div class=rec><h3>{}</h3><p class=cause>{}</p>{}</div>",
            esc(&format!("{} — {}", a.site, a.what)),
            esc(&a.why),
            match &a.can_open {
                // Atlas opens the page. Changing the setting is yours, and
                // saying so plainly is better than a button that quietly does
                // less than it looks like it does.
                Some(url) => format!(
                    "<a class=more href='{}' target=_blank rel=noreferrer>Open the settings page</a>",
                    esc(url)
                ),
                None => String::new(),
            }
        ));
    }

    if !undescribed.is_empty() {
        body.push_str("<h2>I know nothing about these</h2>");
        body.push_str(&nothing(
            "An account I've been told nothing about looks exactly like a safe \
             one — I find nothing wrong because I know nothing. Tell me what \
             protects these.",
        ));
    }

    body.push_str("<h2>Accounts</h2>");
    if accounts.is_empty() {
        body.push_str(&nothing(
            "I'm not tracking any accounts yet. Add one below and I'll tell you \
             where it's weak.",
        ));
    }
    for a in accounts {
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div>\
             <div class=what>{} · {} protects it{}</div>\
             <div class=handles>",
            esc(&a.site),
            esc(stakes_word(a.stakes)),
            esc(a.second_factor.plain()),
            if a.reused_password {
                " · password reused elsewhere"
            } else {
                ""
            },
        ));
        for (value, label) in [
            ("app", "Authenticator app"),
            ("key", "Physical key"),
            ("passkey", "Passkey"),
            ("sms", "Text message"),
            ("none", "Password only"),
        ] {
            body.push_str(&format!(
                "<form class=inline method=post action=/hub/accounts>\
                 <input type=hidden name=what value=factor>\
                 <input type=hidden name=site value='{}'>\
                 <input type=hidden name=to value='{value}'>\
                 <button class=go>{label}</button></form>",
                esc(&a.site)
            ));
        }
        body.push_str(&format!(
            "<form class=inline method=post action=/hub/accounts>\
             <input type=hidden name=what value='{}'>\
             <input type=hidden name=site value='{}'>\
             <button class=go>{}</button></form>\
             <form class=inline method=post action=/hub/accounts>\
             <input type=hidden name=what value=forget>\
             <input type=hidden name=site value='{}'>\
             <button class=go>Stop tracking</button></form></div></div>",
            if a.reused_password { "unique" } else { "reused" },
            esc(&a.site),
            if a.reused_password {
                "Password is unique now"
            } else {
                "Password is reused"
            },
            esc(&a.site),
        ));
    }

    body.push_str(
        "<form method=post action=/hub/accounts style='margin-top:18px'>\
         <input type=hidden name=what value=note>\
         <input name=site aria-label='Which site?' placeholder='Which site?' autocomplete=off>\
         <button class=primary>Track it</button></form>",
    );

    body.push_str("<h2>What I'm holding for you</h2>");
    body.push_str(&note_line(if vault_open {
        "The vault is unlocked. It locks itself again after a while."
    } else {
        "The vault is locked. I can tell you what's in it, not what it says."
    }));
    if stored.is_empty() {
        body.push_str(&nothing(
            "Nothing stored yet. I never hold a password unless you ask me to.",
        ));
    }
    for (name, kind) in stored {
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div><div class=what>{}</div></div>",
            esc(name),
            esc(kind)
        ));
    }

    shell_at(Some(Page::Accounts), "Stored logins", &body)
}

pub(super) fn note_line(said: &str) -> String {
    format!("<p class=note>{}</p>", esc(said))
}

/// How much an account matters, said rather than named.
pub(super) fn stakes_word(s: crate::accounts::Stakes) -> &'static str {
    match s {
        crate::accounts::Stakes::Keystone => "Losing this loses the others",
        crate::accounts::Stakes::High => "Matters a lot",
        crate::accounts::Stakes::Medium => "Matters",
        crate::accounts::Stakes::Low => "Minor",
    }
}
