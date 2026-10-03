//! The daemon's side of `connect`: the "Connect an account" section at the
//! top of the Accounts page, what its buttons do, and the calendar links read
//! again in the background.
//!
//! Free functions taking the daemon, so every call site names this module.

use crate::connect::{self, CalendarLink, Way};
use crate::daemon::Daemon;
use crate::hub::{self, esc, Page};
use crate::server::Reply;
use std::sync::Mutex;

/// Where a lookup of an unknown provider is kept between the "Next" press
/// and the page that shows the result: domain -> (host, port).
const LOOKED_UP: &str = "connect_looked_up";

/// Calendar links read on their own thread, waiting to be taken in.
static READ: Mutex<Vec<(String, Result<String, String>)>> = Mutex::new(Vec::new());

fn field(fields: &[(String, String)], name: &str) -> String {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim().to_string()).unwrap_or_default()
}

fn back(said: &str) -> Reply {
    hub::back_with(&format!("{}#connect", Page::Accounts.href()), "", said)
}

/// The section, for the top of the Accounts page. `asked` is the address
/// you typed, when you pressed Next.
pub fn section(d: &mut Daemon, asked: Option<&str>) -> String {
    let mut out = String::from(
        "<section id=connect aria-labelledby=connect-h><h2 id=connect-h>Connect an account</h2>\
         <p class=what>Type your email address and I'll work out the rest: mail first, and the same address \
         is how Outlook and Google calendars connect once their sign-in is ready.</p>\
         <form method=post action='/hub/connect' class=inline><input type=hidden name=what value=start>\
         <label>Email address <input name=address type=email autocomplete=email required></label>\
         <button>Next</button></form>",
    );
    if let Some(a) = asked.and_then(connect::address) {
        out.push_str(&step_for(d, &a));
    }
    out.push_str(&calendar_form());
    out.push_str(&connected_list(d));
    out.push_str(
        "<p class=note>Bluesky, YouTube and the other networks Atlas can read connect on the \
         <a href='/hub/social'>Social page</a>. Instagram, TikTok, X and Reddit can't be: they've closed \
         their doors to apps like Atlas, or charge for every read.</p></section>",
    );
    out
}

/// What connecting this address takes, as the form for it.
fn step_for(d: &mut Daemon, address: &str) -> String {
    let way = match connect::way_for(address) {
        Some(Way::LookItUp { domain }) => {
            let found: std::collections::BTreeMap<String, (String, u16)> = d.store.load(LOOKED_UP);
            match found.get(&domain) {
                Some((host, port)) => connect::found_mailbox(&domain, host, *port),
                None => Way::LookItUp { domain },
            }
        }
        Some(w) => w,
        None => return String::new(),
    };
    let mut out = format!("<div class=box id=connect-next><div class=lab>{}</div>", esc(address));
    match way {
        Way::AppPassword(m) => {
            out.push_str(&format!("<p><b>{}</b>. {}</p>", esc(&m.provider), esc(&m.steps)));
            if let Some(link) = &m.link {
                out.push_str(&format!(
                    "<p><a class=btn href='{}' target=_blank rel=noopener>Open {}'s app password page</a></p>",
                    esc(link),
                    esc(&m.provider)
                ));
            }
            out.push_str(&format!(
                "<form method=post action='/hub/connect'><input type=hidden name=what value=password>\
                 <input type=hidden name=address value='{a}'><input type=hidden name=host value='{h}'>\
                 <input type=hidden name=port value='{p}'>\
                 <label>The app password <input name=password type=password autocomplete=off required></label>",
                a = esc(address),
                h = esc(&m.imap_host),
                p = m.imap_port
            ));
            out.push_str(&vault_field(d));
            out.push_str("<button class=primary>Connect</button></form>");
            out.push_str("<p class=note>I try it against the mail server before keeping it. It's kept sealed in your vault, never in a settings file.</p>");
        }
        Way::MicrosoftSignIn => out.push_str(
            "<p><b>Outlook / Hotmail.</b> Microsoft switched passwords off for these in 2024, so they connect \
             through Microsoft's own sign-in. That button isn't here yet: it needs Atlas registered with \
             Microsoft once, which is being set up. Until then it can be connected by saying \
             \"connect my outlook account\" with the address and an app registration's client ID.</p>",
        ),
        Way::LookItUp { domain } => out.push_str(&format!(
            "<p>I couldn't find the mail settings for {} by myself. If you know your mail server's address \
             (your provider's help pages call it the IMAP server), put it here with your password.</p>\
             <form method=post action='/hub/connect'><input type=hidden name=what value=password>\
             <input type=hidden name=address value='{}'><input type=hidden name=port value=993>\
             <label>IMAP server <input name=host required placeholder='imap.example.com'></label>\
             <label>Password <input name=password type=password autocomplete=off required></label>{}\
             <button class=primary>Connect</button></form>",
            esc(&domain),
            esc(address),
            vault_field(d)
        )),
        Way::NotPossible { provider, why } => out.push_str(&format!("<p><b>{}.</b> {}</p>", esc(provider), esc(why))),
    }
    out.push_str("</div>");
    out
}

/// The vault's passphrase box, when the vault is shut and can't open itself.
fn vault_field(d: &Daemon) -> String {
    if d.vault.state() == crate::vault::State::Open || d.vault.sealed_to_this_login() {
        return String::new();
    }
    if d.vault.has_a_passphrase() {
        "<label>Your vault passphrase (it keeps the password sealed) \
         <input name=passphrase type=password autocomplete=current-password required></label>"
            .into()
    } else {
        "<label>Choose a vault passphrase -- a sentence of 12 characters or more. It seals every password \
         Atlas keeps; you'll need it again. <input name=passphrase type=password autocomplete=new-password required></label>"
            .into()
    }
}

fn calendar_form() -> String {
    "<h3>A calendar by its private link</h3>\
     <p class=what>Google: Settings, your calendar, then \"Secret address in iCal format\". Outlook: Settings, \
     Calendar, Shared calendars, Publish, then the ICS link. iCloud: share the calendar as Public. Paste it here \
     and I'll keep reading it, every 15 minutes.</p>\
     <form method=post action='/hub/connect' class=inline><input type=hidden name=what value=calendar>\
     <label>Calendar link <input name=url type=url required placeholder='https://… or webcal://…'></label>\
     <button>Add it</button></form>"
        .into()
}

fn light(h: Option<connect::Health>) -> String {
    match h {
        Some(h) if h.ok => "<span class='tag ok'>Working</span>".into(),
        Some(h) => format!("<span class='tag bad'>Not working</span> <span class=note>{}</span>", esc(&h.said)),
        None => "<span class=tag>Not tried yet</span>".into(),
    }
}

fn disconnect(kind: &str, id: &str) -> String {
    format!(
        "<form method=post action='/hub/connect' class=inline><input type=hidden name=what value=disconnect>\
         <input type=hidden name=kind value={kind}><input type=hidden name=id value='{}'><button>Disconnect</button></form>",
        esc(id)
    )
}

/// Everything connected, each with whether it's working.
fn connected_list(d: &mut Daemon) -> String {
    let mine: Vec<crate::mail::Account> = d.store.load(crate::daemon::CONNECTED_ACCOUNTS);
    let accounts = d.tools_cfg().mail.accounts.clone();
    let links: Vec<CalendarLink> = d.store.load(connect::CALENDAR_LINKS);
    if accounts.is_empty() && links.is_empty() {
        return "<p class=note>Nothing connected yet.</p>".into();
    }
    let mut out = String::from("<h3>Connected</h3><ul class=connected>");
    for a in &accounts {
        let ours = mine.iter().any(|m| m.address.eq_ignore_ascii_case(&a.address));
        out.push_str(&format!(
            "<li><b>{}</b> mail {} {}</li>",
            esc(&a.address),
            light(connect::health_of(&d.store, &a.address)),
            if ours { disconnect("mail", &a.address) } else { "<span class=note>(listed in tools.yaml)</span>".to_string() }
        ));
    }
    for l in &links {
        let h = (l.last_read > 0).then(|| connect::Health { at: l.last_read, ok: l.last_ok, said: l.last_said.clone() });
        out.push_str(&format!("<li><b>{}</b> calendar {} {}</li>", esc(&l.name), light(h), disconnect("calendar", &l.url)));
    }
    if !d.tools_cfg().mail.enabled && !accounts.is_empty() {
        out.push_str("<li class=note>Reading mail is switched off in Settings, so these aren't being read.</li>");
    }
    out.push_str("</ul>");
    out
}

/// POST /hub/connect.
pub fn post(d: &mut Daemon, fields: &[(String, String)]) -> Reply {
    if d.handover().stance.handed_over() {
        return back("Accounts are the owner's to connect.");
    }
    match field(fields, "what").as_str() {
        "start" => {
            let typed = field(fields, "address");
            let Some(address) = connect::address(&typed) else {
                return back("That doesn't look like an email address -- it should be like name@example.com.");
            };
            if let Some(Way::LookItUp { domain }) = connect::way_for(&address) {
                look_up(d, &domain);
            }
            hub::back_with(&format!("{}#connect-next", Page::Accounts.href()), &format!("connect={}", crate::research::urlencode(&address)), "")
        }
        "password" => connect_mailbox(d, fields),
        "calendar" => add_calendar(d, &field(fields, "url")),
        "disconnect" => {
            let id = field(fields, "id");
            match field(fields, "kind").as_str() {
                "mail" => match d.drop_connected_account(&id) {
                    Ok(true) => {
                        d.vault.secrets.retain(|s| s.name != connect::vault_name(&id));
                        let _ = d.vault.save(&crate::roots::install_state());
                        back(&format!("Disconnected {id}, and its password is gone from the vault. You can also delete the app password at your provider."))
                    }
                    Ok(false) => back(&format!("{id} is listed in tools.yaml, so it's taken off there.")),
                    Err(e) => back(&format!("I couldn't disconnect it: {e}")),
                },
                "calendar" => {
                    let mut links: Vec<CalendarLink> = d.store.load(connect::CALENDAR_LINKS);
                    links.retain(|l| l.url != id);
                    match d.store.save(connect::CALENDAR_LINKS, &links) {
                        Ok(()) => back("That calendar won't be read again. Its events already here stay until you remove them."),
                        Err(e) => back(&format!("I couldn't keep that: {e}")),
                    }
                }
                _ => back("That button isn't wired to anything, so nothing changed."),
            }
        }
        _ => back("That button isn't wired to anything, so nothing changed."),
    }
}

/// An unknown provider, looked up in Mozilla's ISPDB and kept for the page.
fn look_up(d: &mut Daemon, domain: &str) {
    let url = connect::ispdb_url(domain);
    let Some((host, path)) = connect::host_and_path(&url) else { return };
    let Ok(r) = crate::http::https_get(&host, &path, std::time::Duration::from_secs(8)) else { return };
    if !r.ok() {
        return;
    }
    if let Some(found) = connect::imap_from_autoconfig(&r.body) {
        let mut all: std::collections::BTreeMap<String, (String, u16)> = d.store.load(LOOKED_UP);
        all.insert(domain.to_string(), found);
        let _ = d.store.save(LOOKED_UP, &all);
    }
}

/// Try the password against the mail server; keep it only if it worked.
fn connect_mailbox(d: &mut Daemon, fields: &[(String, String)]) -> Reply {
    let Some(address) = connect::address(&field(fields, "address")) else { return back("That address went missing -- type it again.") };
    let password = fields.iter().find(|(k, _)| k == "password").map(|(_, v)| v.clone()).unwrap_or_default();
    let host = field(fields, "host").trim_start_matches("imaps://").to_string();
    let port: u16 = field(fields, "port").parse().unwrap_or(993);
    if password.trim().is_empty() || host.is_empty() {
        return back("The password box was empty, so nothing was tried.");
    }
    let again = |said: &str| hub::back_with(&format!("{}#connect-next", Page::Accounts.href()), &format!("connect={}", crate::research::urlencode(&address)), said);
    // The vault first: there's no point proving a password that can't be kept.
    let now = crate::store::now();
    if d.vault.state() != crate::vault::State::Open {
        let opened = if d.vault.sealed_to_this_login() {
            d.vault.open_unattended(now)
        } else {
            let phrase = field(fields, "passphrase");
            if phrase.is_empty() {
                Err("your vault is locked -- type its passphrase too".to_string())
            } else {
                let cfg = d.tools_cfg().vault.clone();
                d.vault.open(&phrase, now, &cfg)
            }
        };
        if let Err(e) = opened {
            return again(&format!("Nothing was connected: {e}."));
        }
    }
    // Spaces in an app password are how Google shows it, not part of it.
    let password = if host.contains("gmail") { password.replace(' ', "") } else { password };
    let tried = crate::imap::connect(&host, port).and_then(|mut s| s.login(&address, &password));
    if let Err(e) = tried {
        connect::note_health(&d.store, &address, Some(&e));
        return again(&format!(
            "{host} didn't let me in, so nothing was kept: {e}. Check it's an app password (not your usual one) and try again."
        ));
    }
    let vault_name = connect::vault_name(&address);
    let kept = d
        .vault
        .put(&vault_name, crate::vault::Kind::Login, &password, now)
        .and_then(|()| d.vault.save(&crate::roots::install_state()).map_err(|e| e.to_string()));
    if let Err(e) = kept {
        return again(&format!("The password worked, but I couldn't seal it in the vault, so nothing was kept: {e}."));
    }
    let account = crate::mail::Account {
        name: address.split('@').next().unwrap_or(&address).to_string(),
        address: address.clone(),
        imap_host: host,
        password_from_vault: vault_name,
        ..Default::default()
    };
    connect::note_health(&d.store, &address, None);
    match d.keep_connected_account(account) {
        Ok(true) => back(&format!("Connected {address}. I'll read it with your mail from now on.")),
        Ok(false) => {
            // Connecting a mailbox is asking for it to be read.
            let dir = crate::roots::config_dir();
            let mut prefs = crate::preferences::Preferences::load(&dir);
            prefs.set("mail.enabled", "true");
            match prefs.save(&dir) {
                Ok(()) => back(&format!("Connected {address}, and reading mail is now on -- I'll start within a minute.")),
                Err(e) => back(&format!("Connected {address}, but I couldn't switch reading mail on ({e}) -- turn on Email in Settings.")),
            }
        }
        Err(e) => back(&format!("The password worked and is sealed, but I couldn't add the account: {e}.")),
    }
}

fn add_calendar(d: &mut Daemon, typed: &str) -> Reply {
    let url = match connect::calendar_link(typed) {
        Ok(u) => u,
        Err(e) => return back(&e),
    };
    let mut links: Vec<CalendarLink> = d.store.load(connect::CALENDAR_LINKS);
    if links.iter().any(|l| l.url == url) {
        return back("That calendar is already connected.");
    }
    let name = connect::calendar_name(&url);
    links.push(CalendarLink { name: name.clone(), url, ..Default::default() });
    match d.store.save(connect::CALENDAR_LINKS, &links) {
        Ok(()) => back(&format!("Added {name}. I'm reading it now; its events appear on your calendar within a minute.")),
        Err(e) => back(&format!("I couldn't keep that: {e}")),
    }
}

/// Once a tick: start reading any calendar link that's due, and take in what
/// came back. Reading happens on its own thread; a slow calendar never holds
/// anything up.
pub fn tick(d: &mut Daemon, now: u64) {
    // Every half minute is plenty for links read every 15; the store isn't
    // read every second for nothing.
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let waiting = READ.lock().map(|r| !r.is_empty()).unwrap_or(false);
    if !waiting && now.saturating_sub(LAST.load(std::sync::atomic::Ordering::Relaxed)) < 30 {
        return;
    }
    LAST.store(now, std::sync::atomic::Ordering::Relaxed);
    let mut links: Vec<CalendarLink> = d.store.load(connect::CALENDAR_LINKS);
    if links.is_empty() {
        return;
    }
    let came: Vec<(String, Result<String, String>)> = READ.lock().map(|mut r| std::mem::take(&mut *r)).unwrap_or_default();
    let mut changed = false;
    for (url, got) in came {
        let Some(l) = links.iter_mut().find(|l| l.url == url) else { continue };
        let home = d.home_zone();
        let took = got.and_then(|text| d.calendar.import_ics(&text, now, &home).map(|(n, _)| n));
        match took {
            Ok(n) => {
                let _ = d.calendar.save(&d.store);
                l.last_ok = true;
                l.last_said = format!("{n} event{} changed", if n == 1 { "" } else { "s" });
            }
            Err(e) => {
                l.last_ok = false;
                l.last_said = e;
            }
        }
        changed = true;
    }
    for l in links.iter_mut().filter(|l| connect::read_due(l, now)) {
        l.last_read = now;
        changed = true;
        let url = l.url.clone();
        std::thread::spawn(move || {
            let got = connect::host_and_path(&url)
                .ok_or_else(|| "not an https link".to_string())
                .and_then(|(host, path)| {
                    crate::http::https_get(&host, &path, std::time::Duration::from_secs(20)).map_err(|e| e.to_string())
                })
                .and_then(|r| if r.ok() { Ok(r.body) } else { Err(format!("the calendar's site answered {}", r.status)) });
            if let Ok(mut q) = READ.lock() {
                q.push((url, got));
            }
        });
    }
    if changed {
        let _ = d.store.save(connect::CALENDAR_LINKS, &links);
    }
}
