//! The daemon's side of `hunt`: the daily read, what you say to it, the
//! lines in the morning brief, and the Opportunities page.
//!
//! Free functions taking the daemon, so every call site names this module
//! (the wiring guards count modules by what calls them).
//!
//! The reading happens on its own thread, one source after another with a
//! pause between requests, and the tick only collects what came back -- a
//! slow site never holds up anything you asked for. Nothing here applies,
//! replies, contacts anyone or spends: the three things you can do to a
//! found item are read more, say no, and keep it.

use crate::daemon::Daemon;
use crate::hub::{self, esc, Page};
use crate::hunt::{self, Ask, HuntState, Interests, Said, Source, Took};
use crate::server::Reply;

/// The pause between two requests of one read.
const SPACING: std::time::Duration = std::time::Duration::from_millis(1500);

/// What the running Atlas holds between ticks.
#[derive(Default)]
pub struct Live {
    state: Option<HuntState>,
    in_flight: Option<std::sync::mpsc::Receiver<Vec<(Source, Took)>>>,
    /// Ids of the list last shown, in order. Which list a number means is
    /// `workday`'s follow-up (`Follow::Opportunities`), like every other
    /// numbered list.
    shown: Vec<String>,
    /// The one last asked about ("tell me more about 3"): what "that one"
    /// and "that kind" mean.
    last_more: Option<String>,
}

fn state<'a>(d: &'a mut Daemon<'_>) -> &'a mut HuntState {
    if d.workday.hunt.state.is_none() {
        d.workday.hunt.state = Some(d.store.load(hunt::FILE));
    }
    d.workday.hunt.state.as_mut().expect("loaded")
}

fn keep(d: &mut Daemon) -> Result<(), String> {
    let Some(s) = d.workday.hunt.state.as_ref() else { return Ok(()) };
    d.store.save(hunt::FILE, s).map_err(|e| e.to_string())
}

/// A request, made. Https only for the public sources (plain http is only
/// for a SearXNG on your own network), three redirects at most and never
/// from https down to http.
fn fetch_listing(a: &Ask) -> Result<String, String> {
    let timeout = std::time::Duration::from_secs(20);
    let headers = [("User-Agent", hunt::USER_AGENT)];
    match a {
        Ask::PostJson { host, path, body } => {
            let r = crate::http::https_post_json(host, path, body, None, timeout).map_err(|e| e.to_string())?;
            if r.ok() { Ok(r.body) } else { Err(format!("{} answered {}", a.shown(), r.status)) }
        }
        Ask::Get { https, host, path, secret } => {
            let (mut https, mut host, mut path) = (*https, host.clone(), path.clone());
            for _ in 0..4 {
                let r = if https {
                    crate::http::https_get_with(&host, &path, &headers, timeout)
                } else {
                    crate::http::get(&host, &path, timeout)
                }
                .map_err(|e| e.to_string())?;
                if (300..400).contains(&r.status) {
                    let here = format!("{}://{host}{path}", if https { "https" } else { "http" });
                    let loc = r.location.ok_or_else(|| format!("{host}: a redirect with nowhere to go"))?;
                    let next = crate::feeds::absolute_link(&here, &loc).ok_or_else(|| format!("{host}: a redirect I couldn't follow"))?;
                    let (h2, host2, path2) = crate::feeds::split_feed_url(&next).ok_or_else(|| format!("{host}: a redirect I couldn't follow"))?;
                    if https && !h2 {
                        return Err(format!("{host} redirects to plain http, which I won't follow"));
                    }
                    (https, host, path) = (h2, host2, path2);
                    continue;
                }
                if !r.ok() {
                    // Said the way a status line shows a request: a key in
                    // the address is never repeated.
                    let asked = Ask::Get { https, host: host.clone(), path: path.split('?').next().unwrap_or("").to_string(), secret: *secret };
                    return Err(format!("{} answered {}", asked.shown(), r.status));
                }
                return Ok(r.body);
            }
            Err(format!("{host}: too many redirects"))
        }
    }
}

// ---------------------------------------------------------------- the tick

/// Collect a finished read; start today's when it's due. Called from the
/// workday tick, so it stops when Atlas is paused.
pub fn tick(d: &mut Daemon, t: u64, online: bool) {
    tick_with(d, t, online, fetch_listing, SPACING)
}

/// `tick`, with the requests made by `get` and spaced by `spacing` -- the
/// same path, so a test drives the real schedule, budget and thread with
/// saved answers instead of the network.
pub fn tick_with(d: &mut Daemon, t: u64, online: bool, get: fn(&Ask) -> Result<String, String>, spacing: std::time::Duration) {
    if let Some(rx) = &d.workday.hunt.in_flight {
        match rx.try_recv() {
            Ok(results) => {
                d.workday.hunt.in_flight = None;
                let you = Interests::from_facts(&d.facts);
                let s = state(d);
                for (src, took) in results {
                    s.note_source(src, &took, t);
                    s.merge(took.found, &you, t);
                }
                if let Err(e) = keep(d) {
                    d.log.warn(&format!("opportunities: couldn't keep what I found: {e}"));
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => d.workday.hunt.in_flight = None,
        }
    }
    let cfg = d.tools_cfg().hunt.clone();
    if !cfg.enabled || d.handover().stance.handed_over() {
        return;
    }
    let off = crate::localclock::offset_secs();
    let day = crate::localclock::day(t, off);
    let hour = ((t as i64 + off).rem_euclid(86_400) / 3600) as u32;
    if hour < cfg.from_hour {
        return;
    }
    // Looked at hourly at most; each source decides for itself if it's due.
    if t.saturating_sub(state(d).last_run) < 3600 {
        return;
    }
    let (y, m, dom) = crate::hubpages::ymd(day);
    let searxng = d.tools_cfg().research.searxng_url.trim().to_string();
    let sources = cfg.sources();

    // Your own mail first: on this machine, no request made.
    if sources.contains(&Source::Mail) {
        let last = state(d).sources.get("mail").map(|s| s.last_read).unwrap_or(0);
        if hunt::source_due(Source::Mail, last, t, dom) {
            let book: crate::mailbook::MailBook = d.store.load(crate::mailbook::MailBook::FILE);
            let since = if last == 0 { t.saturating_sub(7 * 86_400) } else { last };
            let found = hunt::alerts_in_mail(&book.letters, since);
            let took = Took { found, requests: 0, error: None };
            let you = Interests::from_facts(&d.facts);
            let s = state(d);
            s.note_source(Source::Mail, &took, t);
            s.merge(took.found, &you, t);
        }
    }

    let s = state(d);
    s.roll_day(day);
    s.last_run = t;
    let mut left = s.left_today(&cfg);
    let mut plan = Vec::new();
    let mut over: Vec<Source> = Vec::new();
    for src in sources.iter().copied().filter(|s| *s != Source::Mail) {
        let last = s.sources.get(src.key()).map(|x| x.last_read).unwrap_or(0);
        if !hunt::source_due(src, last, t, dom) {
            // Not due: nothing to do, and nothing to say.
            continue;
        }
        let c = hunt::cost(src, &cfg, !searxng.is_empty());
        if c == 0 && src != Source::Search {
            continue;
        }
        if c > left {
            over.push(src);
            continue;
        }
        left -= c;
        plan.push(src);
    }
    for src in over {
        let st = s.sources.entry(src.key().to_string()).or_default();
        st.last_error = format!("skipped today: it would go past your {} requests a day", cfg.budget());
    }
    // The SAM.gov key, if SAM is due: out of the vault, never into a log.
    let mut sam_key = String::new();
    if plan.contains(&Source::Sam) {
        match d.vault.get(&cfg.sam_key_vault, t) {
            Ok(k) => sam_key = k,
            Err(why) => {
                plan.retain(|s| *s != Source::Sam);
                let st = state(d).sources.entry("sam".into()).or_default();
                st.last_read = t;
                st.last_error = format!("couldn't read the SAM.gov key from the vault: {why}");
            }
        }
    }
    let _ = keep(d);
    if plan.is_empty() || !online {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("atlas-hunt".into()).spawn(move || {
        let first = std::cell::Cell::new(true);
        let spaced = |a: &Ask| {
            if !first.replace(false) {
                std::thread::sleep(spacing);
            }
            get(a)
        };
        let mut out = Vec::new();
        for src in plan {
            out.push((src, hunt::read(src, &cfg, &spaced, &sam_key, &searxng, t, (y, m, dom))));
        }
        let _ = tx.send(out);
    });
    if spawned.is_ok() {
        d.workday.hunt.in_flight = Some(rx);
    }
}

// ---------------------------------------------------------------- what you say

/// How many are on the list a number would refer to now: the opportunities
/// just shown, unless another tool has shown a list since.
fn listed(d: &Daemon, t: u64) -> usize {
    match d.workday.follow(t) {
        Some(crate::workday::Follow::Opportunities(n)) => n.min(d.workday.hunt.shown.len()),
        _ => 0,
    }
}

fn show(d: &mut Daemon, ids: Vec<String>, t: u64) {
    let n = ids.len();
    d.workday.hunt.shown = ids;
    d.workday.follow_opportunities(n, t);
}

/// Add to what you look for, what you're good at, or what to skip. Said out
/// loud it adds ("look for video editing gigs" keeps what was there); the
/// page's boxes replace. Skipping a thing also takes it off what you look
/// for, so the two never disagree.
fn add_to(d: &mut Daemon, which: &str, items: Vec<String>, t: u64) -> String {
    let now = Interests::from_facts(&d.facts);
    let mut have = match which {
        hunt::FACT_SKILLS => now.skills,
        hunt::FACT_AVOID => now.avoid,
        _ => now.want,
    };
    for i in items {
        if !have.iter().any(|h| h.eq_ignore_ascii_case(&i)) {
            have.push(i);
        }
    }
    let fact = Interests::fact(which, &have, t);
    let summary = fact.summary.clone();
    d.facts.put(fact);
    if which == hunt::FACT_AVOID {
        let want: Vec<String> = Interests::from_facts(&d.facts).want.into_iter().filter(|w| !have.iter().any(|a| a.eq_ignore_ascii_case(w))).collect();
        if d.facts.get(hunt::FACT_WANT).is_some() {
            d.facts.put(Interests::fact(hunt::FACT_WANT, &want, t));
        }
    }
    let mut reply = match d.facts.save(&d.store) {
        Ok(()) => format!("{summary}. I'll use that from the next look."),
        Err(e) => format!("{summary} -- but I couldn't keep it: {e}"),
    };
    if !d.tools_cfg().hunt.enabled {
        reply.push_str(" Looking is off, though: say \"start hunting for opportunities\" or turn it on in Settings.");
    }
    reply
}

/// `Intent::Opportunities`: something said to the hunter, answered.
///
/// The sentence is read by `hunt::understand`, with the list just shown
/// for numbers. A bare "opportunities", or what the model passed when it
/// picked this command without one of the shapes, gets the list.
pub fn said(d: &mut Daemon, said: &str, t: u64) -> String {
    let n = listed(d, t);
    let what = match hunt::understand(said, n) {
        Some(w) => w,
        None => {
            let bare = said.trim().trim_end_matches(['?', '.', '!']).to_lowercase();
            if bare.is_empty() || bare.split_whitespace().all(|w| matches!(w, "any" | "new" | "opportunities" | "gigs" | "my" | "the" | "show" | "me" | "list" | "today")) {
                hunt::Said::List
            } else {
                return "I didn't follow that one. I can list the opportunities I found, tell you more about one by its number, \
                        drop one, or save it -- and \"look for\" or \"skip\" a kind of work, like \"look for video editing gigs\"."
                    .into();
            }
        }
    };
    heard_as(d, what, t)
}

fn heard_as(d: &mut Daemon, what: Said, t: u64) -> String {
    if d.handover().stance.handed_over() {
        return "The opportunities list is the owner's.".into();
    }
    let cfg = d.tools_cfg().hunt.clone();
    let id_at = |d: &Daemon, i: usize| {
        if i == hunt::THAT_ONE {
            d.workday.hunt.last_more.clone()
        } else {
            d.workday.hunt.shown.get(i).cloned()
        }
    };
    const WHICH: &str = "Which one? Say its number.";
    match what {
        Said::List => {
            let you = Interests::from_facts(&d.facts);
            let top: Vec<hunt::Ranked> = state(d).top(cfg.top_n.max(1) as usize, t).into_iter().cloned().collect();
            if top.is_empty() {
                let mut s = if cfg.enabled {
                    "Nothing worth your time yet.".to_string()
                } else {
                    "I'm not looking for opportunities: it's off. Say \"start hunting for opportunities\" or turn it on in Settings.".to_string()
                };
                if you.is_empty() {
                    s.push(' ');
                    s.push_str(hunt::ASK);
                    state(d).asked = true;
                    let _ = keep(d);
                }
                return s;
            }
            show(d, top.iter().map(|r| r.found.id.clone()).collect(), t);
            let mut lines: Vec<String> = top.iter().enumerate().map(|(i, r)| hunt::line(i + 1, r)).collect();
            lines.push("Say \"tell me more about 1\", \"not interested in 1\" or \"save 1\" -- or any other number on the list.".into());
            lines.join("\n")
        }
        Said::More(i) => {
            let Some(id) = id_at(d, i) else { return WHICH.into() };
            // Talking about it keeps the list's numbers live.
            let shown = d.workday.hunt.shown.len();
            d.workday.follow_opportunities(shown, t);
            match state(d).find(&id).cloned() {
                Some(r) => {
                    d.workday.hunt.last_more = Some(id);
                    hunt::more(&r)
                }
                None => "That one's gone from the list.".into(),
            }
        }
        Said::NotThatKind(i) => {
            let Some(id) = id_at(d, i) else { return WHICH.into() };
            match state(d).not_that_kind(&id) {
                Some(title) => {
                    d.workday.hunt.shown.retain(|x| *x != id);
                    match keep(d) {
                        Ok(()) => format!("Dropped \"{title}\", and I'll hold back anything like it from now on."),
                        Err(e) => format!("I dropped it, but couldn't keep that: {e}"),
                    }
                }
                None => "That one's gone from the list.".into(),
            }
        }
        Said::NotInterested(i) => {
            let Some(id) = id_at(d, i) else { return WHICH.into() };
            match state(d).not_interested(&id) {
                Some(title) => {
                    d.workday.hunt.shown.retain(|x| *x != id);
                    let _ = keep(d);
                    format!("Dropped \"{title}\". I'll hold back ones like it.")
                }
                None => "That one's gone from the list.".into(),
            }
        }
        Said::Save(i) => {
            let Some(id) = id_at(d, i) else { return WHICH.into() };
            match state(d).save(&id) {
                Some(f) => {
                    d.workday.hunt.shown.retain(|x| *x != id);
                    let kept = keep(d);
                    match kept {
                        Ok(()) => format!("Saved \"{}\". It's on the Opportunities page; nothing's been sent.", f.title),
                        Err(e) => format!("I couldn't keep that: {e}"),
                    }
                }
                None => "That one's gone from the list.".into(),
            }
        }
        Said::Saved => {
            let saved = state(d).saved.clone();
            if saved.is_empty() {
                "You haven't saved any.".into()
            } else {
                saved.iter().rev().take(8).map(|f| format!("{} — {}", f.title, f.link)).collect::<Vec<_>>().join("\n")
            }
        }
        Said::LookFor(v) => add_to(d, hunt::FACT_WANT, v, t),
        Said::Skills(v) => add_to(d, hunt::FACT_SKILLS, v, t),
        Said::Avoid(v) => add_to(d, hunt::FACT_AVOID, v, t),
        Said::Start | Said::Stop => {
            let on = what == Said::Start;
            let mut reply = if on {
                format!(
                    "Looking once a day from {}, at most {} requests a day. I only read; I never apply or reply.",
                    cfg.sources().iter().map(|s| s.plain()).collect::<Vec<_>>().join(", "),
                    cfg.budget()
                )
            } else {
                "Stopped looking for opportunities. What I found is still on the page.".to_string()
            };
            match d.settings_dir() {
                Some(dir) => {
                    let mut settings = crate::settings::registry(&d.tools_cfg());
                    let kept = settings.set_and_keep("hunt.enabled", if on { "on" } else { "off" }, &dir);
                    let warned: Vec<String> = d.pick_up_settings().into_iter().filter(|l| l.starts_with("I couldn't read")).collect();
                    if !warned.is_empty() {
                        reply = warned.join(" ");
                    }
                    if kept.starts_with("I couldn't keep") || kept.starts_with("no setting") {
                        reply = kept;
                    }
                }
                None => reply = "I can't change that from here -- it's on the Settings page, under Looking for opportunities.".into(),
            }
            if on && Interests::from_facts(&d.facts).is_empty() {
                reply.push(' ');
                reply.push_str(hunt::ASK);
                state(d).asked = true;
                let _ = keep(d);
            }
            reply
        }
    }
}

// ---------------------------------------------------------------- the brief

/// The morning brief's lines: the best few, with why, and -- once -- the
/// question about what to look for.
pub fn brief_items(d: &mut Daemon, t: u64) -> Vec<crate::brief::Item> {
    use crate::brief::{Item, Outcome, Source as From, Weight};
    let cfg = d.tools_cfg().hunt.clone();
    if !cfg.enabled || d.handover().stance.handed_over() {
        return Vec::new();
    }
    let item = |id: String, subject: String| Item {
        id,
        source: From::Day,
        from: "Opportunity".into(),
        subject,
        weight: Weight::Info,
        outcome: Outcome::Yours,
        draft: None,
        conflicts_with: None,
    };
    let mut out = Vec::new();
    if Interests::from_facts(&d.facts).is_empty() && !state(d).asked {
        state(d).asked = true;
        let _ = keep(d);
        out.push(item("opportunity:ask".into(), hunt::ASK.into()));
    }
    let top: Vec<hunt::Ranked> = state(d).top(cfg.top_n.max(1) as usize, t).into_iter().cloned().collect();
    for (i, r) in top.iter().enumerate() {
        out.push(item(format!("opportunity:{}", r.found.id), hunt::line(i + 1, r)));
    }
    // Numbered as the brief numbers them, so "save 2" after it works --
    // unless another tool's list is live, whose numbers stay its own.
    if !top.is_empty() && d.workday.follow(t).is_none() {
        show(d, top.iter().map(|r| r.found.id.clone()).collect(), t);
    }
    out
}

// ---------------------------------------------------------------- the page

fn button(what: &str, id: &str, label: &str) -> String {
    format!(
        "<form method=post action='/hub/opportunities' class=inline><input type=hidden name=what value={what}>\
         <input type=hidden name=id value='{}'><button>{}</button></form>",
        esc(id),
        esc(label)
    )
}

/// The Opportunities page: the list, what each rests on, the saved ones,
/// what you told it to look for, and how each source last answered.
pub fn opportunities_page(d: &mut Daemon, fields: &[(String, String)]) -> String {
    let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let now = crate::store::now();
    let cfg = d.tools_cfg().hunt.clone();
    let you = Interests::from_facts(&d.facts);
    let open = get("more");
    let s = state(d).clone();
    let mut body = String::new();
    if let Some(n) = get("said").filter(|n| !n.trim().is_empty()) {
        body.push_str(&format!("<p class=notice role=status>{}</p>", esc(&n)));
    }
    if !cfg.enabled {
        body.push_str(
            "<p class=note>Looking is off. Turn on <b>Opportunity hunting</b> in Settings (Looking for opportunities), \
             or say \"start hunting for opportunities\". It reads public listings once a day and never applies, replies or pays for anything.</p>",
        );
    }
    body.push_str("<h2>Worth a look</h2>");
    let top = s.top(20, now);
    if top.is_empty() {
        body.push_str("<p class=empty>Nothing yet. Once it has looked, the best ones land here, with why.</p>");
    } else {
        body.push_str("<ol class=plainlist>");
        for r in top {
            let f = &r.found;
            body.push_str(&format!(
                "<li><span><b>{}</b> <span class=meta>{} · {}</span></span>",
                esc(&f.title),
                esc(f.kind.plain()),
                esc(f.source.plain())
            ));
            if let Some(w) = r.why.first() {
                body.push_str(&format!("<span class=note>{}</span>", esc(w)));
            }
            if open.as_deref() == Some(f.id.as_str()) {
                body.push_str(&format!("<pre class=detail>{}</pre>", esc(&hunt::more(r))));
            } else {
                body.push_str(&format!(
                    "<a href='{}?more={}'>Tell me more</a>",
                    Page::Opportunities.href(),
                    crate::research::urlencode(&f.id)
                ));
            }
            body.push_str(&button("nope", &f.id, "Not interested"));
            body.push_str(&button("kind", &f.id, "None like this"));
            body.push_str(&button("save", &f.id, "Save it"));
            body.push_str("</li>");
        }
        body.push_str("</ol>");
    }
    body.push_str("<h2>Saved</h2>");
    if s.saved.is_empty() {
        body.push_str("<p class=empty>None saved.</p>");
    } else {
        body.push_str("<ul class=plainlist>");
        for f in s.saved.iter().rev() {
            let link = if f.link.starts_with("https://") || f.link.starts_with("http://") {
                format!(" <a href='{}' rel=noreferrer target=_blank>open</a>", esc(&f.link))
            } else {
                String::new()
            };
            body.push_str(&format!("<li><span>{}</span><span class=meta>{}</span>{link}</li>", esc(&f.title), esc(f.source.plain())));
        }
        body.push_str("</ul>");
    }
    body.push_str(&format!(
        "<h2>What to look for</h2><form method=post action='/hub/opportunities'><input type=hidden name=what value=interests>\
         <label for=huntwant>Kinds of opportunity, comma-separated</label><input id=huntwant name=want autocomplete=off value='{}'>\
         <label for=huntskills>Your skills</label><input id=huntskills name=skills autocomplete=off value='{}'>\
         <label for=huntavoid>Skip anything about</label><input id=huntavoid name=avoid autocomplete=off value='{}'>\
         <button class=primary>Keep these</button></form>",
        esc(&you.want.join(", ")),
        esc(&you.skills.join(", ")),
        esc(&you.avoid.join(", "))
    ));
    body.push_str("<h2>Sources</h2><ul class=plainlist>");
    let on = cfg.sources();
    for src in hunt::ALL_SOURCES {
        let st = s.sources.get(src.key());
        let state_line = match (on.contains(src), st) {
            (false, _) => "not picked".to_string(),
            (true, None) => "not read yet".to_string(),
            (true, Some(x)) if !x.last_error.is_empty() => format!("last time: {}", x.last_error),
            (true, Some(x)) => format!("{} found last time", x.last_count),
        };
        body.push_str(&format!("<li><span>{}</span><span class=meta>{}</span></li>", esc(src.plain()), esc(&state_line)));
    }
    body.push_str(&format!(
        "</ul><p class=note>{} of {} requests used today. Change the sources, feeds and searches in Settings.</p>",
        s.requests_today,
        cfg.budget()
    ));
    hub::shell_at(Some(Page::Opportunities), "Opportunities", &body)
}

/// The page's buttons.
pub fn post(d: &mut Daemon, fields: &[(String, String)]) -> Reply {
    let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let now = crate::store::now();
    let id = get("id");
    let said = match get("what").as_str() {
        "nope" => match state(d).not_interested(&id) {
            Some(t) => {
                let _ = keep(d);
                format!("Dropped \"{t}\". I'll hold back ones like it.")
            }
            None => "That one's already gone.".into(),
        },
        "kind" => match state(d).not_that_kind(&id) {
            Some(t) => match keep(d) {
                Ok(()) => format!("Dropped \"{t}\", and I'll hold back anything like it from now on."),
                Err(e) => format!("I dropped it, but couldn't keep that: {e}"),
            },
            None => "That one's already gone.".into(),
        },
        "save" => match state(d).save(&id) {
            Some(f) => match keep(d) {
                Ok(()) => format!("Saved \"{}\".", f.title),
                Err(e) => format!("I couldn't keep that: {e}"),
            },
            None => "That one's already gone.".into(),
        },
        "interests" => {
            for (field, which) in [("want", hunt::FACT_WANT), ("skills", hunt::FACT_SKILLS), ("avoid", hunt::FACT_AVOID)] {
                let v = hunt::list(&get(field));
                // An emptied box clears what was there; one never filled in
                // adds nothing.
                if v.is_empty() && d.facts.get(which).is_none() {
                    continue;
                }
                d.facts.put(Interests::fact(which, &v, now));
            }
            match d.facts.save(&d.store) {
                Ok(()) => "Kept. I'll use them from the next look.".to_string(),
                Err(e) => format!("I couldn't keep them: {e}"),
            }
        }
        _ => "That button isn't wired to anything, so nothing changed.".into(),
    };
    hub::back_with(Page::Opportunities.href(), "", &said)
}
