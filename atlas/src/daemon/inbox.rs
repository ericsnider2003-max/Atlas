//! The context a turn is answered in, and mail: checking it, unsubscribing,
//! Outlook, reading orders and drafts, and drafting outreach.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    pub fn context(&mut self) -> String {
        let mut s = brain::context(self.cfg, self.plat);

        // The question this turn is answering, first, because everything else
        // in the context is standing information and this is the one thing
        // that makes the next sentence mean what it means. "The second one"
        // is unreadable without it.
        if let Some(q) = self.answering.take() {
            s.push_str(&format!(
                "You just asked: {q}\nWhat follows is their answer to that, not a new \
                 request. Take it as the missing piece and carry on.\n"
            ));
        }

        // Where the model is decides how much of this goes in. See
        // `brain::focus_line`: a window title is both written by someone else
        // and private, and those need different answers.
        //
        // No configured model means nothing is sent at all, so the question is
        // moot -- but `CannotTell` is the honest answer for a context built
        // without one, and it is the cautious one, so that is what it gets.
        // With no hand-written `llm:`, the connection is the one Atlas built
        // for itself from `models:` -- local unless the server is elsewhere.
        let at = match self.tools_ref() {
            Some(t) => match &t.llm {
                Some(l) => l.endpoint(),
                None => crate::models::self_built_endpoint(&t.models),
            },
            None => brain::Endpoint::CannotTell,
        };

        let active = self.plat.active_window().unwrap_or(None);
        // Only when what was said is about the screen (`doing::refers_to_screen`,
        // 29 Sep 2026); the title is still read for orders below either way.
        if let Some(a) = &active {
            let app = a.process.trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
            if crate::doing::refers_to_screen(&self.last_said, &app) {
                s.push_str(&brain::focus_line(a, at));
            }
        }
        let names: Vec<String> =
            self.index.recent(5).iter().map(|e| e.name.clone()).collect();
        s.push_str(&brain::recent_files_line(&names, at));

        // Somebody wrote an instruction into a window title or a file name.
        //
        // The quoting above is what protects Atlas; this is so Eric hears
        // about it. Recorded under the same store key `atlas read` uses, so
        // "what have you been fed" has ONE answer covering everything Atlas
        // has taken in -- whether it fetched the page or merely had it in
        // front of it.
        //
        // Inside the `if`, so the ordinary turn -- which is every turn --
        // neither loads nor saves the inbox. Reading a file on each context
        // build to record nothing would be a real cost for the rare case.
        let tried = brain::orders_in_view(active.as_ref(), &names);
        if !tried.is_empty() {
            let what = tried.join("; ");
            self.log.info(&format!("order-shaped text in view: {what}"));
            let mut fed: crate::untrusted::Inbox = self.store.load("read-from-outside");
            fed.took_in(
                crate::untrusted::Read::new(
                    active.as_ref().map(|a| a.process.as_str()).unwrap_or("a file name"),
                    active.as_ref().map(|a| a.title.as_str()).unwrap_or_default(),
                    crate::store::now(),
                ),
                500,
            );
            // Same `let _ =` as the other saves on this path: a context build
            // must not fail because the record of an attempt could not be
            // written, and `log.info` above has already said it happened.
            let _ = self.store.save("read-from-outside", &fed);
        }
        // What you have corrected, in front of the model on every turn.
        //
        // This is the line that makes `revise.rs` mean anything. Its rule 1 is
        // that where a lesson is written decides whether it works -- a lesson
        // about how a task is done has to be read *every time* the task runs,
        // or it is a note in a diary nobody opens. Before this, nothing
        // learned reached the model at all: context was displays, apps, the
        // focused window, recent files and the conversation.
        //
        // Bounded by `MAX_STANDING` for the same reason the brief is bounded:
        // a context that grows with every correction eventually crowds out the
        // thing you just said.
        let learned = crate::revise::standing(&self.mending.applied);
        if !learned.is_empty() {
            s.push_str(&learned);
            s.push('\n');
        }
        let convo = self.thread.context(&self.thread_cfg());
        if !convo.trim().is_empty() {
            s.push_str("Conversation so far:\n");
            s.push_str(&convo);
            s.push('\n');
        }
        s
    }

    /// Look something up on the web and write a note about it.
    ///
    /// Used to run the whole thing here, synchronously, on the tick or
    /// turn thread — a search-and-summarize can easily run twenty or
    /// thirty seconds, which is exactly the class of thing `crew.rs`
    /// exists for: it is not fixing a chore nobody's waiting on (that's
    /// `backup`/`housekeeping`), it is the difference between Atlas
    /// answering everything else while it works, and going silent for
    /// half a minute because you asked it to look something up.
    ///
    /// So `research` is spoken in two parts now. This function returns the
    /// honest immediate truth — "looking into it" — and the real answer,
    /// whichever it turns out to be, is reported later by
    /// `take_crew_news` once the crew errand actually finishes. Every
    /// failure below that can be known *now* (switched off, offline, no
    /// model) still returns immediately and finally — there is nothing to
    /// wait on in those cases, so there is nothing to be honest about
    /// deferring.
    ///
    /// The note is saved automatically either way — Eric's call: he
    /// doesn't want the save narrated (where it went, or whether it
    /// worked). Asking for a note by name, or asking Atlas to save
    /// something, are separate, deliberate asks; this one just happens.
    /// "What's in my inbox." Every account, fetched for real — each one is
    /// a TCP connection, a TLS handshake, a login, and a search, which is
    /// genuinely slow with several accounts, so this goes to the crew the
    /// same way `research` and `ask_the_room` do: an honest immediate
    /// acknowledgment, and the real sorted inbox once every reachable
    /// account has actually answered.
    /// The vault lookup every mail feature needs first: for each
    /// configured account, work out its vault entry and read the actual
    /// password out — on the tick thread, since a crew errand can't
    /// borrow `self.vault`. Shared between `check_mail` and
    /// `check_unsubscribe` rather than duplicated, since it's the same
    /// question either way: which accounts can Atlas actually get into
    /// right now.
    pub(super) fn resolve_mail_jobs(
        &mut self,
        cfg: &crate::mail::MailConfig,
        now: u64,
    ) -> (Vec<(crate::mail::Account, String)>, Vec<String>) {
        let mut jobs = Vec::new();
        let mut problems = Vec::new();
        // Scheduled work with nobody there to type: the sign-in copy, if you
        // made one (`vault.open_on_this_login`).
        self.open_vault_for_scheduled_work(now);
        for account in &cfg.accounts {
            // Himalaya keeps its own passwords: nothing comes out of the vault
            // for it, and its errand asks Himalaya rather than a server.
            if cfg.by_himalaya() {
                let mut a = account.clone();
                a.imap_host = crate::himalaya::as_host(&cfg.himalaya, account.for_himalaya());
                jobs.push((a, String::new()));
                continue;
            }
            let vault_name = match crate::mail::credential_source(account) {
                Ok(n) => n.to_string(),
                Err(e) => {
                    problems.push(format!("{}: {e}", account.name));
                    continue;
                }
            };
            match self.vault.get(&vault_name, now) {
                Ok(password) => jobs.push((account.clone(), password)),
                Err(e) => problems.push(format!("{}: {e}", account.name)),
            }
        }
        (jobs, problems)
    }

    /// Make or remove the vault's sign-in copy to match the setting, while
    /// the vault is open with your passphrase. Returns a sentence to add.
    pub(super) fn keep_sign_in_copy(&mut self, now: u64) -> String {
        let want = self.tools_cfg().vault.open_on_this_login;
        let have = self.vault.sealed_to_this_login();
        let said = if want && !have {
            match self.vault.seal_to_this_login(now) {
                Ok(()) => " Sealed a copy to your Windows sign-in, so scheduled mail checks can open it while you're signed in.".to_string(),
                Err(why) => format!(" I couldn't seal it to your sign-in: {why}."),
            }
        } else if !want && have {
            self.vault.unseal_from_this_login();
            " Took away the sign-in copy, as your settings say.".to_string()
        } else {
            String::new()
        };
        if !said.is_empty() {
            let _ = self.vault.save(&self.vault_home);
        }
        said
    }

    /// Open the vault for scheduled work through the sign-in copy, when the
    /// setting is on and it is shut. Never counts as proving it's you.
    fn open_vault_for_scheduled_work(&mut self, now: u64) {
        if self.vault.state() == crate::vault::State::Open || !self.tools_cfg().vault.open_on_this_login {
            return;
        }
        if !self.vault.sealed_to_this_login() {
            return;
        }
        if let Err(why) = self.vault.open_unattended(now) {
            self.log.warn(&format!("couldn't open the vault on your sign-in: {why}"));
        }
    }

    /// Update the contact book from the messages just read, and say what is
    /// worth saying about it.
    ///
    /// Groups the kept messages by sender, asks `messaging::note_on` for a
    /// note on each (which is where the folder is decided from what they
    /// wrote), and merges those into the stored notes so the book accumulates
    /// across reads rather than being rebuilt each time. The only thing said
    /// out loud is a *new* work or prospect contact -- the approach you would
    /// otherwise miss in a count of "3 messages". Personal and unsorted senders
    /// are kept silently.
    pub(super) fn note_the_senders(
        &mut self,
        kept: &[crate::messaging::Message],
        names: &[String],
    ) -> String {
        // By sender, in first-seen order, so each note is built from that one
        // person's messages the way `note_on` expects.
        let mut order: Vec<String> = Vec::new();
        let mut by_sender: std::collections::HashMap<String, Vec<crate::messaging::Message>> =
            std::collections::HashMap::new();
        for m in kept {
            if !by_sender.contains_key(&m.from) {
                order.push(m.from.clone());
            }
            by_sender.entry(m.from.clone()).or_default().push(m.clone());
        }

        let mut people: Vec<crate::messaging::Person> =
            self.store.load(crate::messaging::PEOPLE);
        let mut fresh_work: Vec<crate::messaging::Person> = Vec::new();
        for from in &order {
            let msgs = &by_sender[from];
            let Some(note) = crate::messaging::note_on(msgs, names) else {
                continue;
            };
            let is_work = matches!(
                note.folder,
                crate::messaging::Folder::Work | crate::messaging::Folder::Prospect
            );
            match people
                .iter_mut()
                .find(|p| p.name == note.name && p.platform == note.platform)
            {
                Some(existing) => {
                    // Was this already someone we knew was work? Only a folder
                    // that firms up now is news.
                    let was_work = matches!(
                        existing.folder,
                        crate::messaging::Folder::Work | crate::messaging::Folder::Prospect
                    );
                    existing.last_at = note.last_at;
                    existing.messages = existing.messages.saturating_add(note.messages);
                    existing.folder = note.folder;
                    if is_work && !was_work {
                        fresh_work.push(existing.clone());
                    }
                }
                None => {
                    if is_work {
                        fresh_work.push(note.clone());
                    }
                    people.push(note);
                }
            }
        }
        let _ = self.store.save(crate::messaging::PEOPLE, &people);

        // The first new work contact is said the way you asked (F3), with the
        // offer to find the answer; saying yes starts the research. Any others
        // are named.
        let Some(first) = fresh_work.first() else { return String::new() };
        let mut said = String::new();
        if let Some(line) = crate::messaging::filed(first) {
            let topic = first.first_about.trim().to_string();
            if !topic.is_empty() && self.pending_offer.is_none() {
                self.session.ask(&line);
                self.pending_offer = Some(crate::proactive::Offer {
                    kind: "answer_a_contact".into(),
                    message: line.clone(),
                    command: format!("research {topic}"),
                    confidence: 0.8,
                    cost: 1,
                });
            }
            said.push(' ');
            said.push_str(&line);
        }
        if fresh_work.len() > 1 {
            let rest: Vec<String> = fresh_work[1..].iter().map(|p| format!("{} ({})", p.name, p.folder.name())).collect();
            said.push_str(&format!(" Also new and worth a note: {}.", rest.join(", ")));
        }
        said
    }

    pub(super) fn check_mail(&mut self) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if !cfg.enabled {
            return "Reading your email is switched off.".into();
        }
        if cfg.accounts.is_empty() {
            return "I don't have any mail accounts set up yet.".into();
        }
        if self.connectivity.cached() == Reach::Offline {
            return "I can't check your mail without a connection.".into();
        }

        let now = crate::store::now();
        // The password comes out of the vault here, on the tick thread —
        // a crew errand can't borrow `self.vault`, and a decrypted app
        // password shouldn't be threaded through any more code than it
        // has to be. What actually crosses into the errand is the
        // account and its password, already resolved, nothing else.
        let (jobs, setup_problems) = self.resolve_mail_jobs(&cfg, now);
        if jobs.is_empty() {
            return format!("I couldn't get at any of your mail accounts: {}", setup_problems.join("; "));
        }

        let store = self.store.clone();
        // Drafting replies is background work (`deepbrain`).
        let llm = self.background_llm();
        let draft_cfg = self.tools_cfg().draft.clone();
        let may_email_clients = cfg.may_email_clients;

        // The mail cache the waiting-for list and meeting prep read from
        // (round 11): letters, not whole messages -- excerpts, scrubbed.
        let wd = self.workday_cfg();
        let mail_book_on = wd.waiting_for.enabled;
        let look_back_days = wd.waiting_for.look_back_days;
        let keep_days = wd.mail_keep_days;
        let work: crew::Work = Box::new(move |ctl| {
            let clients = crate::clients::ClientList::load(&store);
            let mut outbox = crate::outbox::Outbox::load(&store);
            let mut mail_book: crate::mailbook::MailBook = store.load(crate::mailbook::MailBook::FILE);
            let mut orders = crate::orders::Orders::load(&store);
            let mut triaged = Vec::new();
            let mut failures = setup_problems;
            // Names of clients a reply was just drafted for, so the
            // spoken report can say "there's a reply to Jane ready" —
            // built as messages are processed, not recomputed from the
            // outbox afterward, so it only ever names what happened on
            // *this* check.
            let mut fresh_drafts: Vec<String> = Vec::new();
            let mut fresh_sent: Vec<String> = Vec::new();
            let mut order_updates: Vec<String> = Vec::new();
            // Everything fetched, for grouping into conversations afterwards.
            let mut for_threads: Vec<crate::mailthread::Mail> = Vec::new();
            // The domains you deal with — your clients' and your own — which
            // is exactly the list a lookalike sender is built against.
            let mut known_domains: Vec<String> = clients
                .all()
                .iter()
                .filter_map(|c| c.address.rsplit_once('@').map(|x| x.1.to_lowercase()))
                .chain(jobs.iter().filter_map(|(a, _)| a.address.rsplit_once('@').map(|x| x.1.to_lowercase())))
                .collect();
            known_domains.sort();
            known_domains.dedup();
            let mut careful: Vec<String> = Vec::new();
            for (account, password) in &jobs {
                // Between accounts: a pause holds here, nothing half-read.
                if ctl.checkpoint() {
                    break;
                }
                let host = if !account.imap_host.is_empty() {
                    account.imap_host.clone()
                } else {
                    match crate::mail::Provider::from_address(&account.address).imap_host() {
                        Some(h) => h.to_string(),
                        None => {
                            failures.push(format!(
                                "{}: unrecognised provider, needs imap_host set explicitly",
                                account.name
                            ));
                            continue;
                        }
                    }
                };
                // An Outlook/Microsoft 365 account with `oauth` off
                // lands here and fails cleanly at login — Microsoft
                // killed password-based IMAP entirely, and there's no
                // password to fall back to. The server's own error comes
                // back rather than anything guessed at.
                let sent_since = mail_book_on.then(|| {
                    let last = mail_book.checked.get(&account.address).copied().unwrap_or(0);
                    let back = crate::store::now().saturating_sub(last.max(crate::store::now().saturating_sub(look_back_days * 86_400)));
                    crate::triage::imap_date((back / 86_400 + 1) as u32, crate::store::now())
                });
                match connect_and_fetch_inbox(
                    &host,
                    &account.address,
                    password,
                    account.oauth.then_some(account.client_id.as_str()),
                    sent_since.as_deref(),
                ) {
                    Ok((msgs, sent)) => {
                        if mail_book_on {
                            let fetched = crate::store::now();
                            let mut letters: Vec<crate::mailbook::Letter> = msgs.iter().map(|m| crate::mailbook::Letter::from_imap(m, false, fetched)).collect();
                            match sent {
                                Ok(sent) => {
                                    letters.extend(sent.iter().take(300).map(|m| crate::mailbook::Letter::from_imap(m, true, fetched)));
                                    mail_book.checked.insert(account.address.clone(), fetched);
                                }
                                Err(e) => failures.push(format!("{} (sent mail): {e}", account.name)),
                            }
                            mail_book.add(letters, look_back_days.max(keep_days), fetched);
                        }
                        for_threads.extend(msgs.iter().map(|m| crate::mailthread::Mail::from_imap(m)));
                        for m in &msgs {
                            let t: crate::triage::Message = m.into();
                            let triaged_one = crate::triage::triage(&t);
                            let (_, address) = crate::unsub::split_from(&m.from);
                            // A sender that only looks like someone you deal
                            // with, or that its own server says is forged,
                            // is named and gets no draft, however polite.
                            let suspect = crate::lookalike::sender_warning(&m.from, &m.authentication_results, &known_domains);
                            if let Some(w) = &suspect {
                                careful.push(w.clone());
                            }
                            if let Some(status) = crate::orders::status_from_subject(&m.subject) {
                                if let Some(merchant) = crate::orders::merchant_from_address(&address) {
                                    if let Some(order) =
                                        orders.update(&merchant, &m.subject, status, crate::store::now())
                                    {
                                        order_updates
                                            .push(format!("{}: {}", order.merchant, order.status.spoken()));
                                    }
                                }
                            }
                            if triaged_one.draftable
                                && suspect.is_none()
                                && matches!(triaged_one.needs, crate::triage::Needs::Reply)
                                && clients.is_client(&address)
                            {
                                if let Some(llm) = &llm {
                                    if let Some(client) = clients.get(&address) {
                                        match draft_client_reply(
                                            llm.as_ref(),
                                            client.name_or_address(),
                                            &m.subject,
                                            &m.body,
                                        ) {
                                            Ok(body) => {
                                                // Rewrite it, up to max_passes,
                                                // keeping only genuine
                                                // improvements — the revise
                                                // loop that `max_passes` was
                                                // bounding but that nothing
                                                // ever ran.
                                                let body = match crate::draft::revise(
                                                    &body,
                                                    llm.as_ref(),
                                                    &draft_cfg,
                                                ) {
                                                    crate::draft::Outcome::Revised { text, .. } => text,
                                                    _ => body,
                                                };
                                                let notes =
                                                    crate::draft::critique(&body, None, &draft_cfg);
                                                let created = crate::store::now();
                                                let id = crate::outbox::Outbox::make_id(
                                                    &account.name,
                                                    &client.address,
                                                    created,
                                                );
                                                let mut pending = crate::outbox::PendingReply {
                                                    id,
                                                    account: account.name.clone(),
                                                    to_address: client.address.clone(),
                                                    to_name: client.name_or_address().to_string(),
                                                    subject: format!("Re: {}", m.subject),
                                                    body,
                                                    kind: crate::outbox::Kind::Client,
                                                    critique: notes,
                                                    created_at: created,
                                                    status: crate::outbox::Status::Waiting,
                                                };
                                                if may_email_clients {
                                                    match send_reply(
                                                        &pending,
                                                        &account.address,
                                                        password,
                                                        account.oauth.then_some(account.client_id.as_str()),
                                                    ) {
                                                        Ok(()) => {
                                                            pending.status = crate::outbox::Status::Sent;
                                                            fresh_sent.push(pending.to_name.clone());
                                                        }
                                                        Err(e) => failures.push(format!(
                                                            "sending a reply to {address}: {e}"
                                                        )),
                                                    }
                                                } else {
                                                    fresh_drafts.push(pending.spoken_notice());
                                                }
                                                outbox.add(pending);
                                            }
                                            Err(e) => failures
                                                .push(format!("drafting a reply to {address}: {e}")),
                                        }
                                    }
                                }
                            }
                            triaged.push(t);
                        }
                    }
                    Err(e) => failures.push(format!("{}: {e}", account.name)),
                }
            }
            let _ = outbox.save(&store);
            let _ = orders.save(&store);
            if mail_book_on {
                let _ = store.save(crate::mailbook::MailBook::FILE, &mail_book);
            }
            let sorted = crate::triage::sort_all(&triaged);
            let mut said = crate::triage::spoken(&sorted);
            // Said first-thing after the summary, not buried under drafts.
            for w in careful.iter().take(3) {
                said.push_str(&format!(" {w}"));
            }
            // Several new messages that are one conversation are said as one:
            // "the installer thread has 3 new" is the thing to know, not three
            // separate lines that happen to share a subject.
            for (subject, n) in crate::mailthread::conversations(&for_threads).iter().take(3) {
                said.push_str(&format!(" {n} of these are one conversation: \"{subject}\"."));
            }
            // The sentence is the outbox's own, not a second copy of it.
            for notice in &fresh_drafts {
                said.push_str(&format!(" {notice}"));
            }
            for name in &fresh_sent {
                said.push_str(&format!(" Sent a reply to {name}."));
            }
            for update in &order_updates {
                said.push_str(&format!(" Order update — {update}."));
            }
            if !failures.is_empty() {
                said.push_str(&format!(
                    " ({} account{} had trouble: {})",
                    failures.len(),
                    if failures.len() == 1 { "" } else { "s" },
                    failures.join("; ")
                ));
            }
            Ok(said)
        });

        if self.hand_off("mail", now, work, None, SpeakPolicy::Always) {
            "Checking your mail. I'll let you know what's in it.".into()
        } else {
            "I'm swamped with background work right now — ask me to check mail again in a moment."
                .into()
        }
    }

    /// "Clear out my inbox." Unlike `check_mail`'s `UNSEEN` search, this
    /// needs a wider window and *every* message in it, read or not — a
    /// sender's engagement ratio (opened, replied, out of how many) is
    /// only real once it's counted over more than the last few unread
    /// messages. `\Seen`/`\Answered` are the server's own record of what
    /// you did with each one, so one fetch over that window is a real
    /// verdict, not a guess — no separate history for Atlas to keep.
    pub(super) fn check_unsubscribe(&mut self) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if !cfg.enabled {
            return "Reading your email is switched off.".into();
        }
        if cfg.accounts.is_empty() {
            return "I don't have any mail accounts set up yet.".into();
        }
        if self.connectivity.cached() == Reach::Offline {
            return "I can't look at your mail without a connection.".into();
        }

        let now = crate::store::now();
        let (jobs, setup_problems) = self.resolve_mail_jobs(&cfg, now);
        if jobs.is_empty() {
            return format!("I couldn't get at any of your mail accounts: {}", setup_problems.join("; "));
        }

        let unsub_cfg = self.tools_cfg().unsub.clone();
        const WINDOW_DAYS: u32 = 60;
        let since = crate::triage::imap_date(WINDOW_DAYS, now);

        let keep_in = self.store.clone();
        let work: crew::Work = Box::new(move |ctl| {
            let mut failures = setup_problems;
            let mut per_account_spoken = Vec::new();
            // What you can say yes to afterwards (30 Sep 2026: the report
            // ended there, and there was no way to act on it).
            let mut waiting: Vec<(String, crate::unsub::Cleanup)> = Vec::new();
            let mut total_unsubscribed = 0usize;
            for (account, password) in &jobs {
                // Between accounts: a pause holds here, nothing half-read.
                if ctl.checkpoint() {
                    break;
                }
                let host = if !account.imap_host.is_empty() {
                    account.imap_host.clone()
                } else {
                    match crate::mail::Provider::from_address(&account.address).imap_host() {
                        Some(h) => h.to_string(),
                        None => {
                            failures.push(format!(
                                "{}: unrecognised provider, needs imap_host set explicitly",
                                account.name
                            ));
                            continue;
                        }
                    }
                };
                let messages = match connect_and_fetch_since(
                    &host,
                    &account.address,
                    password,
                    &since,
                    account.oauth.then_some(account.client_id.as_str()),
                ) {
                    Ok(m) => m,
                    Err(e) => {
                        failures.push(format!("{}: {e}", account.name));
                        continue;
                    }
                };
                let senders = crate::unsub::senders_from(&messages, crate::store::now());
                let cleanup = crate::unsub::plan(&senders, &unsub_cfg);
                // Per account, not combined: a real send has to come from
                // the same mailbox the message arrived at, and knowing
                // that only works while each account's own cleanup plan
                // is still its own.
                if unsub_cfg.bulk_without_asking && !cleanup.unsubscribe.is_empty() {
                    let (done, send_failures) = carry_out_unsubscribes(&cleanup, account, password);
                    total_unsubscribed += done;
                    failures.extend(send_failures);
                } else {
                    per_account_spoken.push(crate::unsub::spoken(&cleanup));
                    if !cleanup.unsubscribe.is_empty() {
                        waiting.push((account.name.clone(), cleanup));
                    }
                }
            }
            let mut said = if unsub_cfg.bulk_without_asking {
                format!("Unsubscribed from {total_unsubscribed}.")
            } else if per_account_spoken.iter().all(|s| s.starts_with("Nothing worth")) {
                "Nothing worth clearing out — you read most of what you get.".into()
            } else {
                per_account_spoken.join(" ")
            };
            if !waiting.is_empty() {
                match keep_in.save(crate::unsub::PENDING, &waiting) {
                    Ok(()) => said.push_str(" Say \"unsubscribe from those\" and I will."),
                    Err(e) => said.push_str(&format!(" (I couldn't keep the list to act on: {e})")),
                }
            }
            if !failures.is_empty() {
                said.push_str(&format!(
                    " ({} thing{} had trouble: {})",
                    failures.len(),
                    if failures.len() == 1 { "" } else { "s" },
                    failures.join("; ")
                ));
            }
            Ok(said)
        });

        if self.hand_off("unsubscribe", now, work, None, SpeakPolicy::Always) {
            format!("Looking at the last {WINDOW_DAYS} days of mail to see what's worth clearing out.")
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    /// "Unsubscribe from those", after the report: the one-click
    /// unsubscribes it found, sent from the mailbox each came to. Blocking
    /// is left alone -- that's for the mail sorter's rules.
    pub(super) fn unsubscribe_help(&mut self, said: &str) -> Option<String> {
        if !crate::unsub::go_ahead(said) {
            return None;
        }
        let waiting: Vec<(String, crate::unsub::Cleanup)> = self.store.load(crate::unsub::PENDING);
        if waiting.is_empty() {
            return Some("There's nothing lined up to unsubscribe from. Ask me to clear out your email and I'll look first.".into());
        }
        let cfg = self.tools_cfg().mail.clone();
        let now = crate::store::now();
        let (jobs, problems) = self.resolve_mail_jobs(&cfg, now);
        let mut todo: Vec<(crate::unsub::Cleanup, crate::mail::Account, String)> = Vec::new();
        for (name, cleanup) in waiting {
            if let Some((a, p)) = jobs.iter().find(|(a, _)| a.name == name) {
                todo.push((cleanup, a.clone(), p.clone()));
            }
        }
        if todo.is_empty() {
            return Some(format!("I couldn't get at the mailbox those came to: {}", problems.join("; ")));
        }
        let store = self.store.clone();
        let count: usize = todo.iter().map(|(c, _, _)| c.unsubscribe.len()).sum();
        let work: crew::Work = Box::new(move |_ctl| {
            let mut done = 0;
            let mut failed = Vec::new();
            for (cleanup, account, password) in &todo {
                let (d, f) = carry_out_unsubscribes(cleanup, account, password);
                done += d;
                failed.extend(f);
            }
            let _ = store.save(crate::unsub::PENDING, &Vec::<(String, crate::unsub::Cleanup)>::new());
            Ok(if failed.is_empty() {
                format!("Unsubscribed from {done}.")
            } else {
                format!("Unsubscribed from {done}; {} didn't go: {}.", failed.len(), failed.join("; "))
            })
        });
        Some(if self.hand_off("unsubscribe", now, work, None, SpeakPolicy::Always) {
            format!("Unsubscribing from {count} now.")
        } else {
            "I'm swamped with background work right now -- ask me again in a moment.".into()
        })
    }

    /// "Connect my outlook account me@outlook.com with client id
    /// abc-123." Pulls the address (the word with an `@` in it) and
    /// everything after "client" as the client ID — no fancier parsing
    /// than that, since both are things you'd type or paste exactly
    /// rather than phrase naturally.
    pub(super) fn connect_outlook_from_request(&mut self, what: &str) -> String {
        let Some(address) = what.split_whitespace().find(|w| w.contains('@')) else {
            return "What's the Outlook address, and what's the client ID from your app \
                     registration? Ask me for outlook setup help if you haven't registered one yet."
                .into();
        };
        let Some(client_pos) = what.find("client ") else {
            return "I have the address, but I need the client ID from your Azure app \
                     registration too."
                .into();
        };
        let client_id = what[client_pos + "client ".len()..].trim();
        if client_id.is_empty() {
            return "I have the address, but the client ID looks empty.".into();
        }
        let address = address.to_string();
        let client_id = client_id.to_string();
        self.connect_outlook(&address, &client_id)
    }

    /// Starts the device code flow: one fast request for a code, an
    /// immediate honest answer (the code and where to enter it), and a
    /// crew errand that polls in the background — for as long as fifteen
    /// minutes, per Microsoft's own `expires_in` — until you've actually
    /// gone and approved it somewhere else. Nothing here drives a
    /// browser; that's the entire reason this flow exists instead of a
    /// redirect-based one.
    fn connect_outlook(&mut self, address: &str, client_id: &str) -> String {
        let dc = match crate::msoauth::request_device_code(client_id) {
            Ok(dc) => dc,
            Err(e) => return format!("Couldn't start connecting that account: {e}"),
        };
        let now = crate::store::now();
        let client_id_owned = client_id.to_string();
        let device_code = dc.device_code.clone();
        let mut wait_secs = dc.interval.max(5);
        let expires_in = dc.expires_in;

        let work: crew::Work = Box::new(move |stop| {
            let started = std::time::Instant::now();
            loop {
                if stop.checkpoint() {
                    return Err("cancelled".to_string());
                }
                if started.elapsed().as_secs() >= expires_in {
                    return Err("the code expired before it was approved".to_string());
                }
                std::thread::sleep(std::time::Duration::from_secs(wait_secs));
                match crate::msoauth::poll_once(&client_id_owned, &device_code) {
                    crate::msoauth::PollOutcome::Ready(tokens) => return Ok(tokens.refresh_token),
                    crate::msoauth::PollOutcome::Pending => {}
                    // The server's own request to poll less often — not a
                    // fixed guess, since a fixed backoff either ignores
                    // this or reinvents it worse.
                    crate::msoauth::PollOutcome::SlowDown => wait_secs += 5,
                    crate::msoauth::PollOutcome::Denied => {
                        return Err("you declined the sign-in".to_string())
                    }
                    crate::msoauth::PollOutcome::Expired => {
                        return Err("the code expired before it was approved".to_string())
                    }
                    crate::msoauth::PollOutcome::Other(e) => return Err(e),
                }
            }
        });

        // The client id travels with the address, so the account can be
        // added once connected (30 Sep 2026: it ended "edit tools.yaml").
        if self.hand_off("outlook-connect", now, work, Some(format!("{address} {client_id}")), SpeakPolicy::Always) {
            dc.message
        } else {
            "I'm swamped with background work right now — try connecting that account again in a moment."
                .into()
        }
    }

    /// "What's up with my Amazon order" / "where's my cable." Reads the
    /// order store directly, the same reasoning as `read_draft`: local
    /// data, already fetched, never needs the crew.
    pub(super) fn read_order(&mut self, what: &str) -> String {
        let orders = crate::orders::Orders::load(&self.store);
        match orders.find(what) {
            Some(order) => {
                format!("{} — {}: {}", order.merchant, order.description, order.status.spoken())
            }
            None => "I don't have an order matching that.".into(),
        }
    }

    /// "Pull up the reply to Jane." Reads the outbox directly — this is
    /// always fast (one file, already local), so unlike `check_mail` it
    /// never needs the crew or an acknowledgment first.
    pub(super) fn read_draft(&mut self, who: &str) -> String {
        if who.is_empty() {
            return "Pull up the draft to who?".into();
        }
        let outbox = crate::outbox::Outbox::load(&self.store);
        match outbox.waiting_for(who) {
            Some(reply) => format!(
                "Reply to {}: \"{}\"",
                reply.to_name,
                reply.body.trim()
            ),
            None => format!("I don't have a draft waiting for {who}."),
        }
    }

    /// Drafted replies by voice, before anything else reads the sentence:
    /// "send the reply to Jane", "send it" (after one was read out), "read
    /// me the draft to Jane", "what drafts are waiting", "throw away the
    /// reply to Jane".
    ///
    /// 30 Sep 2026: a reply Atlas drafted could be looked at and thrown away
    /// and never sent -- `Outbox::mark_sent` had no caller -- so drafts sat
    /// "waiting" for ever.
    pub(super) fn drafts_help(&mut self, said: &str) -> Option<String> {
        let t: String = said.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '@' || c == '.' || c == ' ' { c } else { ' ' }).collect();
        let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
        let about_drafts = t.contains("reply") || t.contains("draft");
        let who = t.rsplit_once(" to ").map(|(_, w)| w.trim().to_string()).unwrap_or_default();
        if about_drafts && (t.contains("what drafts") || t.contains("my drafts") || t.contains("drafts waiting") || t.contains("any drafts")) {
            let outbox = crate::outbox::Outbox::load(&self.store);
            let waiting = outbox.waiting();
            return Some(if waiting.is_empty() {
                "No drafts waiting.".into()
            } else {
                format!("Waiting: {}.", waiting.iter().map(|r| format!("a reply to {}", r.to_name)).collect::<Vec<_>>().join("; "))
            });
        }
        let sending = t.starts_with("send") && (about_drafts || t == "send it" || t == "send that" || t == "send it off");
        if sending {
            let pick = if who.is_empty() { self.draft_last_read.clone() } else { Some(who) };
            return Some(self.send_draft(pick.as_deref()));
        }
        if about_drafts && (t.starts_with("read") || t.starts_with("pull up") || t.starts_with("show")) && !who.is_empty() {
            let reply = self.read_draft(&who);
            if reply.starts_with("Reply to") {
                self.draft_last_read = Some(who);
                return Some(format!("{reply} Say \"send it\" and it goes."));
            }
            return Some(reply);
        }
        if about_drafts && ["throw away", "discard", "scrap", "delete"].iter().any(|v| t.starts_with(v)) && !who.is_empty() {
            return Some(self.discard_draft(&who));
        }
        None
    }

    /// "Email Sam saying I'll be late": a draft to them, held until you say
    /// "send it" -- never sent on its own (30 Sep 2026: nothing started an
    /// email; the model claimed to have sent one). The address is what you
    /// said, or the one you gave for them ("Sam's email is ..."), or the one
    /// your mail has with them; a name that could be two people is asked
    /// about, not guessed.
    pub(super) fn compose_help(&mut self, said: &str) -> Option<String> {
        let (who, message) = crate::outbox::email_asked(said)?;
        let cfg = self.tools_cfg().mail.clone();
        let Some(account) = cfg.accounts.first().cloned() else {
            return Some(format!(
                "I can't email {who} yet -- there's no mail account set up. Add one in Settings, under Mail, and ask me again."
            ));
        };
        let (name, address) = if who.contains('@') {
            (who.clone(), who.to_lowercase())
        } else {
            let people = self.people_known().clone();
            let known = match people.find(&who) {
                crate::people::Found::One(k) => people.by_key.get(k).and_then(|c| c.emails.first().map(|e| (c.name.clone(), e.clone()))),
                crate::people::Found::Several(names) => {
                    return Some(format!("Which {who}? {}.", names.join(" or ")));
                }
                crate::people::Found::None => None,
            };
            match known.or_else(|| {
                let book: crate::mailbook::MailBook = self.store.load(crate::mailbook::MailBook::FILE);
                address_in_mail(&book, &who).map(|a| (who.clone(), a))
            }) {
                Some(found) => found,
                None => {
                    return Some(format!(
                        "I don't have an email address for {who}. Tell me \"{who}'s email is\" and the address, then ask again."
                    ))
                }
            }
        };
        let body = crate::outbox::body_from_spoken(&message);
        let now = crate::store::now();
        let draft = crate::outbox::PendingReply {
            id: crate::outbox::Outbox::make_id(&account.name, &address, now),
            account: account.name.clone(),
            to_address: address.clone(),
            to_name: name.clone(),
            subject: crate::outbox::subject_from_body(&body),
            body: body.clone(),
            kind: crate::outbox::Kind::Client,
            critique: Vec::new(),
            created_at: now,
            status: crate::outbox::Status::Waiting,
        };
        let mut outbox = crate::outbox::Outbox::load(&self.store);
        outbox.add(draft);
        if let Err(e) = outbox.save(&self.store) {
            return Some(format!("I wrote it but couldn't keep the draft ({e}), so nothing's waiting to send."));
        }
        self.draft_last_read = Some(address.clone());
        Some(format!("To {name} ({address}): \"{body}\" Say \"send it\" and it goes, or \"scrap the draft to {name}\"."))
    }

    /// Send a waiting draft: to `who` (a name or address), or the only one
    /// waiting. On the crew, because it's a network call.
    fn send_draft(&mut self, who: Option<&str>) -> String {
        let cfg = self.tools_cfg().mail.clone();
        let outbox = crate::outbox::Outbox::load(&self.store);
        let pending = match who {
            Some(w) => outbox.waiting_for(w).cloned(),
            None => match outbox.waiting().as_slice() {
                [only] => Some((*only).clone()),
                [] => return "There's no draft waiting to send.".into(),
                many => {
                    return format!(
                        "Which one? {}.",
                        many.iter().map(|r| format!("the reply to {}", r.to_name)).collect::<Vec<_>>().join(", ")
                    )
                }
            },
        };
        let Some(pending) = pending else {
            return format!("I don't have a draft waiting for {}.", who.unwrap_or("them"));
        };
        let Some(account) = cfg.accounts.iter().find(|a| a.name == pending.account).or(cfg.accounts.first()).cloned() else {
            return "There's no mail account set up to send it from.".into();
        };
        let now = crate::store::now();
        let password = match crate::mail::credential_source(&account)
            .map(|n| n.to_string())
            .map_err(|e| e.to_string())
            .and_then(|n| self.vault.get(&n, now).map_err(|e| e.to_string()))
        {
            Ok(p) => p,
            Err(e) => return format!("I can't send from {} yet: {e}. It's still waiting.", account.name),
        };
        let store = self.store.clone();
        let to = pending.to_name.clone();
        let work: crew::Work = Box::new(move |_ctl| {
            send_reply(&pending, &account.address, &password, account.oauth.then_some(account.client_id.as_str()))
                .map_err(|e| format!("the reply to {} didn't go: {e}. It's still waiting.", pending.to_name))?;
            let mut outbox = crate::outbox::Outbox::load(&store);
            outbox.mark_sent(&pending.id);
            outbox.save(&store).map_err(|e| format!("it went, but I couldn't note that it did ({e})"))?;
            Ok(format!("Sent the reply to {}.", pending.to_name))
        });
        self.draft_last_read = None;
        if self.hand_off("send reply", now, work, Some(to.clone()), SpeakPolicy::Always) {
            format!("Sending the reply to {to}.")
        } else {
            "I'm swamped with background work right now -- ask me again in a moment.".into()
        }
    }

    /// "Throw away the reply to Jane." / "Scrap the draft to Jane." You
    /// looked at a held draft and said no, so it is marked `Discarded` and
    /// leaves `waiting()`/`waiting_for()`. Without this there was no way to
    /// reject a held draft: it stayed `Waiting` for ever, so "pull up the
    /// reply to Jane" kept surfacing the thing you'd already turned down and
    /// the count of waiting drafts never came down. `read_draft` let you look
    /// at one; this is how you say no to one.
    pub(super) fn discard_draft(&mut self, who: &str) -> String {
        if who.is_empty() {
            return "Throw away the draft to who?".into();
        }
        let mut outbox = crate::outbox::Outbox::load(&self.store);
        let Some(id) = outbox.waiting_for(who).map(|r| r.id.clone()) else {
            return format!("I don't have a draft waiting for {who}.");
        };
        let to_name = outbox.get(&id).map(|r| r.to_name.clone()).unwrap_or_else(|| who.to_string());
        outbox.mark_discarded(&id);
        if let Err(e) = outbox.save(&self.store) {
            return format!("I couldn't set that draft aside: {e}");
        }
        format!("Thrown away — the reply to {to_name} won't go out.")
    }

    /// "Draft outreach to brand@example.com about a partnership." Pulls
    /// the recipient out of the request; everything after "about" is the
    /// purpose, handed to the model as-is rather than parsed further.
    pub(super) fn draft_outreach_from_request(&mut self, what: &str) -> String {
        let Some(rest) = what.split("to ").nth(1) else {
            return "Outreach to whom?".into();
        };
        let (address, purpose) = match rest.split_once("about ") {
            Some((a, p)) => (a.trim(), p.trim().to_string()),
            None => (rest.trim(), "introducing yourself and exploring working together".to_string()),
        };
        if address.is_empty() {
            return "Outreach to whom?".into();
        }
        self.draft_outreach(address, &purpose)
    }

    /// The cold-outreach half of the drafting feature. Always goes to the
    /// crew — the model call is the same class of slow as `research`'s —
    /// and, unlike a client reply, has nowhere reactive to hang off: this
    /// only ever runs because you asked for it by name.
    fn draft_outreach(&mut self, to_address: &str, purpose: &str) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if !cfg.enabled {
            return "Reading your email is switched off.".into();
        }
        let Some(account) = cfg.accounts.first().cloned() else {
            return "I don't have any mail accounts set up yet.".into();
        };
        let Some(llm) = self.background_llm() else {
            return "I need a model to draft that, and I haven't got one configured.".into();
        };
        let now = crate::store::now();
        let vault_name = match crate::mail::credential_source(&account) {
            Ok(n) => n.to_string(),
            Err(e) => return format!("Can't draft from {}: {e}", account.name),
        };
        let password = match self.vault.get(&vault_name, now) {
            Ok(p) => p,
            Err(e) => return format!("Can't draft from {}: {e}", account.name),
        };

        let store = self.store.clone();
        let draft_cfg = self.tools_cfg().draft.clone();
        let may_email_brands = cfg.may_email_brands;
        let daily_cap = cfg.cold_outreach_daily_cap;
        let to_address = to_address.to_string();
        let purpose = purpose.to_string();
        let to_address_for_ack = to_address.clone();

        let work: crew::Work = Box::new(move |ctl| {
            if ctl.checkpoint() {
                return Err("stopped before drafting".into());
            }
            let system = "You draft a short, professional cold outreach email on behalf of the \
                          person you work for, to someone who has never heard from them before. \
                          Write only the reply body -- no subject line, no signature, no \
                          placeholder brackets. Keep it brief and respectful of their time.";
            let user = format!("Write an outreach email to {to_address} about: {purpose}");
            let body = llm.complete(system, &user).map_err(|e| e.to_string())?;
            let notes = crate::draft::critique(&body, None, &draft_cfg);
            let created = crate::store::now();
            let id = crate::outbox::Outbox::make_id(&account.name, &to_address, created);
            let mut pending = crate::outbox::PendingReply {
                id,
                account: account.name.clone(),
                to_address: to_address.clone(),
                to_name: to_address.clone(),
                subject: "Introduction".into(),
                body,
                kind: crate::outbox::Kind::ColdOutreach,
                critique: notes,
                created_at: created,
                status: crate::outbox::Status::Waiting,
            };

            let mut outbox = crate::outbox::Outbox::load(&store);
            let targets = crate::outreach::OutreachTargets::load(&store);
            let today_start = crate::localclock::midnight(created, crate::localclock::offset_secs());
            let sent_today = outbox.cold_outreach_sent_since(today_start);

            let said = if !may_email_brands {
                format!("There's an outreach draft to {to_address} ready to look at.")
            } else if !targets.is_approved(&to_address) {
                format!(
                    "There's an outreach draft to {to_address} ready, but that recipient isn't \
                     on your approved outreach list yet."
                )
            } else if sent_today >= daily_cap as usize {
                format!(
                    "There's an outreach draft to {to_address} ready, but today's outreach cap \
                     ({daily_cap}) is already reached."
                )
            } else {
                match send_reply(
                    &pending,
                    &account.address,
                    &password,
                    account.oauth.then_some(account.client_id.as_str()),
                ) {
                    Ok(()) => {
                        pending.status = crate::outbox::Status::Sent;
                        format!("Sent the outreach to {to_address}.")
                    }
                    Err(e) => format!("Tried to send the outreach to {to_address}, but: {e}"),
                }
            };
            outbox.add(pending);
            let _ = outbox.save(&store);
            Ok(said)
        });

        if self.hand_off("outreach", now, work, None, SpeakPolicy::Always) {
            format!("Drafting outreach to {to_address_for_ack}. I'll let you know when it's ready.")
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }
}

/// The address your mail has with someone of this name: the latest letter
/// from them, or to them.
fn address_in_mail(book: &crate::mailbook::MailBook, who: &str) -> Option<String> {
    let w = who.to_lowercase();
    book.letters.iter().rev().find_map(|l| {
        if !l.mine && (l.from_name.to_lowercase() == w || l.from_name.to_lowercase().split_whitespace().next() == Some(w.as_str()) && !w.contains(' ')) {
            return Some(l.from.clone());
        }
        None
    })
}
