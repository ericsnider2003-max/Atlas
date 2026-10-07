//! Around a turn: abilities, subjects, settings, apps, zones, finding files.
//!
//! Moved out of `turn.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl<'a> Daemon<'a> {
    /// Is there work in hand -- queued in a lane, or a workflow mid-run?
    ///
    /// Public because the busy signal is a fact about the daemon that the
    /// outside is entitled to ask, and the tests that hold the signal honest
    /// ask it here rather than reaching into the queue and re-deriving it.
    pub fn queue_is_busy(&self) -> bool {
        // A workflow mid-run is work in hand, exactly as queued work is --
        // it just lives in `current_flow` instead of a lane.
        self.queue.pending() > 0 || self.current_flow.is_some()
    }

    /// A request for a new ability, its yes or no, or the list (`growth`).
    pub(in crate::daemon) fn ability_request(&mut self, said: &str, t: u64) -> Option<String> {
        let mut wanted: crate::growth::WantedAbilities = self.store.load(crate::growth::STORE);
        if crate::growth::asks_for_the_list(said) {
            return Some(wanted.spoken());
        }
        if crate::growth::asks_to_set_up(said) {
            let Some(w) = crate::growth::latest_approved(&wanted) else {
                return Some("There's no approved ability to set up. Say \"approve that ability\" first.".into());
            };
            let scfg = self.tools_cfg().self_work.clone();
            let root = crate::selfwork::source_root(&scfg).unwrap_or_default();
            if !crate::selfwork::is_a_source_checkout(&root) {
                return Some("That needs my source code on this computer, and there isn't a copy here.".into());
            }
            let (y, m, d) = crate::hubpages::ymd((t / 86_400) as i64);
            const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let ask = crate::scaffold::Ask {
                id: crate::scaffold::name_for(&w.what),
                what: w.what.clone(),
                day: format!("{d} {} {y}", MONTHS[(m as usize).saturating_sub(1).min(11)]),
            };
            return Some(match crate::scaffold::on_a_branch(&root, &ask) {
                Ok(branch) => format!(
                    "Set up \"{}\" on a branch of its own, {branch} -- not installed, not pushed, main untouched. {}",
                    w.what,
                    crate::scaffold::left_to_do(&ask)
                ),
                Err(why) => format!("I couldn't set it up: {why}."),
            });
        }
        if let Some(state) = crate::growth::answer(said) {
            if let Some(what) = wanted.decide_latest(state) {
                let _ = self.store.save(crate::growth::STORE, &wanted);
                return Some(match state {
                    crate::growth::State::Approved => format!(
                        "Approved: \"{what}\". It's on the build list now -- the next build picks it up, and I'll tell you when it's in. \
                         Say \"set that ability up\" and I'll do its bookkeeping in my source now, on a branch of its own."
                    ),
                    _ => format!("Left it: \"{what}\" won't be built."),
                });
            }
            return None;
        }
        let what = crate::growth::asks_for_an_ability(said)?;
        // "Work that so you have this capability": the ability is whatever
        // you were just asking about -- your previous sentence.
        let what = if what.trim().is_empty() {
            match self.thread.recent.iter().rev().map(|e| e.said.trim()).find(|s| !s.is_empty() && crate::growth::asks_for_an_ability(s).is_none()) {
                Some(before) => before.trim_end_matches(['.', '?', '!']).to_string(),
                None => return Some("Which ability? Say \"give yourself the ability to\" and what it is, and I'll write it down for your yes.".into()),
            }
        } else {
            what
        };
        wanted.ask(&what, t);
        let _ = self.store.save(crate::growth::STORE, &wanted);
        self.log.info(&format!("ability asked for: {what}"));
        Some(crate::growth::noted_beside(&what, &crate::growth::already_close(&what, &crate::capability::all())))
    }

    /// What "this" refers to right now.
    ///
    /// Returns a question when it genuinely can't tell, which stops the
    /// intent rather than guessing at it.
    pub(in crate::daemon) fn resolve_subject(&mut self, intent: &Intent) -> Option<String> {
        let arg = match intent {
            Intent::Research(a) | Intent::DraftPost(a) | Intent::Capture(a) | Intent::Files(a) => a,
            // `ReviewPost`'s argument is the actual text being reviewed, not
            // a bare reference to it — "review this post: I love it when
            // things work" has "it" and "this" in it as ordinary English,
            // not as something to resolve. Treating it the same as
            // "explain this" meant any post whose own wording happened to
            // contain "it", "this" or "that" anywhere was silently
            // hijacked into a clipboard/selection question instead of ever
            // being reviewed at all.
            Intent::ReviewPost(_) => return None,
            // "Explain this" with UseClipboard already says where "this" is.
            Intent::UseClipboard(_) => return None,
            _ => return None,
        };
        // Only an argument that *is* a reference -- "research this", "note
        // that", "find it" -- needs resolving. One that merely contains a
        // pronoun is ordinary English (1 Oct 2026: "do some research on
        // things that would allow you to advance your own capabilities" got
        // "I can't tell what you mean -- nothing copied, nothing selected",
        // and so did "... when it gets approval").
        if !crate::references::argument_leans_on_earlier(arg) {
            return None;
        }
        // Only what the daemon genuinely knows. Filling these in with guesses
        // would make the resolver confident about nothing — and leaving the
        // clipboard out made it resolve to nothing at all, which broke
        // "explain this".
        //
        // `only_on_request` (on by default) means Atlas does not reach for
        // what you copied unless your words point at it — "explain this" when
        // "this" could be the clipboard, not every pronoun. The predicate
        // written for exactly this (`refers_to_clipboard`) had no caller, so
        // the setting was dead and the clipboard was silently in scope for
        // every resolution.
        let use_clipboard = !self.clipboard_cfg().only_on_request
            || crate::clipboard::refers_to_clipboard(arg);
        let candidates = crate::subject::Candidates {
            clipboard: if use_clipboard { self.clipboard_text.clone() } else { None },
            ..Default::default()
        };
        match crate::subject::resolve(arg, &candidates) {
            // Two equally likely things is a question, not a coin toss.
            crate::subject::Resolution::Ambiguous { question, .. } => Some(question),
            crate::subject::Resolution::Nothing(why) => Some(why),
            crate::subject::Resolution::Found { .. } => None,
        }
    }

    /// Something learned. Merges with what's already known rather than
    /// adding a second copy.
    ///
    /// The point: asking about the same thing twice, worded differently,
    /// shouldn't cost you what you knew. Two notes means two decay curves and
    /// confidence in a settled fact drifting down because you happened to ask
    /// again.
    pub fn learned(&mut self, says: &str, source: &str, now: u64) -> bool {
        let strengthened = crate::consolidate::learn(&mut self.known, says, source, now);

        let cfg = self.tools_cfg().consolidate.clone();
        // Squeeze the long ones first, drop only if that isn't enough, and
        // keep a line for whatever goes (H10: memory keeps a stub of what it
        // forgot).
        let (_squeezed, dropped) =
            crate::consolidate::make_room(&mut self.known, &mut self.stones, cfg.keep_at_most, now);
        if !dropped.is_empty() {
            let _ = self.store.save("known_stones", &self.stones);
        }
        if let Some(note) = crate::consolidate::dropped_note(&dropped) {
            // Said rather than done quietly — a store that silently forgets is
            // one you stop trusting.
            self.history.note(&note, "knowledge",
                crate::undo::Undo::Cannot("what was dropped is gone".into()), false, now);
        }
        // Trim holds settled facts and things about your own setup out of the
        // budget entirely, because they cannot be looked up again. When those
        // alone exceed the cap, nothing is dropped and the store sits over
        // budget on purpose -- and until now, silently. A cap quietly exceeded
        // is a cap doing nothing, so which of the two it is gets said.
        if let Some(note) =
            crate::consolidate::over_budget_on_purpose(&self.known, cfg.keep_at_most)
        {
            self.history.note(&note, "knowledge",
                crate::undo::Undo::Cannot(
                    "nothing was dropped; the budget is exceeded by facts that can't be relearned"
                        .into(),
                ), false, now);
        }
        strengthened
    }

    /// Turn what Atlas records about itself into signals it can act on.
    ///
    /// The gap this closes: `selfaudit` could rank signals and had nothing
    /// producing them. A self-audit with an empty input list reports that
    /// everything is fine, which is the most misleading possible answer.
    pub fn refresh_signals(&mut self) {
        // What requests have actually used (`used`, 30 Sep 2026). Only
        // abilities there's a way to ask for, and only after two weeks of
        // counting: before that it's empty and `from_unused` says nothing
        // (29 Sep: every working ability was listed as never called, with a
        // "Have a go" that could never be done).
        let never_used = self.used.unasked(crate::store::now());
        let total = crate::capability::all().len() as u32;
        self.signals = crate::signals::gather(
            &self.history,
            self.unknown_count,
            self.utterance_count,
            &self.last_unknown,
            &never_used,
            total,
        );
        // What the last mutation run found no test catching (`mutation`).
        let survivors: Vec<crate::mutation::Survivor> = self.store.load(crate::mutation::KEPT);
        self.signals.extend(crate::signals::from_survivors(&survivors));
        // What the last self-test found broken (`regressions`).
        let cases: Vec<crate::regressions::Case> = self.store.load(crate::regressions::FILE);
        self.signals.extend(crate::signals::from_regressions(&cases));
        // What a coverage run of the self-test never reached (`coverage`).
        let reports = crate::selftest::reports_dir(&self.store.install_root());
        if let Ok(json) = std::fs::read_to_string(reports.join(crate::coverage::NEVER_REACHED)) {
            if let Ok((paths, total)) = serde_json::from_str::<(Vec<String>, u32)>(&json) {
                self.signals.extend(crate::signals::from_never_reached(&paths, total));
            }
        }
    }

    /// Anything that was running when the machine stopped.
    ///
    /// Resolved before the brief, not during it — you sat down to get on with
    /// something, and a system that opens with questions about last night has
    /// answered the wrong one. Bounded: anything it can't judge inside the
    /// budget is left alone and mentioned, never asked about.
    pub(in crate::daemon) fn settle_interrupted(&mut self) -> Vec<String> {
        let mut said = Vec::new();
        let jobs = std::mem::take(&mut self.interrupted);
        for (what, checked, reversible) in jobs {
            let decision = crate::awake::on_waking_checked(&checked, reversible, 0);
            said.push(crate::awake::woke_checked(&what, decision, &checked));
        }
        said
    }

    /// Should the machine be kept awake right now?
    ///
    /// Only while something is actually running, and never with the lid shut
    /// or the battery low. Called by whatever is running rather than held
    /// open by the daemon.
    pub fn keep_awake(&self, why: crate::awake::Because, power: &crate::awake::Power, held_mins: u32)
        -> (crate::awake::Hold, String)
    {
        crate::awake::decide(why, power, held_mins, &self.tools_cfg().awake)
    }

    /// Something didn't work. Is there another way?
    ///
    /// Reporting a failure is what a program does. Trying the next route is
    /// what an assistant does, and the module for it has been sitting there
    /// unreachable.
    pub fn another_way(&self, kind: crate::route::Kind, failed: &str) -> Option<String> {
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        let next = crate::route::known_routes()
            .into_iter()
            .filter(|r| r.for_what == kind)
            .filter(|r| r.name != failed)
            .filter(|r| online || !crate::route::needs_internet(r))
            .max_by(|a, b| {
                a.reliability.partial_cmp(&b.reliability).unwrap_or(std::cmp::Ordering::Equal)
            })?;
        Some(next.name)
    }

    /// The apps Atlas has configured, by name — what `grants::check` counts
    /// as "known", and what `grant_in_instruction` matches a named tool
    /// against.
    pub(super) fn known_app_names(&self) -> Vec<String> {
        self.cfg.apps.apps.keys().cloned().collect()
    }

    /// What Atlas knows about an app, for the permission check. `known` is
    /// simply whether it is in the configured apps; `confirm_each_time` is the
    /// per-app "ask every single time" flag, which today only the no-input
    /// apps (Discord and its kind) carry — a stray keystroke there is public
    /// and permanent, so touching one is always a question.
    pub(in crate::daemon) fn app_facts(&self, app: &str) -> crate::grants::AppFacts {
        let spec = self.cfg.apps.apps.iter().find(|(name, _)| name.eq_ignore_ascii_case(app));
        crate::grants::AppFacts {
            known: spec.is_some(),
            confirm_each_time: spec.map(|(_, s)| s.no_input).unwrap_or(false),
        }
    }

    /// The permission gate for acting on an app. A configured app Atlas
    /// already knows sails straight through — the gate is not a nag, it is
    /// the "I don't know this one, may I?" question rule 1 of the grants
    /// module describes, plus the confirm-every-time apps. On a question it
    /// parks the action as a pending approval so the next thing you say is
    /// the answer; on a yes there, the grant is recorded (with the breadth
    /// you gave — once, this session, or always) before the action runs, so
    /// the re-check finds it and does not ask twice.
    pub(in crate::daemon) fn gate_app(&mut self, app: &str, action: &str, intent: &Intent) -> AppGate {
        let facts = self.app_facts(app);
        match self.permissions.check(app, action, &facts) {
            crate::grants::Verdict::Allowed(_) => {
                // A one-off grant is spent the moment it is used, so the next
                // action on the same app asks again.
                self.permissions.consume(app, action);
                AppGate::Go
            }
            crate::grants::Verdict::Ask(question) => {
                self.session.await_approval(intent.clone(), &question);
                AppGate::Ask(question)
            }
        }
    }

    /// What Atlas can still do with the network unplugged.
    pub fn offline_coverage(&self, kind: crate::route::Kind) -> String {
        let (offline, all) = crate::route::coverage(kind);
        format!("{offline} of {all} ways work with no internet.")
    }

    /// Is this something Atlas already knows how to do?
    ///
    /// Checked before falling back to the model — a procedure Atlas has
    /// written down beats a guess, and it works with the network unplugged.
    pub(in crate::daemon) fn known_procedure(&self, asked: &str) -> Option<String> {
        let book = crate::knowhow::Knowhow::load(&self.store);
        // Offline is the normal case, not the exception, so a procedure that
        // needs the internet is filtered out rather than offered and failed.
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        book.for_request(asked, online)
            .map(|p| crate::knowhow::announce(p, online))
    }

    /// A symptom you describe, matched to a known snag and its fix.
    ///
    /// The other half of `known_procedure`: that answers "how do I do X" from
    /// the procedures' goals; this reads `knowhow::for_symptom`, which scores
    /// what you say against every procedure's *snags* -- the things that
    /// usually go wrong -- and names the likely cause and what to do. Offline
    /// by construction, because the snags ship compiled in. Named
    /// `diagnose_symptom` rather than `diagnose` on purpose: `diagnose.rs`
    /// already owns the bare name `diagnose` for the machine-vitals check, and
    /// the reachability scan matches by bare name.
    pub(in crate::daemon) fn diagnose_symptom(&self, symptom: &str) -> String {
        let book = crate::knowhow::Knowhow::load(&self.store);
        match book.for_symptom(symptom) {
            Some((p, snag)) => format!(
                "That's the sort of thing I've run into ({}): usually {}. \
                 What I'd do: {}. It comes up while trying to {}.",
                snag.looks_like, snag.cause, snag.fix, p.goal
            ),
            None if symptom.trim().is_empty() => {
                "Tell me what you're actually seeing and I'll check whether I know \
                 the cause -- something like \"an app that launches and closes \
                 immediately\"."
                    .into()
            }
            None => format!(
                "I can't match \"{}\" to any snag I've run into before. Describe \
                 what's on screen a bit differently and I'll look again.",
                symptom.trim()
            ),
        }
    }

    /// A task you name, read back as the steps to follow.
    ///
    /// The other side of `known_procedure`: that finds the same procedure with
    /// `for_request` but only says `announce` -- "I know this one, N steps" --
    /// and never the steps. This is the caller `knowhow::as_plan` never had,
    /// so a match becomes a numbered plan you can actually work through.
    /// Offline by construction: the procedures ship compiled in, and one that
    /// needs the internet is filtered out rather than offered and failed.
    ///
    /// Named `walk_me_through` rather than any bare `plan`/`steps` on purpose:
    /// the reachability scan matches by bare name, and this name is its own.
    pub(in crate::daemon) fn walk_me_through(&self, asked: &str) -> String {
        let asked = asked.trim();
        if asked.is_empty() {
            return "Walk you through what? Name the task -- \"walk me through \
                    freeing up memory\", \"how do I open something that won't \
                    launch\"."
                .into();
        }
        let book = crate::knowhow::Knowhow::load(&self.store);
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        match book.for_request(asked, online) {
            Some(p) => {
                let mut s = format!("{} -- here's how:\n{}\n", p.goal, crate::knowhow::checklist(p));
                for (n, step) in crate::knowhow::as_plan(p).into_iter().enumerate() {
                    s.push_str(&format!("  {}. {}\n", n + 1, step));
                }
                s
            }
            // Not one of Atlas's own procedures -- "how do I make pancakes"
            // is a question, and it gets an answer when there's a model to
            // give one, not a pointer at a troubleshooting command (26 Sep
            // 2026).
            None => {
                if let Some(llm) = self.llm.clone() {
                    let system = format!(
                        "{}\n\n{}",
                        self.persona_now().prompt_for(crate::register::Register::Working),
                        crate::brain::TALK
                    );
                    if let Some(said) = llm
                        .complete(&system, &format!("{}\nUser said: how do I {asked}", crate::capability::about_atlas(asked, 6)))
                        .ok()
                        .and_then(|r| crate::brain::spoken_text(&r))
                    {
                        return said;
                    }
                }
                format!(
                    "I don't have steps for \"{asked}\" written down, and questions like that need my \
                     language model, which isn't running here. If something's gone wrong, tell me what \
                     you're seeing -- \"troubleshoot ...\" -- and I'll see if I know the cause."
                )
            }
        }
    }

    /// Before saying something, is it worth saying?
    ///
    /// The one gate everything unprompted goes through. Without it every
    /// feature politely announces itself and the sum is a system that talks
    /// constantly about nothing.
    pub fn worth_saying(&mut self, thing: &crate::interrupt::Thing, doing: crate::interrupt::Doing) -> Option<String> {
        let mut cfg = self.tools_cfg().interrupt.clone();
        // What you wrote in the config, plus what you said out loud. Two
        // lists because they come from two places and neither should silently
        // overwrite the other -- `interrupt::Muted` has the argument.
        cfg.muted.extend(crate::interrupt::Muted::load(&self.store).topics);
        match self.gate.consider(thing, doing, &cfg, clock()) {
            crate::interrupt::Decision::Say(s) => Some(s),
            _ => None,
        }
    }

    /// How sure is the answer, and should it say so?
    pub(in crate::daemon) fn hedge(&self, answer: &str, grounding: crate::certainty::Grounding) -> String {
        let cfg = self.tools_cfg().certainty.clone();
        let (confidence, _, why) = crate::certainty::assess(answer, &grounding, &cfg);
        crate::certainty::phrase(answer, confidence, &why)
    }

    /// The tools config, or its defaults when there isn't one.
    ///
    /// Everything is off by default, so a missing tools.yaml means Atlas does
    /// less rather than more.
    /// Your time zone: the `time_zone` setting if you've chosen one, and
    /// this computer's own clock if you haven't (`tz::home`).
    pub fn home_zone(&self) -> crate::tz::Zone {
        crate::tz::home(self.tools_ref().map(|t| t.time_zone.as_str()).unwrap_or(""))
    }

    /// The zone you chose, or `None` for "this computer's clock": what
    /// `localclock` is told, so the hub and the calendar agree.
    pub(super) fn chosen_zone(&self) -> Option<crate::tz::Zone> {
        let set = self.tools_ref().map(|t| t.time_zone.trim().to_string()).unwrap_or_default();
        (!set.is_empty() && !set.eq_ignore_ascii_case("automatic")).then(|| crate::tz::home(&set))
    }

    /// Your settings as they stand now: the ones Atlas started with, or the
    /// ones it has picked up since. Every read of a setting goes through here,
    /// which is what lets a change apply without a restart.
    pub(crate) fn tools_ref(&self) -> Option<&crate::voice::ToolsConfig> {
        self.tools_live.as_ref().or(self.cfg.tools.as_ref())
    }

    /// A security, vault or confirmation step is waiting on you: no wit
    /// until it's done (`wit::holds_back`).
    pub(in crate::daemon) fn mid_flow(&self) -> bool {
        self.pending_security.is_some()
            || self.pending_signin.is_some()
            || self.pending_window_confirm.is_some()
            || self.pending_post_approval.is_some()
            || self.pending_press.is_some()
            || self.pending_offer.is_some()
    }

    /// The settings folder being watched, when Atlas runs for real: where a
    /// setting changed by voice is kept (`talkback`, `hunting`).
    pub(crate) fn settings_dir(&self) -> Option<std::path::PathBuf> {
        self.settings_watch.as_ref().map(|(d, _)| d.clone())
    }

    /// Watch this folder's settings, so a change made in the settings window,
    /// the hub, or by hand is picked up while Atlas runs.
    pub fn watch_settings(mut self, config_dir: std::path::PathBuf) -> Self {
        let seen = settings_fingerprint(&config_dir);
        self.settings_stamp = settings_stamp(&config_dir);
        self.settings_watch = Some((config_dir, seen));
        self
    }

    /// Pick up a change to your settings, if there's been one since the last
    /// look. Returns one line per setting that changed, saying whether it
    /// applies now or when Atlas next starts.
    ///
    /// Cheap enough for every tick: two files' sizes and modified times are
    /// looked at, and only when one of those has moved are the files read and
    /// hashed (27 Sep 2026 -- they used to be read and hashed every tick).
    /// Nothing is reloaded unless the contents differ from last time.
    pub fn pick_up_settings(&mut self) -> Vec<String> {
        let Some((dir, seen)) = self.settings_watch.clone() else { return Vec::new() };
        let stamp = settings_stamp(&dir);
        if stamp == self.settings_stamp {
            return Vec::new();
        }
        self.settings_stamp = stamp;
        let now = settings_fingerprint(&dir);
        if now == seen {
            return Vec::new();
        }
        let fresh = match crate::config::Config::load(&dir) {
            Ok(c) => c.tools.map(|t| t.anchored()),
            Err(e) => {
                // Half-written or hand-broken: keep running on what we have,
                // look again next tick, and say why once.
                self.settings_watch = Some((dir, now));
                let line = format!("I couldn't read my changed settings, so I'm keeping the ones I had: {e}");
                self.log.warn(&line);
                return vec![line];
            }
        };
        self.settings_watch = Some((dir.clone(), now));
        // Your settings file is applied quietly over tools.yaml, so one that
        // won't read would otherwise just look like every choice reverting.
        if let Err(e) = crate::preferences::Preferences::load_checked(&dir) {
            let line = format!("I couldn't read your settings file, so I'm using the defaults until it's fixed: {e}");
            self.log.warn(&line);
            return vec![line];
        }
        let Some(mut fresh) = fresh else { return Vec::new() };
        let before = self.tools_ref().cloned().unwrap_or_default();
        // `vars` aren't settings — nothing on the settings page changes them —
        // and some hold what Atlas worked out for itself at the start (the
        // microphone it picked, over the one the file names). So the running
        // ones stay, and only a var the file has newly gained is added.
        let mut vars = before.vars.clone();
        for (k, v) in std::mem::take(&mut fresh.vars) {
            vars.entry(k).or_insert(v);
        }
        fresh.vars = vars;
        let changed = crate::settings::registry(&before).differences(&crate::settings::registry(&fresh));
        // The three parts that keep their own copy of a setting, refreshed so
        // they read the new one.
        self.proactive.cfg = fresh.proactive.clone();
        self.persona = fresh.persona.clone();
        self.eyes.cfg = fresh.presence.clone();
        self.tools_live = Some(fresh);
        // The shared, resolved copy every `tools_cfg()` hands out, and the
        // home zone the clock follows, are rebuilt from the new settings.
        self.tools_resolved = std::sync::Arc::new(resolve_tools(self.tools_ref(), &self.store));
        crate::localclock::set_home_zone(self.chosen_zone());
        let crew_cfg = self.tools_resolved.crew.clone();
        self.crew.set_margins(crew_cfg.keep_free_mb, crew_cfg.battery_floor_percent);
        self.mcp_configure();
        // The wake word switched on or off from the hub or the settings
        // window takes effect now, not at the next start.
        if self.wake_on() != self.tiers.wake_on() {
            let on = self.wake_on();
            self.tiers.set_wake(on);
        }
        let lines: Vec<String> = changed
            .iter()
            .map(|s| {
                let v = s.value.as_display();
                if crate::settings::needs_a_restart(&s.key) {
                    format!("{} will be {v} when I next start.", s.name)
                } else {
                    format!("{} is now {v}.", s.name)
                }
            })
            .collect();
        for l in &lines {
            self.log.info(&format!("settings: {l}"));
        }
        lines
    }

    /// Your tools.yaml, resolved and shared.
    ///
    /// This was `self.cfg.tools.clone().unwrap_or_default()` on every call —
    /// every tool's command and arguments, the whole variable map, several
    /// hundred allocations — at over a hundred call sites, most of which
    /// wanted one field: `let cfg = self.tools_cfg().signin;` copied the lot
    /// to keep one. It is materialised once, handed out by reference count,
    /// and rebuilt only when your settings change (`pick_up_settings`). A
    /// site that needs to own a section clones that one section.
    pub fn tools_cfg(&self) -> std::sync::Arc<crate::voice::ToolsConfig> {
        self.tools_resolved.clone()
    }

    /// "Find me the thing about the budget."
    ///
    /// This used to answer by classifying the words as if they were a
    /// filename — "the budget is a Document. It needs an application that
    /// opens it" — which has the shape of an answer without being one.
    /// `index.search` had existed the whole time and nothing called it.
    ///
    /// The question is run through `asking` first. A spoken question is
    /// mostly scaffolding, and sometimes it points at something rather than
    /// naming it; searching the raw words either dilutes the one term that
    /// mattered or searches for nothing at all and reports that as an answer.
    /// The three things `Intent::Files` was written to do: convert one kind of
    /// file into another, join several, or find one. They share a phrase list
    /// (`convert this`, `join these`, `what is this file`) and, until now, a
    /// single answer -- every one of them went to `find_files` and came back as
    /// a keyword search. "Convert this pdf to text" looked the disk over for
    /// files whose *names* held the words *pdf*, *to* and *text*, which is a
    /// well-formed answer to a question nobody asked.
    ///
    /// A conversion is not a search, so it is answered as one: `files::convert`
    /// knows what each change costs you (a sheet to text loses its formulas)
    /// and what it plainly cannot do (an archive does not become a PDF). Only
    /// what is *not* a recognised conversion falls through to the filename
    /// search, so the existing "find me the note about..." path is untouched.
    pub(in crate::daemon) fn files_request(&self, what: &str) -> String {
        if let Some(answer) = convert_answer(what) {
            return answer;
        }
        self.find_files(what)
    }

    pub(super) fn find_files(&self, what: &str) -> String {
        let prepared = match crate::asking::prepare(what) {
            Ok(p) => p,
            // Asked, not guessed. An invented referent retrieves confidently
            // and wrongly, and you cannot tell that from a real answer.
            Err(unsearchable) => return unsearchable.ask(),
        };

        // Each term separately: the index matches filenames, and a filename
        // almost never contains every word of a spoken question.
        //
        // Each term gives its own ranked list; they are merged by Reciprocal
        // Rank Fusion (`bm25::rrf`, k = 60), so a file several terms find
        // rises above one only the first term found. Before, the first term's
        // list simply came first, whatever the others said.
        let mut names: Vec<(String, String)> = Vec::new();
        let mut lists: Vec<Vec<u64>> = Vec::new();
        for term in &prepared.terms {
            let mut list = Vec::new();
            for e in self.index.search(term) {
                let at = match names.iter().position(|(p, _)| *p == e.path) {
                    Some(i) => i,
                    None => {
                        names.push((e.path.clone(), e.name.clone()));
                        names.len() - 1
                    }
                };
                list.push(at as u64);
            }
            lists.push(list);
        }
        let found: Vec<(String, String)> = crate::bm25::rrf(&lists, crate::bm25::RRF_K)
            .into_iter()
            .map(|(i, _)| names[i as usize].clone())
            .collect();

        let mut answer = if found.is_empty() {
            // Filenames matched nothing. Before giving up, look *inside* the
            // documents and code -- "the thing about the budget" rarely has
            // the word "budget" in its name. This is the expensive search: it
            // reads files off the disk, so it runs only now, once the cheap
            // pass over filenames has already come back empty. Skipped when
            // there is no indexing config, because then the size cap and the
            // roots that bound the read do not exist.
            let inside: Vec<crate::index::ContentHit> = match self.cfg.indexing.as_ref() {
                Some(idx_cfg) => {
                    // All the terms at once: the content search ranks by
                    // BM25, which scores a chunk holding several of them
                    // above one holding a single word, and that only works
                    // if it sees them together.
                    self.index.search_content(&prepared.terms.join(" "), idx_cfg, 3)
                }
                None => Vec::new(),
            };

            // Says what it looked for. "I couldn't find anything" leaves you
            // unable to tell a bad search from an empty disk.
            let caveat = match self.index.missed.caveat() {
                Some(c) => format!(" {c}"),
                None => String::new(),
            };

            if inside.is_empty() && self.index_load.is_loading() {
                // Not "nothing matches": the list isn't all here yet.
                crate::index::STILL_READING.to_string()
            } else if inside.is_empty() {
                // "Found nothing" and "could not look" are different answers,
                // and the index already knows which one this was.
                format!(
                    "Nothing in the {} indexed files matches {}.{}",
                    self.index.entries.len(),
                    prepared.searched_for(),
                    caveat
                )
            } else {
                // Nothing was *named* for it, but something *says* it. Show
                // the line each was found on, so you can tell a real match
                // from a word that happened to appear.
                let shown: Vec<String> = inside
                    .iter()
                    .take(3)
                    .map(|h| format!("{} -- {}", h.cite, h.excerpt))
                    .collect();
                format!(
                    "No filename matches {}, but it's written inside {}.{}",
                    prepared.searched_for(),
                    shown.join("; "),
                    caveat
                )
            }
        } else {
            // What each one is, not just what it's called. A search that
            // returns five names you can't open is a list, not an answer —
            // and `files::Sort` knows which of them needs something you may
            // not have.
            let shown: Vec<String> = found
                .iter()
                .take(5)
                .map(|(_, n)| {
                    let sort = crate::files::Sort::of(n);
                    if sort.readable_offline() {
                        n.clone()
                    } else {
                        format!("{n} (needs {})", sort.needs())
                    }
                })
                .collect();
            let more = found.len().saturating_sub(shown.len());
            let tail = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            format!(
                "Searching {} — {}{}.",
                prepared.searched_for(),
                shown.join(", "),
                tail
            )
        };

        // Two questions in one utterance retrieve the average of two topics
        // and the best match for neither. Said rather than silently dropped.
        if prepared.is_more_than_one_question() {
            answer.push_str(&format!(
                " That was two questions — I answered the first. The other was: {}",
                prepared.also.join("; ")
            ));
        }
        answer
    }
}
