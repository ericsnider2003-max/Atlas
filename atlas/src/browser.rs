//! The browser Atlas drives.
//!
//! A headless Chrome, started on demand, attached to over DevTools, and shut
//! down when idle. Never the window you are using.
//!
//! Site profiles are the piece that makes "post this to X" possible without an
//! API key: a small table of selectors per site, so Atlas knows which box is
//! the compose field and which button is Post.

use crate::cdp::{ws_url_from_targets, Cdp};
use crate::error::{AtlasError, Result};
use crate::tools::{ExternalTool, Vars};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub port: u16,
    pub timeout_ms: u64,
    /// How long to wait for Chrome to come up before giving up.
    pub startup_ms: u64,
    pub launch: Option<ExternalTool>,
    /// Where each site's controls are.
    pub sites: Vec<SiteProfile>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        BrowserConfig {
            port: 9222,
            timeout_ms: 5000,
            startup_ms: 8000,
            launch: None,
            sites: default_sites(),
        }
    }
}

/// Where the controls are on a given site.
///
/// Selectors rot — sites redesign. So a profile carries several candidates per
/// control and Atlas uses the first that exists, and a profile that matches
/// nothing is a clear error rather than a silent misclick.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SiteProfile {
    pub name: String,
    /// Domain fragment that identifies the site.
    pub host: String,
    pub compose_url: String,
    /// Candidate selectors for the text box, in order of preference.
    pub compose_box: Vec<String>,
    /// Candidate selectors for the button that publishes.
    pub submit: Vec<String>,
    /// Something that must be present once the page is usable.
    pub ready: Vec<String>,
    /// Present only when signed in. Absent means: stop, do not type.
    #[serde(default)]
    pub signed_in: Vec<String>,
}

pub fn default_sites() -> Vec<SiteProfile> {
    vec![
        SiteProfile {
            name: "x".into(),
            host: "x.com".into(),
            compose_url: "https://x.com/compose/post".into(),
            compose_box: vec![
                "div[data-testid='tweetTextarea_0']".into(),
                "div[role='textbox'][contenteditable='true']".into(),
            ],
            submit: vec![
                "button[data-testid='tweetButton']".into(),
                "div[data-testid='tweetButtonInline']".into(),
            ],
            ready: vec!["div[role='textbox']".into()],
            signed_in: vec!["a[data-testid='AppTabBar_Profile_Link']".into()],
        },
        SiteProfile {
            name: "linkedin".into(),
            host: "linkedin.com".into(),
            compose_url: "https://www.linkedin.com/feed/".into(),
            compose_box: vec![
                "div.ql-editor[contenteditable='true']".into(),
                "div[role='textbox']".into(),
            ],
            submit: vec!["button.share-actions__primary-action".into()],
            ready: vec!["main".into()],
            signed_in: vec!["img.global-nav__me-photo".into()],
        },
    ]
}

impl BrowserConfig {
    pub fn profile(&self, name_or_host: &str) -> Option<&SiteProfile> {
        let n = name_or_host.to_lowercase();
        self.sites
            .iter()
            .find(|s| s.name == n || n.contains(&s.host) || s.host.contains(&n))
    }
}

pub struct Browser {
    pub cdp: Cdp,
    timeout: Duration,
}

impl Browser {
    /// Attach to a Chrome already listening on the debug port.
    pub fn attach(cfg: &BrowserConfig) -> Result<Browser> {
        let host = format!("127.0.0.1:{}", cfg.port);
        let timeout = Duration::from_millis(cfg.timeout_ms);
        let r = crate::http::get(&host, "/json/list", timeout)?;
        if !r.ok() {
            return Err(AtlasError::Platform(format!(
                "Chrome debug endpoint returned {}",
                r.status
            )));
        }
        let ws = ws_url_from_targets(&r.body).ok_or_else(|| {
            AtlasError::Platform("Chrome is running but has no page to attach to".into())
        })?;
        Ok(Browser { cdp: Cdp::connect(&ws, timeout)?, timeout })
    }

    /// Start Chrome if it isn't up, then attach. Polls rather than sleeping a
    /// fixed amount, so a fast machine isn't punished and a slow one isn't cut
    /// off early.
    pub fn start(cfg: &BrowserConfig, vars: &Vars) -> Result<Browser> {
        if let Ok(b) = Browser::attach(cfg) {
            return Ok(b);
        }
        let tool = cfg
            .launch
            .as_ref()
            .ok_or_else(|| AtlasError::Config("no browser launch command configured".into()))?;
        let (cmd, args) = tool.resolved(vars);
        crate::tools::command(&cmd)
            .args(&args)
            // Chrome writes a stream of its own diagnostics to stderr;
            // they'd land in Atlas's console as noise.
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            // Not waited for -- the browser is attached to over CDP below,
            // not through its exit status -- but not dropped either. See
            // `unwaited`.
            .map(crate::unwaited::dont_wait)
            .map_err(|e| AtlasError::Platform(format!("could not start {cmd}: {e}")))?;

        let deadline = std::time::Instant::now() + Duration::from_millis(cfg.startup_ms);
        let mut last: Option<AtlasError> = None;
        while std::time::Instant::now() < deadline {
            match Browser::attach(cfg) {
                Ok(b) => return Ok(b),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err(last.unwrap_or_else(|| AtlasError::Platform("Chrome never came up".into())))
    }

    pub fn open(&mut self, url: &str) -> Result<()> {
        self.cdp.navigate(url)
    }

    pub fn read(&mut self) -> Result<String> {
        self.cdp.text()
    }

    /// Every absolute link on the current page, as the page itself resolved
    /// them after its scripts ran. What research falls back on when a search
    /// engine hands curl a page with no results in its HTML.
    pub fn links(&mut self) -> Result<Vec<String>> {
        self.cdp.links()
    }

    /// Try each candidate selector and use the first that is present.
    /// Redesigns break single selectors; this degrades instead of failing.
    fn first_present(&mut self, candidates: &[String]) -> Result<String> {
        for c in candidates {
            if self.cdp.wait_for(c, 50).unwrap_or(false) {
                return Ok(c.clone());
            }
        }
        Err(AtlasError::Platform(format!(
            "none of these are on the page: {}",
            candidates.join(", ")
        )))
    }

    fn wait_ready(&mut self, p: &SiteProfile) -> Result<()> {
        let ms = self.timeout.as_millis() as u64;
        for c in &p.ready {
            if self.cdp.wait_for(c, ms)? {
                return Ok(());
            }
        }
        Err(AtlasError::Platform(format!("{} never finished loading", p.name)))
    }

    /// Is there a signed-in session? Typing a post into a logged-out page
    /// silently does nothing, which looks like success.
    fn signed_in(&mut self, p: &SiteProfile) -> Result<bool> {
        if p.signed_in.is_empty() {
            return Ok(true);
        }
        for c in &p.signed_in {
            if self.cdp.wait_for(c, 300).unwrap_or(false) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Put text in the compose box. **Does not submit** — publishing is a
    /// separate, deliberate call, so nothing can go out as a side effect of
    /// drafting.
    pub fn compose(&mut self, p: &SiteProfile, text: &str) -> Result<()> {
        self.open(&p.compose_url)?;
        self.wait_ready(p)?;
        if !self.signed_in(p)? {
            return Err(AtlasError::Platform(format!(
                "not signed in to {} — sign in and try again",
                p.name
            )));
        }
        let box_sel = self.first_present(&p.compose_box)?;
        self.cdp.fill(&box_sel, text)
    }

    /// Click the publish button. Only ever called after an approval upstream.
    pub fn publish(&mut self, p: &SiteProfile) -> Result<()> {
        let btn = self.first_present(&p.submit)?;
        self.cdp.click(&btn)
    }

    /// Press the one control on the page labelled with the words for this
    /// change, after the read-back and your yes (`confirmed`). Waits for the
    /// page to settle first; never types anything.
    /// Press the one control for `change`, but only on the site `expected`
    /// names: a redirect to a sign-in page or another site stops it.
    pub fn press_the_one_at(
        &mut self,
        change: &crate::confirmed::Change,
        expected: Option<&str>,
    ) -> Result<crate::confirmed::Pressed> {
        use crate::confirmed::{before_pressing, labels_for, press_js, pressed_from, Pressed};
        let words = labels_for(change);
        if words.is_empty() {
            return Ok(Pressed::NoWordsForIt);
        }
        // Pages build their controls after loading; look for a few seconds.
        // A page mid-navigation can refuse a script once or twice; that's
        // retried until the deadline rather than reported as a failure.
        let deadline = std::time::Instant::now() + self.timeout;
        loop {
            if let Some(want) = expected {
                match self.cdp.eval("location.host") {
                    Ok(host) => {
                        if let Some(stop) = before_pressing(want, host.as_str().unwrap_or("")) {
                            // A redirect may still be on its way; only a
                            // settled page is judged.
                            if std::time::Instant::now() > deadline || matches!(stop, Pressed::WantsYouToSignIn) {
                                return Ok(stop);
                            }
                            std::thread::sleep(Duration::from_millis(250));
                            continue;
                        }
                    }
                    Err(e) if std::time::Instant::now() > deadline => return Err(e),
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(250));
                        continue;
                    }
                }
            }
            let pressed = match self.cdp.eval(&press_js(&words)) {
                Ok(got) => pressed_from(got.as_str().unwrap_or("")),
                Err(e) if std::time::Instant::now() > deadline => return Err(e),
                Err(_) => Pressed::NotFound,
            };
            if pressed != Pressed::NotFound || std::time::Instant::now() > deadline {
                return Ok(pressed);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    pub fn close(&mut self) {
        self.cdp.close();
    }
}

/// Steps to post, so the sequence is inspectable and testable without a live
/// browser.
#[derive(Debug, Clone, PartialEq)]
pub enum PostStep {
    Open(String),
    WaitReady,
    CheckSignedIn,
    Fill(String),
    /// Only present when the caller has an approval in hand.
    Submit,
}

pub fn post_plan(p: &SiteProfile, text: &str, approved: bool) -> Vec<PostStep> {
    let mut steps = vec![
        PostStep::Open(p.compose_url.clone()),
        PostStep::WaitReady,
        PostStep::CheckSignedIn,
        PostStep::Fill(text.to_string()),
    ];
    if approved {
        steps.push(PostStep::Submit);
    }
    steps
}
