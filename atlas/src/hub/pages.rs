//! The hub's pages, one function each.
//!
//! Moved out of `hub.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// The workspace page.
///
/// A board rather than a list. The columns are statuses, so what's stuck is
/// visible as a shape rather than as a word you have to read.
pub fn workspace_page(
    view: &crate::workspace_view::View,
    groups: &[(String, Vec<&crate::workspace_view::Item>)],
    overview: &crate::workspace_view::Overview,
    other_views: &[String],
) -> String {
    let mut body = String::new();

    // The numbers, but only the ones you'd act on. A count of everything is
    // an accusation rather than information.
    body.push_str("<div class=board-top>");
    for (n, label) in [
        (overview.needs_you, "need you"),
        (overview.doing, "in hand"),
        (overview.blocked, "stuck"),
        (overview.overdue, "past date"),
    ] {
        if n > 0 {
            body.push_str(&format!(
                "<span class=stat><b>{n}</b> {}</span>",
                esc(label)
            ));
        }
    }
    body.push_str("</div>");

    // Views are saved questions, not folders — so they're links across the
    // top rather than a tree down the side.
    body.push_str("<nav class=views>");
    for v in other_views {
        let here = *v == view.name;
        body.push_str(&format!(
            "<a class='{}' href='/hub/workspace?view={}'>{}</a>",
            if here { "view here" } else { "view" },
            esc(v),
            esc(v)
        ));
    }
    body.push_str("</nav>");

    if groups.is_empty() {
        body.push_str("<p class=note>Nothing here.</p>");
        return shell_at(Some(Page::Workspace), &view.name, &body);
    }

    body.push_str("<div class=columns>");
    for (name, items) in groups {
        body.push_str(&format!(
            "<section class=col><h2>{} <small>{}</small></h2>",
            esc(name),
            items.len()
        ));
        for i in items {
            let stuck = i.blocked_by.is_some();
            body.push_str(&format!(
                "<article class='card{}'><span class=what>{}</span>",
                if stuck { " waiting" } else { "" },
                esc(&i.title)
            ));
            if let Some(p) = &i.project {
                body.push_str(&format!("<span class=chip>{}</span>", esc(p)));
            }
            if let Some(b) = &i.blocked_by {
                body.push_str(&format!("<span class=why>waiting on {}</span>", esc(b)));
            }
            body.push_str("</article>");
        }
        body.push_str("</section>");
    }
    body.push_str("</div>");

    if let Some((what, n)) = &overview.biggest_blocker {
        // The single most useful line on the page.
        body.push_str(&format!(
            "<p class=note><b>{n}</b> things are waiting on {}. Unsticking that is worth more \
             than anything else here.</p>",
            esc(what)
        ));
    }
    shell_at(Some(Page::Workspace), &view.name, &body)
}

/// A past day.
///
/// What actually got finished, what was already open, and what Atlas was
/// thinking as it went. The last of those is the part no task app has, and
/// it's the one that answers "why did this take three days".
pub fn looking_back_page(day: &crate::workspace_view::Day, label: &str) -> String {
    let mut body = format!("<h2>{}</h2>", esc(label));

    body.push_str("<div class=board-top>");
    for (n, l) in [
        (day.finished.len(), "finished"),
        (day.started.len(), "started"),
        (day.carried_over.len(), "carried over"),
    ] {
        body.push_str(&format!("<span class=stat><b>{n}</b> {}</span>", esc(l)));
    }
    body.push_str("</div>");

    if !day.finished.is_empty() {
        body.push_str("<h3 class=sec>Finished</h3><ul class=rows>");
        for f in &day.finished {
            body.push_str(&format!("<li class=row>{}</li>", esc(f)));
        }
        body.push_str("</ul>");
    }

    // Atlas's working out, in the order it happened. Grouped by item, because
    // "why did this take three days" is a question about one thing and
    // hunting for it in a stream is why nobody ever does.
    if day.thoughts.is_empty() {
        body.push_str("<p class=note>Nothing recorded that day.</p>");
    } else {
        body.push_str("<h3 class=sec>What I was thinking</h3><div class=thoughts>");
        let mut last = String::new();
        for (item, t) in &day.thoughts {
            if *item != last {
                body.push_str(&format!("<div class=thought-item>{}</div>", esc(item)));
                last = item.clone();
            }
            body.push_str(&format!(
                "<div class=thought><span class=when>{}</span>{} {}</div>",
                esc(t.kind.plain()),
                "",
                esc(&t.what)
            ));
        }
        body.push_str("</div>");
    }
    shell_at(Some(Page::LookingBack), "Looking back", &body)
}

/// What Atlas would change about itself.
///
/// A page rather than an interruption, because none of this is urgent — it has
/// waited this long and can wait until you're looking. Each one carries its
/// evidence so you can disagree with that rather than with the conclusion.
pub fn recommendations_page(
    recs: &[crate::selfaudit::Recommendation],
    granted: Option<&str>,
    // Ways Atlas can get better on the hardware already here: each with
    // whether it's on here, and the button that turns it on when it isn't
    // (30 Sep 2026: they were a list you could only read).
    free_wins: &[FreeWin],
) -> String {
    let mut body = String::new();

    match granted {
        Some(level) => body.push_str(&format!(
            "<p class=note>I fix <b>{}</b> on my own and tell you after. Everything below \
             reaches further than that, so it's yours.</p>",
            esc(level)
        )),
        None => body.push_str(
            "<p class=note>I'm not fixing anything on my own at the moment. Everything below is \
             waiting on you.</p>",
        ),
    }

    if recs.is_empty() {
        body.push_str("<p class=note>Nothing I'd change.</p>");
    }

    for r in recs.iter() {
        // Certainty as a word rather than a number — a percentage on a
        // self-assessment is false precision.
        let sure = if r.certainty > 0.8 {
            "fairly sure"
        } else if r.certainty > 0.5 {
            "reasonably sure"
        } else {
            "not certain"
        };

        body.push_str(&format!(
            "<article class=rec><h2>{}</h2>\
             <p class=cause>I think {}.</p>\
             <p class=evidence>{} — {}</p>\
             <p class=proof>What would prove it: {}</p>\
             <form method=post action=/hub/recommendations/go>\
             <input type=hidden name=which value='{}'>\
             <button class=go>Have a go</button>\
             <button class=revoke name=drop value='1'>Not worth it</button>\
             </form></article>",
            esc(&r.symptom),
            esc(&r.cause),
            esc(sure),
            esc(&r.because),
            esc(&r.proof),
            // By what it says, not where it sat: the list can change between
            // drawing the page and pressing the button, and a position would
            // then act on a different idea (27 Sep 2026).
            esc(&r.symptom)
        ));
    }
    // Ways to get better on the hardware already here. `improve` listed these
    // and nothing ever asked it.
    if !free_wins.is_empty() {
        body.push_str("<h2>Free wins</h2><ul class=tight>");
        for w in free_wins {
            let act = match &w.get {
                Some((label, value)) => format!(
                    " <form class=inline method=post action=/hub/brains><input type=hidden name=from value=ideas>\
                     <button name=what value='{}'>{}</button></form>",
                    esc(value),
                    esc(label)
                ),
                None => String::new(),
            };
            body.push_str(&format!("<li><b>{}</b> {} <span class=note>{}</span>{act}</li>", esc(&w.what), esc(&w.worth), esc(&w.here)));
        }
        body.push_str("</ul>");
    }

    shell_at(Some(Page::Recommendations), "Ideas", &body)
}

/// A one-line nudge naming switches that are on but can't do anything,
/// because the capability behind them is blocked on something not installed.
///
/// Plain text, not HTML — the caller escapes it. `None` when there is nothing
/// to say, which is the usual case.
pub fn idle_banner(s: &Settings, blocked_capabilities: &[String]) -> Option<String> {
    let keys: Vec<String> = IDLE_TOGGLES
        .iter()
        .filter(|(cap, _)| blocked_capabilities.iter().any(|b| b.as_str() == *cap))
        .map(|(_, key)| key.to_string())
        .collect();
    if keys.is_empty() {
        return None;
    }
    let idle = s.idle_but_on(&keys);
    if idle.is_empty() {
        return None;
    }
    let names: Vec<&str> = idle.iter().map(|item| item.name.as_str()).collect();
    Some(format!(
        "On but doing nothing right now: {} — what {} needs isn't installed yet, \
         so you could switch {} off until it is.",
        names.join(", "),
        if names.len() == 1 { "it" } else { "each" },
        if names.len() == 1 { "it" } else { "them" },
    ))
}

pub fn settings_page(s: &Settings) -> String {
    let groups = s.groups();
    let changed = s.changed().len();

    let mut body = String::new();
    // The design's Settings index: one place that ties it together, with the
    // cards that open each area. The full list of switches follows.
    body.push_str("<nav class=setcards aria-label='Settings areas'>");
    for (href, title, what) in [
        ("#how-it-looks", "Appearance & access", "Colourway, accent, text size, density, colour-blind mode."),
        (Page::Sound.href(), "Sound & voice", "Atlas's voice, volume, the wake word, quiet hours, mute — and when it may pop up."),
        (Page::Gestures.href(), "Gestures", "Hands-free on camera: whether it's on, and how each works."),
        (Page::Trusted.href(), "Trusted recipients", "Who Atlas can send to without asking."),
        (Page::Accounts.href(), "Calendars & accounts", "Connect your email and calendars, and see which are working."),
        (Page::Help.href(), "Help & accessibility", "Using Atlas with a screen reader, keyboard or larger text; the statement."),
    ] {
        body.push_str(&format!(
            "<a class=setcard href='{href}'><b>{}</b><span>{}</span></a>",
            esc(title),
            esc(what)
        ));
    }
    body.push_str("</nav>");
    body.push_str(&format!(
        "<p class=note>A change is kept the moment you make it, and a running Atlas \
         picks it up within seconds. The few that set something up at the start — the \
         voice, the wake word, phone access — wait for a restart. This page works when the voice is down too, and if Atlas itself won't \
         start, open Atlas from the Start menu and choose <b>Settings</b> in its window.{}</p>",
        if changed > 0 {
            format!(
                " You've changed {changed} thing{} from the defaults.",
                if changed == 1 { "" } else { "s" }
            )
        } else {
            String::new()
        }
    ));

    // Switches that are on but have nothing to run on. The capability table
    // is the one place that knows what's blocked, so ask it here rather than
    // making every caller of this page pass the list in.
    let blocked: Vec<String> = crate::capability::blocked()
        .into_iter()
        .map(|(c, _)| c.id.to_string())
        .collect();
    if let Some(banner) = idle_banner(s, &blocked) {
        body.push_str(&format!("<p class=note>{}</p>", esc(&banner)));
    }

    // How it looks: the colourway (Warm Paper by default), accent, colour-blind
    // mode and density. These were kept by `appearance` with no page to
    // change them until 26 Sep 2026.
    body.push_str("<h2 id='how-it-looks'>How it looks</h2>");
    body.push_str(&crate::appearance::Appearance::load().settings_html());

    // The index. Forty-odd settings down one page is a scroll, not a choice —
    // and the thing you came to change is never the one at the top.
    body.push_str("<div class=jump>");
    for g in &groups {
        body.push_str(&format!(
            "<a href='#{}'>{}<span>{}</span></a>",
            slug(g),
            esc(g),
            s.in_group(g).len()
        ));
    }
    body.push_str("</div>");

    for group in &groups {
        body.push_str(&format!(
            "<h2 id='{}'>{}</h2>",
            slug(group),
            esc(group)
        ));
        // A heading with nothing under it makes you open every section to
        // find out which one holds the thing you came for.
        if let Some(note) = Settings::group_note(group) {
            body.push_str(&format!("<p class=groupnote>{}</p>", esc(note)));
        }
        for item in s.in_group(group) {
            body.push_str(&format!(
                "<div class=row id='set-{}'><div class=name>{}{}</div>\
                 <div class=what>{}</div>{}<div style=\"margin-top:8px\">{}</div></div>",
                esc(&item.key),
                esc(&item.name),
                tag(item.weight),
                esc(&item.what),
                if item.cost.is_empty() {
                    String::new()
                } else {
                    format!("<div class=cost>{}</div>", esc(&item.cost))
                },
                control(item)
            ));
        }
    }
    shell_at(Some(Page::Settings), "Settings", &body)
}

/// A heading turned into something an anchor can point at.
pub(super) fn slug(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}

/// The page worth reading once a month: everything that can act without
/// asking, or reach a sensor or the network.
/// Your projects, and the queue for each: what's ready to implement, what's
/// being worked, and what's still outstanding. This is the window you sort by
/// project — Atlas's own queue, any other, each on its own.
pub fn workshop_page(w: &crate::workshop::Workshop) -> String {
    let mut body = String::from(
        "<p class=note>Each project's queue. A change waits here, checked and titled, until you \
         say implement — nothing touches your files before that.</p>",
    );
    if w.projects.is_empty() {
        body.push_str(
            "<p>No projects yet. Tell Atlas \"on the &lt;name&gt; project, …\" and one starts here.</p>",
        );
        return shell_at(Some(Page::Workshop), "Projects", &body);
    }
    for p in &w.projects {
        body.push_str(&format!("<h2>{}</h2>", esc(&p.name)));
        if !p.folder.is_empty() {
            body.push_str(&format!("<p class=note>{}</p>", esc(&p.folder)));
        }

        let ready = p.ready();
        if ready.is_empty() {
            body.push_str("<p class=note>Nothing waiting on you.</p>");
        } else {
            body.push_str("<p class=note><b>Waiting on you</b></p>");
            for c in ready {
                let mark = if c.verified { "checked" } else { "unchecked" };
                body.push_str(&format!(
                    "<div class=row><div class=name>{}</div><div class=what>{}</div>\
                     <div class=cost>{mark}</div>\
                     <div style=\"margin-top:8px\">\
                       <form class=inline method=post action=/hub/implement>\
                         <input type=hidden name=title value=\"{}\">\
                         <button type=submit>implement</button>\
                       </form></div></div>",
                    esc(&c.title),
                    esc(&c.what),
                    esc(&c.title),
                ));
            }
        }

        let working = p.in_progress();
        if !working.is_empty() {
            body.push_str("<p class=note><b>Being worked</b></p>");
            for c in working {
                body.push_str(&format!(
                    "<div class=row><div class=name>{}</div><div class=what>{}</div></div>",
                    esc(&c.title),
                    esc(&c.what),
                ));
            }
        }

        let outstanding = p.outstanding();
        if !outstanding.is_empty() {
            body.push_str("<p class=note><b>Outstanding</b></p>");
            for t in outstanding {
                body.push_str(&format!(
                    "<div class=row><div class=name>{}</div></div>",
                    esc(&t.title),
                ));
            }
        }
    }
    shell_at(Some(Page::Workshop), "Projects", &body)
}

/// Your calendar, soonest first. Lean on purpose — what's coming up, when, and
/// where, and nothing else competing for the space.
/// Bring a file in and take one out, by clicking: an invite or contacts file
/// is read straight in (`Daemon::bring_in`), and the calendar and client list
/// download as the files every other app opens. These were typed commands
/// (`atlas calendar import`, `atlas clients export`) until 23 Sep.
pub(super) fn files_in_and_out() -> String {
    "<div class=files-io>\
     <label class=btn for=bring-in-file>Bring in an invite or contacts file (.ics, .vcf)</label>\
     <input id=bring-in-file type=file accept=\".ics,.vcf,text/calendar,text/vcard\" hidden>\
     <a class=btn href=\"/hub/calendar.ics\">Download your calendar (.ics)</a>\
     <a class=btn href=\"/hub/clients.vcf\">Download your clients (.vcf)</a>\
     <p id=bring-in-said class=note aria-live=polite></p></div>\
     <script>(function(){var f=document.getElementById('bring-in-file'),o=document.getElementById('bring-in-said');\
     f.addEventListener('change',function(){var file=f.files[0];if(!file)return;o.textContent='Reading '+file.name+'…';\
     var r=new FileReader();r.onload=function(){var b64=String(r.result).split(',')[1]||'';\
     fetch('/hub/bring-in',{method:'POST',headers:{'Content-Type':'application/json'},credentials:'same-origin',\
     body:JSON.stringify({name:file.name,data:b64})}).then(function(x){return x.json()}).then(function(j){o.textContent=j.said;\
     setTimeout(function(){location.reload()},2500)}).catch(function(){o.textContent='That didn\\'t reach Atlas. Is it still running?'})};\
     r.readAsDataURL(file)})})();</script>\
     <div class=files-io><h2>Teach Atlas to hear you</h2>\
     <p class=note>Recordings made on this computer's microphone (.wav). Nothing leaves the machine.</p>\
     <label class=btn data-prefix=wake->Your wake phrase — one take (do this three times)<input type=file accept=\".wav,audio/wav\" hidden></label>\
     <label class=btn data-prefix=room->The room, with nobody talking<input type=file accept=\".wav,audio/wav\" hidden></label>\
     <label class=btn data-prefix=you->You, talking somewhere quiet<input type=file accept=\".wav,audio/wav\" hidden></label>\
     <label class=btn data-prefix=voices->Other voices — a podcast or a call<input type=file accept=\".wav,audio/wav\" hidden></label>\
     <p id=hearing-said class=note aria-live=polite></p></div>\
     <script>(function(){var o=document.getElementById('hearing-said');\
     document.querySelectorAll('label[data-prefix]').forEach(function(l){var f=l.querySelector('input');\
     f.addEventListener('change',function(){var file=f.files[0];if(!file)return;o.textContent='Listening to '+file.name+'…';\
     var r=new FileReader();r.onload=function(){var b64=String(r.result).split(',')[1]||'';\
     fetch('/hub/bring-in',{method:'POST',headers:{'Content-Type':'application/json'},credentials:'same-origin',\
     body:JSON.stringify({name:l.dataset.prefix+file.name,data:b64})}).then(function(x){return x.json()}).then(function(j){o.textContent=j.said})\
     .catch(function(){o.textContent='That didn\\'t reach Atlas. Is it still running?'})};r.readAsDataURL(file)})})})();</script>".to_string()
}

/// This week on one time grid — personal, business and Atlas's own on the
/// same calendar (the design's Calendar-Week artboard), each event with its
/// area as a word and a time block hatched as booked time. Each day is also a
/// plain ordered list of what's on it, so a screen reader hears the day in
/// order rather than a picture of boxes.
pub(super) fn week_html(cal: &crate::calendar::Calendar, now: u64, zone: &crate::tz::Zone) -> String {
    const FIRST: i64 = 7; // 07:00
    const LAST: i64 = 22; // 22:00
    const HOUR_PX: i64 = 40;
    let off = zone.offset_at(now as i64);
    let today = (now as i64 + off).div_euclid(86_400);
    let monday = today - (today + 3).rem_euclid(7);
    let start_utc = (monday * 86_400 - off).max(0) as u64;
    let events = cal.occurrences_between(start_utc, start_utc + 7 * 86_400);
    let mut out = String::from("<section class=weekwrap aria-labelledby=weekh><h2 id=weekh>This week</h2>\
        <div class=week tabindex=0 role=region aria-label='This week — scrolls sideways on a small screen'><div class=hours aria-hidden=true>");
    for h in FIRST..LAST {
        out.push_str(&format!("<span style='top:{}px'>{:02}:00</span>", (h - FIRST) * HOUR_PX, h));
    }
    out.push_str("</div>");
    for d in 0..7 {
        let day = monday + d;
        let (_, m, dd) = crate::hubpages::ymd(day);
        let name = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][d as usize];
        let here: Vec<&crate::calendar::Event> = events
            .iter()
            .filter(|e| (e.start as i64 + off).div_euclid(86_400) == day)
            .collect();
        out.push_str(&format!(
            "<div class='wday{}'><h3 class=wdh>{name} {} {dd}</h3><ol class=wevents style='height:{}px'>",
            if day == today { " today" } else { "" },
            crate::hubpages::MONTHS[(m - 1) as usize],
            (LAST - FIRST) * HOUR_PX
        ));
        if here.is_empty() {
            out.push_str("<li class=sr>Nothing on.</li>");
        }
        for e in here {
            let local = e.start as i64 + off;
            let mins = local.rem_euclid(86_400) / 60;
            let len = ((e.end.max(e.start) - e.start) as i64 / 60).max(20);
            let top = if e.all_day { 0 } else { ((mins - FIRST * 60) * HOUR_PX / 60).clamp(0, (LAST - FIRST) * HOUR_PX - 20) };
            let height = if e.all_day { 22 } else { (len * HOUR_PX / 60).max(20).min((LAST - FIRST) * HOUR_PX - top) };
            let area = match &e.space {
                crate::earned::Space::Business(b) => b.clone(),
                crate::earned::Space::Personal => "Personal".to_string(),
            };
            let block = e.kind == crate::calendar::EventKind::TimeBlock;
            out.push_str(&format!(
                "<li class='wev{}' style='top:{top}px;height:{height}px'><time>{}</time> {}<span class=area>{}</span>{}</li>",
                if block { " block" } else { "" },
                if e.all_day { "All day".to_string() } else { format!("{:02}:{:02}", mins / 60, mins % 60) },
                esc(&e.title),
                esc(&area),
                if block { "<span class=sr> (focus block)</span>" } else { "" },
            ));
        }
        out.push_str("</ol></div>");
    }
    out.push_str("</div></section>");
    out
}

pub fn calendar_page(cal: &crate::calendar::Calendar, now: u64, zone: &crate::tz::Zone) -> String {
    let mut body = String::from(
        "<p class=note>Your own calendar, kept here and offline. The one on your phone syncs in \
         when it's connected.</p>",
    );
    body.push_str(&week_html(cal, now, zone));
    body.push_str("<h2>Coming up</h2>");
    body.push_str(&files_in_and_out());
    // Occurrence-aware so a repeating event shows on each day it's on in the
    // next month, the same as the spoken agenda — not once at its series start.
    let upcoming = cal.occurrences_between(now, now + 30 * 86_400);
    if upcoming.is_empty() {
        body.push_str(
            "<p>Nothing coming up. Tell Atlas \"schedule &lt;something&gt; tomorrow at 3\" and it \
             lands here.</p>",
        );
        return shell_at(Some(Page::Calendar), "Calendar", &body);
    }
    for e in upcoming {
        let place = e.place.as_deref().map(|p| format!(" · {}", esc(p))).unwrap_or_default();
        // The firewall side, shown as a word rather than a colour alone, so a
        // business event reads as one at a glance and the personal/business
        // split is visible on the same combined calendar.
        let area = match &e.space {
            crate::earned::Space::Business(b) => format!(" · {}", esc(b)),
            crate::earned::Space::Personal => String::new(),
        };
        // A time block reads as reserved focus, not a meeting — said as a word
        // so it's clear without relying on colour.
        let kind = match e.kind {
            crate::calendar::EventKind::TimeBlock => " · focus",
            crate::calendar::EventKind::Meeting => "",
        };
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div>\
             <div class=what>{}{place}{area}{kind}</div></div>",
            esc(&e.title),
            esc(&e.say_when_in(zone)),
        ));
    }
    shell_at(Some(Page::Calendar), "Calendar", &body)
}

/// The add-ons on this install: what each asks, what you allowed, and the
/// buttons for approving, taking a permission away, and switching off.
///
/// The approve button carries the fingerprint of the file this page showed,
/// so pressing it approves exactly what you read -- a file changed in between
/// is refused, not approved.
/// Also what friends have shared with you (`offers`), who you can share with
/// (`share_to`: your groups, then paired people) and the groups you could
/// recommend one in.
pub fn addons_page_with(
    plugins: &[crate::plugins::Plugin],
    offers: &[crate::plugins::Offered],
    share_to: &[String],
    groups: &[String],
) -> String {
    use crate::plugins::Status;
    let mut body = String::from(
        "<p class=note>Add-ons give Atlas new things to do by putting together what it already \
         does. None of them run code. Each does nothing until you approve it, only what you \
         allowed, and still asks you first wherever Atlas would ask you. Some things no add-on \
         can ever do: open the vault, pair devices, use your accounts, hand Atlas over, or \
         change Atlas itself.</p>",
    );
    // What friends shared: nothing here is installed. Adding one is one
    // decision, made seeing exactly what it would be allowed to do.
    if !offers.is_empty() {
        body.push_str("<h2>Shared with you</h2>");
        for o in offers {
            let place = match &o.in_group {
                Some(g) => format!("shared by {} in {}", esc(&o.from), esc(g)),
                None => format!("sent to you by {}", esc(&o.from)),
            };
            let perms: Vec<String> = o
                .permissions
                .iter()
                .map(|k| esc(crate::plugins::permission(k).map(|p| p.plain).unwrap_or(k)))
                .collect();
            let form = |what: &str, extra: &str, label: &str| {
                format!(
                    "<form method=post action=/hub/addons style='display:inline'>\
                     <input type=hidden name=what value='{what}'><input type=hidden name=id value='{}'>{extra}\
                     <button class=revoke>{}</button></form> ",
                    o.offer,
                    esc(label)
                )
            };
            // What it would actually do, step by step -- judged by its steps,
            // not only its description.
            let steps: String = serde_yaml::from_str::<crate::plugins::Manifest>(&o.text)
                .map(|m| {
                    m.flows
                        .iter()
                        .map(|f| {
                            let start = match (&f.schedule, f.triggers.first()) {
                                (Some(when), _) => format!("runs {when}"),
                                (None, Some(t)) => format!("say “{t}”"),
                                (None, None) => "nothing starts it".into(),
                            };
                            format!(
                                "<li>“{}” — {}: {}</li>",
                                esc(&f.name),
                                esc(&start),
                                esc(&f.steps.iter().map(|s| s.command.clone()).collect::<Vec<_>>().join(", then "))
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            body.push_str(&format!(
                "<div class=row><div class=name>{}</div><div class=what>{place}. Says it's by {} — only \
                 who sent it is certain.{}</div><div class=cost>It would be allowed to: {}</div><ul>{steps}</ul>{}{}</div>",
                esc(&o.name),
                esc(&o.author),
                if o.description.is_empty() { String::new() } else { format!(" {}", esc(&o.description)) },
                if perms.is_empty() { "nothing beyond talking back".to_string() } else { perms.join("; ") },
                form("take", &format!("<input type=hidden name=sha value='{}'>", esc(&o.sha256)), "Add it and allow that"),
                form("decline", "", "No thanks"),
            ));
        }
        body.push_str("<h2>Yours</h2>");
    }
    if plugins.is_empty() {
        body.push_str(&nothing(
            "No add-ons yet. Add-ons friends share with you appear here.",
        ));
    }
    let button = |what: &str, id: &str, extra: &str, label: &str| {
        format!(
            "<form method=post action=/hub/addons style='display:inline'>\
             <input type=hidden name=what value='{}'><input type=hidden name=id value='{}'>{extra}\
             <button class=revoke>{}</button></form> ",
            esc(what),
            esc(id),
            esc(label)
        )
    };
    for p in plugins {
        let mut row = format!(
            "<div class=row><div class=name>{}</div><div class=what>{}</div>",
            esc(&p.name()),
            esc(&p.status.plain())
        );
        if let Some(m) = &p.manifest {
            row.push_str(&format!(
                "<div class=cost>By {}{}</div>",
                esc(&m.author),
                if m.description.is_empty() { String::new() } else { format!(" — {}", esc(&m.description)) }
            ));
            row.push_str("<ul>");
            for k in &m.permissions {
                let plain = crate::plugins::permission(k).map(|x| x.plain).unwrap_or("");
                let allowed = p.granted.contains(k);
                row.push_str(&format!(
                    "<li>{} {}{}</li>",
                    if allowed { "<b>allowed:</b>" } else { "asks to:" },
                    esc(plain),
                    if allowed && p.status == Status::Active {
                        button("revoke", &p.id, &format!("<input type=hidden name=key value='{}'>", esc(k)), "take this away")
                    } else {
                        String::new()
                    }
                ));
            }
            for f in &p.flows {
                let starts = if f.triggers.is_empty() {
                    "nothing starts it".to_string()
                } else {
                    format!("say “{}”", f.triggers.join("” or “"))
                };
                row.push_str(&format!(
                    "<li>“{}” — {}: {}</li>",
                    esc(&f.name),
                    esc(&starts),
                    esc(&f.steps.iter().map(|s| s.command.clone()).collect::<Vec<_>>().join(", then "))
                ));
            }
            row.push_str("</ul>");
        }
        if let Some(who) = &p.sent_by {
            row.push_str(&format!(
                "<div class=what>Sent to you by {} — your paired Atlas, so that part is certain; \
                 who wrote it is only what the file says.</div>",
                esc(who)
            ));
        }
        for (name, when) in &p.schedules {
            let when = match when {
                crate::plugins::Schedule::Every(secs) if secs % 3600 == 0 => format!("every {} hours", secs / 3600),
                crate::plugins::Schedule::Every(secs) => format!("every {} minutes", secs / 60),
                crate::plugins::Schedule::DailyAt(m) => format!("daily at {:02}:{:02}", m / 60, m % 60),
            };
            row.push_str(&format!("<div class=what>“{}” runs by itself, {}.</div>", esc(name), esc(&when)));
        }
        // Steps that ask first, and the choice not to be asked every time.
        // Offered only where it's safe to stop asking: a step that is the same
        // every time, and not one that speaks to people as you.
        if !p.questions.is_empty() {
            row.push_str("<div class=what>Steps I ask about before doing:</div><ul>");
            for q in &p.questions {
                let control = match (&q.always_asks, q.trusted, p.status == Status::Active) {
                    (Some(why), _, _) => format!(" — always asks: {}", esc(why)),
                    (None, true, true) => format!(
                        " — you said don't ask {}",
                        button("untrust", &p.id, &format!("<input type=hidden name=key value='{}'>", esc(&q.command)), "ask again")
                    ),
                    (None, false, true) => button(
                        "trust",
                        &p.id,
                        &format!("<input type=hidden name=key value='{}'>", esc(&q.command)),
                        "don't ask me each time",
                    ),
                    _ => String::new(),
                };
                row.push_str(&format!("<li>“{}” {control}</li>", esc(&q.command)));
            }
            row.push_str("</ul>");
        }
        for t in &p.trouble {
            row.push_str(&format!("<div class=what>Note: {}</div>", esc(t)));
        }
        let fp = format!("<input type=hidden name=sha value='{}'>", esc(&p.sha256));
        match p.status {
            Status::Waiting | Status::Changed if p.manifest.is_some() => {
                row.push_str(&button("approve", &p.id, &fp, "Approve it to do the above"))
            }
            Status::Active => row.push_str(&button("off", &p.id, "", "Switch it off")),
            Status::Disabled => row.push_str(&button("on", &p.id, "", "Switch it back on")),
            _ => {}
        }
        row.push_str(&button("remove", &p.id, "", "Remove it"));
        if p.manifest.is_some() && !share_to.is_empty() {
            let opts: String = share_to.iter().map(|t| format!("<option>{}</option>", esc(t))).collect();
            row.push_str(&button("share", &p.id, &format!("<select name=key aria-label='Share it with'>{opts}</select>"), "Share it with"));
        }
        if p.status == Status::Active && !groups.is_empty() {
            let opts: String = groups.iter().map(|t| format!("<option>{}</option>", esc(t))).collect();
            row.push_str(&button("recommend", &p.id, &format!("<select name=key aria-label='Recommend it in'>{opts}</select>"), "Recommend it in"));
        }
        row.push_str("</div>");
        body.push_str(&row);
    }
    shell_at(Some(Page::AddOns), "Add-ons", &body)
}

/// Your own edits to the shipped config files, with a way back to the
/// default for each. Where the "undo my edit" lives, because editing the
/// shipped file back does nothing once the edit has been kept.
pub fn edits_page(kept: &[crate::yourchanges::KeptEdit], problems: &[String]) -> String {
    use crate::yourchanges::shown;
    let mut body = String::from(
        "<p class=note>When you edit one of Atlas's config files by hand, Atlas moves the edit \
         here the next time it starts, so an update can't undo it. To go back to what Atlas \
         ships, press the button — editing the file back won't do it, because the edit lives \
         here now.</p>",
    );
    for p in problems {
        body.push_str(&format!("<div class='rec'><p class=cause>{}</p></div>", esc(p)));
    }
    if kept.is_empty() {
        body.push_str(&nothing("No edits of yours. Every config file is as Atlas ships it."));
    }
    for e in kept {
        let yours = if e.change.removed { "you removed it".to_string() } else { shown(e.change.yours.as_ref()) };
        let mut notes = String::new();
        if e.change.unsure {
            notes.push_str(
                "<div class=what>Atlas couldn't tell whether this was your edit or an older \
                 version's default. If it wasn't you, go back to the default.</div>",
            );
        }
        if e.default_moved() {
            notes.push_str(&format!(
                "<div class=what>The default has changed since you set this: it was {}, it's now {}. Yours still applies.</div>",
                esc(&shown(e.change.was.as_ref())),
                esc(&shown(e.shipped_now.as_ref()))
            ));
        }
        body.push_str(&format!(
            "<div class=row><div class=name>{} — {}</div><div class=what>Yours: {}</div>\
             <div class=cost>Atlas ships: {}</div>{notes}\
             <form method=post action=/hub/edits><input type=hidden name=file value='{}'>\
             <input type=hidden name=path value='{}'><button class=revoke>Back to the default</button></form></div>",
            esc(&e.file),
            esc(&e.change.dotted()),
            esc(&yours),
            esc(&shown(e.shipped_now.as_ref())),
            esc(&e.file),
            esc(&e.change.dotted())
        ));
    }
    shell_at(Some(Page::Edits), "Your edits", &body)
}

/// Friends: one link to add someone, requests, and who can reach you.
pub fn friends_page(v: &FriendsView) -> String {
    let form = |what: &str, inner: &str, label: &str| {
        format!(
            "<form method=post action=/hub/friends style='display:inline'>\
             <input type=hidden name=what value='{}'>{inner}<button>{}</button></form> ",
            esc(what),
            esc(label)
        )
    };
    let who = |n: &str| format!("<input type=hidden name=who value='{}'>", esc(n));
    let mut body = String::new();
    if let Some(said) = &v.said {
        body.push_str(&format!("<div class=row><div class=what>{}</div></div>", esc(said)));
    }
    body.push_str("<h2>Add a friend</h2>");
    match &v.link {
        Some(link) => {
            let qr = crate::phonelink::qr_svg(link).unwrap_or_default();
            body.push_str(&format!(
                "<p class=what>Send them this -- a text, an email, anything. When their Atlas opens it \
                 you're friends, both ways, with nothing to send back. It works once, for a week.</p>\
                 <textarea autocomplete=off readonly rows=4 style='width:100%' aria-label='Your friend link' onclick='this.select()'>{}</textarea>\
                 <p class=note>In person: they can scan this with their phone and paste what it reads.</p>\
                 <div style=\"width:220px;max-width:100%;margin:12px 0\">{qr}</div>",
                esc(link)
            ));
        }
        None => body.push_str(&format!(
            "<p class=what>Make a link and send it to them. That's all -- no codes back and forth, \
             no waiting for anyone to confirm.</p>{}",
            form("link", "", "Make a friend link")
        )),
    }
    body.push_str("<h2>Got a link from someone?</h2><form method=post action=/hub/friends>\
         <input type=hidden name=what value=add><input autocomplete=off name=link style='width:70%' aria-label='Their link, or the whole message' \
         placeholder='Paste the whole message -- I&#39;ll find the link in it'> <button>Add them</button></form>");
    if !v.requests.is_empty() {
        body.push_str("<h2>Friend requests</h2>");
        for (from, group) in &v.requests {
            body.push_str(&format!(
                "<div class=row><div class=name>{}</div><div class=what>From \"{}\"</div>{}{}</div>",
                esc(from),
                esc(group),
                form("accept", &who(from), "Accept"),
                form("decline", &who(from), "Decline")
            ));
        }
    }
    if !v.could_ask.is_empty() {
        body.push_str("<h2>People in your groups</h2><p class=note>Not friends yet. A request goes to them \
                       alone, through the group's owner.</p>");
        for (name, group) in &v.could_ask {
            body.push_str(&format!(
                "<div class=row><div class=name>{}</div><div class=what>In \"{}\"</div>{}</div>",
                esc(name),
                esc(group),
                form("request", &who(name), "Send a friend request")
            ));
        }
    }
    body.push_str("<h2>Your friends</h2>");
    if !v.reach.is_empty() {
        body.push_str(&format!("<p class=note>{}</p>", esc(&v.reach)));
    }
    if v.friends.is_empty() {
        body.push_str(&nothing("No friends yet. Make a link above and send it to someone."));
    }
    for f in &v.friends {
        let waiting = if v.reaching.iter().any(|r| r.eq_ignore_ascii_case(f)) {
            "<div class=note>Their Atlas hasn't answered yet -- I keep trying for a week.</div>"
        } else {
            ""
        };
        body.push_str(&format!(
            "<div class=row><div class=name>{}</div>{waiting}{}</div>",
            esc(f),
            form("forget", &who(f), "Unfriend")
        ));
    }
    body.push_str(
        "<p class=note>How their Atlas reaches yours: through Tor, which Atlas runs itself -- no \
         server, no shared network, nothing to set up, and nobody in between can read your messages \
         or see who you talk to. On the same wifi it goes straight across. If a friend's Atlas is \
         off, yours keeps the message and sends it when theirs is back.</p>",
    );
    shell_at(Some(Page::Friends), "Friends", &body)
}

/// Group chats with an owner: yours to change, others' to read.
pub fn groups_page(views: &[crate::groups::View], addable: &[String]) -> String {
    groups_page_with(views, addable, &[])
}

/// The same, with groups made before groups had owners (`ownerless`) and the
/// button that gives one an owner.
pub fn groups_page_with(views: &[crate::groups::View], addable: &[String], ownerless: &[String]) -> String {
    use crate::groups::Role;
    let mut body = String::from(
        "<p class=note>A group you make is yours: you decide who's in it and whether each \
         person can post or only read. Everyone's Atlas checks your signed list, so nobody \
         else can add people or speak where they're only reading. A release channel is a \
         group where only you post — that's where Atlas updates come from.</p>",
    );
    let form = |what: &str, id: &str, inner: &str, label: &str| {
        format!(
            "<form method=post action=/hub/groups style='display:inline'>\
             <input type=hidden name=what value='{}'><input type=hidden name=group value='{}'>{inner}\
             <button class=revoke>{}</button></form> ",
            esc(what),
            esc(id),
            esc(label)
        )
    };
    let hidden = |k: &str, v: &str| format!("<input type=hidden name={k} value='{}'>", esc(v));
    let options = |names: &[String]| {
        names.iter().map(|n| format!("<option>{}</option>", esc(n))).collect::<String>()
    };
    if views.is_empty() {
        body.push_str(&nothing("No groups with an owner yet. Make one below."));
    }
    for v in views {
        let kind = if v.release_channel { " — release channel" } else { "" };
        let mut row = format!(
            "<div class=row><div class=name>{}{kind}</div><div class=what>{}</div><ul>",
            esc(&v.name),
            if v.mine { "You made this, so you decide who's in it.".to_string() } else { format!("Made by {}.", esc(&v.owner)) }
        );
        for (who, role, _key) in &v.seats {
            let mut controls = String::new();
            if v.mine && *role != Role::Owner {
                controls.push_str(&form("remove", &v.id, &hidden("who", who), "take out"));
                if !v.release_channel {
                    let (to, label) = if *role == Role::Reader { ("member", "let them post") } else { ("reader", "reading only") };
                    controls.push_str(&form("role", &v.id, &format!("{}{}", hidden("who", who), hidden("role", to)), label));
                }
            }
            row.push_str(&format!("<li>{} — {} {controls}</li>", esc(who), esc(role.plain())));
        }
        row.push_str("</ul>");
        if v.mine {
            let others: Vec<String> =
                addable.iter().filter(|a| !v.seats.iter().any(|(w, _, _)| w.eq_ignore_ascii_case(a))).cloned().collect();
            if !others.is_empty() {
                row.push_str(&form(
                    "add",
                    &v.id,
                    &format!("<select name=who aria-label='Who to add'>{}</select>", options(&others)),
                    "add to this group",
                ));
            }
        }
        row.push_str("</div>");
        body.push_str(&row);
    }
    if !ownerless.is_empty() {
        body.push_str(
            "<h2>Groups without an owner</h2><p class=note>Made before groups had owners: anyone \
             in them can bring people in, and nobody can take anyone out. Give one an owner — \
             you — and it starts again with the same people, under your list.</p>",
        );
        for g in ownerless {
            body.push_str(&format!(
                "<div class=row><div class=name>{}</div>{}</div>",
                esc(g),
                form("adopt", g, "", "Give it an owner (you)")
            ));
        }
    }
    body.push_str("<h2>Make a group</h2>");
    if addable.is_empty() {
        body.push_str(
            "<p class=note>Pair with someone first. Once their Atlas has introduced itself \
             (it does that by itself), they can be added.</p>",
        );
    } else {
        body.push_str(&format!(
            "<form method=post action=/hub/groups><input type=hidden name=what value=new>\
             <input autocomplete=off name=group aria-label='Group name' placeholder='Name'> <input autocomplete=off name=who aria-label='Who is in it' placeholder='{}'>\
             <label><input type=checkbox name=role value=release> release channel (only you post)</label>\
             <button>Make it</button></form>",
            esc(&format!("People, e.g. {}", addable.iter().take(2).cloned().collect::<Vec<_>>().join(", ")))
        ));
    }
    shell_at(Some(Page::Groups), "Groups", &body)
}

/// After a button: back to the page if it worked; if not, say why, with the
/// way back. A refusal shown as the same page reloading would look exactly
/// like success.
///
/// What it did is said on the page it goes back to (27 Sep 2026: every button
/// came back to a page that looked the same whether or not anything happened).
pub fn after_button(page: Page, done: std::result::Result<String, String>) -> crate::server::Reply {
    match done {
        Ok(said) if !said.trim().is_empty() => back_with(page.href(), "", &said),
        Ok(_) => crate::server::Reply::redirect(page.href()),
        Err(why) => crate::server::Reply::html(shell(
            page.label(),
            &format!(
                "<p class=note>{}</p><p><a href='{}'>Back to {}</a></p>",
                esc(&why),
                page.href(),
                esc(page.label())
            ),
        )),
    }
}

/// Where to go after a form: the page, with what happened said once
/// (`?said=`). `extra` is more of the query (`room=…`, `job=…`), or empty.
/// Nothing private goes through here -- an address lands in the browser's
/// history. Private text is a `hubjobs::Flash`.
pub fn back_with(href: &str, extra: &str, said: &str) -> crate::server::Reply {
    // The anchor can come from a form (a setting's key), and this becomes a
    // `Location` header: only characters an id uses, so nothing can end the
    // header or the address.
    let (path, anchor) = match href.split_once('#') {
        Some((p, a)) => (p, format!("#{}", a.chars().filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c)).collect::<String>())),
        None => (href, String::new()),
    };
    // Kept short: the hub reads at most 8 KB of request head, and a long
    // error sentence would make the address it's sent back to unreadable.
    let said: String = if said.chars().count() > 600 {
        said.chars().take(600).collect::<String>() + "…"
    } else {
        said.to_string()
    };
    let sep = if extra.is_empty() { "" } else { "&" };
    crate::server::Reply::redirect(&format!(
        "{path}?{extra}{sep}said={}{anchor}",
        crate::research::urlencode(&said)
    ))
}

/// A block put straight after the page's heading (and the note under it).
pub fn with_block_after_heading(page: String, block: &str) -> String {
    match page.find("</h1>") {
        Some(at) => {
            let mut at = at + 5;
            if page[at..].starts_with("<p class=pagenote>") {
                if let Some(end) = page[at..].find("</p>") {
                    at += end + 4;
                }
            }
            format!("{}{block}{}", &page[..at], &page[at..])
        }
        None => page,
    }
}

/// A page with a sentence saying what just happened, under its heading.
pub fn with_said(page: String, said: Option<&str>) -> String {
    let Some(said) = said.filter(|s| !s.trim().is_empty()) else { return page };
    let notice = format!("<p class=notice role=status>{}</p>", esc(said));
    match page.find("</h1>").or_else(|| page.rfind("</main>")) {
        Some(at) => {
            let at = if page[at..].starts_with("</h1>") { at + 5 } else { at };
            format!("{}{notice}{}", &page[..at], &page[at..])
        }
        None => page,
    }
}

/// A page that looks again by itself in `secs` seconds: a hub job still
/// running (`hubjobs`). No script -- the hub works without one.
pub fn with_refresh(page: String, secs: u32) -> String {
    match page.find("</head>") {
        Some(at) => format!("{}<meta http-equiv=refresh content={secs}>{}", &page[..at], &page[at..]),
        None => page,
    }
}

pub fn permissions_page(s: &Settings, granted_apps: &[String]) -> String {
    let items = s.consequential();
    let mut body = String::from(
        "<p class=note>Everything that can use a sensor, reach outside this machine, \
         or act without asking. Worth a look now and then.</p>",
    );
    // The apps you've allowed Atlas to act on, from the grants gate. Only the
    // "always" grants survive a restart and reach here; a session or one-off
    // grant is gone by the next start. `granted_apps` had no reader before —
    // the gate could record a standing permission and nothing showed it.
    if !granted_apps.is_empty() {
        body.push_str("<p class=note>Apps you've allowed Atlas to act on: ");
        body.push_str(&esc(&granted_apps.join(", ")));
        body.push_str(".</p>");
    }
    if items.is_empty() {
        body.push_str("<p>Nothing is enabled.</p>");
    }
    // Every row can be changed here, on or off (29 Sep 2026). A row that was
    // off showed the word "off" and no button, so this page listed what
    // Atlas wasn't allowed to do and gave no way to allow it: Eric looked for
    // where to change a permission and there was nowhere. Turning one on
    // still asks first (`control`, `Weight::needs_confirming`).
    for item in items {
        body.push_str(&format!(
            "<div class=row><div class=name>{}{}</div><div class=what>{}</div>\
             <div class=cost>{}</div><div style=\"margin-top:8px\">{}</div></div>",
            esc(&item.name),
            tag(item.weight),
            esc(&item.what),
            esc(&item.cost),
            control(item)
        ));
    }
    shell_at(Some(Page::Permissions), "Permissions", &body)
}

/// The dashboard: your cards, in your order.
///
/// `bodies` supplies the contents for each card, so this function knows about
/// arrangement and nothing about what is being arranged. A card with no body
/// says it has nothing rather than rendering an empty frame — an empty frame
/// reads as a broken page.
///
/// Two modes, because a dashboard you rearrange by accident while reading it
/// is worse than one you cannot rearrange at all. Reading is the default;
/// arranging is a thing you turn on.
pub fn dashboard_page(
    layout: &crate::dash::Layout,
    bodies: &[(crate::dash::Card, String)],
    arranging: bool,
) -> String {
    dashboard_page_with(layout, bodies, arranging, 0)
}

/// The dashboard: your cards, in your order.
///
/// `bodies` supplies the contents, so this knows about arrangement and nothing
/// about what is being arranged.
///
/// Two modes. Reading is the default, because a dashboard you rearrange by
/// accident while reading it is worse than one you cannot rearrange at all.
pub(super) fn dashboard_page_with(
    layout: &crate::dash::Layout,
    bodies: &[(crate::dash::Card, String)],
    arranging: bool,
    waiting: usize,
) -> String {
    dashboard_deck(layout, bodies, arranging, waiting, &Deck::not_running())
}
