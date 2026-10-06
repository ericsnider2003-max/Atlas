//! The local API.
//!
//! This is what makes "carry on from my phone" possible. Atlas listens on
//! loopback only; anything from another device reaches it through a VPN that
//! terminates on this machine, so the listener itself is never exposed.
//!
//! Three rules, none of them optional:
//!
//! 1. **Loopback only.** Binding to 0.0.0.0 would put a command endpoint for
//!    your workspace on whatever café network you're on.
//! 2. **Every request carries a token.** Generated on first run, stored with
//!    the rest of Atlas's state, compared in constant time.
//! 3. **Read and queue, never execute directly.** A phone can ask what's
//!    happening and add to the queue. It cannot make Atlas type into a window
//!    you can't see.

use crate::error::{AtlasError, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub enabled: bool,
    pub port: u16,
    /// Body size ceiling. A phone sends sentences, not files.
    pub max_body: usize,
    /// The cap for `/hand/file` only.
    ///
    /// Separate from `max_body` on purpose. Every other endpoint takes a short
    /// JSON request, and raising the limit for all of them so a photo can get
    /// through would mean any request on the port could ask Atlas to hold
    /// twenty megabytes.
    #[serde(default = "default_max_upload")]
    pub max_upload: usize,
    /// One more address the hub may answer on, for reaching it from your
    /// phone.
    ///
    /// Empty, and empty means loopback only — which is what this listener has
    /// always done and what it should keep doing for anyone who does not
    /// deliberately change it.
    ///
    /// The case it exists for: the sync page has the buttons that fix sync,
    /// and the day sync is broken is a day you may be holding a phone rather
    /// than sitting at the machine. Loopback is unreachable from a phone even
    /// over a VPN, so "use a VPN" was never an answer on its own.
    ///
    /// What it will accept is narrow and checked: a private or overlay-network
    /// address — `10.`, `192.168.`, `172.16–31.`, Tailscale's `100.64–127.`,
    /// link-local, or a loopback address. Never `0.0.0.0`, never a public
    /// address. See [`bind_address`], which is where the rule lives and where
    /// it is tested.
    #[serde(default)]
    pub reachable_from: String,
}

/// Which address the hub may listen on.
///
/// The rule rather than the intention, so it can be tested. Anything outside
/// it is refused with a sentence naming what was wrong, because a listener
/// that silently falls back to loopback after you asked for something else is
/// a setting that does nothing — and a listener that silently does what you
/// asked when you asked for the whole internet is worse.
pub fn bind_address(reachable_from: &str) -> std::result::Result<std::net::IpAddr, String> {
    use std::net::IpAddr;
    let want = reachable_from.trim();
    if want.is_empty() {
        return Ok(IpAddr::from([127, 0, 0, 1]));
    }
    let addr: IpAddr = want
        .parse()
        .map_err(|_| format!("`{want}` isn't an address. Leave it empty for this machine only."))?;
    match addr {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            let private = o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                // Tailscale and other CGNAT overlays.
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 169 && o[1] == 254)
                || o[0] == 127;
            if v4.is_unspecified() {
                return Err(
                    "0.0.0.0 means every network this machine is on, including whatever wifi you are on next week. Give the one address you reach it by."
                        .into(),
                );
            }
            if !private {
                return Err(format!(
                    "`{want}` is a public address. This is a personal assistant with your notes in it; put it on your own network or a VPN and give me that address instead."
                ));
            }
            Ok(addr)
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() {
                return Err("`::` means every network. Give the one address you reach it by.".into());
            }
            let s = v6.segments()[0];
            // Unique-local (fc00::/7), link-local (fe80::/10), or loopback.
            let private = (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80 || v6.is_loopback();
            if !private {
                return Err(format!(
                    "`{want}` is a public address. Put it on your own network or a VPN and give me that address instead."
                ));
            }
            Ok(addr)
        }
    }
}

/// Room for a 20 MB file once base64 has made it a third larger, plus the
/// rest of the request.
fn default_max_upload() -> usize {
    28 * 1024 * 1024
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            enabled: false,
            port: 8787,
            max_body: 16 * 1024,
            max_upload: default_max_upload(),
            reachable_from: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: String,
    /// The path alone. The query is in [`Request::query`] — keeping them
    /// together is what made `hub::route` miss `/hub?t=…`.
    pub path: String,
    /// Everything after the `?`, undecoded. Read through
    /// [`query_field`].
    pub query: String,
    pub token: Option<String>,
    /// The token arrived in the URL and nowhere else.
    ///
    /// Read by `handle_conn`, which answers such a request with a redirect
    /// that sets a `SameSite=Strict` cookie and drops the token from the
    /// address. Two reasons, and the second is the one that matters:
    ///
    /// 1. A token in a URL goes into browser history, into the `Referer` of
    ///    anything the page links to, and into whatever the person pasted
    ///    the address into. One visit, and it is out of the address bar.
    /// 2. **`SameSite=Strict` is what keeps CSRF shut.** A cookie the
    ///    browser sends automatically would otherwise let any page on any
    ///    other origin submit a form to this one; `Strict` means the browser
    ///    does not send it on a cross-site request at all. The header-only
    ///    scheme had that property by accident (no browser sends a custom
    ///    header unasked) and lost it the moment the hub became reachable,
    ///    so it is now held on purpose.
    pub token_from_url: bool,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    Json,
    Html,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    pub kind: Body,
    /// A `Set-Cookie` value to send with this reply, if any.
    ///
    /// Only ever set by [`Reply::cookie_then`]. A field rather than something
    /// smuggled inside `body`, because `render` has to emit it as a header
    /// and a header is not a body.
    pub set_cookie: Option<String>,
    /// A file to save rather than a page to show: (filename, media type).
    /// The hub's "download your calendar" and "your contacts" buttons.
    pub download: Option<(String, &'static str)>,
    /// Bytes to play or show in place, not text: (media type, bytes). A
    /// voice's sample on the Sound page (`voicepick`).
    pub bytes: Option<(&'static str, Vec<u8>)>,
}


/// The cookie the browser carries once the URL has handed its token over.
pub const COOKIE: &str = "atlas_hub";


/// Something typed that must not be repeated: a passphrase, a household key.
///
/// `Action` derives `Debug`, and actions are logged, cloned and compared. A
/// passphrase in a plain `String` field would print itself into the first
/// `{:?}` anyone wrote -- so the words are kept in a type whose `Debug` says
/// only that there is a secret here (27 Sep 2026, when the hub began taking
/// passphrases).
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);


/// What the API can be asked to do.
///
/// Deliberately small. Everything here either reads state or adds to a queue
/// the daemon drains under its normal rules — the phone gets no shortcut past
/// the approval gate.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Status,
    Outstanding,
    Queued,
    Health,
    /// Add a command to the queue. Runs under the same policy as speech.
    Say(String),
    /// Approve something Atlas already asked about, by id.
    Approve(u64),
    Deny(u64),
    /// Show a hub page.
    Hub(crate::hub::Page),
    /// Change a setting from the hub.
    HubSet { key: String, value: String },
    /// Your calendar as an .ics file, and your client list as a .vcf — the
    /// hub's download buttons.
    ExportCalendar,
    ExportClients,
    /// A file brought in from the hub: an .ics goes into the calendar, a .vcf
    /// into the client list, anything else to the tray to be read.
    BringIn { name: String, base64: String },
    /// Implement a queued change, by its title — the hub's implement button.
    Implement(String),
    /// A button on the sync page: make a new household key, write the
    /// recovery card out again, or invite another device.
    SyncKey(String),
    /// The other half of inviting: the code typed on the device that is
    /// joining, and what to call it.
    SyncJoin { code: String, device: String },
    /// Take Atlas's access to one site away. Immediate, and it does not touch
    /// your password — Atlas simply stops having it.
    RevokeAccess(String),
    /// Take all of it away at once.
    RevokeAllAccess,
    /// A button on the Add-ons page: approve, revoke, off, on.
    AddOn { what: String, id: String, key: String, sha: String },
    /// "Back to the default" on the Your edits page.
    ForgetEdit { file: String, path: String },
    /// A button on the Groups page: new, add, remove, role, rename.
    GroupChange { what: String, group: String, who: String, role: String },
    /// A button on the Friends page.
    Friend { what: String, who: String, link: String },
    /// Rearrange the dashboard. Both the move buttons and a drop post this --
    /// one route, so the two ways of moving a card cannot disagree.
    DashMove(crate::dash::Move),
    /// Turn arranging on or off. Reading is the default, because a dashboard
    /// you rearrange by accident while reading is worse than one you cannot
    /// rearrange at all.
    DashArrange(bool),
    /// Pause Atlas (true) or have it carry on (false), from Now's button —
    /// the same as saying "pause" or "carry on".
    Pause(bool),
    /// A hub page that reads its address's query: which conversation, which
    /// business, which view — or, for Give, what a phone's share sheet sent.
    HubQ(crate::hub::Page, String),
    /// A hub form that arrived without what it needs (an empty site, a
    /// button with no card): back to its page, saying what was missing,
    /// rather than "that isn't a page in Atlas".
    HubBack(crate::hub::Page, String),
    /// The Talk page as data: the last exchanges, what's queued, and
    /// whether Atlas is answering now (`atlas mcp`'s `atlas_ask`).
    TalkJson,
    /// What Atlas is doing and what's ready for you, as data: the phone app's
    /// live-activity card and notifications read it.
    LiveJson,
    /// What a phone widget shows (`glance`).
    GlanceJson,
    /// Has a live page (`now`, `talk`) changed since it was drawn? A few
    /// bytes a page polls instead of fetching itself whole every few seconds
    /// (28 Sep 2026).
    Changed(String),
    /// The phone's calendar, read on the phone and sent here to be merged;
    /// the answer carries Atlas's own events back (H7).
    PhoneCalendar(String),
    /// The iPhone app's push address, for its own Atlas to carry to the
    /// laptop (item 15).
    PushToken(String),
    /// An Android phone's UnifiedPush address and keys (item 15).
    WebPushEndpoint(String),
    /// A voice's sample, to hear it before downloading it (`voicepick`).
    VoiceSample(String),
    /// The Accounts page's vault forms: set a first passphrase, change it,
    /// or make a recovery key (`what` = set | change | recovery). `nonce` is
    /// the form's one-time mark, so a refresh cannot send it twice.
    Vault { what: String, old: Secret, new: Secret, again: Secret, nonce: String },
    /// "Take it back" on the Accounts page, while handed over.
    TakeBack { phrase: Secret, nonce: String },
    /// "Use a key from another device" on the Sync page.
    SyncKeySet { phrase: Secret, replace: bool },
    /// "Start one here" on the Sync page: a household, what to call this
    /// device, and whether to make a household key with it.
    HouseholdInit { name: String, device: String, key: bool },
    /// A form posted to one of the hub pages the locked design added
    /// (messages, tasks, clients, sound, trusted, give, talk, help, a new
    /// project): the path, and its fields decoded.
    HubPost { path: String, fields: Vec<(String, String)> },
    /// A file handed over from another device: the bytes, base64-encoded.
    ///
    /// Base64 rather than a multipart upload because the thing sending this is
    /// usually a phone shortcut, and every shortcut tool on every platform can
    /// base64 a file and post JSON. Multipart would be smaller on the wire and
    /// harder to set up, and the setup is where this feature lives or dies.
    HandFile {
        name: String,
        base64: String,
        space: Option<String>,
        from: String,
        asked: Option<String>,
    },
    /// Something handed over from another device: a link, a path, or words.
    ///
    /// Deliberately on the hub's own door rather than `kin`'s. Your phone is
    /// you; another Atlas is not, and giving them the same entrance would mean
    /// one door to keep honest for two different kinds of trust.
    Hand {
        what: String,
        space: Option<String>,
        from: String,
        /// What you wanted done with it, if you said.
        asked: Option<String>,
    },
    /// Mark a handed-over thing finished with.
    TrayDone(u64),
    /// Search everything you can reach. `q` may be empty, which is the
    /// palette opened and not yet typed into.
    Find(String),
    /// One choice from the hub's appearance menu.
    Appearance { what: String, to: String },
    /// Change what Atlas knows about one of your accounts. Never a password —
    /// only what kind of protection the site has, which is what the audit
    /// reads and is not itself worth stealing.
    Account(crate::accounts::Change),
    /// A message from another, trusted Atlas. Reachable only through
    /// `route_signal`, never through `route` -- see the module doc on why
    /// that separation is structural rather than a convention.
    Signal(crate::kin::Incoming),
    /// A note another, trusted Atlas handed over. Reachable only through
    /// `route_handoff`, and its only destination is the waiting list a
    /// person then decides about -- never the tray, never an intent.
    Handed(crate::kin::Delivered),
    /// A chat message from another, trusted Atlas. Reachable only through
    /// `route_chat`, and its only destination is `chat::Chats::receive` in a
    /// room the receiver opens for the sender the token names -- never an
    /// intent, never the tray.
    Chatted(crate::kin::Chatted),
    /// A read receipt from another, trusted Atlas. Reachable only through
    /// `route_read`, and its only destination is `chat::Chats::mark_read` --
    /// never a room, a message, an intent, or the tray.
    ReadReceipt(crate::kin::ReadReceipt),
    /// A notice from another, trusted Atlas that they have left a group.
    /// Reachable only through `route_left`, and its only destination is
    /// dropping that peer from that group's membership.
    LeftGroup(crate::kin::LeftGroup),
    /// A paired Atlas introducing its key. Reachable only through
    /// `route_hello`; its only destination is pinning that key.
    PeerHello(crate::kin::Hello),
    /// A group's signed list. Reachable only through `route_group`; its only
    /// destination is `groups::Groups::take`.
    PeerGroup(crate::kin::GroupList),
    /// Someone used one of your friend links. Reachable only through
    /// `route_friend`; its only destination is recording a friend.
    Befriended(crate::kin::Befriended),
    /// Feedback from a paired Atlas. Reachable only through
    /// `route_feedback`; its only destination is filing it.
    PeerFeedback(crate::kin::FeedbackIn),
    /// An answer to feedback this Atlas sent. Reachable only through
    /// `route_feedback`; its only destination is `feedback::heard_answer`.
    PeerFeedbackAnswer(crate::kin::FeedbackIn),
}


/// Room for a few thousand calendar events: far more than six weeks of
/// anyone's diary, and still small.
pub const CALENDAR_BODY: usize = 2 * 1024 * 1024;


/// No other page may show Atlas's inside a frame of its own, where a click
/// meant for that page lands on a hub button (1 Oct 2026 security pass).
const NOT_IN_A_FRAME: &str = "X-Frame-Options: DENY\r\nContent-Security-Policy: frame-ancestors 'none'\r\n";


/// What the token is stored in, between runs.
#[derive(Debug, Default, Serialize, Deserialize)]
struct HeldToken {
    token: String,
}


/// Failed attempts, and the delay they earn.
///
/// On its own this is hygiene: a genuinely random 24-character token isn't
/// brute-forceable. It matters because it's the mechanism that would make a
/// weak token exploitable, and a control that only works while another control
/// holds is one worth having anyway.
///
/// Delay rather than lockout: locking out means something else on the machine
/// can deny you your own hub by failing twice.
#[derive(Debug, Clone, Default)]
pub struct Failures {
    count: u32,
    last_at: u64,
}


/// The listener.
///
/// `handle` receives an authenticated action and returns what to send back.
/// Serving one connection at a time is deliberate: this is a phone talking to
/// your own laptop, and a threadpool would be more moving parts than the job
/// needs.
pub struct Server {
    listener: TcpListener,
    /// Loopback as well, when `reachable_from` named another address: Atlas's
    /// own window and this machine's browser open `127.0.0.1`, and a server
    /// bound only to the network address refused them (27 Sep 2026).
    also: Option<TcpListener>,
    token: String,
    max_body: usize,
    max_upload: usize,
    /// Peers on the narrow, notify-only door. `None` means the door does not
    /// exist for this server at all -- most Atlas installs will never have
    /// another Atlas to talk to, and an empty-but-present door is a
    /// different, worse thing than no door.
    signals: Option<std::sync::Mutex<crate::kin::Door>>,
    /// Wrong tokens lately (Eric, B2): each answer to a wrong one is slowed,
    /// so guessing the token from the phone link's address is slow too.
    failures: std::sync::Mutex<Failures>,
    /// How long a connection waits for the daemon (`ANSWER_WAIT`).
    answer_wait: std::time::Duration,
    /// Which Atlas this is, said to `/hub/ping` (28 Sep 2026): how Setup,
    /// "Open Atlas" and `atlas hub` tell Atlas's own hub from another
    /// program that happens to answer on the port. Not a secret -- it opens
    /// nothing -- and new at every start.
    id: String,
}

impl Server {


}

/// A socket read that never waits past `until`, however slowly the other
/// end sends (28 Sep 2026). Each read's timeout is set to the time left, so
/// a client trickling a byte at a time runs out of time, not the reader out
/// of patience one byte at a time.
struct Deadlined {
    stream: TcpStream,
    until: std::time::Instant,
}


/// The slowest a request body may arrive, in bytes a second.
const MIN_BODY_RATE: u64 = 16 * 1024;


/// How long a peer's request may take to arrive in full.
const PEER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// A request `Server::read_asked` has read in full and let through, with the
/// socket its answer goes back on. `page` is the method and path when it
/// came through the hub's routes (the reply gets the app head and the
/// failed-page treatment), `None` for the narrow peer door.
struct Asked {
    stream: TcpStream,
    action: Action,
    page: Option<(String, String)>,
}

/// One authenticated hub request waiting for the daemon to answer it.
pub struct Waiting {
    pub action: Action,
    /// `GET /hub/notes` -- method and path only, never the query (the token
    /// can travel there) and never a body.
    pub what: String,
    arrived: std::time::Instant,
    reply: std::sync::mpsc::SyncSender<Reply>,
    /// `WAITING`, then `TAKEN` by the daemon or `ABANDONED` by the
    /// connection that gave up and answered "busy" -- whichever comes first.
    /// A request answered "busy" used to stay queued and run anyway when
    /// the daemon got to it (28 Sep 2026): a POST applied after the browser
    /// was told it wasn't, and a one-time flash (a recovery key, a friend
    /// link) spent on a page nobody saw.
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    /// How long its connection waits (`Server::answer_wait`).
    wait: std::time::Duration,
    /// Where a Talk page message goes when it is given up on.
    late: LateTalk,
}

/// Words sent from the Talk page that Atlas didn't get to before the page
/// gave up (29 Sep 2026: during a long stall they were answered "busy" and
/// never run -- what you typed was simply gone). Kept by the door, off the
/// loop, and taken into the Talk page's queue on its next pass
/// (`HubDoor::take_late_talk`). The page's own retry then shows them
/// "thinking".
type LateTalk = std::sync::Arc<std::sync::Mutex<Vec<(String, bool)>>>;


const WAITING: u8 = 0;
const TAKEN: u8 = 1;
const ABANDONED: u8 = 2;


/// What answering one waiting request cost, for the log.
#[derive(Debug, Clone)]
pub struct HubCost {
    pub what: String,
    /// From the request arriving in full to Atlas starting on it.
    pub waited_ms: u64,
    /// Atlas working out the answer.
    pub took_ms: u64,
}

/// The hub on its own threads (27 Sep 2026).
///
/// ## Why
///
/// Eric: "Atlas felt slow when clicking around and trying to talk to it and
/// that is not acceptable." The cause was here. The daemon's loop called
/// `poll_once` once a pass, and `poll_once` takes ONE connection. A pass
/// also waits on the keyboard (half a second), the wake word (seconds of
/// recording) and a nap of up to two seconds. One click in a browser is
/// four or more requests (the page, the manifest, icons, the service
/// worker), the open page refetches itself every few seconds, and the setup
/// window probed the port with empty connections every two seconds -- each
/// of those used up a whole pass. So a click waited seconds in a queue
/// behind other connections, with the daemon asleep between them.
///
/// ## What
///
/// A listener thread accepts, and each connection gets a short-lived thread
/// of its own that reads and checks it with exactly the code `poll_once`
/// uses (`read_asked`): the token, the cookie trade, the caps, the deadline
/// and the delay for a wrong guess are unchanged, they just happen off the
/// daemon's thread. The public files, the manifest, every refusal and every
/// 404 are answered right there. Only an authenticated action reaches the
/// daemon, through a channel, and the daemon answers every one waiting each
/// time it looks (`answer_waiting`), which it now does every 50ms while idle
/// (`wait_and_answer`) instead of napping.
///
/// The daemon still owns every answer: `&mut Daemon` never crosses a thread,
/// which is the reason the loop was single-threaded in the first place.
pub struct HubDoor {
    /// The server, once it is listening (`open_hub` may still be waiting
    /// for its port).
    server: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<Server>>>,
    asks: std::sync::mpsc::Receiver<Waiting>,
    /// Where it answers now, 0 while it has no port yet.
    port: std::sync::Arc<std::sync::atomic::AtomicU16>,
    /// What there is to say about the door (its port taken, a fallback),
    /// for the log; emptied as it's read (`take_news`).
    news: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    /// Set when the door is dropped: the retrying thread stops.
    shut: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Talk page messages given up on (`LateTalk`).
    late: LateTalk,
}

/// The hub's end of a `HubDoor`: what a listener, bound now or later, feeds.
struct Serving {
    server: std::sync::Arc<std::sync::OnceLock<std::sync::Arc<Server>>>,
    tx: crate::doorbell::Sender<Waiting>,
    open: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    per_address: PerAddress,
    port: std::sync::Arc<std::sync::atomic::AtomicU16>,
    news: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    shut: std::sync::Arc<std::sync::atomic::AtomicBool>,
    late: LateTalk,
}


/// How `open_hub` waits for a port that's taken. Production values in
/// `Default`; the tests shorten them.
#[derive(Debug, Clone, Copy)]
pub struct Retry {
    /// The first wait before trying the port again; doubled each time...
    pub first: std::time::Duration,
    /// ...up to this.
    pub longest: std::time::Duration,
    /// How long the configured port is waited for before the hub opens on
    /// another one.
    pub fall_back_after: std::time::Duration,
    /// Once on another port, how often the configured one is tried again.
    pub look_again_every: std::time::Duration,
}


/// The ports tried after the configured one, before any free port at all:
/// the next few, so the address is still easy to find.
pub const FALLBACK_PORTS: u16 = 10;


/// How many connections may be being read at once. A browser opens six or
/// so per origin; a phone and a laptop together, a few more. Past this a new
/// connection is closed unread rather than given a thread -- something on
/// the machine opening hundreds is not a person clicking.
const MAX_OPEN: usize = 48;

/// How many of those may come from one address (28 Sep 2026), so one
/// machine on the network can't take every place and lock the phone and the
/// desktop out. A browser opens about six at once.
const MAX_OPEN_PER_ADDRESS: usize = 16;

type PerAddress = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, usize>>>;


/// How long a connection waits for the daemon's answer before it is told
/// Atlas is busy. Long, because a page asked for while Atlas is in the
/// middle of answering you (a model reply can take tens of seconds on a
/// laptop) is answered as soon as it's free, and that is better than an
/// error; but bounded, so a connection never waits forever on a daemon that
/// has stopped.
const ANSWER_WAIT: std::time::Duration = std::time::Duration::from_secs(90);


// ---------------------------------------------------------------------------
// Which Atlas answers on the hub's port (28 Sep 2026)

/// Where `Server` answers "which Atlas is this?".
pub const PING_PATH: &str = "/hub/ping";

/// The file in `data/state` that says where the running Atlas's hub is and
/// which Atlas it is (`open_hub`'s `on_open` writes it).
pub const DOOR_FILE: &str = "hub_door.json";

/// What `DOOR_FILE` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Door {
    pub port: u16,
    pub id: String,
}


// ---------------------------------------------------------------------------
// A door that fits inside the daemon's own loop.
//
// `Server` blocks on `accept()` — right for `run_hub`, which has nothing
// else to do while it waits. `Daemon::run` is different: it already polls
// everything once per tick (typing, the wake word, scheduled work) rather
// than blocking on any one of them, and bolting a second, blocking listener
// onto that would mean either freezing the whole assistant while it waits
// for a peer that may never call, or standing up a second thread sharing
// `&mut Daemon` across threads — which `Daemon<'a>`'s borrowed lifetimes and
// `dyn Platform`/`dyn Llm` trait objects are not set up for, and forcing that
// through under time pressure is exactly the kind of thing that ships a race
// condition nobody notices until it's a nudge that fires twice or a message
// that's lost. So: same shape as everything else in the loop instead.
//
// A non-blocking accept, checked once per tick, handled inline, on the same
// thread as everything else. `Daemon::run`'s loop already tolerates
// `keyboard.poll()` costing nothing when nothing was typed; this costs the
// same — nothing — when nobody has signalled.
// ---------------------------------------------------------------------------

/// The kin door, opened in a way `Daemon::run`'s loop can check without
/// blocking on it.
///
/// Two kinds of socket, since 25 Sep:
///
/// * **The ordinary door**, on every address this machine has, for your own
///   devices -- on this machine, your home network, or your own private
///   network. From anywhere else a request in the clear is refused before its
///   token is looked at (`onion::is_local_origin`).
/// * **The sealed door**, on this machine only, which Tor forwards your onion
///   address to (`onion`). Everything from a friend arrives there, and there
///   **only sealed envelopes (`wire`) are read**: what Tor delivers looks as if
///   it came from this machine, so it gets its own socket rather than a rule
///   about where it came from.
pub struct SignalListener {
    /// Each socket, and whether it reads sealed envelopes only.
    listeners: Vec<(TcpListener, bool)>,
    door: std::sync::Mutex<crate::kin::Door>,
    /// The state folder release files are handed out from
    /// (`update_courier::FILES` inside it), once set.
    releases: std::sync::Mutex<Option<std::path::PathBuf>>,
    /// This Atlas's key, to open envelopes sealed to it.
    me: std::sync::Mutex<Option<crate::peerkey::Identity>>,
    /// One-time keys already accepted.
    seen: std::sync::Mutex<crate::wire::Seen>,
    /// Connections through Tor that a friend's Atlas asked to keep open for
    /// its next request (gap AN): each with where it came from and when it
    /// was last used. Sealed door only, and bounded (`kin::MAX_KEPT`).
    kept: std::sync::Mutex<Vec<(TcpStream, std::net::SocketAddr, std::time::Instant)>>,
    /// Where to note which paired Atlas came to the door (`kin::Reached`).
    reached: std::sync::Mutex<Option<std::sync::Arc<crate::kin::Reached>>>,
}

/// What a door said, and what (if anything) goes on to the daemon.
type Answered = (Reply, Option<crate::kin::Arrived>);

impl SignalListener {


}

// The rest of this module, by what it does (audit Q6, 6 Oct 2026).
mod requests;
pub use requests::*;
mod routing;
pub use routing::*;
mod tokens;
pub use tokens::*;
mod serving;
mod hub_door;
pub use hub_door::*;
mod signal_door;

