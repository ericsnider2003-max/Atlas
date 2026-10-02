//! The hub — a page you open when you want to change something.
//!
//! Deliberately not an app. It's a page served on loopback by the API server
//! that already exists, which means: nothing new to install, it works from
//! your phone and iPad over the same connection, and **Atlas still runs with
//! no window open.** You open the hub the way you open a router's admin page —
//! occasionally, on purpose, then close it.
//!
//! Plain HTML with no JavaScript framework and no external requests. It has to
//! work offline, because Atlas does.

use crate::settings::{Settings, Value, Weight};

/// Every page the hub serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// The one you open on purpose: the cards you arranged, in your order.
    ///
    /// Distinct from the windows Atlas raises when you ask it something out
    /// loud. Asking "what's outstanding" gets Atlas's own answer; opening the
    /// dashboard is you going to look.
    Dashboard,
    /// What Atlas is doing right now.
    Status,
    /// Live: the current task, its steps, and the thinking as it happens.
    Now,
    /// Every hand gesture, drawn from its own definition.
    Gestures,
    /// Which sites Atlas can sign into, and taking that away.
    Access,
    /// Whether each connection Atlas depends on is still working.
    Connections,
    /// Everything outstanding, as a board.
    Workspace,
    /// Your projects, and the changes queued for each — proposed, waiting on
    /// you, and outstanding.
    Workshop,
    /// Your calendar: what's coming up, soonest first.
    Calendar,
    /// A past day: what moved, and what Atlas was thinking.
    LookingBack,
    /// What Atlas would change about itself.
    Recommendations,
    Settings,
    /// Everything that can act without asking, in one list.
    Permissions,
    /// Add-ons: what each may do, approving, and taking it back.
    AddOns,
    /// Your own edits to the shipped config files, kept through updates.
    Edits,
    /// Your friends: adding one with a link, requests, and taking one back.
    Friends,
    /// Group chats you own or are in: who's in them, and who may post.
    Groups,
    /// Stored logins — references only, never the secrets themselves.
    Accounts,
    /// What it did while you weren't watching.
    Activity,
    /// What it couldn't do.
    Outstanding,
    /// Carrying things between your devices, and the key that seals them.
    ///
    /// Here rather than in a terminal because the person this is for does not
    /// use one. A protection nobody can find is not a protection.
    Sync,
    /// Messages: one-to-ones and groups, Atlas-to-Atlas, held and delivered
    /// when the other side is back.
    Messages,
    /// Everything on your machine in one list, with what's private and
    /// what's gone out, and to whom.
    Documents,
    /// A business at a glance: open work, clients, partners, and Atlas's read.
    Business,
    /// A business's shared tasks, as a table, a board or a calendar.
    SharedTasks,
    /// Your clients, each with the firewall shown on the record.
    Clients,
    /// The people you run a business with.
    Partners,
    /// Atlas's voice, how it speaks, the wake word, push-to-talk, quiet hours.
    Sound,
    /// Who Atlas may send to without asking each time.
    Trusted,
    /// Hand Atlas something: a link, words, a photo, any file.
    Give,
    /// What still works offline, and what is waiting for a connection.
    Offline,
    /// Talk to Atlas: type, or hold to talk where the device can listen.
    Talk,
    /// Help, and how Atlas is made usable for everyone: the accessibility
    /// statement, what each accessibility feature does, and how to report a
    /// barrier.
    Help,
    /// Atlas's own updates: what's out, installing it, going back a
    /// version, and how updates install here (OPEN_GAPS 8.2).
    Updates,
    /// Feedback about Atlas: telling whoever sends you Atlas that something's
    /// wrong, and on their side what came in and answering it (8.14).
    Feedback,
    /// Putting Atlas on your phone or iPad: pick which, scan a code (D5).
    Phone,
    /// Your social accounts' numbers, and what's working for the people and
    /// topics you watch (`social`, 29 Sep 2026).
    Social,
    /// Gigs, grants and niches the hunter found, with why (`hunting`).
    Opportunities,
}


/// The hub's navigation, as data rather than a hardcoded string of links.
///
/// It was a flat row of eleven sibling links across the top. Eleven is well
/// past the point where a flat bar stops being navigation and starts being a
/// list you read every time — the researched cut-off is about six, after which
/// the answer is grouped side navigation. Grouping also gives a page a parent,
/// which is what a breadcrumb needs and what "drill into one of these" needs
/// after that.
///
/// It is data so that `tests/hub_navigation.rs` can check the thing that
/// actually went wrong: `Page::Connections` was routable and reachable by
/// typing the URL, and no link anywhere pointed at it. A page nothing links to
/// is a page nobody opens.
pub const NAV: &[(&str, &[Page])] = &[
    // Now and Talk sit at the top of the sidebar on their own, under Search:
    // Atlas's thought process, and talking to it, one press from anywhere.
    ("", &[Page::Now, Page::Talk]),
    // The design's Personal group. Home leads it: the design files the front
    // page under Personal ("Personal / Home"), beside the business groups
    // that appear once a business exists.
    (
        "Personal",
        &[
            Page::Dashboard,
            Page::Calendar,
            Page::Workshop,
            Page::Outstanding,
            Page::Messages,
            Page::Documents,
        ],
    ),
    // The people you talk to through Atlas, who decides what in each group,
    // and who may be sent to without asking.
    ("People", &[Page::Friends, Page::Groups, Page::Trusted]),
    // Your work in other shapes, and handing Atlas something.
    ("Your work", &[Page::Workspace, Page::LookingBack, Page::Give, Page::Social, Page::Opportunities]),
    // The business section: under More until a business exists, when each
    // business also gets its own group in the sidebar.
    ("Business", &[Page::Business, Page::SharedTasks, Page::Clients, Page::Partners]),
    // Atlas talking about itself. Under More in the sidebar.
    (
        "Atlas itself",
        &[
            Page::Status,
            Page::Connections,
            Page::Recommendations,
            Page::Gestures,
            Page::Activity,
            // Your edits to Atlas's own config files: about Atlas itself.
            Page::Edits,
        ],
    ),
    // Under More, except Settings, which the design keeps at the sidebar's foot.
    (
        "What it may do",
        &[
            Page::Settings,
            Page::Sound,
            Page::Permissions,
            Page::AddOns,
            Page::Accounts,
            Page::Access,
        ],
    ),
    ("Your devices", &[Page::Sync, Page::Phone, Page::Updates, Page::Offline, Page::Help, Page::Feedback]),
];

impl Page {
    /// Pages whose address carries a choice: which conversation, business or
    /// view, or what a share sheet sent.
    pub fn reads_query(self) -> bool {
        matches!(
            self,
            Page::Messages | Page::Business | Page::SharedTasks | Page::Clients | Page::Give | Page::Sound
                | Page::Trusted | Page::Talk | Page::Help | Page::Workshop | Page::Updates | Page::Feedback
                | Page::Connections | Page::Documents | Page::Phone | Page::Recommendations
        )
    }

    /// Where this page lives. The one place a path is written down, so
    /// `route` and the navigation cannot drift apart.
    pub fn href(self) -> &'static str {
        match self {
            Page::Dashboard => "/hub",
            Page::Status => "/hub/status",
            Page::Now => "/hub/now",
            Page::Gestures => "/hub/gestures",
            Page::Access => "/hub/access",
            Page::Connections => "/hub/connections",
            Page::Workspace => "/hub/workspace",
            Page::Workshop => "/hub/workshop",
            Page::Calendar => "/hub/calendar",
            Page::LookingBack => "/hub/back",
            Page::Recommendations => "/hub/recommendations",
            Page::Settings => "/hub/settings",
            Page::Permissions => "/hub/permissions",
            Page::AddOns => "/hub/addons",
            Page::Edits => "/hub/edits",
            Page::Friends => "/hub/friends",
            Page::Groups => "/hub/groups",
            Page::Accounts => "/hub/accounts",
            Page::Activity => "/hub/activity",
            Page::Outstanding => "/hub/outstanding",
            Page::Sync => "/hub/sync",
            Page::Messages => "/hub/messages",
            Page::Documents => "/hub/documents",
            Page::Business => "/hub/business",
            Page::SharedTasks => "/hub/tasks",
            Page::Clients => "/hub/clients",
            Page::Partners => "/hub/partners",
            Page::Sound => "/hub/sound",
            Page::Trusted => "/hub/trusted",
            Page::Give => "/hub/give",
            Page::Social => "/hub/social",
            Page::Offline => "/hub/offline",
            Page::Talk => "/hub/talk",
            Page::Help => "/hub/help",
            Page::Updates => "/hub/updates",
            Page::Phone => "/hub/phone",
            Page::Feedback => "/hub/feedback",
            Page::Opportunities => "/hub/opportunities",
        }
    }

    /// What it is called in the navigation.
    ///
    /// A name, not a sentence. These used to read "What I may do unasked" and
    /// "Sites I can sign into" — accurate, and doing the description's job, so
    /// a menu of them scanned like a paragraph. The sentence still exists; it
    /// is `note()`, and it sits under the heading where a sentence belongs.
    pub fn label(self) -> &'static str {
        match self {
            Page::Dashboard => "Home",
            Page::Status => "Health",
            Page::Now => "Now",
            Page::Gestures => "Gestures",
            Page::Access => "Access",
            Page::Connections => "Connections",
            Page::Workspace => "Board",
            Page::Workshop => "Projects",
            Page::Calendar => "Calendar",
            Page::LookingBack => "History",
            Page::Recommendations => "Improvements",
            Page::Settings => "Settings",
            Page::Permissions => "Permissions",
            Page::AddOns => "Add-ons",
            Page::Edits => "Your edits",
            Page::Friends => "Friends",
            Page::Groups => "Groups",
            Page::Accounts => "Accounts",
            Page::Activity => "Activity",
            Page::Outstanding => "Outstanding",
            Page::Sync => "Your devices",
            Page::Messages => "Messages",
            Page::Documents => "Documents",
            Page::Business => "Overview",
            Page::SharedTasks => "Shared tasks",
            Page::Clients => "Clients",
            Page::Partners => "Partners",
            Page::Sound => "Sound & voice",
            Page::Trusted => "Trusted",
            Page::Give => "Give Atlas something",
            Page::Social => "Social",
            Page::Offline => "Offline",
            Page::Talk => "Talk",
            Page::Help => "Help & accessibility",
            Page::Updates => "Updates",
            Page::Phone => "Your phone",
            Page::Feedback => "Feedback",
            Page::Opportunities => "Opportunities",
        }
    }

    /// The one line under the heading.
    ///
    /// This is where "what I may do unasked" went. A short name is only
    /// clearer than a long one if the long one is still available the moment
    /// you arrive — otherwise you have traded clutter for guessing.
    pub fn note(self) -> &'static str {
        match self {
            Page::Dashboard => "Your brief, today, and what's waiting on you.",
            Page::Status => "Memory, disk, and whether Atlas is listening.",
            Page::Now => "Everything I'm doing, in order — nothing hidden.",
            Page::Gestures => "Every gesture I know, and how to make it.",
            Page::Access => "Everything Atlas can reach, and how to take it back.",
            Page::Connections => "Whether the things Atlas relies on are answering.",
            Page::Workspace => "Everything outstanding, grouped the way you asked for.",
            Page::Workshop => "Your projects, and the changes queued for each.",
            Page::Calendar => "What's coming up on your calendar, soonest first.",
            Page::LookingBack => "Earlier days: what moved, and what Atlas was thinking.",
            Page::Recommendations => "What Atlas would change about itself, and why.",
            Page::Settings => "Everything Atlas can be told to do or not do.",
            Page::Permissions => "Everything allowed to act without asking you first.",
            Page::AddOns => "What you and friends added to Atlas, what each may do, and taking it back.",
            Page::Edits => "Your own changes to Atlas's config files, kept through every update.",
            Page::Friends => "Add a friend with one link, answer friend requests, and see who can reach you.",
            Page::Groups => "Group chats you made or are in: who's in each, and who may post.",
            Page::Accounts => "Your accounts, where they're weak, and what the vault holds.",
            Page::Activity => "Everything Atlas did without being watched.",
            Page::Outstanding => "Everything still open — personal and business — most pressing first.",
            Page::Sync => "What Atlas carries between your machines, and how it's sealed.",
            Page::Messages => "Atlas-to-Atlas, encrypted, no server between. Sent now; held and delivered if they're offline.",
            Page::Documents => "Everything on your machine, one list — what's private, and what's gone out and to whom.",
            Page::Business => "Your business at a glance: open work, clients, partners, and what needs you.",
            Page::SharedTasks => "The business's shared tasks — as a table, a board, or a calendar.",
            Page::Clients => "Your clients, each with what they can and can't see.",
            Page::Partners => "The people you run the business with, and whether their Atlas is reachable.",
            Page::Sound => "Atlas's voice, how it speaks, the wake word, push-to-talk, and quiet hours.",
            Page::Trusted => "People Atlas may send to without asking each time. Personal things still ask.",
            Page::Give => "A link, a photo, words, or any file, any size. It stays on your machine.",
            Page::Social => "Your accounts' numbers over time, and what's working for the people you watch.",
            Page::Offline => "What still works with no connection, and what's queued to go when it's back.",
            Page::Talk => "Say it or type it. Atlas does the in-between and shows you what it did.",
            Page::Help => "How to use Atlas with a screen reader, keyboard, voice or larger text — and how to tell me when something's in the way.",
            Page::Updates => "Which Atlas this is, what's new, putting it in, and going back if you'd rather.",
            Page::Phone => "Putting Atlas on your phone or iPad: pick which, and scan a code with it.",
            Page::Feedback => "Tell whoever sends you Atlas that something's wrong — you see exactly what goes — and hear back what they did.",
            Page::Opportunities => "Gigs, grants and niches from the sources you picked, weighed and with why. Nothing is applied for or sent.",
        }
    }

    /// The group this page sits under, for the breadcrumb.
    pub fn group(self) -> Option<&'static str> {
        NAV.iter()
            .find(|(_, pages)| pages.contains(&self))
            .map(|(g, _)| *g)
            .filter(|g| !g.is_empty())
    }
}

/// The trail back up, so a deep page says where it is.
///
/// <group> / <page>, as the design's top bar draws it ("Personal / Home").
/// Nothing in it links to the page you are on — a breadcrumb whose last item
/// is a link to itself is a small lie about where you are — and the groups
/// are headings in the sidebar, not pages, so they are words here too.
pub fn crumbs(here: Page) -> String {
    let group = here.group().unwrap_or("Atlas");
    format!(
        "<p class=crumbs>{}<span>/</span><b>{}</b></p>",
        esc(group),
        esc(here.label())
    )
}


/// A list of things you can open, one per row.
///
/// The shape Eric asked for and the hub did not have: an index, then the one
/// you picked — rather than every subject being its own flat top-level page.
/// A business, a partner, a project all take this shape, so it is written once
/// here rather than per subject.
pub fn index_rows(rows: &[(String, String, String)]) -> String {
    if rows.is_empty() {
        return "<p class=note>Nothing here yet.</p>".into();
    }
    let mut out = String::from("<div class=index>");
    for (href, name, detail) in rows {
        out.push_str(&format!(
            "<a class=entry href='{}'><span class=name>{}</span>\
             <span class=what>{}</span></a>",
            esc(href),
            esc(name),
            esc(detail)
        ));
    }
    out.push_str("</div>");
    out
}


// ---------------------------------------------------------------------------
// The pieces a card is built from.
//
// Written once, here, so every card looks like it came from the same place.
// A dashboard where each card invents its own spacing and its own type size is
// exactly what reads as homemade, and that impression arrives before anyone
// has read a word of it.
// ---------------------------------------------------------------------------

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

/// How a row's dot is coloured: ember for yours to act on, blue for proposed,
/// grey for the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Act,
    Proposed,
    Record,
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
fn opens_word(o: crate::credentials::Opens) -> &'static str {
    match o {
        crate::credentials::Opens::Nothing => "nothing on its own",
        crate::credentials::Opens::OneThing => "one narrow thing",
        crate::credentials::Opens::AService => "a whole service",
        crate::credentials::Opens::Everything => "the account and everything that resets through it",
    }
}

/// Where it lives, said rather than named.
fn kept_word(k: crate::credentials::Kept) -> &'static str {
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

/// Escape anything that came from outside before it goes into HTML.
///
/// Note names, device names and app paths all end up on these pages, and a
/// window title containing a `<` should render as a window title.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// The thinking indicator.
///
/// Three arcs at different speeds around a still centre. Pure SVG and CSS —
/// no images, no libraries, nothing fetched. It has to work offline because
/// Atlas does, and it has to be small enough to sit in a page that reloads.
pub const THINKING: &str = "\
<svg class=think viewBox='0 0 120 120' xmlns='http://www.w3.org/2000/svg' aria-label='thinking'>\
  <circle cx=60 cy=60 r=54 class='ring r1'/>\
  <circle cx=60 cy=60 r=42 class='ring r2'/>\
  <circle cx=60 cy=60 r=30 class='ring r3'/>\
  <circle cx=60 cy=60 r=6  class=core/>\
</svg>";

/// The same shape, still, for when nothing is happening.
pub const IDLE: &str = "\
<svg class='think idle' viewBox='0 0 120 120' xmlns='http://www.w3.org/2000/svg' aria-label='idle'>\
  <circle cx=60 cy=60 r=42 class='ring r2'/>\
  <circle cx=60 cy=60 r=6  class=core/>\
</svg>";

const STYLE: &str = include_str!("../assets/hub/hub.css");


pub fn shell(title: &str, body: &str) -> String {
    shell_at(None, title, body)
}

/// The page, with the navigation knowing where you are.
///
/// `here` marks the current item and draws the trail back up. `None` is for
/// the few pages served outside the routing table (an error, a spoken reply),
/// which have no place in the tree and should not pretend to.
pub fn shell_at(here: Option<Page>, title: &str, body: &str) -> String {
    shell_with(here, title, body, 0)
}

/// Where your appearance choices are kept.
pub const APPEARANCE_KEY: &str = "appearance";

/// How the hub looks to you: the "Aa" menu, recovered from the second chat's
/// hub (seen only in its rendered preview, 23 Sep 2026 — the code behind it
/// never reached this tree). Theme, text size, high contrast, reduce motion.
///
/// Kept by Atlas rather than in the browser, so the phone and the laptop's
/// window agree, and applied to every page as attributes the CSS already
/// reads — no script, per `the_hub_works_offline`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// "light", "dark", "auto" (follow the system), or empty for the default:
    /// Warm Paper, unless Settings chose another theme.
    pub theme: String,
    /// "", "large" or "larger".
    pub text: String,
    pub high_contrast: bool,
    pub reduce_motion: bool,
}

impl Appearance {
    /// Apply one choice from the menu. `false` for anything the menu can't
    /// send, so a mistyped address changes nothing.
    pub fn choose(&mut self, what: &str, to: &str) -> bool {
        match (what, to) {
            // Warm Paper is the default (Eric, 26 Sep), so choosing it clears
            // the choice; "auto" is kept, because it has to beat that default.
            ("theme", "light" | "dark" | "auto") => self.theme = to.into(),
            ("theme", "paper") => self.theme.clear(),
            ("text", "large" | "larger") => self.text = to.into(),
            ("text", "normal") => self.text.clear(),
            ("contrast", "high") => self.high_contrast = true,
            ("contrast", "normal") => self.high_contrast = false,
            ("motion", "off") => self.reduce_motion = true,
            ("motion", "on") => self.reduce_motion = false,
            _ => return false,
        }
        true
    }

    /// The attributes that carry these on the page's root element.
    pub fn attrs(&self) -> String {
        let mut a = String::new();
        if matches!(self.theme.as_str(), "light" | "dark" | "auto") {
            a.push_str(&format!(" data-theme={}", self.theme));
        }
        if matches!(self.text.as_str(), "large" | "larger") {
            a.push_str(&format!(" data-text={}", self.text));
        }
        if self.high_contrast {
            a.push_str(" data-contrast=high");
        }
        if self.reduce_motion {
            a.push_str(" data-motion=off");
        }
        a
    }
}

/// Apply your appearance to a finished page. One insertion point, like
/// [`with_palette`], so the page functions stay free of per-person state.
pub fn with_appearance(page: String, a: &Appearance) -> String {
    let attrs = a.attrs();
    if attrs.is_empty() {
        return page;
    }
    page.replacen("<html lang=en", &format!("<html lang=en{attrs}"), 1)
}

/// The "Aa" menu. Links, not a script: each one sets a choice and brings you
/// back. Which option is current is shown by CSS reading the root's
/// attributes, so the menu needs no knowledge of who is looking.
fn appearance_menu() -> String {
    let seg = |what: &str, opts: &[(&str, &str)]| {
        let mut s = String::from("<div class=segmented>");
        for (to, label) in opts {
            s.push_str(&format!(
                "<a href='/hub/appearance?set={what}&amp;to={to}' data-v='{what}-{to}'>{label}</a>"
            ));
        }
        s.push_str("</div>");
        s
    };
    let toggle = |what: &str, label: &str, on: &str, off: &str| {
        format!(
            "<div class=toggle>{label}<span class=pair>\
             <a href='/hub/appearance?set={what}&amp;to={on}' data-v='{what}-{on}' aria-label='{label} on'></a>\
             <a href='/hub/appearance?set={what}&amp;to={off}' data-v='{what}-{off}' aria-label='{label} off'></a>\
             </span></div>"
        )
    };
    format!(
        "<details class=appwrap><summary class='tool icon' aria-label='Appearance and access'>Aa</summary>\
         <div class=appmenu role=menu aria-label='Appearance and access'>\
         <p class=mh>Theme</p>{theme}<p class=mh>Text size</p>{text}<p class=mh>Access</p>{contrast}{motion}</div></details>",
        theme = seg("theme", &[("paper", "Paper"), ("light", "Light"), ("dark", "Dark"), ("auto", "Auto")]),
        text = seg("text", &[("normal", "Normal"), ("large", "Large"), ("larger", "Larger")]),
        contrast = toggle("contrast", "High contrast", "high", "normal"),
        motion = toggle("motion", "Reduce motion", "off", "on"),
    )
}

/// Put the true "waiting" count in a finished page's header.
///
/// Only the dashboard used to be told the count, so every other page's header
/// said "Nothing waiting" while the dashboard beside it said "1 waiting". One
/// insertion point, like [`with_appearance`], so no page can be the one that forgot.
pub fn with_waiting(page: String, waiting: usize) -> String {
    if waiting == 0 {
        return page;
    }
    let href = Page::Outstanding.href();
    let page = page.replacen(
        &format!("<a class=tool href='{href}'>Nothing waiting</a>"),
        &format!("<a class=tool href='{href}'><span class=dot></span>{waiting} waiting</a>"),
        1,
    );
    // And the phone's Home tab carries it as a badge, unless it already does.
    if page.contains("<span class=badge>") {
        return page;
    }
    let home_tab = page.find("<nav class=tabs").and_then(|t| page[t..].find("</svg>Home</a>").map(|i| t + i));
    match home_tab {
        Some(i) => {
            let at = i + "</svg>Home".len();
            format!(
                "{}<span class=badge><span class=sr>, </span>{waiting}<span class=sr> waiting</span></span>{}",
                &page[..at],
                &page[at..]
            )
        }
        None => page,
    }
}

/// Put the palette on a finished page.
///
/// One insertion point rather than a parameter threaded through thirteen page
/// functions. Those thirteen would become fourteen, and the fourteenth would
/// be the page someone forgot — a palette missing from one page is worse than
/// no palette, because you learn to reach for it and then it is not there.
/// `tests/palette.rs` checks every page has it.
pub fn with_palette(
    page: String,
    entries: &[crate::palette::Entry],
    recent: &crate::palette::Recent,
) -> String {
    match page.rfind("</body>") {
        Some(at) => {
            let overlay = palette_overlay(entries, recent);
            format!("{}{overlay}{}", &page[..at], &page[at..])
        }
        // Not a whole page — an error fragment, or something rendered for a
        // test. Left alone rather than having an overlay bolted to the end of
        // it, which would produce markup that looks right and is not.
        None => page,
    }
}

// ---------------------------------------------------------------------------
// The hub as an app on the phone's home screen.
//
// The phone reaches the hub over HTTPS through `tailscale serve`, so the page
// is a secure context there and can be installed. Four pieces make that work,
// all of them served by Atlas itself — nothing from anywhere else:
//
// 1. A manifest, which names the app and says where it opens. It has to carry
//    the token in `start_url`: iOS gives a home-screen app its own cookie jar,
//    separate from Safari's (WebKit bug 181849), so an installed hub that
//    opened to plain `/hub` would open to a 401. It is therefore behind the
//    token itself, and `server.rs` accepts `?t=` on it without the usual
//    redirect, because a manifest is fetched without the cookie.
// 2. A service worker, which lets the app open with the laptop asleep: the
//    last copy of the page you were on, or a sentence saying where Atlas is.
// 3. Icons, drawn from Atlas's mark on the hub's own background.
// 4. The tags in every page's head that point at the three above.
// ---------------------------------------------------------------------------

/// Where the manifest is served. Behind the token; see the section note.
pub const MANIFEST_PATH: &str = "/hub/manifest.webmanifest";
/// Where the service worker is served. Public: it holds nothing private.
pub const SERVICE_WORKER_PATH: &str = "/hub/sw.js";

/// Ember Dark's background — the dark-mode status-bar colour.
const APP_BG: &str = "#0c0f14";
/// Warm Paper's background — the lead colourway, the colour the app opens on
/// and the light-mode status-bar colour.
const APP_BG_LIGHT: &str = "#f7f4ee";

/// The home-screen icons: the Folded A (`mark`) on Warm Paper, with the
/// rounded tile Eric chose, written by `design/mark/make_marks.py`. The
/// maskable one is square, with the mark inside the central 80%, because the
/// system that uses it cuts its own shape.
pub const ICON_192: &[u8] = include_bytes!("../assets/icon-192.png");
pub const ICON_512: &[u8] = include_bytes!("../assets/icon-512.png");
pub const ICON_MASKABLE_512: &[u8] = include_bytes!("../assets/icon-maskable-512.png");
pub const APPLE_TOUCH_ICON: &[u8] = include_bytes!("../assets/apple-touch-icon.png");

/// A file the phone app needs that is safe to hand out with no token.
pub struct PublicFile {
    pub content_type: &'static str,
    /// Extra response headers, each ending `\r\n`.
    pub headers: &'static str,
    pub bytes: &'static [u8],
}

/// The files anyone may fetch: the service worker and the icons.
///
/// Public because a browser fetches them without the cookie (an icon for a
/// home screen, a worker registration's update check) and because nothing in
/// them is private — they are the same bytes on every install. The manifest is
/// deliberately not here: it carries the token.
pub fn public_file(method: &str, path: &str) -> Option<PublicFile> {
    if method != "GET" {
        return None;
    }
    const ICON_CACHE: &str = "Cache-Control: max-age=86400\r\n";
    match path {
        SERVICE_WORKER_PATH => Some(PublicFile {
            content_type: "text/javascript; charset=utf-8",
            // The worker lives at /hub/sw.js, which on its own may only
            // control /hub/…; this lets it control /hub itself as well.
            headers: "Service-Worker-Allowed: /hub\r\nCache-Control: no-cache\r\n",
            bytes: SERVICE_WORKER.as_bytes(),
        }),
        "/hub/icon-192.png" => {
            Some(PublicFile { content_type: "image/png", headers: ICON_CACHE, bytes: ICON_192 })
        }
        "/hub/icon-512.png" => {
            Some(PublicFile { content_type: "image/png", headers: ICON_CACHE, bytes: ICON_512 })
        }
        "/hub/icon-maskable-512.png" => {
            Some(PublicFile { content_type: "image/png", headers: ICON_CACHE, bytes: ICON_MASKABLE_512 })
        }
        "/hub/apple-touch-icon.png" => Some(PublicFile {
            content_type: "image/png",
            headers: ICON_CACHE,
            bytes: APPLE_TOUCH_ICON,
        }),
        _ => None,
    }
}

/// The web app manifest, with the token in `start_url`.
///
/// See the section note for why the token has to be there. Icons are listed
/// once as `any` and once as `maskable` rather than as `"any maskable"`,
/// which some browsers read as one purpose they do not know.
pub fn manifest(token: &str) -> String {
    let icon = |src: &str, size: &str, purpose: &str| {
        serde_json::json!({ "src": src, "sizes": size, "type": "image/png", "purpose": purpose })
    };
    serde_json::json!({
        "id": "/hub",
        "name": "Atlas",
        "short_name": "Atlas",
        "description": "Your Atlas, from your phone.",
        "start_url": format!("/hub?t={token}"),
        "scope": "/hub",
        "display": "standalone",
        "background_color": APP_BG_LIGHT,
        "theme_color": APP_BG_LIGHT,
        "icons": [
            icon("/hub/icon-192.png", "192x192", "any"),
            icon("/hub/icon-512.png", "512x512", "any"),
            icon("/hub/icon-maskable-512.png", "512x512", "maskable"),
        ],
        // The phone's share sheet hands a link or words straight to Give.
        // No orientation lock: WCAG 1.3.4 — the hub works either way up.
        "share_target": { "action": "/hub/give", "method": "GET", "params": { "title": "title", "text": "text", "url": "url" } },
    })
    .to_string()
}

/// Put the app tags in a finished page's head.
///
/// One insertion point, like [`with_palette`], and in `server.rs` rather than
/// in `shell_with`: the server is the one place that knows the token, and it
/// is what both the running Atlas (`hublive`) and settings-only mode
/// (`run_hub`) send every page through — so neither can be the one that
/// forgot. It also keeps the page functions themselves script-free, which
/// `tests/the_hub_works_offline.rs` holds them to.
///
/// A fragment with no `</head>` is left alone.
///
/// 30 Sep 2026: it also keeps a slider's `<output>` saying the slider's value
/// as it moves -- it showed the saved value, and dragging changed nothing on
/// screen until Save.
pub fn with_app_head(page: String, token: &str) -> String {
    let Some(at) = page.find("</head>") else { return page };
    let t = esc(token);
    let tags = format!(
        "<link rel=manifest href=\"{MANIFEST_PATH}?t={t}\">\
         <meta name=theme-color content=\"{APP_BG}\" media=\"(prefers-color-scheme: dark)\">\
         <meta name=theme-color content=\"{APP_BG_LIGHT}\" media=\"(prefers-color-scheme: light)\">\
         <meta name=apple-mobile-web-app-capable content=yes>\
         <meta name=mobile-web-app-capable content=yes>\
         <meta name=apple-mobile-web-app-title content=Atlas>\
         <link rel=apple-touch-icon href=\"/hub/apple-touch-icon.png\">\
         <script>if('serviceWorker' in navigator)\
         navigator.serviceWorker.register('{SERVICE_WORKER_PATH}',{{scope:'/hub'}});\
         document.addEventListener('input',function(e){{var t=e.target;if(!t||t.type!=='range'||!t.id)return;\
         var o=document.querySelector('output[for=\"'+t.id+'\"]');if(!o)return;\
         o.textContent=(Number(t.step)<1?Number(t.value).toFixed(2):t.value)+(o.getAttribute('data-unit')||'')}})</script>"
    );
    format!("{}{tags}{}", &page[..at], &page[at..])
}

/// The service worker.
///
/// Network only, with a fallback: the hub is live state, and it stores
/// nothing on the phone — `tests/server_safety.rs` holds that "a phone must
/// not cache workspace state", and a service worker's cache would ignore the
/// pages' `no-store`. When the laptop can't be reached, it shows one sentence
/// saying where Atlas is, which reloads itself every thirty seconds so the app
/// comes back on its own (30 Sep 2026: the thirty seconds were promised here
/// and only the `online` event was listened for; and a laptop whose Atlas is
/// down answers through Tailscale with a 502, which now counts as away too).
/// A form post is never touched. A laptop that's asleep can leave the
/// request hanging rather than refused, so a page that hasn't answered in
/// twelve seconds counts as away too (research report, Stage 1 item 10).
pub const SERVICE_WORKER: &str = r#"const OFFLINE="<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>Atlas — can't reach Atlas</title><style>body{margin:0;min-height:100vh;display:flex;flex-direction:column;align-items:center;justify-content:center;background:#ffffff;color:#37352f;font:17px/1.5 system-ui,sans-serif}p{max-width:22em;padding:0 24px}button{font:inherit;padding:12px 20px;min-height:44px;border-radius:10px;border:0;background:#b26206;color:#fff}</style></head><body><main><p role=status>I can't reach Atlas from here right now. This page comes back by itself when it can.</p><p><button onclick='location.reload()'>Try again</button></p></main><script>addEventListener('online',function(){location.reload()});setTimeout(function(){location.reload()},30000)</script></body></html>";
function underHub(u){return u.origin===location.origin&&(u.pathname==='/hub'||u.pathname.startsWith('/hub/'));}
self.addEventListener('install',()=>self.skipWaiting());
self.addEventListener('activate',e=>e.waitUntil(caches.keys().then(ks=>Promise.all(ks.map(k=>caches.delete(k)))).then(()=>self.clients.claim())));
self.addEventListener('fetch',e=>{
  const r=e.request;
  if(r.method!=='GET'||r.mode!=='navigate')return;
  if(!underHub(new URL(r.url)))return;
  const off=()=>new Response(OFFLINE,{headers:{'Content-Type':'text/html; charset=utf-8'}});
  const c=new AbortController();const t=setTimeout(()=>c.abort(),12000);
  e.respondWith(fetch(r,{signal:c.signal}).then(x=>{clearTimeout(t);return (x.status>=502&&x.status<=504)?off():x}).catch(()=>{clearTimeout(t);return off()}));
});
"#;

/// The page, with the navigation knowing where you are.
///
/// The header carries the three things that must never be more than one click
/// away no matter how deep you are: the way home, anything waiting for you,
/// and the settings. Settings buried three levels down is how a tool teaches
/// you it does not expect you to change anything.
fn shell_with(here: Option<Page>, title: &str, body: &str, waiting: usize) -> String {
    let trail = here.map(crumbs).unwrap_or_default();
    let heading = match here {
        Some(p) if p != Page::Dashboard => format!(
            "<h1 class=page>{}</h1><p class=pagenote>{}</p>",
            esc(p.label()),
            esc(p.note())
        ),
        _ => String::new(),
    };
    format!(
        "<!doctype html><html lang=en{theme}><head><meta charset=utf-8>\
         <meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <title>Atlas — {t}</title><style>{STYLE}</style></head><body>\
         <a class=skip href='#main'>Skip to the page</a>\
         <div class=wrap>{sidebar}<div class=col-main>\
         <header><a class=hbrand href='/hub' aria-label='Atlas, Home'>{MARK}<span>Atlas</span></a>{trail}<span class=grow></span>\
         <a class=tool id=palopen href='/hub/find' aria-label='Find anything'>{SEARCH_ICON}<span class=lbl>Find anything</span><kbd class=ck>Ctrl K</kbd></a>\
         <a class='tool talkbtn' href='/hub/talk' aria-label='Talk'>{MIC_ICON}</a>\
         {waiting_link}<a class='tool help' href='/hub/help' aria-label='Help'>{HELP_ICON}<span class=lbl>Help</span></a>{theme_chip}{menu}</header>\
         <main id=main tabindex=-1>{heading}{body}</main></div></div>{tabs}</body></html>",
        t = esc(title),
        // Two stored appearances, merged 26 Sep 2026. The hub's own "Aa" menu
        // (theme paper/light/dark/auto, text size, contrast, motion) is written in by
        // `with_appearance`, right after `<html lang=en`, so it comes first and
        // wins where both say the same thing. The Settings page's choices
        // (`appearance`: Warm Paper by default, or Ember Dark, the
        // colour-blind theme or the system's; an accent, a colour-blind mode,
        // density) are written here.
        theme = crate::appearance::Appearance::load().html_attrs(),
        sidebar = sidebar_html(here, waiting),
        tabs = tabs_html(here, waiting),
        menu = menu_html(here),
        theme_chip = appearance_menu(),
        waiting_link = if waiting > 0 {
            format!(
                "<a class=tool href='{}'><span class=dot></span>{} waiting</a>",
                Page::Outstanding.href(),
                waiting
            )
        } else {
            format!("<a class=tool href='{}'>Nothing waiting</a>", Page::Outstanding.href())
        },
    )
}

/// The presence rail: Atlas's mark, the four pages you use most, every other
/// page one press away, and Settings. Eric's command-deck design (23 Sep 2026).
///
/// Drawn, not fetched: the icons are inline SVG, because the hub works offline
/// and an icon font is the first thing to go missing.
/// The design's mark: a ring bearing a point, in the accent.
/// Atlas's mark, the Folded A, in the page's own colours. The same shapes as
/// `mark::for_the_hub()`, written out because the pages build with `format!`
/// and a test holds the two together.
pub const MARK: &str = "<svg class=mark viewBox='0 0 100 100' aria-hidden=true><path d='M13.51 92.00 L34.49 92.00 L60.49 10.00 L39.51 10.00 Z' fill='var(--ink)'/><path d='M86.49 92.00 L65.51 92.00 L39.51 10.00 L60.49 10.00 Z' fill='var(--accent)'/><path d='M60.49 10.00 L39.51 10.00 L50.00 43.09 Z' fill='color-mix(in srgb, var(--accent) 78%, #000)'/><circle cx='50' cy='79' r='6.5' fill='var(--accent)'/></svg>";

/// A page's icon in the sidebar: 24-unit strokes, drawn here, nothing fetched.
fn icon(page: Page) -> &'static str {
    match page {
        Page::Dashboard => "<path d='M3 11l9-8 9 8M5 10v10h14V10'/>",
        Page::Now => "<circle cx=12 cy=12 r=9 /><path d='M12 7v5l3 2'/>",
        Page::Calendar => "<rect x=3 y=4.5 width=18 height=16.5 rx=2.5 /><path d='M3 9.5h18M8 2.5v4M16 2.5v4'/>",
        Page::Workshop => "<path d='M4 6h16M4 12h16M4 18h10'/>",
        Page::Outstanding => "<path d='M9 11l3 3L22 4M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11'/>",
        Page::Workspace => "<rect x=3 y=3 width=7 height=7 rx=1 /><rect x=14 y=3 width=7 height=11 rx=1 /><rect x=3 y=14 width=7 height=7 rx=1 />",
        Page::LookingBack => "<path d='M3 12a9 9 0 1 0 9-9'/><path d='M3 4v5h5'/>",
        Page::Friends => "<circle cx=9 cy=8 r=3.2 /><path d='M15 21v-2a4 4 0 0 0-4-4H7a4 4 0 0 0-4 4v2'/>",
        Page::Groups => "<path d='M21 15a2 2 0 0 1-2 2H8l-4 4V5a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2z'/>",
        Page::Messages => "<path d='M21 15a2 2 0 0 1-2 2H8l-4 4V5a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2z'/>",
        Page::Documents => "<path d='M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z'/><path d='M14 3v6h6'/>",
        Page::Business => "<path d='M3 21V7l9-4 9 4v14'/>",
        Page::SharedTasks => "<path d='M9 11l3 3L22 4'/>",
        Page::Clients => "<circle cx=9 cy=8 r=3.2 /><path d='M15 21v-2a4 4 0 0 0-4-4H7a4 4 0 0 0-4 4v2'/>",
        Page::Partners => "<circle cx=12 cy=12 r=9 /><path d='M12 3v18M3 12h18'/>",
        Page::Sound => "<path d='M11 5L6 9H2v6h4l5 4V5z'/><path d='M15.5 8.5a5 5 0 0 1 0 7'/>",
        Page::Trusted => "<path d='M12 3l7 3v6c0 4-3 7-7 9-4-2-7-5-7-9V6z'/><path d='M9 12l2 2 4-4'/>",
        Page::Give => "<path d='M12 5v14M5 12h14'/>",
        Page::Social => "<path d='M4 19V9M10 19V5M16 19v-7M22 19H2'/>",
        Page::Opportunities => "<circle cx=11 cy=11 r=7 /><path d='M21 21l-5-5'/>",
        Page::Offline => "<path d='M2 8.8a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0'/><path d='M12 20h.01'/>",
        Page::Talk => "<rect x=9 y=3 width=6 height=12 rx=3 /><path d='M6 11a6 6 0 0 0 12 0M12 17v4'/>",
        Page::Help => "<circle cx=12 cy=12 r=9 /><path d='M9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.7M12 17h.01'/>",
        Page::Updates => "<path d='M21 12a9 9 0 1 1-3-6.7L21 8'/><path d='M21 3v5h-5'/>",
        Page::Phone => "<rect x=6.5 y=2.5 width=11 height=19 rx=2.5 /><path d='M11 18.5h2'/>",
        Page::Feedback => "<path d='M21 15a2 2 0 0 1-2 2H8l-4 4V5a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2z'/><path d='M12 7v4M12 14h.01'/>",
        Page::Settings => "<circle cx=12 cy=12 r=3 /><path d='M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2'/>",
        _ => "<circle cx=12 cy=12 r=8 />",
    }
}

/// The navigation the design draws: a labelled sidebar in Notion's calm shape.
///
/// The brand, Search and Now at the top; the Personal pages; the people you
/// talk to; a business's own group, once a business exists (written in by
/// `with_business`, since only the running Atlas knows your businesses);
/// everything else one press away under More; Settings at the foot.
///
/// It replaced the command deck's icon rail on 26 Sep 2026. Four icons and a
/// grid button hid every page's name, which is the opposite of the calm,
/// read-at-a-glance structure Eric chose on 20-21 Sep.
fn sidebar_html(here: Option<Page>, waiting: usize) -> String {
    let item = |page: Page, name: &str, extra: &str| {
        format!(
            "<a class='nav{on}' href='{href}'{cur}><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>{paths}</svg>{name}{extra}</a>",
            on = if here == Some(page) { " on" } else { "" },
            cur = if here == Some(page) { " aria-current=page" } else { "" },
            href = page.href(),
            paths = icon(page),
            name = esc(name),
        )
    };
    let mut out = format!(
        "<nav class=sidebar aria-label='Atlas'><a class=brand href='/hub' aria-label='Atlas, Home'>{MARK}<span class=owner>Atlas</span></a>\
         <a class=nav href='/hub/find'><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>\
         <circle cx=11 cy=11 r=7 /><path d='M21 21l-4-4'/></svg>Search<kbd class=ck>Ctrl K</kbd></a>"
    );
    // The sidebar's own groups, shown open. Settings sits at the foot rather
    // than in a group, as drawn; the rest go under More.
    let shown = |g: &str| g.is_empty() || g == "Personal" || g == "People";
    for (group, pages) in NAV.iter().filter(|(g, _)| shown(g)) {
        if !group.is_empty() {
            out.push_str(&format!("<p class=grp>{}</p>", esc(group)));
        }
        for page in *pages {
            let extra = if *page == Page::Outstanding && waiting > 0 {
                format!("<span class=count>{waiting}</span>")
            } else {
                String::new()
            };
            out.push_str(&item(*page, page.label(), &extra));
        }
    }
    // The three things from Your devices people reach for, shown open.
    out.push_str("<p class=grp>Your devices</p>");
    for page in [Page::Phone, Page::Updates, Page::Help] {
        out.push_str(&item(page, page.label(), ""));
    }
    out.push_str(BUSINESS_SLOT);
    // Eric, 27 Sep 2026: More was "too complicated and not organized" --
    // twenty-six pages in five groups. Now two short menus: More, for your own
    // work and the business pages; Atlas setup, for how Atlas behaves, what
    // it's connected to, and Atlas about itself.
    let folded = |label: &str, groups: &[(&str, &[Page])]| {
        let inside = here.is_some_and(|h| groups.iter().any(|(_, ps)| ps.contains(&h)));
        let mut m = format!(
            "<details class=more{open}><summary class=nav><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>\
             <circle cx=5 cy=12 r=1.6 /><circle cx=12 cy=12 r=1.6 /><circle cx=19 cy=12 r=1.6 /></svg>{label}</summary><div class=side>",
            open = if inside { " open" } else { "" },
        );
        for (group, pages) in groups {
            m.push_str(&format!("<p class=gh>{}</p>", esc(group)));
            for page in *pages {
                m.push_str(&format!(
                    "<a class='{}' href='{}'{}>{}</a>",
                    if here == Some(*page) { "here" } else { "" },
                    page.href(),
                    if here == Some(*page) { " aria-current=page" } else { "" },
                    esc(page.label())
                ));
            }
        }
        m.push_str("</div></details>");
        m
    };
    out.push_str(&folded(
        "More",
        &[("Your work", &[Page::Workspace, Page::LookingBack, Page::Give, Page::Social, Page::Opportunities]), ("Business", &[Page::Business, Page::SharedTasks, Page::Clients, Page::Partners])],
    ));
    out.push_str(&folded(
        "Atlas setup",
        &[
            ("How Atlas behaves", &[Page::Sound, Page::Permissions, Page::Gestures, Page::Access]),
            ("What it's connected to", &[Page::Accounts, Page::AddOns, Page::Connections, Page::Sync, Page::Offline]),
            ("About Atlas", &[Page::Status, Page::Recommendations, Page::Activity, Page::Edits, Page::Feedback]),
        ],
    ));
    out.push_str("<span class=grow></span>");
    out.push_str(&item(Page::Settings, "Settings", ""));
    out.push_str("</nav>");
    out
}

/// Every page, one press away: the phone's Menu, at the end of its one-row
/// top bar. (On a laptop, a tablet or an unfolded phone the sidebar is this.)
fn menu_html(here: Option<Page>) -> String {
    let mut out = String::from(
        "<details class=allpages><summary class='tool icon' aria-label='Menu, every page'><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><path d='M4 6h16M4 12h16M4 18h16'/></svg></summary><div class=side>",
    );
    for (group, pages) in NAV {
        out.push_str(&format!("<p class=gh>{}</p>", esc(if group.is_empty() { "Atlas" } else { group })));
        for page in *pages {
            out.push_str(&format!(
                "<a class='{}' href='{}'{}>{}</a>",
                if here == Some(*page) { "here" } else { "" },
                page.href(),
                if here == Some(*page) { " aria-current=page" } else { "" },
                esc(page.label())
            ));
        }
    }
    out.push_str("</div></details>");
    out
}

const SEARCH_ICON: &str = "<svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><circle cx=11 cy=11 r=7 /><path d='M21 21l-4-4'/></svg>";
const MIC_ICON: &str = "<svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><rect x=9 y=3 width=6 height=12 rx=3 /><path d='M6 11a6 6 0 0 0 12 0M12 17v4'/></svg>";
const HELP_ICON: &str = "<svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><circle cx=12 cy=12 r=9 /><path d='M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17.5v.01'/></svg>";

/// The phone's bottom tab bar, as the design's phone screens (24 Sep) draw
/// it: five, evenly spaced — Home, Projects, Messages, Business, Settings.
/// Talk is the microphone in the top bar, not a sixth tab (Eric, 26 Sep: a
/// raised sixth sat off-centre). What's waiting on you is a badge on Home.
fn tabs_html(here: Option<Page>, waiting: usize) -> String {
    let mut out = String::from("<nav class=tabs aria-label='Main'>");
    for (p, name) in [
        (Page::Dashboard, "Home"),
        (Page::Workshop, "Projects"),
        (Page::Messages, "Messages"),
        (Page::Business, "Business"),
        (Page::Settings, "Settings"),
    ] {
        let on = here == Some(p);
        out.push_str(&format!(
            "<a class='tab{}' href='{}'{}><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>{}</svg>{}{}</a>",
            if on { " on" } else { "" },
            p.href(),
            if on { " aria-current=page" } else { "" },
            icon(p),
            name,
            if p == Page::Dashboard && waiting > 0 {
                format!("<span class=badge><span class=sr>, </span>{waiting}<span class=sr> waiting</span></span>")
            } else {
                String::new()
            }
        ));
    }
    out.push_str("</nav>");
    out
}

/// Where a business's group goes in the sidebar, until one is written in.
const BUSINESS_SLOT: &str = "<!--atlas:business-->";

/// Your businesses in the sidebar, each as its own group — the design's
/// "Northwind LLC · business" — or nothing at all when there are none: the
/// Business section appears only once a business is added.
///
/// Each group opens that business's own pages: Overview, Shared tasks,
/// Clients and Partners, as the design draws them.
pub fn with_business(page: String, businesses: &[String]) -> String {
    if businesses.is_empty() {
        return page.replacen(BUSINESS_SLOT, "", 1);
    }
    let mut g = String::new();
    for b in businesses {
        let q = format!("b={}", esc(&crate::research::urlencode(b)));
        g.push_str(&format!("<p class=grp>{} <small>· business</small></p>", esc(b)));
        for p in [Page::Business, Page::SharedTasks, Page::Clients, Page::Partners] {
            g.push_str(&format!(
                "<a class=nav href='{}?{q}'><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>{}</svg>{}</a>",
                p.href(),
                icon(p),
                esc(p.label())
            ));
        }
    }
    page.replacen(BUSINESS_SLOT, &g, 1)
}

/// The owner's name on the sidebar's brand — "Eric's Atlas" — when Atlas
/// knows what to call you. Written in by the running Atlas, like the rest.
pub fn with_owner(page: String, name: Option<&str>) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => page.replacen(
            "<span class=owner>Atlas</span>",
            &format!("<span class=owner>{}'s Atlas</span>", esc(n)),
            1,
        ),
        None => page,
    }
}

fn control(s: &crate::settings::Setting) -> String {
    let key = esc(&s.key);
    // Every control carries its setting's name, so a screen reader and voice
    // control hear "Speaking volume", not "edit text" (WCAG 1.3.1, 4.1.2).
    let name = esc(&s.name);
    match &s.value {
        Value::Toggle(on) => {
            let (label, next) = if *on { ("Turn off", "off") } else { ("Turn on", "on") };
            let confirm = if s.weight.needs_confirming() && !*on {
                // The words in an attribute of their own, read by the script:
                // put inside the script's quotes, an apostrophe ("that's")
                // ended the string, the handler didn't compile, and the switch
                // turned on with no question asked (29 Sep 2026).
                format!(" data-confirm=\"{}\" onsubmit=\"return confirm(this.dataset.confirm)\"", esc(&s.cost))
            } else {
                String::new()
            };
            format!(
                "<span class={} >{}</span> <form method=post action=/hub/set{confirm}>\
                 <input type=hidden name=key value=\"{key}\">\
                 <input type=hidden name=value value=\"{next}\">\
                 <button aria-label=\"{label} {name}\">{label}</button></form>",
                if *on { "on" } else { "off" },
                if *on { "on" } else { "off" }
            )
        }
        Value::Choice { value, options } => {
            let opts: String = options
                .iter()
                .map(|o| {
                    format!(
                        "<option{}>{}</option>",
                        if o == value { " selected" } else { "" },
                        esc(o)
                    )
                })
                .collect();
            format!(
                "<form method=post action=/hub/set><input type=hidden name=key value=\"{key}\">\
                 <select name=value aria-label=\"{name}\">{opts}</select><button aria-label=\"Save {name}\">Save</button></form>"
            )
        }
        Value::Number { value, min, max } => format!(
            "<form method=post action=/hub/set><input type=hidden name=key value=\"{key}\">\
             <input name=value type=number step=any min={min} max={max} value=\"{value}\" aria-label=\"{name}\">\
             <button aria-label=\"Save {name}\">Save</button></form>"
        ),
        Value::Text(v) => format!(
            "<form method=post action=/hub/set><input type=hidden name=key value=\"{key}\">\
             <input autocomplete=off name=value value=\"{}\" aria-label=\"{name}\"><button aria-label=\"Save {name}\">Save</button></form>",
            esc(v)
        ),
        Value::List(v) => format!(
            "<form method=post action=/hub/set><input type=hidden name=key value=\"{key}\">\
             <input autocomplete=off name=value value=\"{}\" placeholder=\"comma separated\" aria-label=\"{name}\">\
             <button aria-label=\"Save {name}\">Save</button></form>",
            esc(&v.join(", "))
        ),
    }
}

fn tag(w: Weight) -> String {
    let class = match w {
        Weight::Permission => "tag perm",
        Weight::Sensitive => "tag sens",
        _ => "tag",
    };
    format!("<span class=\"{class}\">{}</span>", w.label())
}

/// The workspace page.
///
/// A board rather than a list. The columns are statuses, so what's stuck is
/// visible as a shape rather than as a word you have to read.
pub fn workspace_page(
    view: &crate::workspace_view::View,
    groups: &[(String, Vec<&crate::workspace_view::Item>)],
    overview: &crate::workspace_view::Overview,
    other_views: &[String],
) -> String {
    let mut body = String::new();

    // The numbers, but only the ones you'd act on. A count of everything is
    // an accusation rather than information.
    body.push_str("<div class=board-top>");
    for (n, label) in [
        (overview.needs_you, "need you"),
        (overview.doing, "in hand"),
        (overview.blocked, "stuck"),
        (overview.overdue, "past date"),
    ] {
        if n > 0 {
            body.push_str(&format!(
                "<span class=stat><b>{n}</b> {}</span>",
                esc(label)
            ));
        }
    }
    body.push_str("</div>");

    // Views are saved questions, not folders — so they're links across the
    // top rather than a tree down the side.
    body.push_str("<nav class=views>");
    for v in other_views {
        let here = *v == view.name;
        body.push_str(&format!(
            "<a class='{}' href='/hub/workspace?view={}'>{}</a>",
            if here { "view here" } else { "view" },
            esc(v),
            esc(v)
        ));
    }
    body.push_str("</nav>");

    if groups.is_empty() {
        body.push_str("<p class=note>Nothing here.</p>");
        return shell_at(Some(Page::Workspace), &view.name, &body);
    }

    body.push_str("<div class=columns>");
    for (name, items) in groups {
        body.push_str(&format!(
            "<section class=col><h2>{} <small>{}</small></h2>",
            esc(name),
            items.len()
        ));
        for i in items {
            let stuck = i.blocked_by.is_some();
            body.push_str(&format!(
                "<article class='card{}'><span class=what>{}</span>",
                if stuck { " waiting" } else { "" },
                esc(&i.title)
            ));
            if let Some(p) = &i.project {
                body.push_str(&format!("<span class=chip>{}</span>", esc(p)));
            }
            if let Some(b) = &i.blocked_by {
                body.push_str(&format!("<span class=why>waiting on {}</span>", esc(b)));
            }
            body.push_str("</article>");
        }
        body.push_str("</section>");
    }
    body.push_str("</div>");

    if let Some((what, n)) = &overview.biggest_blocker {
        // The single most useful line on the page.
        body.push_str(&format!(
            "<p class=note><b>{n}</b> things are waiting on {}. Unsticking that is worth more \
             than anything else here.</p>",
            esc(what)
        ));
    }
    shell_at(Some(Page::Workspace), &view.name, &body)
}

/// A past day.
///
/// What actually got finished, what was already open, and what Atlas was
/// thinking as it went. The last of those is the part no task app has, and
/// it's the one that answers "why did this take three days".
pub fn looking_back_page(day: &crate::workspace_view::Day, label: &str) -> String {
    let mut body = format!("<h2>{}</h2>", esc(label));

    body.push_str("<div class=board-top>");
    for (n, l) in [
        (day.finished.len(), "finished"),
        (day.started.len(), "started"),
        (day.carried_over.len(), "carried over"),
    ] {
        body.push_str(&format!("<span class=stat><b>{n}</b> {}</span>", esc(l)));
    }
    body.push_str("</div>");

    if !day.finished.is_empty() {
        body.push_str("<h3 class=sec>Finished</h3><ul class=rows>");
        for f in &day.finished {
            body.push_str(&format!("<li class=row>{}</li>", esc(f)));
        }
        body.push_str("</ul>");
    }

    // Atlas's working out, in the order it happened. Grouped by item, because
    // "why did this take three days" is a question about one thing and
    // hunting for it in a stream is why nobody ever does.
    if day.thoughts.is_empty() {
        body.push_str("<p class=note>Nothing recorded that day.</p>");
    } else {
        body.push_str("<h3 class=sec>What I was thinking</h3><div class=thoughts>");
        let mut last = String::new();
        for (item, t) in &day.thoughts {
            if *item != last {
                body.push_str(&format!("<div class=thought-item>{}</div>", esc(item)));
                last = item.clone();
            }
            body.push_str(&format!(
                "<div class=thought><span class=when>{}</span>{} {}</div>",
                esc(t.kind.plain()),
                "",
                esc(&t.what)
            ));
        }
        body.push_str("</div>");
    }
    shell_at(Some(Page::LookingBack), "Looking back", &body)
}

/// What Atlas would change about itself.
///
/// A page rather than an interruption, because none of this is urgent — it has
/// waited this long and can wait until you're looking. Each one carries its
/// evidence so you can disagree with that rather than with the conclusion.
pub fn recommendations_page(
    recs: &[crate::selfaudit::Recommendation],
    granted: Option<&str>,
    // Ways Atlas can get better on the hardware already here: each with
    // whether it's on here, and the button that turns it on when it isn't
    // (30 Sep 2026: they were a list you could only read).
    free_wins: &[FreeWin],
) -> String {
    let mut body = String::new();

    match granted {
        Some(level) => body.push_str(&format!(
            "<p class=note>I fix <b>{}</b> on my own and tell you after. Everything below \
             reaches further than that, so it's yours.</p>",
            esc(level)
        )),
        None => body.push_str(
            "<p class=note>I'm not fixing anything on my own at the moment. Everything below is \
             waiting on you.</p>",
        ),
    }

    if recs.is_empty() {
        body.push_str("<p class=note>Nothing I'd change.</p>");
    }

    for r in recs.iter() {
        // Certainty as a word rather than a number — a percentage on a
        // self-assessment is false precision.
        let sure = if r.certainty > 0.8 {
            "fairly sure"
        } else if r.certainty > 0.5 {
            "reasonably sure"
        } else {
            "not certain"
        };

        body.push_str(&format!(
            "<article class=rec><h2>{}</h2>\
             <p class=cause>I think {}.</p>\
             <p class=evidence>{} — {}</p>\
             <p class=proof>What would prove it: {}</p>\
             <form method=post action=/hub/recommendations/go>\
             <input type=hidden name=which value='{}'>\
             <button class=go>Have a go</button>\
             <button class=revoke name=drop value='1'>Not worth it</button>\
             </form></article>",
            esc(&r.symptom),
            esc(&r.cause),
            esc(sure),
            esc(&r.because),
            esc(&r.proof),
            // By what it says, not where it sat: the list can change between
            // drawing the page and pressing the button, and a position would
            // then act on a different idea (27 Sep 2026).
            esc(&r.symptom)
        ));
    }
    // Ways to get better on the hardware already here. `improve` listed these
    // and nothing ever asked it.
    if !free_wins.is_empty() {
        body.push_str("<h2>Free wins</h2><ul class=tight>");
        for w in free_wins {
            let act = match &w.get {
                Some((label, value)) => format!(
                    " <form class=inline method=post action=/hub/brains><input type=hidden name=from value=ideas>\
                     <button name=what value='{}'>{}</button></form>",
                    esc(value),
                    esc(label)
                ),
                None => String::new(),
            };
            body.push_str(&format!("<li><b>{}</b> {} <span class=note>{}</span>{act}</li>", esc(&w.what), esc(&w.worth), esc(&w.here)));
        }
        body.push_str("</ul>");
    }

    shell_at(Some(Page::Recommendations), "Ideas", &body)
}

/// One free win on the Ideas page.
#[derive(Debug, Clone, PartialEq)]
pub struct FreeWin {
    pub what: String,
    pub worth: String,
    /// Whether it's working here, in a few words.
    pub here: String,
    /// The button that turns it on, when it isn't: (label, `/hub/brains` value).
    pub get: Option<(String, String)>,
}

/// Which switch in the hub turns on which registered capability, as
/// `(capability id, setting key)`.
///
/// Only pairs where the two are plainly the same feature: same name on both
/// sides, or the capability's `modules`/`needs` name the thing the setting's
/// own text says it needs. Left out on purpose:
///
/// * `voice.enabled` — it spans `wake` (whisper) *and* `speak` (piper); with
///   only one of them blocked the switch still does half its job.
/// * `accents` — no switch of its own.
/// * `reason` — its only setting is `models.memory_budget_mb`, a number.
/// * `delegate_online` — no switch; `budget.enabled` is the paid model, not
///   the Cloudflare worker.
/// * `recall.semantic` — `recall` works by words without a model, so a
///   blocked `recall` would not mean the meaning-search switch was idle.
/// * `watching.enabled` — it is finish alerts; `watch` is window tracking,
///   even though `watch` claims the `watching` module.
pub const IDLE_TOGGLES: &[(&str, &str)] = &[
    ("wake", "wake.enabled"),
    ("endpoint", "endpoint.enabled"),
    ("dictate", "dictate.enabled"),
    ("ocr", "ocr.enabled"),
    // Both switches name the multilingual model; the capability is
    // "understand and translate other languages" and needs exactly that.
    ("translate", "language.multilingual"),
    ("translate", "language.translate_others"),
    ("research", "research.enabled"),
    ("prose", "prose.enabled"),
    ("mail", "mail.enabled"),
    ("unsub", "unsub.enabled"),
    ("system", "system.enabled"),
    ("tune", "tune.enabled"),
    ("signin", "signin.enabled"),
    ("confirmed", "confirmed.enabled"),
    ("certainty", "certainty.enabled"),
    ("vision", "vision.enabled"),
    ("goingaway", "going_away.enabled"),
    ("selfwork", "self_work.enabled"),
];

/// A one-line nudge naming switches that are on but can't do anything,
/// because the capability behind them is blocked on something not installed.
///
/// Plain text, not HTML — the caller escapes it. `None` when there is nothing
/// to say, which is the usual case.
pub fn idle_banner(s: &Settings, blocked_capabilities: &[String]) -> Option<String> {
    let keys: Vec<String> = IDLE_TOGGLES
        .iter()
        .filter(|(cap, _)| blocked_capabilities.iter().any(|b| b.as_str() == *cap))
        .map(|(_, key)| key.to_string())
        .collect();
    if keys.is_empty() {
        return None;
    }
    let idle = s.idle_but_on(&keys);
    if idle.is_empty() {
        return None;
    }
    let names: Vec<&str> = idle.iter().map(|item| item.name.as_str()).collect();
    Some(format!(
        "On but doing nothing right now: {} — what {} needs isn't installed yet, \
         so you could switch {} off until it is.",
        names.join(", "),
        if names.len() == 1 { "it" } else { "each" },
        if names.len() == 1 { "it" } else { "them" },
    ))
}

pub fn settings_page(s: &Settings) -> String {
    let groups = s.groups();
    let changed = s.changed().len();

    let mut body = String::new();
    // The design's Settings index: one place that ties it together, with the
    // cards that open each area. The full list of switches follows.
    body.push_str("<nav class=setcards aria-label='Settings areas'>");
    for (href, title, what) in [
        ("#how-it-looks", "Appearance & access", "Colourway, accent, text size, density, colour-blind mode."),
        (Page::Sound.href(), "Sound & voice", "Atlas's voice, volume, the wake word, quiet hours, mute — and when it may pop up."),
        (Page::Gestures.href(), "Gestures", "Hands-free on camera: whether it's on, and how each works."),
        (Page::Trusted.href(), "Trusted recipients", "Who Atlas can send to without asking."),
        (Page::Accounts.href(), "Calendars & accounts", "Mailboxes, phone calendar sync, connected devices."),
        (Page::Help.href(), "Help & accessibility", "Using Atlas with a screen reader, keyboard or larger text; the statement."),
    ] {
        body.push_str(&format!(
            "<a class=setcard href='{href}'><b>{}</b><span>{}</span></a>",
            esc(title),
            esc(what)
        ));
    }
    body.push_str("</nav>");
    body.push_str(&format!(
        "<p class=note>A change is kept the moment you make it, and a running Atlas \
         picks it up within seconds. The few that set something up at the start — the \
         voice, the wake word, phone access — wait for a restart. This page works when the voice is down too, and if Atlas itself won't \
         start, open Atlas from the Start menu and choose <b>Settings</b> in its window.{}</p>",
        if changed > 0 {
            format!(
                " You've changed {changed} thing{} from the defaults.",
                if changed == 1 { "" } else { "s" }
            )
        } else {
            String::new()
        }
    ));

    // Switches that are on but have nothing to run on. The capability table
    // is the one place that knows what's blocked, so ask it here rather than
    // making every caller of this page pass the list in.
    let blocked: Vec<String> = crate::capability::blocked()
        .into_iter()
        .map(|(c, _)| c.id.to_string())
        .collect();
    if let Some(banner) = idle_banner(s, &blocked) {
        body.push_str(&format!("<p class=note>{}</p>", esc(&banner)));
    }

    // How it looks: the colourway (Warm Paper by default), accent, colour-blind
    // mode and density. These were kept by `appearance` with no page to
    // change them until 26 Sep 2026.
    body.push_str("<h2 id='how-it-looks'>How it looks</h2>");
    body.push_str(&crate::appearance::Appearance::load().settings_html());

    // The index. Forty-odd settings down one page is a scroll, not a choice —
    // and the thing you came to change is never the one at the top.
    body.push_str("<div class=jump>");
    for g in &groups {
        body.push_str(&format!(
            "<a href='#{}'>{}<span>{}</span></a>",
            slug(g),
            esc(g),
            s.in_group(g).len()
        ));
    }
    body.push_str("</div>");

    for group in &groups {
        body.push_str(&format!(
            "<h2 id='{}'>{}</h2>",
            slug(group),
            esc(group)
        ));
        // A heading with nothing under it makes you open every section to
        // find out which one holds the thing you came for.
        if let Some(note) = Settings::group_note(group) {
            body.push_str(&format!("<p class=groupnote>{}</p>", esc(note)));
        }
        for item in s.in_group(group) {
            body.push_str(&format!(
                "<div class=row id='set-{}'><div class=name>{}{}</div>\
                 <div class=what>{}</div>{}<div style=\"margin-top:8px\">{}</div></div>",
                esc(&item.key),
                esc(&item.name),
                tag(item.weight),
                esc(&item.what),
                if item.cost.is_empty() {
                    String::new()
                } else {
                    format!("<div class=cost>{}</div>", esc(&item.cost))
                },
                control(item)
            ));
        }
    }
    shell_at(Some(Page::Settings), "Settings", &body)
}

/// A heading turned into something an anchor can point at.
fn slug(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

/// The page worth reading once a month: everything that can act without
/// asking, or reach a sensor or the network.
/// Your projects, and the queue for each: what's ready to implement, what's
/// being worked, and what's still outstanding. This is the window you sort by
/// project — Atlas's own queue, any other, each on its own.
pub fn workshop_page(w: &crate::workshop::Workshop) -> String {
    let mut body = String::from(
        "<p class=note>Each project's queue. A change waits here, checked and titled, until you \
         say implement — nothing touches your files before that.</p>",
    );
    if w.projects.is_empty() {
        body.push_str(
            "<p>No projects yet. Tell Atlas \"on the &lt;name&gt; project, …\" and one starts here.</p>",
        );
        return shell_at(Some(Page::Workshop), "Projects", &body);
    }
    for p in &w.projects {
        body.push_str(&format!("<h2>{}</h2>", esc(&p.name)));
        if !p.folder.is_empty() {
            body.push_str(&format!("<p class=note>{}</p>", esc(&p.folder)));
        }

        let ready = p.ready();
        if ready.is_empty() {
            body.push_str("<p class=note>Nothing waiting on you.</p>");
        } else {
            body.push_str("<p class=note><b>Waiting on you</b></p>");
            for c in ready {
                let mark = if c.verified { "checked" } else { "unchecked" };
                body.push_str(&format!(
                    "<div class=row><div class=name>{}</div><div class=what>{}</div>\
                     <div class=cost>{mark}</div>\
                     <div style=\"margin-top:8px\">\
                       <form class=inline method=post action=/hub/implement>\
                         <input type=hidden name=title value=\"{}\">\
                         <button type=submit>implement</button>\
                       </form></div></div>",
                    esc(&c.title),
                    esc(&c.what),
                    esc(&c.title),
                ));
            }
        }

        let working = p.in_progress();
        if !working.is_empty() {
            body.push_str("<p class=note><b>Being worked</b></p>");
            for c in working {
                body.push_str(&format!(
                    "<div class=row><div class=name>{}</div><div class=what>{}</div></div>",
                    esc(&c.title),
                    esc(&c.what),
                ));
            }
        }

        let outstanding = p.outstanding();
        if !outstanding.is_empty() {
            body.push_str("<p class=note><b>Outstanding</b></p>");
            for t in outstanding {
                body.push_str(&format!(
                    "<div class=row><div class=name>{}</div></div>",
                    esc(&t.title),
                ));
            }
        }
    }
    shell_at(Some(Page::Workshop), "Projects", &body)
}

/// Your calendar, soonest first. Lean on purpose — what's coming up, when, and
/// where, and nothing else competing for the space.
/// Bring a file in and take one out, by clicking: an invite or contacts file
/// is read straight in (`Daemon::bring_in`), and the calendar and client list
/// download as the files every other app opens. These were typed commands
/// (`atlas calendar import`, `atlas clients export`) until 23 Sep.
fn files_in_and_out() -> String {
    "<div class=files-io>\
     <label class=btn for=bring-in-file>Bring in an invite or contacts file (.ics, .vcf)</label>\
     <input id=bring-in-file type=file accept=\".ics,.vcf,text/calendar,text/vcard\" hidden>\
     <a class=btn href=\"/hub/calendar.ics\">Download your calendar (.ics)</a>\
     <a class=btn href=\"/hub/clients.vcf\">Download your clients (.vcf)</a>\
     <p id=bring-in-said class=note aria-live=polite></p></div>\
     <script>(function(){var f=document.getElementById('bring-in-file'),o=document.getElementById('bring-in-said');\
     f.addEventListener('change',function(){var file=f.files[0];if(!file)return;o.textContent='Reading '+file.name+'…';\
     var r=new FileReader();r.onload=function(){var b64=String(r.result).split(',')[1]||'';\
     fetch('/hub/bring-in',{method:'POST',headers:{'Content-Type':'application/json'},credentials:'same-origin',\
     body:JSON.stringify({name:file.name,data:b64})}).then(function(x){return x.json()}).then(function(j){o.textContent=j.said;\
     setTimeout(function(){location.reload()},2500)}).catch(function(){o.textContent='That didn\\'t reach Atlas. Is it still running?'})};\
     r.readAsDataURL(file)})})();</script>\
     <div class=files-io><h2>Teach Atlas to hear you</h2>\
     <p class=note>Recordings made on this computer's microphone (.wav). Nothing leaves the machine.</p>\
     <label class=btn data-prefix=wake->Your wake phrase — one take (do this three times)<input type=file accept=\".wav,audio/wav\" hidden></label>\
     <label class=btn data-prefix=room->The room, with nobody talking<input type=file accept=\".wav,audio/wav\" hidden></label>\
     <label class=btn data-prefix=you->You, talking somewhere quiet<input type=file accept=\".wav,audio/wav\" hidden></label>\
     <label class=btn data-prefix=voices->Other voices — a podcast or a call<input type=file accept=\".wav,audio/wav\" hidden></label>\
     <p id=hearing-said class=note aria-live=polite></p></div>\
     <script>(function(){var o=document.getElementById('hearing-said');\
     document.querySelectorAll('label[data-prefix]').forEach(function(l){var f=l.querySelector('input');\
     f.addEventListener('change',function(){var file=f.files[0];if(!file)return;o.textContent='Listening to '+file.name+'…';\
     var r=new FileReader();r.onload=function(){var b64=String(r.result).split(',')[1]||'';\
     fetch('/hub/bring-in',{method:'POST',headers:{'Content-Type':'application/json'},credentials:'same-origin',\
     body:JSON.stringify({name:l.dataset.prefix+file.name,data:b64})}).then(function(x){return x.json()}).then(function(j){o.textContent=j.said})\
     .catch(function(){o.textContent='That didn\\'t reach Atlas. Is it still running?'})};r.readAsDataURL(file)})})})();</script>".to_string()
}

/// This week on one time grid — personal, business and Atlas's own on the
/// same calendar (the design's Calendar-Week artboard), each event with its
/// area as a word and a time block hatched as booked time. Each day is also a
/// plain ordered list of what's on it, so a screen reader hears the day in
/// order rather than a picture of boxes.
fn week_html(cal: &crate::calendar::Calendar, now: u64, zone: &crate::tz::Zone) -> String {
    const FIRST: i64 = 7; // 07:00
    const LAST: i64 = 22; // 22:00
    const HOUR_PX: i64 = 40;
    let off = zone.offset_at(now as i64);
    let today = (now as i64 + off).div_euclid(86_400);
    let monday = today - (today + 3).rem_euclid(7);
    let start_utc = (monday * 86_400 - off).max(0) as u64;
    let events = cal.occurrences_between(start_utc, start_utc + 7 * 86_400);
    let mut out = String::from("<section class=weekwrap aria-labelledby=weekh><h2 id=weekh>This week</h2>\
        <div class=week tabindex=0 role=region aria-label='This week — scrolls sideways on a small screen'><div class=hours aria-hidden=true>");
    for h in FIRST..LAST {
        out.push_str(&format!("<span style='top:{}px'>{:02}:00</span>", (h - FIRST) * HOUR_PX, h));
    }
    out.push_str("</div>");
    for d in 0..7 {
        let day = monday + d;
        let (_, m, dd) = crate::hubpages::ymd(day);
        let name = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][d as usize];
        let here: Vec<&crate::calendar::Event> = events
            .iter()
            .filter(|e| (e.start as i64 + off).div_euclid(86_400) == day)
            .collect();
        out.push_str(&format!(
            "<div class='wday{}'><h3 class=wdh>{name} {} {dd}</h3><ol class=wevents style='height:{}px'>",
            if day == today { " today" } else { "" },
            crate::hubpages::MONTHS[(m - 1) as usize],
            (LAST - FIRST) * HOUR_PX
        ));
        if here.is_empty() {
            out.push_str("<li class=sr>Nothing on.</li>");
        }
        for e in here {
            let local = e.start as i64 + off;
            let mins = local.rem_euclid(86_400) / 60;
            let len = ((e.end.max(e.start) - e.start) as i64 / 60).max(20);
            let top = if e.all_day { 0 } else { ((mins - FIRST * 60) * HOUR_PX / 60).clamp(0, (LAST - FIRST) * HOUR_PX - 20) };
            let height = if e.all_day { 22 } else { (len * HOUR_PX / 60).max(20).min((LAST - FIRST) * HOUR_PX - top) };
            let area = match &e.space {
                crate::earned::Space::Business(b) => b.clone(),
                crate::earned::Space::Personal => "Personal".to_string(),
            };
            let block = e.kind == crate::calendar::EventKind::TimeBlock;
            out.push_str(&format!(
                "<li class='wev{}' style='top:{top}px;height:{height}px'><time>{}</time> {}<span class=area>{}</span>{}</li>",
                if block { " block" } else { "" },
                if e.all_day { "All day".to_string() } else { format!("{:02}:{:02}", mins / 60, mins % 60) },
                esc(&e.title),
                esc(&area),
                if block { "<span class=sr> (focus block)</span>" } else { "" },
            ));
        }
        out.push_str("</ol></div>");
    }
    out.push_str("</div></section>");
    out
}

pub fn calendar_page(cal: &crate::calendar::Calendar, now: u64, zone: &crate::tz::Zone) -> String {
    let mut body = String::from(
        "<p class=note>Your own calendar, kept here and offline. The one on your phone syncs in \
         when it's connected.</p>",
    );
    body.push_str(&week_html(cal, now, zone));
    body.push_str("<h2>Coming up</h2>");
    body.push_str(&files_in_and_out());
    // Occurrence-aware so a repeating event shows on each day it's on in the
    // next month, the same as the spoken agenda — not once at its series start.
    let upcoming = cal.occurrences_between(now, now + 30 * 86_400);
    if upcoming.is_empty() {
        body.push_str(
            "<p>Nothing coming up. Tell Atlas \"schedule &lt;something&gt; tomorrow at 3\" and it \
             lands here.</p>",
        );
        return shell_at(Some(Page::Calendar), "Calendar", &body);
    }
    for e in upcoming {
        let place = e.place.as_deref().map(|p| format!(" · {}", esc(p))).unwrap_or_default();
        // The firewall side, shown as a word rather than a colour alone, so a
        // business event reads as one at a glance and the personal/business
        // split is visible on the same combined calendar.
        let area = match &e.space {
            crate::earned::Space::Business(b) => format!(" · {}", esc(b)),
            crate::earned::Space::Personal => String::new(),
        };
        // A time block reads as reserved focus, not a meeting — said as a word
        // so it's clear without relying on colour.
        let kind = match e.kind {
            crate::calendar::EventKind::TimeBlock => " · focus",
            crate::calendar::EventKind::Meeting => "",
        };
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div>\
             <div class=what>{}{place}{area}{kind}</div></div>",
            esc(&e.title),
            esc(&e.say_when_in(zone)),
        ));
    }
    shell_at(Some(Page::Calendar), "Calendar", &body)
}

/// The add-ons on this install: what each asks, what you allowed, and the
/// buttons for approving, taking a permission away, and switching off.
///
/// The approve button carries the fingerprint of the file this page showed,
/// so pressing it approves exactly what you read -- a file changed in between
/// is refused, not approved.
/// Also what friends have shared with you (`offers`), who you can share with
/// (`share_to`: your groups, then paired people) and the groups you could
/// recommend one in.
pub fn addons_page_with(
    plugins: &[crate::plugins::Plugin],
    offers: &[crate::plugins::Offered],
    share_to: &[String],
    groups: &[String],
) -> String {
    use crate::plugins::Status;
    let mut body = String::from(
        "<p class=note>Add-ons give Atlas new things to do by putting together what it already \
         does. None of them run code. Each does nothing until you approve it, only what you \
         allowed, and still asks you first wherever Atlas would ask you. Some things no add-on \
         can ever do: open the vault, pair devices, use your accounts, hand Atlas over, or \
         change Atlas itself.</p>",
    );
    // What friends shared: nothing here is installed. Adding one is one
    // decision, made seeing exactly what it would be allowed to do.
    if !offers.is_empty() {
        body.push_str("<h2>Shared with you</h2>");
        for o in offers {
            let place = match &o.in_group {
                Some(g) => format!("shared by {} in {}", esc(&o.from), esc(g)),
                None => format!("sent to you by {}", esc(&o.from)),
            };
            let perms: Vec<String> = o
                .permissions
                .iter()
                .map(|k| esc(crate::plugins::permission(k).map(|p| p.plain).unwrap_or(k)))
                .collect();
            let form = |what: &str, extra: &str, label: &str| {
                format!(
                    "<form method=post action=/hub/addons style='display:inline'>\
                     <input type=hidden name=what value='{what}'><input type=hidden name=id value='{}'>{extra}\
                     <button class=revoke>{}</button></form> ",
                    o.offer,
                    esc(label)
                )
            };
            // What it would actually do, step by step -- judged by its steps,
            // not only its description.
            let steps: String = serde_yaml::from_str::<crate::plugins::Manifest>(&o.text)
                .map(|m| {
                    m.flows
                        .iter()
                        .map(|f| {
                            let start = match (&f.schedule, f.triggers.first()) {
                                (Some(when), _) => format!("runs {when}"),
                                (None, Some(t)) => format!("say “{t}”"),
                                (None, None) => "nothing starts it".into(),
                            };
                            format!(
                                "<li>“{}” — {}: {}</li>",
                                esc(&f.name),
                                esc(&start),
                                esc(&f.steps.iter().map(|s| s.command.clone()).collect::<Vec<_>>().join(", then "))
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            body.push_str(&format!(
                "<div class=row><div class=name>{}</div><div class=what>{place}. Says it's by {} — only \
                 who sent it is certain.{}</div><div class=cost>It would be allowed to: {}</div><ul>{steps}</ul>{}{}</div>",
                esc(&o.name),
                esc(&o.author),
                if o.description.is_empty() { String::new() } else { format!(" {}", esc(&o.description)) },
                if perms.is_empty() { "nothing beyond talking back".to_string() } else { perms.join("; ") },
                form("take", &format!("<input type=hidden name=sha value='{}'>", esc(&o.sha256)), "Add it and allow that"),
                form("decline", "", "No thanks"),
            ));
        }
        body.push_str("<h2>Yours</h2>");
    }
    if plugins.is_empty() {
        body.push_str(&nothing(
            "No add-ons yet. Add-ons friends share with you appear here.",
        ));
    }
    let button = |what: &str, id: &str, extra: &str, label: &str| {
        format!(
            "<form method=post action=/hub/addons style='display:inline'>\
             <input type=hidden name=what value='{}'><input type=hidden name=id value='{}'>{extra}\
             <button class=revoke>{}</button></form> ",
            esc(what),
            esc(id),
            esc(label)
        )
    };
    for p in plugins {
        let mut row = format!(
            "<div class=row><div class=name>{}</div><div class=what>{}</div>",
            esc(&p.name()),
            esc(&p.status.plain())
        );
        if let Some(m) = &p.manifest {
            row.push_str(&format!(
                "<div class=cost>By {}{}</div>",
                esc(&m.author),
                if m.description.is_empty() { String::new() } else { format!(" — {}", esc(&m.description)) }
            ));
            row.push_str("<ul>");
            for k in &m.permissions {
                let plain = crate::plugins::permission(k).map(|x| x.plain).unwrap_or("");
                let allowed = p.granted.contains(k);
                row.push_str(&format!(
                    "<li>{} {}{}</li>",
                    if allowed { "<b>allowed:</b>" } else { "asks to:" },
                    esc(plain),
                    if allowed && p.status == Status::Active {
                        button("revoke", &p.id, &format!("<input type=hidden name=key value='{}'>", esc(k)), "take this away")
                    } else {
                        String::new()
                    }
                ));
            }
            for f in &p.flows {
                let starts = if f.triggers.is_empty() {
                    "nothing starts it".to_string()
                } else {
                    format!("say “{}”", f.triggers.join("” or “"))
                };
                row.push_str(&format!(
                    "<li>“{}” — {}: {}</li>",
                    esc(&f.name),
                    esc(&starts),
                    esc(&f.steps.iter().map(|s| s.command.clone()).collect::<Vec<_>>().join(", then "))
                ));
            }
            row.push_str("</ul>");
        }
        if let Some(who) = &p.sent_by {
            row.push_str(&format!(
                "<div class=what>Sent to you by {} — your paired Atlas, so that part is certain; \
                 who wrote it is only what the file says.</div>",
                esc(who)
            ));
        }
        for (name, when) in &p.schedules {
            let when = match when {
                crate::plugins::Schedule::Every(secs) if secs % 3600 == 0 => format!("every {} hours", secs / 3600),
                crate::plugins::Schedule::Every(secs) => format!("every {} minutes", secs / 60),
                crate::plugins::Schedule::DailyAt(m) => format!("daily at {:02}:{:02}", m / 60, m % 60),
            };
            row.push_str(&format!("<div class=what>“{}” runs by itself, {}.</div>", esc(name), esc(&when)));
        }
        // Steps that ask first, and the choice not to be asked every time.
        // Offered only where it's safe to stop asking: a step that is the same
        // every time, and not one that speaks to people as you.
        if !p.questions.is_empty() {
            row.push_str("<div class=what>Steps I ask about before doing:</div><ul>");
            for q in &p.questions {
                let control = match (&q.always_asks, q.trusted, p.status == Status::Active) {
                    (Some(why), _, _) => format!(" — always asks: {}", esc(why)),
                    (None, true, true) => format!(
                        " — you said don't ask {}",
                        button("untrust", &p.id, &format!("<input type=hidden name=key value='{}'>", esc(&q.command)), "ask again")
                    ),
                    (None, false, true) => button(
                        "trust",
                        &p.id,
                        &format!("<input type=hidden name=key value='{}'>", esc(&q.command)),
                        "don't ask me each time",
                    ),
                    _ => String::new(),
                };
                row.push_str(&format!("<li>“{}” {control}</li>", esc(&q.command)));
            }
            row.push_str("</ul>");
        }
        for t in &p.trouble {
            row.push_str(&format!("<div class=what>Note: {}</div>", esc(t)));
        }
        let fp = format!("<input type=hidden name=sha value='{}'>", esc(&p.sha256));
        match p.status {
            Status::Waiting | Status::Changed if p.manifest.is_some() => {
                row.push_str(&button("approve", &p.id, &fp, "Approve it to do the above"))
            }
            Status::Active => row.push_str(&button("off", &p.id, "", "Switch it off")),
            Status::Disabled => row.push_str(&button("on", &p.id, "", "Switch it back on")),
            _ => {}
        }
        row.push_str(&button("remove", &p.id, "", "Remove it"));
        if p.manifest.is_some() && !share_to.is_empty() {
            let opts: String = share_to.iter().map(|t| format!("<option>{}</option>", esc(t))).collect();
            row.push_str(&button("share", &p.id, &format!("<select name=key aria-label='Share it with'>{opts}</select>"), "Share it with"));
        }
        if p.status == Status::Active && !groups.is_empty() {
            let opts: String = groups.iter().map(|t| format!("<option>{}</option>", esc(t))).collect();
            row.push_str(&button("recommend", &p.id, &format!("<select name=key aria-label='Recommend it in'>{opts}</select>"), "Recommend it in"));
        }
        row.push_str("</div>");
        body.push_str(&row);
    }
    shell_at(Some(Page::AddOns), "Add-ons", &body)
}

/// Your own edits to the shipped config files, with a way back to the
/// default for each. Where the "undo my edit" lives, because editing the
/// shipped file back does nothing once the edit has been kept.
pub fn edits_page(kept: &[crate::yourchanges::KeptEdit], problems: &[String]) -> String {
    use crate::yourchanges::shown;
    let mut body = String::from(
        "<p class=note>When you edit one of Atlas's config files by hand, Atlas moves the edit \
         here the next time it starts, so an update can't undo it. To go back to what Atlas \
         ships, press the button — editing the file back won't do it, because the edit lives \
         here now.</p>",
    );
    for p in problems {
        body.push_str(&format!("<div class='rec'><p class=cause>{}</p></div>", esc(p)));
    }
    if kept.is_empty() {
        body.push_str(&nothing("No edits of yours. Every config file is as Atlas ships it."));
    }
    for e in kept {
        let yours = if e.change.removed { "you removed it".to_string() } else { shown(e.change.yours.as_ref()) };
        let mut notes = String::new();
        if e.change.unsure {
            notes.push_str(
                "<div class=what>Atlas couldn't tell whether this was your edit or an older \
                 version's default. If it wasn't you, go back to the default.</div>",
            );
        }
        if e.default_moved() {
            notes.push_str(&format!(
                "<div class=what>The default has changed since you set this: it was {}, it's now {}. Yours still applies.</div>",
                esc(&shown(e.change.was.as_ref())),
                esc(&shown(e.shipped_now.as_ref()))
            ));
        }
        body.push_str(&format!(
            "<div class=row><div class=name>{} — {}</div><div class=what>Yours: {}</div>\
             <div class=cost>Atlas ships: {}</div>{notes}\
             <form method=post action=/hub/edits><input type=hidden name=file value='{}'>\
             <input type=hidden name=path value='{}'><button class=revoke>Back to the default</button></form></div>",
            esc(&e.file),
            esc(&e.change.dotted()),
            esc(&yours),
            esc(&shown(e.shipped_now.as_ref())),
            esc(&e.file),
            esc(&e.change.dotted())
        ));
    }
    shell_at(Some(Page::Edits), "Your edits", &body)
}

/// What the Friends page shows.
#[derive(Debug, Clone, Default)]
pub struct FriendsView {
    /// Everyone paired with you, by your name for them.
    pub friends: Vec<String>,
    /// Friend requests waiting: (who, which group it came through).
    pub requests: Vec<(String, String)>,
    /// People you're in a group with and aren't friends with yet.
    pub could_ask: Vec<(String, String)>,
    /// Friends you added whose Atlas hasn't answered yet.
    pub reaching: Vec<String>,
    /// A link just made, to show once.
    pub link: Option<String>,
    /// What the last button did, when it's worth saying on the page.
    pub said: Option<String>,
    /// Where friends can reach you, in a sentence (`friends::Reach`).
    pub reach: String,
}

/// Friends: one link to add someone, requests, and who can reach you.
pub fn friends_page(v: &FriendsView) -> String {
    let form = |what: &str, inner: &str, label: &str| {
        format!(
            "<form method=post action=/hub/friends style='display:inline'>\
             <input type=hidden name=what value='{}'>{inner}<button>{}</button></form> ",
            esc(what),
            esc(label)
        )
    };
    let who = |n: &str| format!("<input type=hidden name=who value='{}'>", esc(n));
    let mut body = String::new();
    if let Some(said) = &v.said {
        body.push_str(&format!("<div class=row><div class=what>{}</div></div>", esc(said)));
    }
    body.push_str("<h2>Add a friend</h2>");
    match &v.link {
        Some(link) => {
            let qr = crate::phonelink::qr_svg(link).unwrap_or_default();
            body.push_str(&format!(
                "<p class=what>Send them this -- a text, an email, anything. When their Atlas opens it \
                 you're friends, both ways, with nothing to send back. It works once, for a week.</p>\
                 <textarea autocomplete=off readonly rows=4 style='width:100%' aria-label='Your friend link' onclick='this.select()'>{}</textarea>\
                 <p class=note>In person: they can scan this with their phone and paste what it reads.</p>\
                 <div style=\"width:220px;max-width:100%;margin:12px 0\">{qr}</div>",
                esc(link)
            ));
        }
        None => body.push_str(&format!(
            "<p class=what>Make a link and send it to them. That's all -- no codes back and forth, \
             no waiting for anyone to confirm.</p>{}",
            form("link", "", "Make a friend link")
        )),
    }
    body.push_str(&format!(
        "<h2>Got a link from someone?</h2><form method=post action=/hub/friends>\
         <input type=hidden name=what value=add><input autocomplete=off name=link style='width:70%' aria-label='Their link, or the whole message' \
         placeholder='Paste the whole message -- I&#39;ll find the link in it'> <button>Add them</button></form>"
    ));
    if !v.requests.is_empty() {
        body.push_str("<h2>Friend requests</h2>");
        for (from, group) in &v.requests {
            body.push_str(&format!(
                "<div class=row><div class=name>{}</div><div class=what>From \"{}\"</div>{}{}</div>",
                esc(from),
                esc(group),
                form("accept", &who(from), "Accept"),
                form("decline", &who(from), "Decline")
            ));
        }
    }
    if !v.could_ask.is_empty() {
        body.push_str("<h2>People in your groups</h2><p class=note>Not friends yet. A request goes to them \
                       alone, through the group's owner.</p>");
        for (name, group) in &v.could_ask {
            body.push_str(&format!(
                "<div class=row><div class=name>{}</div><div class=what>In \"{}\"</div>{}</div>",
                esc(name),
                esc(group),
                form("request", &who(name), "Send a friend request")
            ));
        }
    }
    body.push_str("<h2>Your friends</h2>");
    if !v.reach.is_empty() {
        body.push_str(&format!("<p class=note>{}</p>", esc(&v.reach)));
    }
    if v.friends.is_empty() {
        body.push_str(&nothing("No friends yet. Make a link above and send it to someone."));
    }
    for f in &v.friends {
        let waiting = if v.reaching.iter().any(|r| r.eq_ignore_ascii_case(f)) {
            "<div class=note>Their Atlas hasn't answered yet -- I keep trying for a week.</div>"
        } else {
            ""
        };
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div>{waiting}{}</div>",
            esc(f),
            form("forget", &who(f), "Unfriend")
        ));
    }
    body.push_str(
        "<p class=note>How their Atlas reaches yours: through Tor, which Atlas runs itself -- no \
         server, no shared network, nothing to set up, and nobody in between can read your messages \
         or see who you talk to. On the same wifi it goes straight across. If a friend's Atlas is \
         off, yours keeps the message and sends it when theirs is back.</p>",
    );
    shell_at(Some(Page::Friends), "Friends", &body)
}

/// Group chats with an owner: yours to change, others' to read.
pub fn groups_page(views: &[crate::groups::View], addable: &[String]) -> String {
    groups_page_with(views, addable, &[])
}

/// The same, with groups made before groups had owners (`ownerless`) and the
/// button that gives one an owner.
pub fn groups_page_with(views: &[crate::groups::View], addable: &[String], ownerless: &[String]) -> String {
    use crate::groups::Role;
    let mut body = String::from(
        "<p class=note>A group you make is yours: you decide who's in it and whether each \
         person can post or only read. Everyone's Atlas checks your signed list, so nobody \
         else can add people or speak where they're only reading. A release channel is a \
         group where only you post — that's where Atlas updates come from.</p>",
    );
    let form = |what: &str, id: &str, inner: &str, label: &str| {
        format!(
            "<form method=post action=/hub/groups style='display:inline'>\
             <input type=hidden name=what value='{}'><input type=hidden name=group value='{}'>{inner}\
             <button class=revoke>{}</button></form> ",
            esc(what),
            esc(id),
            esc(label)
        )
    };
    let hidden = |k: &str, v: &str| format!("<input type=hidden name={k} value='{}'>", esc(v));
    let options = |names: &[String]| {
        names.iter().map(|n| format!("<option>{}</option>", esc(n))).collect::<String>()
    };
    if views.is_empty() {
        body.push_str(&nothing("No groups with an owner yet. Make one below."));
    }
    for v in views {
        let kind = if v.release_channel { " — release channel" } else { "" };
        let mut row = format!(
            "<div class=row><div class=name>{}{kind}</div><div class=what>{}</div><ul>",
            esc(&v.name),
            if v.mine { "You made this, so you decide who's in it.".to_string() } else { format!("Made by {}.", esc(&v.owner)) }
        );
        for (who, role, _key) in &v.seats {
            let mut controls = String::new();
            if v.mine && *role != Role::Owner {
                controls.push_str(&form("remove", &v.id, &hidden("who", who), "take out"));
                if !v.release_channel {
                    let (to, label) = if *role == Role::Reader { ("member", "let them post") } else { ("reader", "reading only") };
                    controls.push_str(&form("role", &v.id, &format!("{}{}", hidden("who", who), hidden("role", to)), label));
                }
            }
            row.push_str(&format!("<li>{} — {} {controls}</li>", esc(who), esc(role.plain())));
        }
        row.push_str("</ul>");
        if v.mine {
            let others: Vec<String> =
                addable.iter().filter(|a| !v.seats.iter().any(|(w, _, _)| w.eq_ignore_ascii_case(a))).cloned().collect();
            if !others.is_empty() {
                row.push_str(&form(
                    "add",
                    &v.id,
                    &format!("<select name=who aria-label='Who to add'>{}</select>", options(&others)),
                    "add to this group",
                ));
            }
        }
        row.push_str("</div>");
        body.push_str(&row);
    }
    if !ownerless.is_empty() {
        body.push_str(
            "<h2>Groups without an owner</h2><p class=note>Made before groups had owners: anyone \
             in them can bring people in, and nobody can take anyone out. Give one an owner — \
             you — and it starts again with the same people, under your list.</p>",
        );
        for g in ownerless {
            body.push_str(&format!(
                "<div class=row><div class=name>{}</div>{}</div>",
                esc(g),
                form("adopt", g, "", "Give it an owner (you)")
            ));
        }
    }
    body.push_str("<h2>Make a group</h2>");
    if addable.is_empty() {
        body.push_str(
            "<p class=note>Pair with someone first. Once their Atlas has introduced itself \
             (it does that by itself), they can be added.</p>",
        );
    } else {
        body.push_str(&format!(
            "<form method=post action=/hub/groups><input type=hidden name=what value=new>\
             <input autocomplete=off name=group aria-label='Group name' placeholder='Name'> <input autocomplete=off name=who aria-label='Who is in it' placeholder='{}'>\
             <label><input type=checkbox name=role value=release> release channel (only you post)</label>\
             <button>Make it</button></form>",
            esc(&format!("People, e.g. {}", addable.iter().take(2).cloned().collect::<Vec<_>>().join(", ")))
        ));
    }
    shell_at(Some(Page::Groups), "Groups", &body)
}

/// After a button: back to the page if it worked; if not, say why, with the
/// way back. A refusal shown as the same page reloading would look exactly
/// like success.
///
/// What it did is said on the page it goes back to (27 Sep 2026: every button
/// came back to a page that looked the same whether or not anything happened).
pub fn after_button(page: Page, done: std::result::Result<String, String>) -> crate::server::Reply {
    match done {
        Ok(said) if !said.trim().is_empty() => back_with(page.href(), "", &said),
        Ok(_) => crate::server::Reply::redirect(page.href()),
        Err(why) => crate::server::Reply::html(shell(
            page.label(),
            &format!(
                "<p class=note>{}</p><p><a href='{}'>Back to {}</a></p>",
                esc(&why),
                page.href(),
                esc(page.label())
            ),
        )),
    }
}

/// Where to go after a form: the page, with what happened said once
/// (`?said=`). `extra` is more of the query (`room=…`, `job=…`), or empty.
/// Nothing private goes through here -- an address lands in the browser's
/// history. Private text is a `hubjobs::Flash`.
pub fn back_with(href: &str, extra: &str, said: &str) -> crate::server::Reply {
    // The anchor can come from a form (a setting's key), and this becomes a
    // `Location` header: only characters an id uses, so nothing can end the
    // header or the address.
    let (path, anchor) = match href.split_once('#') {
        Some((p, a)) => (p, format!("#{}", a.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect::<String>())),
        None => (href, String::new()),
    };
    // Kept short: the hub reads at most 8 KB of request head, and a long
    // error sentence would make the address it's sent back to unreadable.
    let said: String = if said.chars().count() > 600 {
        said.chars().take(600).collect::<String>() + "…"
    } else {
        said.to_string()
    };
    let sep = if extra.is_empty() { "" } else { "&" };
    crate::server::Reply::redirect(&format!(
        "{path}?{extra}{sep}said={}{anchor}",
        crate::research::urlencode(&said)
    ))
}

/// A page with a sentence saying what just happened, under its heading.
pub fn with_said(page: String, said: Option<&str>) -> String {
    let Some(said) = said.filter(|s| !s.trim().is_empty()) else { return page };
    let notice = format!("<p class=notice role=status>{}</p>", esc(said));
    match page.find("</h1>").or_else(|| page.rfind("</main>")) {
        Some(at) => {
            let at = if page[at..].starts_with("</h1>") { at + 5 } else { at };
            format!("{}{notice}{}", &page[..at], &page[at..])
        }
        None => page,
    }
}

/// A page that looks again by itself in `secs` seconds: a hub job still
/// running (`hubjobs`). No script -- the hub works without one.
pub fn with_refresh(page: String, secs: u32) -> String {
    match page.find("</head>") {
        Some(at) => format!("{}<meta http-equiv=refresh content={secs}>{}", &page[..at], &page[at..]),
        None => page,
    }
}

pub fn permissions_page(s: &Settings, granted_apps: &[String]) -> String {
    let items = s.consequential();
    let mut body = String::from(
        "<p class=note>Everything that can use a sensor, reach outside this machine, \
         or act without asking. Worth a look now and then.</p>",
    );
    // The apps you've allowed Atlas to act on, from the grants gate. Only the
    // "always" grants survive a restart and reach here; a session or one-off
    // grant is gone by the next start. `granted_apps` had no reader before —
    // the gate could record a standing permission and nothing showed it.
    if !granted_apps.is_empty() {
        body.push_str("<p class=note>Apps you've allowed Atlas to act on: ");
        body.push_str(&esc(&granted_apps.join(", ")));
        body.push_str(".</p>");
    }
    if items.is_empty() {
        body.push_str("<p>Nothing is enabled.</p>");
    }
    // Every row can be changed here, on or off (29 Sep 2026). A row that was
    // off showed the word "off" and no button, so this page listed what
    // Atlas wasn't allowed to do and gave no way to allow it: Eric looked for
    // where to change a permission and there was nowhere. Turning one on
    // still asks first (`control`, `Weight::needs_confirming`).
    for item in items {
        body.push_str(&format!(
            "<div class=row><div class=name>{}{}</div><div class=what>{}</div>\
             <div class=cost>{}</div><div style=\"margin-top:8px\">{}</div></div>",
            esc(&item.name),
            tag(item.weight),
            esc(&item.what),
            esc(&item.cost),
            control(item)
        ));
    }
    shell_at(Some(Page::Permissions), "Permissions", &body)
}


/// The dashboard: your cards, in your order.
///
/// `bodies` supplies the contents for each card, so this function knows about
/// arrangement and nothing about what is being arranged. A card with no body
/// says it has nothing rather than rendering an empty frame — an empty frame
/// reads as a broken page.
///
/// Two modes, because a dashboard you rearrange by accident while reading it
/// is worse than one you cannot rearrange at all. Reading is the default;
/// arranging is a thing you turn on.
pub fn dashboard_page(
    layout: &crate::dash::Layout,
    bodies: &[(crate::dash::Card, String)],
    arranging: bool,
) -> String {
    dashboard_page_with(layout, bodies, arranging, 0)
}

/// The dashboard: your cards, in your order.
///
/// `bodies` supplies the contents, so this knows about arrangement and nothing
/// about what is being arranged.
///
/// Two modes. Reading is the default, because a dashboard you rearrange by
/// accident while reading it is worse than one you cannot rearrange at all.
fn dashboard_page_with(
    layout: &crate::dash::Layout,
    bodies: &[(crate::dash::Card, String)],
    arranging: bool,
    waiting: usize,
) -> String {
    dashboard_deck(layout, bodies, arranging, waiting, &Deck::not_running())
}

/// Where a moment sits on today's spine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Already happened.
    Done,
    /// Now — what Atlas is working on.
    Now,
    /// Still to come.
    Later,
}

/// What Home says: the greeting, whether Atlas is on, the Brief with what's
/// waiting on you inside it, today in order, and your businesses at a glance.
///
/// Built by `hublive` from live state. Everything here is a sentence a person
/// would say; a section with nothing to report says so rather than showing
/// an empty frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Deck {
    /// "Good evening, Eric." — or just "Hello." in the small hours.
    pub greeting: String,
    /// "On, listening" / "On, typing" / "Paused".
    pub status: String,
    /// `""` live, `"held"` paused, `"off"` not running.
    pub tone: &'static str,
    /// What Atlas is doing right now, as a headline.
    pub now: String,
    /// The line under that.
    pub now_sub: String,
    /// Today, in order: (wall-clock time, what, where it sits).
    pub spine: Vec<(String, String, Mark)>,
    /// The Brief: Atlas's own summary of where things stand, in its words.
    pub brief: String,
    /// What's waiting on you, riding inside the Brief (the design: "Waiting
    /// on you rides inside the Brief, not as a separate home panel"). Each is
    /// (what, where to act on it).
    pub asks: Vec<(String, String)>,
    /// Your businesses, for Business at a glance. Empty: the section doesn't
    /// show, as the design has it — it appears once a business exists.
    pub businesses: Vec<Glance>,
    /// Nothing known yet: Home is the design's first-run welcome, calm but
    /// not blank, instead of a page of empty sections.
    pub first_run: bool,
}

/// One business, at a glance on Home.
#[derive(Debug, Clone, PartialEq)]
pub struct Glance {
    pub name: String,
    /// Open work filed under this business.
    pub open: usize,
    /// People in it: (name, what they are).
    pub people: Vec<(String, String)>,
}

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
fn pill(kind: &str, word: &str) -> String {
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
fn today_html(d: &Deck) -> String {
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
fn right_now_html(d: &Deck) -> String {
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
fn glance_html(g: &Glance) -> String {
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
fn brief_html(d: &Deck) -> String {
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
fn first_run_html(d: &Deck) -> String {
    format!(
        "<section class=brief aria-label='Welcome'>{MARK}<div><p><b>{greet}</b> This is home. It fills in as you go — \
         your brief, your day, what I'm working on. Nothing here yet, so let's give it a little to work with. \
         You can skip any of these and come back; I run fine offline the whole time.</p></div></section>\
         <ol class=firststeps>\
         <li><b>Bring in your phone's calendar</b><span>The Atlas app on your phone sends its calendar here and keeps the two in step, kept on this machine.</span>\
         <a class=btn href='{cal}'>Add your phone</a></li>\
         <li><b>Email</b><span>Turn Email on in Settings so I can pull out what needs a reply. Adding the mailbox itself has no page yet.</span>\
         <a class=btn href='{acc}'>Open the switch</a></li>\
         <li><b>Hand me something to look at</b><span>A file, a link, a photo — any type, any size.</span>\
         <a class=btn href='{give}'>Give Atlas a file</a></li>\
         </ol><p class=note>Or just tell me what you need — say “Atlas”, or type in the bar. I'll take it from there.</p>",
        greet = esc(&d.greeting),
        // Each goes where the thing is actually done (27 Sep 2026: "Connect"
        // opened the calendar, "Add" opened site security, and "Give Atlas a
        // file" opened search).
        cal = Page::Phone.href(),
        acc = format!("{}#set-mail.enabled", Page::Settings.href()),
        give = Page::Give.href(),
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

/// Dragging, with a mouse or a finger.
///
/// Pointer events rather than HTML5 drag-and-drop, because the HTML5 API does
/// not fire on touch at all — a dashboard that can only be rearranged with a
/// mouse is one you cannot rearrange on the device you are most often holding.
///
/// It posts the same fields the buttons post, to the same address, so the two
/// cannot drift apart. If the script never runs, the buttons still do.
const DRAG_SCRIPT: &str = concat!("<script>", include_str!("../assets/hub/drag.js"), "</script>");


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

fn note_line(said: &str) -> String {
    format!("<p class=note>{}</p>", esc(said))
}

/// How much an account matters, said rather than named.
fn stakes_word(s: crate::accounts::Stakes) -> &'static str {
    match s {
        crate::accounts::Stakes::Keystone => "Losing this loses the others",
        crate::accounts::Stakes::High => "Matters a lot",
        crate::accounts::Stakes::Medium => "Matters",
        crate::accounts::Stakes::Low => "Minor",
    }
}


// ---------------------------------------------------------------------------
// The palette.
// ---------------------------------------------------------------------------

/// One result, as a row you can act on.
///
/// A destination is a link and an action is a form, so picking either does the
/// real thing rather than taking you to the page that holds the button.
fn palette_row(e: &crate::palette::Entry, first: bool) -> String {
    let inner = format!(
        "<span class=name>{}</span><span class=what>{}</span>",
        esc(e.label),
        esc(e.hint)
    );
    let mark = if first { " first" } else { "" };
    match &e.does {
        crate::palette::Does::Go(href) => format!(
            "<a class='hit{mark}' href='{}'>{inner}</a>",
            esc(href)
        ),
        crate::palette::Does::Run(action, fields) => {
            let hidden: String = fields
                .iter()
                .map(|(k, v)| {
                    format!("<input type=hidden name='{}' value='{}'>", esc(k), esc(v))
                })
                .collect();
            format!(
                "<form class=hitform method=post action='{}'>{hidden}\
                 <button class='hit{mark}'>{inner}</button></form>",
                esc(action)
            )
        }
    }
}

/// The palette, on every page.
///
/// Rendered into the page rather than fetched, because Atlas works offline and
/// a palette that needs a round trip per keystroke is a palette that stutters
/// on the day the machine is busy — which is exactly the day you are looking
/// for something.
///
/// The whole thing is inside a plain `<form>` pointing at a real page, so with
/// no script at all typing and pressing enter still lands somewhere useful.
pub fn palette_overlay(entries: &[crate::palette::Entry], recent: &crate::palette::Recent) -> String {
    let first = recent.first(entries);
    let shown: Vec<&crate::palette::Entry> = if first.is_empty() {
        entries.iter().take(crate::palette::SHOW).collect()
    } else {
        first
            .into_iter()
            .chain(entries.iter().filter(|e| !recent.picked.iter().any(|p| p == e.id())))
            .take(crate::palette::SHOW)
            .collect()
    };

    let mut rows = String::new();
    for (i, e) in shown.iter().enumerate() {
        rows.push_str(&palette_row(e, i == 0));
    }
    // Every entry, hidden, so typing filters instantly instead of only
    // filtering the seven that happened to be offered first.
    let mut all = String::new();
    for e in entries {
        all.push_str(&format!(
            "<div class=allhit data-words='{}'>{}</div>",
            esc(&format!(
                "{} {} {}",
                e.label.to_lowercase(),
                e.hint.to_lowercase(),
                e.also.join(" ")
            )),
            palette_row(e, false)
        ));
    }

    format!(
        "<div class=palette id=palette hidden>\
         <form class=palbox method=get action='/hub/find'>\
         <input class=palq id=palq name=q aria-label='Find anything' autocomplete=off spellcheck=false \
         placeholder='What do you want to do?'>\
         <div class=hits id=palhits>{rows}</div>\
         <div class=allhits id=palall hidden>{all}</div>\
         <p class=palfoot>Type to filter. Enter opens the first one. Escape closes.</p>\
         </form></div>{PALETTE_SCRIPT}"
    )
}

/// The plain version, for when there is no script — and the page the palette's
/// form posts to if you press enter before it has filtered anything.
pub fn find_page(
    query: &str,
    results: &[&crate::palette::Entry],
    did_you_mean: Option<&crate::palette::Entry>,
) -> String {
    let mut body = format!(
        "<form method=get action='/hub/find'>\
         <input name=q value='{}' aria-label='What do you want to do?' placeholder='What do you want to do?' autocomplete=off>\
         <button class=primary>Find</button></form>",
        esc(query)
    );
    if results.is_empty() {
        match did_you_mean {
            // Asked, not opened: a typo guess is a question.
            Some(e) => {
                body.push_str(&format!(
                    "<p class=note>Nothing matches \"{}\" exactly. Did you mean:</p><div class='hits plain'>",
                    esc(query.trim())
                ));
                body.push_str(&palette_row(e, true));
                body.push_str("</div>");
            }
            None => body.push_str(&nothing(&format!(
                "Nothing here matches \"{}\". Try what you'd call it out loud.",
                query.trim()
            ))),
        }
    } else {
        body.push_str("<div class='hits plain'>");
        for (i, e) in results.iter().take(crate::palette::SHOW).enumerate() {
            body.push_str(&palette_row(e, i == 0));
        }
        body.push_str("</div>");
    }
    shell("Find", &body)
}

/// Opening and filtering.
///
/// Filtering happens against words already in the page, so it costs nothing
/// and works with the network down. The ranking that decides *order* stays on
/// the Rust side — a second ranking here would be a second set of rules that
/// quietly disagrees with the one the plain page uses.
const PALETTE_SCRIPT: &str = concat!("<script>", include_str!("../assets/hub/palette.js"), "</script>");


/// Every gesture, drawn from its own definition.
///
/// Eric asked for a way to be reminded, and specifically **not** a recording
/// of him doing them — he is handing instances of this to friends, and a video
/// of the author waving at a webcam is both odd to ship and wrong the moment
/// anybody changes a gesture.
///
/// So every card here is generated from the same tests the recogniser
/// evaluates. It cannot drift, and teaching a new gesture adds its card with
/// no second step anyone has to remember.
pub fn gestures_page(
    known: &[crate::handshape::Gesture],
    undecided: &[(&'static str, &'static str)],
    enabled: bool,
    may_approve: bool,
) -> String {
    let mut body = String::new();

    // Say plainly, at the top, whether hand control is actually on. "How do I
    // make this shape" and "is this even watching right now" are different
    // questions, and someone opening this page could be asking either — so the
    // page answers both, rather than only teaching gestures that may be off.
    let (dot, word, msg) = if enabled {
        (
            "var(--good)",
            "on",
            "Your camera reads gestures on this machine — nothing leaves it.",
        )
    } else {
        (
            "var(--ink-faint)",
            "off",
            "Turn it on to answer and drive Atlas with your hands. The gestures \
             below still work as a reference.",
        )
    };
    body.push_str(&format!(
        "<div class=state style=\"display:flex;align-items:center;gap:10px;\
         background:var(--raise);border:1px solid var(--edge);border-radius:10px;\
         padding:12px 14px;margin:0 0 14px\">\
         <span style=\"width:9px;height:9px;border-radius:99px;background:{dot};\
         flex:none\"></span><b>Hand control is {word}</b>\
         <span style=\"color:var(--ink-dim);font-size:13.5px\">{msg}</span>\
         <a style=\"margin-left:auto\" href=\"/hub/settings#what-it-can-see\">Change</a></div>"
    ));

    // The safety rule, read from the setting rather than asserted here: a
    // gesture can say no, but by default it cannot approve something
    // irreversible. If someone has deliberately loosened that, say so instead.
    let safety = if may_approve {
        "You've allowed gestures to approve anything, including irreversible \
         actions like sending or deleting."
    } else {
        "A gesture can answer or dismiss, but it never approves something \
         irreversible — sending, deleting or anything leaving your machine \
         still needs your voice or a click."
    };
    body.push_str(&format!(
        "<p class=note style=\"margin:0 0 18px\">{}</p>",
        esc(safety)
    ));

    if known.is_empty() {
        body.push_str(&nothing(
            "No gestures yet. Say \"watch my hands\", hold a shape up and tell \
             me what it should do, and it'll appear here.",
        ));
    }

    body.push_str("<div class=gestures>");
    for g in known {
        body.push_str(&format!(
            "<div class=gesture><div class=draw>{}</div>\
             <div class=meaning><b>{}</b><span class=what>{}</span></div></div>",
            crate::handshape::sketch(g),
            esc(&g.does),
            esc(&crate::handshape::how_to(g))
        ));
    }
    body.push_str("</div>");

    // Said here rather than left in a comment somewhere: a gesture Atlas
    // cannot tell from another is one you would spend a week working around
    // before finding out why.
    if !undecided.is_empty() {
        body.push_str("<h2>Two I can't tell apart</h2>");
        for (what, why) in undecided {
            body.push_str(&format!(
                "<div class=rec><h3>{}</h3><p class=cause>{}</p></div>",
                esc(what),
                esc(why)
            ));
        }
    }

    body.push_str(&more("/hub/settings#what-it-can-see", "Turn hand control on or off"));
    shell_at(Some(Page::Gestures), "Hand gestures", &body)
}

pub fn status_page(lines: &[(String, String)], changed: usize) -> String {
    let mut body = String::new();
    for (k, v) in lines {
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div><div class=what>{}</div></div>",
            esc(k),
            esc(v)
        ));
    }
    body.push_str(&format!(
        "<p class=note style=\"margin-top:20px\">{changed} setting{} changed from the defaults. \
         <a href=/hub/settings>Review</a></p>",
        if changed == 1 { "" } else { "s" }
    ));
    shell_at(Some(Page::Status), "Status", &body)
}

/// One thing Atlas stopped on, told the design's way: what it tried, what
/// stopped it, and what it needs from you.
#[derive(Debug, Clone, PartialEq)]
pub struct Stopped {
    pub what: String,
    pub tried: String,
    pub stopped: String,
    pub needs: String,
    /// A business's name when it's that business's work; `None` for yours.
    pub area: Option<String>,
}

/// Outstanding, in the design's four lanes: waiting on you, blocked, in
/// progress, and carried over.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Open {
    /// (what, a line on why it's yours, where to act on it)
    pub waiting: Vec<(String, String, String)>,
    pub blocked: Vec<Stopped>,
    /// (what, who's on it or how far along)
    pub in_progress: Vec<(String, String)>,
    /// (what, days carried)
    pub carried: Vec<(String, u64)>,
    /// What each item's remove button sends back, lane by lane, in the same
    /// order as the items. `None`, or no entry at all, draws no button: a
    /// running step Atlas can't safely stop has none rather than one that
    /// does nothing. (2 Oct 2026: "Can't remove things from the outstanding
    /// list" -- the page said "tell me to drop it" and had no way to.)
    pub drops: Drops,
}

/// The keys behind the remove buttons on Outstanding. A key is
/// `<kind>:<id>` -- `b` a backlog item, `w` a workspace item, `t` a queued
/// task, `e` a worker's errand, `c` a project change waiting for your yes
/// -- and is only ever read back by `Daemon::drop_outstanding`, which looks
/// the id up again rather than trusting anything else in the form.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drops {
    pub waiting: Vec<Option<String>>,
    pub blocked: Vec<Option<String>>,
    pub in_progress: Vec<Option<String>>,
    pub carried: Vec<Option<String>>,
}

/// The remove button for one item, or nothing. Work already running is
/// asked to stop ("Stop it"); everything else is taken off ("Drop it").
fn drop_button(key: Option<&Option<String>>, what: &str) -> String {
    let Some(Some(key)) = key else { return String::new() };
    let label = if key.starts_with("e:") { "Stop it" } else { "Drop it" };
    format!(
        "<form class=inline method=post action=/hub/outstanding><input type=hidden name=what value=drop>\
         <input type=hidden name=key value='{}'><button class=quiet aria-label='{label}: {}'>{label}</button></form>",
        esc(key),
        esc(what)
    )
}

impl Open {
    pub fn count(&self) -> usize {
        self.waiting.len() + self.blocked.len() + self.in_progress.len() + self.carried.len()
    }
}

fn lane_head(icon_paths: &str, colour: &str, name: &str, n: usize) -> String {
    format!(
        "<h2><svg viewBox='0 0 24 24' fill=none stroke='var({colour})' stroke-width=2.2 aria-hidden=true>{icon_paths}</svg>\
         {name} <span class=n>{n}</span></h2>"
    )
}

/// Outstanding — the open backlog, personal and business together, most
/// pressing first. Nothing rots quietly: the rule is on the page.
pub fn outstanding_page(o: &Open) -> String {
    let mut left = String::new();
    left.push_str("<section class=lane>");
    left.push_str(&lane_head("<path d='M12 8v4l3 2'/><circle cx=12 cy=12 r=9 />", "--accent", "Waiting on you", o.waiting.len()));
    if o.waiting.is_empty() {
        left.push_str("<p class=nothing>Nothing waiting on you.</p>");
    }
    for (n, (what, why, href)) in o.waiting.iter().enumerate() {
        left.push_str(&format!(
            "<div class='item wait'><div class=t>{}</div>{}<div class=acts><a class='btn primary' href='{}'>Open it</a>{}</div></div>",
            esc(what),
            if why.is_empty() { String::new() } else { format!("<div class=d>{}</div>", esc(why)) },
            esc(href),
            drop_button(o.drops.waiting.get(n), what)
        ));
    }
    left.push_str("</section><section class=lane>");
    left.push_str(&lane_head("<path d='M12 8v4M12 16h.01'/><circle cx=12 cy=12 r=9 />", "--hot", "Blocked", o.blocked.len()));
    if o.blocked.is_empty() {
        left.push_str("<p class=nothing>Nothing is stuck.</p>");
    }
    for (n, b) in o.blocked.iter().enumerate() {
        let drop = drop_button(o.drops.blocked.get(n), &b.what);
        left.push_str(&format!(
            "<div class='item stop'>{area}<div class=t>{what}</div><div class=tsn>\
             <span class=k>Tried</span><span class=v>{tried}</span>\
             <span class='k stopped'>Stopped</span><span class=v>{stopped}</span>\
             <span class='k needs'>Needs</span><span class=v>{needs}</span></div>{acts}</div>",
            acts = if drop.is_empty() { String::new() } else { format!("<div class=acts>{drop}</div>") },
            area = b.area.as_deref().map(|a| format!("<span class=area>{}</span>", esc(a))).unwrap_or_default(),
            what = esc(&b.what),
            tried = esc(&b.tried),
            stopped = esc(&b.stopped),
            needs = esc(&b.needs),
        ));
    }
    left.push_str("</section>");

    let mut right = String::new();
    right.push_str("<section class=lane>");
    right.push_str(&lane_head("<circle cx=12 cy=12 r=9 /><path d='M12 7v5l3 2'/>", "--cool", "In progress", o.in_progress.len()));
    if o.in_progress.is_empty() {
        right.push_str("<p class=nothing>Nothing running right now.</p>");
    }
    for (n, (what, how)) in o.in_progress.iter().enumerate() {
        let drop = drop_button(o.drops.in_progress.get(n), what);
        right.push_str(&format!(
            "<div class=item><div class=t>{}</div><div class=d>{}</div>{}</div>",
            esc(what),
            esc(how),
            if drop.is_empty() { String::new() } else { format!("<div class=acts>{drop}</div>") }
        ));
    }
    right.push_str("</section><section class=lane>");
    right.push_str(&lane_head("<path d='M3 12a9 9 0 1 0 9-9'/><path d='M3 4v5h5'/>", "--ink-faint", "Carried over", o.carried.len()));
    if o.carried.is_empty() {
        right.push_str("<p class=nothing>Nothing carried over.</p>");
    } else {
        right.push_str("<div class=carried>");
        for (n, (what, days)) in o.carried.iter().enumerate() {
            right.push_str(&format!(
                "<div class=row2><span class=w>{}</span><span class=chipd>{} day{}</span>{}</div>",
                esc(what),
                days,
                if *days == 1 { "" } else { "s" },
                drop_button(o.drops.carried.get(n), what)
            ));
        }
        right.push_str("</div>");
    }
    right.push_str(
        "</section><div class=rule><b>Nothing here rots quietly.</b> Anything carried more than a week, \
         I raise in your brief. If you want something gone, press Drop it, or tell me to take it off your outstanding list, \
         and it's gone — I won't keep nagging.</div>",
    );
    let body = format!("<div class=lanes><div>{left}</div><div>{right}</div></div>");
    shell_at(Some(Page::Outstanding), "Outstanding", &body)
}

/// A step in Atlas's thought process, named the way the design names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    Plan,
    Doing,
    Delegated,
    Rerouted,
    Checked,
    Waiting,
    Stuck,
    Now,
    Next,
}

impl Step {
    fn label(self) -> (&'static str, &'static str) {
        match self {
            Step::Plan => ("plan", "◇ Plan"),
            Step::Doing => ("doing", "▷ Doing"),
            Step::Delegated => ("doing", "⌞ Delegated"),
            Step::Rerouted => ("rerouted", "↝ Rerouted"),
            Step::Checked => ("checked", "✓ Checked"),
            Step::Waiting => ("doing", "◷ Waiting"),
            Step::Stuck => ("stuck", "! Stuck"),
            Step::Now => ("now", "▷ Now"),
            Step::Next => ("next", "◷ Next"),
        }
    }
}

/// Now — Atlas's thought process, live, as a legible stream rather than a
/// raw log. Plan, doing, a step handed to a worker, a check, a reroute
/// instead of a cut corner, what it's on now and what's next. The rail holds
/// the time it has spent and what it will do if this doesn't hold up.
#[derive(Debug, Clone, PartialEq, Hash)]
pub struct NowView {
    /// What it's working on, or "Waiting for you." when nothing is.
    pub title: String,
    /// "Started 18:31 · checking it worked".
    pub since: String,
    /// The stream, oldest first.
    pub steps: Vec<(Step, String)>,
    /// How many of the oldest steps are left out in Plain view.
    pub plain_from: usize,
    /// "4 min", when something is running.
    pub spent: Option<String>,
    /// What it will do if this doesn't hold up.
    pub fallback: String,
    pub paused: bool,
    /// Something is underway: the page refreshes itself and the mark moves.
    pub working: bool,
    /// Also running, in the background.
    pub background: Vec<String>,
    /// Notes kept for you because they couldn't reach you when they came
    /// (the notification outbox): their titles, oldest first. 30 Sep 2026:
    /// they were only said when you came back, and nothing on the hub showed
    /// they were waiting.
    pub held: Vec<String>,
}

/// A number that changes when anything the Now page shows changes: what a
/// page drawn from `v` polls `/hub/changed.json` against.
pub fn live_version(v: &NowView) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

pub fn now_page(v: &NowView) -> String {
    // Live without a timed reload (WCAG 2.2.1: a page that reloads itself is
    // a time limit you can't turn off). A small script fetches the page and
    // swaps the stream in place, politely announced; Pause stops it; with
    // scripts off, Refresh is a link.
    let mut body = String::new();
    let mut steps = String::new();
    for (i, (kind, text)) in v.steps.iter().enumerate() {
        let (class, label) = kind.label();
        let early = if i < v.plain_from { " early" } else { "" };
        steps.push_str(&format!(
            "<div class='step {class}{early}'><span class=dot></span><div><div class=lab>{label}</div><div class=txt>{}</div></div></div>",
            esc(text)
        ));
    }
    if v.steps.is_empty() {
        steps.push_str("<p class=nothing>Nothing underway. Ask me something and you'll see how I work it through here.</p>");
    }
    let spent = match &v.spent {
        Some(s) => format!(
            "<div class=box><div class=lab>Time on this</div><div class=big>{} <small>so far</small></div>\
             <p>If it runs long, I'll stop and tell you where I got to.</p></div>",
            esc(s)
        ),
        None => String::new(),
    };
    // Pause turns the microphone off too (28 Sep 2026), and says so.
    let (pause_what, pause_label) = if v.paused { ("resume", "Carry on and listen again") } else { ("pause", "Pause Atlas and stop listening") };
    let background = if v.background.is_empty() {
        String::new()
    } else {
        format!("<div class=box><div class=lab>Also running</div><p>{}</p></div>", esc(&v.background.join(" · ")))
    };
    let background = if v.held.is_empty() {
        background
    } else {
        let n = v.held.len();
        let items: String = v.held.iter().map(|t| format!("<li>{}</li>", esc(t))).collect();
        format!(
            "{background}<div class=box><div class=lab>Kept for you</div><p>{n} note{} that couldn't reach you when {} came. \
             I'll tell you {} when you're next back at the computer.</p><ul>{items}</ul></div>",
            if n == 1 { "" } else { "s" },
            if n == 1 { "it" } else { "they" },
            if n == 1 { "it" } else { "them" },
        )
    };
    body.push_str(&format!(
        "<div class=nowgrid><div class=stream id=detailed data-live=now data-v='{version}'>\
         <div class=streamhead><div style='display:flex;gap:11px'>{mark}<div><div class=title>{title}</div>\
         <div class=since>{since}</div></div></div>\
         <nav class=seg aria-label='How much to show'><a class=plain href='#'>Plain</a><a class=detail href='#detailed'>Detailed</a></nav></div>\
         {steps}</div>\
         <p class=liveline><span id=liveword role=status>Updating live.</span> <button type=button id=livepause aria-pressed=false>Pause live updates</button> <a href='/hub/now'>Refresh</a></p>\
         <aside class=rail2>{spent}\
         <div class=box><div class=lab>If this doesn't hold up</div><p>{fallback}</p></div>{background}\
         <div class='box soft'>{on}<p>Running on your machine. Nothing left it.</p>\
         <form method=post action='/hub/pause'><input type=hidden name=what value={pause_what}>\
         <button>{pause_label}</button></form></div></aside></div>",
        mark = if v.working { THINKING } else { IDLE },
        version = live_version(v),
        title = esc(&v.title),
        since = esc(&v.since),
        fallback = esc(&v.fallback),
        on = pill("online", "On"),
    ));
    body.push_str(LIVE_SCRIPT);
    shell_at(Some(Page::Now), "Now", &body)
}

/// Keeps a page live without reloading it: every few seconds, ask
/// `/hub/changed.json` whether what the page shows has changed -- a few
/// bytes -- and only then fetch the page and swap in the part marked
/// `data-live`, unless paused. It used to fetch the whole page every three
/// seconds whether or not anything had changed (28 Sep 2026). Stops by
/// itself after ten minutes with no change (WCAG 2.2.2), and says so.
pub const LIVE_SCRIPT: &str = concat!("<script>", include_str!("../assets/hub/live.js"), "</script>");


/// A plain list, knowing where it sits in the tree.
pub fn list_page_at(here: Option<Page>, title: &str, intro: &str, items: &[String]) -> String {
    let mut body = format!("<p class=note>{}</p>", esc(intro));
    if items.is_empty() {
        // Said the way a person would say it, and specific to the page --
        // "Nothing here." is the same sentence whether the list is genuinely
        // empty or never loaded.
        body.push_str(&nothing("Nothing on this list right now."));
    }
    for i in items {
        body.push_str(&format!("<div class=row>{}</div>", esc(i)));
    }
    match here {
        Some(p) => shell_at(Some(p), title, &body),
        None => shell(title, &body),
    }
}

/// Parse a posted form body.
/// Every field of a posted form, decoded, in order.
pub fn form_fields(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (urldecode(k), urldecode(v)),
            None => (urldecode(kv), String::new()),
        })
        .collect()
}

pub fn form_field(body: &str, name: &str) -> Option<String> {
    for pair in body.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == name {
            return Some(urldecode(v));
        }
    }
    None
}

pub fn urldecode(s: &str) -> String {
    // Bytes, then UTF-8: a %-escaped "é" is two bytes, and turning each byte
    // into a char on its own (as this did until 26 Sep 2026) wrote "Ã©" into
    // every message, name or search that wasn't plain English.
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}


/// "Put Atlas on your phone": the QR code and the link, or why there isn't one.
///
/// `link` is `phonelink::phone_url(..)` once `phonelink::publish` succeeded;
/// `why_not` is `phonelink::say(..)` for any other outcome. With neither, the
/// block offers the button that runs `publish`. Self-contained so the
/// devices page can place it without changing its own signature.
///
/// The link carries the hub token. That is deliberate and bounded: it is
/// shown only on a page that needed the token to open, and it resolves only
/// inside the tailnet.
pub fn phone_block(link: Option<&str>, why_not: Option<&str>) -> String {
    let mut out = String::from("<h2>Put Atlas on your phone</h2>");
    match link {
        Some(url) => {
            let qr = crate::phonelink::qr_svg(url).unwrap_or_default();
            out.push_str(&format!(
                "<p class=what>Point the phone's camera at this, with Tailscale switched on \
                 on the phone. Nothing to type.</p>\
                 <div style=\"width:220px;max-width:100%;margin:12px 0\">{qr}</div>\
                 <p class=note>Or open this on the phone: <a href=\"{u}\">{u}</a></p>",
                u = esc(url)
            ));
        }
        None => {
            if let Some(why) = why_not {
                out.push_str(&format!("<p class=note>{}</p>", esc(why)));
            } else {
                out.push_str(
                    "<p class=what>Open Atlas on your laptop — its window makes the code \
                     for your phone once Tailscale is on, and it shows up here too.</p>",
                );
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The pages that replaced terminal commands (27 Sep 2026): the vault on the
// Accounts page, a Sync page that starts a household, and "Free up space" on
// Status. The handlers are in `hubvault.rs`; these only draw.
// ---------------------------------------------------------------------------

/// What the Accounts page's vault section needs to know.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VaultView {
    pub has_passphrase: bool,
    pub has_recovery_key: bool,
    pub handed_over: bool,
    /// The one-time mark each form carries (`hubvault::ShownOnce::nonce`).
    pub nonce: String,
    /// A recovery key made a moment ago, shown this once.
    pub key_to_show: Option<String>,
    /// What the last press did.
    pub said: Option<String>,
}

/// The vault, on the Accounts page (`id=vault`).
///
/// Every field is `type=password` with the autocomplete a password manager
/// needs to offer the right thing: `new-password` where one is being chosen,
/// `current-password` where one is being proved.
pub fn vault_section(v: &VaultView) -> String {
    let nonce = format!("<input type=hidden name=nonce value=\"{}\">", esc(&v.nonce));
    let mut out = String::from("<section id=vault aria-labelledby=vault-h><h2 id=vault-h>Vault</h2>");
    if let Some(said) = v.said.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("<p class=notice role=status>{}</p>", esc(said)));
    }
    if let Some(code) = &v.key_to_show {
        out.push_str(&format!(
            "<div class='banner stop' role=alert><div><h3>Your recovery key — write this down now</h3>\
             <p style=\"font-size:1.4em;letter-spacing:.06em\"><code>{}</code></p>\
             <p>This is the only time it is shown. It is kept nowhere — not here, not in the vault, not in \
             any file. If the passphrase ever goes, this is the way back in; without either, what's in the \
             vault cannot be recovered by anyone.</p>\
             <form method=post action=/hub/vault><input type=hidden name=what value=written>\
             <button class=primary>I've written it down</button></form></div></div>",
            esc(code)
        ));
    }
    if v.handed_over {
        if !v.has_passphrase {
            out.push_str(&format!(
                "<p>{}</p>",
                esc(&crate::handover::not_yours_to_set())
            ));
        } else {
            out.push_str(&format!(
                "<p>This is handed over. The vault passphrase is what makes it yours again — typed here, \
                 never said out loud.</p>\
                 <form method=post action=/hub/vault autocomplete=off><input type=hidden name=what value=back>{nonce}\
                 <label for=vault-back>Vault passphrase</label>\
                 <input id=vault-back name=phrase type=password autocomplete=current-password required>\
                 <button class=primary>Take it back</button></form>"
            ));
        }
        out.push_str("</section>");
        return out;
    }
    if !v.has_passphrase {
        out.push_str(&format!(
            "<p>No passphrase yet. Until there is one, unlocking the vault proves nothing — the first \
             unlock chooses the passphrase, whoever types it — and a handover could never be taken back.</p>\
             <p class=note>Long beats complicated: a sentence you would not forget, at least twelve \
             characters. Nobody can recover it for you, and that is what makes the vault worth having. \
             A recovery key is made in the same step.</p>\
             <form method=post action=/hub/vault><input type=hidden name=what value=set>{nonce}\
             <label for=vault-new>New passphrase</label>\
             <input id=vault-new name=new type=password autocomplete=new-password minlength=12 required>\
             <label for=vault-again>The same again</label>\
             <input id=vault-again name=again type=password autocomplete=new-password minlength=12 required>\
             <button class=primary>Set the passphrase</button></form>"
        ));
        out.push_str("</section>");
        return out;
    }
    out.push_str(&format!(
        "<p>The vault has a passphrase.</p>\
         <h3>Change it</h3>\
         <form method=post action=/hub/vault><input type=hidden name=what value=change>{nonce}\
         <label for=vault-old>Current passphrase</label>\
         <input id=vault-old name=old type=password autocomplete=current-password required>\
         <label for=vault-new>New passphrase</label>\
         <input id=vault-new name=new type=password autocomplete=new-password minlength=12 required>\
         <label for=vault-again>The same again</label>\
         <input id=vault-again name=again type=password autocomplete=new-password minlength=12 required>\
         <button>Change the passphrase</button></form>"
    ));
    out.push_str(&format!(
        "<h3>Recovery key</h3><p>{}</p>\
         <form method=post action=/hub/vault><input type=hidden name=what value=recovery>{nonce}\
         <label for=vault-rk>Vault passphrase</label>\
         <input id=vault-rk name=old type=password autocomplete=current-password required>\
         <button>Make a recovery key</button></form>",
        if v.has_recovery_key {
            "There is one — the one you wrote down. Making a new one retires it: the old one stops working."
        } else {
            "There is no recovery key. If the passphrase goes, so does everything in here — and a \
             handover could never be taken back. Making one takes ten seconds."
        }
    ));
    out.push_str("</section>");
    out
}

/// The line across the top of every hub page while the machine is handed
/// over, pointing at where it is taken back.
fn handed_over_banner() -> String {
    "<div class='banner wait' role=status><span>This is handed over — the owner's things are \
     kept back.</span> <a href='/hub/accounts#vault'>Take it back</a></div>"
        .to_string()
}

/// Put the handed-over banner at the top of a page's main content.
pub fn with_handed_over_banner(page: String) -> String {
    let banner = handed_over_banner();
    match page.find("<main id=main tabindex=-1>") {
        Some(at) => {
            let at = at + "<main id=main tabindex=-1>".len();
            format!("{}{banner}{}", &page[..at], &page[at..])
        }
        None => match page.find("<body>") {
            Some(at) => format!("{}{banner}{}", &page[..at + 6], &page[at + 6..]),
            None => page,
        },
    }
}

/// Where this device stands with its household, for the Sync page.
#[derive(Debug, Clone, PartialEq)]
pub enum HouseView {
    /// Not known to the caller: every form is shown, as the page was before
    /// it knew. Only `sync_page` uses this, for its older callers.
    Unknown,
    /// No household on this device yet.
    NoneYet,
    /// This device belongs to one.
    Named { name: String, devices: Vec<String> },
}

/// Everything the Sync page shows beyond `sync_page`'s arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncView {
    pub house: HouseView,
    /// A folder to offer when none is set (`sync::best_folder`).
    pub suggested_folder: Option<String>,
}

/// The Sync page, by where this device stands: no sync folder → choose one;
/// no household → start one here, or join one; a household → its devices,
/// inviting another, and taking a key from another device.
pub fn sync_page_with(
    sealing: bool,
    folder: &str,
    phrase: Option<&str>,
    card: Option<&str>,
    last: Option<&str>,
    this_device: &str,
    view: &SyncView,
) -> String {
    let mut body = String::new();
    if let Some(said) = last.filter(|s| !s.is_empty()) {
        body.push_str(&format!("<p class=notice role=status>{}</p>", esc(said)));
    }
    let no_folder = folder.trim().is_empty();

    // Step one, when there is no folder: where your devices meet.
    if no_folder && view.house != HouseView::Unknown {
        body.push_str(&format!(
            "<section aria-labelledby=sf><h2 id=sf>Where your devices meet</h2>\
             <p class=what>Pick a folder both machines can see — one your cloud drive already keeps in \
             step is best, so they meet even when they're never on at the same time. I leave what I \
             carry there, sealed if sealing is on.</p>\
             <form method=post action=/hub/sync-setup>\
             <label for=sync-folder>Sync folder</label>\
             <input id=sync-folder name=folder autocomplete=off size=40 required value=\"{}\">\
             <label for=sync-device>What to call this device</label>\
             <input id=sync-device name=device autocomplete=off size=24 value=\"{}\">\
             <button class=primary>Use this folder</button></form>{}</section>",
            esc(view.suggested_folder.as_deref().unwrap_or("")),
            esc(this_device),
            if view.suggested_folder.is_some() {
                "<p class=note>Filled in with a cloud folder this machine already syncs. Change it if you'd rather another.</p>"
            } else {
                "<p class=note>I didn't find a cloud folder on this machine. A folder on a drive you carry between them works too.</p>"
            }
        ));
    }

    body.push_str(&format!(
        "<div class=row><div class=name>Sealing</div><div class=what>{}</div></div>",
        if sealing {
            "On — what I leave in the folder is unreadable without the key."
        } else {
            "Off — what I leave in the folder is plain text, and anyone who can read \
             the folder can read it."
        }
    ));
    body.push_str(&format!(
        "<div class=row><div class=name>Folder</div><div class=what>{}</div></div>",
        if no_folder { "Not set. Nothing is being carried anywhere.".to_string() } else { esc(folder) }
    ));
    if let HouseView::Named { name, devices } = &view.house {
        body.push_str(&format!(
            "<div class=row><div class=name>Household</div><div class=what>{}</div></div>\
             <div class=row><div class=name>Devices</div><div class=what>{}</div></div>",
            esc(name),
            esc(&devices.join(", "))
        ));
    }
    body.push_str(&format!(
        "<div class=row><div class=name>Key</div><div class=what>{}</div></div>",
        match phrase {
            Some(p) => format!("<code>{}</code>", esc(p)),
            None => "None on this device yet. I make one the first time I seal anything.".to_string(),
        }
    ));
    if let Some(path) = card {
        body.push_str(&format!(
            "<div class=row><div class=name>Written down</div><div class=what>{}</div></div>",
            esc(path)
        ));
    }

    let join_form = format!(
        "<form method=post action=/hub/sync style=\"margin-top:12px\">\
           <input type=hidden name=what value=join>\
           <label for=join-code>Code from the other machine</label>\
           <input id=join-code autocomplete=off name=code size=16>\
           <label for=join-device>What to call this machine</label>\
           <input id=join-device autocomplete=off name=device size=18 value=\"{}\">\
           <button>Join</button>\
         </form>",
        esc(this_device)
    );
    let invite = "<p class=what>Press this, and I'll put an invitation in your sync folder and show \
         you a ten-character code. Type that code on the other machine, in its own copy of this page, \
         and it joins, and the key comes with it. The invitation clears itself after fifteen minutes \
         whether it is used or not, and nothing in the folder says whose it is or what is in it.</p>\
         <form method=post action=/hub/sync style=\"margin-top:12px\">\
           <input type=hidden name=what value=pair>\
           <button>Invite a device</button>\
         </form>";

    match &view.house {
        HouseView::Unknown => {
            body.push_str("<h2>Another device</h2>");
            body.push_str(invite);
            body.push_str(&join_form);
        }
        HouseView::NoneYet if !no_folder => {
            body.push_str(&format!(
                "<section aria-labelledby=sh><h2 id=sh>Start one here</h2>\
                 <p class=what>A household is your own devices and nobody else's. Start it on this one, \
                 then invite the others from this page.</p>\
                 <form method=post action=/hub/sync><input type=hidden name=what value=init>\
                 <label for=hh-name>What to call it</label>\
                 <input id=hh-name name=name autocomplete=off size=24 placeholder=\"My devices\" required>\
                 <label for=hh-device>What to call this device</label>\
                 <input id=hh-device name=device autocomplete=off size=24 value=\"{}\">\
                 <label><input type=checkbox name=key value=yes checked> Make a household key too, so what \
                 I carry between them is sealed</label>\
                 <button class=primary>Start it</button></form></section>\
                 <section aria-labelledby=sj><h2 id=sj>Or join one</h2>\
                 <p class=what>If another of your devices already has a household, press \
                 \u{201c}Invite a device\u{201d} on its Sync page and type the code here.</p>{join_form}</section>",
                esc(this_device)
            ));
        }
        HouseView::NoneYet => {}
        HouseView::Named { .. } => {
            body.push_str("<h2>Another device</h2>");
            body.push_str(invite);
        }
    }

    body.push_str(
        "<h2>If you lose the key</h2>\
         <p class=what>You lose nothing that matters, and this is worth reading once so \
         you never worry about it again.</p>\
         <p class=what>A bundle is a courier, not where your things live. Your notes are \
         in Atlas's own folder on each machine, and every bundle I write carries the whole \
         record from the beginning — not just what changed. So if every copy of the key \
         were gone tomorrow: press the button below, your devices start using a new key, \
         and the next bundle carries everything again. The only thing lost is whatever was \
         still sitting unread in the sync folder, and that came from a machine that still \
         has it.</p>\
         <p class=what>That is why I make the key myself and do not ask you to look after \
         it. Keep the file if you like. Losing it is a nuisance, not a loss.</p>\
         <form method=post action=/hub/sync style=\"margin-top:16px\" \
           onsubmit=\"return confirm('Make a new key? Anything still unread in the \
           sync folder becomes unreadable — your devices will send it again.')\">\
           <input type=hidden name=what value=new>\
           <button>Make a new key</button>\
         </form>\
         <form method=post action=/hub/sync style=\"margin-top:8px\">\
           <input type=hidden name=what value=card>\
           <button>Write the key down again</button>\
         </form>",
    );
    if view.house != HouseView::Unknown {
        body.push_str(
            "<section aria-labelledby=sk><h2 id=sk>Use a key from another device</h2>\
             <p class=what>If another of your devices made the household key, its Sync page shows it. \
             Type it here and bundles sealed there open here too.</p>\
             <form method=post action=/hub/sync><input type=hidden name=what value=set-key>\
             <label for=sk-phrase>Household key</label>\
             <input id=sk-phrase name=phrase type=password autocomplete=off required>\
             <label><input type=checkbox name=replace value=yes> Replace my key — anything sealed under \
             the old one stops opening here</label>\
             <button>Use this key</button></form></section>",
        );
    }

    body.push_str(&format!(
        "<p class=note style=\"margin-top:20px\">The switch itself is on the \
         <a href={}>settings page</a>, under “Reaching outside this machine”.</p>",
        Page::Settings.href()
    ));
    shell_at(Some(Page::Sync), "Your devices", &body)
}

/// What the Status page's "Free up space" shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpaceView {
    /// A survey is running on the crew now.
    pub looking: bool,
    /// When the last survey finished, said for a person ("an hour ago").
    pub looked: Option<String>,
    /// What the last survey found. Those `atlas_may_move` get a checkbox.
    pub found: Vec<crate::reclaim::Candidate>,
    pub said: Option<String>,
}

/// "Free up space", on the Status page.
/// "Make it run well", on the Status page beside "Free up space" (2 Oct
/// 2026). Each button says its sentence to Atlas on the Talk page, where the
/// answer and its offer come back and "yes" carries it out -- the same path
/// as saying it, so the page can't do anything saying couldn't.
pub fn speed_section() -> String {
    let mut out = String::from(
        "<section id=speed aria-labelledby=speed-h><h2 id=speed-h>Make it run well</h2>\
         <p class=what>I measure what each program is doing for a couple of seconds, then offer what I'd \
         close or switch off. Nothing changes until you say yes, Windows and I are never on the list, and \
         \"undo\" switches startup programs back on and moves files back.</p>",
    );
    for (said, label) in [
        ("what's slowing my computer down", "What's slowing it down"),
        ("close what I don't need", "Close what I don't need"),
        ("what starts with Windows", "What starts with Windows"),
        ("what's taking up my space", "Where the space went"),
    ] {
        out.push_str(&format!(
            "<form class=inline method=post action=/hub/talk><input type=hidden name=text value=\"{}\">\
             <button>{}</button></form> ",
            esc(said),
            esc(label)
        ));
    }
    out.push_str("</section>");
    out
}

/// "Sort my files", on the Status page beside "Make it run well" (2 Oct
/// 2026). Like those buttons, each says its sentence to Atlas on the Talk
/// page: the plan comes back there, and "yes" carries it out.
pub fn sorting_section() -> String {
    let mut out = String::from(
        "<section id=sorting aria-labelledby=sorting-h><h2 id=sorting-h>Sort my files</h2>\
         <p class=what>I say what I'd move first: a folder for each kind of file, copies and old installers \
         into \"To review\" for you to empty. Nothing is deleted or written over, and \"undo that\" puts \
         everything back. To sort another folder, say \"sort the files in\" and its full path.</p>",
    );
    for (said, label) in [
        ("organize my PC", "Organize my PC"),
        ("organize my downloads", "Sort Downloads"),
        ("clean up my desktop", "Clean up the desktop"),
        ("find duplicates in my downloads", "Find duplicates in Downloads"),
    ] {
        out.push_str(&format!(
            "<form class=inline method=post action=/hub/talk><input type=hidden name=text value=\"{}\">\
             <button>{}</button></form> ",
            esc(said),
            esc(label)
        ));
    }
    out.push_str("</section>");
    out
}

pub fn space_section(v: &SpaceView) -> String {
    let mut out = String::from("<section id=space aria-labelledby=space-h><h2 id=space-h>Free up space</h2>");
    if let Some(said) = v.said.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("<p class=notice role=status>{}</p>", esc(said)));
    }
    if v.looking {
        out.push_str("<p>Looking through the disk now. It takes a minute or two; this page shows what I \
                      found once it's done.</p>");
    } else {
        out.push_str(
            "<p class=what>I look through your home and temp folders and list what's using the space. \
             Nothing is moved unless you tick it and press the button, and what is moved goes to my \
             trash for 30 days, where it can be put back.</p>\
             <form method=post action=/hub/reclaim><input type=hidden name=what value=look>\
             <button>Look for space</button></form>",
        );
    }
    if let Some(when) = &v.looked {
        let (mine, yours): (Vec<_>, Vec<_>) = v.found.iter().partition(|c| c.kind.atlas_may_move());
        out.push_str(&format!("<p class=note>Last looked {}.</p>", esc(when)));
        if v.found.is_empty() {
            out.push_str(&nothing("Nothing worth clearing turned up."));
        }
        if !mine.is_empty() {
            out.push_str("<h3>I can clear these — they rebuild</h3><form method=post action=/hub/reclaim>\
                          <input type=hidden name=what value=move>");
            for c in mine.iter().take(40) {
                out.push_str(&format!(
                    "<div class=row><label><input type=checkbox name=pick value=\"{}\"> {}</label></div>",
                    esc(&c.path.display().to_string()),
                    esc(&c.line())
                ));
            }
            out.push_str("<button class=primary>Move chosen to the trash</button></form>");
        }
        if !yours.is_empty() {
            out.push_str("<h3>Yours to judge — I won't touch these</h3>");
            for c in yours.iter().take(25) {
                out.push_str(&format!("<div class=row><div class=what>{}</div></div>", esc(&c.line())));
            }
        }
    }
    out.push_str("</section>");
    out
}
