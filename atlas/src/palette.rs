//! Everything you can reach, by typing.
//!
//! Menus stop scaling somewhere around six items. Atlas has thirteen pages and
//! a growing number of things you can actually *do* — arrange the dashboard,
//! track an account, lock the vault — and none of those actions were reachable
//! at all except by first navigating to the page that happens to hold the
//! button.
//!
//! ## What makes this different from a search box
//!
//! Three rules, all of which are the difference between a palette people use
//! and one they try twice:
//!
//! **Results are things you can do, not places to go.** "Track a new account"
//! is actionable; "Accounts" is navigation wearing an action's clothes. Both
//! are here, but the doing ones rank above the going ones.
//!
//! **It is useful before you type.** An empty palette shows what you reached
//! recently and the handful of things most people want, because the moment you
//! open it is exactly the moment you have not yet decided what to call the
//! thing you want.
//!
//! **It does not replace the menu.** The sidebar stays. A palette is how
//! someone who knows the product moves; the menu is how anyone learns what is
//! in it, and a product that only has the former cannot be learned.
//!
//! ## Why the matching is here rather than in the browser
//!
//! So it can be tested, and so the same ranking answers the typed palette and
//! the plain `/hub/find` page. A second implementation in script would be a
//! second set of rules that quietly disagrees with the first.

use serde::{Deserialize, Serialize};

/// What happens when you pick something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Does {
    /// Go somewhere.
    Go(&'static str),
    /// Do something, as a form post: (address, field pairs).
    Run(&'static str, &'static [(&'static str, &'static str)]),
}

/// One thing you can reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What it does, phrased as the doing of it.
    pub label: &'static str,
    /// The half-sentence underneath, when the label alone is ambiguous.
    pub hint: &'static str,
    pub does: Does,
    /// Other words you might reach for. Nobody types the label.
    pub also: &'static [&'static str],
}

impl Entry {
    /// A stable name for this entry, used to remember what you reached for.
    pub fn id(&self) -> &'static str {
        self.label
    }

    /// Doing something outranks going somewhere, all else equal.
    fn is_action(&self) -> bool {
        matches!(self.does, Does::Run(..))
    }
}

/// Everything, in the order it is offered before you type anything.
///
/// Hand-ordered rather than alphabetical: the top of an untyped palette is
/// prime space and should hold what you most often want, not whatever begins
/// with A.
pub fn catalogue() -> Vec<Entry> {
    vec![
        Entry {
            label: "Open the board",
            hint: "Everything still open, most pressing first",
            does: Does::Go("/hub/workspace"),
            also: &[
                "todo", "tasks", "open", "left", "pending", "work", "board",
                "kanban", "columns", "projects", "board", "outstanding",
            ],
        },
        Entry {
            label: "Your projects",
            hint: "Each project, its proposed changes, and what's waiting to be implemented",
            does: Does::Go("/hub/workshop"),
            also: &[
                "projects", "workshop", "changes", "proposed", "implement", "queue",
                "atlas", "master document", "download",
            ],
        },
        Entry {
            label: "Your calendar",
            hint: "What's coming up, soonest first",
            does: Does::Go("/hub/calendar"),
            also: &[
                "calendar", "agenda", "schedule", "events", "upcoming", "diary",
                "whats on", "appointments", "meetings", "today", "week",
            ],
        },
        Entry {
            label: "Read your messages",
            hint: "Your conversations, people and groups, in one place",
            does: Does::Go("/hub/messages"),
            also: &["messages", "chat", "chats", "inbox", "conversations", "texts", "reply", "dm"],
        },
        Entry {
            label: "Talk to Atlas",
            hint: "Say or type something to Atlas",
            does: Does::Go("/hub/talk"),
            also: &["talk", "speak", "say", "ask atlas", "voice", "microphone", "mic", "conversation"],
        },
        Entry {
            label: "See your documents",
            hint: "Everything you've handed Atlas, and what it made of each",
            does: Does::Go("/hub/documents"),
            also: &["documents", "docs", "files", "papers", "pdfs", "tray", "handed"],
        },
        Entry {
            label: "Give Atlas something",
            hint: "A link, a photo, words or a file, any size; it stays on your machine",
            does: Does::Go("/hub/give"),
            also: &["give", "hand", "share", "upload", "send a file", "drop", "attach", "paste a link"],
        },
        // Social and Opportunities joined the hub's "Your work" menu on 29 Sep
        // 2026 (the social and hunting merges); the palette gets them the
        // same day, so neither depends on which way in you reach for.
        Entry {
            label: "See how your posts are doing",
            hint: "Your accounts' numbers over time, and what's working for the people you watch",
            does: Does::Go("/hub/social"),
            also: &["social", "followers", "my posts", "my videos", "best time to post", "watch list", "trending", "analytics"],
        },
        Entry {
            label: "See the opportunities found",
            hint: "Gigs, jobs, grants and niches Atlas found, with why -- read more, drop or save",
            does: Does::Go("/hub/opportunities"),
            also: &["opportunities", "gigs", "jobs", "grants", "contracts", "leads", "hunting", "saved opportunities"],
        },
        Entry {
            label: "Check on a business",
            hint: "A business at a glance: open work, people, clients",
            does: Does::Go("/hub/business"),
            also: &["overview", "business", "company", "llc"],
        },
        Entry {
            label: "See the shared tasks",
            hint: "A business's tasks as a table, a board or a calendar",
            does: Does::Go("/hub/tasks"),
            also: &["shared tasks", "team tasks", "business tasks", "assign", "table", "kanban", "due"],
        },
        Entry {
            label: "Look up a client",
            hint: "Who you work for, and what to know about each",
            does: Does::Go("/hub/clients"),
            also: &["clients", "customers", "contacts", "crm"],
        },
        Entry {
            label: "See who you work with",
            hint: "The people and Atlases a business works with",
            does: Does::Go("/hub/partners"),
            also: &["partners", "team", "roster", "colleagues", "devs"],
        },
        Entry {
            label: "Change how Atlas sounds",
            hint: "When Atlas speaks, how loud, quiet hours, and pop-ups",
            does: Does::Go("/hub/sound"),
            also: &["sound & voice", "sound", "voice", "volume", "mute", "quiet hours", "do not disturb", "pop-ups", "popups", "wake word"],
        },
        Entry {
            label: "Choose who to trust",
            hint: "Who Atlas may send to without stopping to ask",
            does: Does::Go("/hub/trusted"),
            also: &["trusted", "recipients", "allowed", "send without asking", "safe list"],
        },
        Entry {
            label: "Put Atlas on my phone or iPad",
            hint: "Pick iPhone, iPad or Android and scan a code with it",
            does: Does::Go("/hub/phone"),
            also: &["your phone", "phone", "iphone", "ipad", "android", "install on phone", "phone app", "add my phone", "udid"],
        },
        Entry {
            label: "Check for updates",
            hint: "Which Atlas this is, installing what's new, or going back a version",
            does: Does::Go("/hub/updates"),
            also: &["updates", "update", "upgrade", "new version", "version", "install update", "go back", "roll back", "undo update"],
        },
        Entry {
            label: "Report a problem with Atlas",
            hint: "Tell whoever sends you Atlas what's wrong, and see their answer",
            does: Does::Go("/hub/feedback"),
            also: &["feedback", "report a bug", "bug", "problem", "something's wrong", "complain", "answer feedback"],
        },
        Entry {
            label: "Get help",
            hint: "Using Atlas your way, the keyboard, and reporting a barrier",
            does: Does::Go("/hub/help"),
            also: &["help & accessibility", "help", "accessibility", "a11y", "screen reader", "keyboard", "shortcuts", "barrier", "how do i"],
        },
        Entry {
            label: "See what's blocked",
            hint: "What it tried, what stopped it, and what it needs from you",
            does: Does::Go("/hub/outstanding"),
            also: &[
                "stuck", "blocked", "failed", "couldn't", "could not", "waiting", "backlog",
                "outstanding", "whats open", "still open",
            ],
        },
        Entry {
            label: "Arrange the dashboard",
            hint: "Move, resize or hide the cards",
            does: Does::Run("/hub/dash", &[("what", "arrange")]),
            also: &["customise", "customize", "rearrange", "layout", "move", "edit"],
        },
        Entry {
            label: "Track a new account",
            hint: "Tell Atlas about a site so it can say where you're exposed",
            does: Does::Go("/hub/accounts"),
            also: &["password", "login", "security", "2fa", "two factor", "add"],
        },
        Entry {
            label: "See what I sent over",
            hint: "Links and files handed to Atlas from another device",
            does: Does::Go("/hub"),
            also: &["sent", "handed", "phone", "link", "shared", "tray", "inbox"],
        },
        Entry {
            label: "Go to the dashboard",
            hint: "The front page",
            does: Does::Go("/hub"),
            also: &["home", "start", "front", "overview"],
        },
        Entry {
            label: "See recent activity",
            hint: "Everything it did without being watched",
            does: Does::Go("/hub/activity"),
            also: &["history", "log", "journal", "activity", "recent"],
        },
        Entry {
            label: "See its thinking",
            hint: "The current task and its reasoning, live",
            does: Does::Go("/hub/now"),
            also: &["now", "live", "thinking", "current", "doing"],
        },
        Entry {
            label: "Check the connections",
            hint: "Whether the things Atlas relies on are answering",
            does: Does::Go("/hub/connections"),
            also: &["internet", "offline", "network", "model", "broken", "down", "connections"],
        },
        Entry {
            label: "Check its health",
            hint: "Memory, disk and whether it's listening",
            does: Does::Go("/hub/status"),
            also: &["health", "machine", "memory", "disk", "status", "slow"],
        },
        Entry {
            label: "Change what it can see",
            hint: "The screen, the camera, the clipboard",
            does: Does::Go("/hub/settings#what-it-can-see"),
            also: &["camera", "screen", "clipboard", "ocr", "watch", "privacy"],
        },
        Entry {
            label: "Change what it may touch",
            hint: "What it can alter on this machine without asking each time",
            does: Does::Go("/hub/settings#what-it-may-touch"),
            also: &["system", "optimise", "optimize", "overnight", "backup", "tidy"],
        },
        Entry {
            label: "Change when it speaks first",
            hint: "Whether Atlas starts a conversation, and when it holds off",
            does: Does::Go("/hub/settings#when-it-speaks-first"),
            also: &["nudge", "interrupt", "quiet", "proactive", "focus", "notifications"],
        },
        Entry {
            label: "Show me the gestures",
            hint: "Every hand gesture, drawn, and how to make it",
            does: Does::Go("/hub/gestures"),
            also: &["gesture", "gestures", "hands", "hand", "remind", "forgot", "signs"],
        },
        Entry {
            label: "Open settings",
            hint: "Every switch, in one place",
            does: Does::Go("/hub/settings"),
            also: &["options", "preferences", "config", "switches", "change", "settings"],
        },
        Entry {
            label: "Check its permissions",
            hint: "Everything allowed to act without checking with you first",
            does: Does::Go("/hub/permissions"),
            also: &["permissions", "allowed", "autonomy", "consent", "unattended"],
        },
        Entry {
            label: "See your add-ons",
            hint: "What you and friends added, what each may do, and taking it back",
            does: Does::Go("/hub/addons"),
            also: &["add-ons", "addons", "plugins", "extensions", "approve", "revoke", "capabilities you added"],
        },
        Entry {
            label: "Your friends",
            hint: "Add a friend with one link, answer friend requests, and see who can reach you",
            does: Does::Go("/hub/friends"),
            also: &["friends", "add a friend", "friend link", "friend request", "pair", "pairing", "contacts", "unfriend"],
        },
        Entry {
            label: "Your groups",
            hint: "Group chats you made or are in: who's in each, and who may post",
            does: Does::Go("/hub/groups"),
            also: &["groups", "group chat", "members", "roles", "add someone", "remove someone", "release channel", "readers"],
        },
        Entry {
            label: "See your config edits",
            hint: "Your own changes to Atlas's config files, and going back to the default",
            does: Does::Go("/hub/edits"),
            also: &["edits", "config", "yaml", "customizations", "defaults", "revert", "overrides"],
        },
        Entry {
            label: "Check what I carry between devices",
            hint: "Whether bundles are sealed, the key, and how to start a new one",
            does: Does::Go("/hub/sync"),
            also: &[
                "your devices", "sync", "devices", "key", "sealed", "encrypt", "bundle",
                "folder", "dropbox", "onedrive", "lost", "recovery",
            ],
        },
        Entry {
            label: "Open the vault list",
            hint: "What the vault is holding, by name — never the values",
            does: Does::Go("/hub/accounts"),
            also: &["vault", "secrets", "keys", "codes", "stored", "accounts"],
        },
        Entry {
            label: "Take back site access",
            hint: "Stop Atlas being able to sign into somewhere",
            does: Does::Go("/hub/access"),
            also: &["revoke", "remove", "sign out", "access", "stop"],
        },
        Entry {
            label: "Read its self-review",
            hint: "Its own findings, with the reasoning",
            does: Does::Go("/hub/recommendations"),
            also: &["ideas", "suggestions", "improve", "improvements", "self", "audit"],
        },
        Entry {
            label: "Open the history",
            hint: "What moved, and what Atlas was thinking at the time",
            does: Does::Go("/hub/back"),
            also: &["yesterday", "history", "past", "review", "back", "earlier days"],
        },
        Entry {
            label: "Put the dashboard back how it was",
            hint: "Undo every card you moved or hid",
            does: Does::Run("/hub/dash", &[("what", "reset")]),
            also: &["reset", "default", "undo", "restore", "start over"],
        },
        Entry {
            label: "See what works without internet",
            hint: "What works with no internet, and what's waiting to go out",
            does: Does::Go("/hub/offline"),
            also: &["offline", "no internet", "no signal", "waiting to send", "queue", "airplane"],
        },
    ]
}

/// How well a query matches an entry, or `None` for no match at all.
///
/// Deliberately not fuzzy in the "match any letters anywhere" sense. That kind
/// of matching returns something for every query, which sounds generous and
/// means the palette never says "I don't have that" — so you keep typing at a
/// list of wrong answers instead of learning the thing is not there.
fn score(entry: &Entry, query: &str) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let label = entry.label.to_lowercase();
    let hint = entry.hint.to_lowercase();

    let mut best = None;

    // Whole query, at the start of the label: the strongest possible signal.
    if label.starts_with(&q) {
        best = Some(1000);
    } else if label.contains(&q) {
        best = Some(700);
    }

    // A word in the label starting with the query. "acc" should find
    // "Track a new account" even though the label starts with "Track".
    if best.is_none() && label.split_whitespace().any(|w| w.starts_with(&q)) {
        best = Some(600);
    }

    // The words you would actually reach for.
    for alt in entry.also {
        let a = alt.to_lowercase();
        if a == q {
            best = Some(best.map_or(900, |b: i32| b.max(900)));
        } else if a.starts_with(&q) {
            best = Some(best.map_or(500, |b: i32| b.max(500)));
        }
    }

    if best.is_none() && hint.contains(&q) {
        best = Some(300);
    }

    // Every word of a multi-word query has to land somewhere. "open set"
    // should find settings; "open banana" should find nothing.
    if best.is_none() && q.split_whitespace().count() > 1 {
        let all = format!("{label} {hint} {}", entry.also.join(" "));
        if q.split_whitespace().all(|w| all.contains(w)) {
            best = Some(400);
        }
    }

    // Doing beats going. A palette full of destinations is a menu with extra
    // steps.
    best.map(|b| if entry.is_action() { b + 40 } else { b })
}

/// The entries that match, best first.
///
/// An empty query returns everything in catalogue order, so the palette is
/// useful the instant it opens rather than only once you have decided what to
/// call the thing you want.
pub fn find<'a>(entries: &'a [Entry], query: &str, recent: &Recent) -> Vec<&'a Entry> {
    let mut hits: Vec<(i32, usize, &Entry)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| score(e, query).map(|s| (s + recent.bonus(e), i, e)))
        .collect();
    // Ties break on catalogue order, not on hash order: a palette whose
    // results reshuffle between identical queries cannot be learned.
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, _, e)| e).collect()
}

/// When nothing matches, the one entry a typo away — "setings" → Settings.
///
/// This does not loosen `score`. The palette stays exact; this runs only
/// after it has found nothing, uses Meilisearch's typo allowance (`typos`:
/// none under 5 letters, one up to 8, two from 9, a wrong first letter
/// counts double), and the answer is shown as a question, never opened on
/// your behalf. "banana" is two edits from nothing here, so it still gets
/// "nothing matches".
pub fn did_you_mean<'a>(entries: &'a [Entry], query: &str) -> Option<&'a Entry> {
    let words: Vec<String> =
        entries.iter().map(|e| format!("{} {}", e.label, e.also.join(" ")).to_lowercase()).collect();
    let refs: Vec<&str> = words.iter().map(|w| w.as_str()).collect();
    let best = crate::typos::suggest(&query.to_lowercase(), &refs).into_iter().next()?;
    // A zero-typo "suggestion" would mean `score` missed a match, which is a
    // bug to fix there, not something to paper over here.
    (best.1 > 0).then(|| &entries[best.0])
}

/// How many entries to offer at once.
///
/// Enough to hold the answer, few enough to read without scrolling. A palette
/// that lists everything is the menu again.
pub const SHOW: usize = 7;

/// What you have reached for lately.
///
/// The palette's memory. Not a frequency count: what you did most this year is
/// a worse predictor than what you did this morning, and a count that only
/// grows means the thing you used heavily once outranks the thing you use now.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    /// Most recent first.
    pub picked: Vec<String>,
}

/// Where it is kept.
pub const FILE: &str = "palette";

/// How many to remember.
pub const KEEP: usize = 8;

impl Recent {
    pub fn load(store: &crate::store::Store) -> Recent {
        store.load::<Recent>(FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    /// Remember that this was picked. Moves it to the front rather than
    /// adding a duplicate.
    pub fn picked(&mut self, id: &str) {
        self.picked.retain(|p| p != id);
        self.picked.insert(0, id.to_string());
        self.picked.truncate(KEEP);
    }

    /// A nudge up the list, decaying with position.
    ///
    /// Small on purpose. It should break a tie between two equally good
    /// matches, never float a poor match above a good one — a palette that
    /// answers with what you did last time instead of what you just typed is
    /// worse than one with no memory at all.
    fn bonus(&self, e: &Entry) -> i32 {
        match self.picked.iter().position(|p| p == e.id()) {
            Some(i) => (KEEP as i32 - i as i32) * 6,
            None => 0,
        }
    }

    /// What to offer before anything is typed.
    pub fn first<'a>(&self, entries: &'a [Entry]) -> Vec<&'a Entry> {
        self.picked
            .iter()
            .filter_map(|id| entries.iter().find(|e| e.id() == id))
            .collect()
    }
}
