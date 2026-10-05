//! Drafting, scheduling, and sending things that go out into the world.
//!
//! Social posts and emails are the one category where a mistake is public and
//! cannot be taken back. So the rules here are stricter than anywhere else in
//! Atlas:
//!
//! * Nothing is ever sent without approval of **that specific post**.
//!   Approving one does not approve the next.
//! * **Editing after approval voids the approval.** Otherwise "yes, send that"
//!   could be attached to text you never read.
//! * A scheduled post is checked again at send time — approval, length, and a
//!   connection. Time passing is not consent.
//! * Cancelling is always available right up to the moment it goes.

use crate::error::Result;
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    X,
    LinkedIn,
    Instagram,
    Facebook,
    Discord,
    Email { to: String, subject: String },
    Other(String),
}

impl Channel {
    pub fn name(&self) -> String {
        match self {
            Channel::X => "X".into(),
            Channel::LinkedIn => "LinkedIn".into(),
            Channel::Instagram => "Instagram".into(),
            Channel::Facebook => "Facebook".into(),
            Channel::Discord => "Discord".into(),
            Channel::Email { to, .. } => format!("email to {to}"),
            Channel::Other(n) => n.clone(),
        }
    }

    /// Character ceiling, where the platform has one. Configurable rather than
    /// hard-coded, because platforms change these and a stale constant means a
    /// post rejected at send time.
    fn default_limit(&self) -> Option<usize> {
        match self {
            Channel::X => Some(280),
            Channel::LinkedIn => Some(3000),
            Channel::Instagram => Some(2200),
            Channel::Facebook => Some(63206),
            Channel::Discord => Some(2000),
            Channel::Email { .. } | Channel::Other(_) => None,
        }
    }

    /// Does this need a picture or video to post at all?
    fn requires_media(&self) -> bool {
        matches!(self, Channel::Instagram)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostState {
    Draft,
    AwaitingApproval,
    /// Approved and waiting for its time.
    Scheduled,
    /// Approved, due now, nothing left to check.
    ReadyToSend,
    Sent,
    Failed,
    Cancelled,
    /// Everything checked, and nothing to send it with.
    ///
    /// Distinct from `Failed` (something went wrong) and from `Sent` (it
    /// went). This is "Atlas did its part and the last mile is not built" --
    /// and naming it is what stops the tick announcing the same post every
    /// two seconds for the life of the install.
    Held,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Post {
    pub id: u64,
    pub channel: Channel,
    pub body: String,
    #[serde(default)]
    pub media: Vec<String>,
    pub state: PostState,
    /// Unix time to send. None means hold until told.
    #[serde(default)]
    pub send_at: Option<u64>,
    /// The exact text that was approved. If `body` no longer matches, the
    /// approval no longer applies.
    #[serde(default)]
    approved_text: Option<String>,
    pub created: u64,
    #[serde(default)]
    pub result: Option<String>,
}

impl Post {
    /// Is the approval still valid for what the body currently says?
    fn approval_valid(&self) -> bool {
        self.approved_text.as_deref() == Some(self.body.as_str())
    }

    fn over_limit(&self, limit: Option<usize>) -> Option<usize> {
        let limit = limit.or_else(|| self.channel.default_limit())?;
        let n = self.body.chars().count();
        // then_some evaluates eagerly — n - limit underflows for every post
        // that is comfortably under the limit. then() is lazy.
        (n > limit).then(|| n - limit)
    }

    pub fn describe(&self) -> String {
        let when = match self.send_at {
            Some(_) if self.state == PostState::Scheduled => " (scheduled)",
            _ => "",
        };
        let preview: String = self.body.chars().take(60).collect();
        format!("{}{}: {preview}", self.channel.name(), when)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SendCheck {
    Go,
    /// Cannot go, with the reason.
    Hold(String),
}

/// The file in "attach C:\\...\\photo.jpg" / "add the video ~/clip.mp4",
/// quotes taken off. Only a path: something with a folder separator and an
/// extension, so "attach a nice picture" isn't read as a file.
pub fn file_to_attach(said: &str) -> Option<String> {
    let l = said.trim().to_lowercase();
    let rest = ["attach the picture ", "attach the photo ", "attach the video ", "attach ", "add the picture ", "add the photo ", "add the video ", "with the picture ", "with the video "]
        .iter()
        .find_map(|p| l.starts_with(p).then(|| said.trim()[p.len()..].trim()))?;
    let path = rest.trim_matches(['"', '\'']);
    // A sentence's full stop isn't part of the name.
    let path = path.strip_suffix('.').unwrap_or(path).trim_matches(['"', '\'']).trim();
    let p = std::path::Path::new(path);
    (path.contains(['/', '\\']) && p.extension().is_some()).then(|| path.to_string())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Publisher {
    pub posts: Vec<Post>,
    next_id: u64,
}

impl Publisher {
    pub fn load(store: &Store) -> Publisher {
        store.load("posts")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("posts", self)
    }

    pub fn draft(&mut self, channel: Channel, body: &str) -> u64 {
        self.next_id += 1;
        self.posts.push(Post {
            id: self.next_id,
            channel,
            body: body.to_string(),
            media: Vec::new(),
            state: PostState::Draft,
            send_at: None,
            approved_text: None,
            created: now(),
            result: None,
        });
        self.next_id
    }

    pub fn get(&self, id: u64) -> Option<&Post> {
        self.posts.iter().find(|p| p.id == id)
    }

    /// Change the text. This always invalidates any approval, because the
    /// approval was for the words you read, not for this slot in the queue.
    pub fn edit(&mut self, id: u64, body: &str) -> bool {
        let Some(p) = self.posts.iter_mut().find(|p| p.id == id) else { return false };
        if matches!(p.state, PostState::Sent | PostState::Cancelled) {
            return false;
        }
        p.body = body.to_string();
        p.approved_text = None;
        p.state = PostState::Draft;
        true
    }

    pub fn attach(&mut self, id: u64, path: &str) {
        if let Some(p) = self.posts.iter_mut().find(|p| p.id == id) {
            p.media.push(path.to_string());
            p.approved_text = None; // the media changed what you'd be approving
            p.state = PostState::Draft;
        }
    }

    /// Ask for approval, with the full text so you know what you are approving.
    pub fn request_approval(&mut self, id: u64) -> Option<String> {
        let p = self.posts.iter_mut().find(|p| p.id == id)?;
        p.state = PostState::AwaitingApproval;
        // What goes with it is part of what you approve, so it's named.
        let with = if p.media.is_empty() {
            String::new()
        } else {
            let names: Vec<String> = p
                .media
                .iter()
                .map(|m| std::path::Path::new(m).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| m.clone()))
                .collect();
            format!("\n\nWith: {}", names.join(", "))
        };
        Some(format!("Post this to {}?\n\n{}{with}", p.channel.name(), p.body))
    }

    /// Approve this exact text.
    pub fn approve(&mut self, id: u64) -> bool {
        let Some(p) = self.posts.iter_mut().find(|p| p.id == id) else { return false };
        if matches!(p.state, PostState::Sent | PostState::Cancelled) {
            return false;
        }
        p.approved_text = Some(p.body.clone());
        p.state = if p.send_at.is_some() { PostState::Scheduled } else { PostState::ReadyToSend };
        true
    }

    pub fn schedule(&mut self, id: u64, at: u64) -> bool {
        let Some(p) = self.posts.iter_mut().find(|p| p.id == id) else { return false };
        if matches!(p.state, PostState::Sent | PostState::Cancelled) {
            return false;
        }
        p.send_at = Some(at);
        if p.approval_valid() {
            p.state = PostState::Scheduled;
        }
        true
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        let Some(p) = self.posts.iter_mut().find(|p| p.id == id) else { return false };
        if p.state == PostState::Sent {
            return false; // gone is gone
        }
        p.state = PostState::Cancelled;
        true
    }

    /// Everything checked again at send time. Time passing is not consent.
    pub fn check(&self, id: u64, online: bool, limit: Option<usize>) -> SendCheck {
        let Some(p) = self.get(id) else { return SendCheck::Hold("no such post".into()) };
        if p.state == PostState::Cancelled {
            return SendCheck::Hold("cancelled".into());
        }
        if p.state == PostState::Sent {
            return SendCheck::Hold("already sent".into());
        }
        if !p.approval_valid() {
            return SendCheck::Hold("the text changed since you approved it".into());
        }
        if p.body.trim().is_empty() {
            return SendCheck::Hold("nothing to post".into());
        }
        if let Some(over) = p.over_limit(limit) {
            return SendCheck::Hold(format!("{} characters too long for {}", over, p.channel.name()));
        }
        if p.channel.requires_media() && p.media.is_empty() {
            return SendCheck::Hold(format!("{} needs an image", p.channel.name()));
        }
        if !online {
            return SendCheck::Hold("no connection".into());
        }
        SendCheck::Go
    }

    /// Posts whose time has come and which pass every check.
    pub fn due(&self, t: u64, online: bool) -> Vec<u64> {
        self.posts
            .iter()
            .filter(|p| matches!(p.state, PostState::Scheduled | PostState::ReadyToSend))
            .filter(|p| p.send_at.map(|at| at <= t).unwrap_or(p.state == PostState::ReadyToSend))
            .filter(|p| self.check(p.id, online, None) == SendCheck::Go)
            .map(|p| p.id)
            .collect()
    }

    /// Scheduled posts blocked by something you would want to know about.
    pub fn blocked(&self, t: u64, online: bool) -> Vec<(u64, String)> {
        self.posts
            .iter()
            .filter(|p| matches!(p.state, PostState::Scheduled | PostState::ReadyToSend))
            .filter(|p| p.send_at.map(|at| at <= t).unwrap_or(true))
            .filter_map(|p| match self.check(p.id, online, None) {
                SendCheck::Hold(why) if why != "no connection" => Some((p.id, why)),
                _ => None,
            })
            .collect()
    }

    pub fn mark_sent(&mut self, id: u64, result: &str, ok: bool) {
        if let Some(p) = self.posts.iter_mut().find(|p| p.id == id) {
            p.state = if ok { PostState::Sent } else { PostState::Failed };
            p.result = Some(result.to_string());
        }
    }

    pub fn pending(&self) -> Vec<&Post> {
        self.posts
            .iter()
            .filter(|p| {
                matches!(
                    p.state,
                    PostState::Draft | PostState::AwaitingApproval | PostState::Scheduled | PostState::ReadyToSend
                )
            })
            .collect()
    }

    pub fn summary(&self, t: u64) -> String {
        let pending = self.pending();
        if pending.is_empty() {
            return "Nothing queued to go out.".into();
        }
        let scheduled = pending.iter().filter(|p| p.state == PostState::Scheduled).count();
        let waiting = pending.iter().filter(|p| p.state != PostState::Scheduled).count();
        let next = pending
            .iter()
            .filter_map(|p| p.send_at)
            .filter(|at| *at > t)
            .min()
            .map(|at| format!(", next in {} minutes", (at - t) / 60))
            .unwrap_or_default();
        format!("{scheduled} scheduled, {waiting} awaiting you{next}.")
    }
}
