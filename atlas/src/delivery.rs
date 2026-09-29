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
}

impl Outcome {
    pub fn ok(&self) -> bool {
        matches!(self, Outcome::Sent(_))
    }
    pub fn message(&self) -> &str {
        match self {
            Outcome::Sent(m) | Outcome::Retry(m) | Outcome::Blocked(m) => m,
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

    Ok(post_plan(profile, &post.body, true))
}

/// Carry out a post. Only ever called with an id the publisher already
/// cleared, and it checks again anyway.
pub fn send(
    pub_: &mut Publisher,
    browser: &mut Browser,
    cfg: &BrowserConfig,
    id: u64,
    online: bool,
) -> Outcome {
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

    if let Err(e) = browser.compose(profile, &post.body) {
        return classify(e);
    }

    // Belt and braces: the text could have changed while the page loaded.
    match pub_.check(id, online, None) {
        SendCheck::Go => {}
        SendCheck::Hold(why) => return Outcome::Blocked(format!("stopped before posting: {why}")),
    }

    match browser.publish(profile) {
        Ok(()) => {
            pub_.mark_sent(id, "posted", true);
            Outcome::Sent(format!("posted to {}", post.channel.name()))
        }
        Err(e) => {
            let o = classify(e);
            if let Outcome::Blocked(why) = &o {
                pub_.mark_sent(id, why, false);
            }
            o
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
    }
}

pub type SendResult = Result<Outcome>;
