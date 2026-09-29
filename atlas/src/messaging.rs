//! The places people actually message you.
//!
//! Email is where formal things arrive and chat is where everything else
//! does — which means an assistant that only reads email misses the message
//! from the brand and catches the newsletter.
//!
//! The three platforms differ enormously in what they permit, and it's worth
//! being blunt about that rather than promising the same for all three.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Telegram,
    GroupMe,
    WhatsApp,
    Signal,
    Discord,
    Slack,
}

/// How well Atlas can reach it, and honestly why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reach {
    /// A proper open interface. Read and send.
    Full,
    /// Read and send, but only in places a bot has been added.
    BotOnly,
    /// Read only.
    ReadOnly,
    /// Nothing official for a personal account.
    NotReally,
}

impl Platform {
    pub fn name(&self) -> &'static str {
        match self {
            Platform::Telegram => "Telegram",
            Platform::GroupMe => "GroupMe",
            Platform::WhatsApp => "WhatsApp",
            Platform::Signal => "Signal",
            Platform::Discord => "Discord",
            Platform::Slack => "Slack",
        }
    }

    pub fn reach(&self) -> Reach {
        match self {
            // A documented client API as well as the bot one. The best of the
            // three by a distance.
            Platform::Telegram => Reach::Full,
            // A small, free, public API. Unfashionable and completely usable.
            Platform::GroupMe => Reach::Full,
            // The Business API is for businesses, costs per message, and
            // needs a number that isn't already on WhatsApp. There is no
            // personal-account interface.
            Platform::WhatsApp => Reach::NotReally,
            Platform::Signal => Reach::NotReally,
            Platform::Discord => Reach::BotOnly,
            Platform::Slack => Reach::BotOnly,
        }
    }

    /// What this platform actually permits, said plainly.
    ///
    /// Named `what_it_permits` rather than `honest`: `mesh::Mesh` has an
    /// `honest` too, and the deadness scans read bare names -- calling this
    /// one made that one look as though something reached it, and
    /// `mesh::honest` is on `dead_methods.rs`'s list precisely because
    /// nothing does. Same trap as `install::total_mb` and `Where::honest`
    /// before it; third time, so the rename happens at the call rather than
    /// after the guard catches it.
    pub fn what_it_permits(&self) -> &'static str {
        match self {
            Platform::Telegram => {
                "the easiest of the lot — a documented interface, free, and it works for your \
                 own account rather than only for bots"
            }
            Platform::GroupMe => {
                "small, free, public interface. Nobody talks about it and it does everything \
                 needed"
            }
            Platform::WhatsApp => {
                "there is no way to do this for a personal account. The Business interface costs \
                 money per message and needs a separate number. Anything claiming otherwise is \
                 driving the desktop app or the web client by pretending to be you, which gets \
                 accounts banned — I won't do that"
            }
            Platform::Signal => {
                "deliberately closed, and that's the point of Signal. Nothing to connect to"
            }
            Platform::Discord => "a bot in servers you add it to. Not your private messages",
            Platform::Slack => "a bot in workspaces you add it to",
        }
    }

    /// What it takes to connect.
    pub fn to_connect(&self) -> &'static str {
        match self {
            Platform::Telegram => "a bot token from BotFather, which takes two minutes",
            Platform::GroupMe => "an access token from their developer page",
            Platform::WhatsApp => "nothing that exists",
            Platform::Signal => "nothing that exists",
            Platform::Discord => "a bot token and adding it to a server",
            Platform::Slack => "an app in the workspace",
        }
    }

    pub fn worth_setting_up(&self) -> bool {
        self.reach() <= Reach::BotOnly && self.reach() != Reach::NotReally
    }

    /// The platform you wrote in `messaging.platforms`.
    ///
    /// The setting is a list of strings and had no reader at all, so a person
    /// who wrote `platforms: [whatsapp]` got exactly the same behaviour as one
    /// who wrote `[telegram]` — which is to say none, silently, on the one
    /// list where two of the six entries can never work.
    ///
    /// Unknown names come back `None` rather than being guessed at. A typo
    /// answered with the nearest match is how you end up connected to the
    /// wrong thing, and this list is short enough to read.
    pub fn named(s: &str) -> Option<Platform> {
        match s.trim().to_lowercase().replace([' ', '-', '_'], "").as_str() {
            "telegram" => Some(Platform::Telegram),
            "groupme" => Some(Platform::GroupMe),
            "whatsapp" => Some(Platform::WhatsApp),
            "signal" => Some(Platform::Signal),
            "discord" => Some(Platform::Discord),
            "slack" => Some(Platform::Slack),
            _ => None,
        }
    }
}

/// What the platforms you asked for would actually give you.
///
/// This is the answer `messaging.platforms` was written for and never got.
/// Nothing in this tree reads messages -- `imap.rs` reads mail and there is
/// no messaging reader -- so the useful thing Atlas can say about the list is
/// not a count of an inbox it cannot see. It is which of the platforms you
/// named could ever work, what each one needs from you, and which are closed
/// no matter what you do.
///
/// Ordered by what you can act on: the ones worth setting up first, then the
/// dead ends, then anything Atlas did not recognise. A list that buries
/// "Telegram takes two minutes" under two paragraphs about WhatsApp is a list
/// that gets skimmed.
pub fn what_you_asked_for(platforms: &[String]) -> String {
    if platforms.is_empty() {
        return "You haven't named any platforms in `messaging.platforms`, so there's \
                nothing for me to set up."
            .into();
    }

    let mut unknown: Vec<&str> = Vec::new();
    let mut known: Vec<Platform> = Vec::new();
    for p in platforms {
        match Platform::named(p) {
            Some(k) if !known.contains(&k) => known.push(k),
            Some(_) => {}
            None => unknown.push(p.as_str()),
        }
    }

    let mut lines = Vec::new();
    for p in known.iter().filter(|p| p.worth_setting_up()) {
        lines.push(format!("{}: {}. Needs {}.", p.name(), p.what_it_permits(), p.to_connect()));
    }
    for p in known.iter().filter(|p| !p.worth_setting_up()) {
        // No "needs" line for these: saying a thing needs "nothing that
        // exists" reads as an instruction you failed to follow.
        lines.push(format!("{}: {}.", p.name(), p.what_it_permits()));
    }
    for u in &unknown {
        lines.push(format!(
            "{u}: I don't know that one. I can do Telegram, GroupMe, Discord and Slack."
        ));
    }

    // Said plainly at the end rather than implied by the absence of a count,
    // because the whole failure this replaces was a count of an inbox
    // nothing had read.
    lines.push(
        "None of them are connected yet — nothing in this build reads messages, so I \
         won't tell you how many are waiting."
            .into(),
    );
    lines.join(" ")
}

/// A message, whichever platform it came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub platform: Platform,
    /// Who sent it, as you'd know them.
    pub from: String,
    /// The group, if it was one.
    pub group: Option<String>,
    pub text: String,
    pub at: u64,
    /// You were named directly.
    pub mentions_you: bool,
}

/// What a message is, which decides whether it interrupts you.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// A group chat carrying on. Not for you.
    Chatter,
    /// Worth reading, no reply needed.
    Reading,
    /// Someone's waiting on you.
    NeedsReply,
    /// Business — a brand, a client, work.
    Business,
    /// Time-critical.
    Now,
}

impl Sort {
    /// What it is, in the words a person would use.
    ///
    /// Added because `atlas telegram` printed `{:?}` on this and
    /// `tests/hub_is_not_code.rs` caught it: a Rust variant name arriving as
    /// English. "NeedsReply" is a type, not a sentence, and "Now" on its own
    /// beside a message reads as a timestamp.
    pub fn plain(&self) -> &'static str {
        match self {
            Sort::Chatter => "group chatter",
            Sort::Reading => "worth reading",
            Sort::NeedsReply => "waiting on you",
            Sort::Business => "work",
            Sort::Now => "time-critical",
        }
    }
}

const BUSINESS: &[&str] = &[
    "collaboration", "partnership", "sponsor", "sponsored", "brand", "campaign",
    "rate card", "invoice", "contract", "deliverable", "usage rights", "brief",
    "budget", "paid partnership", "our client", "we'd love to work",
];

const URGENT: &[&str] = &[
    "asap", "urgent", "today", "right now", "emergency", "before ", "deadline",
];

/// Sort one, the same way the inbox is sorted: by what it asks of you.
///
/// The distinction that matters here is a message from a brand versus a
/// message in a group chat, and the words are genuinely different.
pub fn sort(m: &Message, your_names: &[String]) -> Sort {
    let t = m.text.to_lowercase();
    let named = m.mentions_you
        || your_names.iter().any(|n| t.contains(&n.to_lowercase()));

    if BUSINESS.iter().any(|b| t.contains(b)) {
        return Sort::Business;
    }
    if URGENT.iter().any(|u| t.contains(u)) && (named || m.group.is_none()) {
        return Sort::Now;
    }
    // A direct message is for you by definition; a group message is only for
    // you if it says so.
    if m.group.is_none() || named {
        if t.contains('?') || t.contains("can you") || t.contains("let me know") {
            return Sort::NeedsReply;
        }
        return Sort::Reading;
    }
    Sort::Chatter
}

/// What reaches you, and what waits.
pub fn interrupts(s: Sort) -> bool {
    matches!(s, Sort::Now | Sort::Business)
}

/// Where a conversation belongs.
///
/// Apps let you make folders and then leave the sorting to you, which is why
/// everyone has three folders and four hundred unsorted chats. Atlas puts the
/// conversation somewhere and writes a note about the person, so a name you
/// half-remember is findable six months later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Folder {
    /// Someone you're doing business with.
    Work,
    /// Someone who might become that.
    Prospect,
    /// Friends and family.
    Personal,
    /// A group you're in.
    Group,
    /// A service, a shop, a bot.
    NotAPerson,
    /// Can't tell yet.
    Unsorted,
}

impl Folder {
    pub fn name(&self) -> &'static str {
        match self {
            Folder::Work => "Work",
            Folder::Prospect => "Might be work",
            Folder::Personal => "Personal",
            Folder::Group => "Groups",
            Folder::NotAPerson => "Services",
            Folder::Unsorted => "Unsorted",
        }
    }
}

const PERSONAL_WORDS: &[&str] = &[
    "mate", "bro", "love you", "mum", "dad", "dinner", "pub", "weekend",
    "birthday", "how are you", "miss you", "haha", "lol", "😂",
];

const AUTOMATED: &[&str] = &[
    "verification code", "do not reply", "your order", "delivery", "otp",
    "unsubscribe", "notification", "bot",
];

/// Where a conversation goes, from what's been said in it.
///
/// One message decides nothing — the folder is a property of the
/// conversation, and a single "hi" tells you nothing about anyone.
pub fn folder_for(messages: &[Message], your_names: &[String]) -> Folder {
    if messages.is_empty() {
        return Folder::Unsorted;
    }
    if messages.iter().any(|m| m.group.is_some()) {
        return Folder::Group;
    }
    let all: String = messages
        .iter()
        .map(|m| m.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");

    if AUTOMATED.iter().any(|w| all.contains(w)) {
        return Folder::NotAPerson;
    }
    let business = messages
        .iter()
        .filter(|m| sort(m, your_names) == Sort::Business)
        .count();
    if business > 0 {
        // One business message is an approach; a conversation of them is a
        // working relationship, and those want different folders.
        return if business > 1 || messages.len() > 4 {
            Folder::Work
        } else {
            Folder::Prospect
        };
    }
    if PERSONAL_WORDS.iter().any(|w| all.contains(w)) {
        return Folder::Personal;
    }
    Folder::Unsorted
}

/// Where the notes Atlas keeps on the people who message you are stored.
///
/// A contact book that grows from real messages rather than being typed in
/// by hand: every time the daemon reads what came in, it updates the note on
/// each sender and leaves it here.
pub const PEOPLE: &str = "messaging_people";

/// A note about someone, so you can find them again.
///
/// Written from what they actually said, never invented — a note that
/// paraphrases wrongly is worse than none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Person {
    pub name: String,
    pub platform: Platform,
    pub folder: Folder,
    /// What they first wanted, in their words, trimmed.
    pub first_about: String,
    pub first_at: u64,
    pub last_at: u64,
    pub messages: u32,
    /// Which client or project they belong to.
    pub about: Option<String>,
}

pub fn note_on(messages: &[Message], your_names: &[String]) -> Option<Person> {
    let first = messages.first()?;
    let opening: String = first.text.chars().take(120).collect();
    Some(Person {
        name: first.from.clone(),
        platform: first.platform,
        folder: folder_for(messages, your_names),
        first_about: opening,
        first_at: first.at,
        last_at: messages.last().map(|m| m.at).unwrap_or(first.at),
        messages: messages.len() as u32,
        about: None,
    })
}

/// What Atlas says when it files someone.
///
/// Once, and only when it's confident enough to be useful. "I've filed
/// someone as unsorted" is not worth a sentence.
pub fn filed(p: &Person) -> Option<String> {
    match p.folder {
        Folder::Unsorted | Folder::Group => None,
        // Eric, 25 Sep 2026 (F3): not "X looks like work", but what they
        // want and an offer to deal with it.
        Folder::Work | Folder::Prospect => Some(format!(
            "It looks like {} wants to know about {}. Would you like me to look for the answer and respond?",
            p.name,
            p.first_about.trim()
        )),
        _ => None,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MessagingConfig {
    pub enabled: bool,
    /// Which to connect.
    pub platforms: Vec<String>,
    // Settable again, the same afternoon it stopped being. The reason it was
    // pinned was that **nothing in this build read messages**, so a list of
    // the names you go by had no message to be found in -- a setting whose
    // only effect was to make you believe something was watching.
    //
    // `telegram.rs` reads messages now. `sort` compares a group message
    // against this list to decide whether it was addressed to you, and that
    // is the whole of what tells a message meant for you from a group
    // carrying on. The reader is what the promise was waiting for, and it
    // arrived, so the promise becomes a setting.
    //
    /// Names you go by, so a group message that's for you is recognised.
    pub your_names: Vec<String>,
    /// Group chatter never interrupts. Not configurable — it's the whole
    /// reason this is bearable.
    #[serde(skip, default = "never")]
    pub chatter_interrupts: bool,
    /// Atlas never sends without you seeing it first.
    pub draft_only: bool,
}

fn never() -> bool {
    false
}

impl Default for MessagingConfig {
    fn default() -> Self {
        MessagingConfig {
            enabled: false,
            platforms: vec!["telegram".into()],
            your_names: Vec::new(),
            chatter_interrupts: false,
            draft_only: true,
        }
    }
}

/// What Atlas says about the messages.
pub fn spoken(messages: &[Message], names: &[String]) -> String {
    let sorted: Vec<(Sort, &Message)> = messages.iter().map(|m| (sort(m, names), m)).collect();
    let business: Vec<&&Message> = sorted
        .iter()
        .filter(|(s, _)| *s == Sort::Business)
        .map(|(_, m)| m)
        .collect();
    let replies = sorted.iter().filter(|(s, _)| *s == Sort::NeedsReply).count();
    let chatter = sorted.iter().filter(|(s, _)| *s == Sort::Chatter).count();

    if business.is_empty() && replies == 0 {
        return format!("{chatter} messages, all group chat.");
    }
    let mut s = String::new();
    if let Some(b) = business.first() {
        s.push_str(&format!("Something from {} looks like work.", b.from));
    }
    if replies > 0 {
        s.push_str(&format!(" {replies} people waiting on a reply."));
    }
    if chatter > 0 {
        s.push_str(&format!(" {chatter} group messages I've left alone."));
    }
    s
}

/// Why WhatsApp isn't on the list, said once.
pub const WHY_NOT_WHATSAPP: &str =
    "There's no way to reach a personal WhatsApp account. The Business interface is for \
     businesses — it costs per message and needs a number not already on WhatsApp. The tools \
     that claim to do it drive the desktop app while pretending to be you, and that gets \
     accounts banned. Telegram does everything you'd want from it and takes two minutes.";
