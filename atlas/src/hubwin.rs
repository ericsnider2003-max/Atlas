//! The hub, inside Atlas's own window.
//!
//! Eric, 23 Sep 2026: *"What was the hub designed for … if it's not going to
//! get used or accessible on the laptop? … that's how everything can be
//! tracked, things get accessed, and settings changed."* He was right. The 17
//! Sep ruling ("the hub should not be a browser — you say that if Atlas has a
//! dependency on the internet") had been read as "the laptop doesn't get the
//! hub", which left every hub page — status, activity, approvals, devices,
//! outstanding, connections — reachable only from the phone.
//!
//! His ruling on 23 Sep: the hub shows **inside Atlas's window**, drawn by the
//! web view that ships with Windows (WebView2), not by a browser. What that
//! means in practice, and what keeps the 17 Sep concern answered:
//!
//! - **No browser.** No address bar, no tabs, no other program launched. It is
//!   a region of the Atlas window.
//! - **No internet.** It loads only Atlas's own hub on this machine
//!   (`127.0.0.1` on Atlas's port). Any link that would leave it is refused —
//!   `stays_on_the_hub` is the one rule, and it is tested.
//! - **The same pages as the phone.** One set of pages, so the laptop and the
//!   phone can never disagree about what a setting is or what's outstanding.
//!
//! On anything but Windows there's no system web view to borrow, so the Hub
//! page says so and gives the address instead.

/// The hub address for one page, carrying the token so the page opens
/// already signed in (the hub then keeps it as a cookie). Built by
/// `server::hub_url`, the one place hub addresses are made, and kept to hub
/// pages: anything that isn't one opens the hub's front page.
pub fn page_url(port: u16, token: &str, page: &str) -> String {
    let page = if page.starts_with("/hub") && !page.contains('?') { page } else { "/hub" };
    crate::server::hub_url(port, token, page)
}

/// May the hub region go to this address? Only Atlas's own hub on this
/// machine. Everything else — another site, another port, a file — is
/// refused, so the region can't become a browser by following a link.
pub fn stays_on_the_hub(url: &str, port: u16) -> bool {
    let url = url.trim();
    for host in ["127.0.0.1", "localhost"] {
        let base = format!("http://{host}:{port}");
        if let Some(rest) = url.strip_prefix(&base) {
            // The port must end where it ends: `:87870` isn't `:8787`.
            if rest.is_empty() || rest.starts_with('/') || rest.starts_with('?') {
                return true;
            }
        }
    }
    // WebView2 shows its own error page on `about:blank`; allow that and
    // nothing else outside the hub.
    url == "about:blank"
}

/// A hub address with its token taken out, for anything written down.
pub fn without_token(url: &str) -> String {
    match url.split_once("t=") {
        Some((before, after)) => {
            let rest = after.find('&').map(|i| &after[i..]).unwrap_or("");
            format!("{before}t=…{rest}")
        }
        None => url.to_string(),
    }
}

/// Note a refused address, and refuse it.
#[cfg_attr(not(windows), allow(dead_code))]
fn turn_away(dir: &std::path::Path, url: &str, port: u16) -> bool {
    if for_the_browser(url, port) {
        // A link out of Atlas (a site's security page, a download): your own
        // browser opens it, and the hub stays where it was. Refusing it
        // outright, as this did until 27 Sep 2026, left the link doing
        // nothing at all.
        open_in_browser(url);
        note(dir, &format!("opened in the browser: {}", without_token(url)));
    } else {
        note(dir, &format!("refused {}", without_token(url)));
    }
    false
}

/// Whether a link a hub page leads to belongs in your own browser: an
/// ordinary web address that isn't this hub. Never a file, a script or
/// anything else a browser would hand to another program.
pub fn for_the_browser(url: &str, port: u16) -> bool {
    let u = url.trim();
    (u.starts_with("https://") || u.starts_with("http://"))
        && !stays_on_the_hub(u, port)
        && u.len() < 4096
        && !u.chars().any(|c| c.is_control() || c == ' ' || c == '"')
}

/// Open `url` in your default browser, through the shell -- no console, no
/// program of Atlas's own.
#[cfg(windows)]
fn open_in_browser(url: &str) {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let target = HSTRING::from(url);
    // SAFETY: every string outlives the call; no window is named.
    unsafe {
        ShellExecuteW(HWND::default(), w!("open"), &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn open_in_browser(_url: &str) {}

/// Keep a line about what the hub region did, beside its own data — the one
/// way to know what happened when nobody was looking at the screen.
#[cfg_attr(not(windows), allow(dead_code))]
fn note(dir: &std::path::Path, what: &str) {
    use std::io::Write;
    crate::heard!(std::fs::create_dir_all(dir));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("hub-region.log")) {
        crate::kept!(writeln!(f, "{} {what}", crate::store::now()));
    }
}

/// The web view's start-up switches: the ones wry uses by default, plus a
/// local-only inspection port when `ATLAS_DEV` names one (for checking the
/// hub region on a machine nobody is looking at).
#[cfg_attr(not(windows), allow(dead_code))]
fn browser_args() -> String {
    let mut args = String::from("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    if let Some(port) = std::env::var("ATLAS_DEV").ok().and_then(|v| v.strip_prefix("hub-inspect:").map(str::to_string)) {
        if port.parse::<u16>().is_ok() {
            args.push_str(&format!(" --remote-debugging-port={port} --remote-debugging-address=127.0.0.1"));
        }
    }
    args
}

/// Where the hub region sits in the window, in the window's logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    /// Close enough that moving the web view would be wasted work.
    pub fn same_as(&self, o: &Area) -> bool {
        (self.x - o.x).abs() < 0.5 && (self.y - o.y).abs() < 0.5 && (self.w - o.w).abs() < 0.5 && (self.h - o.h).abs() < 0.5
    }
}

/// The hub region. Made the first time the Hub page is shown, then shown and
/// hidden as you move between pages, so the hub keeps its place.
pub struct Hub {
    #[cfg_attr(not(windows), allow(dead_code))]
    port: u16,
    #[cfg_attr(not(windows), allow(dead_code))]
    data_dir: std::path::PathBuf,
    #[cfg(windows)]
    view: Option<(wry::WebView, wry::WebContext)>,
    #[cfg_attr(not(windows), allow(dead_code))]
    area: Option<Area>,
    visible: bool,
    problem: Option<String>,
}

impl Hub {
    pub fn new(port: u16, data_dir: std::path::PathBuf) -> Hub {
        Hub {
            port,
            data_dir,
            #[cfg(windows)]
            view: None,
            area: None,
            visible: false,
            problem: None,
        }
    }

    /// The hub moved to another port (`server::open_hub`): the view is made
    /// again for it, since the rule for where it may go names the port.
    pub fn set_port(&mut self, port: u16) {
        if port == self.port {
            return;
        }
        self.port = port;
        #[cfg(windows)]
        {
            self.view = None;
        }
    }

    /// Why the hub couldn't be shown, if it couldn't.
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// Whether this machine can show the hub in the window at all.
    pub fn can_embed() -> bool {
        cfg!(windows)
    }

    /// Show the hub at `url` in `area` of the window `parent`. Makes the web
    /// view on first use; after that only moves it, and loads `url` only when
    /// asked to (`go`), so a page you've navigated to stays put.
    #[cfg(windows)]
    pub fn show<W: raw_window_handle::HasWindowHandle>(&mut self, parent: &W, area: Area, url: &str, go: bool) {
        use wry::dpi::{LogicalPosition, LogicalSize};
        let rect = wry::Rect {
            position: LogicalPosition::new(area.x as f64, area.y as f64).into(),
            size: LogicalSize::new(area.w.max(1.0) as f64, area.h.max(1.0) as f64).into(),
        };
        if self.view.is_none() {
            if self.problem.is_some() {
                return;
            }
            crate::heard!(std::fs::create_dir_all(&self.data_dir));
            let mut ctx = wry::WebContext::new(Some(self.data_dir.clone()));
            let port = self.port;
            let log_dir = self.data_dir.clone();
            use wry::WebViewBuilderExtWindows;
            let built = wry::WebViewBuilder::new_with_web_context(&mut ctx)
                .with_url(url)
                .with_bounds(rect)
                .with_navigation_handler(move |u| stays_on_the_hub(&u, port) || turn_away(&log_dir, &u, port))
                // A link that opens a new window (`target=_blank`): never a
                // window of the hub's own; a web address goes to your browser.
                .with_new_window_req_handler(move |u, _f| {
                    if for_the_browser(&u, port) {
                        open_in_browser(&u);
                    }
                    wry::NewWindowResponse::Deny
                })
                .with_on_page_load_handler({
                    let log_dir = self.data_dir.clone();
                    move |event, u| {
                        if matches!(event, wry::PageLoadEvent::Finished) {
                            note(&log_dir, &format!("showing {}", without_token(&u)));
                        }
                    }
                })
                .with_devtools(false)
                // Not focused on creation: asking WebView2 to take focus
                // fails outright ("the parameter is incorrect") whenever
                // Atlas's window isn't the one in front — and that failure
                // threw the whole hub away. Found on the real laptop, 23 Sep.
                .with_focused(false)
                .with_background_color((13, 15, 18, 255))
                .with_additional_browser_args(browser_args())
                .build_as_child(parent);
            match built {
                Ok(v) => {
                    note(&self.data_dir, "the hub region started");
                    self.view = Some((v, ctx));
                    self.area = Some(area);
                    self.visible = true;
                }
                Err(e) => {
                    note(&self.data_dir, &format!("the hub region didn't start: {e}"));
                    #[cfg(target_env = "gnu")]
                    let loader = format!(" Its loader is kept at {}.", crate::webview2_loader::place().display());
                    #[cfg(not(target_env = "gnu"))]
                    let loader = String::new();
                    self.problem = Some(format!(
                        "Windows' built-in web view didn't start ({e}). It comes with Windows 11 and \
                         with Edge on Windows 10 — Windows Update installs it if it's missing.{loader}"
                    ));
                }
            }
            return;
        }
        let Some((view, _)) = self.view.as_ref() else { return };
        if self.area.map(|a| !a.same_as(&area)).unwrap_or(true) {
            crate::heard!(view.set_bounds(rect));
            self.area = Some(area);
        }
        if !self.visible {
            crate::heard!(view.set_visible(true));
            self.visible = true;
        }
        if go {
            crate::heard!(view.load_url(url));
        }
    }

    /// Put the hub away while another page is showing. It keeps its place.
    pub fn hide(&mut self) {
        #[cfg(windows)]
        if let Some((view, _)) = self.view.as_ref() {
            if self.visible {
                crate::heard!(view.set_visible(false));
            }
        }
        self.visible = false;
    }
}
