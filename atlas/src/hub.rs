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
    /// Shown on this device: on the phone app, not the pages that only make
    /// sense on a laptop -- putting Atlas on a phone, updating itself (the
    /// App Store or TestFlight does that), syncing, offline packs, camera
    /// gestures and add-ons (`phonemode`, 2 Oct 2026).
    pub fn here(self) -> bool {
        !(crate::phonemode::on()
            && matches!(self, Page::Phone | Page::Updates | Page::Sync | Page::Offline | Page::Gestures | Page::AddOns))
    }

    /// Pages whose address carries a choice: which conversation, business or
    /// view, or what a share sheet sent.
    pub fn reads_query(self) -> bool {
        matches!(
            self,
            Page::Messages | Page::Business | Page::SharedTasks | Page::Clients | Page::Give | Page::Sound
                | Page::Trusted | Page::Talk | Page::Help | Page::Workshop | Page::Updates | Page::Feedback
                | Page::Connections | Page::Documents | Page::Phone | Page::Recommendations | Page::Accounts
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


/// How a row's dot is coloured: ember for yours to act on, blue for proposed,
/// grey for the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Act,
    Proposed,
    Record,
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


const SEARCH_ICON: &str = "<svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><circle cx=11 cy=11 r=7 /><path d='M21 21l-4-4'/></svg>";
const MIC_ICON: &str = "<svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><rect x=9 y=3 width=6 height=12 rx=3 /><path d='M6 11a6 6 0 0 0 12 0M12 17v4'/></svg>";
const HELP_ICON: &str = "<svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><circle cx=12 cy=12 r=9 /><path d='M9.5 9.5a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.6V14M12 17.5v.01'/></svg>";


/// Where a business's group goes in the sidebar, until one is written in.
const BUSINESS_SLOT: &str = "<!--atlas:business-->";


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


/// Dragging, with a mouse or a finger.
///
/// Pointer events rather than HTML5 drag-and-drop, because the HTML5 API does
/// not fire on touch at all — a dashboard that can only be rearranged with a
/// mouse is one you cannot rearrange on the device you are most often holding.
///
/// It posts the same fields the buttons post, to the same address, so the two
/// cannot drift apart. If the script never runs, the buttons still do.
const DRAG_SCRIPT: &str = concat!("<script>", include_str!("../assets/hub/drag.js"), "</script>");


// ---------------------------------------------------------------------------
// The palette.
// ---------------------------------------------------------------------------


/// Opening and filtering.
///
/// Filtering happens against words already in the page, so it costs nothing
/// and works with the network down. The ranking that decides *order* stays on
/// the Rust side — a second ranking here would be a second set of rules that
/// quietly disagrees with the one the plain page uses.
const PALETTE_SCRIPT: &str = concat!("<script>", include_str!("../assets/hub/palette.js"), "</script>");


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


/// Keeps a page live without reloading it: every few seconds, ask
/// `/hub/changed.json` whether what the page shows has changed -- a few
/// bytes -- and only then fetch the page and swap in the part marked
/// `data-live`, unless paused. It used to fetch the whole page every three
/// seconds whether or not anything had changed (28 Sep 2026). Stops by
/// itself after ten minutes with no change (WCAG 2.2.2), and says so.
pub const LIVE_SCRIPT: &str = concat!("<script>", include_str!("../assets/hub/live.js"), "</script>");


// ---------------------------------------------------------------------------
// The pages that replaced terminal commands (27 Sep 2026): the vault on the
// Accounts page, a Sync page that starts a household, and "Free up space" on
// Status. The handlers are in `hubvault.rs`; these only draw.
// ---------------------------------------------------------------------------

/// What the Accounts page's vault section needs to know.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VaultView {
    /// Opens with your Windows sign-in (5 Oct 2026: the usual way).
    pub opens_on_login: bool,
    /// Made before that, with a passphrase and no sign-in copy yet.
    pub needs_its_passphrase_once: bool,
    /// What's in the old vault set aside by `vault::move_to_sign_in`, by name.
    pub set_aside: Vec<String>,
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

// The rest of this module, by what it does (audit Q6, 6 Oct 2026).
mod pieces;
pub use pieces::*;
mod frame;
pub use frame::*;
mod pages;
pub use pages::*;
mod deck;
pub use deck::*;
mod palette;
pub use palette::*;
mod status;
pub use status::*;
mod vault_sync;
pub use vault_sync::*;

