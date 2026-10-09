//! The hub is the design Eric locked on 20-21 Sep 2026 — not the mock-up.
//!
//! The design (the pinned canvas "Atlas Hub — Command Deck") lived only as a
//! canvas and a written spec outside the tree. The one design file in the
//! tree was the 20 Sep mock-up that came before it (`design/superseded/
//! command-deck.html`), so the 26 Sep merge took that mock-up for "Eric's
//! design", recoloured it in Warm Paper, and this file — then called
//! `command_deck.rs` — held the hub to the mock-up. Eric caught it the same
//! day: "one of the chats when it was doing a merge still didn't pick up the
//! hub design and was still defaulting to the old one."
//!
//! Now the design is beside the code (`design/hub/`, with `SPEC.md`), and
//! these tests hold the hub to it: the three colourways' tokens as the
//! artboards have them, the labelled sidebar with its Personal group and
//! Settings at the foot, the breadcrumb, Home's Brief carrying what's waiting
//! on you, Outstanding's Tried / Stopped / Needs, and Now as a stream. A merge
//! that brings the mock-up back fails here, by name.

use atlas::dash::{Card, Layout, Span};
use atlas::hub::{self, Deck, Dot, Glance, Mark, Page};

fn design(path: &str) -> String {
    std::fs::read_to_string(format!("design/hub/{path}")).unwrap_or_else(|_| panic!("design/hub/{path}"))
}

#[test]
fn the_locked_design_is_kept_beside_the_code_and_the_mock_up_is_marked_superseded() {
    let spec = design("SPEC.md");
    assert!(spec.contains("locked with Eric, 20–21 Sep 2026"));
    assert!(spec.contains("**not** the design"), "the spec says which file is not the design");
    let canvas = design("locked-2026-09-21/canvas.json");
    assert!(canvas.contains("\"title\": \"Atlas Hub — Command Deck\""));
    // Every artboard the canvas lists is here.
    let boards: Vec<&str> = canvas
        .split("\"order\"")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap()
        .split('"')
        .filter(|s| s.ends_with(".dc.html"))
        .collect();
    assert_eq!(boards.len(), 30, "the locked canvas has thirty artboards");
    for b in boards {
        assert!(std::path::Path::new(&format!("design/hub/locked-2026-09-21/{b}")).exists(), "{b} is missing");
    }
    assert!(!std::path::Path::new("design/command-deck.html").exists(), "the mock-up is not at the top of design/ any more");
    assert!(std::path::Path::new("design/superseded/command-deck.html").exists(), "it is kept, as history");
}

/// A token as the artboard writes it: `--name:#value`.
fn token(css: &str, name: &str) -> String {
    let at = css.find(&format!("--{name}:")).unwrap_or_else(|| panic!("--{name} not in the artboard"));
    css[at + name.len() + 3..].split(|c| c == ';' || c == '}').next().unwrap().trim().to_lowercase()
}

/// Artboard text colours darkened for contrast, (artboard, hub). SPEC.md.
const WCAG_ADJUSTED: &[(&str, &str)] = &[("#787774", "#5f5d58"), ("#9b9a97", "#66645f"), ("#616c78", "#8e99a5")];

/// WCAG 2 contrast ratio between two `#rrggbb` colours.
fn contrast(a: &str, b: &str) -> f64 {
    let lum = |h: &str| {
        let c = |i: usize| {
            let v = u8::from_str_radix(&h[1 + i * 2..3 + i * 2], 16).unwrap() as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * c(0) + 0.7152 * c(1) + 0.0722 * c(2)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn the_hub_wears_the_three_colourways_exactly_as_the_artboards_have_them() {
    let page = hub::shell("Atlas", "<p>x</p>").to_lowercase();
    let root = &page[page.find(":root{").unwrap()..];
    let root = &root[..root.find('}').unwrap()];
    let dark = &page[page.find(":root[data-theme=dark]{").unwrap()..];
    let dark = &dark[..dark.find('}').unwrap()];
    for (board, css, pairs) in [
        ("Main.dc.html", root, [("bg", "bg"), ("side", "side"), ("ink", "ink"), ("accent", "accent"), ("edge", "edge"), ("dim", "ink-dim"), ("faint", "ink-faint")]),
        ("Home-Dark.dc.html", dark, [("bg", "bg"), ("side", "side"), ("ink", "ink"), ("accent", "accent"), ("edge", "edge"), ("dim", "ink-dim"), ("faint", "ink-faint")]),
    ] {
        let art = design(&format!("locked-2026-09-21/{board}"));
        let ground = token(css, "bg");
        for (theirs, ours) in pairs {
            let (want, got) = (token(&art, theirs), token(css, ours));
            if want == got {
                continue;
            }
            // The one allowed difference (26 Sep): a text colour darkened in
            // its own hue to reach WCAG 2.2's 4.5:1 on its ground, written
            // down in design/hub/SPEC.md. Anything else is drift.
            assert!(
                WCAG_ADJUSTED.contains(&(want.as_str(), got.as_str())),
                "{board}: --{theirs} is {want} on the artboard but {got} in the hub, and that isn't a recorded contrast fix"
            );
            assert!(contrast(&got, &ground) >= 4.5, "{board}: --{ours} {got} still under 4.5:1");
            assert!(contrast(&want, &ground) < 4.5, "{board}: --{theirs} {want} passed already; keep the artboard's");
        }
    }
    // Warm Paper is the base: a page with no attributes at all is the design.
    assert!(root.contains("--bg:#ffffff") && root.contains("--accent:#d9730d") && root.contains("--ink:#37352f"));
    // Access, colour-blind safe.
    let access = design("locked-2026-09-21/Home-Access.dc.html");
    assert_eq!(token(&access, "accent"), "#0072b2");
    assert!(page.contains(":root[data-theme=access],:root[data-cvd=deuter],:root[data-cvd=tritan]{\n  --accent:#0072b2"));
    // And none of the mock-up's signature is left in the tokens.
    assert!(!page.contains("--bg:#0b0e13") && !page.contains("--accent:#f0a24b"), "the mock-up's slate and ember are gone");
}

#[test]
fn every_page_has_the_designs_labelled_sidebar_and_breadcrumb() {
    let html = hub::shell_at(Some(Page::Calendar), "Calendar", "<p>x</p>");
    assert!(html.contains("<nav class=sidebar aria-label='Atlas'>"), "the sidebar, not the mock-up's icon rail");
    assert!(!html.contains("class=rail"), "no icon rail");
    assert!(html.contains("<span class=owner>Atlas</span>"), "the brand");
    assert!(html.contains(">Search<kbd class=ck>Ctrl K</kbd></a>"));
    assert!(html.contains("<p class=grp>Personal</p>"), "the Personal group, named");
    let at = html.find("<nav class=sidebar").unwrap();
    let sidebar = &html[at..at + html[at..].find("</nav>").unwrap()];
    for p in [Page::Dashboard, Page::Now, Page::Calendar, Page::Workshop, Page::Outstanding, Page::Settings] {
        assert!(sidebar.contains(&format!("href='{}'", p.href())), "{} isn't in the sidebar", p.label());
    }
    assert!(html.contains("class='nav on' href='/hub/calendar'"), "the page you're on is marked");
    assert!(html.contains("<details class=more>"), "everything else one press away");
    // Settings at the foot, after More.
    assert!(html.rfind("href='/hub/settings'").unwrap() > html.find("<details class=more>").unwrap());
    // Every page in the hub can be reached from the sidebar.
    for (_, pages) in hub::NAV {
        for p in *pages {
            assert!(html.contains(&format!("href='{}'", p.href())), "{} can't be reached", p.label());
        }
    }
    // The top bar is the breadcrumb: "Personal / Calendar".
    assert!(html.contains("<p class=crumbs>Personal<span>/</span><b>Calendar</b></p>"));
    // The names the design uses.
    assert_eq!(Page::Dashboard.label(), "Home");
    assert_eq!(Page::Now.label(), "Now");
    assert_eq!(Page::Outstanding.label(), "Outstanding");
    // Icons are drawn correctly: `r=9/>` reads as r="9/" and the circle
    // vanishes, which is how an earlier version lost its clock icon.
    assert!(!bad_close(sidebar), "an SVG attribute runs into its closing slash");
    // The phone's top bar and tabs draw their own icons too.
    assert!(!bad_close(&html), "an SVG attribute runs into its closing slash");
}

fn bad_close(s: &str) -> bool {
    s.match_indices("/>").any(|(i, _)| {
        let before = s[..i].chars().last().unwrap_or(' ');
        before != ' ' && before != '\'' && before != '"'
    })
}

#[test]
fn a_business_gets_its_own_group_in_the_sidebar_only_once_it_exists() {
    let page = hub::shell("Atlas", "<p>x</p>");
    let none = hub::with_business(page.clone(), &[]);
    assert!(!none.contains("· business"), "no business, no business section");
    let one = hub::with_business(page, &["Northwind <LLC>".to_string()]);
    assert!(one.contains("<p class=grp>Northwind &lt;LLC&gt; <small>· business</small></p>"), "named, and escaped");
    assert!(one.contains(">Shared tasks</a>"));
    let named = hub::with_owner(hub::shell("Atlas", ""), Some("Eric"));
    assert!(named.contains("<span class=owner>Eric's Atlas</span>"));
    assert_eq!(hub::with_owner(hub::shell("Atlas", ""), None), hub::shell("Atlas", ""));
}

fn deck() -> Deck {
    Deck {
        greeting: "Good evening, Eric.".into(),
        status: "On, listening".into(),
        tone: "",
        now: "Drafting the Q3 summary.".into(),
        now_sub: "Started 18:31.".into(),
        spine: vec![
            ("09:00".into(), "Dentist".into(), Mark::Done),
            ("NOW".into(), "Drafting the Q3 summary.".into(), Mark::Now),
            ("19:30".into(), "Call <with> the broker".into(), Mark::Later),
        ],
        brief: "Two things want you.".into(),
        asks: vec![
            ("Approve \"faster date parser\"".into(), "/hub/workshop".into()),
            ("Reply to the broker".into(), "/hub/workspace".into()),
        ],
        businesses: Vec::new(),
        first_run: false,
    }
}

#[test]
fn home_is_the_brief_with_whats_waiting_inside_it_then_today() {
    let html = hub::dashboard_deck(&Layout::default(), &[], false, 2, &deck());
    let brief = html.find("<section class=brief").expect("the Brief is on Home");
    let today = html.find("<section class=today>").expect("Today is on Home");
    let cards = html.find("id=cards").expect("your cards follow");
    assert!(brief < today && today < cards, "Brief, then Today, then your cards");
    // Waiting on you rides inside the Brief, as links, with the first as a button.
    let b = &html[brief..today];
    assert!(b.contains("Two things want you.") && b.contains("<ul class=asks>"));
    assert!(b.contains("<a href='/hub/workshop'>Approve &quot;faster date parser&quot;</a>"));
    assert!(b.contains("class='btn primary' href='/hub/workshop'"));
    // Not as a card of its own.
    assert!(!Layout::default().visible().iter().any(|p| p.card == Card::Outstanding), "waiting on you is not a separate panel");
    // Today: each moment's state as an icon and a word, never colour alone.
    assert!(html.contains("<div class='day done'><time>09:00</time><span class='pill ok'>"));
    assert!(html.contains("</svg>Done</span>") && html.contains("</svg>Working</span>") && html.contains("</svg>Next</span>"));
    assert!(html.contains("Call &lt;with&gt; the broker"), "escaped");
    // The front page is Personal / Home.
    assert!(html.contains("<p class=crumbs>Personal<span>/</span><b>Home</b></p>"));
    assert!(html.contains("<h1>Home</h1>"));
    // With no business, Right now sits beside Today, with the box to find anything.
    assert!(html.contains("<section class=rightnow>") && html.contains("<form class=ask method=get action='/hub/find'>"));
    assert!(html.contains("<input name=q"), "the box searches, by the field the server reads");
}

#[test]
fn a_business_shows_at_a_glance_and_first_run_is_calm_not_blank() {
    let mut d = deck();
    d.businesses = vec![Glance { name: "Northwind".into(), open: 6, people: vec![("Jordan".into(), "on the roster".into())] }];
    let html = hub::dashboard_deck(&Layout::default(), &[], false, 0, &d);
    assert!(html.contains("<section class=glance>") && html.contains("Northwind") && html.contains("<b>6</b><span>open</span>"));
    assert!(!html.contains("<section class=rightnow>"), "business at a glance takes Right now's place");
    assert!(html.contains("<span class=av>J</span>"));

    let mut first = deck();
    first.first_run = true;
    let html = hub::dashboard_deck(&Layout::default(), &[], false, 0, &first);
    // 27 Sep 2026: the steps are named for what their pages can do, and each
    // link goes where that's done (the calendar comes from the phone's app;
    // a file is given on Give, not searched for).
    // 2 Oct 2026: email and calendars both go to Connect an account, where
    // they can actually be connected (the email step said "no page yet", and
    // the calendar one pointed at installing the phone app).
    assert!(html.contains("<ol class=firststeps>") && html.contains("<b>Connect your email</b>") && html.contains("<b>Bring in your calendar</b>"));
    assert!(html.contains("href='/hub/accounts#connect'") && html.contains("href='/hub/give'"), "{html}");
    assert!(!html.contains("has no page yet"));
    assert!(!html.contains("href='/hub/find'>Give"), "Give Atlas a file opens search");
    assert!(html.contains("I run fine offline the whole time"));
}

#[test]
fn a_hub_with_no_atlas_behind_it_says_so_on_home() {
    let html = hub::dashboard_page(&Layout::default(), &[], false);
    assert!(html.contains("running, so nothing is underway."), "{html}");
    let d = Deck::not_running();
    assert_eq!(d.tone, "off");
    assert!(d.spine.is_empty() && d.asks.is_empty(), "no Atlas, no day to show — said, not invented");
    assert!(html.contains("class='live off'"));
}

#[test]
fn the_cards_under_home_start_as_projects_health_and_what_atlas_did() {
    let l = Layout::default();
    let shown: Vec<Card> = l.visible().iter().map(|p| p.card).collect();
    assert_eq!(shown, vec![Card::Projects, Card::Machine, Card::Activity]);
    assert_eq!(l.visible()[2].span, Span::Full, "what Atlas did runs wide");
    assert_eq!(l.cards.len(), Card::all().len(), "the rest are one press of Arrange away, not gone");
}

#[test]
fn outstanding_says_what_it_tried_what_stopped_it_and_what_it_needs() {
    let o = hub::Open {
        waiting: vec![("Reply to the broker".into(), "Two slots offered.".into(), "/hub/workspace".into())],
        blocked: vec![hub::Stopped {
            what: "Connect your mailbox".into(),
            tried: "Signed in over IMAP.".into(),
            stopped: "The server refused the app password.".into(),
            needs: "A fresh app password.".into(),
            area: Some("Northwind".into()),
        }],
        in_progress: vec![("Report export".into(), "Handed to a worker.".into())],
        carried: vec![("Sort the receipts".into(), 3)],
        // 2 Oct 2026: every item that can come off has its button; the
        // worker's errand is stopped rather than dropped.
        drops: hub::Drops {
            waiting: vec![Some("w:a".into())],
            blocked: vec![Some("b:1".into())],
            in_progress: vec![Some("e:4".into())],
            carried: vec![None],
        },
    };
    let html = hub::outstanding_page(&o);
    assert_eq!(html.matches("action=/hub/outstanding").count(), 3, "one button for each item with a key");
    assert!(html.contains("value='b:1'><button class=quiet aria-label='Drop it: Connect your mailbox'>Drop it</button>"), "{html}");
    assert!(html.contains(">Stop it</button>"));
    let lanes = ["Waiting on you", "Blocked", "In progress", "Carried over"];
    let mut at = 0;
    for l in lanes {
        let i = html[at..].find(&format!("{l} <span class=n>")).unwrap_or_else(|| panic!("no {l} lane in order")) + at;
        at = i;
    }
    assert!(html.contains("<span class=k>Tried</span><span class=v>Signed in over IMAP.</span>"));
    assert!(html.contains("<span class='k stopped'>Stopped</span>") && html.contains("<span class='k needs'>Needs</span>"));
    assert!(html.contains("<span class=area>Northwind</span>"));
    assert!(html.contains("3 days"));
    assert!(html.contains("Nothing here rots quietly."), "the rule is on the page");
    assert_eq!(o.count(), 4);
}

#[test]
fn now_is_a_stream_with_its_rail_not_a_log() {
    let v = hub::NowView {
        title: "Drafting the Q3 summary.".into(),
        since: "Started 18:01 · checking it worked".into(),
        steps: vec![
            (hub::Step::Plan, "One page for finance.".into()),
            (hub::Step::Delegated, "Handed the first draft to a worker.".into()),
            (hub::Step::Rerouted, "The draft rounded revenue wrong; rewriting it myself.".into()),
            (hub::Step::Checked, "Totals match the sheet.".into()),
            (hub::Step::Now, "Writing the summary.".into()),
            (hub::Step::Next, "Show you the draft.".into()),
        ],
        plain_from: 0,
        spent: Some("3 min".into()),
        fallback: "I'll build it by hand from the sheet.".into(),
        paused: false,
        working: true,
        background: vec![],
        held: vec![],
        receipt: None,
    };
    let html = hub::now_page(&v);
    for l in ["◇ Plan", "⌞ Delegated", "↝ Rerouted", "✓ Checked", "▷ Now", "◷ Next"] {
        assert!(html.contains(&format!("<div class=lab>{l}</div>")), "no {l} step");
    }
    assert!(html.contains("<a class=plain href='#'>Plain</a><a class=detail href='#detailed'>Detailed</a>"));
    assert!(html.contains("Time on this") && html.contains("If this doesn't hold up"));
    assert!(html.contains("action='/hub/pause'") && html.contains("value=pause"));
    // Live without reloading the page (26 Sep: a meta refresh fails WCAG
    // 2.2.1), and the updates can be paused.
    assert!(!html.contains("http-equiv=refresh"));
    assert!(html.contains("id=livepause"));
    // Pausing from the hub reaches the server as its own action.
    let r = atlas::server::parse_request("POST /hub/pause HTTP/1.1\r\n", "what=pause").unwrap();
    assert_eq!(atlas::server::route(&r), Some(atlas::server::Action::Pause(true)));
}

#[test]
fn atlas_s_own_windows_wear_the_design_not_the_old_slate_and_mint() {
    let t = atlas::look::TOKENS;
    assert!(t.contains("--ink:#FFFFFF") && t.contains("--signal:#D9730D") && t.contains("--text:#37352F"), "Warm Paper");
    assert!(!t.contains("#6FE3C4"), "the old mint is gone");
    assert!(atlas::look::TOKENS_DARK.contains("--signal:#EB9D4A"), "Ember Dark");
    assert!(atlas::look::TOKENS_ACCESS.contains("--signal:#0072B2"), "Access");
    // Every one of Atlas's windows sets the design's look before it draws.
    for f in ["src/window.rs", "src/setupwin.rs", "src/overlaywin.rs", "src/typebox.rs"] {
        let src = std::fs::read_to_string(f).unwrap();
        // `dress` sets the colourway's visuals, and the computer's text size
        // and motion setting with them (26 Sep).
        assert!(src.contains("crate::look_paint::dress(ctx);"), "{f} draws in egui's default grey");
    }
    let tb = std::fs::read_to_string("src/typebox.rs").unwrap();
    assert!(!tb.contains("from_rgb(24, 26, 31)"), "the typing box's own dark grey is gone");
    // And the colourway a window paints follows what you chose in Settings.
    use atlas::appearance::{Appearance, Cvd, Theme};
    use atlas::look_paint::palette::{for_appearance_on, ACCESS, EMBER_DARK, WARM_PAPER};
    // On a computer set to nothing in particular.
    let for_appearance = |a: &Appearance| for_appearance_on(a, &atlas::oslook::OsLook::default());
    assert_eq!(for_appearance(&Appearance::default()), WARM_PAPER, "nothing chosen: Warm Paper");
    assert_eq!(for_appearance(&Appearance { theme: Theme::Ember, ..Default::default() }), EMBER_DARK);
    assert_eq!(for_appearance(&Appearance { theme: Theme::Access, ..Default::default() }), ACCESS);
    assert_eq!(
        for_appearance(&Appearance { theme: Theme::Ember, cvd: Cvd::Deuter, ..Default::default() }),
        ACCESS,
        "colour-blind mode wins over the colourway"
    );
    let v = atlas::look_paint::visuals();
    assert_eq!(v.panel_fill, atlas::look_paint::colourway().ink, "egui's ground is the colourway's");
}

#[test]
fn rows_rings_and_figures_are_drawn_as_designed() {
    let r = hub::rows(&[
        (Dot::Act, "Reply <now>".into(), "2".into()),
        (Dot::Proposed, "Approve it".into(), "Atlas".into()),
        (Dot::Record, "Backed up".into(), "13:40".into()),
    ]);
    assert!(r.contains("<span class='dot'></span><span class=t>Reply &lt;now&gt;</span><span class=meta>2</span>"));
    assert!(r.contains("dot cool") && r.contains("dot mute"));
    assert_eq!(hub::rows(&[]), "");

    assert!(hub::gauge("7.1 GB", "memory, 70% free", 0.3).contains("var(--good)"));
    assert!(hub::gauge("128 GB", "disk, 15% free", 0.85).contains("var(--accent)"));
    assert!(hub::gauge("128 GB", "disk, 5% free", 0.95).contains("var(--hot)"));
    assert!(hub::gauge("x", "y", 0.5).contains("stroke-dasharray='47 95'"));

    let f = hub::deck_figures(&[("To act", 3, Dot::Act), ("Proposed", 2, Dot::Proposed)]);
    assert!(f.contains("<div class='figure warm'><b>3</b><span>To act</span>"));
    assert!(f.contains("<div class='figure cool'><b>2</b><span>Proposed</span>"));
}

#[test]
fn the_appearance_menu_the_second_chat_built_is_back_and_needs_no_script() {
    // Seen only in the second chat's rendered preview (atlas-hub-preview.html):
    // an "Aa" menu for theme, text size, high contrast and reduce motion. The
    // code behind it reached no tree. Rebuilt here as links Atlas answers, so
    // the choice is kept by Atlas and the phone and laptop agree.
    let page = hub::shell("Atlas", "<p>x</p>");
    assert!(page.contains("aria-label='Appearance and access'>Aa</summary>"));
    for (what, to) in [("theme", "paper"), ("theme", "light"), ("theme", "dark"), ("theme", "auto"), ("text", "normal"),
        ("text", "large"), ("text", "larger"), ("contrast", "high"), ("contrast", "normal"),
        ("motion", "off"), ("motion", "on")] {
        assert!(page.contains(&format!("/hub/appearance?set={what}&amp;to={to}")), "no way to set {what} to {to}");
        let mut a = hub::Appearance::default();
        assert!(a.choose(what, to), "{what}={to} is refused");
    }
    assert!(!page.to_lowercase().contains("<script"), "appearance needs no script");

    let mut a = hub::Appearance::default();
    assert_eq!(hub::with_appearance(page.clone(), &a), page, "nothing chosen in the menu leaves Settings' colourway");
    a.choose("theme", "light");
    a.choose("text", "larger");
    a.choose("contrast", "high");
    a.choose("motion", "off");
    assert!(hub::with_appearance(page.clone(), &a)
        .starts_with("<!doctype html><html lang=en data-theme=light data-text=larger data-contrast=high data-motion=off"));
    // Back again. Paper is the default (Eric, 26 Sep), so choosing it clears.
    a.choose("theme", "paper");
    a.choose("text", "normal");
    a.choose("contrast", "normal");
    a.choose("motion", "on");
    assert_eq!(a, hub::Appearance::default());
    // A made-up choice changes nothing.
    assert!(!a.choose("theme", "purple") && !a.choose("colour", "red"));
    assert_eq!(a, hub::Appearance::default());
}

#[test]
fn every_page_tells_the_truth_about_whats_waiting() {
    let page = hub::shell_at(Some(Page::Settings), "Settings", "<p>x</p>");
    assert!(page.contains(">Nothing waiting</a>"));
    let told = hub::with_waiting(page.clone(), 2);
    assert!(told.contains("<span class=dot></span>2 waiting</a>") && !told.contains("Nothing waiting"));
    assert_eq!(hub::with_waiting(page.clone(), 0), page);
}

#[test]
fn a_search_reaches_the_server_with_its_words() {
    // It didn't: the route read the query out of the path, which had already
    // been stripped of it, so every search arrived empty.
    let r = atlas::server::parse_request("GET /hub/find?q=my+calendar HTTP/1.1\r\nHost: x\r\n", "").unwrap();
    assert_eq!(atlas::server::route(&r), Some(atlas::server::Action::Find("my calendar".into())));
    let r = atlas::server::parse_request("GET /hub/appearance?set=text&to=large HTTP/1.1\r\n", "").unwrap();
    assert_eq!(
        atlas::server::route(&r),
        Some(atlas::server::Action::Appearance { what: "text".into(), to: "large".into() })
    );
}

#[test]
fn the_wall_clock_is_the_one_times_are_shown_in() {
    use atlas::localclock::{day, hhmm, hour};
    assert_eq!(hhmm(86_400 + 14 * 3600 + 2 * 60, 0), "14:02");
    assert_eq!(hhmm(86_400 + 14 * 3600 + 2 * 60, -4 * 3600), "10:02", "New York in summer");
    assert_eq!(hour(86_400 + 23 * 3600, 2 * 3600), 1, "past midnight on the wall");
    assert_eq!(day(86_400 + 23 * 3600, 2 * 3600), 2, "and so the next day");
    assert_eq!(day(86_400 + 1 * 3600, -2 * 3600), 0, "and the day before, going west");
}

#[test]
fn the_appearance_becomes_exactly_the_attributes_the_css_reads() {
    let mut a = hub::Appearance::default();
    assert_eq!(a.attrs(), "", "nothing chosen means nothing written on the page");
    a.choose("theme", "dark");
    a.choose("text", "larger");
    a.choose("contrast", "high");
    a.choose("motion", "off");
    assert_eq!(a.attrs(), " data-theme=dark data-text=larger data-contrast=high data-motion=off");
    // A value that slipped past `choose` still can't write itself onto the page.
    // Following the system has to be written, to beat the Warm Paper default.
    let mut auto = hub::Appearance::default();
    auto.choose("theme", "auto");
    assert_eq!(auto.attrs(), " data-theme=auto");
    a.theme = "x onload=alert(1)".into();
    assert!(!a.attrs().contains("onload"));
}

#[test]
fn warm_paper_is_the_default_and_the_settings_change_it() {
    // Warm Paper is the lead colourway of the locked design (and Eric's
    // ruling of 26 Sep): the default, with the settings to change it.
    let page = hub::shell("Atlas", "<p>x</p>");
    assert!(page.contains("<html lang=en data-theme=paper"), "Warm Paper when nothing is chosen");
    assert!(page.contains("class=wrap") && page.contains("Find anything"), "on the design's layout");
    for v in ["paper", "light", "dark", "auto"] {
        assert!(page.contains(&format!("/hub/appearance?set=theme&amp;to={v}")), "the Aa menu offers {v}");
    }
    // "Auto" follows the system: Ember Dark at night, Warm Paper by day.
    assert!(page.contains("@media (prefers-color-scheme:dark){\n  :root[data-theme=auto]{"));
    // High contrast keeps Warm Paper's ink dark rather than turning it white.
    assert!(page.contains(":root[data-contrast=high]:not([data-theme=dark]){ --ink:#1f1d19"));

    // Settings has the rest: colourway, accent, colour-blind mode, density.
    let s = hub::settings_page(&atlas::settings::registry(&Default::default()));
    assert!(s.contains("id='how-it-looks'"));
    for (k, v) in [("theme", "warm"), ("theme", "ember"), ("theme", "system"), ("theme", "access"),
        ("accent", "teal"), ("cvd", "deuter"), ("density", "compact")] {
        assert!(s.contains(&format!("/hub/appearance?set=look.{k}&amp;to={v}")), "Settings can't set {k} to {v}");
    }
    assert!(s.contains("class=on aria-current=true href='/hub/appearance?set=look.theme&amp;to=warm'"),
        "Warm Paper is marked as the one in use");
    assert!(!s.to_lowercase().contains("<script"));

    // The route carries a Settings choice through unchanged.
    let r = atlas::server::parse_request("GET /hub/appearance?set=look.accent&to=teal HTTP/1.1\r\n", "").unwrap();
    assert_eq!(atlas::server::route(&r),
        Some(atlas::server::Action::Appearance { what: "look.accent".into(), to: "teal".into() }));
    // Only `look.` choices reach the stored record; anything else is the menu's.
    assert!(atlas::appearance::choose("theme", "ember").is_none());
}
