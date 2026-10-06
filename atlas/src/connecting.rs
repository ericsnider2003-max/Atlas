//! The daemon's side of `connect`: the "Connect an account" section at the
//! top of the Accounts page, what its buttons do, and the calendar links read
//! again in the background.
//!
//! Free functions taking the daemon, so every call site names this module.

use crate::connect::{self, CalendarLink, Way};
use crate::oauthlink::{self, Provider};
use crate::daemon::Daemon;
use crate::hub::{self, esc, Page};
use crate::server::Reply;
use std::sync::Mutex;

/// Where a lookup of an unknown provider is kept between the "Next" press
/// and the page that shows the result: domain -> (host, port).
const LOOKED_UP: &str = "connect_looked_up";

/// Calendar links read on their own thread, waiting to be taken in.
static READ: Mutex<Vec<(String, Result<String, String>)>> = Mutex::new(Vec::new());

/// Finished one-click sign-ins, waiting for the tick to keep them.
static SIGNED: Mutex<Vec<Result<oauthlink::SignedIn, String>>> = Mutex::new(Vec::new());

/// What the last one-click sign-in came to, shown at the top of the section
/// until the next one (the sign-in finishes in the browser, after the page
/// that started it has gone).
const SIGNIN_SAID: &str = "connect_signin_said";

/// How long a sign-in waits for the browser to come back.
const SIGNIN_WAIT_SECS: u64 = 300;

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
         <p class=what>Sign in with Google or Microsoft in one click, or type any email address and I'll \
         work out the rest.</p>",
    );
    let said: String = d.store.load(SIGNIN_SAID);
    if !said.is_empty() {
        out.push_str(&format!("<p class=said role=status>{}</p>", esc(&said)));
    }
    out.push_str(&one_click(d));
    out.push_str(
        "<form method=post action='/hub/connect' class=inline><input type=hidden name=what value=start>\
         <label>Email address <input name=address type=email autocomplete=email required></label>\
         <button>Next</button></form>",
    );
    if let Some(a) = asked.and_then(connect::address) {
        out.push_str(&step_for(d, &a));
    }
    out.push_str(&calendar_form());
    out.push_str(&connected_list(d));
    out.push_str(
        "<p class=note>YouTube, Bluesky and the other networks connect on the <a href='/hub/social'>Social \
         page</a>.</p></section>",
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
            if m.provider == "Gmail" {
                out.push_str(
                    "<p class=note>Why a password for Gmail and not the Google button: Google only lets apps read \
                     Gmail after a paid security audit. Your Google Calendar connects with the button above.</p>",
                );
            }
        }
        Way::MicrosoftSignIn => {
            out.push_str(
                "<p><b>Outlook / Hotmail.</b> Microsoft switched passwords off for these in 2024, so they connect \
                 through Microsoft's own sign-in -- one click, and it brings your Outlook calendar too.</p>",
            );
            out.push_str(&signin_button(d, Provider::Microsoft));
        }
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
///
/// Never asked for a new vault: one opens on your Windows sign-in (5 Oct
/// 2026). Only a vault made before that, with no sign-in copy yet, asks --
/// once.
fn vault_field(d: &Daemon) -> String {
    let opens_itself = d.vault.state() == crate::vault::State::Open
        || d.vault.sealed_to_this_login()
        || (d.vault.is_brand_new() && d.tools_cfg().vault.open_on_this_login && crate::loginseal::available());
    if opens_itself {
        return String::new();
    }
    if d.vault.has_a_passphrase() {
        "<label>Your vault passphrase -- once; after this it opens with your Windows sign-in \
         <input name=passphrase type=password autocomplete=current-password required></label>"
            .into()
    } else {
        // Off Windows only: there's no sign-in to seal it to.
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
        "oauth" => match Provider::from_key(&field(fields, "provider")) {
            Some(p) => start_signin(d, p, fields),
            None => back("That button isn't wired to anything, so nothing changed."),
        },
        "calendar" => add_calendar(d, &field(fields, "url")),
        "disconnect" => {
            let id = field(fields, "id");
            match field(fields, "kind").as_str() {
                "mail" => match d.drop_connected_account(&id) {
                    Ok(true) => {
                        let token = oauthlink::vault_name(Provider::Microsoft, &id);
                        let calendar_uses_it = d
                            .store
                            .load::<Vec<CalendarLink>>(connect::CALENDAR_LINKS)
                            .iter()
                            .any(|l| l.url == oauthlink::calendar_key(Provider::Microsoft, &id));
                        d.vault.secrets.retain(|s| s.name != connect::vault_name(&id) && (calendar_uses_it || s.name != token));
                        let _ = d.vault.save(&crate::roots::install_state());
                        back(&format!("Disconnected {id}, and its password is gone from the vault. You can also delete the app password at your provider."))
                    }
                    Ok(false) => back(&format!("{id} is listed in tools.yaml, so it's taken off there.")),
                    Err(e) => back(&format!("I couldn't disconnect it: {e}")),
                },
                "calendar" => {
                    let mut links: Vec<CalendarLink> = d.store.load(connect::CALENDAR_LINKS);
                    links.retain(|l| l.url != id);
                    // A sign-in's token goes with its calendar, unless Outlook mail still uses it.
                    if let Some((p, email)) = oauthlink::parse_calendar_key(&id) {
                        let mail_uses_it = d
                            .store
                            .load::<Vec<crate::mail::Account>>(crate::daemon::CONNECTED_ACCOUNTS)
                            .iter()
                            .any(|a| a.password_from_vault == oauthlink::vault_name(p, &email));
                        if !mail_uses_it {
                            d.vault.secrets.retain(|s| s.name != oauthlink::vault_name(p, &email));
                            let _ = d.vault.save(&crate::roots::install_state());
                        }
                    }
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
    if let Err(why) = d.vault_ready(now) {
        let opened = {
            let phrase = field(fields, "passphrase");
            if phrase.is_empty() {
                Err(why)
            } else {
                let cfg = d.tools_cfg().vault.clone();
                d.vault.open(&phrase, now, &cfg).map(|()| {
                    let _ = d.keep_sign_in_copy(now);
                })
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
    let waiting = READ.lock().or_else(crate::crash::unpoison).map(|r| !r.is_empty()).unwrap_or(false) || SIGNED.lock().or_else(crate::crash::unpoison).map(|r| !r.is_empty()).unwrap_or(false);
    if !waiting && now.saturating_sub(LAST.load(std::sync::atomic::Ordering::Relaxed)) < 30 {
        return;
    }
    LAST.store(now, std::sync::atomic::Ordering::Relaxed);
    keep_signed_in(d, now);
    keep_rotated(d, now);
    let mut links: Vec<CalendarLink> = d.store.load(connect::CALENDAR_LINKS);
    if links.is_empty() {
        return;
    }
    let came: Vec<(String, Result<String, String>)> = READ.lock().or_else(crate::crash::unpoison).map(|mut r| std::mem::take(&mut *r)).unwrap_or_default();
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
        if let Some((p, email)) = oauthlink::parse_calendar_key(&url) {
            // Signed in: the token from the vault, read on its own thread.
            let token = d.vault.get(&oauthlink::vault_name(p, &email), now);
            std::thread::spawn(move || {
                let got = token
                    .map_err(|e| format!("its sign-in couldn't be read from the vault ({e})"))
                    .and_then(|t| oauthlink::calendar_ics(&crate::social::apis::Https, p, &t, now));
                if let Ok(mut q) = READ.lock().or_else(crate::crash::unpoison) {
                    q.push((url, got));
                }
            });
            continue;
        }
        std::thread::spawn(move || {
            let got = connect::host_and_path(&url)
                .ok_or_else(|| "not an https link".to_string())
                .and_then(|(host, path)| {
                    crate::http::https_get(&host, &path, std::time::Duration::from_secs(20)).map_err(|e| e.to_string())
                })
                .and_then(|r| if r.ok() { Ok(r.body) } else { Err(format!("the calendar's site answered {}", r.status)) });
            if let Ok(mut q) = READ.lock().or_else(crate::crash::unpoison) {
                q.push((url, got));
            }
        });
    }
    if changed {
        let _ = d.store.save(connect::CALENDAR_LINKS, &links);
    }
}

// ------------------------------------------------------------ one click

/// The sign-in buttons: one each, saying what it connects.
fn one_click(d: &Daemon) -> String {
    let mut out = String::from("<div class=oneclick>");
    out.push_str(&signin_button(d, Provider::Google));
    out.push_str(&signin_button(d, Provider::Microsoft));
    out.push_str(
        "<p class=note>You sign in on Google's or Microsoft's own page, in your browser on this computer. \
         Atlas never sees your password: it's given read access you can take back any time, from your \
         account or with Disconnect below.</p></div>",
    );
    out
}

fn signin_button(d: &Daemon, p: Provider) -> String {
    if p == Provider::Google && oauthlink::google_secret().is_none() {
        return "<p class=note>Google's button isn't in this copy of Atlas (it was built without Google's \
                sign-in key). Your Google calendar can still connect by its private link below.</p>"
            .into();
    }
    format!(
        "<form method=post action='/hub/connect' class=inline><input type=hidden name=what value=oauth>\
         <input type=hidden name=provider value={}>{}<button class=primary>Sign in with {}</button> \
         <span class=note>for {}</span></form>",
        p.key(),
        vault_field(d),
        p.name(),
        p.connects()
    )
}

/// Open the vault if it's shut (the token is kept there), then open the
/// provider's page in the browser and wait on this machine for it to come back.
fn start_signin(d: &mut Daemon, p: Provider, fields: &[(String, String)]) -> Reply {
    match begin_signin(d, p, &field(fields, "passphrase")) {
        Ok(said) | Err(said) => back(&said),
    }
}

/// What "connect my Google calendar" / "connect my Outlook" / "sign me into
/// my socials" asks for (5 Oct 2026, Eric: finding the right page and button
/// was "overly complicated and highly annoying").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connect {
    Google,
    /// Your channel's numbers, through the same Google sign-in.
    Youtube,
    Microsoft,
    Socials,
}

/// Read a request to connect an account, said or typed. Only when the
/// sentence is about connecting: "what's on my google calendar" isn't.
pub fn connect_asked(said: &str) -> Option<Connect> {
    let l = said.trim().trim_end_matches(['.', '!', '?']).to_ascii_lowercase();
    let l = l.trim_start_matches("please ").trim_start_matches("can you ").trim_start_matches("atlas, ").trim();
    let asks = ["connect ", "link ", "sign me in", "sign me into", "sign in to my", "sign into my", "sign in with ", "log me in", "log me into", "hook up ", "set up my "]
        .iter()
        .any(|p| l.starts_with(p));
    if !asks {
        return None;
    }
    if l.contains("youtube") && !l.contains("youtube studio") {
        return Some(Connect::Youtube);
    }
    let social = ["social", "socials", "social media", "instagram", "tiktok", "facebook", "linkedin", "reddit", "twitter", "youtube studio"];
    if social.iter().any(|w| l.contains(w)) || l.ends_with(" x") {
        return Some(Connect::Socials);
    }
    if ["outlook", "microsoft", "hotmail", "live.com", "office 365", "office365"].iter().any(|w| l.contains(w)) {
        return Some(Connect::Microsoft);
    }
    if l.contains("google") || l.contains("gmail calendar") || l.ends_with("my calendar") || l.ends_with("calendar") {
        return Some(Connect::Google);
    }
    None
}

/// Start a provider's sign-in in your own browser. `Ok` and `Err` are both a
/// sentence for you; the result is said again when you've finished there
/// (`keep_signed_in`).
pub(crate) fn begin_signin(d: &mut Daemon, p: Provider, passphrase: &str) -> Result<String, String> {
    if p == Provider::Google && oauthlink::google_secret().is_none() {
        return Err(oauthlink::NO_GOOGLE_SECRET.into());
    }
    let now = crate::store::now();
    // No passphrase asked for: the vault opens on your Windows sign-in. A
    // passphrase is used only for a vault made before that, and only if given.
    if let Err(e) = d.vault_ready(now) {
        if passphrase.is_empty() {
            return Err(format!("Nothing was started: {e}."));
        }
        let cfg = d.tools_cfg().vault.clone();
        if let Err(e) = d.vault.open(passphrase, now, &cfg) {
            return Err(format!("Nothing was started: {e}."));
        }
        let _ = d.keep_sign_in_copy(now);
    }
    let first = match std::net::TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => return Err(format!("I couldn't open a port for {} to come back to: {e}", p.name())),
    };
    let port = first.local_addr().map(|a| a.port()).unwrap_or(0);
    let mut listeners = vec![first];
    // `localhost` may be IPv6 to the browser: listen there on the same port too.
    if p == Provider::Microsoft {
        if let Ok(l) = std::net::TcpListener::bind(("::1", port)) {
            listeners.push(l);
        }
    }
    let redirect = p.redirect(port);
    let verifier = crate::vault::short_code(64);
    let state = crate::vault::short_code(24);
    let url = oauthlink::consent_url(p, &redirect, &state, &oauthlink::challenge(&verifier));
    if let Err(e) = d.plat.open_path(&url) {
        return Err(format!("I couldn't open your browser for {}'s sign-in: {e}", p.name()));
    }
    let _ = d.store.save(SIGNIN_SAID, &format!("Waiting for {}'s sign-in in your browser...", p.name()));
    std::thread::spawn(move || {
        let got = wait_for_code(&listeners, p, &state).and_then(|code| {
            oauthlink::exchange(&crate::social::apis::Https, p, &code, &redirect, &verifier)
        });
        if let Ok(mut q) = SIGNED.lock().or_else(crate::crash::unpoison) {
            q.push(got);
        }
    });
    Ok(format!(
        "{}'s sign-in is open in your browser. Pick your account and allow access; I'll say when it's connected.",
        p.name()
    ))
}

/// Answer the browser's redirect, and give back the code in it.
fn wait_for_code(listeners: &[std::net::TcpListener], p: Provider, state: &str) -> Result<String, String> {
    use std::io::{BufRead, Write};
    for l in listeners {
        let _ = l.set_nonblocking(true);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(SIGNIN_WAIT_SECS);
    while std::time::Instant::now() < deadline {
        for l in listeners {
            let stream = match l.accept() {
                Ok((s, _)) => s,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(format!("the sign-in's port failed: {e}")),
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut line = String::new();
            let _ = std::io::BufReader::new(&stream).read_line(&mut line);
            let mut w = &stream;
            // A browser also asks for /favicon.ico and the like: not the answer.
            if !line.contains("code=") && !line.contains("error=") {
                let _ = write!(w, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let code = oauthlink::code_from(p, &line, state);
            let page = match &code {
                Ok(_) => "Signed in. You can close this tab and go back to Atlas.".to_string(),
                Err(e) => format!("That didn't work: {e}. Go back to Atlas to try again."),
            };
            let _ = write!(
                w,
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{page}",
                page.len()
            );
            return code;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Err(format!("the {} sign-in wasn't finished within five minutes", p.name()))
}

/// Keep what finished sign-ins brought back: the token in the vault, the
/// calendar in the list, and (Microsoft) the Outlook mailbox in mail.
fn keep_signed_in(d: &mut Daemon, now: u64) {
    let came: Vec<Result<oauthlink::SignedIn, String>> = SIGNED.lock().or_else(crate::crash::unpoison).map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    for got in came {
        let said = match got {
            Err(e) => format!("Nothing was connected: {e}."),
            Ok(s) => keep_sign_in(d, &s, now),
        };
        let _ = d.store.save(SIGNIN_SAID, &said);
        // Said too, not only shown on the Connections page: a sign-in started
        // by asking is finished in the browser, away from any page.
        d.to_say_aloud.push(said);
    }
}

/// Keep one finished sign-in, and say what it came to (also shown at the top
/// of the section).
pub fn keep_sign_in(d: &mut Daemon, s: &oauthlink::SignedIn, now: u64) -> String {
    let said = keep_inner(d, s, now);
    let _ = d.store.save(SIGNIN_SAID, &said);
    said
}

fn keep_inner(d: &mut Daemon, s: &oauthlink::SignedIn, now: u64) -> String {
    let name = oauthlink::vault_name(s.provider, &s.email);
    let _ = d.vault_ready(now);
    let kept = d
        .vault
        .put(&name, crate::vault::Kind::Login, &s.refresh_token, now)
        .and_then(|()| d.vault.save(&crate::roots::install_state()).map_err(|e| e.to_string()));
    if let Err(e) = kept {
        return format!("{} let me in as {}, but I couldn't seal the sign-in in the vault, so nothing was kept: {e}.", s.provider.name(), s.email);
    }
    let key = oauthlink::calendar_key(s.provider, &s.email);
    let mut links: Vec<CalendarLink> = d.store.load(connect::CALENDAR_LINKS);
    links.retain(|l| l.url != key);
    let label = match s.provider {
        Provider::Google => format!("Google Calendar ({})", s.email),
        Provider::Microsoft => format!("Outlook Calendar ({})", s.email),
    };
    links.push(CalendarLink { name: label, url: key, ..Default::default() });
    let _ = d.store.save(connect::CALENDAR_LINKS, &links);
    if s.provider == Provider::Google {
        return format!("Connected {}'s Google Calendar. Its events appear on your calendar within a minute.", s.email);
    }
    match d.keep_connected_account(oauthlink::outlook_account(&s.email)) {
        Ok(true) => format!("Connected {}: Outlook mail and calendar. I'll read both from now on.", s.email),
        Ok(false) => {
            let dir = crate::roots::config_dir();
            let mut prefs = crate::preferences::Preferences::load(&dir);
            prefs.set("mail.enabled", "true");
            match prefs.save(&dir) {
                Ok(()) => format!("Connected {}: Outlook mail and calendar, and reading mail is now on.", s.email),
                Err(e) => format!("Connected {}'s calendar; turn on Email in Settings for its mail ({e}).", s.email),
            }
        }
        Err(e) => format!("Connected {}'s calendar, but I couldn't add its mail: {e}.", s.email),
    }
}

/// Write rotated Microsoft refresh tokens over the ones they replace (Q7).
/// The vault entries that can hold one: Outlook mailboxes signed in with
/// Microsoft, and signed-in calendars. A locked vault keeps them queued.
pub fn keep_rotated(d: &mut Daemon, now: u64) {
    let waiting = crate::msoauth::take_rotations();
    if waiting.is_empty() {
        return;
    }
    if d.vault_ready(now).is_err() {
        crate::msoauth::keep_later(waiting);
        return;
    }
    let mut names: Vec<String> = d
        .store
        .load::<Vec<crate::mail::Account>>(crate::daemon::CONNECTED_ACCOUNTS)
        .iter()
        .chain(d.tools_cfg().mail.accounts.iter())
        .filter(|a| a.oauth)
        .map(|a| a.password_from_vault.clone())
        .collect();
    for l in d.store.load::<Vec<CalendarLink>>(connect::CALENDAR_LINKS) {
        if let Some((p, email)) = oauthlink::parse_calendar_key(&l.url) {
            names.push(oauthlink::vault_name(p, &email));
        }
    }
    names.sort();
    names.dedup();
    let mut changed = false;
    for (old, new) in &waiting {
        for name in &names {
            if d.vault.get(name, now).as_deref() == Ok(old.as_str()) && d.vault.put(name, crate::vault::Kind::Login, new, now).is_ok() {
                changed = true;
            }
        }
    }
    if changed {
        if let Err(e) = d.vault.save(&crate::roots::install_state()) {
            // Kept in the open vault already; the next vault save writes it.
            d.log.warn(&format!("a renewed Microsoft sign-in couldn't be saved to the vault yet: {e}"));
        }
    }
}
