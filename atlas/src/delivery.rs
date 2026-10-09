//! Getting an approved post out of the queue and onto the site.
//!
//! The publisher decides *whether* something may go. This decides *how*, and
//! carries it out through the browser. The two are kept apart on purpose: the
//! approval logic should not know or care what a CSS selector is, and the
//! browser code should never be able to decide that something is approved.
//!
//! Approval is re-checked here, immediately before the click. That is
//! deliberate duplication — the last thing between a draft and the public is
//! worth checking twice.

use crate::browser::{post_plan, Browser, BrowserConfig, PostStep, SiteProfile};
use crate::error::{AtlasError, Result};
use crate::publish::{Channel, Publisher, SendCheck};

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Sent(String),
    /// Worth trying again later — a timeout, a slow page.
    Retry(String),
    /// Will never succeed as-is. Needs you.
    Blocked(String),
    Uncertain(String),
}

impl Outcome {
    pub fn ok(&self) -> bool {
        matches!(self, Outcome::Sent(_))
    }
    pub fn message(&self) -> &str {
        match self {
            Outcome::Sent(m) | Outcome::Retry(m) | Outcome::Blocked(m) | Outcome::Uncertain(m) => m,
        }
    }
}

/// Which site profile serves this channel.
pub fn profile_for<'a>(cfg: &'a BrowserConfig, channel: &Channel) -> Option<&'a SiteProfile> {
    match channel {
        Channel::X => cfg.profile("x"),
        Channel::LinkedIn => cfg.profile("linkedin"),
        Channel::Instagram => cfg.profile("instagram"),
        Channel::Facebook => cfg.profile("facebook"),
        Channel::Discord => cfg.profile("discord"),
        Channel::Other(n) => cfg.profile(n),
        // Email is not a web form Atlas fills in. It needs a mail path, and
        // that is a separate piece of work — saying so is better than
        // pretending a browser profile covers it.
        Channel::Email { .. } => None,
    }
}

/// The full sequence for one post, without executing it. Inspectable, and
/// testable without a browser.
pub fn plan(
    pub_: &Publisher,
    cfg: &BrowserConfig,
    id: u64,
    online: bool,
) -> std::result::Result<Vec<PostStep>, Outcome> {
    let Some(post) = pub_.get(id) else {
        return Err(Outcome::Blocked("no such post".into()));
    };

    // The publisher's own gate first: approval, length, media, connection.
    match pub_.check(id, online, None) {
        SendCheck::Go => {}
        SendCheck::Hold(why) if why == "no connection" => return Err(Outcome::Retry(why)),
        SendCheck::Hold(why) => return Err(Outcome::Blocked(why)),
    }

    let Some(profile) = profile_for(cfg, &post.channel) else {
        return Err(Outcome::Blocked(format!(
            "no way to post to {} yet",
            post.channel.name()
        )));
    };

    if let Some(why) = crate::browser::media_problem(profile, &post.media, |f| std::path::Path::new(f).is_file()) {
        return Err(Outcome::Blocked(why));
    }
    let mut steps = post_plan(profile, &post.body, true);
    if !post.media.is_empty() {
        let at = steps.iter().position(|s| *s == PostStep::Submit).unwrap_or(steps.len());
        steps.insert(at, PostStep::Attach(post.media.clone()));
    }
    Ok(steps)
}

/// Carry out a post. Only ever called with an id the publisher already
/// cleared, and it checks again anyway.

pub fn send_unless(pub_: &mut Publisher, browser: &mut Browser, cfg: &BrowserConfig, id: u64, online: bool, stop: &dyn Fn() -> bool) -> Outcome {
    let steps = match plan(pub_, cfg, id, online) {
        Ok(s) => s,
        Err(o) => return o,
    };
    if !steps.contains(&PostStep::Submit) {
        return Outcome::Blocked("plan has no submit step — not approved".into());
    }

    let Some(post) = pub_.get(id).cloned() else {
        return Outcome::Blocked("post vanished".into());
    };
    let Some(profile) = profile_for(cfg, &post.channel) else {
        return Outcome::Blocked("no profile".into());
    };
    let media = match post.verified_media_copies(stop) {
        Ok(media) => media,
        Err(why) => return Outcome::Blocked(why),
    };
    if stop() { return Outcome::Blocked("publication stopped before submission".into()); }

    if let Err(e) = browser.compose(profile, &post.body) {
        return classify(e);
    }
    if let Err(e) = browser.attach_media(profile, &media.paths) {
        return classify(e);
    }

    // Belt and braces: the text could have changed while the page loaded.
    if stop() { return Outcome::Blocked("publication stopped before submission".into()); }
    match pub_.check(id, online, None) {
        SendCheck::Go => {}
        SendCheck::Hold(why) => return Outcome::Blocked(format!("stopped before posting: {why}")),
    }

    pub_.mark_submission(id, false, "Submission in progress; don't repeat until checked");
    match browser.publish_verified(profile) {
        Ok(Some(receipt)) => {
            pub_.mark_sent(id, &format!("Program confirmed publication: {receipt}"), true);
            Outcome::Sent(format!("posted to {}: {receipt}", post.channel.name()))
        }
        Ok(None) | Err(_) => {
            let message = format!("Publication on {} isn't confirmed. Check the service; I won't retry automatically.", post.channel.name());
            pub_.mark_submission(id, true, &message);
            Outcome::Uncertain(message)
        }
    }
}

/// Is this a Bluesky post? Those go through Bluesky's own API
/// (`send_bluesky`), not a browser.
pub fn is_bluesky(channel: &crate::publish::Channel) -> bool {
    matches!(channel, crate::publish::Channel::Other(n) if matches!(n.trim().to_lowercase().as_str(), "bluesky" | "bsky"))
}

/// Carry out a Bluesky post through its API, with the same gate as a
/// browser post: only an id the publisher cleared, checked again here.

pub fn send_bluesky_unless(pub_: &mut Publisher, x: &dyn crate::social::posting::Xrpc, handle: &str, app_password: &str, id: u64, online: bool, now: u64, stop: &dyn Fn() -> bool) -> Outcome {
    let Some(post) = pub_.get(id).cloned() else {
        return Outcome::Blocked("no such post".into());
    };
    match pub_.check(id, online, None) {
        SendCheck::Go => {}
        SendCheck::Hold(why) if why == "no connection" => return Outcome::Retry(why),
        SendCheck::Hold(why) => return Outcome::Blocked(why),
    }
    if handle.trim().is_empty() {
        return Outcome::Blocked("no Bluesky handle -- put yours under Your accounts on the Social page".into());
    }
    if app_password.is_empty() {
        return Outcome::Blocked("no Bluesky app password kept -- make one in Bluesky's settings and keep it on the Social page".into());
    }
    let media = match post.verified_media_copies(stop) {
        Ok(media) => media,
        Err(why) => return Outcome::Blocked(why),
    };
    if stop() { return Outcome::Blocked("publication stopped before submission".into()); }
    pub_.mark_submission(id, false, "Bluesky submission pending");
    match crate::social::post_with_app_password(x, handle, app_password, &post.body, &media.paths, now, &|p| std::fs::read(p)) {
        Ok(uri) if uri.starts_with("at://") && uri.contains("/app.bsky.feed.post/") => {
            pub_.mark_sent(id, &format!("Program confirmed publication: {uri}"), true);
            Outcome::Sent(format!("posted to Bluesky: {uri}"))
        }
        Ok(_) => {
            let why = "Bluesky returned no valid publication receipt; check the service before another attempt.";
            pub_.mark_submission(id, true, why);
            Outcome::Uncertain(why.into())
        }
        Err(e) if e.starts_with("couldn't reach Bluesky") && !e.contains("(posting") => {
            // Authentication and blob upload precede createRecord. Nothing
            // was submitted, so repeating these phases cannot duplicate a post.
            if let Some(queued) = pub_.posts.iter_mut().find(|p| p.id == id) { queued.state = post.state; queued.result = Some(e.clone()); }
            Outcome::Retry(e)
        },
        Err(e) if e.starts_with("couldn't reach Bluesky") => {
            pub_.mark_submission(id, true, &e);
            Outcome::Uncertain(format!("Bluesky publication isn't confirmed ({e}); check it before another attempt."))
        },
        Err(e) if e.contains("(posting") => {
            pub_.mark_submission(id, true, &e);
            Outcome::Uncertain(format!("Bluesky publication isn't confirmed ({e}); check it before another attempt."))
        }
        Err(e) => {
            pub_.mark_sent(id, &e, false);
            Outcome::Blocked(e)
        }
    }
}

/// Distinguish "try again in a minute" from "this needs you".
///
/// Getting this wrong in either direction is bad: retrying a permanent failure
/// forever is noise, and giving up on a slow page loses a post you approved.
pub fn classify(e: AtlasError) -> Outcome {
    let m = e.to_string();
    let low = m.to_lowercase();
    if low.contains("not signed in") {
        return Outcome::Blocked(m);
    }
    if low.contains("none of these are on the page") {
        return Outcome::Blocked(format!("{m} — the site's layout may have changed"));
    }
    if low.contains("timed out")
        || low.contains("never finished loading")
        || low.contains("connect")
        || low.contains("closed by peer")
        || low.contains("never came up")
        || low.contains("never finished uploading")
        || low.contains("stayed greyed out")
    {
        return Outcome::Retry(m);
    }
    Outcome::Blocked(m)
}

/// A short line for speaking after a send attempt.
pub fn spoken(o: &Outcome) -> String {
    match o {
        Outcome::Sent(m) => format!("{m}."),
        Outcome::Retry(_) => "Couldn't reach the site. I'll try again shortly.".into(),
        Outcome::Blocked(why) => format!("Didn't post: {why}."),
        Outcome::Uncertain(why) => why.clone(),
    }
}

pub type SendResult = Result<Outcome>;
