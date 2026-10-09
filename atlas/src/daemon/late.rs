//! The daemon's smaller jobs, each its own `impl` block: code typing and sign-ins,
//! routines, goals, the later list, mail sorting, scheduled posts, pressing buttons,
//! storage, undo, media edits, reading documents off the loop, bringing things back,
//! advice, gestures and keys.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;
use super::running::{LeftWaiting, LEFT_WAITING};

#[cfg(test)]
mod approved_undo_retry_tests {
    use super::*;
    fn exercise(cancel: bool, change_source: bool) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("atlas-approved-undo-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let store = crate::store::Store::new(&path);
        let cfg = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(vec![]);
        let mut daemon = Daemon::new(&cfg, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let original = path.join("original.txt"); let moved = path.join("sorted.txt");
        std::fs::write(&original, b"owner bytes").unwrap();
        let evidence = crate::tune::RecordedMove::new(&original, &moved).unwrap();
        std::fs::rename(&original, &moved).unwrap();
        let id = daemon.history.note("moved one file", "files", crate::undo::Undo::Atlas("put it back".into()), true, 10);
        daemon.history.save_merged(&store).unwrap();
        store.save(crate::tune::TUNE_UNDO_RECORD, &vec![(id, crate::tune::TuneUndo::RecordedMoves { moves: vec![evidence], made: vec![] })]).unwrap();
        let root = store.root().to_path_buf();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel(); let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || { let _guard = crate::store::state_transaction(&root).unwrap(); ready_tx.send(()).unwrap(); release_rx.recv().unwrap(); });
        ready_rx.recv().unwrap();
        assert!(daemon.carry_out_undo(id).contains("waiting for recovery storage"));
        assert!(!original.exists()); assert_eq!(std::fs::read(&moved).unwrap(), b"owner bytes");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !daemon.undo_waiting_for_ack() {
            assert!(daemon.poll_file_moves(20).is_empty());
            assert!(std::time::Instant::now() < deadline, "undo worker never reached its durable ACK");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!original.exists());
        if cancel { assert!(daemon.cancel_approved_undo()); }
        if change_source { std::fs::write(&moved, b"new owner bytes").unwrap(); }
        release_tx.send(()).unwrap(); worker.join().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let result = loop {
            let news = daemon.poll_file_moves(30);
            if let Some(said) = news.into_iter().next() { break Some(said); }
            assert!(std::time::Instant::now() < deadline, "undo worker never reached a terminal result");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        if cancel { assert!(!result.unwrap().starts_with("Undone")); assert!(!original.exists()); }
        else if change_source { assert!(!result.unwrap().starts_with("Undone")); assert!(!original.exists()); assert_eq!(std::fs::read(&moved).unwrap(), b"new owner bytes"); }
        else { assert!(result.unwrap().starts_with("Undone")); assert_eq!(std::fs::read(&original).unwrap(), b"owner bytes"); assert!(!moved.exists()); }
        drop(daemon); let _ = std::fs::remove_dir_all(path);
    }
    #[test] fn approval_waits_without_moving_then_retries_after_backup_release() { exercise(false, false); }
    #[test] fn stopping_a_waiting_undo_prevents_later_movement() { exercise(true, false); }
    #[test] fn changed_owner_bytes_are_not_moved_by_a_waiting_undo() { exercise(false, true); }
}

#[cfg(test)]
mod routine_durable_origin_tests {
    #![cfg(windows)]
    use super::*;
    use crate::proactive::{Proactive, ProactiveConfig};
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn a_failed_routine_reservation_does_not_duplicate_its_saved_queue_after_restart() {
        let path = std::env::temp_dir().join(format!("atlas-routine-origin-{}-{}", std::process::id(), crate::store::now()));
        std::fs::create_dir(&path).unwrap(); let store = crate::store::Store::new(&path);
        let mut cfg = crate::config::Config::load(std::path::Path::new("config")).unwrap(); cfg.tools.as_mut().unwrap().routine.enabled = true;
        let platform = crate::platform::mock::MockPlatform::new(vec![]);
        let t = 1_790_740_000;
        let mut daemon = Daemon::new(&cfg, &platform, None, store.clone(), Proactive::new(ProactiveConfig::default()));
        let hour = crate::localclock::hour(t, crate::localclock::offset_secs()) as u32;
        daemon.routines.routines.push(crate::routine::Routine { name: "fixture".into(), steps: vec!["show my later list".into()], seen: 3, usual_hour: hour, usual_weekday: None, confirmed: true, automatic: true, asked: true, last_day: 0 });
        store.save("routines", &daemon.routines).unwrap();
        let blocker = std::fs::OpenOptions::new().read(true).share_mode(0).open(store.root().join("routines.json")).unwrap();
        let said = daemon.routines_on_the_hour(t);
        assert!(said.iter().any(|line| line.starts_with("Queued your usual fixture")));
        let ids: Vec<_> = daemon.queue.tasks.iter().map(|task| task.id).collect(); assert_eq!(ids.len(), 1);
        drop(daemon); drop(blocker);
        let mut restarted = Daemon::new(&cfg, &platform, None, store.clone(), Proactive::new(ProactiveConfig::default()));
        assert_eq!(restarted.routines.routines[0].last_day, 0);
        restarted.routines_on_the_hour(t);
        assert_eq!(restarted.queue.tasks.iter().map(|task| task.id).collect::<Vec<_>>(), ids);
        assert!(restarted.routines.routines[0].last_day > 0);
        drop(restarted); let _ = std::fs::remove_dir_all(path);
    }
}

impl<'a> Daemon<'a> {
    fn queue_routine_once(&mut self, key: &str, steps: &[String], t: u64) -> crate::error::Result<bool> {
        let _guard = self.store.transaction()?;
        let before = self.queue.clone();
        let (_, added) = self.queue.enqueue_origin_once(key, steps, crate::lanes::Lane::Foreground, t);
        if added {
            if let Err(error) = self.queue.save(&self.store) { self.queue = before; return Err(error); }
        }
        Ok(added)
    }

    /// You at the machine: your own keyboard or mouse in the last five
    /// minutes (Atlas's own typing isn't counted, see `platform::idle`), or
    /// a voice Atlas knows is yours. A turn from the phone is neither.
    fn you_are_at_the_machine(&self) -> bool {
        // A locked machine has nobody at it, whoever spoke last (H13a).
        if self.plat.session_locked() == Some(true) {
            return false;
        }
        if matches!(self.last_verdict, crate::voiceid::Verdict::You(_)) {
            return true;
        }
        if matches!(self.last_verdict, crate::voiceid::Verdict::NotYou(_)) {
            return false;
        }
        self.plat.input_idle_secs().map(|s| s <= 300).unwrap_or(true)
    }

    /// Where a code you asked to be typed goes: the sign-in Atlas is in the
    /// middle of, or the window in front of you.
    fn code_target(&self) -> Option<(crate::twofactor::Target, Option<String>)> {
        if let Some(site) = &self.signing_in_waiting {
            return Some((crate::twofactor::Target::AtlasBrowser { site: site.clone() }, Some(site.clone())));
        }
        let win = self.plat.active_window_id().ok().flatten()?;
        let title = self.plat.active_window().ok().flatten().map(|w| w.title).unwrap_or_default();
        // The site, when the window's title names one ("Sign in - Google
        // Accounts — Chrome"), so a code from that site is preferred.
        let site = title
            .split(|c: char| !c.is_alphanumeric() && c != '.')
            .map(str::to_lowercase)
            .find(|w| {
                ["google", "microsoft", "apple", "github", "amazon", "facebook", "instagram", "paypal",
                 "discord", "reddit", "linkedin", "dropbox", "tiktok", "twitter", "chase", "outlook"]
                    .contains(&w.as_str())
                    || (w.contains('.') && w.len() > 4)
            });
        Some((crate::twofactor::Target::Window { win: win.0, title }, site))
    }

    /// "Type my code 482917" / "it's in my email" / "it's in my texts".
    pub(super) fn type_code(&mut self, said: &str, t: u64) -> String {
        use crate::twofactor::{self, Source};
        let Some((target, site)) = self.code_target() else {
            return "There's nothing in front of you for me to type a code into.".into();
        };
        match twofactor::source_of(said) {
            Source::Spoken => match twofactor::code_in_words(said) {
                Some(code) => self.put_code(&target, &code),
                None => twofactor::ask_for_it(site.as_deref()),
            },
            Source::Texts => {
                let spec = crate::config::AppSpec::for_process("PhoneExperienceHost.exe");
                let text = self
                    .plat
                    .find_window(&spec)
                    .ok()
                    .flatten()
                    .and_then(|w| self.plat.read_window(w).ok().flatten())
                    .map(|tree| tree.text());
                let Some(text) = text else {
                    return "I can only see your texts through Phone Link, and it isn't open. Open it on \
                            your messages, or read me the code."
                        .into();
                };
                let found = twofactor::from_phone_link(&text, t);
                match twofactor::newest(&found, site.as_deref(), t) {
                    Some(f) => self.put_code(&target, &f.code),
                    None => twofactor::none_found(Source::Texts, site.as_deref()),
                }
            }
            Source::Email => self.find_code_in_mail(target, site, t),
        }
    }

    /// Put a code where it was asked for, and say what happened.
    fn put_code(&mut self, target: &crate::twofactor::Target, code: &str) -> String {
        use crate::twofactor::{read_out, Target};
        match target {
            Target::Window { win, title } => {
                match crate::delegate::type_into_window(self.plat, crate::platform::WindowId(*win), code, true) {
                    Ok(()) => {
                        let place = if title.is_empty() { String::new() } else { format!(" in {title}") };
                        format!("Typed {}{place} and pressed Enter.", read_out(code))
                    }
                    Err(e) => format!("I couldn't type it: {e}. The code is {}.", read_out(code)),
                }
            }
            Target::AtlasBrowser { site } => {
                let bcfg = self.tools_cfg().browser.clone();
                let entered = crate::browser::Browser::attach(&bcfg)
                    .map_err(|e| e.to_string())
                    .and_then(|mut b| {
                        let r = crate::webrun::enter_code(&mut b, code);
                        b.close();
                        r
                    });
                match entered {
                    Ok(crate::webrun::SignedIn::In) => {
                        self.signing_in_waiting = None;
                        let mut said = format!("Code's in — you're signed in to {site}.");
                        // A security change that had to sign in first carries on.
                        if let Some(asked) = self.after_code.take() {
                            said.push(' ');
                            said.push_str(&self.make_security_change(asked, crate::store::now()));
                        }
                        said
                    }
                    Ok(crate::webrun::SignedIn::WantsCode) => {
                        format!("{site} took that and wants another code. {}", crate::twofactor::ask_for_it(Some(site)))
                    }
                    Ok(other) => other.say(site),
                    Err(e) => format!("I couldn't put the code in: {e}."),
                }
            }
        }
    }

    /// The code is in your email: look through today's mail on a crew
    /// errand, and type the newest fresh one when it comes back.
    fn find_code_in_mail(&mut self, target: crate::twofactor::Target, site: Option<String>, t: u64) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if !cfg.enabled || cfg.accounts.is_empty() {
            return "I can't read your email yet (it isn't set up), so read me the code.".into();
        }
        let (jobs, problems) = self.resolve_mail_jobs(&cfg, t);
        if jobs.is_empty() {
            return format!("I couldn't get into your email: {}. Read me the code.", problems.join("; "));
        }
        let since = crate::triage::imap_date(1, t);
        let work: crew::Work = Box::new(move |_ctl| {
            let mut found = Vec::new();
            let mut failures = Vec::new();
            for (account, password) in &jobs {
                let host = if !account.imap_host.is_empty() {
                    account.imap_host.clone()
                } else {
                    match crate::mail::Provider::from_address(&account.address).imap_host() {
                        Some(h) => h.to_string(),
                        None => continue,
                    }
                };
                match connect_and_fetch_since(
                    &host,
                    &account.address,
                    password,
                    &since,
                    account.oauth.then_some(account.client_id.as_str()),
                ) {
                    Ok(msgs) => {
                        for m in msgs.iter().rev().take(40) {
                            if let Some(code) = crate::twofactor::code_in_text(&m.subject, &m.body) {
                                found.push(crate::twofactor::Found {
                                    code,
                                    from: m.from.clone(),
                                    at: crate::triage::parse_rfc2822(&m.date).unwrap_or(0),
                                });
                            }
                        }
                    }
                    Err(e) => failures.push(e),
                }
            }
            if found.is_empty() && !failures.is_empty() {
                return Err(failures.join("; "));
            }
            serde_json::to_string(&found).map_err(|e| e.to_string())
        });
        self.code_wanted = Some((target, site));
        if self.hand_off("code", t, work, None, SpeakPolicy::Always) {
            "Looking in your email for the code.".into()
        } else {
            self.code_wanted = None;
            "I'm too busy to look right now — read me the code.".into()
        }
    }

    /// A code search, a sign-in, a security change or a sign-up came back.
    pub(super) fn two_factor_news(&mut self, label: &str, ending: &crew::Ending, t: u64) -> Option<String> {
        let json = match ending {
            crew::Ending::Done(Ok(j)) => j.clone(),
            crew::Ending::Done(Err(e)) => {
                return Some(match label {
                    "code" => {
                        self.code_wanted = None;
                        format!("I couldn't read your email for the code: {e}. Read it out and I'll type it.")
                    }
                    _ => format!("That didn't work: {e}."),
                })
            }
            _ => return None,
        };
        match label {
            "code" => {
                let (target, site) = self.code_wanted.take()?;
                let found: Vec<crate::twofactor::Found> = serde_json::from_str(&json).unwrap_or_default();
                Some(match crate::twofactor::newest(&found, site.as_deref(), t) {
                    Some(f) => self.put_code(&target, &f.code),
                    None => crate::twofactor::none_found(crate::twofactor::Source::Email, site.as_deref()),
                })
            }
            "sign-in" => {
                let o: SignInOutcome = serde_json::from_str(&json).ok()?;
                let worked = matches!(o.outcome, crate::webrun::SignedIn::In | crate::webrun::SignedIn::WantsCode);
                self.access.note_use(&o.site, &o.account, &crate::webrun::login_url(&o.site), worked, true, t);
                let _ = self.access.save(&self.store);
                if o.outcome == crate::webrun::SignedIn::WantsCode {
                    self.signing_in_waiting = Some(o.site.clone());
                }
                Some(o.outcome.say(&o.site))
            }
            "security-change" => {
                let o: SecurityOutcome = serde_json::from_str(&json).ok()?;
                if o.wants_code {
                    self.signing_in_waiting = Some(o.asked.site.clone());
                    self.after_code = Some(o.asked.clone());
                    return Some(format!(
                        "I had to sign in to {} first, and it wants a code. {} Then I'll {}.",
                        o.asked.site,
                        crate::twofactor::ask_for_it(Some(&o.asked.site)),
                        o.asked.change.plainly()
                    ));
                }
                if let Some(why) = o.failed {
                    return Some(format!("I couldn't open my browser to do it ({why}). It's yours from here: {}", o.url));
                }
                let p = o.pressed?;
                let worked = p == crate::confirmed::Pressed::Done;
                let mut trail: Vec<crate::confirmed::Record> = self.store.load("security_changes");
                trail.push(crate::confirmed::record(&o.asked, worked, "yes", t));
                let _ = self.store.save("security_changes", &trail);
                let mut said = p.say(&o.asked, &o.url);
                if worked {
                    said.push(' ');
                    said.push_str(&crate::confirmed::how_to_undo(&o.asked.change));
                }
                Some(said)
            }
            "sign-up" => {
                let o: SignUpOutcome = serde_json::from_str(&json).ok()?;
                let cfg = self.tools_cfg().enrol.clone();
                let mut e = o.enrolment;
                let said = match &o.outcome {
                    crate::webrun::SignedUp::Made => {
                        e.finish();
                        e.spoken_result(&cfg)
                    }
                    crate::webrun::SignedUp::Unconfirmed(why) => format!("The signup attempt on {} is unconfirmed ({why}). Check that service before repeating; Atlas has not verified that the account exists.", e.domain),
                    crate::webrun::SignedUp::Stopped(s) => {
                        let mut said = s.spoken();
                        if let crate::enrol::Stopped::NeedsACodeFromElsewhere(_) = s {
                            // The account exists; confirming it is a code,
                            // and B1's code route applies.
                            self.signing_in_waiting = Some(e.domain.clone());
                            said = format!(
                                "{} is asking for a code. Account creation remains unconfirmed. {}",
                                e.domain,
                                crate::twofactor::ask_for_it(Some(&e.domain))
                            );
                        }
                        said
                    }
                    crate::webrun::SignedUp::NoForm => format!(
                        "I couldn't find a sign-up form on {}. Tell me the sign-up page's address and I'll use that.",
                        e.domain
                    ),
                    crate::webrun::SignedUp::Failed(why) => format!("I couldn't make the account on {}: {why}.", e.domain),
                };
                let state = match &o.outcome { crate::webrun::SignedUp::Made => crate::enrol::SignupState::Confirmed, crate::webrun::SignedUp::NoForm | crate::webrun::SignedUp::Failed(_) => crate::enrol::SignupState::NotSubmitted, crate::webrun::SignedUp::Stopped(_) => crate::enrol::SignupState::Blocked, crate::webrun::SignedUp::Unconfirmed(_) => crate::enrol::SignupState::Unconfirmed };
                let persisted = (|| -> crate::error::Result<()> {
                    let _guard = self.store.transaction()?;
                    let mut attempts: crate::enrol::SignupAttempts = self.store.load_checked(crate::enrol::SIGNUP_ATTEMPTS)?.unwrap_or_default();
                    if !attempts.settle(&e, state, &said) { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "signup attempt identity changed; result wasn't applied").into()); }
                    self.store.save(crate::enrol::SIGNUP_ATTEMPTS, &attempts)
                })();
                if let Err(err) = persisted { return Some(format!("{said} Its result couldn't be saved ({err}); the pending attempt remains fenced. Don't repeat it before checking.")); }
                self.journal.record_at(Act::Upkeep, &format!("sign-up on {}: {}", e.domain, said), matches!(o.outcome, crate::webrun::SignedUp::Made), t);
                self.enrolling = if e.is_waiting() { Some(e) } else { None };
                Some(said)
            }
            _ => None,
        }
    }

    /// Sign in to a site in Atlas's browser, on a crew errand.
    pub(super) fn start_sign_in(&mut self, site: &str, account: &str, t: u64) -> String {
        let Some(g) = self.access.find_account(site, account).cloned() else {
            return format!("I don't have access to {site}.");
        };
        let login = match self.vault.get(&g.vault_entry, t) {
            Ok(v) => v,
            Err(e) => return format!("I couldn't get the login for {site} out of the vault: {e}."),
        };
        let Some((user, password)) = crate::enrol::Enrolment::split_login(&login) else {
            return format!("The vault entry for {site} isn't a username and password.");
        };
        let bcfg = self.tools_cfg().browser.clone();
        let vars = self.tools_cfg().vars.clone();
        let (s, a) = (g.domain.clone(), g.account.clone());
        let work: crew::Work = Box::new(move |_ctl| {
            let outcome = match crate::browser::Browser::start(&bcfg, &vars) {
                Ok(mut b) => {
                    let o = crate::webrun::sign_in(&mut b, &s, &user, &password);
                    b.close();
                    o
                }
                Err(e) => crate::webrun::SignedIn::Failed(e.to_string()),
            };
            serde_json::to_string(&SignInOutcome { site: s, account: a, outcome }).map_err(|e| e.to_string())
        });
        if self.hand_off("sign-in", t, work, Some(site.to_string()), SpeakPolicy::Always) {
            format!("Signing you in to {site} as {account}.")
        } else {
            "I'm too busy to sign in right now — ask me again in a moment.".into()
        }
    }

    /// "Turn off two-factor on github": read back, and wait for your yes.
    pub(super) fn two_factor(&mut self, said: &str) -> String {
        use crate::confirmed::{Asked, Change, Step};
        let t = said.to_lowercase();
        let change = if ["off", "disable", "remove"].iter().any(|w| t.split_whitespace().any(|x| x == *w)) {
            Change::TurnOffTwoFactor
        } else {
            Change::TurnOnTwoFactor
        };
        let Some(site) = site_named_in(&t) else {
            return format!("Which site should I {} for?", change.plainly());
        };
        let cfg = self.tools_cfg().confirmed.clone();
        let asked = Asked { site: site.clone(), account: String::new(), change };
        match crate::confirmed::read_back(
            &asked,
            self.you_are_at_the_machine(),
            self.vault.state() == crate::vault::State::Open,
            &cfg,
        ) {
            Step::ReadBack { say } => {
                self.session.ask(&say);
                self.pending_security = Some(asked);
                say
            }
            Step::Cannot(why) if why.contains("switched off") => {
                "Changing security settings is switched off. Turn on \"Security switches\" in Settings.".into()
            }
            Step::Cannot(why) => format!("I can't do that one: {why}."),
            _ => String::new(),
        }
    }

    /// Your yes to a read-back: sign in if need be, then press the switch,
    /// on a crew errand.
    pub(super) fn make_security_change(&mut self, asked: crate::confirmed::Asked, t: u64) -> String {
        let Some((url, _)) = crate::walkthrough::where_2fa_lives(&asked.site) else {
            return format!(
                "I don't know where the two-factor setting lives on {} yet, and I won't guess. \
                 Open it and I'll tell you what I see.",
                asked.site
            );
        };
        let url = url.to_string();
        let bcfg = self.tools_cfg().browser.clone();
        let vars = self.tools_cfg().vars.clone();
        // The login, if Atlas has one for this site, for signing in first.
        let login = self
            .access
            .find(&asked.site)
            .cloned()
            .and_then(|g| self.vault.get(&g.vault_entry, t).ok())
            .and_then(|v| crate::enrol::Enrolment::split_login(&v));
        let a = asked.clone();
        let work: crew::Work = Box::new(move |_ctl| {
            let mut out = SecurityOutcome { asked: a.clone(), url: url.clone(), pressed: None, wants_code: false, failed: None };
            let mut b = match crate::browser::Browser::start(&bcfg, &vars) {
                Ok(b) => b,
                Err(e) => {
                    out.failed = Some(e.to_string());
                    return serde_json::to_string(&out).map_err(|e| e.to_string());
                }
            };
            let press = |b: &mut crate::browser::Browser| -> std::result::Result<crate::confirmed::Pressed, String> {
                b.open(&url).map_err(|e| e.to_string())?;
                b.press_the_one_at(&a.change, Some(&url)).map_err(|e| e.to_string())
            };
            let mut p = press(&mut b);
            if matches!(p, Ok(crate::confirmed::Pressed::WantsYouToSignIn)) {
                if let Some((user, pw)) = &login {
                    match crate::webrun::sign_in(&mut b, &a.site, user, pw) {
                        crate::webrun::SignedIn::In => p = press(&mut b),
                        crate::webrun::SignedIn::WantsCode => {
                            out.wants_code = true;
                            b.close();
                            return serde_json::to_string(&out).map_err(|e| e.to_string());
                        }
                        other => {
                            out.failed = Some(other.say(&a.site));
                            b.close();
                            return serde_json::to_string(&out).map_err(|e| e.to_string());
                        }
                    }
                }
            }
            b.close();
            match p {
                Ok(pressed) => out.pressed = Some(pressed),
                Err(e) => out.failed = Some(e),
            }
            serde_json::to_string(&out).map_err(|e| e.to_string())
        });
        if self.hand_off("security-change", t, work, Some(asked.site.clone()), SpeakPolicy::Always) {
            format!("Going to {} to {}.", asked.site, asked.change.plainly())
        } else {
            "I'm too busy to do that right now — ask me again in a moment.".into()
        }
    }

    /// Make an account (B6), on a crew errand: the password made and put in
    /// the vault first, then the form, page by page.
    pub(super) fn start_sign_up(&mut self, domain: &str, t: u64) -> String {
        let cfg = self.tools_cfg().enrol.clone();
        if self.vault.state() != crate::vault::State::Open {
            return "The vault's locked, and the new password has to go straight into it — say the passphrase first.".into();
        }
        let email = self
            .tools_cfg()
            .mail
            .accounts
            .first()
            .map(|a| a.address.clone())
            .unwrap_or_default();
        if email.is_empty() {
            return "I need an email address to sign you up with, and I don't have one — set up your mail first.".into();
        }
        let username = email.split('@').next().unwrap_or("").to_string();
        let guard = match self.store.transaction() { Ok(guard) => guard, Err(e) => return format!("Signup didn't start: state is busy ({e}); no external form opened.") };
        let mut attempts: crate::enrol::SignupAttempts = match self.store.load_checked(crate::enrol::SIGNUP_ATTEMPTS) { Ok(attempts) => attempts.unwrap_or_default(), Err(e) => return format!("Signup didn't start: earlier attempt coverage is unavailable ({e}); check the service before repeating.") };
        if !attempts.may_start(domain, &username) { return format!("An earlier signup for {username} on {domain} is still recorded. Check that service before another attempt; no new form opened."); }
        let entropy = crate::vault::random_bytes(64);
        let password = match cfg.password.make(&entropy) {
            Ok(p) => p,
            Err(e) => return format!("I couldn't make a password that {domain} would take: {e}."),
        };
        let mut e = crate::enrol::Enrolment::new(domain, &username, t);
        let (name, kind, value) = e.vault_write(&password);
        let previous_vault = self.vault.clone();
        if let Err(err) = self.vault.put(&name, kind, &value, t) {
            return format!("I couldn't keep the new password in the vault ({err}), so I didn't start.");
        }
        if let Err(err) = self.vault.save(&self.vault_home) { self.vault = previous_vault; return format!("Signup didn't start: its generated password couldn't be saved ({err}); no external form opened."); }
        // Grant sign-in on the new account, so the next "sign me in" works.
        let previous_access = self.access.clone();
        self.access.grant(domain, &username, domain, crate::signin::Allowed::SignIn, &name, t);
        if let Err(err) = self.access.save(&self.store) { self.access = previous_access; return format!("The password is kept, but signup didn't start because its access permission couldn't be saved ({err}). No external form opened."); }
        if !attempts.begin(&e) { return "Signup didn't start: its attempt record could not be reserved. No external form opened.".into(); }
        if let Err(err) = self.store.save(crate::enrol::SIGNUP_ATTEMPTS, &attempts) { return format!("Signup didn't start: its attempt fence couldn't be saved ({err}); no external form opened."); }
        drop(guard);
        let queued_enrolment = e.clone();
        let bcfg = self.tools_cfg().browser.clone();
        let vars = self.tools_cfg().vars.clone();
        let work: crew::Work = Box::new(move |ctl| {
            let outcome = match crate::browser::Browser::start(&bcfg, &vars) {
                Ok(mut b) => {
                    let o = crate::webrun::sign_up_unless(&mut b, &mut e, &email, &password, &cfg, None, &|| ctl.stopping());
                    b.close();
                    o
                }
                Err(err) => crate::webrun::SignedUp::Failed(err.to_string()),
            };
            serde_json::to_string(&SignUpOutcome { enrolment: e, outcome }).map_err(|e| e.to_string())
        });
        if self.hand_off("sign-up", t, work, Some(domain.to_string()), SpeakPolicy::Always) {
            format!("Making you an account on {domain}. I'll stop at anything that wants payment, ID or a robot check.")
        } else {
            let saved = (|| -> crate::error::Result<()> {
                let _guard = self.store.transaction()?;
                let mut current: crate::enrol::SignupAttempts = self.store.load_checked(crate::enrol::SIGNUP_ATTEMPTS)?.unwrap_or_default();
                if !current.settle(&queued_enrolment, crate::enrol::SignupState::NotSubmitted, "The worker queue was full; no browser or signup form started.") { return Err(crate::error::AtlasError::Platform("signup reservation changed".into())); }
                self.store.save(crate::enrol::SIGNUP_ATTEMPTS, &current)
            })();
            match saved {
                Ok(()) => "I'm too busy to start that right now. No signup form opened; ask me again in a moment.".into(),
                Err(err) => format!("No signup form opened, but I couldn't save that stopped attempt ({err}). The attempt remains held until you check it; I won't repeat it automatically."),
            }
        }
    }

    pub(crate) fn signup_inflight(&self, domain: &str) -> bool { self.crew_links.values().any(|link| link.label == "sign-up" && link.topic.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(domain))) }
}

impl<'a> Daemon<'a> {
    /// E1: fix Atlas's own things without asking. Only what `diagnose` marks
    /// as Atlas's to fix (things Atlas made and can remake), recorded in the
    /// activity log, never said out loud: a recreated scratch folder isn't
    /// worth interrupting you for.
    pub fn fix_my_own_things(&mut self, t: u64) -> Vec<String> {
        let symptoms = crate::diagnose::diagnose(&self.vitals());
        let mut done = Vec::new();
        for s in crate::diagnose::self_fixable(&symptoms) {
            let fixed = match s.id.as_str() {
                "scratch" => std::fs::create_dir_all(&self.tools_cfg().work_dir).is_ok(),
                // Already set aside and started fresh when they were read; the
                // fix is to carry on, which is what's happening.
                "preserved" => true,
                _ => false,
            };
            if fixed {
                self.journal.record_at(Act::Upkeep, &format!("fixed on my own: {}", s.what), true, t);
                done.push(s.id.clone());
            }
        }
        done
    }

    /// E3: carry on with the last build that ran out of tries, as a long job.
    pub(super) fn keep_at_it(&mut self, t: u64) -> String {
        let path = crate::build_it::Struggle::path();
        let Some(s) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|j| serde_json::from_str::<crate::build_it::Struggle>(&j).ok())
        else {
            return "There's no build I gave up on to keep at.".into();
        };
        let Some(llm) = self.background_llm() else {
            return "I'd need a model to keep drafting, and none is configured.".into();
        };
        let limits = self.tools_cfg().long_jobs.clone();
        let (attempts, hours) = (limits.max_attempts, limits.max_hours);
        let base = crate::roots::tmp_dir().join("builds");
        let out_dir = crate::roots::data_sub("builds");
        let lang = s.lang;
        let what = s.description.clone();
        let hcfg = self.tools_cfg().handoff.clone();
        let named = what.clone();
        let work: crew::Work = Box::new(move |ctl| {
            let mut sandbox = crate::sandbox::Sandbox::create(&base, "build")
                .map_err(|e| format!("couldn't make a sandbox to build in: {e}"))?;
            let (mut outcome, mut said) = crate::build_it::keep_building(
                &s,
                llm.as_ref(),
                &limits,
                |code: &str| check_draft_in_sandbox(&mut sandbox, lang, code),
                || ctl.checkpoint(),
            );
            // Still stuck: write it up properly and ask the bigger model once
            // (D, minimally). Without one, the write-up is kept for when
            // there is.
            if let crate::build_it::Outcome::Struggled { code, rounds, last_failure } = &outcome {
                let latest = crate::build_it::Struggle { code: code.clone(), failure: last_failure.clone(), ..s.clone() };
                if llm.has_stronger() {
                    let (o, why) = crate::build_it::ask_for_help(&latest, *rounds, llm.as_ref(), &hcfg, |code: &str| {
                        check_draft_in_sandbox(&mut sandbox, lang, code)
                    });
                    outcome = o;
                    said = format!("{said} {why}");
                } else {
                    crate::heard!(std::fs::create_dir_all(&out_dir));
                    let brief = out_dir.join("ask-for-help.md");
                    crate::kept!(std::fs::write(&brief, crate::build_it::write_up(&latest, *rounds, &hcfg)));
                    said = format!(
                        "{said} I've written the problem up for a bigger model in {} — once your server's model is set as the stronger one, I'll ask it myself.",
                        brief.display()
                    );
                }
            }
            crate::heard!(sandbox.discard());
            if let Some(code) = outcome.code() {
                crate::heard!(std::fs::create_dir_all(&out_dir));
                let ext = if outcome.is_built() { ext_for(lang).to_string() } else { format!("draft.{}", ext_for(lang)) };
                let path = crate::build_it::file_name_for(&out_dir, &named, &ext);
                said = match std::fs::write(&path, code) {
                    Ok(()) => format!("{said} Saved as {}, in {}.", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), out_dir.display()),
                    Err(e) => format!("{said} (I couldn't save it to {} -- {e}.)", path.display()),
                };
            }
            if outcome.is_built() {
                crate::heard!(std::fs::remove_file(crate::build_it::Struggle::path()));
            }
            Ok(said)
        });
        if self.hand_off("build", t, work, Some(what.clone()), SpeakPolicy::Always) {
            format!(
                "I'll keep at \"{what}\" on my own — up to {attempts} tries or {hours} hours, and I'll stop \
                 sooner if I'm going in circles. Say stop and I'll leave it with the best draft."
            )
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    /// E4: note what you just did, for spotting routines.
    pub(super) fn note_for_routines(&mut self, said: &str, intent: &Intent, t: u64) {
        if !self.tools_cfg().routine.enabled {
            return;
        }
        // Answers, chat and anything to do with secrets aren't routines.
        if matches!(
            intent,
            Intent::Say(_) | Intent::Unknown(_) | Intent::Unlock(_) | Intent::TypeCode(_) | Intent::TwoFactor(_)
        ) {
            return;
        }
        let off = crate::localclock::offset_secs();
        let words = said.trim().to_lowercase();
        if words.is_empty() {
            return;
        }
        self.routines.did(&words, t, crate::localclock::hour(t, off) as u32, crate::localclock::weekday(t, off));
    }

    /// E4, on the hour: ask about a routine once it's been seen three times,
    /// and run the ones that are due — the concrete ones on their own, the
    /// abstract ones after asking ("Your usual morning setup?").
    pub(super) fn routines_on_the_hour(&mut self, t: u64) -> Vec<String> {
        let cfg = self.tools_cfg().routine.clone();
        if !cfg.enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.pending_routine.is_none() && self.pending_routine_run.is_none() {
            if let Some(r) = self.routines.take_new(&cfg) {
                let q = crate::routine::ask_about(&r, &cfg);
                self.session.ask(&q);
                self.pending_routine = Some(r.name.clone());
                out.push(q);
            }
        }
        let off = crate::localclock::offset_secs();
        let (hour, weekday) = (crate::localclock::hour(t, off) as u32, crate::localclock::weekday(t, off));
        let day = crate::localclock::day(t, off).max(0) as u64;
        let due: Vec<_> = self.routines.routines.iter().filter(|routine| routine.due(hour, weekday) && routine.last_day != day).cloned().collect();
        for r in due {
            if r.automatic {
                let key = format!("routine:{}:{day}", r.name);
                match self.queue_routine_once(&key, &r.steps, t) {
                    Ok(added) => {
                        if let Some(routine) = self.routines.routines.iter_mut().find(|routine| routine.name == r.name) { routine.last_day = day; }
                        if let Err(error) = self.store.save("routines", &self.routines) { self.log.warn(&format!("routine reservation remains pending ({error}); its saved queue origin prevents duplicate work")); }
                        if added { out.push(format!("Queued your usual {} ({} steps).", r.name, r.steps.len())); }
                    }
                    Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {},
                    Err(error) => out.push(format!("I couldn't save your {} routine, so no steps started ({error}).", r.name)),
                }
            } else if self.pending_routine_run.is_none() && matches!(self.session.pending, Pending::Nothing) {
                let question = crate::routine::starting(&r);
                let mut pending: Vec<LeftWaiting> = match self.store.load_checked(LEFT_WAITING) { Ok(value) => value.unwrap_or_default(), Err(error) => { self.log.warn(&format!("couldn't preserve routine question ({error})")); continue; } };
                if !pending.iter().any(|entry| entry.what == question) { pending.push(LeftWaiting { what: question.clone(), asked: true, at: t }); }
                if self.store.save(LEFT_WAITING, &pending).is_err() { continue; }
                self.session.ask(&question); self.pending_routine_run = Some(r.steps.clone()); out.push(question);
            }
        }
        let _ = self.store.save("routines", &self.routines);
        out
    }

    /// Your answer about a routine, if one was asked.
    pub(super) fn answer_about_routine(&mut self, said: &str, t: u64) -> Option<String> {
        if let Some(name) = self.pending_routine.take() {
            let before = self.routines.clone();
            let pending = self.session.pending.clone();
            self.session.pending = Pending::Nothing;
            let reply = if is_yes(said) {
                let steps = self.routines.routines.iter().find(|r| r.name == name).map(|r| r.steps.clone()).unwrap_or_default();
                let cfg = self.tools_cfg().routine.clone();
                let automatic = crate::routine::is_concrete(&steps)
                    && !(cfg.never_automate_outgoing && crate::routine::sends_something(&steps));
                self.routines.confirm(&name, automatic);
                if automatic {
                    format!("Done — I'll do your {name} myself when it's time, and say so as I start.")
                } else {
                    format!("Done — when it's time I'll ask: \"Your usual {name}?\"")
                }
            } else {
                self.routines.declined(&name);
                "Alright, I won't.".into()
            };
            if let Err(error) = self.store.save("routines", &self.routines) {
                self.routines = before; self.pending_routine = Some(name); self.session.pending = pending;
                return Some(format!("I couldn't save that routine decision ({error}); it is still waiting on you."));
            }
            return Some(reply);
        }
        if let Some(steps) = self.pending_routine_run.take() {
            self.session.pending = Pending::Nothing;
            if !is_yes(said) {
                let before = self.routines.clone();
                let day = crate::localclock::day(t, crate::localclock::offset_secs()).max(0) as u64;
                if let Some(routine) = self.routines.routines.iter_mut().find(|routine| routine.steps == steps) { routine.last_day = day; }
                if let Err(error) = self.store.save("routines", &self.routines) {
                    self.routines = before; self.pending_routine_run = Some(steps); self.session.ask("Your routine decision is still waiting to be saved.");
                    return Some(format!("I couldn't save ‘not today’ ({error}); no routine steps started."));
                }
                return Some("Alright, not today.".into());
            }
            let day = crate::localclock::day(t, crate::localclock::offset_secs()).max(0) as u64;
            let name = self.routines.routines.iter().find(|routine| routine.steps == steps).map(|routine| routine.name.clone()).unwrap_or_else(|| steps.join(" | "));
            let key = format!("routine:{name}:{day}");
            return Some(match self.queue_routine_once(&key, &steps, t) {
                Ok(added) => {
                    if let Some(routine) = self.routines.routines.iter_mut().find(|routine| routine.name == name) { routine.last_day = day; }
                    let _ = self.store.save("routines", &self.routines);
                    if added { format!("Queued your usual {name} ({} steps).", steps.len()) } else { "That routine is already queued or has an outcome recorded for today.".into() }
                }
                Err(error) => { self.pending_routine_run = Some(steps); self.session.ask("Your routine is still waiting. Shall I try to save its queue again?"); format!("No routine steps started: its queue couldn't be saved ({error}).") }
            });
        }
        None
    }
}

impl<'a> Daemon<'a> {
    pub(super) fn goals(&mut self, said: &str, t: u64) -> String {
        let lower = said.to_lowercase();
        let words = crate::nudge::goal_words(said);
        let reply = if lower.contains("what are my goals") || lower.trim() == "my goals" {
            let live: Vec<&crate::nudge::Goal> = self.nudger.goals.iter().filter(|g| !g.muted).collect();
            if live.is_empty() {
                "You haven't set any goals. Say \"my goal is …\" and I'll nudge you toward it when it goes quiet.".into()
            } else {
                let each: Vec<String> = live.iter().map(|g| g.what.clone()).collect();
                format!("Your goals: {}.", each.join("; "))
            }
        } else if ["drop the goal", "forget the goal", "done with the goal"].iter().any(|p| lower.contains(p)) {
            match crate::nudge::which_goal(&self.nudger.goals, &words).map(|g| g.id.clone()) {
                Some(id) => {
                    let what = self.nudger.goals.iter().find(|g| g.id == id).map(|g| g.what.clone()).unwrap_or_default();
                    self.nudger.goals.retain(|g| g.id != id);
                    format!("Dropped \"{what}\". I won't bring it up again.")
                }
                None => "I couldn't tell which goal you meant.".into(),
            }
        } else if ["worked on", "progress on"].iter().any(|p| lower.contains(p)) {
            match crate::nudge::which_goal(&self.nudger.goals, &words).map(|g| g.id.clone()) {
                Some(id) => {
                    self.nudger.moved(&id, t);
                    "Noted — that counts as movement, so I'll leave it be for a while.".into()
                }
                None => "I haven't got a goal like that. Say \"my goal is …\" to set one.".into(),
            }
        } else {
            if words.trim().is_empty() {
                return "What's the goal?".into();
            }
            let id: String = words
                .to_lowercase()
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect::<String>()
                .split('-')
                .filter(|s| !s.is_empty())
                .take(6)
                .collect::<Vec<_>>()
                .join("-");
            self.nudger.track(crate::nudge::Goal::new(&id, &words, t));
            format!(
                "Got it: \"{words}\". If it goes quiet for a few days I'll nudge you — less often if the \
                 nudges don't help, and never about anything medical."
            )
        };
        match self.store.save(crate::nudge::GOALS, &self.nudger.goals) {
            Ok(()) => reply,
            Err(error) => format!("{reply} That goal change is still waiting to be saved ({error}); I'll retry while Atlas stays open."),
        }
    }
}

impl<'a> Daemon<'a> {
    /// F6: say up front how long it'll take, for the long kinds only.
    pub(super) fn how_long_up_front(&mut self, intent: &Intent, t: u64) -> Option<String> {
        let (label, open_ended) = match intent {
            Intent::Research(_) => ("research", false),
            Intent::AskTheRoom(_) => ("council", false),
            Intent::Build(_) => ("build", false),
            Intent::Improve(_) => ("improve", false),
            // "Keep at it" already says its limits; saying a time too would
            // be the annoying kind.
            _ => return None,
        };
        let past: Vec<u64> = self
            .long_work
            .jobs
            .iter()
            .filter(|j| j.name == label && j.finished.is_some())
            .map(|j| j.ran_for(t))
            .collect();
        let key = format!("estimate_said_{label}");
        let last: Option<u64> = self.store.load(&key);
        let line = crate::timebox::estimate_worth_saying(crate::timebox::usual_secs(&past), open_ended, last, t)?;
        let _ = self.store.save(&key, &Some(t));
        Some(line)
    }
}

impl<'a> Daemon<'a> {
    /// "Add call the bank to my later list": the list with your words in
    /// it, whatever the sentence starts with (the phrases only catch
    /// sentences that start with them).
    /// One turn of "get to know me" (`getknow`), when it's starting or
    /// under way. `None`: not part of it.
    pub(super) fn interview_turn(&mut self, said: &str, t: u64) -> Option<String> {
        if self.interview.is_none() {
            if !crate::getknow::asked_to_start(said) {
                return None;
            }
            let (iv, say) = crate::getknow::Interview::begin();
            self.interview = Some(iv);
            return Some(say);
        }
        // "Call me Eric" is the answer to the first question, not a command.
        let (parsed, name) = self.parser.parse_named(said);
        let commanded = !matches!(parsed, Intent::Unknown(_)) && name.as_deref() != Some("address_as");
        if said.trim_end().ends_with('?') || commanded {
            self.interview = None;
            return None;
        }
        let mut iv = self.interview.take()?;
        let (keep, say, more) = match iv.answer(said, t) {
            crate::getknow::Next::Ask { keep, say } => (keep, say, true),
            crate::getknow::Next::Done { keep, say } => (keep, say, false),
        };
        for f in keep {
            // The hunt's two lists are lists, kept whole under their own
            // names -- `learn` would merge the second into the first, their
            // words being the same.
            if f.name == crate::facts::slug(crate::hunt::FACT_WANT) || f.name == crate::facts::slug(crate::hunt::FACT_SKILLS) {
                self.facts.put(f);
            } else {
                self.facts.learn(f, t);
            }
        }
        let _ = self.facts.save(&self.store);
        if more {
            self.interview = Some(iv);
        }
        Some(say)
    }

    pub(super) fn later_words_help(&mut self, raw: &str, t: u64) -> Option<String> {
        let low = raw.to_lowercase();
        if !low.contains("later list") && !low.ends_with(" for later") {
            return None;
        }
        later_own_words(raw)?;
        Some(self.later_list(raw, t))
    }

    pub(super) fn later_list(&mut self, said: &str, t: u64) -> String {
        let lower = said.to_lowercase();
        let mut later: crate::later::Later = self.store.load(crate::later::RECORD);
        let reply = if lower.contains("what") || lower.trim() == "later list" {
            later.read_back()
        } else if lower.contains("clear") {
            let n = later.items.len();
            later.items.clear();
            format!("Cleared {n} from your later list.")
        } else if lower.contains("take") || lower.contains("remove") {
            match later.take_off(&lower.replace("later list", "")) {
                Some(i) => format!("Took \"{}\" off your later list.", i.what),
                None => "I couldn't tell which one you meant.".into(),
            }
        } else if let Some(own) = later_own_words(said) {
            // Your own words, when you gave some: "add call the bank to my
            // later list" (1 Oct 2026: every add took Atlas's last reply, and
            // "List, did you hear me?" put Atlas's own sentence on your list).
            if later.add(&own, t) {
                format!("On your later list: \"{own}\".")
            } else {
                "That's already on your later list.".into()
            }
        } else if !["that", "this", "it"].iter().any(|w| lower.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w)) {
            "Say what to put on it -- \"add call the bank to my later list\" -- or \"save that for later\" right after I've said something.".into()
        } else {
            // "That" is what Atlas last said.
            let last = self.session.turns.last().map(|t| t.reply.clone()).unwrap_or_default();
            let what = crate::later::gist(&last);
            if what.is_empty() {
                "There's nothing I just said to put on it.".into()
            } else if later.add(&what, t) {
                format!("On your later list: \"{what}\". I'll mention the list once a week so it isn't forgotten.")
            } else {
                "That's already on your later list.".into()
            }
        };
        let _ = self.store.save(crate::later::RECORD, &later);
        reply
    }

    /// A change Atlas made to itself, staged and waiting on your OK: said
    /// once, reminded once a day later, then left alone (`selfgrant`'s
    /// low-pressure wording). F8: "yes".
    pub(super) fn remind_about_staged_change(&mut self, t: u64) -> Option<String> {
        if self.pending_landing.is_empty() {
            let _ = self.store.save("landing_asked", &(0u32, 0u64));
            return None;
        }
        let what = self.selfwork.as_ref().map(|s| s.goal.clone()).unwrap_or_else(|| "a fix to myself".into());
        let paths: Vec<String> = self.pending_landing.iter().map(|c| c.target.display().to_string()).collect();
        let reach = crate::selfgrant::reach_of_change(&paths);
        let (times, last): (u32, u64) = self.store.load("landing_asked");
        let line = if times == 0 {
            Some(crate::selfgrant::raise_it(&what, reach))
        } else if t.saturating_sub(last) >= 86_400 {
            crate::selfgrant::raised_again(&what, times)
        } else {
            None
        }?;
        let _ = self.store.save("landing_asked", &(times + 1, t));
        Some(format!("{line} Say \"go ahead\" to land it, or \"add that to the later list\"."))
    }
}

impl<'a> Daemon<'a> {
    /// F9: an old correction that bears on the work just asked for, said once.
    pub(super) fn related_old_correction(&mut self, intent: &Intent, said: &str, t: u64) -> Option<String> {
        if !matches!(
            intent,
            Intent::Research(_) | Intent::DraftPost(_) | Intent::Build(_) | Intent::Improve(_)
                | Intent::AskTheRoom(_) | Intent::Message(_) | Intent::Dictate(_) | Intent::Explain(_)
        ) {
            return None;
        }
        let mut mentioned: Vec<u64> = self.store.load("stale_notes_mentioned");
        let hit = crate::revise::related_stale(&self.mending, said, t, &mentioned).first().map(|c| (c.at, c.wanted.clone().unwrap_or_default()))?;
        mentioned.push(hit.0);
        let _ = self.store.save("stale_notes_mentioned", &mentioned);
        Some(format!("Something like this came up before, and you wanted {} — I've kept that in mind.", hit.1.trim().trim_end_matches('.')))
    }
}

impl<'a> Daemon<'a> {
    /// F10: the plain "what I can't do on this machine", for start-up.
    /// Nothing when there's nothing missing.
    pub fn cant_do_here(&self) -> Option<String> {
        let root = crate::roots::install_root();
        let pictures = self.tools_ref().map(|t| crate::picture_talk::ready(&t.picture_talk, &root));
        let limits = what_this_machine_cant_do(crate::fit::limits(&self.fit), pictures, self.llm.is_some());
        (!limits.is_empty()).then(|| format!("Before we start — on this machine: {}", limits.join(" ")))
    }
}

impl<'a> Daemon<'a> {
    pub(super) fn sort_mail(&mut self, said: &str, t: u64) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if !cfg.enabled || cfg.accounts.is_empty() {
            return "I can't sort your mail yet — no mailbox is set up.".into();
        }
        // "Delete the noise": only ever from these words, and only for what
        // the last sort put in that category. Deleting is moving to Trash,
        // where your mail app keeps it for its usual time.
        if let Some(cat) = crate::mail::category_to_delete(said) {
            if self.mail_plans.iter().all(|p| !p.moves.iter().any(|(_, c)| c == cat)) {
                return format!("I haven't sorted anything into {cat} this time — ask me to sort your mailbox first.");
            }
            return self.apply_mail_plan(Some(cat), t);
        }
        let (jobs, problems) = self.resolve_mail_jobs(&cfg, t);
        if jobs.is_empty() {
            return format!("I couldn't get into your mail: {}.", problems.join("; "));
        }
        let since = crate::triage::imap_date(30, t);
        let leave_alone = cfg.leave_alone.clone();
        let work: crew::Work = Box::new(move |_ctl| {
            let mut plans = Vec::new();
            for (account, password) in &jobs {
                let Some(host) = mail_host(account) else { continue };
                let mut s = crate::imap::connect(&host, 993)?;
                authenticate_imap(&mut s, &account.address, password, account.oauth.then_some(account.client_id.as_str()))?;
                let msgs = s.fetch_matching("INBOX", &format!("SINCE {since}"))?;
                s.logout();
                let gmail = crate::mail::Provider::from_address(&account.address).has_labels();
                let moves = msgs
                    .iter()
                    .map(|m| (m.uid, crate::mail::category_of(m).to_string()))
                    // Never into a folder you've said to leave alone.
                    .filter(|(_, c)| !leave_alone.iter().any(|l| l.eq_ignore_ascii_case(c)))
                    .collect();
                plans.push(crate::mail::SortPlan { account: account.name.clone(), gmail, moves });
            }
            serde_json::to_string(&plans).map_err(|e| e.to_string())
        });
        if self.hand_off("mail-sort", t, work, None, SpeakPolicy::Always) {
            "Looking through the last month of your inbox — I'll tell you what I'd do before I move anything.".into()
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    /// Carry out the plan: everything (after "go"), or only the category you
    /// told it to delete.
    pub(super) fn apply_mail_plan(&mut self, delete: Option<&str>, t: u64) -> String {
        let cfg = self.tools_cfg().mail.clone();
        let plans = self.mail_plans.clone();
        if plans.is_empty() {
            return "There's nothing waiting to be sorted.".into();
        }
        let (jobs, problems) = self.resolve_mail_jobs(&cfg, t);
        if jobs.is_empty() {
            return format!("I couldn't get into your mail: {}.", problems.join("; "));
        }
        let mut sort_cfg = cfg.clone();
        sort_cfg.actually_sort = true;
        let delete = delete.map(str::to_string);
        let deleting = delete.clone();
        let work: crew::Work = Box::new(move |ctl| {
            let mut done = 0usize;
            let mut failed = Vec::new();
            for plan in &plans {
                let Some((account, password)) = jobs.iter().find(|(a, _)| a.name == plan.account) else { continue };
                let Some(host) = mail_host(account) else { continue };
                let mut s = crate::imap::connect(&host, 993)?;
                authenticate_imap(&mut s, &account.address, password, account.oauth.then_some(account.client_id.as_str()))?;
                s.select("INBOX")?;
                let provider = crate::mail::Provider::from_address(&account.address);
                let trash = if plan.gmail { "[Gmail]/Trash" } else { "Trash" };
                for (uid, cat) in &plan.moves {
                    if ctl.checkpoint() {
                        break;
                    }
                    let action = match &delete {
                        Some(d) if d == cat => crate::mail::Action::MoveTo(trash.to_string()),
                        Some(_) => continue,
                        None => crate::mail::action_for(cat, provider, &sort_cfg),
                    };
                    match s.apply(*uid, &action, plan.gmail) {
                        Ok(()) => done += 1,
                        Err(e) => failed.push(e),
                    }
                }
                s.logout();
            }
            let mut said = match &delete {
                Some(d) => format!("Moved {done} from {d} to the trash — your mail app keeps them there for its usual time."),
                None => format!("Sorted {done} messages. Nothing was deleted, and all of it can be undone in your mail app."),
            };
            if !failed.is_empty() {
                said.push_str(&format!(" {} didn't go: {}.", failed.len(), failed[0]));
            }
            Ok(said)
        });
        if deleting.is_none() {
            self.mail_plans.clear();
        } else {
            let d = deleting.clone().unwrap_or_default();
            for p in self.mail_plans.iter_mut() {
                p.moves.retain(|(_, c)| *c != d);
            }
        }
        if self.hand_off("mail-sort-apply", t, work, None, SpeakPolicy::Always) {
            "On it.".into()
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    pub(super) fn mail_sort_news(&mut self, label: &str, ending: &crew::Ending) -> Option<String> {
        match ending {
            crew::Ending::Done(Ok(json)) if label == "mail-sort" => {
                let plans: Vec<crate::mail::SortPlan> = serde_json::from_str(json).ok()?;
                let mut counts: Vec<(String, usize)> = Vec::new();
                for p in &plans {
                    for (c, n) in p.counts() {
                        match counts.iter_mut().find(|(x, _)| *x == c) {
                            Some((_, k)) => *k += n,
                            None => counts.push((c, n)),
                        }
                    }
                }
                let said = crate::mail::rehearsal(&counts, &self.tools_cfg().mail);
                self.mail_plans = plans;
                if !self.mail_plans.iter().all(|p| p.moves.is_empty()) {
                    self.pending_mail_sort = true;
                    self.session.ask(&said);
                }
                Some(said)
            }
            crew::Ending::Done(Ok(said)) => Some(said.clone()),
            crew::Ending::Done(Err(e)) => Some(format!("I couldn't sort your mail: {e}.")),
            _ => None,
        }
    }
}

impl<'a> Daemon<'a> {
    /// Schedule a post for a time you said, and approve it. "now" is now.
    pub(crate) fn approve_post_in_background(&mut self, id: u64, at: Option<u64>, review: Option<&str>, t: u64) -> String {
        let Some(expected) = self.publisher.get(id).cloned() else { return "That post no longer exists.".into(); };
        if let Some(review) = review {
            if expected.state != crate::publish::PostState::Draft || expected.result.as_deref() != Some(crate::publish::CHECKED_ABSENT) || crate::publish::review_fingerprint(&expected) != review {
                return "That reviewed draft changed, or its publication remains unconfirmed. Reload and review again.".into();
            }
        }
        if matches!(expected.state, crate::publish::PostState::Sent | crate::publish::PostState::Cancelled | crate::publish::PostState::PendingSubmission | crate::publish::PostState::Uncertain) { return "That post cannot be approved now.".into(); }
        let mut candidate = self.publisher.clone();
        let fingerprint = crate::publish::review_fingerprint(&expected);
        let topic = format!("{id}:{}:{fingerprint}", at.map(|v| v.to_string()).unwrap_or_else(|| "now".into()));
        let work: crew::Work = Box::new(move |ctl| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
            if let Some(at) = at { if !candidate.schedule(id, at) { return Err("That post cannot be scheduled.".into()); } }
            if crate::publish::review_fingerprint(&expected) != fingerprint { return Err("An attachment changed after review.".into()); }
            candidate.approve_unless(id, &|| ctl.stopping() || std::time::Instant::now() >= deadline)?;
            if crate::publish::review_fingerprint(&expected) != fingerprint { return Err("An attachment changed during approval.".into()); }
            let approved = candidate.get(id).cloned().ok_or("post vanished")?;
            serde_json::to_string(&crate::publish::PostApprovalReceipt { tag: "atlas.post_approval".into(), version: 1, expected, approved, requested_at: t, requested_send_at: at }).map_err(|e| e.to_string())
        });
        if self.hand_off_as("post approval", t, work, Some(topic), SpeakPolicy::Always).is_some() {
            "Checking the exact approved attachments in the background. Publication is not confirmed; the result will say whether approval was saved.".into()
        } else { "Approval work could not start; the post remains unchanged.".into() }
    }

    pub(super) fn post_approval_news(&mut self, ending: &crew::Ending, t: u64) -> crate::taskloop::Outcome {
        use crate::taskloop::Outcome;
        let receipt = match ending {
            crew::Ending::Done(Ok(text)) if text.len() <= 131072 => serde_json::from_str::<crate::publish::PostApprovalReceipt>(text),
            _ => return Outcome::Failed("Approval did not finish; nothing was approved by this worker.".into()),
        };
        let Ok(receipt) = receipt else { return Outcome::Failed("Approval returned an invalid receipt; nothing changed.".into()); };
        let before = self.publisher.clone();
        if !self.publisher.apply_approval_receipt(&receipt) { return Outcome::Failed("The post changed during approval; review it again.".into()); }
        if let Err(e) = self.publisher.save(&self.store) { self.publisher = before; return Outcome::Failed(format!("Approval could not be saved: {e}")); }
        if receipt.approved.send_at.is_some_and(|at| at > t) {
            Outcome::Done(format!("Approved and queued for {}. Publication will be checked separately.", crate::localclock::hhmm_here(receipt.approved.send_at.unwrap())))
        } else { Outcome::NeedsYou("Approval saved for the exact post and attachments. Submission is queued; publication is not confirmed.".into()) }
    }

    pub(super) fn schedule_post_at(&mut self, id: u64, said: &str, t: u64) -> String {
        // Read on your clock, kept in UTC (28 Sep 2026): "tomorrow at 9" was
        // read as 9 in UTC, hours out, the way offered meeting times once
        // were (`booking_from`).
        let now_said = crate::intent::normalize(said).split_whitespace().any(|w| w == "now");
        let when = if now_said {
            Some(t)
        } else {
            let zone = self.home_zone();
            let lnow = zone.to_local(t as i64).max(0) as u64;
            crate::calendar::resolve_when(said, lnow).map(|w| zone.to_utc(w.start as i64).max(0) as u64)
        };
        let Some(at) = when else {
            self.pending_post_when = Some(id);
            let q = "I didn't catch the time — \"now\", \"at 6pm\" or \"tomorrow at 9\"?".to_string();
            self.session.ask(&q);
            return q;
        };
        if self.publisher.get(id).is_some_and(|p| !p.media.is_empty()) { return self.approve_post_in_background(id, Some(at), None, t); }
        let before = self.publisher.clone();
        if !self.publisher.schedule(id, at) || !self.publisher.approve(id) {
            return "That post can't be scheduled any more.".into();
        }
        if let Err(e) = self.publisher.save(&self.store) { self.publisher = before; return format!("The approval and time couldn't be saved ({e}); the previous draft or schedule is unchanged. Nothing submitted by this request."); }
        let what = self.publisher.get(id).map(|p| p.describe()).unwrap_or_default();
        if at <= t {
            format!("Posting {what} now.")
        } else {
            format!(
                "Scheduled {what} for {} — I'll check it again just before it goes, and \"cancel the post\" stops it any time until then.",
                crate::localclock::hhmm_here(at)
            )
        }
    }

    /// "Post it at 6pm" / "schedule the post for tomorrow at 9" (G2), for
    /// the newest post that's approved or waiting for approval.
    pub(super) fn schedule_post(&mut self, said: &str, t: u64) -> String {
        if said.to_lowercase().contains("cancel") {
            let id = self.publisher.pending().iter().rev().map(|p| p.id).next();
            return match id {
                Some(id) if self.publisher.cancel(id) => {
                    if let Err(e) = self.publisher.save(&self.store) { return format!("The post is paused here, but cancellation couldn't be saved ({e}). Check it before restarting Atlas; the earlier saved schedule may remain."); }
                    "Cancelled — it won't go.".into()
                }
                _ => "There's no post waiting to cancel.".into(),
            };
        }
        let id = self
            .publisher
            .posts
            .iter()
            .rev()
            .find(|p| {
                matches!(
                    p.state,
                    crate::publish::PostState::AwaitingApproval
                        | crate::publish::PostState::Scheduled
                        | crate::publish::PostState::ReadyToSend
                        | crate::publish::PostState::Held
                )
            })
            .map(|p| p.id);
        match id {
            Some(id) => self.schedule_post_at(id, said, t),
            None => "There's no post ready to schedule — draft one first.".into(),
        }
    }

    /// A due post, sent through Atlas's browser on a crew errand.
    pub(crate) fn publication_inflight(&self, id: u64) -> bool {
        self.publication_jobs
            .iter()
            .any(|(job, (post, _))| *post == id && self.crew.in_hand(*job))
    }

    pub(crate) fn check_publication_timeouts(&mut self, now: u64) -> Vec<String> {
        let mut out = Vec::new();
        let jobs: Vec<_> = self
            .publication_jobs
            .iter()
            .map(|(job, info)| (*job, *info))
            .collect();
        for (job, (post, started)) in jobs {
            if now.saturating_sub(started) < 120 {
                continue;
            }
            let first = self
                .publisher
                .get(post)
                .is_some_and(|p| p.state == crate::publish::PostState::PendingSubmission);
            if first {
                self.crew.ask_to_stop(job);
                let why = "Publication took too long to confirm. Check the service; I won't repeat it automatically.";
                self.publisher.mark_submission(post, true, why);
                out.push(why.into());
            }
            if !self.crew.in_hand(job) {
                self.publication_jobs.remove(&job);
                self.posting.retain(|(id, _)| *id != post);
            }
        }
        if !out.is_empty() {
            if let Err(e) = self.publisher.save(&self.store) {
                out.push(format!(
                    "The uncertain publication record couldn't be saved: {e}"
                ));
            }
        }
        out
    }

    pub(super) fn send_post(&mut self, id: u64, t: u64, online: bool) -> Option<String> {
        if self.posting.iter().any(|(p, not_before)| *p == id && t < *not_before) {
            return None;
        }
        self.posting.retain(|(p, _)| *p != id);
        let bcfg = self.browser_cfg();
        let vars = self.tools_cfg().vars.clone();
        let mut publisher = self.publisher.clone();
        let prior_state = self.publisher.get(id)?.state;
        if self.publisher.check(id, online, None) != crate::publish::SendCheck::Go {
            return None;
        }
        self.publisher.mark_submission(
            id,
            false,
            "Submission pending; check the provider before repeating",
        );
        if let Err(e) = self.publisher.save(&self.store) {
            if let Some(post) = self.publisher.posts.iter_mut().find(|p| p.id == id) {
                post.state = prior_state;
            }
            return Some(format!(
                "Didn't submit: I couldn't save its submission record ({e})."
            ));
        }
        // Bluesky goes through its own API with the app password from the
        // vault (social step 4); everything else through Atlas's browser.
        let bluesky = self.publisher.get(id).is_some_and(|p| crate::delivery::is_bluesky(&p.channel)).then(|| {
            if self.vault.state() != crate::vault::State::Open {
                crate::heard!(self.vault.open_unattended(t));
            }
            let password = self.vault.get(crate::social::VAULT_BLUESKY, t).unwrap_or_default();
            (self.social_cfg().bluesky_handle, password)
        });
        let work: crew::Work = Box::new(move |ctl| {
            let outcome = match &bluesky {
                Some((handle, password)) => crate::delivery::send_bluesky_unless(&mut publisher, &crate::social::posting::Live, handle, password, id, online, crate::store::now(), &|| ctl.stopping()),
                None => match crate::browser::Browser::start(&bcfg, &vars) {
                    Ok(mut b) => {
                        let o = crate::delivery::send_unless(&mut publisher, &mut b, &bcfg, id, online, &|| ctl.stopping());
                        b.close();
                        o
                    }
                    Err(e) => crate::delivery::classify(e),
                },
            };
            let (kind, msg) = match &outcome {
                crate::delivery::Outcome::Sent(m) => ("sent", m.clone()),
                crate::delivery::Outcome::Retry(m) => ("retry", m.clone()),
                crate::delivery::Outcome::Blocked(m) => ("blocked", m.clone()),
                crate::delivery::Outcome::Uncertain(m) => ("uncertain", m.clone()),
            };
            Ok(format!("{id}\t{kind}\t{msg}"))
        });
        if let Some(job) =
            self.hand_off_as("post", t, work, Some(id.to_string()), SpeakPolicy::Always) {
            self.posting.push((id, u64::MAX));
            self.publication_jobs.insert(job, (id, t));
        } else {
            if let Some(post) = self.publisher.posts.iter_mut().find(|p| p.id == id) {
                post.state = prior_state;
            }
            if let Err(e) = self.publisher.save(&self.store) {
                return Some(format!("Didn't start submission, but its pending record couldn't be cleared ({e}). Check before retrying."));
            }
        }
        None
    }

    pub(super) fn post_news(&mut self, job: u64, ending: &crew::Ending, t: u64) -> Option<String> {
        let expected = self.publication_jobs.remove(&job).map(|(post, _)| post);
        let crew::Ending::Done(Ok(line)) = ending else {
            let id = expected?;
            let why = match ending {
                crew::Ending::Done(Err(e)) => {
                    format!("Publication worker failed ({e}); the service outcome is unconfirmed.")
                }
                crew::Ending::Stopped => {
                    "Publication stopped; the service outcome is unconfirmed.".into()
                }
                _ => "Publication worker disappeared; the service outcome is unconfirmed.".into(),
            };
            return Some(self.publication_lost(id, &why));
        };
        let expected = expected?;
        let mut parts = line.splitn(3, '\t');
        let id = parts.next().and_then(|id| id.parse::<u64>().ok());
        let kind = parts.next().unwrap_or("");
        let msg = parts.next().unwrap_or("");
        if id != Some(expected)
            || !matches!(kind, "sent" | "retry" | "blocked" | "uncertain")
            || msg.trim().is_empty()
        {
            return Some(self.publication_lost(expected, "Publication returned an incomplete or mismatched result; its service outcome is unconfirmed."));
        }
        let id = expected;
        let kind = kind.to_string();
        let msg = msg.to_string();
        self.posting.retain(|(p, _)| *p != id);
        if kind == "retry" {
            self.posting.push((id, t + 300));
        }
        let what = self.publisher.get(id).map(|p| p.describe()).unwrap_or_default();
        let said = match kind.as_str() {
            "sent" => {
                self.publisher.mark_sent(id, &format!("Program confirmed publication: {msg}"), true);
                self.journal.record_at(Act::Published, &format!("{what}: {msg}"), true, t);
                Some(format!("Posted: {what}."))
            }
            // Tried again on a later tick.
            "retry" => {
                if let Some(post) = self.publisher.posts.iter_mut().find(|p| p.id == id) {
                    post.state = crate::publish::PostState::ReadyToSend;
                }
                None
            }
            "uncertain" => {
                self.publisher.mark_submission(id, true, &msg);
                self.journal
                    .record_at(Act::Blocked, &format!("{what}: {msg}"), false, t);
                Some(msg.clone())
            }
            _ => {
                self.publisher.mark_sent(id, &msg, false);
                self.journal.record_at(Act::Blocked, &format!("{what}: {msg}"), false, t);
                let hint = if msg.to_lowercase().contains("not signed in") {
                    " Say \"sign me into\" the site and I'll post it after."
                } else {
                    ""
                };
                Some(format!("Didn't post {what}: {msg}.{hint}"))
            }
        };
        if let Err(e) = self.publisher.save(&self.store) {
            let why = format!("Its publication result couldn't be saved ({e}). {} Check the service before another attempt.", said.unwrap_or_else(|| "No durable terminal receipt is available.".into()));
            self.publisher.mark_submission(id, true, &why);
            return Some(why);
        }
        said
    }

    fn publication_lost(&mut self, id: u64, why: &str) -> String {
        self.posting.retain(|(post, _)| *post != id);
        self.publisher.mark_submission(id, true, why);
        if let Err(e) = self.publisher.save(&self.store) {
            return format!("{why} Its record couldn't be saved ({e}); don't repeat it.");
        }
        format!("{why} Check the service before another attempt.")
    }
}

impl<'a> Daemon<'a> {
    pub(super) fn press_button(&mut self, said: &str) -> String {
        let Some((name, app)) = crate::uia::button_request(said) else {
            return "Which button?".into();
        };
        // The window: the named app's, or the one in front.
        let win = match &app {
            Some(a) => match self.cfg.apps.apps.iter().find(|(n, _)| n.eq_ignore_ascii_case(a)).map(|(_, s)| s) {
                Some(spec) => self.plat.find_window(spec).ok().flatten(),
                None => {
                    let spec = crate::config::AppSpec::for_process(&format!("{a}.exe"));
                    self.plat.find_window(&spec).ok().flatten()
                }
            },
            None => self.plat.active_window_id().ok().flatten(),
        };
        let where_ = app.clone().unwrap_or_else(|| "the window in front".into());
        let Some(win) = win else {
            return format!("I can't find {where_} open.");
        };
        // Read the window first: exactly the control you named, and enabled.
        let tree = self.plat.read_window(win).ok().flatten();
        match tree.as_ref().and_then(|t| t.by_name(&name)) {
            None => return format!("I can't see a \"{name}\" button in {where_}."),
            Some(n) if !n.enabled => return format!("\"{name}\" is greyed out in {where_}."),
            Some(_) => {}
        }
        if crate::uia::cannot_be_undone(&name) {
            let q = format!("Press \"{name}\" in {where_}? That one can't be taken back.");
            self.session.ask(&q);
            self.pending_press = Some((win.0, name, where_));
            return q;
        }
        self.press_now(win, &name, &where_)
    }

    pub(super) fn press_now(&mut self, win: crate::platform::WindowId, name: &str, where_: &str) -> String {
        match self.plat.press_named(win, name) {
            Ok(true) => {
                self.journal.record_at(Act::Upkeep, &format!("pressed \"{name}\" in {where_}"), true, crate::store::now());
                format!("Pressed \"{name}\" in {where_}.")
            }
            Ok(false) => format!("I found \"{name}\" in {where_} but it wouldn't press — it's yours to click."),
            Err(e) => format!("I couldn't press it: {e}."),
        }
    }
}

impl<'a> Daemon<'a> {
    pub(super) fn move_big_files(&mut self, said: &str, _t: u64) -> String {
        let moved: Vec<crate::tune::Moved> = self.store.load(crate::tune::MOVED_RECORD);
        if said.to_lowercase().contains("where did you move") {
            if moved.is_empty() {
                return "I haven't moved anything.".into();
            }
            let each: Vec<String> = moved.iter().map(|m| format!("{} → {} ({} MB)", m.from, m.to, m.mb)).collect();
            return format!("Moved: {}.", each.join("; "));
        }
        let r = self.plat.readings();
        let survey = crate::tune::Survey {
            disk_free_gb: r.disk_free_gb,
            disk_total_gb: r.disk_total_gb,
            other_drives: crate::tune::other_drives(),
            ..Default::default()
        };
        let root = self.store.install_root().display().to_string();
        let Some(plan) = crate::tune::storage_plan(&survey, &root) else {
            return if survey.other_drives.is_empty() {
                "There's no other drive plugged in with room to take them.".into()
            } else {
                "There's nothing big of mine worth moving.".into()
            };
        };
        let each: Vec<String> = plan.moves.iter().map(|(f, to, mb)| format!("{f} ({mb} MB) to {to}")).collect();
        let q = format!(
            "I'd move {}, freeing about {} MB. {} I'll check every file arrived before removing anything, leave a note \
             where each was, and point my settings at the new place. Go ahead?",
            each.join(", "),
            plan.frees_mb,
            plan.note
        );
        self.session.ask(&q);
        self.pending_storage = Some(plan);
        q
    }

    pub(super) fn carry_out_storage_plan(&mut self, plan: crate::tune::StoragePlan, t: u64) -> String {
        if let Err(error) = self.history.save_merged(&self.store) { return format!("No folder move started: pending recovery history could not be saved ({error})."); }
        let store = self.store.clone();
        let work: crew::Work = Box::new(move |ctl| {
            let mut done = Vec::new();
            let mut failed = Vec::new();
            for (from, to, _) in &plan.moves {
                if ctl.checkpoint() {
                    break;
                }
                match crate::tune::move_folder_durably(&store, std::path::Path::new(from), std::path::Path::new(to), t, &|| ctl.checkpoint()) {
                    Ok(m) => done.push(m),
                    Err(e) => failed.push(e),
                }
            }
            serde_json::to_string(&(done, failed)).map_err(|e| e.to_string())
        });
        if self.hand_off("move-files", t, work, None, SpeakPolicy::Always) {
            "Moving them now — I'll say when it's done.".into()
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    /// "Organize my PC", "clean up my desktop", "sort the files in D:\Stuff",
    /// "find duplicates in my downloads" (Eric, 29 Sep and 2 Oct 2026: "can't
    /// organize my PC properly"). The whole sentence is read
    /// (`organize::read_sort_request`): the folder named, or the desktop,
    /// Downloads and Documents as this machine has them. The plan is said --
    /// how many of each kind go where, with a few names, the copies and old
    /// installers that go to "To review", what's left and why -- and nothing
    /// moves until the yes (`carry_out_sorting`). Before 2 Oct this only
    /// filed the desktop's and Downloads' loose files by extension into
    /// Documents\Filed, found no copies and couldn't be undone.
    pub(super) fn tidy_desktop(&mut self) -> String {
        let sys = self.tools_cfg().system.clone();
        if !sys.enabled {
            return "Moving your files is switched off -- turn on System changes in Settings and ask me again. I'd only move files into folders, never delete anything."
                .into();
        }
        let said = self.last_said.clone();
        let ask = crate::organize::read_sort_request(&said);
        if let Some(words) = &ask.not_found {
            return format!(
                "I couldn't find a folder called \"{words}\" on this computer. Say it with its full path, like the one Explorer shows at the top."
            );
        }
        if ask.folders.is_empty() {
            return "I couldn't work out where your desktop, Downloads and Documents are on this computer -- name the folder with its full path.".into();
        }
        // The same gate every move will go through, asked once up front, so
        // a folder outside the ones Atlas may work in is said now rather
        // than as a list of refusals after the yes.
        let probe = |from: &std::path::Path, to: &std::path::Path| {
            crate::system::judge(
                &crate::system::Change::MoveFile {
                    from: from.join("x").display().to_string(),
                    to: to.join(crate::organize::TO_REVIEW).join("x").display().to_string(),
                },
                &sys,
            )
        };
        for f in &ask.folders {
            if let crate::system::Verdict::Refuse(why) = probe(f, ask.into.as_deref().unwrap_or(f)) {
                return format!("I can't sort {}: {why}", f.display());
            }
        }
        let now = crate::store::now();
        let plan = crate::organize::plan_folders(&ask.folders, ask.into.as_deref(), ask.copies_only, now, std::time::Duration::from_secs(8));
        let words = crate::organize::plan_said(&plan);
        if plan.has_moves() {
            self.session.ask(&words);
            self.pending_desktop = Some(plan);
        }
        words
    }

    /// What an optimization run would offer, measured now (2 Oct 2026):
    /// the programs worth closing from a sampled reading (`tune::pick_to_close`,
    /// with Atlas's own process tree, its name on this machine, the program
    /// in front of you and what you've used in the last hour all spared), and
    /// the startup entries you haven't opened this week. Temp and moves are
    /// the caller's to add.
    pub(super) fn tune_plan(&mut self, sampled: Option<&crate::tune::Sampled>, with_tasks: bool) -> crate::tune::Plan {
        let cfg = self.tools_ref().map(|t| t.tune.clone()).unwrap_or_default();
        let now = crate::store::now();
        let apps_since = |since: u64| -> Vec<String> {
            let mut v: Vec<String> = self.worklog.between(since, now).iter().map(|sp| sp.app.clone()).collect();
            v.sort();
            v.dedup();
            v
        };
        let in_use = apps_since(now.saturating_sub(3600));
        let week = apps_since(now.saturating_sub(7 * 86_400));
        let own_exe = std::env::current_exe().ok();
        let own_name = own_exe.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let close = match sampled {
            Some(sm) => {
                let mut own_names: Vec<String> = crate::tune::ATLAS_HELPERS.iter().map(|s| s.to_string()).collect();
                if !own_name.is_empty() {
                    own_names.push(own_name.clone());
                }
                let spare = crate::tune::Spare {
                    own_pids: crate::tune::atlas_family(&sm.procs, std::process::id()),
                    own_names,
                    foreground: self.plat.active_window().ok().flatten().map(|w| w.process),
                    foreground_pid: sm.foreground_pid,
                    in_use,
                    keep: cfg.keep.clone(),
                    min_mb: cfg.min_mb,
                    min_cpu: 5.0,
                };
                crate::tune::pick_to_close(&sm.load, &spare)
            }
            None => Vec::new(),
        };
        let own_path = own_exe.map(|p| p.display().to_string()).unwrap_or_default();
        let stop_starting = crate::tune::pick_startup_to_stop(&crate::tune::startup_entries_kept(with_tasks), &week, &cfg.keep, &own_path);
        crate::tune::Plan { close, stop_starting, temp: None, moves: None }
    }

    /// "Close what I don't need", "what's slowing my computer down", "what
    /// starts with Windows", "what's taking my space", "clear my temp
    /// files", "move my big downloads to D:\Archive" (Eric, 2 Oct 2026: "for
    /// optimization I want Atlas to be able to do more"). Each looks, says
    /// what it found with the numbers, and offers what it would do -- done
    /// only on your yes (`carry_out_optimize`), and every change kept where
    /// "undo" and "what did you do" find it.
    pub(super) fn tune_up(&mut self, said: &str) -> String {
        use crate::tune::TuneAsk;
        let cfg = self.tools_ref().map(|t| t.tune.clone()).unwrap_or_default();
        let home = crate::doctor::lookup_env("USERPROFILE").or_else(|| crate::doctor::lookup_env("HOME")).map(std::path::PathBuf::from);
        let downloads = home.as_ref().map(|h| h.join("Downloads"));
        let temp = std::env::temp_dir();
        let ask = crate::tune::tune_ask(said);
        let (mut words, plan) = match ask {
            TuneAsk::Slowing | TuneAsk::Close => {
                let Some(sm) = crate::tune::sample_machine(std::time::Duration::from_secs(2)) else {
                    let mem: Vec<String> =
                        crate::tune::memory_by_app().into_iter().take(4).map(|(a, mb)| format!("{a} {mb} MB")).collect();
                    return format!(
                        "Measuring each program's processor use, and closing programs, is only done on Windows, and this isn't Windows.{}",
                        if mem.is_empty() { String::new() } else { format!(" Holding the most memory: {}.", mem.join(", ")) }
                    );
                };
                let plan = self.tune_plan(Some(&sm), false);
                let mut w = crate::tune::slowest_words(&sm.load);
                if plan.close.is_empty() {
                    w.push_str(" Nothing is worth closing: everything heavy is either in use, in front of you, part of Windows, or me.");
                }
                (w, crate::tune::Plan { stop_starting: Vec::new(), ..plan })
            }
            TuneAsk::Startup => {
                if !cfg!(windows) {
                    return "Startup programs are only read and changed on Windows, and this isn't Windows.".into();
                }
                let all = crate::tune::startup_entries(true);
                let mut plan = self.tune_plan(None, true);
                plan.close.clear();
                (crate::tune::startup_words(&all), plan)
            }
            TuneAsk::Space | TuneAsk::ClearTemp => {
                let look = match &downloads {
                    Some(d) => crate::tune::look_at_space(d, &temp, std::time::Duration::from_secs(3)),
                    None => crate::tune::SpaceLook {
                        temp_mb: crate::tune::folder_mb(&temp, std::time::Duration::from_millis(500)),
                        complete: true,
                        ..Default::default()
                    },
                };
                let mut w = if ask == TuneAsk::ClearTemp {
                    format!("{} MB of temporary files.", look.temp_mb)
                } else {
                    crate::tune::space_words(&look)
                };
                if ask == TuneAsk::Space && (!look.biggest.is_empty() || !look.duplicates.is_empty()) {
                    w.push_str(" Say \"move my big downloads to\" a folder, or \"move the duplicates to\" one, and I'll move them there -- undo moves them back.");
                }
                let floor = if ask == TuneAsk::ClearTemp { 1 } else { cfg.min_mb };
                let temp_offer = (look.temp_mb >= floor).then(|| (temp.clone(), look.temp_mb));
                (w, crate::tune::Plan { temp: temp_offer, ..Default::default() })
            }
            TuneAsk::MoveWhere => {
                return "Where to? Name the folder in full -- \"move my big downloads to D:\\Archive\" -- and I'll say what would go before anything moves.".into();
            }
            TuneAsk::MoveInto { to, duplicates } => {
                if !self.tools_cfg().system.enabled {
                    return "Moving your files is switched off -- turn on System changes in Settings and ask me again. I'd only move them into the folder you named, and undo moves them back.".into();
                }
                let Some(d) = downloads.clone().filter(|d| d.is_dir()) else {
                    return "I couldn't find your Downloads folder, so there's nothing for me to move.".into();
                };
                if let Err(why) = crate::tune::may_move_into(&to, &d) {
                    return format!("I won't move them there: {why}.");
                }
                let look = crate::tune::look_at_space(&d, &temp, std::time::Duration::from_secs(3));
                let files: Vec<std::path::PathBuf> = if duplicates {
                    look.extra_copies()
                } else {
                    look.biggest.iter().filter(|(_, mb)| *mb >= 100).map(|(p, _)| p.clone()).collect()
                };
                if files.is_empty() {
                    return if duplicates {
                        "There are no duplicate files in Downloads to move.".into()
                    } else {
                        "Nothing in Downloads is 100 MB or more, so there's nothing big to move.".into()
                    };
                }
                let names: Vec<String> = files
                    .iter()
                    .take(6)
                    .map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
                    .collect();
                let more = files.len().saturating_sub(names.len());
                let w = format!(
                    "From Downloads: {}{}.",
                    names.join(", "),
                    if more > 0 { format!(" and {more} more") } else { String::new() }
                );
                (w, crate::tune::Plan { moves: Some((files, to)), ..Default::default() })
            }
        };
        if !plan.is_empty() {
            let offer = plan.offer();
            words.push(' ');
            words.push_str(&offer);
            self.session.ask(&offer);
            self.pending_optimize = Some(plan);
        }
        words
    }

    /// An optimization run's plan, done. Each program asked to close and
    /// waited on; a background one that won't is ended, one with a window is
    /// left (it's nearly always asking to save). Each startup entry switched
    /// off the way Task Manager does it. Old temporary files cleared. Files
    /// moved into the folder you named. Every one is written into the history
    /// "what did you do" reads, with how to take it back where that's
    /// possible -- startup and moves through "undo" itself (2 Oct 2026).
    pub(super) fn carry_out_optimize(&mut self, plan: crate::tune::Plan, t: u64) -> String {
        use crate::undo::Undo;
        let mut done: Vec<String> = Vec::new();
        let mut not: Vec<String> = Vec::new();
        let mut undo_record: Vec<(u64, crate::tune::TuneUndo)> = self.store.load(crate::tune::TUNE_UNDO_RECORD);
        let closed = crate::tune::close_loads(&plan.close, std::time::Duration::from_secs(5));
        for (l, how) in plan.close.iter().zip(closed) {
            let app = &l.name;
            match how {
                crate::tune::Closed::Asked | crate::tune::Closed::Ended => {
                    done.push(format!("closed {app} ({} MB)", l.mem_mb));
                    self.journal.record_at(Act::Upkeep, &format!("closed {app} to free memory"), true, t);
                    // "closed X" is what `undo_intent` reads to open it again;
                    // a background program comes back by itself.
                    let undo = if l.windowed {
                        Undo::Atlas(format!("open {app} again"))
                    } else {
                        Undo::You("it starts again the next time you open it or restart".into())
                    };
                    let what = if l.windowed { format!("closed {app}") } else { format!("ended {app}, running in the background") };
                    self.history.note(&what, "windows", undo, true, t);
                }
                crate::tune::Closed::Gone => done.push(format!("{app} had already closed")),
                crate::tune::Closed::LeftOpen => {
                    not.push(format!("{app} is still open -- it may be asking you to save something, so I didn't force it"))
                }
                crate::tune::Closed::Failed(e) => not.push(format!("{app} didn't close ({e})")),
            }
        }
        for e in &plan.stop_starting {
            let name = &e.name;
            match crate::tune::set_startup(e, false) {
                Ok(()) => {
                    done.push(format!("{name} won't start with Windows"));
                    self.journal.record_at(Act::Upkeep, &format!("stopped {name} starting with Windows"), true, t);
                    let id = self.history.note(
                        &format!("stopped {name} starting with Windows"),
                        "settings",
                        Undo::Atlas(format!("let {name} start with Windows again")),
                        true,
                        t,
                    );
                    undo_record.push((id, crate::tune::TuneUndo::Startup(e.clone())));
                }
                Err(err) => not.push(format!("{name}'s startup ({err})")),
            }
        }
        if let Some((dir, _)) = &plan.temp {
            let c = crate::tune::clear_old_files(dir, 86_400);
            done.push(format!("cleared {} MB of temporary files ({} files; {} in use or recent, left)", c.mb, c.files, c.skipped));
            self.journal.record_at(Act::Upkeep, &format!("cleared {} MB of temporary files", c.mb), true, t);
            self.history.note(
                &format!("cleared {} MB of temporary files", c.mb),
                "files",
                Undo::Cannot("temporary files are deleted, not kept".into()),
                true,
                t,
            );
        }
        // Never evict an outstanding file recovery record to meet a quota.
        let mut recovery_ready = true;
        if let Err(e) = self.store.save(crate::tune::TUNE_UNDO_RECORD, &undo_record) {
            recovery_ready = false;
            not.push(format!("the latest recovery status couldn't be saved ({e}); the saved move intents remain available"));
        }
        if let Err(e) = self.history.save_merged(&self.store) {
            recovery_ready = false;
            not.push(format!("the latest history couldn't be saved ({e})"));
        }
        let moving = plan.moves.as_ref().map(|(files, to)| if recovery_ready {
            self.start_download_worker(files.clone(), to.clone(), t)
        } else { "No Downloads file moved: the preceding recovery changes could not be saved.".into() });
        let mut said = if done.is_empty() { moving.clone().unwrap_or_else(|| "Nothing changed.".to_string()) } else { format!("Done: {}. {}", done.join("; "), moving.unwrap_or_default()) };
        if !not.is_empty() {
            said.push_str(&format!(" Not done: {}.", not.join("; ")));
        }
        let r = self.plat.readings();
        if r.ram_total_gb > 0.0 && !plan.close.is_empty() {
            said.push_str(&format!(" Memory is at {:.0}% now.", r.ram_used_gb / r.ram_total_gb * 100.0));
        }
        said
    }

    /// A sorting plan, carried out on its yes (2 Oct 2026): each move judged
    /// and done (`organize::carry_out_moves` -- never over a file, never one
    /// open elsewhere, nothing deleted), every one written into the history
    /// "what did you do" reads and kept beside it, so "undo that" puts each
    /// file back where it was (`tune::TuneUndo::Organized`).
    pub(super) fn carry_out_sorting(&mut self, plan: crate::organize::SortPlan, t: u64) -> String {
        self.start_sort_worker(plan, t)
    }

    /// "Use my webcam mic" (Eric, 29 Sep 2026): the microphones this machine
    /// has, the one whose name fits `kind` ("webcam" fits "Microphone (HD Pro
    /// Webcam C920)"), recorded from now on and kept to -- the periodic
    /// re-pick leaves a microphone you chose alone while it's plugged in
    /// (`hearing::Hearing::chosen`). Named back, or the ones there are when
    /// none fits or several do.
    pub(super) fn use_microphone(&mut self, kind: &str) -> String {
        let Some(tc) = self.tools_ref().cloned() else {
            return "I can't hear on this machine yet -- there's no voice set up.".into();
        };
        let ffmpeg = tc.vars.get("ffmpeg").cloned().unwrap_or_else(|| "ffmpeg".into());
        let devices = match crate::audio::probe_devices(&ffmpeg) {
            Ok(d) => d,
            Err(e) => return format!("I couldn't list the microphones: {e}"),
        };
        let camera = tc.vars.get("webcam_device").cloned().unwrap_or_default();
        let mics: Vec<&crate::audio::Device> = devices.iter().filter(|d| d.kind == crate::audio::Kind::Input).collect();
        let names: Vec<String> = mics.iter().map(|d| crate::hearing::short(&d.name)).collect();
        match crate::hearing::mic_by_kind(&devices, kind, &camera) {
            crate::hearing::MicFit::One(d) => {
                let device = d.ffmpeg_name();
                crate::voice::set_microphone(&d.name, &device);
                let store = crate::roots::store();
                let mut hearing = crate::hearing::Hearing::load_from(&store);
                hearing.choose(&d.name);
                crate::kept!(hearing.save_to(&store));
                let line = format!("Listening with {} now, and I'll stay on it until you pick another.", crate::hearing::short(&d.name));
                self.log.info(&line);
                line
            }
            crate::hearing::MicFit::Several(ds) => {
                let n: Vec<String> = ds.iter().map(|d| crate::hearing::short(&d.name)).collect();
                format!("More than one microphone fits \"{kind}\": {}. Which one?", n.join(", "))
            }
            crate::hearing::MicFit::None => {
                if names.is_empty() {
                    "I can't find any microphone on this machine right now.".into()
                } else {
                    format!(
                        "None of the microphones here sounds like \"{}\". The ones I can hear from: {}.",
                        kind.trim_start_matches("the ").trim_start_matches("a "),
                        names.join(", ")
                    )
                }
            }
        }
    }

    pub(super) fn moved_news(&mut self, ending: &crew::Ending) -> Option<String> {
        let crew::Ending::Done(Ok(json)) = ending else {
            return Some("Moving the files didn't finish; nothing was removed that hadn't been copied.".into());
        };
        let (done, failed): (Vec<crate::tune::Moved>, Vec<String>) = serde_json::from_str(json).ok()?;
        // Settings follow the folders, so Atlas still finds its models.
        let dir = crate::roots::config_dir();
        let prefs_read = crate::preferences::Preferences::load_checked(&dir);
        let settings_unreadable = prefs_read.as_ref().err().cloned();
        let mut prefs = prefs_read.unwrap_or_default();
        for m in &done {
            if m.from.ends_with("models") {
                prefs.set("models.dir", &m.to);
            } else if m.from.ends_with("video") {
                prefs.set("video.work_dir", &m.to);
            }
            self.journal.record_at(Act::Upkeep, &format!("moved {} to {}", m.from, m.to), true, crate::store::now());
        }
        // Checked (30 Sep 2026): "Moved" was said when the new place wasn't
        // saved, and after a restart Atlas couldn't find its models.
        let settings_kept = match &settings_unreadable {
            Some(e) => Err(e.clone()),
            None => prefs.save(&dir).map_err(|e| e.to_string()),
        };
        let mut said = if done.is_empty() {
            if failed.is_empty() { "Nothing moved.".to_string() } else { "No folder move has a fully saved completion receipt; inspect the recovery history before trying again.".to_string() }
        } else {
            let mb: u64 = done.iter().map(|m| m.mb).sum();
            format!(
                "Moved {} folder{} ({mb} MB) — each old spot has a note saying where it went, and \"where did you move\" tells you any time.",
                done.len(),
                if done.len() == 1 { "" } else { "s" }
            )
        };
        if let (Err(e), false) = (&settings_kept, done.is_empty()) {
            said.push_str(&format!(" But I couldn't note the new place in your settings ({e}) -- set it on the Settings page, or I won't find them after a restart."));
        }
        if let Some(f) = failed.first() {
            said.push_str(&format!(" One didn't: {f}."));
        }
        Some(said)
    }
}

#[derive(Debug, Clone)]
pub(super) struct ApprovedUndo { pub id: u64, row: crate::undo::Did, identity: Option<String>, recovery: crate::tune::TuneUndo }

impl<'a> Daemon<'a> {
    pub(super) fn poll_approved_undo(&mut self) -> Option<String> {
        if self.attention.is_paused() || self.active_file_move_id().is_some() { return None; }
        let pending = self.approved_undo.clone()?;
        let store = self.store.clone();
        let _guard = match store.transaction() {
            Ok(guard) => guard,
            Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => return None,
            Err(error) => { self.approved_undo = None; return Some(format!("The approved undo stopped before moving files: {error}. Its recovery record remains available.")); },
        };
        let checked = (|| -> crate::error::Result<bool> {
            let history: crate::undo::History = store.load_checked("undo_history")?.unwrap_or_default();
            let records: Vec<(u64, crate::tune::TuneUndo)> = store.load_checked(crate::tune::TUNE_UNDO_RECORD)?.unwrap_or_default();
            Ok(history.identity(pending.id) == pending.identity.as_deref() && history.done.iter().any(|row| row == &pending.row) && records.iter().any(|(id, recovery)| *id == pending.id && recovery == &pending.recovery))
        })();
        self.approved_undo = None;
        match checked {
            Ok(true) => Some(self.carry_out_undo(pending.id)),
            Ok(false) => Some("The approved undo stopped because its recovery record changed; no file moved.".into()),
            Err(error) => Some(format!("The approved undo stopped because recovery storage could not be read ({error}); no file moved.")),
        }
    }

    pub(super) fn cancel_approved_undo(&mut self) -> bool { let waiting = self.approved_undo.take().is_some(); self.cancel_undo_worker() || waiting }

    pub(super) fn carry_out_undo(&mut self, id: u64) -> String {
        let store = self.store.clone();
        let current: Vec<(u64, crate::tune::TuneUndo)> = match store.load_checked(crate::tune::TUNE_UNDO_RECORD) {
            Ok(records) => records.unwrap_or_default(), Err(error) => return format!("No undo started: file recovery could not be read ({error})."),
        };
        if let Some((_, recovery)) = current.into_iter().find(|(row_id, recovery)| *row_id == id && !matches!(recovery, crate::tune::TuneUndo::Startup(_))) {
            let Some(row) = self.history.done.iter().find(|row| row.id == id && !row.undone).cloned() else { return "That recovery row is no longer available for undo.".into(); };
            return self.start_undo_worker(row, self.history.identity(id).map(str::to_owned), recovery);
        }
        let _guard = match store.transaction() {
            Ok(guard) => guard,
            Err(crate::error::AtlasError::Io(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let row = self.history.done.iter().find(|row| row.id == id && !row.undone).cloned();
                let records = store.load_checked::<Vec<(u64, crate::tune::TuneUndo)>>(crate::tune::TUNE_UNDO_RECORD);
                if let (Some(row), Ok(Some(records))) = (row, records) {
                    if let Some((_, recovery)) = records.into_iter().find(|(key, _)| *key == id) {
                        self.approved_undo = Some(ApprovedUndo { id, row, identity: self.history.identity(id).map(str::to_owned), recovery });
                        return "The approved undo is waiting for recovery storage. No file moved yet; I'll retry when it is available. Stop cancels this waiting undo.".into();
                    }
                }
                return "No undo started: recovery storage is busy. The saved recovery record remains available.".into();
            }
            Err(error) => return format!("No undo started: recovery storage is unavailable ({error}). The saved recovery record remains available."),
        };
        if let Err(error) = self.history.save_merged(&store) { return format!("No undo started: pending history could not be saved ({error})."); }
        self.history = match store.load_checked::<crate::undo::History>("undo_history") {
            Ok(history) => history.unwrap_or_default(),
            Err(error) => return format!("No undo started: recovery history could not be read ({error})."),
        };
        let Some(d) = self.history.done.iter().find(|d| d.id == id).cloned() else {
            return "That one isn't in my history any more.".into();
        };
        if d.undone {
            return format!("\"{}\" is already undone.", d.what);
        }
        // A startup entry switched off, or files moved, by an optimization
        // run (2 Oct 2026): taken back from what was kept beside it.
        let mut record: Vec<(u64, crate::tune::TuneUndo)> = match store.load_checked(crate::tune::TUNE_UNDO_RECORD) {
            Ok(records) => records.unwrap_or_default(),
            Err(error) => return format!("No undo started: file recovery records could not be read ({error})."),
        };
        if let Some(pos) = record.iter().position(|(rid, _)| *rid == id) {
            let undo = record[pos].1.clone();
            return match crate::tune::undo_tune_change_with_checkpoint(&undo, &mut |next| {
                record[pos].1 = next.clone();
                self.store
                    .save(crate::tune::TUNE_UNDO_RECORD, &record)
                    .map_err(|e| e.to_string())
            }) {
                Ok(said) => {
                    // Save completion before dropping the recovery intent. If
                    // either save fails, repeating the file undo is harmless.
                    self.history.mark_undone(id);
                    if let Err(e) = self.history.save_merged(&self.store) {
                        if let Some(entry) = self.history.done.iter_mut().find(|d| d.id == id) {
                            entry.undone = false;
                        }
                        return format!("{said} I couldn't save completion ({e}); the recovery record is kept for retry.");
                    }
                    record.remove(pos);
                    if let Err(e) = self.store.save(crate::tune::TUNE_UNDO_RECORD, &record) {
                        return format!("Undone: {}. {said} The old recovery record remains because cleanup couldn't be saved ({e}).", d.what);
                    }
                    format!("Undone: {}. {said}", d.what)
                }
                Err(why) => why,
            };
        }
        let said = if d.what == "wrote a draft" {
            // The newest draft, thrown away (cancelled, never sent).
            let draft = self
                .publisher
                .posts
                .iter()
                .rev()
                .find(|p| matches!(p.state, crate::publish::PostState::Draft | crate::publish::PostState::AwaitingApproval))
                .map(|p| p.id);
            match draft {
                Some(pid) if self.publisher.cancel(pid) => {
                    if let Err(e) = self.publisher.save(&self.store) { return format!("The draft is paused here, but discarding it couldn't be saved ({e}); its earlier saved state may remain after restart."); }
                    "Threw the draft away.".to_string()
                }
                _ => return "There's no draft left to throw away.".into(),
            }
        } else {
            match undo_intent(&d.what) {
                Some(intent) => self.execute(&intent),
                None => return format!("I don't know how to take back \"{}\" myself.", d.what),
            }
        };
        self.history.mark_undone(id);
        if let Err(error) = self.history.save_merged(&self.store) { return format!("{said} The undo acknowledgment is pending a durable save ({error}); I retained it for retry."); }
        format!("Undone: {}. {said}", d.what)
    }
}

impl<'a> Daemon<'a> {
    /// A flow seen as a chain: which steps can be taken back and which leave
    /// the machine, with the first `done` of them marked done (G7).
    pub(super) fn chain_of(&self, run: &crate::flow::Run, done: usize) -> crate::chain::Chain {
        let steps: Vec<crate::chain::Step> = run
            .steps
            .iter()
            .map(|s| {
                let intent = self.parser.parse(&s.command);
                let cat = crate::categories::category_of(&intent);
                let goes_out = matches!(
                    cat,
                    crate::categories::Category::AgreementExternal | crate::categories::Category::ExternalAiCreative
                ) || matches!(intent, Intent::Message(_) | Intent::DraftPost(_) | Intent::SchedulePost(_));
                // Reversible: what "undo" knows how to take back, or anything
                // that only works on this machine and isn't a send.
                let reversible = !goes_out
                    && (undo_intent(&crate::intent::Intent::plain(&intent)).is_some()
                        || matches!(cat, crate::categories::Category::LocalOperational | crate::categories::Category::LocalCreative));
                crate::chain::Step {
                    what: s.command.clone(),
                    app: String::new(),
                    reversible,
                    goes_out,
                    needs: None,
                    produces: s.produces.clone(),
                }
            })
            .collect();
        let mut chain = crate::chain::Chain::new(&run.workflow, steps);
        for _ in 0..done.min(chain.steps.len()) {
            chain.done(None);
        }
        chain
    }
}

impl<'a> Daemon<'a> {
    /// "Get this video ready: C:\clips\bakery.mp4" -- the studio (`studio`):
    /// dead air cut, captions, thumbnail frames, a title, all on a copy.
    pub(super) fn studio_help(&mut self, said: &str, t: u64) -> Option<String> {
        let low = said.to_ascii_lowercase();
        if !["get this video ready", "get my video ready", "get the video ready", "video studio", "prep this video", "prepare this video", "ready this video", "studio this"]
            .iter()
            .any(|p| low.contains(p))
        {
            return None;
        }
        let Some((path, wish)) = crate::edit::path_and_wish(said) else {
            return Some("Which video? Give me its path -- get this video ready: \"C:\\clips\\bakery.mp4\".".into());
        };
        let original = std::path::PathBuf::from(&path);
        let aspect = crate::studio::requested_aspect(&wish);
        if !original.is_file() {
            return Some(format!("I can't find {path}."));
        }
        let tools = self.tools_cfg();
        let video = tools.video.clone();
        let timed = tools.stt_timed.clone();
        let mut vars = tools.vars.clone();
        self.add_language_vars(&mut vars);
        let llm = self.background_llm();
        let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "video".into());
        let work: crew::Work = Box::new(move |ctl| {
            let run = |tool: &crate::tools::ExternalTool, args: Vec<String>| -> std::result::Result<std::process::Output, String> {
                if ctl.checkpoint() { return Err("stopped".into()); }
                crate::studio::run_tool(tool, args, &|| ctl.stopping())
            };
            let folder = crate::studio::reserve_folder(&original).map_err(|e| format!("couldn't reserve a result folder: {e}"))?;
            let s = |p: &std::path::Path| p.display().to_string();
            let src = s(&original);
            let probed = run(&video.ffprobe, crate::edit::probe_args(&src))?;
            let (before, audio) = crate::studio::probe(&String::from_utf8_lossy(&probed.stdout))?;
            if ctl.checkpoint() {
                return Err("stopped".into());
            }
            let mut notes = Vec::new();
            let mut remaining = Vec::new();
            let spans = if audio {
                let found = run(&video.ffmpeg, crate::studio::silence_args(&src))?;
                let keep = crate::studio::keep_spans(&crate::studio::silences(&String::from_utf8_lossy(&found.stderr), before), before);
                if keep.is_empty() { notes.push("The audio is entirely silent; the full picture was preserved for your review.".to_string()); vec![(0.0, before)] } else { keep }
            } else {
                notes.push("No audio stream: the full picture was preserved; captions and audio editing were skipped.".to_string());
                vec![(0.0, before)]
            };
            let cut = folder.join(format!("{stem} - cut.mp4"));
            let made = run(&video.ffmpeg, crate::studio::cut_args_with_audio(&src, &spans, &s(&cut), audio))?;
            if !made.status.success() || !cut.is_file() {
                return Err("ffmpeg couldn't make the cut".into());
            }
            let output_probe = run(&video.ffprobe, crate::edit::probe_args(&s(&cut)))?;
            let (after, output_audio) = crate::studio::probe(&String::from_utf8_lossy(&output_probe.stdout))?;
            if audio != output_audio { return Err(format!("Rendered audio does not match the source; review {}", folder.display())); }
            if let Some((width, height)) = aspect {
                let framed = folder.join(format!("{stem} - {width}x{height}.mp4"));
                run(&video.ffmpeg, crate::studio::aspect_args(&s(&cut), &s(&framed), width, height))?;
                let checked = run(&video.ffprobe, crate::edit::probe_args(&s(&framed)))?;
                let json = String::from_utf8_lossy(&checked.stdout);
                let (length, has_audio) = crate::studio::probe(&json)?;
                if crate::studio::dimensions(&json) != Some((width, height)) || has_audio != audio || (length-after).abs() > 0.5 {
                    return Err(format!("Aspect copy failed output checks; review {}", folder.display()));
                }
                notes.push(format!("Requested aspect copy: {width}x{height}, with padding to keep the whole image. Preview it in the destination app: captions, controls and account overlays vary; no fixed safe-area claim has been verified."));
            }
            if let Err(error) = run(&video.ffmpeg, crate::studio::thumb_args(&s(&cut), &s(&folder.join("thumbnail-%02d.jpg")))) {
                if ctl.stopping() { return Err("stopped".into()); }
                notes.push(format!("Scene thumbnails failed: {error}"));
            }
            if !folder.join("thumbnail-01.jpg").is_file() {
                if let Err(error) = run(&video.ffmpeg, crate::studio::thumbnail_fallback_args(&s(&cut), &s(&folder.join("thumbnail-%02d.jpg")), after)) {
                    if ctl.stopping() { return Err("stopped".into()); }
                    notes.push(format!("Sample thumbnails failed: {error}"));
                }
            }
            let thumbs = std::fs::read_dir(&folder).map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("thumbnail-") && e.path().extension().is_some_and(|s| s == "jpg") && e.metadata().is_ok_and(|m| m.len() > 0)).count()).unwrap_or(0);
            if thumbs == 0 { remaining.push("Choose or make a thumbnail; no usable frame was saved.".into()); }
            let caption_file = folder.join(format!("{stem} - cut.srt"));
            let transcript = if audio {
                match transcript_checked(&video, timed.as_ref(), &vars, &s(&cut), &folder, Some(&caption_file), &|| ctl.stopping()) {
                    Ok(text) => text,
                    Err(error) => { if ctl.stopping() { return Err("stopped".into()); } remaining.push("Captions still need to be prepared or explicitly skipped.".into()); notes.push(format!("Captions unavailable: {error}")); String::new() }
                }
            } else { String::new() };
            let mut title = None;
            if let (Some(m), false) = (llm.as_deref(), transcript.trim().is_empty()) {
                let excerpt: String = transcript.chars().take(8000).collect();
                let quoted = crate::untrusted::Read::new("a video transcript excerpt", &excerpt, crate::store::now()).quoted();
                let request = crate::brain::ChatRequest { messages: vec![crate::brain::Msg::system(crate::studio::TITLE_PROMPT), crate::brain::Msg::user(quoted)], max_tokens: 384, aside: true, ..Default::default() };
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
                let mut bytes = 0usize;
                let result = if m.supports_bounded_chat() { m.chat_until(&request, &mut |text| { bytes += text.len(); bytes <= 4096 && !ctl.stopping() && std::time::Instant::now() < deadline }, &|| !ctl.stopping() && std::time::Instant::now() < deadline).ok().filter(|r| r.text.len() <= 4096 && !ctl.stopping() && std::time::Instant::now() < deadline) } else { None };
                if let Some((t, d)) = result.and_then(|r| crate::studio::title_and_description(&r.text, &excerpt)) {
                    std::fs::write(folder.join("title and description.txt"), format!("Suggested wording; review against the transcript before approval.\n\n{t}\n\n{d}\n")).map_err(|e| format!("couldn't save suggested wording: {e}"))?;
                    title = Some(t);
                }
            }
            if title.is_none() { remaining.push("Title and description need your wording or a valid model suggestion.".into()); }
            if ctl.checkpoint() { return Err("stopped".into()); }
            notes.push(if aspect.is_none() { "Platform/aspect not chosen: original framing preserved. Audio correction, colour and platform safe areas still need review. Nothing has been published.".into() } else { "The native cut is also preserved. Audio correction, colour and platform safe areas still need review. Nothing has been published.".into() });
            std::fs::write(folder.join("review.txt"), format!("Source: {src}\nOutput: {}\nMeasured duration: {after:.3}s\nCaptions saved: {}\nThumbnail files: {thumbs}\n{}\n", s(&cut), caption_file.is_file(), notes.join("\n")))
                .map_err(|e| format!("couldn't save the review: {e}"))?;
            let text = format!("A review copy is in {}: measured {} seconds, from {} seconds. Captions saved: {}. Thumbnail choices: {thumbs}. {} {} Original untouched.", s(&folder), after.round(), before.round(), caption_file.is_file(), title.map(|t| format!("Suggested title: {t}. Wording needs your review.")).unwrap_or_else(|| "No title suggestion was saved.".into()), notes.join(" "));
            let files = std::fs::read_dir(&folder).map_err(|e| format!("couldn't verify review files: {e}"))?.filter_map(|e| e.ok()).filter(|e| e.path().is_file()).map(|e| s(&e.path())).collect();
            crate::content::review_worker_result("studio_review", if remaining.is_empty() { "review_ready" } else { "partial" }, text, Some(s(&folder)), files, remaining)
        });
        Some(if self.hand_off("studio", t, work, Some(path.clone()), SpeakPolicy::Always) {
            format!("Preparing a review copy of {path}. I'll report which editing steps and files actually succeeded; nothing will be posted.")
        } else {
            "I'm swamped with background work right now -- ask me again in a moment.".into()
        })
    }

    pub(super) fn edit_media(&mut self, said: &str, t: u64) -> String {
        let Some((path, wish)) = crate::edit::path_and_wish(said) else {
            return "Which video? Give me its path — \"edit \"C:\\clips\\trip.mp4\" to cut the dead air\".".into();
        };
        let original = std::path::PathBuf::from(&path);
        if !original.is_file() {
            return format!("I can't find {path}.");
        }
        if wish.trim().is_empty() {
            return "What should the edit do?".into();
        }
        let Some(llm) = self.llm.clone() else {
            return "I'd need a model to plan the edit, and none is configured.".into();
        };
        let video = self.tools_cfg().video.clone();
        let work_dir = std::path::PathBuf::from(&video.work_dir);
        let (copy, result) = crate::edit::copy_and_result_paths(&original, &work_dir);
        let work: crew::Work = Box::new(move |_ctl| {
            // The copy first: everything happens to it, never the original.
            std::fs::create_dir_all(&work_dir).map_err(|e| e.to_string())?;
            std::fs::copy(&original, &copy).map_err(|e| format!("couldn't make a copy to work on: {e}"))?;
            let probed = crate::tools::command(&video.ffprobe.command)
                .args(crate::edit::probe_args(&copy.display().to_string()))
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default();
            let len = crate::edit::duration_from_probe(&probed).unwrap_or(0.0);
            let reply = llm
                .complete(crate::edit::PLANNER_PROMPT, &format!("The video is {len:.0} seconds long. What's wanted: {wish}"))
                .map_err(|e| e.to_string())?;
            let plan = crate::edit::plan_from_model(&reply, vec![copy.display().to_string()], &result.display().to_string())
                .map_err(|e| e.to_string())?;
            plan.validate(&[len]).map_err(|e| format!("the plan didn't hold up: {e}"))?;
            crate::edit::render(&video.ffmpeg, &plan, &crate::tools::Vars::new()).map_err(|e| e.to_string())?;
            let described = crate::edit::describe(&plan, len);
            serde_json::to_string(&(
                original.display().to_string(),
                copy.display().to_string(),
                result.display().to_string(),
                described,
            ))
            .map_err(|e| e.to_string())
        });
        if self.hand_off("edit-media", t, work, Some(path.clone()), SpeakPolicy::Always) {
            format!("Working on a copy of {path} — the original isn't touched. I'll show you the result.")
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }
}

impl<'a> Daemon<'a> {
    /// A handed-over file that's a PDF, a Word file or a zip is read as one
    /// rather than as plain text.
    pub(super) fn read_as_document(item: &crate::tray::Item) -> bool {
        let name = item.stored_at.as_deref().unwrap_or(&item.what).to_lowercase();
        [".pdf", ".docx", ".zip"].iter().any(|e| name.ends_with(e))
    }

    /// The file you meant: a path in what you said, or the last one you
    /// handed over of the right kind.
    fn file_meant(&self, said: &str, exts: &[&str]) -> Option<String> {
        if let Some(p) = crate::files::path_in(said, exts) {
            return Some(p);
        }
        // From the list a search just gave: "read number 2", "read the second
        // one", "read the lease" (30 Sep 2026: only open/show/merge/sign took a
        // number, and read said "Which file? Give me its path").
        let listed = self.files_last_listed();
        let fits = |p: &String| exts.iter().any(|e| p.to_lowercase().ends_with(&format!(".{e}")));
        if !listed.is_empty() {
            let low = said.to_lowercase();
            let as_open = low.split_once(' ').map(|x| x.1).map(|rest| format!("open {rest}")).unwrap_or_default();
            if let Some(i) = crate::findfile::which(&as_open, listed.len()) {
                if let Some(p) = listed.get(i).filter(|p| fits(p)) {
                    return Some(p.clone());
                }
            }
            let words: Vec<&str> = low
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() > 2 && !["read", "the", "that", "this", "file", "pdf", "document", "summarise", "summarize", "open", "me", "what", "does", "say"].contains(w))
                .collect();
            if !words.is_empty() {
                if let Some(p) = listed.iter().filter(|p| fits(p)).find(|p| {
                    let name = std::path::Path::new(p.as_str()).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                    words.iter().all(|w| name.contains(w))
                }) {
                    return Some(p.clone());
                }
            }
        }
        self.tray
            .items
            .iter()
            .rev()
            .map(|i| i.stored_at.clone().unwrap_or_else(|| i.what.clone()))
            .find(|p| {
                let l = p.to_lowercase();
                exts.iter().any(|e| l.ends_with(&format!(".{e}")))
            })
    }

    pub(super) fn read_document_asked(&mut self, said: &str) -> String {
        let Some(path) = self.file_meant(said, &["pdf", "docx", "txt", "md", "zip"]) else {
            let question = "Which file? Give me its path, or hand it to me first.".to_string();
            self.session.ask(&question);
            self.execution_receipt = Some((Intent::ReadDocument(said.into()), crate::taskloop::Outcome::NeedsYou(question.clone())));
            return question;
        };
        let job = if path.to_lowercase().ends_with(".zip") { FileJob::Unzip } else { FileJob::Read };
        self.last_crew_handoff = None;
        let text = self.file_work_off_the_loop(job, &path, false);
        let outcome = if self.last_crew_handoff.is_some() { crate::taskloop::Outcome::Started(text.clone()) } else { crate::taskloop::Outcome::Failed(text.clone()) };
        self.execution_receipt = Some((Intent::ReadDocument(said.into()), outcome));
        text
    }

    pub(super) fn unzip_asked(&mut self, said: &str) -> String {
        let Some(path) = self.file_meant(said, &["zip"]) else {
            let question = "Which zip? Give me its path, or hand it to me first.".to_string();
            self.session.ask(&question);
            self.execution_receipt = Some((Intent::Unzip(said.into()), crate::taskloop::Outcome::NeedsYou(question.clone())));
            return question;
        };
        self.last_crew_handoff = None;
        let text = self.file_work_off_the_loop(FileJob::Unzip, &path, false);
        let outcome = if self.last_crew_handoff.is_some() { crate::taskloop::Outcome::Started(text.clone()) } else { crate::taskloop::Outcome::Failed(text.clone()) };
        self.execution_receipt = Some((Intent::Unzip(said.into()), outcome));
        text
    }

    /// Reading or unpacking a file, on the crew (28 Sep 2026).
    ///
    /// Both are things the model can choose for you ("read this pdf",
    /// "unzip that"), and both ran inside the turn on the daemon's loop: a
    /// Windows Defender scan first (seconds, or minutes on a big zip), then
    /// the reading -- a scanned PDF is every page through the word reader --
    /// or the unpacking and a second scan. All that time the hub answered
    /// nothing and "stop" reached nothing. Now the whole of it is an errand:
    /// the approval rules are unchanged (they are decided in `turn_from` and
    /// `execute` before this is reached, and a file the scan couldn't check
    /// is still opened only on your yes -- the question is asked when the
    /// errand comes back), and the answer is said when it's ready. With the
    /// crew full it fails visibly; it never scans on the control loop.
    pub(super) fn file_work_off_the_loop(&mut self, job: FileJob, path: &str, anyway: bool) -> String {
        let tools = self.tools_cfg();
        let (p, what) = (path.to_string(), job);
        let work: crew::Work = Box::new(move |ctl: &crew::Control| {
            if ctl.checkpoint() { return Err("Stopped before opening the file.".into()); }
            let done = match what {
                FileJob::Read => read_document_until(&p, anyway, &tools, &|| ctl.stopping()),
                FileJob::Unzip => unzip_until(&p, anyway, &tools, &|| ctl.stopping()),
            };
            // Once unpacking has happened, retain its actual result rather
            // than claim the file was untouched by a late cancellation.
            serde_json::to_string(&done).map_err(|e| e.to_string())
        });
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
        if self.hand_off("read-file", crate::store::now(), work, Some(format!("{} {path}", if job == FileJob::Read { "read" } else { "unzip" })), SpeakPolicy::Always) {
            match job {
                FileJob::Read => format!("Reading {name} -- I'll tell you what's in it in a moment."),
                FileJob::Unzip => format!("Unpacking {name} -- I'll tell you when it's done."),
            }
        } else {
            "The background work queue is full, so that file was left unopened. Ask me again when a worker is free.".into()
        }
    }

    /// What a file errand came to, said -- and a file the scan couldn't
    /// check asked about, to be opened only on a yes.
    pub(super) fn file_done(&mut self, done: FileDone) -> String {
        match done {
            FileDone::Said { said, log, .. } => {
                if let Some(l) = log {
                    self.log.info(&l);
                }
                said
            }
            FileDone::Ask { what, path, question } => {
                self.session.ask(&question);
                self.pending_unscanned = Some((what, path));
                question
            }
        }
    }

    /// Read a PDF or Word file, after scanning it, here and now (the tray's
    /// reading, which is already its own step). `Err` is the sentence.
    pub(super) fn read_document_at(&mut self, path: &str, anyway: bool) -> std::result::Result<String, String> {
        let tools = self.tools_cfg();
        match read_document_off(path, anyway, &tools) {
            FileDone::Said { said, ok, log } => {
                if let Some(l) = log {
                    self.log.info(&l);
                }
                if ok {
                    Ok(said)
                } else {
                    Err(said)
                }
            }
            ask => Err(self.file_done(ask)),
        }
    }
}

impl<'a> Daemon<'a> {
    pub(super) fn drop_or_bring_back(&mut self, said: &str, t: u64) -> String {
        let l = said.to_lowercase();
        let bringing = ["bring back", "what did i drop", "what have i dropped", "pick back up", "pick up what", "un drop"]
            .iter()
            .any(|p| l.contains(p));
        if bringing {
            return self.what_was_dropped(said);
        }
        // "drop the task X", "I'm not doing X", "take X off my list".
        let what = words_after(
            &l,
            &["drop the task", "i'm not doing", "im not doing", "take off my list", "take it off my list", "take"],
        )
        .replace("off my list", "")
        .trim()
        .to_string();
        let words: Vec<&str> = what.split_whitespace().filter(|w| w.len() > 2).collect();
        // Looked for across everything the Outstanding page can take off,
        // not only the backlog, and taken off the one way the page's buttons
        // do it (2 Oct 2026), which keeps a dropped backlog item findable.
        let found = self
            .outstanding_removable(t)
            .into_iter()
            .map(|(key, title)| (words.iter().filter(|w| title.to_lowercase().contains(**w)).count(), key))
            .filter(|(n, _)| *n > 0)
            .max_by_key(|(n, _)| *n);
        let Some((_, key)) = found else {
            return if what.is_empty() {
                "Which one? Say \"drop the task\" and some of its words.".into()
            } else {
                format!("I can't find \"{what}\" on your list.")
            };
        };
        match self.drop_outstanding(&key, t) {
            Ok(off) if off.can_bring_back && off.unsaved.is_none() => {
                format!("Dropped \"{}\". It's kept — \"bring back what I dropped\" finds it again.", off.title)
            }
            Ok(off) => off.said(),
            Err(why) => why,
        }
    }

    fn what_was_dropped(&mut self, said: &str) -> String {
        if self.dropped.is_empty() {
            return "You haven't dropped anything.".into();
        }
        let asked = words_after(
            &said.to_lowercase(),
            &["bring back what i dropped", "bring back the task", "bring back", "pick up what i dropped", "pick back up", "what did i drop", "what have i dropped", "un drop"],
        );
        let found: Vec<crate::daily::Dropped> = if asked.trim().is_empty() {
            let mut all = self.dropped.clone();
            all.sort_by_key(|b| std::cmp::Reverse(b.when));
            all.truncate(5);
            all
        } else {
            crate::daily::find_dropped(&self.dropped, &asked).into_iter().cloned().collect()
        };
        match found.as_slice() {
            [] => format!("Nothing you dropped matches \"{}\".", asked.trim()),
            [one] => {
                let q = format!("{} Put it back on your list?", crate::daily::picking_back_up(one));
                self.session.ask(&q);
                self.pending_bring_back = Some(one.title.clone());
                q
            }
            many => {
                let names: Vec<String> = many.iter().map(|d| format!("\"{}\"", d.title)).collect();
                format!("You dropped {}. Say \"bring back the task\" and which one.", names.join(", "))
            }
        }
    }

    pub(super) fn bring_back(&mut self, title: &str, t: u64) -> String {
        self.dropped.retain(|d| d.title != title);
        let _ = self.store.save("dropped", &self.dropped);
        self.backlog.record(title, crate::backlog::Blocker::Unsupported("do that one for you".into()), t);
        let _ = self.backlog.save(&self.store);
        format!("\"{title}\" is back on your list.")
    }
}

impl<'a> Daemon<'a> {
    /// A question about something it once knew and let go to make room:
    /// said, with where it came from, rather than nothing.
    pub(super) fn knew_once_help(&self, said: &str) -> Option<String> {
        let l = said.trim().to_lowercase();
        let asking = said.contains('?')
            || ["what", "who", "when", "where", "which", "how", "why", "do you know", "tell me about", "remind me what"]
                .iter()
                .any(|w| l.starts_with(w));
        if !asking {
            return None;
        }
        crate::consolidate::once_knew(&self.stones, said).map(crate::consolidate::knew_once)
    }
}

impl<'a> Daemon<'a> {
    /// Locked or screens-off is "up but not here", not asleep (H13a).
    pub(super) fn note_running_state(&mut self) {
        let locked = self.plat.session_locked().unwrap_or(false);
        // Only the lid decides "asleep", and nothing reads the lid yet; the
        // battery doesn't enter into it, so it isn't read on every tick.
        let power = crate::awake::Power {
            on_battery: false,
            battery_pct: 100,
            lid_closed: false,
            lid_action: crate::awake::LidAction::Unknown,
            external_display: false,
        };
        let now = crate::awake::running_state(&power, locked, false);
        if now != self.running {
            if now.work_continues() && now != crate::awake::Running::Awake {
                self.log.info("locked — carrying on with the work in hand");
            }
            self.running = now;
        }
    }

    /// Where the machine is: awake, locked but working, or asleep.
    pub fn running(&self) -> crate::awake::Running {
        self.running
    }

    /// "Stop suggesting the morning backup", "what do you suggest on your
    /// own" (H13b).
    pub(super) fn suggestions(&mut self, said: &str) -> String {
        let l = said.to_lowercase();
        let off = ["stop suggesting", "dont suggest", "don't suggest", "turn off the suggestion"].iter().find(|p| l.contains(*p));
        let on = ["start suggesting", "suggest again", "turn on the suggestion"].iter().find(|p| l.contains(*p));
        let listing = off.is_none() && on.is_none();
        if listing || self.anticipator.rules.is_empty() {
            if self.anticipator.rules.is_empty() {
                return "I don't make any suggestions of my own yet.".into();
            }
            let each: Vec<String> = self
                .anticipator
                .rules
                .iter()
                .map(|r| format!("{} ({})", r.name, if r.enabled { "on" } else { "off" }))
                .collect();
            return format!("My suggestions: {}. Say \"stop suggesting\" and the name to turn one off.", each.join(", "));
        }
        let phrase = off.or(on).copied().unwrap_or("");
        let named = words_after(&l, &[phrase]).trim_start_matches("the ").to_string();
        let words: Vec<&str> = named.split_whitespace().filter(|w| w.len() > 2).collect();
        let found = self
            .anticipator
            .rules
            .iter()
            .map(|r| (words.iter().filter(|w| r.name.to_lowercase().contains(**w)).count(), r.name.clone()))
            .filter(|(n, _)| *n > 0)
            .max_by_key(|(n, _)| *n)
            .map(|(_, name)| name);
        let Some(name) = found else {
            return format!("I don't have a suggestion called \"{named}\".");
        };
        let turn_on = on.is_some();
        self.anticipator.enable(&name, turn_on);
        let _ = self.anticipator.save(&self.store);
        if turn_on {
            format!("I'll suggest {name} again.")
        } else {
            format!("I'll stop suggesting {name}. \"Start suggesting {name}\" brings it back.")
        }
    }

    /// Notes that point at things that don't exist yet (H13d).
    pub(super) fn dangling_notes(&self) -> String {
        let d = self.facts.dangling();
        if d.is_empty() {
            return "Every note that points somewhere points at something that exists.".into();
        }
        let each: Vec<String> = d.iter().take(6).map(|(from, to)| format!("{from} points to {to}")).collect();
        let more = d.len().saturating_sub(6);
        format!(
            "{} that don't exist yet: {}{}. Those are worth writing next.",
            if d.len() == 1 { "One note points at something" } else { "Some notes point at things" },
            each.join("; "),
            if more > 0 { format!(", and {more} more") } else { String::new() }
        )
    }

    /// The overnight account, on request (H13j).
    pub(super) fn overnight_account(&self) -> String {
        let detail: String = self.store.load("overnight_detail");
        if detail.trim().is_empty() {
            "I haven't worked overnight yet.".into()
        } else {
            detail
        }
    }

    /// A turn heard through the current microphone, counted for or against
    /// it, so the ear that actually understands you wins (H13e).
    pub(super) fn heard_through_this_ear(&mut self, said: &str) {
        // The turn knows it came by voice: a correction of it may be a
        // mishearing (`learning`, 2 Oct 2026).
        self.heard_by_voice(said);
        let understood = !matches!(self.parser.parse(said), Intent::Unknown(_)) || said.split_whitespace().count() >= 3;
        if let Some(line) = self.note_how_well_i_heard(understood) {
            self.heard_note = Some(line);
        }
        // By the microphone's own name: `mic_device` holds what ffmpeg opens,
        // which on Windows is now the device's id (29 Sep 2026), and the
        // record of which microphone understands you is kept by name.
        let mic = self.tools_ref().map(|t| crate::voice::microphone_now(t).0).unwrap_or_default();
        if mic.is_empty() {
            return;
        }
        let store = crate::roots::store();
        let mut h = crate::hearing::Hearing::load_from(&store);
        h.record_turn(&crate::hearing::Ear::Desk(mic.clone()), understood);
        crate::kept!(h.save_to(&store));
        // A voice that only just gets through: said plainly, once a day, and
        // Windows' input level raised once if it's what is low (30 Sep 2026:
        // "I feel like I have to yell").
        let mut changes: crate::miclevel::Changes = store.load(crate::miclevel::Changes::RECORD);
        if let Some(line) = crate::miclevel::after_a_turn(&mic, &crate::leveller::remembered(), &mut changes, crate::store::now()) {
            let _ = store.save(crate::miclevel::Changes::RECORD, &changes);
            self.log.info(&line);
            self.heard_note = Some(line);
        }
    }

    /// Your edit of Atlas's words, learned from (H13g).
    pub(super) fn learned_from_your_edit(&mut self, before: &str, after: &str, t: u64) {
        if before.trim().is_empty() {
            return;
        }
        if let Some((what, kind)) = crate::person::learn_from_edit(before, after) {
            self.person.notice(&what, kind, t);
            let _ = self.person.save(&self.store);
        }
    }
}

/// How long you've been quiet before a summary may use the talking model
/// (when the deep one isn't up).
pub const FOLD_ON_TALK_MODEL_AFTER_SECS: u64 = 20 * 60;

impl<'a> Daemon<'a> {
    /// Fold old conversation when it's due (H12).
    pub(super) fn fold_if_due(&mut self, t: u64) {
        // Not while you're talking (29 Sep 2026). The summary is a second
        // call on the same model: on Eric's laptop one ran for eight minutes
        // beside the conversation, every turn meanwhile shared the graphics
        // with it and took 26-58 seconds. It waits for a quiet stretch.
        let talking = self.llm.is_some() && (self.pending_turn.is_some() || t.saturating_sub(self.thread.last_active) < FOLD_WHEN_QUIET_SECS);
        if self.thread.needs_folding(&self.thread_cfg()) && !self.folding && !talking {
            // Compressed rather than dropped (H12): summarised by the local
            // model off the turn, with anything important it leaves out
            // added back; without a model, what it was about and the
            // important lines, which is still more than a count.
            let cfg = self.thread_cfg();
            let old: Vec<crate::thread::Exchange> = self.thread.foldable(&cfg).to_vec();
            let n = old.len();
            let earlier = self.thread.summary.clone();
            // The running summary is background work: the deep model's. When
            // the deep model isn't up it would run on the talking model, and
            // on Eric's laptop that ran 4-13 minutes beside the conversation
            // while every turn waited 25-130 s (30 Sep 2026 logs): then it's
            // the plain summary unless you've been away a good while.
            let deep_up = matches!(self.deep.gate.state(), crate::deepbrain::State::Up | crate::deepbrain::State::Starting);
            let long_quiet = t.saturating_sub(self.thread.last_active) >= FOLD_ON_TALK_MODEL_AFTER_SECS;
            let llm = if deep_up || long_quiet { self.background_llm() } else { None };
            match llm {
                Some(llm) => {
                    let input = self.thread.fold_input(&cfg);
                    let work: crew::Work = Box::new(move |_ctl| {
                        let summary = llm.complete(crate::thread::FOLD_PROMPT, &input).map_err(|e| e.to_string())?;
                        // Checked before it is kept: no reply boilerplate,
                        // no commentary, else the plain summary.
                        let kept = crate::thread::accepted_summary(&summary, &old, &earlier);
                        serde_json::to_string(&(n, kept)).map_err(|e| e.to_string())
                    });
                    if self.hand_off("fold", t, work, None, SpeakPolicy::ViaWatcher) {
                        self.folding = true;
                    }
                }
                None => {
                    let s = crate::thread::plain_fold(&earlier, &old);
                    self.thread.fold_first(n, &s);
                }
            }
        }
    }
}

impl<'a> Daemon<'a> {
    pub(super) fn creator_advice(&mut self, said: &str) -> String {
        if !self.tools_cfg().editcraft.enabled {
            return "Video and creator advice is switched off in your settings.".into();
        }
        if let Some((kind, topic)) = crate::content::creator_request(said) {
            if kind == crate::content::CreatorAsk::Research && said.to_ascii_lowercase().contains("video research online for") {
                return self.research(&format!("Videos about {topic}: cite real sources and observation dates; distinguish observed views/retention from unknowns and predictions."));
            }
            let source = self.store.root().join(crate::social::snapshots::FILE);
            let t = crate::store::now();
            let model = self.creator_llm();
            let work: crew::Work = Box::new(move |ctl| {
                if ctl.checkpoint() { return Err("stopped".into()); }
                let (book, cache_note) = match crate::content::creator_evidence(&source) {
                    Ok(book) => (book, "Saved evidence read within the planning limit.".to_string()),
                    Err(error) => (crate::social::snapshots::Book::default(), format!("Cached evidence not used: {error}. Observations remain unknown.")),
                };
                if ctl.checkpoint() { return Err("stopped".into()); }
                let mut model_note = "No structured background model is available; these are limited offline planning prompts.".to_string();
                let mut proposed = None;
                if kind != crate::content::CreatorAsk::Research {
                    if let Some(model) = model.as_deref().filter(|m| m.supports_bounded_chat()) {
                        let request = crate::content::creator_model_request(kind, &topic, &book, t);
                        let began = std::time::Instant::now();
                        let count = std::cell::Cell::new(0usize);
                        let keep = || !ctl.stopping() && began.elapsed().as_secs() < 45 && count.get() <= 16_000;
                        let reply = model.chat_until(&request, &mut |piece| { count.set(count.get().saturating_add(piece.len())); keep() }, &keep);
                        if ctl.stopping() { return Err("stopped".into()); }
                        proposed = reply.ok().filter(|r| r.tool_calls.is_empty() && keep()).and_then(|r| crate::content::creator_model_output(kind, &topic, &book, &r.text).ok());
                        model_note = if proposed.is_some() { "Structured model suggestions; supplied source IDs checked, factual wording still needs review.".into() }
                            else { "The model did not return a valid proposal within this request's limits; these are limited offline planning prompts.".into() };
                    }
                }
                let ready = proposed.is_some() || (kind == crate::content::CreatorAsk::Research && crate::content::creator_has_evidence(&topic, &book));
                let answer = if let Some(proposed) = proposed {
                    format!("{proposed}\nObserved evidence, separate from suggestions:\n{}", crate::content::creator_plan(crate::content::CreatorAsk::Research, &topic, &book, t))
                } else { crate::content::creator_plan(kind, &topic, &book, t) };
                if ctl.checkpoint() { return Err("stopped".into()); }
                crate::content::review_worker_result("creator_review", if ready { "review_ready" } else { "limited" }, format!("{answer}\n{model_note}\n{cache_note}"), None, vec![], if ready { vec![] } else { vec!["More evidence or a valid model proposal is needed to fulfill this request.".into()] })
            });
            return if self.hand_off("creator planning", t, work, Some(said.into()), SpeakPolicy::Always) {
                "Preparing suggestions from the saved video evidence. Nothing will be posted.".into()
            } else { "Background work is full; the planning request has not started.".into() };
        }
        let l = said.to_lowercase();
        // A brand deal: what it actually asks of you.
        if ["deal", "affiliate", "sponsor", "commission"].iter().any(|w| l.contains(w)) {
            let terms = crate::editcraft::terms_from(said);
            let mut s = crate::editcraft::judge_deal(&terms);
            if terms.exclusive {
                s.push(' ');
                s.push_str(crate::editcraft::AFFILIATE_IS_NOT_EXCLUSIVE);
            }
            return s;
        }
        // Colour: the fixed order, and the setup that makes it work.
        if l.contains("grad") || l.contains("colour") || l.contains("color") {
            let setup = crate::grading::recommended_setup();
            let steps: Vec<String> = crate::grading::tree()
                .iter()
                .enumerate()
                .map(|(i, n)| match n.the_mistake() {
                    Some(m) => format!("{}. {} — watch {}; the usual mistake: {m}", i + 1, n.what(), n.watch()),
                    None => format!("{}. {} — watch {}", i + 1, n.what(), n.watch()),
                })
                .collect();
            return format!(
                "Always the same nodes, in this order:\n{}\nSet the project up once: {}, working in {}, delivering in {}, contrast pivot {}.",
                steps.join("\n"),
                setup.science,
                setup.timeline_space,
                setup.output_space,
                setup.contrast_pivot
            );
        }
        // The profile ladder.
        if l.contains("profile") {
            let has = crate::editcraft::rungs_from(said);
            return crate::editcraft::profile_note(&has);
        }
        // Where it's going: export settings and what the format needs.
        let platform = crate::publishing::platform_in(said);
        let format = crate::publishing::format_named(said);
        let mut out = Vec::new();
        if let Some(p) = platform {
            let e = crate::publishing::export_for(p);
            out.push(format!(
                "For {}: {}x{} at {}fps, {} Mbps, {} kbps audio. Best at {}–{} seconds (up to {}). {}.",
                p.name(), e.width, e.height, e.fps, e.bitrate, e.audio_kbps, e.sweet_spot_secs.0, e.sweet_spot_secs.1, e.max_secs, e.note
            ));
        }
        if let Some(f) = format {
            let (lo, hi) = f.length();
            out.push(format!("What it needs: {}. Length: {lo}–{hi} seconds.", f.rules().join("; ")));
        }
        if out.is_empty() {
            "Ask me about grading, a brand deal, your profile, or where a video's going — TikTok, Reels, Shorts, YouTube, X or LinkedIn.".into()
        } else {
            out.join(" ")
        }
    }

    pub(super) fn money_advice(&self, said: &str) -> String {
        let l = said.to_lowercase();
        if l.contains("bank") || l.contains("statement") && (l.contains("login") || l.contains("password") || l.contains("read") || l.contains("get")) {
            let ways: Vec<String> = crate::finance::sources()
                .iter()
                .map(|s| format!("{}{}", s.describe(), if s.needs_credentials() { " (that one means I'd hold your login)" } else { "" }))
                .collect();
            return format!(
                "The ways I could read it, best first: {}. I'd use the first one that works for your bank.",
                ways.join("; ")
            );
        }
        let what = words_after(&l, &["how long should i keep", "how long do i keep", "how long to keep", "keep my", "keep"]);
        format!("{} That's the general rule, not advice for your situation.", crate::ledger::keep_for(&what))
    }
}

#[cfg(test)]
mod creator_daemon_journey_tests {
    use super::*;
    use crate::brain::{ChatReply, ChatRequest, Llm};
    use crate::platform::mock::MockPlatform;
    use crate::proactive::ProactiveConfig;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    struct BoundedProposal(Arc<AtomicUsize>);

    #[cfg(windows)]
    #[test]
    #[ignore = "requires coordinated owned synthetic speech WAV, localhost model and installed video/Whisper tools"]
    fn installed_video_speech_and_configured_model_prepare_actual_daemon_captions_and_title_for_review() {
        let speech = std::path::PathBuf::from(std::env::var("ATLAS_SYNTHETIC_VIDEO_WAV").expect("owned synthetic speech WAV"));
        assert!(speech.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()), "refuse non-disposable speech input");
        let url = std::env::var("ATLAS_REAL_MODEL_URL").expect("coordinated local model");
        let authority = url.strip_prefix("http://").expect("localhost HTTP only").split('/').next().unwrap();
        let (host, port) = authority.rsplit_once(':').unwrap();
        assert!(matches!(host, "127.0.0.1" | "localhost") && port.parse::<u16>().is_ok_and(|p| p != 0));
        assert!(url.ends_with("/v1/chat/completions"));
        let install = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap()).join("Atlas");
        let ffmpeg = crate::tools::ExternalTool { command: install.join("tools/ffmpeg/ffmpeg.exe").display().to_string(), timeout_secs: 15, ..Default::default() };
        let ffprobe = crate::tools::ExternalTool { command: install.join("tools/ffmpeg/ffprobe.exe").display().to_string(), timeout_secs: 15, ..Default::default() };
        let whisper = install.join("tools/whisper/whisper-cli.exe");
        let asr_model = install.join("models/ggml-base.en.bin");
        assert!(whisper.is_file() && asr_model.is_file());
        let root = std::env::current_dir().unwrap().join("scratch").join(format!("atlas-daemon-spoken-video-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("synthetic-spoken.mp4");
        let args = vec!["-hide_banner".into(), "-n".into(), "-f".into(), "lavfi".into(), "-i".into(), "color=c=blue:s=320x240:d=60:r=25".into(), "-i".into(), speech.display().to_string(), "-shortest".into(), "-c:v".into(), "libx264".into(), "-pix_fmt".into(), "yuv420p".into(), "-c:a".into(), "aac".into(), input.display().to_string()];
        crate::studio::run_tool(&ffmpeg, args, &|| false).unwrap();
        let original = std::fs::read(&input).unwrap();
        let mut cfg = Config::load(std::path::Path::new("config")).unwrap();
        let tools = cfg.tools.as_mut().unwrap();
        tools.video.ffmpeg = ffmpeg.clone(); tools.video.ffprobe = ffprobe.clone();
        tools.browser.launch = None; tools.models.online_second = false; tools.llm_secondary = None;
        tools.vars.insert("stt_model".into(), asr_model.display().to_string());
        tools.stt_timed = Some(crate::tools::ExternalTool { command: whisper.display().to_string(), args: ["-m", "{stt_model}", "-f", "{in_wav}", "-osrt", "-of", "{stem}", "-t", "4", "-l", "en"].iter().map(|s| s.to_string()).collect(), result_file: Some("{srt}".into()), timeout_secs: 30, ..Default::default() });
        let mut http = crate::models::server_post();
        http.args = http.args.iter().map(|arg| arg.replace("{url}", &url)).collect();
        tools.llm = Some(crate::brain::LlmConfig { tool: http, request: "{}".into(), response_path: "content".into(), vision_request: None });
        let factory = crate::models::connection(tools).unwrap();
        assert!(factory.supports_bounded_chat());
        let captured = Arc::new(std::sync::Mutex::new(String::new()));
        let model = Arc::new(CaptureConfiguredCreator { inner: factory, last: captured.clone() });
        let platform = MockPlatform::new(vec![]);
        let mut d = Daemon::new(&cfg, &platform, Some(model), Store::new(root.join("state")), Proactive::new(ProactiveConfig::default()));
        d.use_deep_brain_for_test(crate::deepbrain::DeepBrain::none());
        d.crew = crew::Crew::new(1).with_room(Box::new(|| crew::Room { free_mb: Some(16_384), on_battery: false, battery_percent: Some(100) }));
        let command = format!("get this video ready: \"{}\"", input.display());
        let t = crate::store::now(); let id = d.queue.push_at(&command, crate::lanes::Lane::Background, t);
        assert!(d.queue.ready_durably(&d.store, &crate::awareness::Signals::default(), &crate::lanes::LaneConfig::default(), t, crate::connectivity::Reach::Offline).unwrap().contains(&id));
        let reply = d.turn(&command, t); let worker = d.last_crew_handoff.expect("actual studio worker");
        d.queue.attach_worker(id, worker, &reply); d.queue.save(&d.store).unwrap();
        let began = std::time::Instant::now(); let mut output = Vec::new();
        while d.queue.tasks.iter().find(|task| task.id == id).unwrap().state == crate::lanes::TaskState::Running && began.elapsed() < std::time::Duration::from_secs(60) {
            output.extend(d.take_crew_news(t + 1)); std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if d.queue.tasks.iter().find(|task| task.id == id).unwrap().state == crate::lanes::TaskState::Running {
            d.stop_linked_worker(worker, t + 2);
            let drain = std::time::Instant::now();
            while d.crew.in_hand(worker) && drain.elapsed() < std::time::Duration::from_secs(5) { output.extend(d.take_crew_news(t + 2)); std::thread::sleep(std::time::Duration::from_millis(10)); }
        }
        let task = d.queue.tasks.iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.state, crate::lanes::TaskState::Failed, "review-ready preparation is not publication Done");
        assert!(task.interrupted, "real captions/title must reach NeedsYou: {}; {}", output.join("; "), captured.lock().unwrap());
        let review = root.join("synthetic-spoken - ready");
        let captions = std::fs::read_to_string(review.join("synthetic-spoken - cut.srt")).unwrap();
        let timed = crate::viewing::read_timed(&captions);
        assert!(!timed.is_empty() && timed.iter().map(|s| s.words.split_whitespace().count()).sum::<usize>() >= 5);
        let transcript = timed.iter().map(|s| s.words.clone()).collect::<Vec<_>>().join(" ");
        let title = std::fs::read_to_string(review.join("title and description.txt")).unwrap();
        assert!(title.contains("Suggested wording; review against the transcript"));
        assert!(crate::studio::title_and_description(&captured.lock().unwrap().split("synthetic reply: ").last().unwrap_or(""), &transcript).is_some(), "actual model title must pass the unchanged grounding check");
        let rendered = review.join("synthetic-spoken - cut.mp4");
        let probed = crate::studio::run_tool(&ffprobe, crate::edit::probe_args(&rendered.display().to_string()), &|| false).unwrap();
        let json = String::from_utf8_lossy(&probed.stdout); let (duration, audio) = crate::studio::probe(&json).unwrap();
        assert!(duration > 1.0 && duration < 60.0 && audio);
        assert_eq!(crate::studio::dimensions(&json), Some((320, 240)));
        assert!(review.join("thumbnail-01.jpg").metadata().unwrap().len() > 0);
        assert!(std::fs::read_to_string(review.join("review.txt")).unwrap().contains("Captions saved: true"));
        assert!(d.long_work.jobs.iter().any(|job| job.outcome == crate::watching::Outcome::Finished));
        assert!(d.publisher.posts.is_empty()); assert_eq!(std::fs::read(&input).unwrap(), original);
        let restarted = crate::lanes::Queue::load_checked(&d.store).unwrap();
        assert!(restarted.tasks.iter().find(|task| task.id == id).unwrap().interrupted && restarted.pending() == 0);
        drop(d);
        println!("SYNTHETIC VIDEO REVIEW PROOF: {}", review.display());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires coordinated installed ffmpeg/ffprobe CPU lane; creates only disposable silent footage"]
    fn installed_video_tools_run_the_actual_daemon_studio_route_and_report_partial_work() {
        let proof_started = std::time::Instant::now();
        let tools = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap()).join("Atlas/tools/ffmpeg");
        let ffmpeg = crate::tools::ExternalTool { command: tools.join("ffmpeg.exe").display().to_string(), timeout_secs: 15, ..Default::default() };
        let ffprobe = crate::tools::ExternalTool { command: tools.join("ffprobe.exe").display().to_string(), timeout_secs: 15, ..Default::default() };
        assert!(std::path::Path::new(&ffmpeg.command).is_file() && std::path::Path::new(&ffprobe.command).is_file());
        let root = std::env::temp_dir().join(format!("atlas-daemon-video-proof-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("synthetic-camera.mp4");
        let args = ["-hide_banner", "-n", "-f", "lavfi", "-i", "color=c=blue:s=320x240:d=2:r=25", "-c:v", "libx264", "-pix_fmt", "yuv420p"].iter().map(|s| s.to_string()).chain([input.display().to_string()]).collect();
        crate::studio::run_tool(&ffmpeg, args, &|| false).unwrap();
        let original = std::fs::read(&input).unwrap();
        let mut cfg = Config::load(std::path::Path::new("config")).unwrap();
        let configured = cfg.tools.as_mut().unwrap();
        configured.video.ffmpeg = ffmpeg.clone(); configured.video.ffprobe = ffprobe.clone();
        configured.browser.launch = None; configured.models.online_second = false;
        configured.stt_timed = None;
        let platform = MockPlatform::new(vec![]);
        let mut d = Daemon::new(&cfg, &platform, None, Store::new(root.join("state")), Proactive::new(ProactiveConfig::default()));
        d.crew = crew::Crew::new(1).with_room(Box::new(|| crew::Room { free_mb: Some(16_384), on_battery: false, battery_percent: Some(100) }));
        let command = format!("get this video ready: \"{}\"", input.display());
        let t = crate::store::now();
        let id = d.queue.push_at(&command, crate::lanes::Lane::Background, t);
        assert!(d.queue.ready_durably(&d.store, &crate::awareness::Signals::default(), &crate::lanes::LaneConfig::default(), t, crate::connectivity::Reach::Offline).unwrap().contains(&id));
        let began = std::time::Instant::now();
        let reply = d.turn(&command, t);
        assert!(reply.contains("Preparing a review copy") && reply.contains("nothing will be posted"), "{reply}");
        let worker = d.last_crew_handoff.expect("actual studio worker");
        d.queue.attach_worker(id, worker, &reply); d.queue.save(&d.store).unwrap();
        let mut output = Vec::new();
        while d.queue.tasks.iter().find(|task| task.id == id).unwrap().state == crate::lanes::TaskState::Running && began.elapsed() < std::time::Duration::from_secs(55) {
            output.extend(d.take_crew_news(t + 1));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if d.queue.tasks.iter().find(|task| task.id == id).unwrap().state == crate::lanes::TaskState::Running { d.stop_linked_worker(worker, t + 2); }
        let task = d.queue.tasks.iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.state, crate::lanes::TaskState::Failed, "silent footage without a title cannot finish the full preparation: {}", output.join("; "));
        assert!(!task.interrupted, "partial work must not be labelled review-ready");
        let review = root.join("synthetic-camera - ready");
        let rendered = review.join("synthetic-camera - cut.mp4");
        let probed = crate::studio::run_tool(&ffprobe, crate::edit::probe_args(&rendered.display().to_string()), &|| false).unwrap();
        let json = String::from_utf8_lossy(&probed.stdout);
        let (duration, audio) = crate::studio::probe(&json).unwrap();
        assert!((duration - 2.0).abs() < 0.2 && !audio);
        assert_eq!(crate::studio::dimensions(&json), Some((320, 240)));
        assert!(review.join("thumbnail-01.jpg").metadata().unwrap().len() > 0);
        let manifest = std::fs::read_to_string(review.join("review.txt")).unwrap();
        assert!(manifest.contains("captions and audio editing were skipped") && manifest.contains("Captions saved: false"));
        assert!(output.iter().any(|text| text.contains("No title suggestion was saved")));
        assert!(d.long_work.jobs.iter().any(|job| job.outcome == crate::watching::Outcome::Failed));
        assert!(d.publisher.posts.is_empty());
        assert_eq!(std::fs::read(&input).unwrap(), original);
        drop(d); std::fs::remove_dir_all(root).unwrap();
        assert!(proof_started.elapsed() < std::time::Duration::from_secs(90));
    }
    struct HeldCreator {
        entered: std::sync::mpsc::Sender<()>,
        released: Arc<std::sync::atomic::AtomicBool>,
    }
    impl Llm for HeldCreator {
        fn complete(&self, _: &str, _: &str) -> crate::error::Result<String> { panic!("creator must remain cancellable") }
        fn supports_bounded_chat(&self) -> bool { true }
        fn chat_until(&self, req: &ChatRequest, _: &mut dyn FnMut(&str) -> bool, keep: &dyn Fn() -> bool) -> crate::error::Result<ChatReply> {
            assert!(req.aside && req.output_schema.is_some());
            self.entered.send(()).unwrap();
            while keep() && !self.released.load(Ordering::SeqCst) { std::thread::sleep(std::time::Duration::from_millis(5)); }
            while !self.released.load(Ordering::SeqCst) { std::thread::sleep(std::time::Duration::from_millis(5)); }
            Err(crate::error::AtlasError::Platform("creator fixture stopped".into()))
        }
    }
    struct ReleaseOnDrop(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for ReleaseOnDrop { fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); } }
    struct CaptureConfiguredCreator {
        inner: Arc<dyn Llm>,
        last: Arc<std::sync::Mutex<String>>,
    }
    impl Llm for CaptureConfiguredCreator {
        fn complete(&self, _: &str, _: &str) -> crate::error::Result<String> { panic!("native creator proof must use bounded chat") }
        fn supports_bounded_chat(&self) -> bool { self.inner.supports_bounded_chat() }
        fn chat_until(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool, keep: &dyn Fn() -> bool) -> crate::error::Result<ChatReply> {
            let result = self.inner.chat_until(req, on_text, keep);
            *self.last.lock().unwrap() = match &result {
                Ok(reply) => format!("schema supplied: {}; synthetic reply: {}", req.output_schema.is_some(), reply.text.chars().take(2000).collect::<String>()),
                Err(error) => format!("bounded configured adapter error: {error}"),
            };
            result
        }
    }

    #[test]
    fn actual_configured_model_factory_preserves_bounded_cancellation_without_transport() {
        let mut cfg = Config::load(std::path::Path::new("config")).unwrap();
        let tools = cfg.tools.as_mut().unwrap();
        tools.models.online_second = false;
        tools.llm_secondary = None;
        let mut http = crate::models::server_post();
        http.args = http.args.iter().map(|arg| arg.replace("{url}", "http://127.0.0.1:1/v1/chat/completions")).collect();
        tools.llm = Some(crate::brain::LlmConfig { tool: http, request: "{}".into(), response_path: "content".into(), vision_request: None });
        let model = crate::models::connection(tools).unwrap();
        assert!(model.supports_bounded_chat());
        let request = crate::content::creator_model_request(crate::content::CreatorAsk::Idea, "sourdough starter", &crate::social::snapshots::Book::default(), 0);
        let began = std::time::Instant::now();
        let mut callback_called = false;
        let result = model.chat_until(&request, &mut |_| { callback_called = true; true }, &|| false);
        assert!(result.is_err());
        assert!(crate::models::chat_was_stopped(result.as_ref().unwrap_err()));
        assert!(!callback_called);
        assert!(began.elapsed() < std::time::Duration::from_secs(1));
    }
    impl Llm for BoundedProposal {
        fn complete(&self, _: &str, _: &str) -> crate::error::Result<String> { panic!("creator must use the bounded production request") }
        fn supports_bounded_chat(&self) -> bool { true }
        fn chat_until(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool, keep: &dyn Fn() -> bool) -> crate::error::Result<ChatReply> {
            assert!(keep());
            assert!(req.aside && req.tools.is_empty());
            assert!(req.messages.iter().any(|m| m.content.contains("sourdough starter")));
            assert_eq!(req.output_schema.as_ref().unwrap()["properties"]["suggestions"]["items"]["properties"]["reference_ids"]["maxItems"], 0);
            self.0.fetch_add(1, Ordering::SeqCst);
            let text = r#"{"suggestions":[{"title":"Compare two starter jars","action":"Film the sourdough starter before feeding and again after rising, keeping the camera position fixed.","reason":"Visible changes give the viewer a concrete comparison.","reference_ids":[]}],"duration_seconds":null}"#;
            assert!(on_text(text) && keep());
            Ok(ChatReply::from_text(text))
        }
    }

    #[test]
    fn actual_creator_worker_requires_review_and_missing_model_never_completes_the_request() {
        for available in [true, false] {
            let mut cfg = Config::load(std::path::Path::new("config")).unwrap();
            cfg.tools.as_mut().unwrap().editcraft.enabled = true;
            cfg.tools.as_mut().unwrap().browser.launch = None;
            let platform = MockPlatform::new(vec![]);
            let root = std::env::temp_dir().join(format!("atlas-creator-daemon-{}-{}-{available}", std::process::id(), crate::store::now()));
            let calls = Arc::new(AtomicUsize::new(0));
            let model = available.then(|| Arc::new(BoundedProposal(calls.clone())) as Arc<dyn Llm>);
            let mut d = Daemon::new(&cfg, &platform, model, Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
            d.crew = crew::Crew::new(1).with_room(Box::new(|| crew::Room { free_mb: Some(16_384), on_battery: false, battery_percent: Some(100) }));
            let command = "video idea for sourdough starter";
            let t = crate::store::now();
            let id = d.queue.push_at(command, crate::lanes::Lane::Background, t);
            assert!(d.queue.ready_durably(&d.store, &crate::awareness::Signals::default(), &crate::lanes::LaneConfig::default(), t, crate::connectivity::Reach::Offline).unwrap().contains(&id));
            let reply = d.turn(command, t);
            assert!(reply.contains("Nothing will be posted"), "{reply}");
            let worker = d.last_crew_handoff.expect("actual creator handoff");
            d.queue.attach_worker(id, worker, &reply);
            assert_eq!(d.queue.tasks.iter().find(|task| task.id == id).unwrap().worker_id, Some(worker));
            d.queue.save(&d.store).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut output = Vec::new();
            while d.queue.tasks.iter().find(|task| task.id == id).unwrap().state == crate::lanes::TaskState::Running && std::time::Instant::now() < deadline {
                output.extend(d.take_crew_news(t + 1));
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let task = d.queue.tasks.iter().find(|task| task.id == id).unwrap();
            assert_eq!(task.state, crate::lanes::TaskState::Failed, "review or limited capability must not mark the whole requested journey Done: {}", output.join("; "));
            assert_eq!(task.interrupted, available, "only a valid prepared proposal is waiting for owner review");
            assert_eq!(calls.load(Ordering::SeqCst), usize::from(available));
            assert!(d.long_work.jobs.iter().any(|job| job.outcome == if available { crate::watching::Outcome::Finished } else { crate::watching::Outcome::Failed }));
            drop(d);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn outstanding_cancels_the_exact_creator_worker_and_restart_does_not_replay_it() {
        let mut cfg = Config::load(std::path::Path::new("config")).unwrap();
        cfg.tools.as_mut().unwrap().editcraft.enabled = true;
        cfg.tools.as_mut().unwrap().browser.launch = None;
        let platform = MockPlatform::new(vec![]);
        let root = std::env::temp_dir().join(format!("atlas-creator-cancel-{}-{}", std::process::id(), crate::store::now()));
        let released = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (entered, receiver) = std::sync::mpsc::channel();
        let model = Arc::new(HeldCreator { entered, released: released.clone() });
        let mut d = Daemon::new(&cfg, &platform, Some(model), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
        // Drop before Daemon on every assertion failure so its worker can wind down.
        let release_guard = ReleaseOnDrop(released.clone());
        d.crew = crew::Crew::new(1).with_room(Box::new(|| crew::Room { free_mb: Some(16_384), on_battery: false, battery_percent: Some(100) }));
        let command = "video idea for sourdough starter";
        let t = crate::store::now();
        let id = d.queue.push_at(command, crate::lanes::Lane::Background, t);
        assert!(d.queue.ready_durably(&d.store, &crate::awareness::Signals::default(), &crate::lanes::LaneConfig::default(), t, crate::connectivity::Reach::Offline).unwrap().contains(&id));
        let reply = d.turn(command, t);
        let worker = d.last_crew_handoff.expect("actual creator worker");
        d.queue.attach_worker(id, worker, &reply);
        d.queue.save(&d.store).unwrap();
        receiver.recv_timeout(std::time::Duration::from_secs(2)).expect("bounded creator model entered");
        let cancelled = d.drop_outstanding(&format!("t:{id}"), t + 1).unwrap();
        assert!(cancelled.stopping && cancelled.unsaved.is_none());
        let pending: crate::lanes::Queue = d.store.load_checked("queue").unwrap().unwrap();
        let task = pending.tasks.iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.state, crate::lanes::TaskState::Running);
        assert!(task.stop_requested);
        assert_eq!(task.worker_id, Some(worker));
        released.store(true, Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while d.queue.tasks.iter().find(|task| task.id == id).unwrap().state == crate::lanes::TaskState::Running && std::time::Instant::now() < deadline {
            d.take_crew_news(t + 2);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let restarted = crate::lanes::Queue::load_checked(&d.store).unwrap();
        let task = restarted.tasks.iter().find(|task| task.id == id).unwrap();
        assert_eq!(task.state, crate::lanes::TaskState::Failed);
        assert!(task.result.as_ref().is_some_and(|text| text.contains("stopped")));
        assert!(d.long_work.jobs.iter().any(|job| job.outcome == crate::watching::Outcome::Failed));
        assert_eq!(restarted.pending(), 0);
        drop(release_guard);
        drop(d);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires the parent's coordinated owned localhost model server"]
    fn configured_local_model_runs_the_actual_daemon_creator_route_without_publication() {
        let url = std::env::var("ATLAS_REAL_MODEL_URL").expect("coordinated local chat endpoint");
        let authority = url.strip_prefix("http://").expect("local HTTP only").split('/').next().unwrap();
        let (host, port) = authority.rsplit_once(':').expect("explicit localhost port");
        assert!(matches!(host, "127.0.0.1" | "localhost"));
        assert!(port.parse::<u16>().is_ok_and(|port| port != 0));
        assert!(url.ends_with("/v1/chat/completions"));
        let mut cfg = Config::load(std::path::Path::new("config")).unwrap();
        let tools = cfg.tools.as_mut().unwrap();
        tools.editcraft.enabled = true;
        tools.browser.launch = None;
        tools.models.online_second = false;
        tools.llm_secondary = None;
        let mut http = crate::models::server_post();
        http.args = http.args.iter().map(|arg| arg.replace("{url}", &url)).collect();
        tools.llm = Some(crate::brain::LlmConfig { tool: http, request: "{}".into(), response_path: "content".into(), vision_request: None });
        let model = crate::models::connection(tools).expect("production configured-primary factory");
        assert!(model.supports_bounded_chat(), "production wrappers must preserve the bounded local interface");
        let captured = Arc::new(std::sync::Mutex::new(String::new()));
        let model = Arc::new(CaptureConfiguredCreator { inner: model, last: captured.clone() });
        let platform = MockPlatform::new(vec![]);
        let root = std::env::temp_dir().join(format!("atlas-native-creator-daemon-{}-{}", std::process::id(), crate::store::now()));
        let mut d = Daemon::new(&cfg, &platform, Some(model), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
        d.crew = crew::Crew::new(1).with_room(Box::new(|| crew::Room { free_mb: Some(16_384), on_battery: false, battery_percent: Some(100) }));
        for command in ["video idea for sourdough starter troubleshooting for beginners", "video structure for sourdough starter troubleshooting for beginners 60 seconds", "editing advice for sourdough starter troubleshooting for beginners"] {
            let t = crate::store::now();
            let id = d.queue.push_at(command, crate::lanes::Lane::Background, t);
            assert!(d.queue.ready_durably(&d.store, &crate::awareness::Signals::default(), &crate::lanes::LaneConfig::default(), t, crate::connectivity::Reach::Offline).unwrap().contains(&id));
            let began = std::time::Instant::now();
            let reply = d.turn(command, t);
            assert!(reply.contains("Nothing will be posted"), "{reply}");
            let worker = d.last_crew_handoff.expect("actual creator worker");
            d.queue.attach_worker(id, worker, &reply);
            d.queue.save(&d.store).unwrap();
            let mut output = Vec::new();
            while d.queue.tasks.iter().find(|task| task.id == id).unwrap().state == crate::lanes::TaskState::Running && began.elapsed() < std::time::Duration::from_secs(45) {
                output.extend(d.take_crew_news(t + 1));
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let task = d.queue.tasks.iter().find(|task| task.id == id).unwrap();
            assert!(began.elapsed() < std::time::Duration::from_secs(45), "actual configured creator route exceeded its unchanged budget");
            assert_eq!(task.state, crate::lanes::TaskState::Failed, "preparation alone cannot complete publication");
            assert!(task.interrupted, "a real valid proposal must reach NeedsYou, not limited-model failure: {}; {}", output.join("; "), captured.lock().unwrap());
            assert!(output.iter().any(|line| line.contains("originality unverified")));
            assert!(d.publisher.posts.is_empty());
            let restarted = crate::lanes::Queue::load_checked(&d.store).unwrap();
            assert!(restarted.tasks.iter().find(|task| task.id == id).unwrap().interrupted);
            assert_eq!(restarted.pending(), 0);
        }
        drop(d);
        std::fs::remove_dir_all(root).unwrap();
    }
}

impl<'a> Daemon<'a> {
    pub(super) fn teach_gesture(&mut self, said: &str, t: u64) -> String {
        let Some((name, does)) = gesture_asked(said) else {
            return "What's it called and what should it do? Say \"teach you a gesture called thumbs up that opens spotify\".".into();
        };
        if does.is_empty() {
            return format!("What should {name} do?");
        }
        if !self.tools_cfg().gaze.enabled {
            return "The camera's switched off — turn on Watching the room in settings and I'll learn it.".into();
        }
        // The camera can't watch for gestures and learn one at once.
        if let Some(mut running) = self.hands.take() {
            running.stop();
        }
        let models = std::path::Path::new(&self.tools_cfg().models.dir).to_path_buf();
        let missing = crate::infer::whats_missing(&models, &crate::infer::Kind::for_hands());
        if !missing.is_empty() {
            return crate::infer::spoken(&missing);
        }
        let plan = self.hands_plan();
        // The kept camera steps aside: one reader on a camera at a time.
        self.let_go_of_the_camera("hand tracking takes the camera");
        let Some(mut eyes) = self.build_eyes(&models, &plan) else {
            return "I couldn't start the camera.".into();
        };
        let (n, d) = (name.clone(), does.clone());
        let work: crew::Work = Box::new(move |_ctl| {
            // Twenty readings, and up to about ten seconds of looking.
            let shown = crate::handloop::shape_shown(eyes.as_mut(), &n, &d, 300);
            serde_json::to_string(&shown).map_err(|e| e.to_string())
        });
        if self.hand_off("teach-gesture", t, work, Some(name.clone()), SpeakPolicy::Always) {
            format!("Hold the shape for {name} up to the camera and keep it still for a couple of seconds.")
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    pub(super) fn gesture_news(&mut self, ending: &crew::Ending) -> Option<String> {
        let crew::Ending::Done(Ok(json)) = ending else {
            return Some("I couldn't watch for the gesture.".into());
        };
        let shown: std::result::Result<crate::handshape::Gesture, String> = serde_json::from_str(json).ok()?;
        Some(match shown {
            Ok(g) => match self.gestures.adopt(g) {
                Ok(said) => {
                    let _ = self.gestures.save(&self.store);
                    said
                }
                Err(why) => why,
            },
            Err(why) => why,
        })
    }

    /// Which languages Atlas can hear, from the speech model it has (H5).
    pub(super) fn languages_heard(&self) -> String {
        let tools = self.tools_cfg();
        let model = tools.vars.get("stt_model").cloned().unwrap_or_default();
        let facts = crate::language::model_facts(&model);
        let cfg = &tools.language;
        if facts.english_only || !cfg.multilingual {
            let swap = if facts.english_only {
                format!(" {} is English-only — the multilingual one of the same size hears ninety-odd languages and costs no more memory.", facts.name)
            } else {
                String::new()
            };
            return format!("Only English right now.{swap} Turn on other languages in settings and I'll {}.",
                if cfg.translate_others { "bring them back in English" } else { "write them down as they're said" });
        }
        format!(
            "Any language Whisper knows — ninety-odd. {}",
            if cfg.translate_others { "Anything that isn't yours comes back in English, and call notes keep both." } else { "I write each down as it's said." }
        )
    }

    /// Mishearing you, measured: a bigger speech model is suggested once
    /// when the evidence says so (H5).
    fn note_how_well_i_heard(&mut self, understood: bool) -> Option<String> {
        let mut l: crate::language::Listening = self.store.load("listening");
        l.record(if understood { 0.9 } else { 0.3 });
        let tools = self.tools_cfg();
        let model = tools.vars.get("stt_model").cloned().unwrap_or_default();
        let said = l.suggestion(&model, &tools.language);
        if said.is_some() {
            l.suggested_at = Some(crate::store::now());
        }
        let _ = self.store.save("listening", &l);
        said
    }
}

impl<'a> Daemon<'a> {
    pub(super) fn set_key(&mut self, said: &str) -> String {
        let l = said.to_lowercase();
        let tools = self.tools_cfg();
        let (talk, typing) = (tools.push_to_talk.key.clone(), tools.quick_input.hotkey.clone());
        let Some(i) = l.rfind(" to ") else {
            return format!(
                "Hold {} to talk, and press {} to type. Say \"set my typing key to\" and the keys to change one, or set it by pressing it in Settings.",
                crate::settingswin::pretty_key(&talk),
                crate::settingswin::pretty_key(&typing)
            );
        };
        let which = if l.contains("talk") { "push_to_talk.key" } else { "quick_input.hotkey" };
        let spec = key_spoken(&l[i + 4..]);
        match crate::settingswin::keep_setting(&crate::roots::config_dir(), which, &spec) {
            Ok(_) => format!(
                "Done — {} is now {}. It takes hold when Atlas restarts.",
                if which == "push_to_talk.key" { "push-to-talk" } else { "the typing box" },
                crate::settingswin::pretty_key(&spec)
            ),
            Err(why) => format!("I've left it as it was: {why}."),
        }
    }

    /// A photo, or a folder of them, edited on a new copy (`photo`).
    pub(super) fn edit_photo(&mut self, said: &str) -> String {
        // The clipboard only when the words point at it, as "explain this" does.
        let copied = if crate::clipboard::refers_to_clipboard(said) { self.clipboard_text.clone().or_else(|| self.plat.read_clipboard().ok().flatten()) } else { None };
        let handed = crate::photo::which_photo(said, copied.as_deref(), self.file_meant(said, crate::photo::PHOTO_EXTS));
        let setup = crate::photo::Setup::here(&self.tools_cfg().video.ffmpeg.command, self.store.root());
        match crate::photo::ask(said, handed, setup) {
            crate::photo::Plan::Now(answer) => answer,
            crate::photo::Plan::Later { start, work } => {
                let work: crew::Work = Box::new(move |c: &crew::Control| Ok(work(&|| c.stopping())));
                if self.hand_off("photo", crate::store::now(), work, Some(said.to_string()), SpeakPolicy::Always) { start } else { "I'm swamped with background work right now — ask me again in a moment.".into() }
            }
        }
    }
}

/// What you said to put on the later list, in your own words: "add call the
/// bank to my later list" gives "call the bank". `None` when the words are
/// only "that"/"this"/"it" (Atlas's last reply is meant) or there are none.
fn later_own_words(said: &str) -> Option<String> {
    let low = said.trim().trim_end_matches(['.', '!', '?']).to_lowercase();
    let mut t = low.as_str();
    for lead in ["please ", "can you ", "could you ", "add ", "put ", "save "] {
        t = t.strip_prefix(lead).unwrap_or(t);
    }
    let cut = [" to the later list", " to my later list", " on the later list", " on my later list", " for later", " to later"]
        .iter()
        .filter_map(|p| t.find(p))
        .min()?;
    let own = t[..cut].trim();
    if own.is_empty() || ["that", "this", "it", "that one", "this one"].contains(&own) {
        return None;
    }
    Some(own.to_string())
}

/// What's said in a video, from the local timed transcriber, or empty when
/// there is none or it can't be read. The sound is a scratch copy in
/// `folder`, never kept whatever the recordings setting says: the guard
/// drops it and the transcriber's own file on every way out. `keep_srt`
/// keeps the captions there when given (the studio does; watching doesn't).
pub(super) fn transcript_of(
    video: &crate::voice::VideoConfig,
    timed: Option<&crate::tools::ExternalTool>,
    vars: &crate::tools::Vars,
    src: &str,
    folder: &std::path::Path,
    keep_srt: Option<&std::path::Path>,
) -> String {
    transcript_checked(video, timed, vars, src, folder, keep_srt, &|| false).unwrap_or_default()
}

fn transcript_checked(
    video: &crate::voice::VideoConfig,
    timed: Option<&crate::tools::ExternalTool>,
    vars: &crate::tools::Vars,
    src: &str,
    folder: &std::path::Path,
    keep_srt: Option<&std::path::Path>,
    stop: &dyn Fn() -> bool,
) -> std::result::Result<String, String> {
    let timed = timed.ok_or("no local transcriber is configured")?;
    let s = |p: &std::path::Path| p.display().to_string();
    let wav = folder.join("sound.wav");
    let scratch = crate::retention::RetentionConfig { delete_audio_after_transcribing: true, ..Default::default() };
    let mut sound = crate::retention::Recording::new(&wav, &scratch);
    sound.and_also(&wav.with_extension("srt"));
    crate::studio::run_tool(&video.ffmpeg, crate::studio::audio_args(src, &s(&wav)), stop)?;
    let mut v = vars.clone();
    let stem_path = wav.with_extension("");
    v.insert("in_wav".into(), s(&wav));
    v.insert("stem".into(), s(&stem_path));
    v.insert("srt".into(), format!("{}.srt", s(&stem_path)));
    v.entry("task_opt".into()).or_default();
    v.entry("lang_opt".into()).or_default();
    v.entry("lang_val".into()).or_default();
    let mut tool = timed.clone();
    tool.timeout_secs = tool.timeout_secs.max(crate::callnotes::transcribe_timeout_secs(&wav));
    let srt = tool.run_stoppable(&v, None, stop).map_err(|e| e.to_string())?.ok_or("stopped")?;
    let words = crate::viewing::read_timed(&srt).iter().map(|x| x.words.clone()).collect::<Vec<_>>().join(" ");
    if words.trim().is_empty() { return Err("the transcriber returned no usable timed words".into()); }
    if let Some(keep) = keep_srt {
        std::fs::write(keep, &srt).map_err(|e| format!("couldn't save captions: {e}"))?;
    }
    Ok(words)
}

#[cfg(test)]
mod publication_and_signup_durability {
    use super::*;
    use crate::{config::Config, platform::mock::MockPlatform, proactive::{Proactive, ProactiveConfig}, store::Store};
    #[test]
    fn backup_contention_never_claims_a_post_was_scheduled_or_cancelled_durably() {
        let root = std::env::temp_dir().join(format!("atlas-post-busy-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = Store::new(root.clone());
        let cfg = Config::load(std::path::Path::new("config")).unwrap(); let platform = MockPlatform::new(vec![]);
        let mut daemon = Daemon::new(&cfg, &platform, None, store.clone(), Proactive::new(ProactiveConfig::default()));
        let id = daemon.publisher.draft(crate::publish::Channel::X, "reviewed words"); daemon.publisher.save(&store).unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel(); let (release_tx, release_rx) = std::sync::mpsc::channel();
        let held = std::thread::spawn(move || { let _guard = crate::store::state_transaction(&root).unwrap(); ready_tx.send(()).unwrap(); release_rx.recv().unwrap(); }); ready_rx.recv().unwrap();
        let schedule = daemon.schedule_post_at(id, "now", 1000);
        let scheduled_state = daemon.publisher.get(id).unwrap().state;
        let cancelled = daemon.schedule_post("cancel the post", 1001);
        let cancelled_state = daemon.publisher.get(id).unwrap().state;
        release_tx.send(()).unwrap(); held.join().unwrap();
        assert!(schedule.contains("couldn't be saved"), "{schedule}"); assert_eq!(scheduled_state, crate::publish::PostState::Draft);
        assert!(cancelled.contains("cancellation couldn't be saved"), "{cancelled}"); assert_eq!(cancelled_state, crate::publish::PostState::Cancelled);
        assert_eq!(crate::publish::Publisher::load(&store).get(id).unwrap().state, crate::publish::PostState::Draft);
    }
    #[cfg(windows)]
    #[test]
    fn unsaved_signup_password_or_access_never_starts_a_browser_worker() {
        for blocked in ["vault", crate::signin::Access::RECORD] {
            let root = std::env::temp_dir().join(format!("atlas-signup-save-{blocked}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
            let store = Store::new(root.clone()); let mut cfg = Config::load(std::path::Path::new("config")).unwrap();
            cfg.tools.as_mut().unwrap().mail.accounts = vec![crate::mail::Account { name: "fixture".into(), address: "fixture@example.test".into(), ..Default::default() }];
            cfg.tools.as_mut().unwrap().browser.launch = None;
            let platform = MockPlatform::new(vec![]); let mut daemon = Daemon::new(&cfg, &platform, None, store.clone(), Proactive::new(ProactiveConfig::default()));
            assert_eq!(daemon.vault_home.root(), store.root());
            daemon.vault = crate::vault::Vault::default(); daemon.vault.start_on_this_login(1000).unwrap();
            std::fs::create_dir_all(&root).unwrap(); let path = root.join(format!("{blocked}.json")); if path.is_file() { std::fs::remove_file(&path).unwrap(); } std::fs::create_dir(&path).unwrap();
            let result = daemon.start_sign_up("fixture.test", 1001);
            assert!(result.contains("couldn't be saved") && result.contains("No external form opened") || result.contains("couldn't be saved") && result.contains("no external form opened"), "{result}");
            assert_eq!(daemon.crew.active(), 0); assert!(store.load_checked::<crate::enrol::SignupAttempts>(crate::enrol::SIGNUP_ATTEMPTS).unwrap().is_none());
        }
    }
}

#[cfg(test)]
mod publication_terminal_results {
    use super::*;
    use crate::{
        config::Config,
        platform::mock::MockPlatform,
        proactive::{Proactive, ProactiveConfig},
        publish::{Channel, PostState},
        store::Store,
    };

    #[test]
    fn malformed_stopped_and_missing_results_clear_busy_and_keep_duplicate_fence() {
        let cfg = Config::load(std::path::Path::new("config")).unwrap();
        let platform = MockPlatform::new(vec![]);
        let root = std::env::temp_dir().join(format!(
            "atlas-post-terminal-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        let mut daemon = Daemon::new(
            &cfg,
            &platform,
            None,
            Store::new(root.clone()),
            Proactive::new(ProactiveConfig::default()),
        );
        let endings = [
            crew::Ending::Stopped,
            crew::Ending::Vanished,
            crew::Ending::Done(Err("worker failed".into())),
            crew::Ending::Done(Ok("broken".into())),
            crew::Ending::Done(Ok("999999\tsent\twrong identity".into())),
            crew::Ending::Done(Ok("1\tsent".into())),
        ];
        for ending in endings {
            let id = daemon.publisher.draft(Channel::X, "hello");
            daemon.publisher.mark_submission(id, false, "pending");
            daemon.publication_jobs.insert(42, (id, 0));
            daemon.posting.push((id, u64::MAX));
            assert!(daemon
                .post_news(42, &ending, 10)
                .unwrap()
                .contains("unconfirmed"));
            assert_eq!(
                daemon.publisher.get(id).unwrap().state,
                PostState::Uncertain
            );
            assert!(!daemon.posting.iter().any(|(post, _)| *post == id));
            assert!(daemon.publication_jobs.is_empty());
            assert!(!daemon.publisher.approve(id));
        }
        drop(daemon);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_worker_after_deadline_becomes_uncertain_without_automatic_retry() {
        let cfg = Config::load(std::path::Path::new("config")).unwrap();
        let platform = MockPlatform::new(vec![]);
        let root = std::env::temp_dir().join(format!("atlas-post-timeout-{}-{}", std::process::id(), crate::store::now()));
        let mut daemon = Daemon::new(&cfg, &platform, None, Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
        let id = daemon.publisher.draft(Channel::X, "hello"); daemon.publisher.approve(id);
        daemon.publisher.mark_submission(id, false, "pending");
        daemon.publication_jobs.insert(999999, (id, 0)); daemon.posting.push((id, u64::MAX));
        assert!(daemon.check_publication_timeouts(119).is_empty());
        assert_eq!(daemon.check_publication_timeouts(120).len(), 1);
        assert_eq!(daemon.publisher.get(id).unwrap().state, PostState::Uncertain);
        assert!(daemon.publisher.due(120, true).is_empty());
        assert!(!daemon.publication_inflight(id));
        assert!(daemon.posting.is_empty());
        drop(daemon); let _ = std::fs::remove_dir_all(root);
    }
}
