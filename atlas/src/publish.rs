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
    /// Submission may have reached the service. Never automatically repeat it.
    PendingSubmission,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(into = "PostWire", from = "PostWire")]
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
    #[serde(default)]
    media_approval: Option<MediaApproval>,
    pub created: u64,
    #[serde(default)]
    pub result: Option<String>,
}

/// Keep the stored state readable by older Atlas builds. They see Held and
/// no valid approval, so neither their tick nor a direct send can repeat it.
/// The additive field survives their store's unknown-field preservation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostWire {
    id: u64,
    channel: Channel,
    body: String,
    #[serde(default)]
    media: Vec<String>,
    state: PostState,
    #[serde(default)]
    send_at: Option<u64>,
    #[serde(default)]
    approved_text: Option<String>,
    created: u64,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    submission: Option<SubmissionStatus>,
    #[serde(default)]
    media_approval: Option<MediaApproval>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaIdentity {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ApprovedMediaState { Ready, Scheduled, #[serde(other)] Held }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MediaApproval {
    body: String,
    files: Vec<MediaIdentity>,
    state: ApprovedMediaState,
}

impl MediaApproval {
    fn matches(&self, body: &str, media: &[String]) -> bool {
        self.body == body && self.files.len() == media.len() && self.files.iter().zip(media).all(|(a,p)| a.path == *p && a.sha256.len() == 64 && a.sha256.bytes().all(|b| b.is_ascii_hexdigit()))
    }
}

fn media_identities(media: &[String], stop: &dyn Fn() -> bool) -> std::result::Result<Vec<MediaIdentity>, String> {
    if media.len() > 16 { return Err("too many attachments to approve safely".into()); }
    media.iter().map(|path| {
        if stop() { return Err("stopped".into()); }
        let before = media_stamp(path).ok_or_else(|| format!("attachment is missing or unreadable: {path}"))?;
        let hash = crate::digest::sha256_file_hex_unless(std::path::Path::new(path), stop).map_err(|e| format!("couldn't inspect attachment {path}: {e}"))?.ok_or("stopped")?;
        if media_stamp(path).as_ref() != Some(&before) { return Err(format!("attachment changed during approval: {path}; review it again")); }
        Ok(MediaIdentity { path: path.clone(), sha256: hash })
    }).collect()
}

fn media_stamp(path: &str) -> Option<(u64, Option<String>)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() { return None; }
    Some((meta.len(), meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|n| n.as_nanos().to_string())))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SubmissionStatus {
    Pending,
    #[serde(other)]
    Uncertain,
}

impl From<Post> for PostWire {
    fn from(post: Post) -> Self {
        let submission = match post.state { PostState::PendingSubmission => Some(SubmissionStatus::Pending), PostState::Uncertain => Some(SubmissionStatus::Uncertain), _ => None };
        let mut media_approval = post.media_approval;
        let media_fence = !post.media.is_empty() && media_approval.is_some() && matches!(post.state, PostState::ReadyToSend | PostState::Scheduled | PostState::Held);
        if let Some(a) = media_approval.as_mut() { a.state = match post.state { PostState::ReadyToSend => ApprovedMediaState::Ready, PostState::Scheduled => ApprovedMediaState::Scheduled, _ => ApprovedMediaState::Held }; }
        Self { id: post.id, channel: post.channel, body: post.body, media: post.media, state: if submission.is_some() || media_fence { PostState::Held } else { post.state }, send_at: post.send_at, approved_text: if submission.is_some() || media_fence { None } else { post.approved_text }, created: post.created, result: post.result, submission, media_approval }
    }
}

impl From<PostWire> for Post {
    fn from(post: PostWire) -> Self {
        let mut state = match post.submission { Some(SubmissionStatus::Pending) => PostState::PendingSubmission, Some(SubmissionStatus::Uncertain) => PostState::Uncertain, None => post.state };
        let mut approved_text = if post.submission.is_some() { None } else { post.approved_text };
        let mut result = post.result;
        let media_approval = post.media_approval.filter(|a| a.matches(&post.body, &post.media));
        if post.submission.is_none() && !post.media.is_empty() {
            if state == PostState::Held && approved_text.is_none() {
                if let Some(a) = media_approval.as_ref() {
                    state = match a.state { ApprovedMediaState::Ready => PostState::ReadyToSend, ApprovedMediaState::Scheduled => PostState::Scheduled, ApprovedMediaState::Held => PostState::Held };
                    if state != PostState::Held { approved_text = Some(post.body.clone()); }
                }
            }
            if media_approval.is_none() && approved_text.is_some() {
                if matches!(state, PostState::ReadyToSend | PostState::Scheduled) { state = PostState::Held; }
                approved_text = None;
                result = Some("Attachments need fresh review and approval; the older approval has no content identity.".into());
            }
        }
        Self { id: post.id, channel: post.channel, body: post.body, media: post.media, state, send_at: post.send_at, approved_text, created: post.created, result, media_approval }
    }
}

impl Post {
    /// Upload a captured copy of the approved bytes, never a mutable owner path.
    pub fn verified_media_copies(&self, stop: &dyn Fn() -> bool) -> std::result::Result<VerifiedMedia, String> {
        use std::io::{Read, Write};
        let mut captured = VerifiedMedia { paths: Vec::new(), folder: None };
        if self.media.is_empty() { return Ok(captured); }
        let approval = self.media_approval.as_ref().filter(|a| a.matches(&self.body, &self.media)).ok_or("attachments need fresh approval")?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        let folder = std::env::temp_dir().join(format!("atlas-approved-media-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&folder).map_err(|e| format!("couldn't prepare approved attachments: {e}"))?;
        captured.folder = Some(folder.clone());
        let mut buffer = vec![0u8; 1024 * 1024];
        for (index, approved) in approval.files.iter().enumerate() {
            if stop() { return Err("attachment preparation stopped".into()); }
            let extension = std::path::Path::new(&approved.path).extension().and_then(|s| s.to_str()).unwrap_or("bin");
            let target = folder.join(format!("attachment-{index}.{extension}"));
            let mut input = std::fs::File::open(&approved.path).map_err(|e| format!("couldn't read approved attachment: {e}"))?;
            let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| e.to_string())?;
            captured.paths.push(target.to_string_lossy().into_owned());
            loop {
                if stop() { return Err("attachment preparation stopped".into()); }
                let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
                if n == 0 { break; }
                output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            }
            drop(output);
            let digest = crate::digest::sha256_file_hex_unless(&target, stop).map_err(|e| e.to_string())?.ok_or("attachment preparation stopped")?;
            if digest != approved.sha256 { return Err("an attachment changed since approval; review it and approve again".into()); }
        }
        Ok(captured)
    }
    /// Is the approval still valid for what the body currently says?
    fn approval_valid(&self) -> bool {
        self.approved_text.as_deref() == Some(self.body.as_str()) && (self.media.is_empty() || self.media_approval.as_ref().is_some_and(|a| a.matches(&self.body, &self.media)))
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

pub struct VerifiedMedia { pub paths: Vec<String>, folder: Option<std::path::PathBuf> }
impl Drop for VerifiedMedia {
    fn drop(&mut self) {
        for path in &self.paths { crate::heard!(std::fs::remove_file(path)); }
        if let Some(folder) = &self.folder { crate::heard!(std::fs::remove_dir(folder)); }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostApprovalReceipt {
    pub tag: String, pub version: u8, pub expected: Post, pub approved: Post,
    pub requested_at: u64, pub requested_send_at: Option<u64>,
}

impl Publisher {
    pub fn apply_approval_receipt(&mut self, receipt: &PostApprovalReceipt) -> bool {
        if receipt.tag != "atlas.post_approval" || receipt.version != 1 { return false; }
        let Some(current) = self.posts.iter_mut().find(|p| p.id == receipt.expected.id) else { return false; };
        let approved = &receipt.approved;
        if current != &receipt.expected || approved.id != current.id || approved.channel != current.channel
            || approved.body != current.body || approved.media != current.media || approved.created != current.created || approved.send_at != receipt.requested_send_at.or(current.send_at)
            || !approved.approval_valid() || !matches!(approved.state, PostState::ReadyToSend | PostState::Scheduled) { return false; }
        *current = approved.clone();
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SendCheck {
    Go,
    /// Cannot go, with the reason.
    Hold(String),
}

pub const CHECKED_ABSENT: &str = "Owner checked: not published; fresh approval required";

pub fn review_fingerprint(post: &Post) -> String {
    let stamps: Vec<_> = post.media.iter().map(|p| (p, media_stamp(p))).collect();
    crate::digest::sha256_hex(&serde_json::to_vec(&(&post.channel, &post.body, &post.media, stamps)).unwrap_or_default())
}

/// A receipt belongs to the destination, not an arbitrary pasted web page.
pub fn valid_receipt(channel: &Channel, receipt: &str) -> bool {
    if receipt.chars().any(char::is_whitespace) || receipt.chars().any(char::is_control) { return false; }
    let Some(rest) = receipt.trim().strip_prefix("https://") else { return false };
    let Some((host, path)) = rest.split_once('/') else { return false };
    match channel {
        Channel::X => matches!(host, "x.com" | "www.x.com") && path.contains("/status/"),
        Channel::LinkedIn => matches!(host, "linkedin.com" | "www.linkedin.com") && path.starts_with("feed/update/"),
        Channel::Instagram => matches!(host, "instagram.com" | "www.instagram.com") && (path.starts_with("p/") || path.starts_with("reel/")),
        Channel::Facebook => matches!(host, "facebook.com" | "www.facebook.com") && (path.contains("posts/") || path.starts_with("permalink.php?")),
        Channel::Discord => host == "discord.com" && path.starts_with("channels/"),
        Channel::Other(n) if matches!(n.to_lowercase().as_str(), "bluesky" | "bsky") => host == "bsky.app" && path.starts_with("profile/") && path.contains("/post/"),
        _ => false,
    }
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
            media_approval: None,
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
        if matches!(p.state, PostState::Sent | PostState::Cancelled | PostState::PendingSubmission | PostState::Uncertain) {
            return false;
        }
        p.body = body.to_string();
        p.approved_text = None;
        p.media_approval = None;
        p.state = PostState::Draft;
        true
    }

    pub fn attach(&mut self, id: u64, path: &str) {
        if let Some(p) = self.posts.iter_mut().find(|p| p.id == id) {
            if matches!(p.state, PostState::Sent | PostState::Cancelled | PostState::PendingSubmission | PostState::Uncertain) { return; }
            p.media.push(path.to_string());
            p.approved_text = None; // the media changed what you'd be approving
            p.media_approval = None;
            p.state = PostState::Draft;
        }
    }

    /// Ask for approval, with the full text so you know what you are approving.
    pub fn request_approval(&mut self, id: u64) -> Option<String> {
        let p = self.posts.iter_mut().find(|p| p.id == id)?;
        if matches!(p.state, PostState::Sent | PostState::Cancelled | PostState::PendingSubmission | PostState::Uncertain) { return None; }
        p.approved_text = None;
        p.media_approval = None;
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
        self.approve_unless(id, &|| false).is_ok()
    }

    /// Called by the approval worker for real media; never hash on the control loop.
    pub fn approve_unless(&mut self, id: u64, stop: &dyn Fn() -> bool) -> std::result::Result<(), String> {
        let p = self.posts.iter_mut().find(|p| p.id == id).ok_or("no such post")?;
        if matches!(p.state, PostState::Sent | PostState::Cancelled | PostState::PendingSubmission | PostState::Uncertain) {
            return Err("this post cannot be approved in its current state".into());
        }
        if stop() { return Err("stopped".into()); }
        let files = media_identities(&p.media, stop)?;
        p.media_approval = if files.is_empty() { None } else { Some(MediaApproval { body: p.body.clone(), files, state: if p.send_at.is_some() { ApprovedMediaState::Scheduled } else { ApprovedMediaState::Ready } }) };
        p.approved_text = Some(p.body.clone());
        p.state = if p.send_at.is_some() { PostState::Scheduled } else { PostState::ReadyToSend };
        Ok(())
    }

    pub fn schedule(&mut self, id: u64, at: u64) -> bool {
        let Some(p) = self.posts.iter_mut().find(|p| p.id == id) else { return false };
        if matches!(p.state, PostState::Sent | PostState::Cancelled | PostState::PendingSubmission | PostState::Uncertain) {
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
        if matches!(p.state, PostState::Sent | PostState::PendingSubmission | PostState::Uncertain) {
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
        if matches!(p.state, PostState::PendingSubmission | PostState::Uncertain) {
            return SendCheck::Hold("publication isn't confirmed; check the service before trying again".into());
        }
        if !p.approval_valid() {
            return SendCheck::Hold(if p.approved_text.as_deref() != Some(p.body.as_str()) { "the text changed since you approved it" } else { "the attachments need fresh content approval" }.into());
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

    pub fn mark_submission(&mut self, id: u64, uncertain: bool, result: &str) {
        if let Some(post) = self.posts.iter_mut().find(|p| p.id == id) {
            post.state = if uncertain { PostState::Uncertain } else { PostState::PendingSubmission };
            post.result = Some(result.into());
        }
    }

    /// Only after checking the provider, explicitly resolve an uncertain send.
    /// A confirmed absence voids approval; another attempt needs fresh approval.
    pub fn reconcile_submission(&mut self, id: u64, receipt: Option<&str>) -> bool {
        let Some(post) = self.posts.iter_mut().find(|p| p.id == id) else { return false };
        if !matches!(post.state, PostState::PendingSubmission | PostState::Uncertain) { return false; }
        match receipt.filter(|r| !r.trim().is_empty()) {
            Some(receipt) => { post.state = PostState::Sent; post.result = Some(format!("Owner checked publication: {receipt}")); }
            None => { post.state = PostState::Draft; post.approved_text = None; post.media_approval = None; post.result = Some(CHECKED_ABSENT.into()); }
        }
        true
    }

    /// Approval applies only to the exact reviewed draft after an owner checked
    /// the service for absence. This cannot bypass an uncertain-send fence.
    pub fn approve_checked_retry(&mut self, id: u64, fingerprint: &str) -> bool {
        let Some(post) = self.get(id) else { return false };
        if post.state != PostState::Draft || post.result.as_deref() != Some(CHECKED_ABSENT) || review_fingerprint(post) != fingerprint { return false; }
        if let Some(post) = self.posts.iter_mut().find(|p| p.id == id) { post.send_at = None; }
        self.approve(id)
    }

    pub fn pending(&self) -> Vec<&Post> {
        self.posts
            .iter()
            .filter(|p| {
                matches!(
                    p.state,
                    PostState::Draft | PostState::AwaitingApproval | PostState::Scheduled | PostState::ReadyToSend | PostState::PendingSubmission | PostState::Uncertain
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
