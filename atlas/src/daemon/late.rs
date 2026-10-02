//! The daemon's smaller jobs, each its own `impl` block: code typing and sign-ins,
//! routines, goals, the later list, mail sorting, scheduled posts, pressing buttons,
//! storage, undo, media edits, reading documents off the loop, bringing things back,
//! advice, gestures and keys.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
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
                    crate::webrun::SignedUp::Stopped(s) => {
                        let mut said = s.spoken();
                        if let crate::enrol::Stopped::NeedsACodeFromElsewhere(_) = s {
                            // The account exists; confirming it is a code,
                            // and B1's code route applies.
                            self.signing_in_waiting = Some(e.domain.clone());
                            said = format!(
                                "The form's in on {}, and it wants a code to confirm. {}",
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
                self.journal.record_at(Act::Upkeep, &format!("sign-up on {}: {}", e.domain, said), true, t);
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
        let entropy = crate::vault::random_bytes(64);
        let password = match cfg.password.make(&entropy) {
            Ok(p) => p,
            Err(e) => return format!("I couldn't make a password that {domain} would take: {e}."),
        };
        let mut e = crate::enrol::Enrolment::new(domain, &username, t);
        let (name, kind, value) = e.vault_write(&password);
        if let Err(err) = self.vault.put(&name, kind, &value, t) {
            return format!("I couldn't keep the new password in the vault ({err}), so I didn't start.");
        }
        let _ = self.vault.save(&self.store);
        // Grant sign-in on the new account, so the next "sign me in" works.
        self.access.grant(domain, &username, domain, crate::signin::Allowed::SignIn, &name, t);
        let _ = self.access.save(&self.store);
        let bcfg = self.tools_cfg().browser.clone();
        let vars = self.tools_cfg().vars.clone();
        let work: crew::Work = Box::new(move |_ctl| {
            let outcome = match crate::browser::Browser::start(&bcfg, &vars) {
                Ok(mut b) => {
                    let o = crate::webrun::sign_up(&mut b, &mut e, &email, &password, &cfg, None);
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
            "I'm too busy to start that right now — ask me again in a moment.".into()
        }
    }
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
                    let _ = std::fs::create_dir_all(&out_dir);
                    let brief = out_dir.join("ask-for-help.md");
                    let _ = std::fs::write(&brief, crate::build_it::write_up(&latest, *rounds, &hcfg));
                    said = format!(
                        "{said} I've written the problem up for a bigger model in {} — once your server's model is set as the stronger one, I'll ask it myself.",
                        brief.display()
                    );
                }
            }
            let _ = sandbox.discard();
            if let Some(code) = outcome.code() {
                let _ = std::fs::create_dir_all(&out_dir);
                let ext = if outcome.is_built() { ext_for(lang).to_string() } else { format!("draft.{}", ext_for(lang)) };
                let path = crate::build_it::file_name_for(&out_dir, &named, &ext);
                said = match std::fs::write(&path, code) {
                    Ok(()) => format!("{said} Saved as {}, in {}.", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), out_dir.display()),
                    Err(e) => format!("{said} (I couldn't save it to {} -- {e}.)", path.display()),
                };
            }
            if outcome.is_built() {
                let _ = std::fs::remove_file(crate::build_it::Struggle::path());
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
        for r in self.routines.due_today(hour, weekday, day) {
            out.push(crate::routine::starting(&r));
            if r.automatic {
                for step in &r.steps {
                    let said = self.run_command(step, t);
                    if !said.trim().is_empty() {
                        out.push(said);
                    }
                }
            } else if self.pending_routine_run.is_none() {
                self.session.ask(&crate::routine::starting(&r));
                self.pending_routine_run = Some(r.steps.clone());
            }
        }
        let _ = self.store.save("routines", &self.routines);
        out
    }

    /// Your answer about a routine, if one was asked.
    pub(super) fn answer_about_routine(&mut self, said: &str, t: u64) -> Option<String> {
        if let Some(name) = self.pending_routine.take() {
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
            let _ = self.store.save("routines", &self.routines);
            return Some(reply);
        }
        if let Some(steps) = self.pending_routine_run.take() {
            self.session.pending = Pending::Nothing;
            if !is_yes(said) {
                return Some("Alright, not today.".into());
            }
            let mut out = Vec::new();
            for step in &steps {
                let s = self.run_command(step, t);
                if !s.trim().is_empty() {
                    out.push(s);
                }
            }
            return Some(if out.is_empty() { "Done.".into() } else { out.join("\n") });
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
        let _ = self.store.save(crate::nudge::GOALS, &self.nudger.goals);
        reply
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
        if !self.publisher.schedule(id, at) || !self.publisher.approve(id) {
            return "That post can't be scheduled any more.".into();
        }
        let _ = self.publisher.save(&self.store);
        let what = self.publisher.get(id).map(|p| p.describe()).unwrap_or_default();
        if at <= t {
            format!("Posting {what} now.")
        } else {
            let off = crate::localclock::offset_secs();
            format!(
                "Scheduled {what} for {} — I'll check it again just before it goes, and \"cancel the post\" stops it any time until then.",
                crate::localclock::hhmm(at, off)
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
                    let _ = self.publisher.save(&self.store);
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
    pub(super) fn send_post(&mut self, id: u64, t: u64, online: bool) -> Option<String> {
        if self.posting.iter().any(|(p, not_before)| *p == id && t < *not_before) {
            return None;
        }
        self.posting.retain(|(p, _)| *p != id);
        let bcfg = self.browser_cfg();
        let vars = self.tools_cfg().vars.clone();
        let mut publisher = self.publisher.clone();
        let work: crew::Work = Box::new(move |_ctl| {
            let outcome = match crate::browser::Browser::start(&bcfg, &vars) {
                Ok(mut b) => {
                    let o = crate::delivery::send(&mut publisher, &mut b, &bcfg, id, online);
                    b.close();
                    o
                }
                Err(e) => crate::delivery::classify(e),
            };
            let (kind, msg) = match &outcome {
                crate::delivery::Outcome::Sent(m) => ("sent", m.clone()),
                crate::delivery::Outcome::Retry(m) => ("retry", m.clone()),
                crate::delivery::Outcome::Blocked(m) => ("blocked", m.clone()),
            };
            Ok(format!("{id}\t{kind}\t{msg}"))
        });
        if self.hand_off("post", t, work, None, SpeakPolicy::Always) {
            self.posting.push((id, u64::MAX));
        }
        None
    }

    pub(super) fn post_news(&mut self, ending: &crew::Ending, t: u64) -> Option<String> {
        let crew::Ending::Done(Ok(line)) = ending else { return None };
        let mut parts = line.splitn(3, '\t');
        let id: u64 = parts.next()?.parse().ok()?;
        let kind = parts.next()?.to_string();
        let msg = parts.next().unwrap_or("").to_string();
        self.posting.retain(|(p, _)| *p != id);
        if kind == "retry" {
            self.posting.push((id, t + 300));
        }
        let what = self.publisher.get(id).map(|p| p.describe()).unwrap_or_default();
        let said = match kind.as_str() {
            "sent" => {
                self.publisher.mark_sent(id, "posted", true);
                self.journal.record_at(Act::Published, &format!("{what}: {msg}"), true, t);
                Some(format!("Posted: {what}."))
            }
            // Tried again on a later tick.
            "retry" => None,
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
        let _ = self.publisher.save(&self.store);
        said
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
        let r = crate::health::read_machine();
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
        let work: crew::Work = Box::new(move |ctl| {
            let mut done = Vec::new();
            let mut failed = Vec::new();
            for (from, to, _) in &plan.moves {
                if ctl.checkpoint() {
                    break;
                }
                match crate::tune::move_folder(std::path::Path::new(from), std::path::Path::new(to), t) {
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
        let stop_starting = crate::tune::pick_startup_to_stop(&crate::tune::startup_entries(with_tasks), &week, &cfg.keep, &own_path);
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
        if let Some((files, to)) = &plan.moves {
            let (moved, failed) = crate::tune::move_files_into(files, to);
            if !moved.is_empty() {
                let what = format!("moved {} file{} from Downloads into {}", moved.len(), if moved.len() == 1 { "" } else { "s" }, to.display());
                done.push(what.clone());
                self.journal.record_at(Act::Upkeep, &what, true, t);
                let id = self.history.note(&what, "files", Undo::Atlas("move them back".into()), true, t);
                undo_record.push((id, crate::tune::TuneUndo::Moves(moved)));
            }
            not.extend(failed);
        }
        // Kept bounded, like the history it belongs to.
        if undo_record.len() > 200 {
            undo_record.drain(0..undo_record.len() - 200);
        }
        let _ = self.store.save(crate::tune::TUNE_UNDO_RECORD, &undo_record);
        let _ = self.store.save("undo_history", &self.history);
        let mut said = if done.is_empty() { "Nothing changed.".to_string() } else { format!("Done: {}.", done.join("; ")) };
        if !not.is_empty() {
            said.push_str(&format!(" Not done: {}.", not.join("; ")));
        }
        let r = crate::health::read_machine();
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
        let sys = self.tools_cfg().system.clone();
        let done = crate::organize::carry_out_moves(&plan, &sys, crate::store::now());
        if !done.moved.is_empty() {
            let names: Vec<String> =
                plan.folders.iter().map(|f| f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| f.display().to_string())).collect();
            let what = format!(
                "sorted {} file{} in {} into folders",
                done.moved.len(),
                if done.moved.len() == 1 { "" } else { "s" },
                names.join(", ")
            );
            self.journal.record_at(Act::Upkeep, &what, true, t);
            let id = self.history.note(&what, "files", crate::undo::Undo::Atlas("move them back".into()), true, t);
            let mut record: Vec<(u64, crate::tune::TuneUndo)> = self.store.load(crate::tune::TUNE_UNDO_RECORD);
            record.push((id, crate::tune::TuneUndo::Organized { moves: done.moved.clone(), made: done.made.clone() }));
            // Kept bounded, like the history it belongs to.
            if record.len() > 200 {
                record.drain(0..record.len() - 200);
            }
            let _ = self.store.save(crate::tune::TUNE_UNDO_RECORD, &record);
            let _ = self.store.save("undo_history", &self.history);
        }
        crate::organize::done_said(&plan, &done)
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
                let _ = hearing.save_to(&store);
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
        let mut record: Vec<crate::tune::Moved> = self.store.load(crate::tune::MOVED_RECORD);
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
            record.push(m.clone());
        }
        // Checked (30 Sep 2026): "Moved" was said when the new place wasn't
        // saved, and after a restart Atlas couldn't find its models.
        let settings_kept = match &settings_unreadable {
            Some(e) => Err(e.clone()),
            None => prefs.save(&dir).map_err(|e| e.to_string()),
        };
        let _ = self.store.save(crate::tune::MOVED_RECORD, &record);
        let mut said = if done.is_empty() {
            "Nothing moved.".to_string()
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

impl<'a> Daemon<'a> {
    pub(super) fn carry_out_undo(&mut self, id: u64) -> String {
        let Some(d) = self.history.done.iter().find(|d| d.id == id).cloned() else {
            return "That one isn't in my history any more.".into();
        };
        if d.undone {
            return format!("\"{}\" is already undone.", d.what);
        }
        // A startup entry switched off, or files moved, by an optimization
        // run (2 Oct 2026): taken back from what was kept beside it.
        let mut record: Vec<(u64, crate::tune::TuneUndo)> = self.store.load(crate::tune::TUNE_UNDO_RECORD);
        if let Some(pos) = record.iter().position(|(rid, _)| *rid == id) {
            return match crate::tune::undo_tune_change(&record[pos].1) {
                Ok(said) => {
                    record.remove(pos);
                    let _ = self.store.save(crate::tune::TUNE_UNDO_RECORD, &record);
                    self.history.mark_undone(id);
                    let _ = self.store.save("undo_history", &self.history);
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
                    let _ = self.publisher.save(&self.store);
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
        let _ = self.store.save("undo_history", &self.history);
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
        let Some((path, _)) = crate::edit::path_and_wish(said) else {
            return Some("Which video? Give me its path -- get this video ready: \"C:\\clips\\bakery.mp4\".".into());
        };
        let original = std::path::PathBuf::from(&path);
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
        let folder = original.with_file_name(format!("{stem} - ready"));
        let work: crew::Work = Box::new(move |ctl| {
            let run = |tool: &crate::tools::ExternalTool, args: Vec<String>| -> std::result::Result<std::process::Output, String> {
                crate::tools::command(&tool.command).args(args).output().map_err(|e| format!("couldn't run {}: {e}", tool.command))
            };
            std::fs::create_dir_all(&folder).map_err(|e| format!("couldn't make {}: {e}", folder.display()))?;
            let s = |p: &std::path::Path| p.display().to_string();
            let src = s(&original);
            let probed = run(&video.ffprobe, crate::edit::probe_args(&src))?;
            let before = crate::edit::duration_from_probe(&String::from_utf8_lossy(&probed.stdout)).ok_or("I couldn't read how long the video is")?;
            if ctl.checkpoint() {
                return Err("stopped".into());
            }
            let found = run(&video.ffmpeg, crate::studio::silence_args(&src))?;
            let spans = crate::studio::keep_spans(&crate::studio::silences(&String::from_utf8_lossy(&found.stderr), before), before);
            let cut = folder.join(format!("{stem} - cut.mp4"));
            let made = run(&video.ffmpeg, crate::studio::cut_args(&src, &spans, &s(&cut)))?;
            if !made.status.success() || !cut.is_file() {
                return Err("ffmpeg couldn't make the cut".into());
            }
            let after: f64 = spans.iter().map(|(a, b)| b - a).sum();
            let _ = run(&video.ffmpeg, crate::studio::thumb_args(&s(&cut), &s(&folder.join("thumbnail-%02d.jpg"))));
            let thumbs = std::fs::read_dir(&folder).map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("thumbnail-")).count()).unwrap_or(0);
            let mut transcript = String::new();
            if let Some(timed) = &timed {
                let wav = folder.join("sound.wav");
                // The sound pulled out for the transcriber, and the .srt it
                // writes beside it, are scratch: gone however this ends, an
                // early return or a panic included.
                struct Scratch(Vec<std::path::PathBuf>);
                impl Drop for Scratch {
                    fn drop(&mut self) {
                        for f in &self.0 {
                            let _ = std::fs::remove_file(f);
                        }
                    }
                }
                let _scratch = Scratch(vec![wav.clone(), wav.with_extension("srt")]);
                if run(&video.ffmpeg, crate::studio::audio_args(&s(&cut), &s(&wav))).is_ok() {
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
                    if let Ok(srt) = tool.run(&v, None) {
                        let _ = std::fs::write(folder.join(format!("{stem} - cut.srt")), &srt);
                        transcript = crate::viewing::read_timed(&srt).iter().map(|x| x.words.clone()).collect::<Vec<_>>().join(" ");
                    }
                }
            }
            let mut title = None;
            if let (Some(m), false) = (llm.as_deref(), transcript.trim().is_empty()) {
                let quoted = crate::untrusted::Read::new("the video", &transcript, crate::store::now()).quoted();
                if let Some((t, d)) = m.complete(crate::studio::TITLE_PROMPT, &quoted).ok().and_then(|r| crate::studio::title_and_description(&r, &transcript)) {
                    let _ = std::fs::write(folder.join("title and description.txt"), format!("{t}\n\n{d}\n"));
                    title = Some(t);
                }
            }
            Ok(crate::studio::ready_said(&s(&folder), before, after, !transcript.is_empty(), thumbs, title.as_deref()))
        });
        Some(if self.hand_off("studio", t, work, Some(path.clone()), SpeakPolicy::Always) {
            format!("Getting {path} ready on a copy -- dead air, captions, thumbnails and a title. I'll tell you when it's done.")
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
            let as_open = low.splitn(2, ' ').nth(1).map(|rest| format!("open {rest}")).unwrap_or_default();
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
            return "Which file? Give me its path, or hand it to me first.".into();
        };
        if path.to_lowercase().ends_with(".zip") {
            return self.file_work_off_the_loop(FileJob::Unzip, &path, false);
        }
        self.file_work_off_the_loop(FileJob::Read, &path, false)
    }

    pub(super) fn unzip_asked(&mut self, said: &str) -> String {
        let Some(path) = self.file_meant(said, &["zip"]) else {
            return "Which zip? Give me its path, or hand it to me first.".into();
        };
        self.file_work_off_the_loop(FileJob::Unzip, &path, false)
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
    /// crew full it is done here, as before, rather than not at all.
    pub(super) fn file_work_off_the_loop(&mut self, job: FileJob, path: &str, anyway: bool) -> String {
        let tools = self.tools_cfg();
        let (p, what) = (path.to_string(), job);
        let work: crew::Work = Box::new(move |_c: &crew::Control| {
            let done = match what {
                FileJob::Read => read_document_off(&p, anyway, &tools),
                FileJob::Unzip => unzip_off(&p, anyway, &tools),
            };
            serde_json::to_string(&done).map_err(|e| e.to_string())
        });
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
        if self.hand_off("read-file", crate::store::now(), work, Some(format!("{} {path}", if job == FileJob::Read { "read" } else { "unzip" })), SpeakPolicy::Always) {
            match job {
                FileJob::Read => format!("Reading {name} -- I'll tell you what's in it in a moment."),
                FileJob::Unzip => format!("Unpacking {name} -- I'll tell you when it's done."),
            }
        } else {
            let tools = self.tools_cfg();
            let done = match job {
                FileJob::Read => read_document_off(path, anyway, &tools),
                FileJob::Unzip => unzip_off(path, anyway, &tools),
            };
            self.file_done(done)
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
            all.sort_by(|a, b| b.when.cmp(&a.when));
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
        let _ = h.save_to(&store);
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
    pub(super) fn creator_advice(&self, said: &str) -> String {
        if !self.tools_cfg().editcraft.enabled {
            return "Video and creator advice is switched off in your settings.".into();
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
