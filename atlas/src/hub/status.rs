//! Gestures, Status, Outstanding, Now, lists and forms.
//!
//! Moved out of `hub.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// Every gesture, drawn from its own definition.
///
/// Eric asked for a way to be reminded, and specifically **not** a recording
/// of him doing them — he is handing instances of this to friends, and a video
/// of the author waving at a webcam is both odd to ship and wrong the moment
/// anybody changes a gesture.
///
/// So every card here is generated from the same tests the recogniser
/// evaluates. It cannot drift, and teaching a new gesture adds its card with
/// no second step anyone has to remember.
pub fn gestures_page(
    known: &[crate::handshape::Gesture],
    undecided: &[(&'static str, &'static str)],
    enabled: bool,
    may_approve: bool,
) -> String {
    let mut body = String::new();

    // Say plainly, at the top, whether hand control is actually on. "How do I
    // make this shape" and "is this even watching right now" are different
    // questions, and someone opening this page could be asking either — so the
    // page answers both, rather than only teaching gestures that may be off.
    let (dot, word, msg) = if enabled {
        (
            "var(--good)",
            "on",
            "Your camera reads gestures on this machine — nothing leaves it.",
        )
    } else {
        (
            "var(--ink-faint)",
            "off",
            "Turn it on to answer and drive Atlas with your hands. The gestures \
             below still work as a reference.",
        )
    };
    body.push_str(&format!(
        "<div class=state style=\"display:flex;align-items:center;gap:10px;\
         background:var(--raise);border:1px solid var(--edge);border-radius:10px;\
         padding:12px 14px;margin:0 0 14px\">\
         <span style=\"width:9px;height:9px;border-radius:99px;background:{dot};\
         flex:none\"></span><b>Hand control is {word}</b>\
         <span style=\"color:var(--ink-dim);font-size:13.5px\">{msg}</span>\
         <a style=\"margin-left:auto\" href=\"/hub/settings#what-it-can-see\">Change</a></div>"
    ));

    // The safety rule, read from the setting rather than asserted here: a
    // gesture can say no, but by default it cannot approve something
    // irreversible. If someone has deliberately loosened that, say so instead.
    let safety = if may_approve {
        "You've allowed gestures to approve anything, including irreversible \
         actions like sending or deleting."
    } else {
        "A gesture can answer or dismiss, but it never approves something \
         irreversible — sending, deleting or anything leaving your machine \
         still needs your voice or a click."
    };
    body.push_str(&format!(
        "<p class=note style=\"margin:0 0 18px\">{}</p>",
        esc(safety)
    ));

    if known.is_empty() {
        body.push_str(&nothing(
            "No gestures yet. Say \"watch my hands\", hold a shape up and tell \
             me what it should do, and it'll appear here.",
        ));
    }

    body.push_str("<div class=gestures>");
    for g in known {
        body.push_str(&format!(
            "<div class=gesture><div class=draw>{}</div>\
             <div class=meaning><b>{}</b><span class=what>{}</span></div></div>",
            crate::handshape::sketch(g),
            esc(&g.does),
            esc(&crate::handshape::how_to(g))
        ));
    }
    body.push_str("</div>");

    // Said here rather than left in a comment somewhere: a gesture Atlas
    // cannot tell from another is one you would spend a week working around
    // before finding out why.
    if !undecided.is_empty() {
        body.push_str("<h2>Two I can't tell apart</h2>");
        for (what, why) in undecided {
            body.push_str(&format!(
                "<div class=rec><h3>{}</h3><p class=cause>{}</p></div>",
                esc(what),
                esc(why)
            ));
        }
    }

    body.push_str(&more("/hub/settings#what-it-can-see", "Turn hand control on or off"));
    shell_at(Some(Page::Gestures), "Hand gestures", &body)
}

pub fn status_page(lines: &[(String, String)], changed: usize) -> String {
    let mut body = String::new();
    for (k, v) in lines {
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div><div class=what>{}</div></div>",
            esc(k),
            esc(v)
        ));
    }
    body.push_str(&format!(
        "<p class=note style=\"margin-top:20px\">{changed} setting{} changed from the defaults. \
         <a href=/hub/settings>Review</a></p>",
        if changed == 1 { "" } else { "s" }
    ));
    shell_at(Some(Page::Status), "Status", &body)
}

/// The remove button for one item, or nothing. Work already running is
/// asked to stop ("Stop it"); everything else is taken off ("Drop it").
pub(super) fn drop_button(key: Option<&Option<String>>, what: &str) -> String {
    let Some(Some(key)) = key else { return String::new() };
    let label = if key.starts_with("e:") { "Stop it" } else { "Drop it" };
    format!(
        "<form class=inline method=post action=/hub/outstanding><input type=hidden name=what value=drop>\
         <input type=hidden name=key value='{}'><button class=quiet aria-label='{label}: {}'>{label}</button></form>",
        esc(key),
        esc(what)
    )
}

impl Open {
    pub fn count(&self) -> usize {
        self.waiting.len() + self.blocked.len() + self.in_progress.len() + self.carried.len()
    }
}

pub(super) fn lane_head(icon_paths: &str, colour: &str, name: &str, n: usize) -> String {
    format!(
        "<h2><svg viewBox='0 0 24 24' fill=none stroke='var({colour})' stroke-width=2.2 aria-hidden=true>{icon_paths}</svg>\
         {name} <span class=n>{n}</span></h2>"
    )
}

/// Outstanding — the open backlog, personal and business together, most
/// pressing first. Nothing rots quietly: the rule is on the page.
pub fn outstanding_page(o: &Open) -> String {
    let mut left = String::new();
    left.push_str("<section class=lane>");
    left.push_str(&lane_head("<path d='M12 8v4l3 2'/><circle cx=12 cy=12 r=9 />", "--accent", "Waiting on you", o.waiting.len()));
    if o.waiting.is_empty() {
        left.push_str("<p class=nothing>Nothing waiting on you.</p>");
    }
    for (n, (what, why, href)) in o.waiting.iter().enumerate() {
        left.push_str(&format!(
            "<div class='item wait'><div class=t>{}</div>{}<div class=acts><a class='btn primary' href='{}'>Open it</a>{}</div></div>",
            esc(what),
            if why.is_empty() { String::new() } else { format!("<div class=d>{}</div>", esc(why)) },
            esc(href),
            drop_button(o.drops.waiting.get(n), what)
        ));
    }
    left.push_str("</section><section class=lane>");
    left.push_str(&lane_head("<path d='M12 8v4M12 16h.01'/><circle cx=12 cy=12 r=9 />", "--hot", "Blocked", o.blocked.len()));
    if o.blocked.is_empty() {
        left.push_str("<p class=nothing>Nothing is stuck.</p>");
    }
    for (n, b) in o.blocked.iter().enumerate() {
        let drop = drop_button(o.drops.blocked.get(n), &b.what);
        left.push_str(&format!(
            "<div class='item stop'>{area}<div class=t>{what}</div><div class=tsn>\
             <span class=k>Tried</span><span class=v>{tried}</span>\
             <span class='k stopped'>Stopped</span><span class=v>{stopped}</span>\
             <span class='k needs'>Needs</span><span class=v>{needs}</span></div>{acts}</div>",
            acts = if drop.is_empty() { String::new() } else { format!("<div class=acts>{drop}</div>") },
            area = b.area.as_deref().map(|a| format!("<span class=area>{}</span>", esc(a))).unwrap_or_default(),
            what = esc(&b.what),
            tried = esc(&b.tried),
            stopped = esc(&b.stopped),
            needs = esc(&b.needs),
        ));
    }
    left.push_str("</section>");

    let mut right = String::new();
    right.push_str("<section class=lane>");
    right.push_str(&lane_head("<circle cx=12 cy=12 r=9 /><path d='M12 7v5l3 2'/>", "--cool", "In progress", o.in_progress.len()));
    if o.in_progress.is_empty() {
        right.push_str("<p class=nothing>Nothing running right now.</p>");
    }
    for (n, (what, how)) in o.in_progress.iter().enumerate() {
        let drop = drop_button(o.drops.in_progress.get(n), what);
        right.push_str(&format!(
            "<div class=item><div class=t>{}</div><div class=d>{}</div>{}</div>",
            esc(what),
            esc(how),
            if drop.is_empty() { String::new() } else { format!("<div class=acts>{drop}</div>") }
        ));
    }
    right.push_str("</section><section class=lane>");
    right.push_str(&lane_head("<path d='M3 12a9 9 0 1 0 9-9'/><path d='M3 4v5h5'/>", "--ink-faint", "Carried over", o.carried.len()));
    if o.carried.is_empty() {
        right.push_str("<p class=nothing>Nothing carried over.</p>");
    } else {
        right.push_str("<div class=carried>");
        for (n, (what, days)) in o.carried.iter().enumerate() {
            right.push_str(&format!(
                "<div class=row2><span class=w>{}</span><span class=chipd>{} day{}</span>{}</div>",
                esc(what),
                days,
                if *days == 1 { "" } else { "s" },
                drop_button(o.drops.carried.get(n), what)
            ));
        }
        right.push_str("</div>");
    }
    right.push_str(
        "</section><div class=rule><b>Nothing here rots quietly.</b> Anything carried more than a week, \
         I raise in your brief. If you want something gone, press Drop it, or tell me to take it off your outstanding list, \
         and it's gone — I won't keep nagging.</div>",
    );
    let body = format!("<div class=lanes><div>{left}</div><div>{right}</div></div>");
    shell_at(Some(Page::Outstanding), "Outstanding", &body)
}

impl Step {
    pub(super) fn label(self) -> (&'static str, &'static str) {
        match self {
            Step::Plan => ("plan", "◇ Plan"),
            Step::Doing => ("doing", "▷ Doing"),
            Step::Delegated => ("doing", "⌞ Delegated"),
            Step::Rerouted => ("rerouted", "↝ Rerouted"),
            Step::Checked => ("checked", "✓ Checked"),
            Step::Checking => ("doing", "Checking"),
            Step::Finished => ("doing", "Runner ended"),
            Step::Waiting => ("doing", "◷ Waiting"),
            Step::Stuck => ("stuck", "! Stuck"),
            Step::Now => ("now", "▷ Now"),
            Step::Next => ("next", "◷ Next"),
        }
    }
}

/// A number that changes when anything the Now page shows changes: what a
/// page drawn from `v` polls `/hub/changed.json` against.
pub fn live_version(v: &NowView) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

impl TaskReceipt {
    /// Read existing progress only. Displaying a receipt never runs a step.
    pub fn from_work(w: &crate::mind::Work, paused: bool) -> Self {
        use crate::mind::Stage;
        let total = w.steps.len();
        let reported = w.steps.iter().filter(|s| s.reported_output.is_some()).count();
        let status = if w.stage == Stage::Stuck {
            "Stopped"
        } else if w.stage == Stage::Done {
            "Runner ended"
        } else if paused {
            "Paused"
        } else if w.stage == Stage::Waiting {
            if w.blocked_on.is_some() { "Waiting" } else { "Awaiting your answer or approval" }
        } else {
            "In progress"
        };
        let status = format!("{status}; {reported} of {total} steps have recorded replies");
        // Failed does not mean skipped: a retry remains at the runner's position.
        // Old records have no cursor; their first unfinished step is the safe fallback.
        let position = w.runner_position.filter(|p| *p < total)
            .or_else(|| w.steps.iter().position(|s| !s.done));
        let current = if w.stage == Stage::Done {
            "No step running.".into()
        } else {
            position.and_then(|p| w.steps.get(p)).map(|s| s.what.clone())
                .unwrap_or_else(|| "No planned step recorded.".into())
        };
        let dependency = if w.stage.finished() {
            w.blocked_on.clone().unwrap_or_else(|| "No further step will run for this task.".into())
        } else if paused {
            "Carry on when you are ready.".into()
        } else if let Some(b) = &w.blocked_on {
            b.clone()
        } else if w.stage == Stage::Waiting {
            w.thoughts.iter().rev().find(|t| t.stage == Stage::Waiting)
                .map(|t| t.text.clone()).unwrap_or_else(|| "Your answer or approval.".into())
        } else {
            position.and_then(|p| w.steps.get(p + 1))
                .map(|s| format!("Next: {} (after the current step)", s.what))
                .unwrap_or_else(|| "Finish the current step.".into())
        };
        let results = w.steps.iter().filter_map(|s| s.reported_output.as_ref()
            .map(|output| (format!("Recorded reply: {}", s.what), output.clone())))
            .collect();
        Self { task: w.asked.clone(), status, current, dependency, results }
    }

    fn html(&self) -> String {
        let mut results = String::new();
        for (step, output) in &self.results {
            results.push_str(&format!("<dt>{}</dt><dd style='white-space:pre-wrap;overflow-wrap:anywhere'>{}</dd>", esc(step), esc(output)));
        }
        if results.is_empty() {
            results.push_str("<dd>No action output recorded yet.</dd>");
        }
        format!("<section class=box aria-label='Task result receipt'><h2>Task result receipt</h2><p>{}</p><dl>\
            <dt>Status</dt><dd>{}</dd><dt>Current step</dt><dd>{}</dd><dt>Next dependency</dt><dd>{}</dd>\
            <dt>Evidence</dt><dd>Action-reported output below. Not independently checked; service acceptance does not prove recipient delivery.</dd>\
            {results}</dl></section>", esc(&self.task), esc(&self.status), esc(&self.current), esc(&self.dependency))
    }
}

pub fn now_page(v: &NowView) -> String {
    // Live without a timed reload (WCAG 2.2.1: a page that reloads itself is
    // a time limit you can't turn off). A small script fetches the page and
    // swaps the stream in place, politely announced; Pause stops it; with
    // scripts off, Refresh is a link.
    let mut body = String::new();
    let mut steps = String::new();
    if let Some(receipt) = &v.receipt {
        steps.push_str(&receipt.html());
    }
    for (i, (kind, text)) in v.steps.iter().enumerate() {
        let (class, label) = kind.label();
        let early = if i < v.plain_from { " early" } else { "" };
        steps.push_str(&format!(
            "<div class='step {class}{early}'><span class=dot></span><div><div class=lab>{label}</div><div class=txt>{}</div></div></div>",
            esc(text)
        ));
    }
    if v.steps.is_empty() {
        steps.push_str("<p class=nothing>Nothing underway. Ask me something and you'll see how I work it through here.</p>");
    }
    let spent = match &v.spent {
        Some(s) => format!(
            "<div class=box><div class=lab>Time on this</div><div class=big>{} <small>so far</small></div>\
             <p>If it runs long, I'll stop and tell you where I got to.</p></div>",
            esc(s)
        ),
        None => String::new(),
    };
    // Pause turns the microphone off too (28 Sep 2026), and says so.
    let (pause_what, pause_label) = if v.paused { ("resume", "Carry on and listen again") } else { ("pause", "Pause Atlas and stop listening") };
    let background = if v.background.is_empty() {
        String::new()
    } else {
        format!("<div class=box><div class=lab>Also running</div><p>{}</p></div>", esc(&v.background.join(" · ")))
    };
    let background = if v.held.is_empty() {
        background
    } else {
        let n = v.held.len();
        let items: String = v.held.iter().map(|t| format!("<li>{}</li>", esc(t))).collect();
        format!(
            "{background}<div class=box><div class=lab>Kept for you</div><p>{n} note{} that couldn't reach you when {} came. \
             I'll tell you {} when you're next back at the computer.</p><ul>{items}</ul></div>",
            if n == 1 { "" } else { "s" },
            if n == 1 { "it" } else { "they" },
            if n == 1 { "it" } else { "them" },
        )
    };
    body.push_str(&format!(
        "<div class=nowgrid><div class=stream id=detailed data-live=now data-v='{version}'>\
         <div class=streamhead><div style='display:flex;gap:11px'>{mark}<div><div class=title>{title}</div>\
         <div class=since>{since}</div></div></div>\
         <nav class=seg aria-label='How much to show'><a class=plain href='#'>Plain</a><a class=detail href='#detailed'>Detailed</a></nav></div>\
         {steps}</div>\
         <p class=liveline><span id=liveword role=status>Updating live.</span> <button type=button id=livepause aria-pressed=false>Pause live updates</button> <a href='/hub/now'>Refresh</a></p>\
         <aside class=rail2>{spent}\
         <div class=box><div class=lab>If this doesn't hold up</div><p>{fallback}</p></div>{background}\
         <div class='box soft'>{on}<p>Atlas runs on your machine. Task results above describe what each action reported.</p>\
         <form method=post action='/hub/pause'><input type=hidden name=what value={pause_what}>\
         <button>{pause_label}</button></form></div></aside></div>",
        mark = if v.working { THINKING } else { IDLE },
        version = live_version(v),
        title = esc(&v.title),
        since = esc(&v.since),
        fallback = esc(&v.fallback),
        on = pill("online", "On"),
    ));
    body.push_str(LIVE_SCRIPT);
    shell_at(Some(Page::Now), "Now", &body)
}

/// A plain list, knowing where it sits in the tree.
pub fn list_page_at(here: Option<Page>, title: &str, intro: &str, items: &[String]) -> String {
    let mut body = format!("<p class=note>{}</p>", esc(intro));
    if items.is_empty() {
        // Said the way a person would say it, and specific to the page --
        // "Nothing here." is the same sentence whether the list is genuinely
        // empty or never loaded.
        body.push_str(&nothing("Nothing on this list right now."));
    }
    for i in items {
        body.push_str(&format!("<div class=row>{}</div>", esc(i)));
    }
    match here {
        Some(p) => shell_at(Some(p), title, &body),
        None => shell(title, &body),
    }
}

/// Parse a posted form body.
/// Every field of a posted form, decoded, in order.
pub fn form_fields(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (urldecode(k), urldecode(v)),
            None => (urldecode(kv), String::new()),
        })
        .collect()
}

pub fn form_field(body: &str, name: &str) -> Option<String> {
    for pair in body.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == name {
            return Some(urldecode(v));
        }
    }
    None
}

pub fn urldecode(s: &str) -> String {
    // Bytes, then UTF-8: a %-escaped "é" is two bytes, and turning each byte
    // into a char on its own (as this did until 26 Sep 2026) wrote "Ã©" into
    // every message, name or search that wasn't plain English.
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// "Put Atlas on your phone": the QR code and the link, or why there isn't one.
///
/// `link` is `phonelink::phone_url(..)` once `phonelink::publish` succeeded;
/// `why_not` is `phonelink::say(..)` for any other outcome. With neither, the
/// block offers the button that runs `publish`. Self-contained so the
/// devices page can place it without changing its own signature.
///
/// The link carries the hub token. That is deliberate and bounded: it is
/// shown only on a page that needed the token to open, and it resolves only
/// inside the tailnet.
pub fn phone_block(link: Option<&str>, why_not: Option<&str>) -> String {
    let mut out = String::from("<h2>Put Atlas on your phone</h2>");
    match link {
        Some(url) => {
            let qr = crate::phonelink::qr_svg(url).unwrap_or_default();
            out.push_str(&format!(
                "<p class=what>Point the phone's camera at this, with Tailscale switched on \
                 on the phone. Nothing to type.</p>\
                 <div style=\"width:220px;max-width:100%;margin:12px 0\">{qr}</div>\
                 <p class=note>Or open this on the phone: <a href=\"{u}\">{u}</a></p>",
                u = esc(url)
            ));
        }
        None => {
            if let Some(why) = why_not {
                out.push_str(&format!("<p class=note>{}</p>", esc(why)));
            } else {
                out.push_str(
                    "<p class=what>Open Atlas on your laptop — its window makes the code \
                     for your phone once Tailscale is on, and it shows up here too.</p>",
                );
            }
        }
    }
    out
}
