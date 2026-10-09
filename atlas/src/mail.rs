//! Getting at your email, whoever provides it.
//!
//! Gmail, Outlook, Yahoo, Fastmail, a work server — all of them speak IMAP,
//! which means one implementation covers everything rather than four
//! integrations that each break separately.
//!
//! Categorising happens **in your mailbox**, not in Atlas. A label applied
//! here shows up on your phone, and if you stop using Atlas tomorrow the
//! organisation stays. Anything that only exists inside this program is
//! organisation you lose.
//!
//! Nothing here deletes. Moving to a folder is reversible; deleting isn't, and
//! a wrongly-categorised message you can find again is a nuisance where a
//! wrongly-deleted one is a problem.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Gmail,
    Outlook,
    Yahoo,
    Fastmail,
    /// Any other IMAP server.
    Other,
}

impl Provider {
    /// Work it out from the address, so setup asks for less.
    pub fn from_address(addr: &str) -> Provider {
        let a = addr.to_lowercase();
        let domain = a.rsplit('@').next().unwrap_or("");
        match domain {
            "gmail.com" | "googlemail.com" => Provider::Gmail,
            "outlook.com" | "hotmail.com" | "live.com" | "msn.com" => Provider::Outlook,
            "yahoo.com" | "ymail.com" | "yahoo.co.uk" => Provider::Yahoo,
            "fastmail.com" | "fastmail.fm" => Provider::Fastmail,
            _ => Provider::Other,
        }
    }

    /// What to call it out loud.
    ///
    /// Exists because `atlas mail` printed `{:?}` and `tests/hub_is_not_code.rs`
    /// caught it: a Rust variant name arriving as English is the exact failure
    /// that guard was written for, and "Other" tells a person nothing about
    /// why their mailbox needs a host typed in.
    pub fn plain(&self) -> &'static str {
        match self {
            Provider::Gmail => "Gmail",
            Provider::Outlook => "Outlook",
            Provider::Yahoo => "Yahoo",
            Provider::Fastmail => "Fastmail",
            Provider::Other => "an IMAP server I don't recognise",
        }
    }

    pub fn imap_host(&self) -> Option<&'static str> {
        Some(match self {
            Provider::Gmail => "imap.gmail.com",
            Provider::Outlook => "outlook.office365.com",
            Provider::Yahoo => "imap.mail.yahoo.com",
            Provider::Fastmail => "imap.fastmail.com",
            Provider::Other => return None,
        })
    }

    pub fn port(&self) -> u16 {
        993
    }

    /// Where the same account sends from, over implicit TLS. Kept
    /// alongside `imap_host` since they're the same fact about the same
    /// provider — one read, one write address for the one account.
    pub fn smtp_host(&self) -> Option<&'static str> {
        Some(match self {
            Provider::Gmail => "smtp.gmail.com",
            Provider::Outlook => "smtp.office365.com",
            Provider::Yahoo => "smtp.mail.yahoo.com",
            Provider::Fastmail => "smtp.fastmail.com",
            Provider::Other => return None,
        })
    }

    /// Implicit TLS (465) everywhere it's offered. Outlook / Microsoft 365
    /// only serves 587 with STARTTLS (`smtp::connect` upgrades on 587): with
    /// 465 here every Outlook send failed at connect (5 Oct 2026 audit, Q1).
    pub fn smtp_port(&self) -> u16 {
        match self {
            Provider::Outlook => 587,
            _ => 465,
        }
    }

    /// Gmail has labels, which are not folders — a message can carry several,
    /// and moving it doesn't remove it from the inbox the way it does
    /// elsewhere. Getting this wrong makes mail disappear.
    pub fn has_labels(&self) -> bool {
        *self == Provider::Gmail
    }

    /// What the person actually needs to do to let Atlas in.
    ///
    /// None of these is Atlas holding your password: an app password is
    /// revocable and scoped, which is the point, and Outlook's OAuth never
    /// shows Atlas a password at all.
    pub fn how_to_connect(&self) -> &'static str {
        match self {
            Provider::Gmail =>
                "Gmail needs an app password — turn on 2-step verification, then create one \
                 under App passwords. It's revocable and only works for mail.",
            // Not an app password: Microsoft removed every password-based
            // way in. This said otherwise, and `atlas mail setup` (right)
            // disagreed with it — found when this finally got a caller.
            Provider::Outlook =>
                "Outlook and Microsoft 365 need OAuth — Microsoft removed every password-based way \
                 in. You register an app once in the Azure portal and I sign in through it; I \
                 never hold your password.",
            Provider::Yahoo =>
                "Yahoo needs an app password from Account Security. The main password won't work.",
            Provider::Fastmail =>
                "Fastmail needs an app password with mail access, from Settings then Privacy \
                 & Security.",
            Provider::Other =>
                "I'll need the IMAP server address and an app password if your provider has them.",
        }
    }
}

/// One mailbox Atlas has access to. Several of these, so a business
/// address can be added later without redesigning the config — a
/// personal inbox and a business one are the same shape of thing, just
/// two entries.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    /// A short name you'd call it by — "personal", "business". Read back
    /// whenever Atlas has to say which inbox, so it should read naturally
    /// spoken aloud.
    pub name: String,
    pub address: String,
    /// Only when the provider isn't recognised from the address.
    pub imap_host: String,
    /// The name of the vault entry holding *this account's own*
    /// credential — an app password for most providers, or a refresh
    /// token when `oauth` is set. Each account gets its own entry —
    /// sharing one between two mailboxes would mean losing access to one
    /// silently changes the other.
    pub password_from_vault: String,
    /// Outlook/Microsoft 365 only, and only because Microsoft removed
    /// every password-based path entirely — there's no "still works for
    /// now" option to default to. When set, `password_from_vault` holds
    /// a refresh token, not a password, and `client_id` names the Azure
    /// app registration `msoauth`'s device flow authenticated against.
    pub oauth: bool,
    pub client_id: String,
    /// With `backend: himalaya`: the account's name in Himalaya's own
    /// config. Empty: the same as `name`.
    pub himalaya_account: String,
}

impl Account {

    /// The IMAP server to read this account from: the one set on it, or the
    /// provider's own. `Err` says why there isn't one (one place since audit
    /// Q3; the mail check and the clear-out each had a copy).
    pub fn imap_server(&self) -> Result<String, String> {
        if !self.imap_host.is_empty() {
            return Ok(self.imap_host.clone());
        }
        Provider::from_address(&self.address)
            .imap_host()
            .map(str::to_string)
            .ok_or_else(|| format!("{}: unrecognised provider, needs imap_host set explicitly", self.name))
    }
    /// The account name to hand Himalaya.
    pub fn for_himalaya(&self) -> &str {
        if self.himalaya_account.trim().is_empty() { &self.name } else { &self.himalaya_account }
    }
}

/// A place a message can be put.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Where {
    /// What it's called in your mailbox.
    pub name: String,
    /// A Gmail label rather than a folder.
    pub is_label: bool,
}

/// What Atlas does to a message. Deliberately short, and deliberately without
/// a delete.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Apply a label. Gmail only, and non-destructive.
    Label(String),
    /// Move to a folder. Reversible.
    MoveTo(String),
    MarkRead,
    /// Take it out of the inbox without deleting it.
    Archive,
    /// Leave it alone.
    Nothing,
}

impl Action {
    /// Can this be undone by hand in thirty seconds?
    pub fn reversible(&self) -> bool {
        // All of them, which is why there is no delete.
        true
    }
}

/// The categories Atlas sorts into, and what they're for.
///
/// Names chosen to make sense in a mail app you're looking at on a phone, not
/// in a config file.
pub fn categories() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Needs you", "asks a question or a decision"),
        ("Deadline", "asks something, with a date attached"),
        ("Waiting on them", "you've replied, it's their move"),
        ("Read later", "worth knowing, nothing to do"),
        ("Receipts", "orders, invoices, confirmations"),
        ("Newsletters", "subscribed, not urgent"),
        ("Noise", "automated, nothing in it for you"),
    ]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MailConfig {
    pub enabled: bool,
    /// Every mailbox Atlas reads. Empty is the honest default — there is
    /// nothing to connect to until one is added.
    pub accounts: Vec<Account>,
    /// Folders never touched.
    pub leave_alone: Vec<String>,
    /// Apply the categories, or just report what it would do.
    pub actually_sort: bool,
    /// Never move anything out of the inbox, only label it.
    pub label_only: bool,
    /// Standing approval for cold brand outreach *at all*. Not enough on
    /// its own — the separate per-recipient outreach-approval list still
    /// has to say yes to that specific recipient. A master switch and a
    /// per-recipient allowlist, both have to say yes.
    pub may_email_brands: bool,
    /// How many cold-outreach sends are allowed in a day. Client replies
    /// are never counted against this — see `outbox::cold_outreach_sent_since`.
    pub cold_outreach_daily_cap: u32,
    /// How mail is read: `imap` (Atlas's own, the default) or `himalaya`
    /// (the Himalaya program, which decodes every part and keeps its own
    /// accounts and passwords -- `himalaya`).
    pub backend: String,
    /// The Himalaya program, when `backend` is `himalaya`.
    pub himalaya: String,
}

impl MailConfig {
    /// Read through Himalaya rather than Atlas's own IMAP?
    pub fn by_himalaya(&self) -> bool {
        self.backend.trim().eq_ignore_ascii_case("himalaya")
    }
}

impl Default for MailConfig {
    fn default() -> Self {
        MailConfig {
            enabled: false,
            accounts: Vec::new(),
            leave_alone: vec![
                "Sent".into(), "Drafts".into(), "Trash".into(), "Spam".into(),
                "Junk".into(), "Archive".into(),
            ],
            actually_sort: false,
            // The safe default: you can see what Atlas thinks without it
            // moving anything.
            label_only: true,
            may_email_brands: false,
            cold_outreach_daily_cap: 5,
            backend: "imap".into(),
            himalaya: "himalaya".into(),
        }
    }
}

/// Turn a category into the right action for this provider.
pub fn action_for(category: &str, p: Provider, cfg: &MailConfig) -> Action {
    if !cfg.actually_sort {
        return Action::Nothing;
    }
    // Noise is the only thing taken out of the inbox, and even then it's
    // archived rather than deleted.
    if category == "Noise" && !cfg.label_only {
        return Action::Archive;
    }
    if p.has_labels() || cfg.label_only {
        Action::Label(category.into())
    } else {
        Action::MoveTo(category.into())
    }
}

/// Where the credential comes from.
///
/// Never from config, and never typed into a prompt that logs. The vault or
/// nothing.
pub fn credential_source(account: &Account) -> Result<&str, &'static str> {
    if account.password_from_vault.trim().is_empty() {
        return Err("no vault entry named — I won't take a password in config");
    }
    Ok(&account.password_from_vault)
}

/// May Atlas touch this folder?
pub fn may_touch(folder: &str, cfg: &MailConfig) -> bool {
    !cfg.leave_alone.iter().any(|f| f.eq_ignore_ascii_case(folder))
}

/// What Atlas says before doing anything to your mailbox.
///
/// The first run is a rehearsal on purpose: telling you what it would do is
/// worth more than doing it and telling you after.
pub fn rehearsal(counts: &[(String, usize)], cfg: &MailConfig) -> String {
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    if total == 0 {
        return "Nothing to sort.".into();
    }
    let mut parts: Vec<String> = counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(c, n)| format!("{n} {c}"))
        .collect();
    parts.sort_by_key(|s| std::cmp::Reverse(s.len()));

    let what = if cfg.label_only { "label" } else { "sort" };
    format!(
        "{total} messages: {}. Say go and I'll {what} them — nothing gets deleted, and you can \
         undo any of it in your mail app.",
        parts.join(", ")
    )
}

/// Where an address came from, which is what actually stops the flow.
///
/// Unsubscribing one at a time treats the symptom. Knowing that four of your
/// subscriptions all started from one signup tells you something you can act
/// on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trail {
    pub sender: String,
    /// The plus-address or alias it arrived at, when there was one.
    pub arrived_at: Option<String>,
    /// First seen.
    pub since: u64,
}

/// Group subscriptions by how they got your address.
///
/// Plus-addressing is the useful signal here: mail to you+shop@ came from what
/// you gave the shop, so anything else arriving there was sold or leaked.
pub fn trace(trails: &[Trail]) -> Vec<(String, Vec<&str>)> {
    let mut groups: std::collections::BTreeMap<String, Vec<&str>> = Default::default();
    for t in trails {
        let key = t
            .arrived_at
            .clone()
            .unwrap_or_else(|| "your main address".into());
        groups.entry(key).or_default().push(t.sender.as_str());
    }
    groups.into_iter().filter(|(_, v)| v.len() > 1).collect()
}

/// What that means, said plainly.
pub fn what_the_trail_says(alias: &str, senders: &[&str]) -> String {
    if alias == "your main address" {
        return format!("{} of them have your main address, so there's no telling where from.", senders.len());
    }
    format!(
        "{} different senders are mailing {alias}, and you only gave that to one of them. \
         Worth retiring the alias.",
        senders.len()
    )
}

/// Which of `categories()` a message belongs in, from what `triage` makes
/// of it. Receipts and newsletters are told apart first, because triage
/// files both as "nothing to do".
pub fn category_of(m: &crate::imap::Message) -> &'static str {
    if crate::orders::status_from_subject(&m.subject).is_some()
        || ["receipt", "invoice", "order confirmation", "your order", "payment received"]
            .iter()
            .any(|w| m.subject.to_lowercase().contains(w))
    {
        return "Receipts";
    }
    if !m.list_unsubscribe.trim().is_empty() {
        return "Newsletters";
    }
    let t: crate::triage::Message = m.into();
    match crate::triage::triage(&t).needs {
        crate::triage::Needs::Decision | crate::triage::Needs::Reply => "Needs you",
        crate::triage::Needs::Deadline => "Deadline",
        crate::triage::Needs::TheirMove => "Waiting on them",
        crate::triage::Needs::Reading => "Read later",
        crate::triage::Needs::Nothing => "Noise",
    }
}

/// What sorting one mailbox would do: each message and its category.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SortPlan {
    pub account: String,
    pub gmail: bool,
    pub moves: Vec<(u32, String)>,
}

impl SortPlan {
    pub fn counts(&self) -> Vec<(String, usize)> {
        let mut c: Vec<(String, usize)> = Vec::new();
        for (_, cat) in &self.moves {
            match c.iter_mut().find(|(n, _)| n == cat) {
                Some((_, k)) => *k += 1,
                None => c.push((cat.clone(), 1)),
            }
        }
        c
    }
}

/// The category you told Atlas to delete, from what you said ("delete the
/// noise", "bin the newsletters"). Only these words, only when said.
pub fn category_to_delete(said: &str) -> Option<&'static str> {
    let t = said.to_lowercase();
    if !["delete", "bin", "trash", "throw away"].iter().any(|w| t.contains(w)) {
        return None;
    }
    categories().into_iter().map(|(c, _)| c).find(|c| t.contains(&c.to_lowercase()))
}

/// Where this account sends from: host and port.
///
/// From the IMAP host the account reads from when it has one, not the
/// address's domain -- a Microsoft 365 or Google Workspace mailbox on its own
/// domain reads fine but had "no SMTP server known" for every send (Q10).
/// Providers Connect an account knows are listed; any other `imap.x` sends
/// from `smtp.x` on 465. `None` only when nothing can be told.
pub fn smtp_for(address: &str, imap_host: &str) -> Option<(String, u16)> {
    let host = imap_host.trim().trim_start_matches("imaps://").to_lowercase();
    let known = |h: &str, port: u16| Some((h.to_string(), port));
    match host.as_str() {
        "" => {
            let p = Provider::from_address(address);
            p.smtp_host().map(|h| (h.to_string(), p.smtp_port()))
        }
        "imap.gmail.com" => known("smtp.gmail.com", 465),
        "outlook.office365.com" | "imap-mail.outlook.com" => known("smtp.office365.com", 587),
        "imap.mail.yahoo.com" => known("smtp.mail.yahoo.com", 465),
        "imap.aol.com" => known("smtp.aol.com", 465),
        "imap.mail.me.com" => known("smtp.mail.me.com", 587),
        "imap.fastmail.com" => known("smtp.fastmail.com", 465),
        "imap.zoho.com" => known("smtp.zoho.com", 465),
        "imap.gmx.com" | "imap.gmx.net" => known("mail.gmx.com", 587),
        h => h.strip_prefix("imap.").map(|rest| (format!("smtp.{rest}"), 465)),
    }
}
