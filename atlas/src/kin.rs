//! A door another Atlas can knock on, that only ever rings a bell.
//!
//! Built for one thing: a future Atlas running on its own server,
//! not linked to this one, that needs to say "this is urgent" without either
//! instance reaching into the other. "Not linked but able to communicate" is
//! a real tension — a shared network address is not the same thing as shared
//! trust, and the whole design here is making sure it never becomes that.
//!
//! Three rules, and the first is the one that makes the other two matter:
//!
//! 1. **A signal can become exactly one thing: a `Nudge`.** Not a command,
//!    not an approval, not a memory write, not a setting change. There is no
//!    function in this file that turns an `Incoming` into an `Intent` or an
//!    `Action`, and there must never be one. `tests/guards.rs` fails the
//!    build if that stops being true.
//! 2. **Trust is named, not inherited.** Being reachable on the same Tailscale
//!    network is not the same thing as being trusted. A peer has to be added
//!    once, deliberately, the way a person adds a contact — never
//!    trust-on-first-use, never "anything that knows the address."
//! 3. **A channel that is always right stops being checked.** The same
//!    discipline `nudge` already applies to your own stalled goals applies
//!    here: rate-limited per peer, and a peer that floods this loses standing
//!    the same way a nudge you keep ignoring does — see `standing()`.
//!
//! What this deliberately does not do: authenticate the *content* of a
//! message as true. Another Atlas saying "sell everything" is not a
//! command Atlas can act on through this door — it becomes a nudge you read
//! and decide about, exactly like everything else Atlas has ever proposed.

use serde::{Deserialize, Serialize};

/// Distinct from the hub's port (8787) on purpose -- two different doors,
/// two different trust levels, should never share a listener even if it
/// would be convenient to.
pub const DEFAULT_PORT: u16 = 8788;

/// A peer as it appears in config -- name and token, nothing else. Kept
/// separate from `Peer` even though the shapes match today, because config
/// deserialization and the runtime type are different concerns and should
/// not be forced to change together just because they happen to look alike.
#[derive(Debug, Clone, Deserialize)]
pub struct PeerConfig {
    pub name: String,
    pub token: String,
}

impl From<PeerConfig> for Peer {
    fn from(p: PeerConfig) -> Peer {
        Peer::new(&p.name, &p.token)
    }
}

/// Whether this door exists at all, and who is allowed through it.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct KinConfig {
    /// Off unless you have actually registered a peer. See the module doc
    /// on why "enabled with nobody registered" is not a safe middle state to
    /// default into.
    pub enabled: bool,
    pub port: u16,
    pub peers: Vec<PeerConfig>,
    /// What to call yourself in an invite you generate by voice. A spoken
    /// pairing can't ask you to spell your own name out loud each time, so
    /// this is set once, by hand, in config -- the same "config that's set
    /// wins" rule `keep_model_warm` follows. `atlas invite` on the command
    /// line still takes `--as` directly and ignores this.
    pub my_name: Option<String>,
    /// Your own Tailscale address, put into every invite you generate by
    /// voice. Same reasoning as `my_name`.
    pub my_host: Option<String>,
    /// Where the `tor` program is, when it isn't beside Atlas (`onion::find_tor`).
    pub tor: Option<String>,
    /// Extra lines for Atlas's Tor settings -- bridges, for a network that
    /// blocks Tor. Empty for almost everyone.
    pub tor_extra: Vec<String>,
}

impl Default for KinConfig {
    fn default() -> Self {
        // A struct-level Default deriving u16's own default would silently
        // give an enabled-but-unconfigured door port 0 -- which asks the OS
        // for a random port every restart, so a peer's saved address would
        // stop working the next time Atlas started. Named explicitly instead.
        KinConfig {
            enabled: false,
            port: DEFAULT_PORT,
            peers: Vec::new(),
            my_name: None,
            my_host: None,
            tor: None,
            tor_extra: Vec::new(),
        }
    }
}


/// Another Atlas instance you've decided to trust for notifications only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    /// What you call it. Shown in every nudge it causes, so you always know
    /// where something came from.
    pub name: String,
    /// Its own token — never the token your phone uses. A peer that had your
    /// phone's token wouldn't be a peer, it would be you.
    pub token: String,
    /// Its Atlas's public key (`peerkey`), learned from its own `/hello`
    /// over this pairing and then pinned: a different key later is refused
    /// and said, never quietly swapped. How a group's owner and members know
    /// that "Sam" is the same person on everyone's Atlas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl Peer {
    pub fn new(name: &str, token: &str) -> Peer {
        Peer { name: name.into(), token: token.into(), key: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Urgency {
    Urgent,
    Info,
}

/// A message that arrived, already matched to a known, trusted peer.
///
/// There is deliberately no constructor here that skips verification —
/// `receive()` is the only way to get one of these, and it is the only place
/// a peer's token is ever checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Incoming {
    pub from: String,
    pub urgency: Urgency,
    pub what: String,
    pub at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The token matches no registered peer. Not "wrong peer" — unregistered
    /// entirely, which is the default for everything until you add it.
    UnknownPeer,
    /// This peer has sent too many signals too recently.
    TooMany,
    /// Nothing worth turning into a nudge.
    Empty,
    /// A file over `MAX_HANDOFF_FILE_BYTES`.
    TooBig,
}

impl Refused {
    pub fn plain(&self) -> &'static str {
        match self {
            Refused::UnknownPeer => "that isn't a peer I recognise",
            Refused::TooMany => "that peer has sent too many signals recently",
            Refused::Empty => "there was nothing in the message",
            Refused::TooBig => {
                "that file is bigger than I take from a peer in one go — put it somewhere and \
                 send the link instead"
            }
        }
    }
}

/// No more than this many signals from one peer inside the window, before it
/// is refused rather than delivered. Protects against a misbehaving or
/// compromised peer turning "urgent" into noise.
pub const MAX_PER_WINDOW: u32 = 3;
pub const WINDOW_SECS: u64 = 3600;

/// The same protection for handed-over notes, counted separately.
///
/// Separately, not shared, and the reason matters: a signal interrupts you
/// and a note does not — it waits in a list until you look. Sharing one
/// bucket would mean a friend sending you three notes could not then reach
/// you with something urgent for an hour, which is the wrong failure. A
/// higher cap for the quieter thing, its own counter, same window.
pub const MAX_HANDOFFS_PER_WINDOW: u32 = 10;

/// The most chat messages one peer may deliver in a window, before the door
/// starts refusing them. Higher than notes because a conversation is made of
/// many small messages, bounded because a peer that has somehow gone haywire
/// must not be able to fill your disk one message at a time. Two a minute,
/// averaged over the hour, is far above a real back-and-forth and far below a
/// flood.
pub const MAX_CHATS_PER_WINDOW: u32 = 120;

/// The most read receipts one peer may deliver in a window. A receipt is even
/// smaller and even more frequent than a message — reading a backlog can
/// acknowledge many at once — and each carries only ids, so the cap is
/// generous. Its own counter for the same reason chat has one: a burst of
/// receipts must not throttle the conversation they are about, and a peer
/// gone haywire is still bounded.
pub const MAX_READS_PER_WINDOW: u32 = 600;

/// The most "left a group" notices one peer may send in a window. Leaving is
/// a rare event — you do not leave the same group twice — so this is low; its
/// own counter for the same reason the others have theirs.
pub const MAX_LEAVES_PER_WINDOW: u32 = 30;

/// The most introductions and group lists one peer may send in a window.
/// Both are rare -- an introduction once per pairing, a list once per change
/// to a group -- so this is low, on its own counter like the others.
pub const MAX_NOTICES_PER_WINDOW: u32 = 60;

/// A note a trusted peer handed over.
///
/// A separate type from `Incoming` on purpose, and the separation is
/// structural rather than tidiness. `Incoming` has exactly one thing it may
/// become — a `nudge` — and `tests/guards.rs` fails the build if a second
/// path ever appears. Content is a different shape of message and needs a
/// different destination, so it gets its own type with its own single
/// destination (`household::Inbox`) rather than being smuggled through the
/// one that is already spoken for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delivered {
    /// The peer's registered name — what *you* called them when you paired.
    /// Never a name claimed in the message body.
    pub from: String,
    /// The note, or the covering line for a file. Shown, never obeyed.
    pub what: String,
    pub at: u64,
    /// The file, when one came with it.
    #[serde(default)]
    pub file: Option<DeliveredFile>,
}

/// A file a trusted peer handed over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeliveredFile {
    /// The sender's name for it, already run through `tray::safe_name`.
    ///
    /// A filename from another machine is attacker-controlled text that is
    /// about to be used to make a path. Sanitised here, at the door, rather
    /// than at whichever call site eventually writes it — there is only one
    /// door and there will be more than one call site.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The largest file taken from a peer in one go.
///
/// Smaller than `tray::MAX_FILE_BYTES` (20MB), deliberately. Three reasons,
/// all of which are about this being a *peer* transfer rather than something
/// you handed Atlas yourself:
///
/// - base64 inflates by 4/3, so 8MB of file is ~10.7MB on the wire, and the
///   whole body is read into memory by a listener that services one
///   connection at a time. 20MB of file would be 27MB of body.
/// - it arrives unasked. Something you drop in your own tray is a decision
///   you just made; this is a decision someone else made about your disk.
/// - a cap is a sentence, not a wall: over it, the sender is told to put the
///   file somewhere and share the link, which is what you would do with
///   anything genuinely large anyway.
pub const MAX_HANDOFF_FILE_BYTES: usize = 8 * 1024 * 1024;

/// What came through the door.
#[derive(Debug, Clone, PartialEq)]
pub enum Arrived {
    /// A signal, which may only become a nudge.
    Signal(Incoming),
    /// A note, which may only wait in the inbox.
    Handoff(Delivered),
    /// A chat message, which may only be handed to `chat::Chats::receive` in
    /// a room the receiver opens for the sender the token names.
    Chat(Chatted),
    /// A read receipt: the peer the token names has read these messages of
    /// yours. Its only destination is `chat::Chats::mark_read` — never a
    /// room, a message, an intent, or the tray. It cannot carry a body, so it
    /// cannot become a message.
    Read(ReadReceipt),
    /// A notice that the peer the token names has left a group. Its only
    /// destination is dropping that peer from that group's membership — never
    /// a room opened, a message, an intent, or the tray.
    Left(LeftGroup),
    /// The peer the token names, introducing its Atlas's public key. Its only
    /// destination is pinning that key to that pairing.
    Hello(Hello),
    /// A signed group list, from the peer the token names. Its only
    /// destination is `groups::Groups::take`, which checks the owner's
    /// signature -- who carried it proves nothing about what it says.
    Group(GroupList),
    /// Someone used one of your friend links -- its one-time secret already
    /// spent at the door. Its only destination is recording them as a friend.
    Friend(Befriended),
    /// Feedback a friend chose to send you. Its only destination is
    /// `feedback::heard_feedback`, which files it -- never a room, an
    /// intent, or a command.
    Feedback(FeedbackIn),
    /// Your answer to feedback you sent. Its only destination is
    /// `feedback::heard_answer`, which takes it only about feedback this
    /// Atlas sent, and only from the person it went to.
    FeedbackAnswer(FeedbackIn),
}

/// Feedback, or an answer to it, as it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackIn {
    /// The peer the token names.
    pub from: String,
    /// The body, as JSON, size-capped.
    pub body: String,
    pub at: u64,
}

/// A friend who used one of your links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Befriended {
    pub hello: crate::friends::Hello,
    pub at: u64,
}

/// A peer introducing its Atlas's public key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    /// The peer the token names.
    pub from: String,
    pub key: String,
    pub at: u64,
}

/// A group's signed list, as it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupList {
    /// The peer the token names -- who handed it over, not who signed it.
    pub from: String,
    pub signed: crate::groups::Signed,
    pub at: u64,
}

/// A notice that a peer has left a group.
///
/// `from` is the peer the token names — who left — never anything in the body.
/// It carries only the group's shared id: no body, no member list to be
/// trusted, nothing that reopens a conversation. All it can do is take the
/// sender out of that group on this end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftGroup {
    /// The peer who left, by the name the token names.
    pub from: String,
    /// The group's shared id.
    pub group_id: String,
    /// When this machine learned of it.
    pub at: u64,
}

/// A read receipt as it arrived from another Atlas.
///
/// `from` is the peer the *token* names, never anything in the body — the same
/// rule every peer-facing type here follows. It carries message ids only: no
/// body, no clock of the sender's, nothing that could be mistaken for a
/// message. The `at` is *this* machine's clock when the receipt landed, used
/// as the "read at" time, because the honest thing to record is when we
/// learned it was read, not a timestamp the sender could have set to anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadReceipt {
    /// The peer's registered name — who read the messages.
    pub from: String,
    /// The sender's own ids of the messages that were read.
    pub ids: Vec<String>,
    /// When this machine learned of the read.
    pub at: u64,
}

/// A chat message as it arrived from another Atlas, with the one thing the
/// wire is never trusted for already resolved: `from` is the peer the *token*
/// names, not a name the body claimed. Everything else is the sender's own —
/// their clock, their offset, their ordering counter — kept exactly as sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Chatted {
    /// The peer's registered name — what you called them when you paired.
    pub from: String,
    /// The business this belongs to, if any. `None` is a personal message.
    /// A named business here is only a claim until the receiver's own roster
    /// agrees the sender may see it — that check lives in `chat`/`roster`,
    /// not at the door, because the door knows peers and the roster knows
    /// businesses.
    pub business: Option<String>,
    /// The message text. Shown, never obeyed.
    pub body: String,
    /// The sender's clock at the moment they wrote it. Never rewritten.
    pub sent_at: u64,
    /// The sender's UTC offset in minutes when they wrote it.
    pub sent_offset_mins: i16,
    /// What the sender had already seen — the ordering that survives two
    /// clocks disagreeing.
    pub after: u64,
    /// The sender's own id for the message, so a re-delivery after a failed
    /// attempt lands as the same message rather than a duplicate.
    pub id: String,
    /// For a group message, the group's shared id — the same on every member's
    /// Atlas, minted once by whoever started it. `None` is a one-to-one.
    pub group_id: Option<String>,
    /// The group's name, carried so a member hearing from the group for the
    /// first time can show it by name rather than by an id.
    pub group_name: Option<String>,
    /// The other members, in the sender's names for them. The receiver keeps
    /// the ones it is itself paired with (resolved by name), adds the sender
    /// from the token, and quietly drops the rest — including the entry that
    /// is the receiver seen through the sender's eyes, which never matches a
    /// pairing because you are not paired with yourself. Empty for a
    /// one-to-one.
    pub members: Vec<String>,
    /// For a message in a group with an owner, passed on by the owner's
    /// Atlas: the public key of the member who wrote it. Believed only when
    /// the peer the token names *is* that group's owner (`groups`) -- the
    /// owner decides who is in the group, so the owner vouching for who said
    /// what is the same trust. From anyone else it's ignored.
    pub on_behalf_of: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Door {
    pub peers: Vec<Peer>,
    /// Every accepted signal, kept long enough to rate-limit against. Not a
    /// full history — `sent recently` is all this needs to answer.
    recent: Vec<(String, u64)>,
    /// The same, for handed-over notes. See `MAX_HANDOFFS_PER_WINDOW` for
    /// why this is not the same list.
    recent_handoffs: Vec<(String, u64)>,
    /// The same, for chat messages. A separate budget again: a conversation
    /// is many short messages where a note is an occasional larger thing, so
    /// the note limit would throttle an ordinary back-and-forth to a stop.
    /// See `MAX_CHATS_PER_WINDOW`.
    recent_chats: Vec<(String, u64)>,
    /// The same, for read receipts. Its own budget again, and a bigger one:
    /// see `MAX_READS_PER_WINDOW`.
    recent_reads: Vec<(String, u64)>,
    /// The same, for "left a group" notices. See `MAX_LEAVES_PER_WINDOW`.
    recent_leaves: Vec<(String, u64)>,
    /// The same, for introductions and group lists. See
    /// `MAX_NOTICES_PER_WINDOW`.
    recent_notices: Vec<(String, u64)>,
    /// Every knock on the friend door, from anyone: it has no token to name
    /// a sender by, so one budget covers them all. See
    /// `MAX_FRIEND_KNOCKS_PER_WINDOW`.
    recent_friend_knocks: Vec<u64>,
    /// Pieces of release files handed out, per peer. See
    /// `MAX_RELEASE_PIECES_PER_WINDOW`.
    recent_pieces: Vec<(String, u64)>,
    /// Where your open friend links are kept (`friends::Invites`). `None`
    /// means you've made none on this door, and the friend door stays shut.
    friend_invites: Option<crate::store::Store>,
}

/// Knocks on the friend door per window, from everyone together. A real
/// friend knocks once; the secret is 128 random bits, so this isn't what
/// stops guessing -- it stops the door being used to keep Atlas busy.
pub const MAX_FRIEND_KNOCKS_PER_WINDOW: usize = 30;

/// Pieces of a release file one peer may fetch per window: about a gigabyte
/// an hour, far more than any release, far less than a flood.
pub const MAX_RELEASE_PIECES_PER_WINDOW: usize = 4000;

impl Door {
    pub fn new(peers: Vec<Peer>) -> Door {
        Door {
            peers,
            recent: Vec::new(),
            recent_handoffs: Vec::new(),
            recent_chats: Vec::new(),
            recent_reads: Vec::new(),
            recent_leaves: Vec::new(),
            recent_notices: Vec::new(),
            recent_friend_knocks: Vec::new(),
            recent_pieces: Vec::new(),
            friend_invites: None,
        }
    }

    /// A paired peer asking for a piece of a release file. Returns who, if
    /// they may: known by token, and within their budget.
    pub fn may_fetch(&mut self, token: &str, now: u64) -> Result<String, Refused> {
        let name = self.find(token).ok_or(Refused::UnknownPeer)?.name.clone();
        self.recent_pieces.retain(|(_, at)| now.saturating_sub(*at) < WINDOW_SECS);
        if self.recent_pieces.iter().filter(|(n, _)| *n == name).count() >= MAX_RELEASE_PIECES_PER_WINDOW {
            return Err(Refused::TooMany);
        }
        self.recent_pieces.push((name.clone(), now));
        Ok(name)
    }

    /// The key pinned for the peer this token names: `None` for no such peer,
    /// `Some(None)` for a peer not introduced yet.
    pub fn key_for_token(&self, token: &str) -> Option<Option<String>> {
        self.find(token).map(|p| p.key.clone())
    }

    /// Whose token this is, by the name they're paired as.
    pub fn name_for_token(&self, token: &str) -> Option<String> {
        self.find(token).map(|p| p.name.clone())
    }

    /// One knock from someone not yet a friend, within the budget all of
    /// them share.
    fn friend_knock_budget(&mut self, now: u64) -> Result<(), Refused> {
        self.recent_friend_knocks.retain(|at| now.saturating_sub(*at) < WINDOW_SECS);
        if self.recent_friend_knocks.len() >= MAX_FRIEND_KNOCKS_PER_WINDOW {
            return Err(Refused::TooMany);
        }
        self.recent_friend_knocks.push(now);
        Ok(())
    }

    /// Open the friend door, checking knocks against the links kept here.
    pub fn accept_friends(&mut self, invites: crate::store::Store) {
        self.friend_invites = Some(invites);
    }

    /// Someone used one of your friend links. The one door with no token:
    /// the link's one-time secret is the credential, spent here, before
    /// anything else happens. Returns the friend to record, or why not.
    pub fn receive_friend(&mut self, hello: crate::friends::Hello, now: u64) -> Result<Befriended, Refused> {
        let store = self.friend_invites.clone().ok_or(Refused::UnknownPeer)?;
        self.friend_knock_budget(now)?;
        if hello.name.trim().is_empty()
            || hello.name.chars().count() > 60
            || hello.routes.is_empty()
            || hello.token.len() < 16
            || !crate::peerkey::is_public_key(&hello.key)
        {
            return Err(Refused::Empty);
        }
        crate::friends::redeem(&store, &hello.invite, now).map_err(|_| Refused::UnknownPeer)?;
        // Let in by the daemon under the name it records them by, in the
        // same pass of the loop, before anything else can arrive.
        Ok(Befriended { hello, at: now })
    }

    fn find(&self, token: &str) -> Option<&Peer> {
        self.peers.iter().find(|p| crate::server::token_matches(&p.token, Some(token)))
    }

    /// Stop letting this peer in, now, on the door that is actually open.
    ///
    /// Returns whether one was there to remove.
    ///
    /// ## Why this has to exist
    ///
    /// `Door` is built once at startup from a cloned `Vec<Peer>` and had no
    /// way to remove one. `Intent::ForgetPeer` rewrote `kin_peers.yaml` and
    /// replied *"Forgotten. {name} can no longer reach you, and you can no
    /// longer reach them."* -- while the live door went on matching the
    /// forgotten token, so the next `/signal` or `/handoff` from that peer
    /// was accepted and delivered. The protection was stated and not
    /// implemented, and it stayed unimplemented until the next restart.
    pub fn forget(&mut self, name: &str) -> bool {
        let had = self.peers.iter().any(|p| same_name(&p.name, name));
        self.peers.retain(|p| !same_name(&p.name, name));
        // The rate-limit history goes with them. Keeping it would mean a
        // re-pairing inherited a stranger's count, and the name is the key.
        self.recent.retain(|(n, _)| !same_name(n, name));
        self.recent_handoffs.retain(|(n, _)| !same_name(n, name));
        self.recent_chats.retain(|(n, _)| !same_name(n, name));
        self.recent_reads.retain(|(n, _)| !same_name(n, name));
        self.recent_leaves.retain(|(n, _)| !same_name(n, name));
        self.recent_notices.retain(|(n, _)| !same_name(n, name));
        had
    }

    /// Let a peer in without a restart.
    ///
    /// The mirror of `forget`, and needed for the same reason: a pairing made
    /// by voice wrote the file and could not reach the live door, so the
    /// pairing completed on both ends and then did not work until Atlas was
    /// restarted -- with nothing saying so.
    pub fn admit(&mut self, peer: Peer) {
        self.peers.retain(|p| !same_name(&p.name, &peer.name));
        self.peers.push(peer);
    }

    /// Does this token belong to a paired peer?
    ///
    /// Says only that, and deliberately returns no `Peer`. The server needs
    /// it to decide whether a request may have its body read at all —
    /// authenticating before allocating, rather than allocating 28MB from an
    /// unauthenticated `Content-Length` and checking afterwards, which is
    /// what it used to do. What such a request may *do* is still decided
    /// solely by `receive` and `receive_handoff_with`, so this cannot widen
    /// a peer's reach.
    pub fn knows(&self, token: &str) -> bool {
        self.find(token).is_some()
    }

    /// How many times this peer has been let through inside the window.
    fn recent_count(&self, name: &str, now: u64) -> u32 {
        self.recent
            .iter()
            .filter(|(n, at)| n == name && now.saturating_sub(*at) < WINDOW_SECS)
            .count() as u32
    }

    /// The only way an `Incoming` comes into existence. Everything before
    /// this point — the network, the HTTP body — is untrusted; this is where
    /// it either becomes real or is refused.
    pub fn receive(
        &mut self,
        token: &str,
        what: &str,
        urgency: Urgency,
        now: u64,
    ) -> Result<Incoming, Refused> {
        let peer = self.find(token).ok_or(Refused::UnknownPeer)?.clone();
        if what.trim().is_empty() {
            return Err(Refused::Empty);
        }
        if self.recent_count(&peer.name, now) >= MAX_PER_WINDOW {
            return Err(Refused::TooMany);
        }
        self.recent.push((peer.name.clone(), now));
        // Bounded rather than left to grow for the life of the process.
        self.recent.retain(|(_, at)| now.saturating_sub(*at) < WINDOW_SECS);
        Ok(Incoming { from: peer.name, urgency, what: what.trim().to_string(), at: now })
    }

    /// Whether a peer is currently trusted with standing, i.e. not presently
    /// rate-limited. Informational — `receive` is what actually enforces it.
    pub fn standing(&self, name: &str, now: u64) -> bool {
        self.recent_count(name, now) < MAX_PER_WINDOW
    }

    /// The only way a `Delivered` comes into existence.
    ///
    /// Same shape as `receive`, same token check, same rate-limiting idea on
    /// its own counter — and deliberately a separate function rather than a
    /// flag on `receive`, so that neither can be turned into the other by
    /// getting one argument wrong at a call site.
    pub fn receive_handoff(
        &mut self,
        token: &str,
        what: &str,
        now: u64,
    ) -> Result<Delivered, Refused> {
        self.receive_handoff_with(token, what, None, now)
    }

    /// The same, with a file attached.
    ///
    /// Same token check, same counter — a file is not a second budget. The
    /// file's own limits are checked here rather than at the call site,
    /// because this is the only place a `DeliveredFile` can be made.
    pub fn receive_handoff_file(
        &mut self,
        token: &str,
        what: &str,
        name: &str,
        bytes: Vec<u8>,
        now: u64,
    ) -> Result<Delivered, Refused> {
        self.receive_handoff_with(token, what, Some((name, bytes)), now)
    }

    fn receive_handoff_with(
        &mut self,
        token: &str,
        what: &str,
        file: Option<(&str, Vec<u8>)>,
        now: u64,
    ) -> Result<Delivered, Refused> {
        let peer = self.find(token).ok_or(Refused::UnknownPeer)?.clone();
        // A file with a blank covering line is still a real handoff; a note
        // with nothing in it is not.
        if what.trim().is_empty() && file.is_none() {
            return Err(Refused::Empty);
        }
        let file = match file {
            None => None,
            Some((_, b)) if b.is_empty() => return Err(Refused::Empty),
            Some((_, b)) if b.len() > MAX_HANDOFF_FILE_BYTES => return Err(Refused::TooBig),
            Some((n, b)) => Some(DeliveredFile { name: crate::tray::safe_name(n), bytes: b }),
        };
        let count = self
            .recent_handoffs
            .iter()
            .filter(|(n, at)| *n == peer.name && now.saturating_sub(*at) < WINDOW_SECS)
            .count() as u32;
        if count >= MAX_HANDOFFS_PER_WINDOW {
            return Err(Refused::TooMany);
        }
        self.recent_handoffs.push((peer.name.clone(), now));
        self.recent_handoffs.retain(|(_, at)| now.saturating_sub(*at) < WINDOW_SECS);
        Ok(Delivered { from: peer.name, what: what.trim().to_string(), at: now, file })
    }

    /// Take in a chat message from a peer.
    ///
    /// The token names the sender; the body's own idea of who it is from is
    /// ignored, exactly as for a handoff — `from` is set from the pairing, not
    /// from anything on the wire. Rate-limited on its own budget so a
    /// conversation is not throttled by the note limit and a flood is still
    /// bounded. The `business` label is passed through untrusted: whether the
    /// sender may actually see that business is the *receiver's* roster's call,
    /// made when the message is filed (`chat::Chats::open`/`receive`), because
    /// the door knows peers and the roster knows businesses.
    #[allow(clippy::too_many_arguments)]
    pub fn receive_chat(
        &mut self,
        token: &str,
        business: Option<String>,
        body: &str,
        sent_at: u64,
        sent_offset_mins: i16,
        after: u64,
        id: &str,
        group_id: Option<String>,
        group_name: Option<String>,
        members: Vec<String>,
        now: u64,
    ) -> Result<Chatted, Refused> {
        let peer = self.find(token).ok_or(Refused::UnknownPeer)?.clone();
        if body.trim().is_empty() {
            return Err(Refused::Empty);
        }
        let count = self
            .recent_chats
            .iter()
            .filter(|(n, at)| *n == peer.name && now.saturating_sub(*at) < WINDOW_SECS)
            .count() as u32;
        if count >= MAX_CHATS_PER_WINDOW {
            return Err(Refused::TooMany);
        }
        self.recent_chats.push((peer.name.clone(), now));
        self.recent_chats
            .retain(|(_, at)| now.saturating_sub(*at) < WINDOW_SECS);
        Ok(Chatted {
            from: peer.name,
            business,
            body: body.trim().to_string(),
            sent_at,
            sent_offset_mins,
            after,
            id: id.to_string(),
            group_id,
            group_name,
            members,
            on_behalf_of: None,
        })
    }

    /// Take in a read receipt from a peer.
    ///
    /// The mirror of `receive_chat`, and a separate function for the same
    /// structural reason: this is the only place a `ReadReceipt` can be made,
    /// so a receipt can never be forged out of a message and a message can
    /// never be forged out of a receipt. `from` is the peer the token names,
    /// never anything on the wire. Its own rate-limit budget. An empty id
    /// list is not a receipt and is refused, so an idle peer cannot use this
    /// as a bare knock.
    pub fn receive_read(
        &mut self,
        token: &str,
        ids: Vec<String>,
        now: u64,
    ) -> Result<ReadReceipt, Refused> {
        let peer = self.find(token).ok_or(Refused::UnknownPeer)?.clone();
        let ids: Vec<String> = ids.into_iter().filter(|s| !s.trim().is_empty()).collect();
        if ids.is_empty() {
            return Err(Refused::Empty);
        }
        let count = self
            .recent_reads
            .iter()
            .filter(|(n, at)| *n == peer.name && now.saturating_sub(*at) < WINDOW_SECS)
            .count() as u32;
        if count >= MAX_READS_PER_WINDOW {
            return Err(Refused::TooMany);
        }
        self.recent_reads.push((peer.name.clone(), now));
        self.recent_reads
            .retain(|(_, at)| now.saturating_sub(*at) < WINDOW_SECS);
        Ok(ReadReceipt { from: peer.name, ids, at: now })
    }

    /// Take in a "left a group" notice from a peer.
    ///
    /// The mirror of `receive_read`, and a separate function for the same
    /// structural reason: this is the only place a `LeftGroup` can be made, so
    /// it can never be forged out of a message or a receipt, nor they out of
    /// it. `from` is the peer the token names — the one leaving — never
    /// anything on the wire. A notice with no group id is not a notice and is
    /// refused. Its own rate-limit budget.
    pub fn receive_left(
        &mut self,
        token: &str,
        group_id: &str,
        now: u64,
    ) -> Result<LeftGroup, Refused> {
        let peer = self.find(token).ok_or(Refused::UnknownPeer)?.clone();
        if group_id.trim().is_empty() {
            return Err(Refused::Empty);
        }
        let count = self
            .recent_leaves
            .iter()
            .filter(|(n, at)| *n == peer.name && now.saturating_sub(*at) < WINDOW_SECS)
            .count() as u32;
        if count >= MAX_LEAVES_PER_WINDOW {
            return Err(Refused::TooMany);
        }
        self.recent_leaves.push((peer.name.clone(), now));
        self.recent_leaves
            .retain(|(_, at)| now.saturating_sub(*at) < WINDOW_SECS);
        Ok(LeftGroup { from: peer.name, group_id: group_id.trim().to_string(), at: now })
    }
}

impl Door {
    fn notice_budget(&mut self, name: &str, now: u64) -> Result<(), Refused> {
        let count = self
            .recent_notices
            .iter()
            .filter(|(n, at)| n == name && now.saturating_sub(*at) < WINDOW_SECS)
            .count() as u32;
        if count >= MAX_NOTICES_PER_WINDOW {
            return Err(Refused::TooMany);
        }
        self.recent_notices.push((name.to_string(), now));
        self.recent_notices.retain(|(_, at)| now.saturating_sub(*at) < WINDOW_SECS);
        Ok(())
    }

    /// A peer introducing its key. The only place a `Hello` can be made; the
    /// name is the token's, never the body's. A body that isn't a real public
    /// key is refused here.
    pub fn receive_hello(&mut self, token: &str, key: &str, now: u64) -> Result<Hello, Refused> {
        let peer = self.find(token).ok_or(Refused::UnknownPeer)?.clone();
        if !crate::peerkey::is_public_key(key) {
            return Err(Refused::Empty);
        }
        self.notice_budget(&peer.name, now)?;
        Ok(Hello { from: peer.name, key: key.trim().to_string(), at: now })
    }

    /// Feedback, or an answer to feedback, from a paired Atlas. Size-capped
    /// here and counted against the same notice budget as a hello; what it
    /// says is read where it's filed.
    pub fn receive_feedback(&mut self, token: &str, body: &str, now: u64) -> Result<FeedbackIn, Refused> {
        let peer = self.find(token).ok_or(Refused::UnknownPeer)?.clone();
        if body.trim().is_empty() {
            return Err(Refused::Empty);
        }
        if body.len() > crate::feedback::MAX_FEEDBACK_BYTES {
            return Err(Refused::TooBig);
        }
        self.notice_budget(&peer.name, now)?;
        Ok(FeedbackIn { from: peer.name, body: body.to_string(), at: now })
    }

    /// A signed group list, handed over by a peer. The only place a
    /// `GroupList` can be made. Its size is capped here; its signature is
    /// checked where it is taken (`groups::open`).
    pub fn receive_group(&mut self, token: &str, state: &str, signature: &str, signer: &str, now: u64) -> Result<GroupList, Refused> {
        let peer = self.find(token).ok_or(Refused::UnknownPeer)?.clone();
        if state.trim().is_empty() || signature.trim().is_empty() {
            return Err(Refused::Empty);
        }
        if state.len() > crate::groups::MAX_STATE_BYTES {
            return Err(Refused::TooBig);
        }
        self.notice_budget(&peer.name, now)?;
        Ok(GroupList {
            from: peer.name,
            signed: crate::groups::Signed {
                state: state.to_string(),
                signature: signature.trim().to_string(),
                signer: signer.trim().to_string(),
            },
            at: now,
        })
    }
}

/// The one and only thing an `Incoming` may become.
///
/// No other function anywhere in this codebase may construct an `Intent`, an
/// `Action`, or a memory write from an `Incoming`. This function's existence
/// and this comment are the contract; `tests/guards.rs` holds the line.
pub fn as_nudge(i: &Incoming) -> crate::nudge::Nudge {
    crate::nudge::Nudge {
        trigger: crate::nudge::Trigger::CheckIn,
        subject: Some(format!("signal:{}", i.from)),
        message: format!("{} says: {}", i.from, i.what),
        // A signal carries no relief -- there is nothing Atlas can offer to
        // take off your hands about another Atlas's own situation.
        relief: None,
        asking_why: false,
        confidence: match i.urgency {
            Urgency::Urgent => 0.9,
            Urgency::Info => 0.6,
        },
    }
}

/// The one and only thing a `Delivered` may become.
///
/// The exact counterpart of `as_nudge`, and for the same reason. No other
/// function anywhere in this codebase may construct an `Intent`, an `Action`,
/// a tray item or a memory write from a `Delivered`: it goes into the
/// waiting list a person then decides about, and nowhere else. This
/// function's existence and this comment are the contract; `tests/guards.rs`
/// holds the line.
pub fn as_waiting(
    d: &Delivered,
    inbox: &mut crate::household::Inbox,
    root: &std::path::Path,
) -> Result<u64, String> {
    let Some(f) = &d.file else {
        return Ok(inbox.add(&d.what, &d.from, d.at));
    };

    // Written to the doorstep, not the tray, and nothing looks at it. The
    // name was sanitised at the door; the timestamp in front of it is what
    // keeps two friends' `notes.txt` from being the same file.
    let dir = root.join(crate::household::HANDOFF_FOLDER);
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't keep that file: {e}"))?;
    let stored = format!("{}-{}", d.at, f.name);
    std::fs::write(dir.join(&stored), &f.bytes)
        .map_err(|e| format!("couldn't keep that file: {e}"))?;

    Ok(inbox.add_with(
        &d.what,
        &d.from,
        d.at,
        Some(crate::household::ReceivedFile {
            name: f.name.clone(),
            size: f.bytes.len() as u64,
            stored_at: format!("{}/{stored}", crate::household::HANDOFF_FOLDER),
        }),
    ))
}

/// What a handoff says on the wire.
fn handoff_body(h: &crate::household::Handoff, file: Option<(&str, &[u8])>) -> String {
    match file {
        None => format!("{{\"what\":{},\"from\":{}}}", json_string(&h.what), json_string(&h.from)),
        Some((name, bytes)) => format!(
            "{{\"what\":{},\"from\":{},\"name\":{},\"data\":{}}}",
            json_string(&h.what),
            json_string(&h.from),
            json_string(name),
            json_string(&crate::b64::encode(bytes))
        ),
    }
}

/// A file to hand over, checked against the cap before it is encoded.
fn read_handoff_file(path: &std::path::Path) -> Result<(String, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    if bytes.is_empty() {
        return Err(format!("{} is empty", path.display()));
    }
    if bytes.len() > MAX_HANDOFF_FILE_BYTES {
        return Err(format!(
            "{} is {} MB — over the {} MB a peer takes in one go. Put it somewhere and send \
             the link instead.",
            path.display(),
            bytes.len() / (1024 * 1024),
            MAX_HANDOFF_FILE_BYTES / (1024 * 1024)
        ));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    Ok((name, bytes))
}

/// Minimal JSON string escaping, for the one body this module sends.
///
/// A note is free text — it will contain quotes and newlines, and pasting it
/// into a body unescaped would produce something the other end cannot parse,
/// or worse, something it parses as different fields.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Exposed for tests only — the escaper is otherwise private.
#[doc(hidden)]
pub fn json_string_for_test(s: &str) -> String {
    json_string(s)
}

// ---------------------------------------------------------------------------
// Pairing without hand-editing config.
//
// Registering a peer used to mean opening tools.yaml and typing a token in
// by hand — exactly the kind of thing a friend who has never touched Atlas
// should not have to do. This is the whole exchange reduced to two commands:
//
//   You:    atlas invite "Friend" --as "Eric" --host your-tailscale-name
//           -> generates a token, remembers Friend can now reach you, and
//              prints one block of text to send them however you like.
//
//   Friend: atlas accept "<the block you sent>"
//           -> registers you as a sender, records how to reach you back,
//              and prints its own block in reply.
//
//   You:    atlas accept "<the block they sent back>"
//           -> now you can reach them too. Done.
//
// Nobody edits a file. Nobody pastes a token into the wrong field. The
// command's own output is the instructions, so there is nothing to explain
// twice.
// ---------------------------------------------------------------------------

pub const INVITE_TAG: &str = "ATLAS-KIN-1";

/// Someone you can now send signals to, once you have their address.
///
/// Separate from `Peer` on purpose: `Peer` is "who may send to me" and only
/// needs a name and a token. `Contact` is "who I may send to" and needs
/// somewhere to send it — the two lists answer different questions and a
/// pairing fills in one, then the other, as the exchange completes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Invite {
    /// What the other side should call you.
    pub from_name: String,
    pub host: String,
    pub port: u16,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InviteError {
    /// Doesn't start with the tag -- almost always means it was cut off in
    /// whatever it was pasted into, or it's just not an invite at all.
    NotAnInvite,
    /// The tag is right but the pieces after it don't add up.
    Malformed,
}

impl InviteError {
    pub fn plain(&self) -> &'static str {
        match self {
            InviteError::NotAnInvite => {
                "that doesn't look like an Atlas invite -- check you copied the whole thing"
            }
            InviteError::Malformed => "that invite looks cut off partway through",
        }
    }
}

/// Characters that would break the encoding if they showed up in a name or
/// host. Rejected at the point Atlas builds an invite, not discovered later
/// by whoever tries to decode a broken one.
const RESERVED: char = '|';

fn clean(s: &str) -> Option<&str> {
    (!s.contains(RESERVED) && !s.trim().is_empty()).then_some(s)
}

/// Turn an invite into the one block of text you actually send someone.
pub fn encode_invite(i: &Invite) -> Option<String> {
    let name = clean(&i.from_name)?;
    let host = clean(&i.host)?;
    Some(format!("{INVITE_TAG}:{name}|{host}|{}|{}", i.port, i.token))
}

/// The other half. Whitespace around the whole block is trimmed, since
/// pasting into a text message or an email routinely adds some.
pub fn decode_invite(code: &str) -> Result<Invite, InviteError> {
    let code = code.trim();
    let rest = code.strip_prefix(&format!("{INVITE_TAG}:")).ok_or(InviteError::NotAnInvite)?;
    let parts: Vec<&str> = rest.split('|').collect();
    let [name, host, port, token] = parts[..] else { return Err(InviteError::Malformed) };
    let port: u16 = port.parse().map_err(|_| InviteError::Malformed)?;
    if name.trim().is_empty() || host.trim().is_empty() || token.trim().is_empty() {
        return Err(InviteError::Malformed);
    }
    Ok(Invite { from_name: name.to_string(), host: host.to_string(), port, token: token.to_string() })
}

/// Everything a pairing needs on one side: who may send to you, and who you
/// may send to. Atlas's own file -- see `save`'s header -- never something a
/// person is expected to hand-edit, unlike `tools.yaml`'s `kin.peers`, which
/// stays as the manual-override path for anyone who wants it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pairings {
    pub peers: Vec<Peer>,
    pub contacts: Vec<Contact>,
    /// Contacts you've told Atlas it may send to without being asked each
    /// time. Names only — trust is a property of the pairing, so `forget`
    /// clears it: a peer you forget and later re-pair starts untrusted rather
    /// than silently keeping a trust you granted the old relationship.
    /// `#[serde(default)]` so a `kin_peers.yaml` written before this field
    /// existed loads with nobody trusted, which is the safe default anyway.
    #[serde(default)]
    pub trusted: Vec<String>,
    /// Where each paired Atlas said it can be reached (`Routes`), by
    /// lower-cased name. Kept beside the contact rather than in it, so the
    /// address a pairing was made with still works as the last thing tried.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub routes: std::collections::BTreeMap<String, Routes>,
}

/// Whether two names refer to the same person for pairing purposes.
///
/// Case-insensitive on purpose: a name spoken by voice and the same name
/// typed at the CLI will not reliably match case (speech-to-text
/// capitalization isn't consistent), and "Sarah" failing to find a pairing
/// made under "sarah" is exactly the kind of silent mismatch a person has
/// no way to debug from the outside -- it just looks like Atlas forgot.
/// ASCII-only comparison to match the rest of this file's ASCII-lowercase
/// handling; not a general Unicode case-folding claim.
/// `pub(crate)`, not private: `roster.rs` needs the identical comparison --
/// a roster entry for "Sarah" must recognise the same peer as "sarah" does
/// here, or the two files would silently disagree about who someone is.
pub(crate) fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Where pairings live, said once.
///
/// ## The answer was in two places and one of them was wrong
///
/// `main.rs`'s startup -- which binds the listener and therefore decides who
/// can actually reach Atlas -- loaded from `roots::state_dir()`. The daemon's
/// `Intent::Pair`, `Intent::AcceptPairing` and `Intent::ForgetPeer`, and two
/// CLI paths, loaded from `store.root()`, which `roots::store()` resolves to
/// `profiles::active_dir()` -- **a different directory whenever there are two
/// or more profiles and one is active.**
///
/// What that cost, with profiles in use:
///
/// * "forget the pairing with Sam" loaded an empty `Pairings` from the profile
///   directory, `forget` returned false, and Atlas said *"I don't have a
///   pairing with Sam"* -- while Sam remained a fully trusted, live peer.
/// * A pairing made by voice wrote the new peer into the profile directory, so
///   startup never loaded it. The pairing completed on both ends and never
///   connected, with nothing saying why.
///
/// That second one is the identical failure `main.rs`'s own comment says was
/// fixed for the listening port: *"One function, because the answer was
/// written in three places and two of them were wrong."* This is that
/// function for this file.
///
/// `state_dir`, not the profile's, because a pairing is a trust relationship
/// of the **install**: the listener is bound once at startup from the
/// install's state, so a peer recorded per-profile could never be served
/// whichever profile was active.
pub fn where_pairings_live() -> std::path::PathBuf {
    crate::roots::state_dir()
}

impl Pairings {
    pub fn load(dir: &std::path::Path) -> Pairings {
        std::fs::read_to_string(dir.join("kin_peers.yaml"))
            .ok()
            .and_then(|t| serde_yaml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let header = "# Written by `atlas invite` / `atlas accept`. Atlas manages this file --\n\
                      # edits here are safe but will be alongside, not merged into, whatever\n\
                      # you add under kin.peers in tools.yaml.\n";
        let body = serde_yaml::to_string(self).unwrap_or_default();
        std::fs::write(dir.join("kin_peers.yaml"), format!("{header}{body}"))
    }

    pub(crate) fn add_peer(&mut self, p: Peer) {
        self.peers.retain(|x| !same_name(&x.name, &p.name));
        self.peers.push(p);
    }

    pub(crate) fn add_contact(&mut self, c: Contact) {
        self.contacts.retain(|x| !same_name(&x.name, &c.name));
        self.contacts.push(c);
    }

    pub fn has_peer(&self, name: &str) -> bool {
        self.peers.iter().any(|p| same_name(&p.name, name))
    }

    /// Undo a pairing, both directions at once. A pairing is one trust
    /// relationship even though it is stored as two lists -- removing only
    /// the peer half would leave them unable to reach you while you can
    /// still reach them, which is not "unpaired", it's asymmetric and worse
    /// than either whole state. Returns whether there was anything to
    /// remove, so a caller can tell "gone" from "was never there".
    pub fn forget(&mut self, name: &str) -> bool {
        let had = self.peers.iter().any(|p| same_name(&p.name, name))
            || self.contacts.iter().any(|c| same_name(&c.name, name));
        self.peers.retain(|p| !same_name(&p.name, name));
        self.contacts.retain(|c| !same_name(&c.name, name));
        // Trust is a property of the pairing, so it goes with it. Cleared
        // unconditionally rather than only when `had`, so a trust left over
        // from a half-removed pairing cannot outlive the peer it was for.
        self.trusted.retain(|n| !same_name(n, name));
        self.routes.remove(&name.to_lowercase());
        had
    }

    /// Tell Atlas it may send to this contact without asking each time.
    /// Idempotent and case-insensitive on the name, like the rest of this
    /// file. Trusting a name before the pairing exists is harmless — it takes
    /// effect the moment the contact does — so this does not require the
    /// contact to be present, which also means the CLI can trust and pair in
    /// either order.
    pub fn trust(&mut self, name: &str) {
        if !self.is_trusted(name) {
            self.trusted.push(name.to_string());
        }
    }

    /// Stop auto-sending to a contact. Returns whether they were trusted, so a
    /// caller can tell "stopped" from "was never trusted" and say the true one.
    pub fn distrust(&mut self, name: &str) -> bool {
        let had = self.is_trusted(name);
        self.trusted.retain(|n| !same_name(n, name));
        self.routes.remove(&name.to_lowercase());
        had
    }

    /// Whether a share to this contact may go without a prompt.
    pub fn is_trusted(&self, name: &str) -> bool {
        self.trusted.iter().any(|n| same_name(n, name))
    }

    /// The trusted names, for listing them back.
    pub fn trusted_names(&self) -> &[String] {
        &self.trusted
    }

    /// The public key pinned for this peer, if it has introduced itself.
    pub fn key_of(&self, name: &str) -> Option<&str> {
        self.peers.iter().find(|p| same_name(&p.name, name)).and_then(|p| p.key.as_deref())
    }

    /// Which of your peers has this key, by your name for them.
    pub fn name_of_key(&self, key: &str) -> Option<String> {
        self.peers.iter().find(|p| p.key.as_deref() == Some(key)).map(|p| p.name.clone())
    }

    /// Pin a peer's key the first time it introduces itself. `Ok(true)` when
    /// newly pinned, `Ok(false)` when it's the key already pinned, `Err` when
    /// it's a *different* key -- refused, never swapped: a new key under an
    /// old pairing is either a reinstall or somebody else, and only you can
    /// tell which (forget the pairing and pair again).
    pub fn pin_key(&mut self, name: &str, key: &str) -> Result<bool, String> {
        let Some(p) = self.peers.iter_mut().find(|p| same_name(&p.name, name)) else {
            return Err(format!("there's no pairing with {name}"));
        };
        match &p.key {
            None => {
                p.key = Some(key.to_string());
                Ok(true)
            }
            Some(k) if k == key => Ok(false),
            Some(_) => Err(format!(
                "{name}'s Atlas introduced itself with a different key from before. If they \
                 reinstalled, forget the pairing and pair again; if they didn't, someone else \
                 has their pairing."
            )),
        }
    }
}

/// Start a pairing. Registers the other side as a peer immediately -- they
/// can send to you the moment they receive this block, before they have even
/// run `accept` -- and returns the block to send them.
pub fn invite(pairings: &mut Pairings, their_name: &str, my_name: &str, my_host: &str, port: u16, token: &str) -> Option<String> {
    pairings.add_peer(Peer::new(their_name, token));
    encode_invite(&Invite { from_name: my_name.to_string(), host: my_host.to_string(), port, token: token.to_string() })
}

/// What accepting a code does, and whether a return block is owed.
#[derive(Debug, Clone, PartialEq)]
pub struct Accepted {
    pub from: String,
    /// Set when this name was not already a peer -- meaning the other side
    /// does not yet have a way to reach you back, and this is what closes
    /// that. `None` when you already had them, so nothing further is owed.
    pub return_block: Option<String>,
}

/// The other half of a pairing. Always registers the sender as both a peer
/// (they may send to you) and a contact (you may send to them, using the
/// same token -- one shared secret for both directions, one thing to leak
/// rather than two). Only offers a return block the first time.
pub fn accept(pairings: &mut Pairings, code: &str, my_name: &str, my_host: &str, my_port: u16) -> Result<Accepted, InviteError> {
    let inv = decode_invite(code)?;
    let already_known = pairings.has_peer(&inv.from_name);
    pairings.add_peer(Peer::new(&inv.from_name, &inv.token));
    pairings.add_contact(Contact { name: inv.from_name.clone(), host: inv.host, port: inv.port, token: inv.token.clone() });
    let return_block = if already_known {
        None
    } else {
        encode_invite(&Invite {
            from_name: my_name.to_string(),
            host: my_host.to_string(),
            port: my_port,
            token: inv.token,
        })
    };
    Ok(Accepted { from: inv.from_name, return_block })
}

// ===========================================================================
// The real chat transport
//
// `courier.rs` owns the *when* of delivery — hold, retry, back off, give up —
// and must never learn an address. This is the *how*: it hands a chat message
// to a peer's Atlas over the same peer channel `PeerLink::hand_note` uses, and the
// only thing it reports back is whether they took it. It owns a snapshot of
// contacts and room metadata so it borrows nothing while the courier mutates
// `Chats`.
// ===========================================================================

/// A room's identity, firewall side, and membership, snapshotted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomMeta {
    pub id: String,
    /// The business this room belongs to, or `None` for personal.
    pub business: Option<String>,
    /// What the room is called — carried on the wire for a group so a member
    /// hearing from it for the first time can name it.
    pub name: String,
    /// The other members. More than one makes this a group, which is the only
    /// thing that changes what goes on the wire: a group message carries the
    /// shared id, the name, and this list; a one-to-one carries none of them.
    pub members: Vec<String>,
}

impl RoomMeta {
    fn is_group(&self) -> bool {
        // A group with an owner is a group even with one other person in it:
        // its id has to travel so the other end files it under the owner's list.
        self.members.len() > 1 || crate::groups::is_owned_id(&self.id)
    }
}

/// Delivers chat messages to peers over the peer channel.
pub struct PeerLink {
    contacts: Vec<Contact>,
    rooms: Vec<RoomMeta>,
    timeout: std::time::Duration,
    /// Each paired Atlas's pinned key, by lower-cased name.
    keys: std::collections::BTreeMap<String, String>,
    /// Where each can be reached, by lower-cased name (`Pairings::routes`).
    routes: std::collections::BTreeMap<String, Routes>,
    /// This Atlas's key, to seal with. Without it nothing leaves this
    /// machine's own networks.
    me: Option<std::sync::Arc<crate::peerkey::Identity>>,
    /// Tor's SOCKS port, once Atlas's own `tor` is ready (`onion`).
    tor: Option<u16>,
    /// Connections to friends through Tor, kept open between requests.
    kept: Option<std::sync::Arc<TorConnections>>,
    /// Where to note who answered.
    reached: Option<std::sync::Arc<Reached>>,
}

/// How a request to another Atlas went.
#[derive(Debug, Clone, PartialEq)]
pub enum Sent {
    /// It answered (sealed answers already opened).
    Answered(crate::http::Response),
    /// Nothing answered: kept by the caller and tried again later.
    Failed(String),
}

impl Sent {
    /// As the older `post_json_with_token` answer.
    fn response(self) -> Result<crate::http::Response, String> {
        match self {
            Sent::Answered(r) => Ok(r),
            Sent::Failed(e) => Err(e),
        }
    }
}

/// Where a paired Atlas can be reached, as its friend link said: its onion
/// address (reachable from anywhere it's online, through Tor), and its
/// address on its home network (for a friend on the same wifi, who then
/// needn't go through Tor).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routes {
    #[serde(default)]
    pub addrs: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub onion: String,
}

impl Routes {
    /// Only what could be real.
    pub fn checked(mut self) -> Routes {
        self.addrs.retain(|a| crate::onion::read_addr(a).is_some());
        self.addrs.truncate(4);
        if !crate::onion::is_onion(&self.onion) {
            self.onion.clear();
        }
        self
    }
    pub fn is_empty(&self) -> bool {
        self.addrs.is_empty() && self.onion.is_empty()
    }
}

/// Where a request goes: straight to an address on your own networks, or to
/// an onion address through Atlas's own Tor.
#[derive(Debug, Clone, Copy)]
pub enum Via<'a> {
    Direct(&'a str),
    Tor { socks: u16, onion: &'a str },
}

/// When each paired Atlas was last heard from -- it answered something this
/// Atlas sent, or came to its door with its token -- by the name it's paired
/// as. What "online" means on the Partners page: heard from lately, not merely
/// paired (26 Sep's gap: every partner showed only "paired"). Kept in memory
/// and in the store under `REACHED`.
#[derive(Default)]
pub struct Reached {
    last: std::sync::Mutex<std::collections::BTreeMap<String, u64>>,
}

/// Where `Reached` is kept, in the state folder.
pub const REACHED: &str = "peers_reached";

/// Heard from within this long counts as online.
pub const ONLINE_SECS: u64 = 15 * 60;

impl Reached {
    pub fn note(&self, name: &str, at: u64) {
        if let Ok(mut m) = self.last.lock() {
            let e = m.entry(name.to_lowercase()).or_insert(0);
            *e = (*e).max(at);
        }
    }

    /// Last heard from, if ever.
    pub fn last(&self, name: &str) -> Option<u64> {
        self.last.lock().ok()?.get(&name.to_lowercase()).copied()
    }

    pub fn load(store: &crate::store::Store) -> Reached {
        let m: std::collections::BTreeMap<String, u64> = store.load(REACHED);
        Reached { last: std::sync::Mutex::new(m) }
    }

    pub fn save(&self, store: &crate::store::Store) {
        if let Ok(m) = self.last.lock() {
            let _ = store.save(REACHED, &*m);
        }
    }
}

/// Connections to friends' onion addresses, kept open between requests
/// (gap AN, OPEN_GAPS 8.7).
///
/// Reaching an onion address means Tor building a meeting point in the
/// middle of its network -- seconds, every time. Kept open, the second
/// message to the same friend goes straight down the connection that's
/// already there, and a release file's pieces follow one another instead of
/// each paying for a new one. Each request is still sealed and checked on
/// its own (`wire`); only the road is reused.
///
/// Bounded: at most `MAX_KEPT` friends, and a connection idle for
/// `KEPT_IDLE_SECS` is let go (the far side lets go at about the same time,
/// and a connection it has closed is noticed and replaced).
#[derive(Default)]
pub struct TorConnections {
    open: std::sync::Mutex<std::collections::HashMap<String, (std::net::TcpStream, std::time::Instant)>>,
}

pub const MAX_KEPT: usize = 16;
pub const KEPT_IDLE_SECS: u64 = 240;

impl TorConnections {
    fn take(&self, onion: &str) -> Option<std::net::TcpStream> {
        let mut open = self.open.lock().ok()?;
        let (s, at) = open.remove(onion)?;
        (at.elapsed().as_secs() < KEPT_IDLE_SECS).then_some(s)
    }

    fn keep(&self, onion: &str, s: std::net::TcpStream) {
        if let Ok(mut open) = self.open.lock() {
            open.retain(|_, (_, at)| at.elapsed().as_secs() < KEPT_IDLE_SECS);
            if open.len() < MAX_KEPT || open.contains_key(onion) {
                open.insert(onion.to_string(), (s, std::time::Instant::now()));
            }
        }
    }

    /// How many are open right now.
    pub fn open_now_for_test(&self) -> usize {
        self.open.lock().map(|o| o.len()).unwrap_or(0)
    }

    /// Let every one go: Tor was restarted, and they all died with it.
    pub fn close_all(&self) {
        if let Ok(mut open) = self.open.lock() {
            open.clear();
        }
    }
}

/// `sealed_post` through Tor on a kept-open connection when there is one:
/// the one already open to `onion` if it's still there, else a new one, which
/// is kept for next time if the far side agrees. A kept connection the far
/// side had closed is replaced once, with the request sealed afresh.
#[allow(clippy::too_many_arguments)]
fn sealed_post_kept(
    me: &crate::peerkey::Identity,
    to: &str,
    socks: u16,
    onion: &str,
    kept: &TorConnections,
    path: &str,
    token: &str,
    body: &str,
    timeout: std::time::Duration,
) -> Result<crate::http::Response, String> {
    use crate::http::KeptReply;
    let timeout = timeout.max(std::time::Duration::from_secs(30));
    let mut reused = kept.take(onion);
    for _ in 0..2 {
        let inner = crate::wire::Inner { path: path.into(), token: token.into(), body: body.into(), at: crate::store::now() };
        let (env, key) = crate::wire::seal(me, to, &inner).ok_or("couldn't seal it")?;
        let env = serde_json::to_string(&env).unwrap_or_default();
        let was_kept = reused.is_some();
        let mut stream = match reused.take() {
            Some(s) => s,
            None => crate::onion::connect(socks, onion, std::time::Duration::from_secs(crate::onion::CONNECT_SECS))
                .map_err(|e| e.to_string())?,
        };
        match crate::http::post_kept(&mut stream, onion, crate::wire::PATH, &env, timeout).map_err(|e| e.to_string())? {
            KeptReply::Closed if was_kept => continue,
            KeptReply::Closed => return Err("their Atlas closed the connection without answering".into()),
            KeptReply::Answered(r, again) => {
                if again {
                    kept.keep(onion, stream);
                }
                if !r.ok() {
                    return Ok(r);
                }
                let reply = crate::wire::open_reply(&key, &r.body).ok_or("the answer wasn't sealed by who it should have been")?;
                return Ok(crate::http::Response { status: reply.status, body: reply.body, location: None });
            }
        }
    }
    Err("their Atlas closed the connection without answering".into())
}

/// Seal a request to the Atlas with key `to` and send it one way.
pub fn sealed_post(
    me: &crate::peerkey::Identity,
    to: &str,
    via: Via,
    path: &str,
    token: &str,
    body: &str,
    timeout: std::time::Duration,
) -> Result<crate::http::Response, String> {
    let inner = crate::wire::Inner { path: path.into(), token: token.into(), body: body.into(), at: crate::store::now() };
    let (env, key) = crate::wire::seal(me, to, &inner).ok_or("couldn't seal it")?;
    let env = serde_json::to_string(&env).unwrap_or_default();
    let r = match via {
        Via::Direct(addr) => crate::http::post_json(addr, crate::wire::PATH, &env, timeout).map_err(|e| e.to_string())?,
        Via::Tor { socks, onion } => {
            let mut stream = crate::onion::connect(socks, onion, std::time::Duration::from_secs(crate::onion::CONNECT_SECS))
                .map_err(|e| e.to_string())?;
            crate::http::post_over(&mut stream, onion, crate::wire::PATH, &env, timeout.max(std::time::Duration::from_secs(30)))
                .map_err(|e| e.to_string())?
        }
    };
    if !r.ok() {
        return Ok(r);
    }
    let reply = crate::wire::open_reply(&key, &r.body).ok_or("the answer wasn't sealed by who it should have been")?;
    Ok(crate::http::Response { status: reply.status, body: reply.body, location: None })
}

impl PeerLink {
    /// Build from the pairings and chats as they are right now. The daemon
    /// makes one of these each tick, just before running the courier, so a
    /// peer paired or a room opened a moment ago is already reachable.
    pub fn from_state(pairings: &Pairings, chats: &crate::chat::Chats) -> PeerLink {
        let rooms = chats
            .rooms
            .iter()
            .map(|r| RoomMeta {
                id: r.id.clone(),
                business: match &r.space {
                    crate::earned::Space::Business(b) => Some(b.clone()),
                    _ => None,
                },
                name: r.name.clone(),
                members: r.members.clone(),
            })
            .collect();
        PeerLink {
            contacts: pairings.contacts.clone(),
            rooms,
            timeout: std::time::Duration::from_secs(10),
            keys: pairings
                .peers
                .iter()
                .filter_map(|p| p.key.clone().map(|k| (p.name.to_lowercase(), k)))
                .collect(),
            routes: pairings.routes.clone(),
            me: None,
            tor: None,
            kept: None,
            reached: None,
        }
    }

    /// Note in `reached` whoever answers.
    pub fn noting(mut self, reached: std::sync::Arc<Reached>) -> PeerLink {
        self.reached = Some(reached);
        self
    }

    /// Keep connections to friends through Tor open between requests, in `kept`.
    pub fn keeping(mut self, kept: std::sync::Arc<TorConnections>) -> PeerLink {
        self.kept = Some(kept);
        self
    }

    /// How long one request may take -- longer for a file.
    pub fn waiting(mut self, timeout: std::time::Duration) -> PeerLink {
        self.timeout = timeout;
        self
    }

    /// Reach onion addresses through the Tor at this SOCKS port.
    pub fn through_tor(mut self, socks: Option<u16>) -> PeerLink {
        self.tor = socks;
        self
    }

    /// Seal everything with this Atlas's key. Without it, a request only ever
    /// goes to an address on your own networks, in the clear.
    pub fn sealing_as(mut self, me: Option<crate::peerkey::Identity>) -> PeerLink {
        self.me = me.map(std::sync::Arc::new);
        self
    }

    fn contact(&self, peer: &str) -> Option<&Contact> {
        self.contacts.iter().find(|c| same_name(&c.name, peer))
    }

    /// Everywhere this contact can be tried, best first.
    fn addresses(&self, c: &Contact) -> Vec<String> {
        let mut out = self.routes.get(&c.name.to_lowercase()).map(|r| r.addrs.clone()).unwrap_or_default();
        if c.host.is_empty() || c.port == 0 {
            return out;
        }
        let paired_at = if c.host.contains(':') && !c.host.starts_with('[') {
            format!("[{}]:{}", c.host, c.port)
        } else {
            format!("{}:{}", c.host, c.port)
        };
        if !out.contains(&paired_at) {
            out.push(paired_at);
        }
        out
    }

    /// Is this address on one of your own networks (this machine, home, or
    /// your own private network)? Only there may anything go in the clear.
    fn own_network(addr: &str) -> bool {
        use std::net::ToSocketAddrs;
        addr.to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .is_some_and(|a| crate::onion::is_local_origin(a.ip()))
    }

    /// Send one request to a paired Atlas: to its address on your own
    /// networks if it has one (same wifi, or your own private network), then
    /// to its onion address through Tor -- sealed whenever its key is known,
    /// and never in the clear to anywhere but your own networks. When nothing
    /// answers, the caller keeps it and tries again: nobody else holds it.
    pub fn post(&self, peer: &str, path: &str, body: &str) -> Sent {
        let sent = self.post_once(peer, path, body);
        if let (Sent::Answered(_), Some(r), Some(c)) = (&sent, &self.reached, self.contact(peer)) {
            r.note(&c.name, crate::store::now());
        }
        sent
    }

    fn post_once(&self, peer: &str, path: &str, body: &str) -> Sent {
        let Some(c) = self.contact(peer) else { return Sent::Failed(format!("there's no pairing with {peer}")) };
        let key = self.keys.get(&c.name.to_lowercase());
        let mut why = String::from("nothing answered");
        for addr in self.addresses(c) {
            let got = match (&self.me, key) {
                (Some(me), Some(k)) => sealed_post(me, k, Via::Direct(&addr), path, &c.token, body, self.timeout),
                _ if Self::own_network(&addr) => {
                    crate::http::post_json_with_token(&addr, path, body, &c.token, self.timeout).map_err(|e| e.to_string())
                }
                _ => Err("not sealed, and not on your own network".into()),
            };
            match got {
                Ok(r) => return Sent::Answered(r),
                Err(e) => why = e,
            }
        }
        let onion = self.routes.get(&c.name.to_lowercase()).map(|r| r.onion.clone()).unwrap_or_default();
        match (&self.me, key, self.tor, onion.is_empty()) {
            (Some(me), Some(k), Some(socks), false) => {
                let sent = match &self.kept {
                    Some(kept) => sealed_post_kept(me, k, socks, &onion, kept, path, &c.token, body, self.timeout),
                    None => sealed_post(me, k, Via::Tor { socks, onion: &onion }, path, &c.token, body, self.timeout),
                };
                match sent {
                    Ok(r) => Sent::Answered(r),
                    Err(e) => Sent::Failed(e),
                }
            }
            (_, _, None, false) => Sent::Failed(format!("{why}; and Tor isn't running here yet to reach them")),
            _ => Sent::Failed(why),
        }
    }

    /// Hand a note (and maybe a file) to a paired Atlas, the same way as
    /// everything else here.
    pub fn hand_note(&self, peer: &str, h: &crate::household::Handoff, file: Option<&std::path::Path>) -> Result<(), String> {
        if h.carries_history {
            return Err("that handoff claims to carry history, which never leaves this machine".into());
        }
        let file = match file {
            None => None,
            Some(p) => Some(read_handoff_file(p)?),
        };
        let body = handoff_body(h, file.as_ref().map(|(n, b)| (n.as_str(), b.as_slice())));
        match self.post(peer, "/handoff", &body) {
            Sent::Answered(r) if r.ok() => Ok(()),
            Sent::Answered(r) => Err(format!(
                "{peer} said no ({}). If you have not paired in a while, their Atlas may have forgotten this end.",
                r.status
            )),
            // A sentence for the page it's shown on, not the network's own
            // words ("Connection refused (os error 111)") after a lowercase
            // start (27 Sep 2026).
            Sent::Failed(e) => Err(format!("I couldn't reach {peer}'s Atlas, so nothing was sent. {}", unreached_why(&e))),
        }
    }

    /// Which of your paired Atlases this key belongs to.
    pub fn name_of_key(&self, key: &str) -> Option<String> {
        let lower = self.keys.iter().find(|(_, v)| *v == key).map(|(n, _)| n.clone())?;
        self.contact(&lower).map(|c| c.name.clone())
    }

    /// Which snapshotted room a message belongs to, matched by the exact id
    /// the sender built (`"{room_id}-{sent_at}-{after}"`). The room decides
    /// the firewall side the message goes out under, and that is not safely
    /// derivable from the message alone, so it is looked up rather than
    /// guessed.
    fn room_for(&self, msg: &crate::chat::Message) -> Option<&RoomMeta> {
        self.rooms
            .iter()
            .find(|r| msg.id == format!("{}-{}-{}", r.id, msg.sent_at, msg.after))
    }
}

/// Why another Atlas wasn't reached, in a sentence: the likely cause and
/// what to do, never the raw network error.
fn unreached_why(e: &str) -> &'static str {
    let e = e.to_lowercase();
    if e.contains("tor isn't running") {
        "Atlas's own Tor isn't connected yet; try again in a minute."
    } else if e.contains("timed out") || e.contains("would block") {
        "Their Atlas didn't answer in time; it may be busy or on a slow connection. Try again later."
    } else if e.contains("refused") || e.contains("reset") || e.contains("nothing answered") || e.contains("unreachable") {
        "Their Atlas may be switched off or offline; try again once it's on."
    } else if e.contains("no pairing") {
        "You aren't paired with them any more."
    } else {
        "Their Atlas may be switched off or offline; try again later."
    }
}

/// The wire form of a chat message.
///
/// `from` is deliberately absent: the receiver takes the sender from the token
/// that carried the request, never from a name in the body. Everything else is
/// the sender's own and travels unchanged. A group message also carries the
/// shared group id, its name, and the sender's member list, so the receiver
/// can reconstruct the same conversation; a one-to-one carries none of that.
pub fn chat_wire_body(room: &RoomMeta, msg: &crate::chat::Message) -> String {
    let biz = match &room.business {
        Some(b) => json_string(b),
        None => "null".to_string(),
    };
    let group = if room.is_group() {
        let members: Vec<String> = room.members.iter().map(|m| json_string(m)).collect();
        format!(
            ",\"group_id\":{},\"group_name\":{},\"members\":[{}]",
            json_string(&room.id),
            json_string(&room.name),
            members.join(",")
        )
    } else {
        String::new()
    };
    format!(
        "{{\"business\":{biz},\"body\":{},\"sent_at\":{},\"offset\":{},\"after\":{},\"id\":{}{group}}}",
        json_string(&msg.body),
        msg.sent_at,
        msg.sent_offset_mins,
        msg.after,
        json_string(&msg.id)
    )
}

/// The wire form of a read receipt: message ids and nothing else.
///
/// No `from` — the receiver takes who read it from the token, exactly as for a
/// message. No body, no clock: a receipt is not a message and its wire form is
/// built so it cannot be mistaken for one. Private: the only thing that builds
/// a receipt for the wire is `PeerLink`'s own `Receipts` impl, just below.
fn read_wire_body(ids: &[String]) -> String {
    let ids: Vec<String> = ids.iter().map(|i| json_string(i)).collect();
    format!("{{\"ids\":[{}]}}", ids.join(","))
}

impl crate::courier::Receipts for PeerLink {
    fn read(&self, peer: &str, ids: &[String]) -> bool {
        let Some(contact) = self.contact(peer) else {
            return false;
        };
        let body = read_wire_body(ids);
        matches!(
            self.post(&contact.name, "/read", &body).response(),
            Ok(r) if r.ok()
        )
    }
}

impl PeerLink {
    /// Introduce this Atlas's public key to one paired Atlas. Returns whether
    /// it took it.
    pub fn say_hello(&self, peer: &str, key: &str) -> bool {
        let Some(contact) = self.contact(peer) else { return false };
        let body = format!("{{\"key\":{}}}", json_string(key));
        matches!(
            self.post(&contact.name, "/hello", &body).response(),
            Ok(r) if r.ok()
        )
    }

    /// Pass a member's message on to another member, as the group's owner --
    /// so people in a group who aren't paired with each other still hear each
    /// other. The author travels as their key (`on_behalf_of`); the far side
    /// believes it only because this Atlas owns the group.
    #[allow(clippy::too_many_arguments)]
    pub fn relay(
        &self,
        peer: &str,
        group_id: &str,
        group_name: &str,
        author_key: &str,
        id: &str,
        body: &str,
        sent_at: u64,
        offset: i16,
        after: u64,
    ) -> bool {
        let Some(contact) = self.contact(peer) else { return false };
        let wire = format!(
            "{{\"business\":null,\"body\":{},\"sent_at\":{sent_at},\"offset\":{offset},\"after\":{after},\"id\":{},\"group_id\":{},\"group_name\":{},\"members\":[],\"on_behalf_of\":{}}}",
            json_string(body),
            json_string(id),
            json_string(group_id),
            json_string(group_name),
            json_string(author_key)
        );
        matches!(
            self.post(&contact.name, "/chat", &wire).response(),
            Ok(r) if r.ok()
        )
    }

    /// Ask one paired Atlas for a piece of a release file it's handing out:
    /// the bytes from `offset` and the whole size. `None` if it isn't
    /// reachable or won't.
    pub fn fetch_release(&self, peer: &str, sha: &str, offset: u64) -> Option<(Vec<u8>, u64)> {
        let body = format!("{{\"sha256\":{},\"offset\":{offset}}}", json_string(sha));
        let r = self.post(peer, "/release", &body).response().ok()?;
        if !r.ok() {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(&r.body).ok()?;
        let data = crate::b64::decode(v.get("data")?.as_str()?).ok()?;
        Some((data, v.get("total")?.as_u64()?))
    }

    /// Hand one paired Atlas a piece of feedback (`path` "/feedback") or an
    /// answer to one ("/feedback-answer"). Returns whether it took it.
    pub fn send_feedback(&self, peer: &str, path: &str, body: &str) -> bool {
        let Some(contact) = self.contact(peer) else { return false };
        let wire = format!("{{\"body\":{}}}", json_string(body));
        matches!(
            self.post(&contact.name, path, &wire).response(),
            Ok(r) if r.ok()
        )
    }

    /// Hand one paired Atlas a group's signed list. Returns whether it took it.
    pub fn push_group(&self, peer: &str, signed: &crate::groups::Signed) -> bool {
        let Some(contact) = self.contact(peer) else { return false };
        let body = format!(
            "{{\"state\":{},\"signature\":{},\"signer\":{}}}",
            json_string(&signed.state),
            json_string(&signed.signature),
            json_string(&signed.signer)
        );
        matches!(
            self.post(&contact.name, "/group", &body).response(),
            Ok(r) if r.ok()
        )
    }

    /// Tell one member that you've left a group. Best-effort, one attempt: a
    /// member who is offline will not hear it now, and will learn you are gone
    /// the next time they try to reach the group and cannot, rather than being
    /// held up by your departure. Returns whether they took it.
    pub fn tell_left(&self, peer: &str, group_id: &str) -> bool {
        let Some(contact) = self.contact(peer) else {
            return false;
        };
        let body = format!("{{\"group_id\":{}}}", json_string(group_id));
        matches!(
            self.post(&contact.name, "/left", &body).response(),
            Ok(r) if r.ok()
        )
    }
}

impl crate::courier::Transport for PeerLink {
    fn hand_over(
        &self,
        device: &crate::courier::Device,
        msg: &crate::chat::Message,
    ) -> crate::courier::Handoff {
        use crate::courier::Handoff;
        let Some(contact) = self.contact(&device.peer) else {
            return Handoff::NotUp;
        };
        let Some(room) = self.room_for(msg) else {
            // A message whose room isn't in the snapshot can't be classified
            // onto a firewall side, so it is not sent — held, not guessed.
            return Handoff::NotUp;
        };
        let body = chat_wire_body(room, msg);
        match self.post(&contact.name, "/chat", &body).response()
        {
            Ok(r) if r.ok() => Handoff::Took,
            // Offline, refused, or a bad reply: all "not up" rather than
            // "took". The only thing that counts as delivered is a 2xx, the
            // receiver saying it has it.
            _ => Handoff::NotUp,
        }
    }

    fn devices_for(&self, peer: &str) -> Vec<crate::courier::Device> {
        match self.contact(peer) {
            Some(c) => vec![crate::courier::Device {
                peer: peer.to_string(),
                kind: crate::courier::Kind::Computer,
                address: format!("{}:{}", c.host, c.port),
            }],
            None => Vec::new(),
        }
    }
}
