//! The page around every page: shell, appearance, the phone app's head, icons, sidebar, menus.
//!
//! Moved out of `hub.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// Escape anything that came from outside before it goes into HTML.
///
/// Note names, device names and app paths all end up on these pages, and a
/// window title containing a `<` should render as a window title.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn shell(title: &str, body: &str) -> String {
    shell_at(None, title, body)
}

/// The page, with the navigation knowing where you are.
///
/// `here` marks the current item and draws the trail back up. `None` is for
/// the few pages served outside the routing table (an error, a spoken reply),
/// which have no place in the tree and should not pretend to.
pub fn shell_at(here: Option<Page>, title: &str, body: &str) -> String {
    shell_with(here, title, body, 0)
}

impl Appearance {
    /// Apply one choice from the menu. `false` for anything the menu can't
    /// send, so a mistyped address changes nothing.
    pub fn choose(&mut self, what: &str, to: &str) -> bool {
        match (what, to) {
            // Warm Paper is the default (Eric, 26 Sep), so choosing it clears
            // the choice; "auto" is kept, because it has to beat that default.
            ("theme", "light" | "dark" | "auto") => self.theme = to.into(),
            ("theme", "paper") => self.theme.clear(),
            ("text", "large" | "larger") => self.text = to.into(),
            ("text", "normal") => self.text.clear(),
            ("contrast", "high") => self.high_contrast = true,
            ("contrast", "normal") => self.high_contrast = false,
            ("motion", "off") => self.reduce_motion = true,
            ("motion", "on") => self.reduce_motion = false,
            _ => return false,
        }
        true
    }

    /// The attributes that carry these on the page's root element.
    pub fn attrs(&self) -> String {
        let mut a = String::new();
        if matches!(self.theme.as_str(), "light" | "dark" | "auto") {
            a.push_str(&format!(" data-theme={}", self.theme));
        }
        if matches!(self.text.as_str(), "large" | "larger") {
            a.push_str(&format!(" data-text={}", self.text));
        }
        if self.high_contrast {
            a.push_str(" data-contrast=high");
        }
        if self.reduce_motion {
            a.push_str(" data-motion=off");
        }
        a
    }
}

/// Apply your appearance to a finished page. One insertion point, like
/// [`with_palette`], so the page functions stay free of per-person state.
pub fn with_appearance(page: String, a: &Appearance) -> String {
    let attrs = a.attrs();
    if attrs.is_empty() {
        return page;
    }
    page.replacen("<html lang=en", &format!("<html lang=en{attrs}"), 1)
}

/// The "Aa" menu. Links, not a script: each one sets a choice and brings you
/// back. Which option is current is shown by CSS reading the root's
/// attributes, so the menu needs no knowledge of who is looking.
pub(super) fn appearance_menu() -> String {
    let seg = |what: &str, opts: &[(&str, &str)]| {
        let mut s = String::from("<div class=segmented>");
        for (to, label) in opts {
            s.push_str(&format!(
                "<a href='/hub/appearance?set={what}&amp;to={to}' data-v='{what}-{to}'>{label}</a>"
            ));
        }
        s.push_str("</div>");
        s
    };
    let toggle = |what: &str, label: &str, on: &str, off: &str| {
        format!(
            "<div class=toggle>{label}<span class=pair>\
             <a href='/hub/appearance?set={what}&amp;to={on}' data-v='{what}-{on}' aria-label='{label} on'></a>\
             <a href='/hub/appearance?set={what}&amp;to={off}' data-v='{what}-{off}' aria-label='{label} off'></a>\
             </span></div>"
        )
    };
    format!(
        "<details class=appwrap><summary class='tool icon' aria-label='Appearance and access'>Aa</summary>\
         <div class=appmenu role=menu aria-label='Appearance and access'>\
         <p class=mh>Theme</p>{theme}<p class=mh>Text size</p>{text}<p class=mh>Access</p>{contrast}{motion}</div></details>",
        theme = seg("theme", &[("paper", "Paper"), ("light", "Light"), ("dark", "Dark"), ("auto", "Auto")]),
        text = seg("text", &[("normal", "Normal"), ("large", "Large"), ("larger", "Larger")]),
        contrast = toggle("contrast", "High contrast", "high", "normal"),
        motion = toggle("motion", "Reduce motion", "off", "on"),
    )
}

/// Put the true "waiting" count in a finished page's header.
///
/// Only the dashboard used to be told the count, so every other page's header
/// said "Nothing waiting" while the dashboard beside it said "1 waiting". One
/// insertion point, like [`with_appearance`], so no page can be the one that forgot.
pub fn with_waiting(page: String, waiting: usize) -> String {
    if waiting == 0 {
        return page;
    }
    let href = Page::Outstanding.href();
    let page = page.replacen(
        &format!("<a class=tool href='{href}'>Nothing waiting</a>"),
        &format!("<a class=tool href='{href}'><span class=dot></span>{waiting} waiting</a>"),
        1,
    );
    // And the phone's Home tab carries it as a badge, unless it already does.
    if page.contains("<span class=badge>") {
        return page;
    }
    let home_tab = page.find("<nav class=tabs").and_then(|t| page[t..].find("</svg>Home</a>").map(|i| t + i));
    match home_tab {
        Some(i) => {
            let at = i + "</svg>Home".len();
            format!(
                "{}<span class=badge><span class=sr>, </span>{waiting}<span class=sr> waiting</span></span>{}",
                &page[..at],
                &page[at..]
            )
        }
        None => page,
    }
}

/// Put the palette on a finished page.
///
/// One insertion point rather than a parameter threaded through thirteen page
/// functions. Those thirteen would become fourteen, and the fourteenth would
/// be the page someone forgot — a palette missing from one page is worse than
/// no palette, because you learn to reach for it and then it is not there.
/// `tests/palette.rs` checks every page has it.
pub fn with_palette(
    page: String,
    entries: &[crate::palette::Entry],
    recent: &crate::palette::Recent,
) -> String {
    match page.rfind("</body>") {
        Some(at) => {
            let overlay = palette_overlay(entries, recent);
            format!("{}{overlay}{}", &page[..at], &page[at..])
        }
        // Not a whole page — an error fragment, or something rendered for a
        // test. Left alone rather than having an overlay bolted to the end of
        // it, which would produce markup that looks right and is not.
        None => page,
    }
}

/// The files anyone may fetch: the service worker and the icons.
///
/// Public because a browser fetches them without the cookie (an icon for a
/// home screen, a worker registration's update check) and because nothing in
/// them is private — they are the same bytes on every install. The manifest is
/// deliberately not here: it carries the token.
pub fn public_file(method: &str, path: &str) -> Option<PublicFile> {
    if method != "GET" {
        return None;
    }
    const ICON_CACHE: &str = "Cache-Control: max-age=86400\r\n";
    match path {
        SERVICE_WORKER_PATH => Some(PublicFile {
            content_type: "text/javascript; charset=utf-8",
            // The worker lives at /hub/sw.js, which on its own may only
            // control /hub/…; this lets it control /hub itself as well.
            headers: "Service-Worker-Allowed: /hub\r\nCache-Control: no-cache\r\n",
            bytes: SERVICE_WORKER.as_bytes(),
        }),
        "/hub/icon-192.png" => {
            Some(PublicFile { content_type: "image/png", headers: ICON_CACHE, bytes: ICON_192 })
        }
        "/hub/icon-512.png" => {
            Some(PublicFile { content_type: "image/png", headers: ICON_CACHE, bytes: ICON_512 })
        }
        "/hub/icon-maskable-512.png" => {
            Some(PublicFile { content_type: "image/png", headers: ICON_CACHE, bytes: ICON_MASKABLE_512 })
        }
        "/hub/apple-touch-icon.png" => Some(PublicFile {
            content_type: "image/png",
            headers: ICON_CACHE,
            bytes: APPLE_TOUCH_ICON,
        }),
        _ => None,
    }
}

/// The web app manifest, with the token in `start_url`.
///
/// See the section note for why the token has to be there. Icons are listed
/// once as `any` and once as `maskable` rather than as `"any maskable"`,
/// which some browsers read as one purpose they do not know.
pub fn manifest(token: &str) -> String {
    let icon = |src: &str, size: &str, purpose: &str| {
        serde_json::json!({ "src": src, "sizes": size, "type": "image/png", "purpose": purpose })
    };
    serde_json::json!({
        "id": "/hub",
        "name": "Atlas",
        "short_name": "Atlas",
        "description": "Your Atlas, from your phone.",
        "start_url": format!("/hub?t={token}"),
        "scope": "/hub",
        "display": "standalone",
        "background_color": APP_BG_LIGHT,
        "theme_color": APP_BG_LIGHT,
        "icons": [
            icon("/hub/icon-192.png", "192x192", "any"),
            icon("/hub/icon-512.png", "512x512", "any"),
            icon("/hub/icon-maskable-512.png", "512x512", "maskable"),
        ],
        // The phone's share sheet hands a link or words straight to Give.
        // No orientation lock: WCAG 1.3.4 — the hub works either way up.
        "share_target": { "action": "/hub/give", "method": "GET", "params": { "title": "title", "text": "text", "url": "url" } },
    })
    .to_string()
}

/// Put the app tags in a finished page's head.
///
/// One insertion point, like [`with_palette`], and in `server.rs` rather than
/// in `shell_with`: the server is the one place that knows the token, and it
/// is what both the running Atlas (`hublive`) and settings-only mode
/// (`run_hub`) send every page through — so neither can be the one that
/// forgot. It also keeps the page functions themselves script-free, which
/// `tests/the_hub_works_offline.rs` holds them to.
///
/// A fragment with no `</head>` is left alone.
///
/// 30 Sep 2026: it also keeps a slider's `<output>` saying the slider's value
/// as it moves -- it showed the saved value, and dragging changed nothing on
/// screen until Save.
pub fn with_app_head(page: String, token: &str) -> String {
    let Some(at) = page.find("</head>") else { return page };
    let t = esc(token);
    let tags = format!(
        "<link rel=manifest href=\"{MANIFEST_PATH}?t={t}\">\
         <meta name=theme-color content=\"{APP_BG}\" media=\"(prefers-color-scheme: dark)\">\
         <meta name=theme-color content=\"{APP_BG_LIGHT}\" media=\"(prefers-color-scheme: light)\">\
         <meta name=apple-mobile-web-app-capable content=yes>\
         <meta name=mobile-web-app-capable content=yes>\
         <meta name=apple-mobile-web-app-title content=Atlas>\
         <link rel=apple-touch-icon href=\"/hub/apple-touch-icon.png\">\
         <script>if('serviceWorker' in navigator)\
         navigator.serviceWorker.register('{SERVICE_WORKER_PATH}',{{scope:'/hub'}});\
         document.addEventListener('input',function(e){{var t=e.target;if(!t||t.type!=='range'||!t.id)return;\
         var o=document.querySelector('output[for=\"'+t.id+'\"]');if(!o)return;\
         o.textContent=(Number(t.step)<1?Number(t.value).toFixed(2):t.value)+(o.getAttribute('data-unit')||'')}})</script>"
    );
    format!("{}{tags}{}", &page[..at], &page[at..])
}

/// The page, with the navigation knowing where you are.
///
/// The header carries the three things that must never be more than one click
/// away no matter how deep you are: the way home, anything waiting for you,
/// and the settings. Settings buried three levels down is how a tool teaches
/// you it does not expect you to change anything.
pub(super) fn shell_with(here: Option<Page>, title: &str, body: &str, waiting: usize) -> String {
    let trail = here.map(crumbs).unwrap_or_default();
    let heading = match here {
        Some(p) if p != Page::Dashboard => format!(
            "<h1 class=page>{}</h1><p class=pagenote>{}</p>",
            esc(p.label()),
            esc(p.note())
        ),
        _ => String::new(),
    };
    format!(
        "<!doctype html><html lang=en{theme}><head><meta charset=utf-8>\
         <meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <title>Atlas — {t}</title><style>{STYLE}</style></head><body>\
         <a class=skip href='#main'>Skip to the page</a>\
         <div class=wrap>{sidebar}<div class=col-main>\
         <header><a class=hbrand href='/hub' aria-label='Atlas, Home'>{MARK}<span>Atlas</span></a>{trail}<span class=grow></span>\
         <a class=tool id=palopen href='/hub/find' aria-label='Find anything'>{SEARCH_ICON}<span class=lbl>Find anything</span>{ctrl_k}</a>\
         <a class='tool talkbtn' href='/hub/talk' aria-label='Talk'>{MIC_ICON}</a>\
         {waiting_link}<a class='tool help' href='/hub/help' aria-label='Help'>{HELP_ICON}<span class=lbl>Help</span></a>{theme_chip}{menu}</header>\
         <main id=main tabindex=-1>{heading}{body}</main></div></div>{tabs}</body></html>",
        t = esc(title),
        // A phone has no Ctrl key (`phonemode`).
        ctrl_k = if crate::phonemode::on() { "" } else { "<kbd class=ck>Ctrl K</kbd>" },
        // Two stored appearances, merged 26 Sep 2026. The hub's own "Aa" menu
        // (theme paper/light/dark/auto, text size, contrast, motion) is written in by
        // `with_appearance`, right after `<html lang=en`, so it comes first and
        // wins where both say the same thing. The Settings page's choices
        // (`appearance`: Warm Paper by default, or Ember Dark, the
        // colour-blind theme or the system's; an accent, a colour-blind mode,
        // density) are written here.
        theme = crate::appearance::Appearance::load().html_attrs(),
        sidebar = sidebar_html(here, waiting),
        tabs = tabs_html(here, waiting),
        menu = menu_html(here),
        theme_chip = appearance_menu(),
        waiting_link = if waiting > 0 {
            format!(
                "<a class=tool href='{}'><span class=dot></span>{} waiting</a>",
                Page::Outstanding.href(),
                waiting
            )
        } else {
            format!("<a class=tool href='{}'>Nothing waiting</a>", Page::Outstanding.href())
        },
    )
}

/// A page's icon in the sidebar: 24-unit strokes, drawn here, nothing fetched.
pub(super) fn icon(page: Page) -> &'static str {
    match page {
        Page::Dashboard => "<path d='M3 11l9-8 9 8M5 10v10h14V10'/>",
        Page::Now => "<circle cx=12 cy=12 r=9 /><path d='M12 7v5l3 2'/>",
        Page::Calendar => "<rect x=3 y=4.5 width=18 height=16.5 rx=2.5 /><path d='M3 9.5h18M8 2.5v4M16 2.5v4'/>",
        Page::Workshop => "<path d='M4 6h16M4 12h16M4 18h10'/>",
        Page::Outstanding => "<path d='M9 11l3 3L22 4M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11'/>",
        Page::Workspace => "<rect x=3 y=3 width=7 height=7 rx=1 /><rect x=14 y=3 width=7 height=11 rx=1 /><rect x=3 y=14 width=7 height=7 rx=1 />",
        Page::LookingBack => "<path d='M3 12a9 9 0 1 0 9-9'/><path d='M3 4v5h5'/>",
        Page::Friends => "<circle cx=9 cy=8 r=3.2 /><path d='M15 21v-2a4 4 0 0 0-4-4H7a4 4 0 0 0-4 4v2'/>",
        Page::Groups => "<path d='M21 15a2 2 0 0 1-2 2H8l-4 4V5a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2z'/>",
        Page::Messages => "<path d='M21 15a2 2 0 0 1-2 2H8l-4 4V5a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2z'/>",
        Page::Documents => "<path d='M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z'/><path d='M14 3v6h6'/>",
        Page::Business => "<path d='M3 21V7l9-4 9 4v14'/>",
        Page::SharedTasks => "<path d='M9 11l3 3L22 4'/>",
        Page::Clients => "<circle cx=9 cy=8 r=3.2 /><path d='M15 21v-2a4 4 0 0 0-4-4H7a4 4 0 0 0-4 4v2'/>",
        Page::Partners => "<circle cx=12 cy=12 r=9 /><path d='M12 3v18M3 12h18'/>",
        Page::Sound => "<path d='M11 5L6 9H2v6h4l5 4V5z'/><path d='M15.5 8.5a5 5 0 0 1 0 7'/>",
        Page::Trusted => "<path d='M12 3l7 3v6c0 4-3 7-7 9-4-2-7-5-7-9V6z'/><path d='M9 12l2 2 4-4'/>",
        Page::Give => "<path d='M12 5v14M5 12h14'/>",
        Page::Social => "<path d='M4 19V9M10 19V5M16 19v-7M22 19H2'/>",
        Page::Opportunities => "<circle cx=11 cy=11 r=7 /><path d='M21 21l-5-5'/>",
        Page::Offline => "<path d='M2 8.8a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0'/><path d='M12 20h.01'/>",
        Page::Talk => "<rect x=9 y=3 width=6 height=12 rx=3 /><path d='M6 11a6 6 0 0 0 12 0M12 17v4'/>",
        Page::Help => "<circle cx=12 cy=12 r=9 /><path d='M9.5 9a2.5 2.5 0 1 1 3.5 2.3c-.6.3-1 .9-1 1.7M12 17h.01'/>",
        Page::Updates => "<path d='M21 12a9 9 0 1 1-3-6.7L21 8'/><path d='M21 3v5h-5'/>",
        Page::Phone => "<rect x=6.5 y=2.5 width=11 height=19 rx=2.5 /><path d='M11 18.5h2'/>",
        Page::Feedback => "<path d='M21 15a2 2 0 0 1-2 2H8l-4 4V5a2 2 0 0 1 2-2h13a2 2 0 0 1 2 2z'/><path d='M12 7v4M12 14h.01'/>",
        Page::Settings => "<circle cx=12 cy=12 r=3 /><path d='M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2'/>",
        _ => "<circle cx=12 cy=12 r=8 />",
    }
}

/// The navigation the design draws: a labelled sidebar in Notion's calm shape.
///
/// The brand, Search and Now at the top; the Personal pages; the people you
/// talk to; a business's own group, once a business exists (written in by
/// `with_business`, since only the running Atlas knows your businesses);
/// everything else one press away under More; Settings at the foot.
///
/// It replaced the command deck's icon rail on 26 Sep 2026. Four icons and a
/// grid button hid every page's name, which is the opposite of the calm,
/// read-at-a-glance structure Eric chose on 20-21 Sep.
pub(super) fn sidebar_html(here: Option<Page>, waiting: usize) -> String {
    let item = |page: Page, name: &str, extra: &str| {
        format!(
            "<a class='nav{on}' href='{href}'{cur}><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>{paths}</svg>{name}{extra}</a>",
            on = if here == Some(page) { " on" } else { "" },
            cur = if here == Some(page) { " aria-current=page" } else { "" },
            href = page.href(),
            paths = icon(page),
            name = esc(name),
        )
    };
    let mut out = format!(
        "<nav class=sidebar aria-label='Atlas'><a class=brand href='/hub' aria-label='Atlas, Home'>{MARK}<span class=owner>Atlas</span></a>\
         <a class=nav href='/hub/find'><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>\
         <circle cx=11 cy=11 r=7 /><path d='M21 21l-4-4'/></svg>Search{}</a>",
        if crate::phonemode::on() { "" } else { "<kbd class=ck>Ctrl K</kbd>" }
    );
    // The sidebar's own groups, shown open. Settings sits at the foot rather
    // than in a group, as drawn; the rest go under More.
    let shown = |g: &str| g.is_empty() || g == "Personal" || g == "People";
    for (group, pages) in NAV.iter().filter(|(g, _)| shown(g)) {
        if !group.is_empty() {
            out.push_str(&format!("<p class=grp>{}</p>", esc(group)));
        }
        for page in pages.iter().filter(|p| p.here()) {
            let extra = if *page == Page::Outstanding && waiting > 0 {
                format!("<span class=count>{waiting}</span>")
            } else {
                String::new()
            };
            out.push_str(&item(*page, page.label(), &extra));
        }
    }
    // The three things from Your devices people reach for, shown open.
    out.push_str("<p class=grp>Your devices</p>");
    for page in [Page::Phone, Page::Updates, Page::Help].into_iter().filter(|p| p.here()) {
        out.push_str(&item(page, page.label(), ""));
    }
    out.push_str(BUSINESS_SLOT);
    // Eric, 27 Sep 2026: More was "too complicated and not organized" --
    // twenty-six pages in five groups. Now two short menus: More, for your own
    // work and the business pages; Atlas setup, for how Atlas behaves, what
    // it's connected to, and Atlas about itself.
    let folded = |label: &str, groups: &[(&str, &[Page])]| {
        let inside = here.is_some_and(|h| groups.iter().any(|(_, ps)| ps.contains(&h)));
        let mut m = format!(
            "<details class=more{open}><summary class=nav><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>\
             <circle cx=5 cy=12 r=1.6 /><circle cx=12 cy=12 r=1.6 /><circle cx=19 cy=12 r=1.6 /></svg>{label}</summary><div class=side>",
            open = if inside { " open" } else { "" },
        );
        for (group, pages) in groups {
            m.push_str(&format!("<p class=gh>{}</p>", esc(group)));
            for page in pages.iter().filter(|p| p.here()) {
                m.push_str(&format!(
                    "<a class='{}' href='{}'{}>{}</a>",
                    if here == Some(*page) { "here" } else { "" },
                    page.href(),
                    if here == Some(*page) { " aria-current=page" } else { "" },
                    esc(page.label())
                ));
            }
        }
        m.push_str("</div></details>");
        m
    };
    out.push_str(&folded(
        "More",
        &[("Your work", &[Page::Workspace, Page::LookingBack, Page::Give, Page::Social, Page::Opportunities]), ("Business", &[Page::Business, Page::SharedTasks, Page::Clients, Page::Partners])],
    ));
    out.push_str(&folded(
        "Atlas setup",
        &[
            ("How Atlas behaves", &[Page::Sound, Page::Permissions, Page::Gestures, Page::Access]),
            ("What it's connected to", &[Page::Accounts, Page::AddOns, Page::Connections, Page::Sync, Page::Offline]),
            ("About Atlas", &[Page::Status, Page::Recommendations, Page::Activity, Page::Edits, Page::Feedback]),
        ],
    ));
    out.push_str("<span class=grow></span>");
    out.push_str(&item(Page::Settings, "Settings", ""));
    out.push_str("</nav>");
    out
}

/// Every page, one press away: the phone's Menu, at the end of its one-row
/// top bar. (On a laptop, a tablet or an unfolded phone the sidebar is this.)
pub(super) fn menu_html(here: Option<Page>) -> String {
    let mut out = String::from(
        "<details class=allpages><summary class='tool icon' aria-label='Menu, every page'><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true><path d='M4 6h16M4 12h16M4 18h16'/></svg></summary><div class=side>",
    );
    for (group, pages) in NAV {
        out.push_str(&format!("<p class=gh>{}</p>", esc(if group.is_empty() { "Atlas" } else { group })));
        for page in pages.iter().filter(|p| p.here()) {
            out.push_str(&format!(
                "<a class='{}' href='{}'{}>{}</a>",
                if here == Some(*page) { "here" } else { "" },
                page.href(),
                if here == Some(*page) { " aria-current=page" } else { "" },
                esc(page.label())
            ));
        }
    }
    out.push_str("</div></details>");
    out
}

/// The phone's bottom tab bar, as the design's phone screens (24 Sep) draw
/// it: five, evenly spaced — Home, Projects, Messages, Business, Settings.
/// Talk is the microphone in the top bar, not a sixth tab (Eric, 26 Sep: a
/// raised sixth sat off-centre). What's waiting on you is a badge on Home.
pub(super) fn tabs_html(here: Option<Page>, waiting: usize) -> String {
    let mut out = String::from("<nav class=tabs aria-label='Main'>");
    for (p, name) in [
        (Page::Dashboard, "Home"),
        (Page::Workshop, "Projects"),
        (Page::Messages, "Messages"),
        (Page::Business, "Business"),
        (Page::Settings, "Settings"),
    ] {
        let on = here == Some(p);
        out.push_str(&format!(
            "<a class='tab{}' href='{}'{}><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>{}</svg>{}{}</a>",
            if on { " on" } else { "" },
            p.href(),
            if on { " aria-current=page" } else { "" },
            icon(p),
            name,
            if p == Page::Dashboard && waiting > 0 {
                format!("<span class=badge><span class=sr>, </span>{waiting}<span class=sr> waiting</span></span>")
            } else {
                String::new()
            }
        ));
    }
    out.push_str("</nav>");
    out
}

/// Your businesses in the sidebar, each as its own group — the design's
/// "Northwind LLC · business" — or nothing at all when there are none: the
/// Business section appears only once a business is added.
///
/// Each group opens that business's own pages: Overview, Shared tasks,
/// Clients and Partners, as the design draws them.
pub fn with_business(page: String, businesses: &[String]) -> String {
    if businesses.is_empty() {
        return page.replacen(BUSINESS_SLOT, "", 1);
    }
    let mut g = String::new();
    for b in businesses {
        let q = format!("b={}", esc(&crate::research::urlencode(b)));
        g.push_str(&format!("<p class=grp>{} <small>· business</small></p>", esc(b)));
        for p in [Page::Business, Page::SharedTasks, Page::Clients, Page::Partners] {
            g.push_str(&format!(
                "<a class=nav href='{}?{q}'><svg viewBox='0 0 24 24' fill=none stroke=currentColor stroke-width=2 aria-hidden=true>{}</svg>{}</a>",
                p.href(),
                icon(p),
                esc(p.label())
            ));
        }
    }
    page.replacen(BUSINESS_SLOT, &g, 1)
}

/// The owner's name on the sidebar's brand — "Eric's Atlas" — when Atlas
/// knows what to call you. Written in by the running Atlas, like the rest.
pub fn with_owner(page: String, name: Option<&str>) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => page.replacen(
            "<span class=owner>Atlas</span>",
            &format!("<span class=owner>{}'s Atlas</span>", esc(n)),
            1,
        ),
        None => page,
    }
}

pub(super) fn control(s: &crate::settings::Setting) -> String {
    let key = esc(&s.key);
    // Every control carries its setting's name, so a screen reader and voice
    // control hear "Speaking volume", not "edit text" (WCAG 1.3.1, 4.1.2).
    let name = esc(&s.name);
    match &s.value {
        Value::Toggle(on) => {
            let (label, next) = if *on { ("Turn off", "off") } else { ("Turn on", "on") };
            let confirm = if s.weight.needs_confirming() && !*on {
                // The words in an attribute of their own, read by the script:
                // put inside the script's quotes, an apostrophe ("that's")
                // ended the string, the handler didn't compile, and the switch
                // turned on with no question asked (29 Sep 2026).
                format!(" data-confirm=\"{}\" onsubmit=\"return confirm(this.dataset.confirm)\"", esc(&s.cost))
            } else {
                String::new()
            };
            format!(
                "<span class={} >{}</span> <form method=post action=/hub/set{confirm}>\
                 <input type=hidden name=key value=\"{key}\">\
                 <input type=hidden name=value value=\"{next}\">\
                 <button aria-label=\"{label} {name}\">{label}</button></form>",
                if *on { "on" } else { "off" },
                if *on { "on" } else { "off" }
            )
        }
        Value::Choice { value, options } => {
            let opts: String = options
                .iter()
                .map(|o| {
                    format!(
                        "<option{}>{}</option>",
                        if o == value { " selected" } else { "" },
                        esc(o)
                    )
                })
                .collect();
            format!(
                "<form method=post action=/hub/set><input type=hidden name=key value=\"{key}\">\
                 <select name=value aria-label=\"{name}\">{opts}</select><button aria-label=\"Save {name}\">Save</button></form>"
            )
        }
        Value::Number { value, min, max } => format!(
            "<form method=post action=/hub/set><input type=hidden name=key value=\"{key}\">\
             <input name=value type=number step=any min={min} max={max} value=\"{value}\" aria-label=\"{name}\">\
             <button aria-label=\"Save {name}\">Save</button></form>"
        ),
        Value::Text(v) => format!(
            "<form method=post action=/hub/set><input type=hidden name=key value=\"{key}\">\
             <input autocomplete=off name=value value=\"{}\" aria-label=\"{name}\"><button aria-label=\"Save {name}\">Save</button></form>",
            esc(v)
        ),
        Value::List(v) => format!(
            "<form method=post action=/hub/set><input type=hidden name=key value=\"{key}\">\
             <input autocomplete=off name=value value=\"{}\" placeholder=\"comma separated\" aria-label=\"{name}\">\
             <button aria-label=\"Save {name}\">Save</button></form>",
            esc(&v.join(", "))
        ),
    }
}

pub(super) fn tag(w: Weight) -> String {
    let class = match w {
        Weight::Permission => "tag perm",
        Weight::Sensitive => "tag sens",
        _ => "tag",
    };
    format!("<span class=\"{class}\">{}</span>", w.label())
}
