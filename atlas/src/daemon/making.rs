//! Things you ask Atlas to make or change: small sites and projects, improving and
//! implementing changes, calendar events and bookings, learning a folder, plain
//! changes, animation, explaining code and the project's master document.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

/// Text over this many bytes is a document, kept whole on the reading
/// shelf, not cut into facts.
const READING_SHELF_OVER: usize = 200_000;

impl<'a> Daemon<'a> {
    /// "Draw me a lighthouse at dusk": a picture made on this machine
    /// (`imagemake`), on the crew -- it takes a few minutes on a laptop, and
    /// Atlas keeps listening meanwhile. "Get the picture maker" fetches it.
    pub(super) fn make_picture(&mut self, said: &str) -> String {
        let l = said.to_lowercase();
        if l.contains("picture maker") && (l.contains("get") || l.contains("download") || l.contains("fetch")) {
            return self.get_picture_maker();
        }
        let tools = self.tools_cfg();
        let cfg = tools.picture_making.clone();
        let root = self.store.install_root();
        if let Err(why) = crate::imagemake::ready(&cfg, &root) {
            return format!("I can't make pictures here yet: {why}.");
        }
        let what = crate::imagemake::subject(said);
        if what.trim().is_empty() {
            return "What should I draw?".into();
        }
        let t = crate::store::now();
        let out = crate::imagemake::folder(&cfg).join(crate::imagemake::file_name(&what, t));
        let name = "picture maker";
        if let Err(why) = self.room_for_heavy(name, crate::imagemake::MEMORY_MB, t) {
            return format!("I can't make it right now: {why}");
        }
        let prompt = what.clone();
        let work: crew::Work = Box::new(move |c: &crew::Control| {
            match crate::imagemake::make(&cfg, &root, &prompt, &out, t, &|| c.stopping()) {
                Ok(p) => Ok(format!("Here's {prompt}: it's in {}.", p.display())),
                Err(why) => Err(format!("I couldn't make that picture: {why}.")),
            }
        });
        if self.hand_off("make-picture", t, work, Some(what.clone()), SpeakPolicy::Always) {
            format!("Making a picture of {what} on this machine -- it takes a few minutes; I'll say when it's ready.")
        } else {
            self.helpers.finished(name);
            "I have too much on to start a picture now -- ask me again in a minute.".into()
        }
    }

    /// "Test everything": `atlas selftest` in its own process, on a copy of
    /// the install, as a crew errand; the summary is said when it ends and
    /// the report is in data/selftest/latest.md.
    pub(super) fn test_everything(&mut self) -> String {
        let exe = std::env::current_exe().unwrap_or_else(|_| "atlas".into());
        let reports = crate::selftest::reports_dir(&self.store.install_root());
        let work: crew::Work = Box::new(move |_c| {
            let out = std::process::Command::new(&exe)
                .arg("selftest")
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| format!("the test wouldn't start: {e}"))?;
            let text = String::from_utf8_lossy(&out.stdout);
            let summary = text.lines().rev().find(|l| l.starts_with("Tested ")).unwrap_or("The test ended without a summary.").to_string();
            Ok(format!("{summary} The full report is in {}.", reports.join("latest.md").display()))
        });
        if self.hand_off("self-test", crate::store::now(), work, None, SpeakPolicy::Always) {
            "Testing everything I can do, on a copy of your install -- nothing gets sent, moved or approved. It takes a few minutes; I'll tell you what I find.".into()
        } else {
            "A test is already going -- I'll tell you when it's done.".into()
        }
    }

    /// Fetch the picture maker and its three model files, checked, on the crew.
    fn get_picture_maker(&mut self) -> String {
        if self.handover().stance.handed_over() {
            return "Not while this is handed over -- downloads onto this machine are the owner's.".into();
        }
        let root = self.store.install_root();
        let pieces = crate::getpieces::picture_making();
        if pieces.iter().all(|p| crate::getpieces::have(p, &root)) {
            return "The picture maker is already here. Say \u{201c}draw me\u{201d} and what.".into();
        }
        let gb = pieces.iter().map(|p| p.bytes).sum::<u64>() as f64 / 1e9;
        let work: crew::Work = Box::new(move |_ctl| {
            for p in &pieces {
                if !crate::getpieces::have(p, &root) {
                    crate::getpieces::fetch(p, &root, &crate::getpieces::Tools::default(), &|_, _| {})?;
                }
            }
            Ok("The picture maker is here and checked. Say \u{201c}draw me\u{201d} and what, and I'll make it on this machine.".into())
        });
        if self.hand_off("model-piece", crate::store::now(), work, Some("the picture maker".into()), SpeakPolicy::Always) {
            format!("Getting the picture maker ({gb:.1} GB) -- I'll say when it's ready.")
        } else {
            "I've too much going on to start that download now. Try again in a minute.".into()
        }
    }

    /// A Cloudflare worker to delegate to, when the machine is online and the
    /// provider is set up — otherwise `None` and the local model does the
    /// work, exactly as before. The token is fetched here, on the tick
    /// thread, and baked into the returned worker's vars: a crew errand runs
    /// on another thread and cannot unlock the vault, so the secret has to be
    /// pulled before the errand is built (the same rule mail follows for its
    /// IMAP password). Returns the ready worker as an `Arc<dyn Llm>` so it can
    /// be moved into an errand.
    pub(super) fn cloudflare_worker(
        &mut self,
    ) -> Option<(std::sync::Arc<dyn crate::brain::Llm>, crate::tools::Vars)> {
        let cfg = self.tools_cfg().cloudflare.clone();
        if !cfg.enabled || self.connectivity.cached() != Reach::Online {
            return None;
        }
        let inference = cfg.inference.clone()?;
        let now = crate::store::now();
        // The token, tick-side. If the vault is locked or the entry is
        // missing, the provider is not ready and the local path is used —
        // `readiness` would say the same, and this is the one place that can
        // actually try the vault.
        let token = self.vault.get(&cfg.token_vault, now).ok()?;
        if !crate::online::readiness(&cfg, true).ready_now() {
            return None;
        }
        // The worker's vars: the shared vars, plus the account id and token
        // the Cloudflare request template expects as `{account_id}`/`{token}`.
        // Returned alongside the worker so a delegated page fetch (Browser
        // Rendering) can reuse the same secret without unlocking the vault
        // again on another thread.
        let mut vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
        vars.insert("account_id".into(), cfg.account_id.clone());
        vars.insert("token".into(), token);
        vars.insert("model".into(), cfg.model.clone());
        Some((std::sync::Arc::new(crate::brain::ShellLlm { cfg: inference, vars: vars.clone() }), vars))
    }

    /// Build code from a description, check it against the real toolchain, and
    /// hand over what passes.
    ///
    /// The generating model is chosen the same way research chooses its
    /// summariser: a Cloudflare worker when the machine is online and the
    /// provider is set up, the local model otherwise — you never have to say
    /// "do it online", Atlas delegates the drafting to free itself up when it
    /// can and falls back to local when it can't. Either way the *check* is
    /// local: the draft is written into a throwaway sandbox and run through
    /// `craft`'s ladder (compile, lint, test) on this machine, because a
    /// model's confidence is worth nothing and the compiler's verdict is worth
    /// everything. The whole thing runs as a crew errand so the turn is not
    /// blocked while cargo grinds.
    pub(super) fn build_from_description(&mut self, what: &str) -> String {
        let cfg = self.tools_cfg().build.clone();
        if !cfg.enabled {
            return "Building code is switched off in your settings.".into();
        }
        let what = what.trim();
        if what.is_empty() {
            return "Tell me what to build — \"write me a script that renames files by date\", say."
                .into();
        }
        // Local model, or a worker when online and set up. This is the
        // auto-delegation: no explicit "online" needed.
        let worker = self.cloudflare_worker();
        let gen_llm: std::sync::Arc<dyn crate::brain::Llm> = match &worker {
            Some((w, _)) => w.clone(),
            // Drafting code is background work (`deepbrain`).
            None => match self.background_llm() {
                Some(l) => l,
                None => {
                    return "I can write code, but I need a model to draft it and I haven't got \
                            one configured."
                        .into()
                }
            },
        };
        let delegated = worker.is_some();
        let max_rounds = cfg.max_fix_rounds;
        let out_dir = crate::roots::data_sub("builds");

        // A web page goes through the taste gate instead of the compiler: draft
        // the HTML, review it against the house style, and iterate until it
        // clears the blocking floors or runs out of budget. The review is the
        // reliable half — it can't say the design is good, only that it's
        // consistent and accessible, and the reply says exactly that.
        if crate::taste::wants_web_page(what) {
            let rules = self.tools_cfg().taste.clone();
            let brief = what.to_string();
            let out_dir = out_dir.clone();
            let work: crew::Work = Box::new(move |ctl| {
                let outcome = crate::taste::build_web(&brief, gen_llm.as_ref(), max_rounds, |html| {
                    // Between rounds: a pause holds with the draft so far intact.
                    let _ = ctl.checkpoint();
                    crate::taste::review(html, &rules)
                });
                let mut said = outcome.spoken();
                if let Some(html) = outcome.html() {
                    let _ = std::fs::create_dir_all(&out_dir);
                    let built = matches!(outcome, crate::taste::Outcome::Built { .. });
                    let name = if built { "page.reviewed.html" } else { "page.draft.html" };
                    let path = out_dir.join(name);
                    // Said only if it's true. A full disk used to fail here
                    // silently and still announce where the page was.
                    match std::fs::write(&path, html) {
                        Ok(()) => said.push_str(&format!("\n\nSaved to {}.", path.display())),
                        Err(e) => said.push_str(&format!("\n\nI couldn't save it to {} ({e}) — is the disk full?", path.display())),
                    }
                }
                Ok(said)
            });
            let taken = self.hand_off("build", crate::store::now(), work, Some(what.to_string()), SpeakPolicy::Always);
            return if taken {
                if delegated {
                    "On it — a worker online will draft the page and I'll review it against the house \
                     style here before I show you.".into()
                } else {
                    "On it — I'll draft the page, review it against the house style, and iterate until \
                     it's consistent and accessible.".into()
                }
            } else {
                "I'm swamped with background work right now — ask me to build it again in a moment.".into()
            };
        }

        let lang = crate::build_it::lang_from_words(what, cfg.default_language);
        let desc = what.to_string();
        let base = crate::roots::tmp_dir().join("builds");

        let work: crew::Work = Box::new(move |ctl| {
            let mut sandbox = match crate::sandbox::Sandbox::create(&base, "build") {
                Ok(s) => s,
                Err(e) => return Err(format!("couldn't make a sandbox to build in: {e}")),
            };
            // The checker: scaffold the draft into the sandbox and run the
            // ladder. Injected into `build_loop` so the loop logic is testable
            // without a toolchain; here it is the real compiler.
            let mut check = |code: &str| -> crate::build_it::Check {
                // Between rounds: a pause holds with the draft so far intact.
                let _ = ctl.checkpoint();
                check_draft_in_sandbox(&mut sandbox, lang, code)
            };
            let outcome = crate::build_it::build_loop(&desc, lang, gen_llm.as_ref(), max_rounds, &mut check);
            // The sandbox has done its job; left behind, one piled up per build.
            drop(check);
            let _ = sandbox.discard();
            // A build that ran out of tries is kept, so "keep at it" carries
            // on from its best draft (E3).
            let mut offer_more = false;
            if let crate::build_it::Outcome::Struggled { code, last_failure, .. } = &outcome {
                let s = crate::build_it::Struggle {
                    description: desc.clone(),
                    lang,
                    code: code.clone(),
                    failure: last_failure.clone(),
                };
                let path = crate::build_it::Struggle::path();
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                offer_more = serde_json::to_string(&s).ok().map(|j| std::fs::write(&path, j).is_ok()).unwrap_or(false);
            }
            // Verified code is written where you can pick it up; a struggle
            // still leaves its best draft there, clearly named.
            let mut said = outcome.spoken(lang);
            if let Some(code) = outcome.code() {
                let _ = std::fs::create_dir_all(&out_dir);
                let name = if outcome.is_built() { "build.verified" } else { "build.draft" };
                let path = out_dir.join(format!("{name}.{}", ext_for(lang)));
                if let Err(e) = std::fs::write(&path, code) {
                    said.push_str(&format!("\n\n(I couldn't save it to {} — {e}. Is the disk full?)", path.display()));
                }
                // Auto-explain: generated code never arrives without a plain-
                // English summary of what it does, iterated to read plainly.
                if let Some(plain) = crate::explain::in_plain_english(code, gen_llm.as_ref(), max_rounds) {
                    said.push_str(&format!("\n\nIn plain English: {plain}"));
                }
            }
            if offer_more {
                said.push_str("\n\nSay \"keep at it\" and I'll carry on from this draft on my own until it passes.");
            }
            Ok(said)
        });

        let taken = self.hand_off("build", crate::store::now(), work, Some(what.to_string()), SpeakPolicy::Always);
        if taken {
            if delegated {
                "On it — I've handed the drafting to a worker online and I'll check what comes back \
                 against the compiler here before I show you."
                    .into()
            } else {
                "On it — I'll write it, check it against the compiler and tests, and tell you how it went."
                    .into()
            }
        } else {
            "I'm swamped with background work right now — ask me to build it again in a moment.".into()
        }
    }

    /// Work on one of your projects: scope it, build it (itself or delegated),
    /// check it, and file a proposed change into that project's queue for you
    /// to implement when you're ready. Nothing touches the project's real
    /// files here — that only happens on `implement`.
    pub(super) fn improve_project(&mut self, what: &str) -> String {
        let cfg = self.tools_cfg().build.clone();
        if !cfg.enabled {
            return "Building is switched off in your settings.".into();
        }
        let what = what.trim();
        if what.is_empty() {
            return "Tell me the project and what to change — \"on the Atlas project, add a date parser\"."
                .into();
        }
        // Which project is this for? A registered name mentioned anywhere in
        // the request wins; otherwise a "project X"/"on X"/"in X" phrase names
        // a new one.
        let project = match self.detect_project(what) {
            Some(p) => p,
            None => {
                return "Which project is this for? Name it — \"on the Atlas project, …\" — and I'll \
                        queue the change there."
                    .into()
            }
        };
        let worker = self.cloudflare_worker();
        let gen_llm: std::sync::Arc<dyn crate::brain::Llm> = match &worker {
            Some((w, _)) => w.clone(),
            // Drafting code is background work (`deepbrain`).
            None => match self.background_llm() {
                Some(l) => l,
                None => return "I can do that, but I need a model to build it and none is configured.".into(),
            },
        };
        let delegated = worker.is_some();
        let lang = crate::build_it::lang_from_words(what, cfg.default_language);
        let max_rounds = cfg.max_fix_rounds;
        let desc = what.to_string();
        let title = workshop_title(what);
        let project_name = project.clone();
        // Copies for the acknowledgement, since the originals move into the
        // errand closure below.
        let title_ack = title.clone();
        let project_ack = project_name.clone();
        // Any context Atlas can read from the project folder now, so the draft
        // is written against what's actually there. Read on the tick thread;
        // the errand runs elsewhere and gets the text, not the folder.
        let folder = self.workshop.resolve(&project).map(|p| p.folder.clone()).unwrap_or_default();
        let context = read_project_context(&folder);
        // What the files this could write look like now, as the model is
        // about to read them — taken here, at the start, not when the change
        // lands, so an edit you make while it's being written still counts
        // as the code moving on.
        let start_bases: Vec<(String, String)> = {
            let mut paths = vec![format!("proposed_change.{}", ext_for(lang))];
            if let Some(t) = named_existing_file(what, std::path::Path::new(&folder)) {
                paths.push(t);
            }
            crate::workshop::bases_in(&folder, &paths)
        };
        let base = crate::roots::tmp_dir().join("improve");
        // Each phase written down as it finishes (`phases`): asked again, or
        // redone after a restart (`resume`), the work carries on after the
        // last finished phase instead of starting over.
        let phases = crate::phases::Phases::for_work(self.store.root(), "improve", &format!("{project}\n{what}"));
        let carrying_on = {
            let done = phases.finished_phases();
            let named: Vec<&str> = done
                .iter()
                .filter_map(|p| match p.as_str() {
                    "1-draft" => Some("the draft"),
                    "2-checked" => Some("checking it"),
                    "3-explained" => Some("explaining it"),
                    _ => None,
                })
                .collect();
            if named.is_empty() {
                String::new()
            } else {
                format!(" Carrying on from where it stopped — {} already done.", named.join(", "))
            }
        };

        let work: crew::Work = Box::new(move |ctl| {
            let prompt = if context.is_empty() {
                desc.clone()
            } else {
                format!("{desc}\n\nHere is some of the existing project for context:\n{context}")
            };
            let mut sandbox = match crate::sandbox::Sandbox::create(&base, "improve") {
                Ok(s) => s,
                Err(e) => return Err(format!("couldn't make a sandbox to work in: {e}")),
            };
            let mut check = |code: &str| -> crate::build_it::Check {
                // Between rounds: a pause holds with the draft so far intact.
                let _ = ctl.checkpoint();
                check_draft_in_sandbox(&mut sandbox, lang, code)
            };
            let outcome = match phases.done::<crate::build_it::Outcome>("1-draft") {
                Some(o) => o,
                None => {
                    let o = crate::build_it::build_loop(&prompt, lang, gen_llm.as_ref(), max_rounds, &mut check);
                    if o.code().is_some() {
                        phases.finished("1-draft", &o);
                    }
                    o
                }
            };
            // The sandbox has done its job; left behind, one piled up per change.
            drop(check);
            let _ = sandbox.discard();
            if ctl.checkpoint() {
                return Err("you asked me to stop".into());
            }
            let Some(code) = outcome.code().map(str::to_string) else {
                return Err(match &outcome {
                    crate::build_it::Outcome::NoDraft(w) => w.clone(),
                    _ => "couldn't build that".into(),
                });
            };
            // Verify as strongly as the project allows: isolated is where
            // `build_loop` stops; if the request names an existing file in a
            // buildable project, prove it *inside a copy of the project*
            // instead. The queued change carries whichever was actually done.
            let (verified, mut summary, files) = match phases.done::<(bool, String, Vec<crate::workshop::FileEdit>)>("2-checked") {
                Some(v) => v,
                None => {
                    let v = verify_project_change(&project_name, &folder, &title, lang, &desc, &code, &outcome, &base);
                    phases.finished("2-checked", &v);
                    v
                }
            };
            if ctl.checkpoint() {
                return Err("you asked me to stop".into());
            }
            // Auto-explain: the queued change is described in plain English, so
            // you know what it does before deciding whether to implement it.
            let plain = match phases.done::<Option<String>>("3-explained") {
                Some(p) => p,
                None => {
                    let p = crate::explain::in_plain_english(&code, gen_llm.as_ref(), max_rounds);
                    phases.finished("3-explained", &p);
                    p
                }
            };
            if let Some(plain) = plain {
                summary.push_str(&format!("\n\nIn plain English: {plain}"));
            }

            let bases: Vec<(String, String)> = start_bases
                .iter()
                .filter(|(p, _)| files.iter().any(|f| &f.path == p))
                .cloned()
                .collect();
            let envelope = ImproveOutcome {
                project: project_name.clone(),
                title: title.clone(),
                what: desc.clone(),
                files,
                verified,
                note: summary.clone(),
                summary,
                bases,
            };
            serde_json::to_string(&envelope).map_err(|e| format!("couldn't package the change: {e}"))
        });

        let taken = self.hand_off("improve", crate::store::now(), work, Some(project), SpeakPolicy::Always);
        if taken {
            if delegated {
                format!(
                    "On it — scoping \"{title_ack}\" for {project_ack}, handing the drafting to a worker \
                     online, and checking it here before it hits your queue.{carrying_on}"
                )
            } else {
                format!(
                    "On it — scoping \"{title_ack}\" for {project_ack}, building and checking it, then \
                     it'll land in that project's queue for your go-ahead.{carrying_on}"
                )
            }
        } else {
            "I'm swamped with background work right now — ask me again in a moment.".into()
        }
    }

    // (verify_project_change is a free function below — it needs no `self`.)

    /// Apply a change you reviewed and named. Writes its files into the
    /// project folder (keeping a backup so it can be undone), marks it
    /// implemented, and files a fresh master document for the project.
    pub(crate) fn implement_change(&mut self, what: &str) -> String {
        let what = what.trim();
        if what.is_empty() {
            return "Which change? Say \"implement <title>\" — the title I gave it when I queued it.".into();
        }
        let plan = match self.workshop.plan_implementation(what) {
            Ok(p) => p,
            Err(crate::workshop::ImplementError::NoMatch) => {
                return format!(
                    "I don't have a change ready under \"{what}\". Ask me what's in the queue if you're \
                     not sure of the title."
                )
            }
            Err(crate::workshop::ImplementError::NoFolder(name)) => {
                return format!(
                    "That change is ready, but I don't know where the {name} project lives. Tell me its \
                     folder and I'll apply it."
                )
            }
            Err(crate::workshop::ImplementError::Outdated { title, files }) => {
                return format!(
                    "I didn't apply \"{title}\": {} changed since I wrote it, so it was written against \
                     code that isn't there any more and would overwrite the newer version. Ask me to \
                     redo it against the current code — nothing was written.",
                    files.join(", ")
                )
            }
        };
        let root = std::path::Path::new(&plan.folder);
        if !root.is_dir() {
            return format!(
                "That change is ready, but the {} folder ({}) isn't reachable from here.",
                plan.project, plan.folder
            );
        }
        // Write each file, keeping a .before backup of anything replaced so
        // the whole thing is reversible.
        let now = crate::store::now();
        let mut written = 0usize;
        let mut failures = Vec::new();
        for f in &plan.files {
            let target = root.join(&f.path);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            // No backup, no overwrite: the .before copy is what makes this
            // reversible, and writing over the original after the backup
            // failed (a full disk) made it not.
            if target.exists() {
                if let Ok(prev) = std::fs::read(&target) {
                    if let Err(e) = std::fs::write(target.with_extension("before"), prev) {
                        failures.push(format!("{}: left as it was — I couldn't keep a backup first ({e})", f.path));
                        continue;
                    }
                }
            }
            match std::fs::write(&target, &f.content) {
                Ok(()) => written += 1,
                Err(e) => failures.push(format!("{}: {e}", f.path)),
            }
        }
        if written > 0 {
            self.workshop.mark_implemented(&plan.project, plan.change_id, now);
            // A fresh master document for the project, filed where you can find
            // it — a record of what the project now contains and what just went
            // in.
            let doc = self.write_project_master_doc(&plan.project);
            let _ = self.workshop.save(&self.store);
            self.history.note(
                &format!("implemented \"{}\" on {}", plan.title, plan.project),
                "code",
                crate::undo::Undo::You(format!(
                    "the previous versions are saved as .before files in {}",
                    plan.folder
                )),
                true,
                now,
            );
            let mut msg = format!(
                "Implemented \"{}\" on {} — wrote {written} file{}.",
                plan.title,
                plan.project,
                if written == 1 { "" } else { "s" }
            );
            if let Some(d) = doc {
                msg.push_str(&format!(" Filed a fresh master document at {d}."));
            }
            if !failures.is_empty() {
                msg.push_str(&format!(" Couldn't write: {}.", failures.join(", ")));
            }
            msg
        } else {
            format!("Couldn't apply \"{}\": {}.", plan.title, failures.join(", "))
        }
    }

    /// Put something on your calendar. Reads the time from your words; if it
    /// can't, it asks rather than guessing. Notes any clash, but still files it
    /// — a clash is yours to sort out, not a reason to refuse.
    pub(super) fn schedule_event(&mut self, what: &str) -> String {
        let cfg = self.calendar_cfg();
        if !cfg.enabled {
            return "The calendar is switched off in your settings.".into();
        }
        let what = what.trim();
        if what.is_empty() {
            return "What should I put on, and when? — \"schedule lunch tomorrow at 12\".".into();
        }
        // The turn's time, as the agenda reads it (`now_acting`): scheduled at
        // one time and read back at another, "tomorrow" was two different days.
        let now = self.now_acting();
        let zone = self.home_zone();
        // Does it repeat? "every weekday", "every Monday", "daily" — read the
        // same way the time is, and never guessed. Read before the time so a
        // repeat named without an explicit day ("standup every weekday at 9")
        // can start today rather than being turned away for want of a date.
        let repeat = crate::calendar::repeat_from(what);
        // Read on your clock, stored in UTC.
        let lnow = zone.to_local(now as i64).max(0) as u64;
        let Some(when) = crate::calendar::resolve_recurring_when(what, lnow, &repeat).map(|w| {
            let start = zone.to_utc(w.start as i64).max(0) as u64;
            crate::calendar::When { start, end: start + (w.end - w.start), all_day: w.all_day }
        }) else {
            return format!(
                "I've got \"{what}\" but not when. Give me a day and a time — \"tomorrow at 3pm\", \
                 \"Monday at 9\" — and I'll put it on."
            );
        };
        // The title is the request with the time words trimmed off the ends,
        // so "schedule lunch tomorrow at 12" files as "lunch".
        let title = crate::calendar::event_title(what);
        // Expanded so a booking landing on a recurring slot — next Tuesday's
        // standup — is caught, not just one whose single stored time overlaps.
        let clashes = self.calendar.clashes_expanded(when.start, when.end);
        let clash_note = match clashes.first() {
            Some(c) if !when.all_day => {
                format!(" Heads up — it runs into \"{}\" ({}).", c.title, c.say_when_in(&zone))
            }
            _ => String::new(),
        };
        // Which side of the firewall this belongs on: an event that names a
        // business you have is filed there, everything else is yours. The
        // roster is the list of businesses you actually have, so a plain
        // "lunch with Sam" is never mistaken for one.
        let roster = crate::roster::Roster::load(&self.store);
        let space = crate::calendar::space_for_request(what, &roster.businesses());
        let side = match &space {
            crate::earned::Space::Business(b) => format!(" (on {b})"),
            crate::earned::Space::Personal => String::new(),
        };
        // Meeting, or time reserved for yourself — "block off two hours for the
        // Q3 summary" is the latter. Both are booked time and still clash; they
        // just read differently.
        let kind = crate::calendar::kind_for_request(what);
        let lead = match kind {
            crate::calendar::EventKind::TimeBlock => "Blocked off",
            crate::calendar::EventKind::Meeting => "On the calendar",
        };
        let id = self.calendar.add_full(&title, when, None, space, kind, repeat, now);
        self.calendar.keep_wall_clock(id, &zone);
        // A reminder, if you asked for one — "remind me 10 minutes before".
        let remind = crate::calendar::reminder_from(what);
        if remind.is_some() {
            self.calendar.set_reminder(id, remind);
        }
        let _ = self.calendar.save(&self.store);
        let ev = self.calendar.event(id);
        let whenn = ev.map(|e| e.say_when_in(&zone)).unwrap_or_default();
        self.history.note(
            &format!("scheduled \"{title}\" for {whenn}"),
            "calendar",
            // What you'd actually say, and now there's something that hears
            // it (`keeping`: 30 Sep 2026 -- this promised "cancel" and
            // nothing could take an event off).
            crate::undo::Undo::You(format!("say \"cancel {title}\" and I'll take it off")),
            false,
            now,
        );
        let remind_note = match remind {
            Some(m) if m % 60 == 0 && m >= 60 => {
                let h = m / 60;
                format!(" I'll remind you {h} hour{} before.", if h == 1 { "" } else { "s" })
            }
            Some(m) => format!(" I'll remind you {m} minutes before."),
            None => String::new(),
        };
        format!("{lead}: \"{title}\"{side}, {whenn}.{clash_note}{remind_note}")
    }

    /// Read your calendar back — a window of days, soonest first.
    pub(super) fn read_agenda(&self, what: &str) -> String {
        let cfg = self.calendar_cfg();
        if !cfg.enabled {
            return "The calendar is switched off in your settings.".into();
        }
        // The turn's time, not the wall clock's (28 Sep 2026): "what's on
        // today" asked at 23:59 and answered at 00:00 read the wrong day.
        let now = self.now_acting();
        let low = what.to_lowercase();
        // "what's on for Northwind" narrows to one side of the firewall; the
        // same classifier the scheduler uses, so a business you have is
        // recognised and a plain "what's on this week" shows everything.
        let roster = crate::roster::Roster::load(&self.store);
        let only = crate::calendar::space_for_request(what, &roster.businesses());
        let business = matches!(only, crate::earned::Space::Business(_));
        let days = if low.contains("week") { 7 } else { cfg.horizon_days };
        // The window to read, then expand recurring events across it so a
        // standup shows on each day it's on, not as one row saying "every
        // weekday". `occurrences_between` returns owned, dated occurrences.
        // "Today" is your day: midnight on your clock, as a real moment. It
        // was UTC's day here, so after 5 pm in Pacific time "what's on today"
        // showed tomorrow (both chats found this; merged 26 Sep).
        let zone = self.home_zone();
        let lnow = zone.to_local(now as i64).max(0) as u64;
        // Midnight to midnight on your clock, each converted on its own: a
        // day the clocks change on is 23 or 25 hours, not 24 (28 Sep 2026).
        let local_day = |start_local: u64| {
            let s = zone.to_utc(start_local as i64).max(0) as u64;
            let e = zone.to_utc((start_local + 86_400) as i64).max(0) as u64;
            (s, e)
        };
        let (from, to) = if low.contains("today") || low.contains("tonight") {
            local_day(crate::calendar::start_of_day(lnow))
        } else if low.contains("tomorrow") {
            local_day(crate::calendar::start_of_day(crate::calendar::start_of_day(lnow) + 86_400))
        } else {
            (now, now + days as u64 * 86_400)
        };
        let mut events = self.calendar.occurrences_between(from, to);
        // Narrow to one side of the firewall when a business was named.
        if business {
            events.retain(|e| e.space == only);
        }
        if events.is_empty() {
            let scope = match &only {
                crate::earned::Space::Business(b) => format!(" for {b}"),
                crate::earned::Space::Personal => String::new(),
            };
            return format!("Nothing on your calendar{scope} for that stretch.");
        }
        let mut lines: Vec<String> =
            // `say_when` is on your clock (`localclock::zone`, the same home
            // zone as `home_zone`).
            events.iter().take(12).map(|e| format!("{} — {}", e.say_when(), e.title)).collect();
        let head = match events.len() {
            1 => "One thing:".to_string(),
            n => format!("{n} things:"),
        };
        lines.insert(0, head);
        lines.join("\n")
    }

    fn calendar_cfg(&self) -> crate::calendar::CalendarConfig {
        self.tools_ref().map(|t| t.calendar.clone()).unwrap_or_default()
    }

    /// Your calendar as busy slots, for judging a proposed time against.
    fn busy_slots(&self, from: u64, to: u64) -> Vec<crate::booking::Slot> {
        self.calendar
            .occurrences_between(from, to)
            .into_iter()
            .filter(|e| !e.all_day)
            .map(|e| crate::booking::Slot {
                start: e.start,
                mins: (e.end.saturating_sub(e.start) / 60) as u32,
            })
            .collect()
    }

    /// Read a proposal out of a request: who, what it's about, and the times
    /// they offered. `None` if it doesn't name a proposal or no time parses —
    /// Atlas asks rather than inventing one.
    fn parse_proposal(&self, text: &str, now: u64) -> Option<crate::booking::Proposal> {
        // ASCII-only lowering keeps every byte where it was, so an index found
        // in `low` cuts `text` at the same place. Full lowering changes lengths
        // ("İ" becomes two characters) and a cut could land inside one.
        let low = text.to_ascii_lowercase();
        const TRIGGERS: &[&str] = &[
            "proposed", "proposes", "wants a time", "wants to meet", "suggested",
            "asked for a time", "asked to meet", "offered a time", "log a proposal",
        ];
        let trigger = TRIGGERS.iter().find(|t| low.contains(**t))?;
        let t_idx = low.find(trigger)?;

        // Who: "from X" wins (the natural way to name them when the sentence
        // starts with a fixed phrase — "log a proposal from Sam …"); else the
        // words before the trigger ("Sam proposed …"); else "someone".
        let from = if let Some(i) = low.find("from ") {
            const STOP: &[&str] = &[
                "tomorrow", "today", "tonight", "monday", "tuesday", "wednesday", "thursday",
                "friday", "saturday", "sunday", "at", "next", "this", "for", "about",
            ];
            let name: Vec<&str> = text[i + 5..]
                .split_whitespace()
                .take_while(|w| {
                    let w = w.trim_matches(|c: char| !c.is_alphanumeric());
                    !w.is_empty()
                        && w.chars().all(|c| c.is_alphabetic())
                        && !STOP.contains(&w.to_lowercase().as_str())
                })
                .take(2)
                .collect();
            if name.is_empty() { "someone".to_string() } else { name.join(" ") }
        } else {
            let head = text[..t_idx].trim().trim_end_matches(',').trim();
            if head.is_empty() || head.eq_ignore_ascii_case("someone") {
                "someone".to_string()
            } else {
                head.to_string()
            }
        };

        // What it's about: after "for the"/"about the"/"for a"/"about", trimmed.
        let about = ["for the ", "about the ", "for a ", "for ", "about "]
            .iter()
            .find_map(|k| low.rfind(k).map(|i| text[i + k.len()..].trim().trim_end_matches('.').to_string()))
            .filter(|s| !s.is_empty() && s.split_whitespace().count() <= 6);

        // The times they offered: split the request on "or"/"and", read a time
        // from each chunk, keep the ones that resolve.
        let after = &text[t_idx..];
        let mut times = Vec::new();
        // Offered times are read on your clock — the one the offer was made
        // to — and kept in UTC like everything else.
        let zone = self.home_zone();
        let lnow = zone.to_local(now as i64).max(0) as u64;
        for chunk in after.split(" or ").flat_map(|c| c.split(" and ")) {
            if let Some(w) = crate::calendar::resolve_when(chunk, lnow) {
                if !w.all_day {
                    let mins = ((w.end.saturating_sub(w.start)) / 60) as u32;
                    times.push(crate::booking::Slot { start: zone.to_utc(w.start as i64).max(0) as u64, mins });
                }
            }
        }
        if times.is_empty() {
            return None;
        }
        Some(crate::booking::Proposal {
            id: now,
            from,
            about,
            their_words: text.to_string(),
            times,
            at: now,
            state: crate::booking::State::NeedsYou,
        })
    }

    /// A time someone proposed, or your answer to one. Atlas does the tedious
    /// half — reads it, checks it against your calendar, lays out what fits —
    /// and stops. Only your explicit accept writes it to your calendar, and
    /// nothing is ever sent on your behalf.
    pub(super) fn booking(&mut self, what: &str) -> String {
        let cfg = self.tools_cfg().booking.clone();
        if !cfg.enabled {
            return "Working through times other people propose is switched off in your settings.".into();
        }
        let arg = what.trim();
        let now = self.now_acting();

        // An answer to a proposal already waiting on you?
        if let Some(state) = crate::booking::answered(arg) {
            if let Some(i) = self.proposals.iter().position(|p| p.state == crate::booking::State::NeedsYou) {
                return self.answer_proposal(i, state, now, &cfg);
            }
            // "yes"/"no" with nothing pending: fall through to the guidance below.
        }

        // A new proposal to log?
        if let Some(p) = self.parse_proposal(arg, now) {
            let busy = self.busy_slots(now, now + 30 * 86_400);
            let assessed = crate::booking::assess(&p, &busy, now, &cfg, &self.home_zone());
            let mins = p.times.first().map(|s| s.mins).unwrap_or(30);
            let alternatives = crate::booking::could_offer(&busy, now, mins, &cfg, 3, &self.home_zone());
            let line = crate::booking::to_decide(&p, &assessed, &alternatives);
            self.proposals.push(p);
            let _ = self.store.save("proposals", &self.proposals);
            return line;
        }

        // Nothing to log and nothing answered — re-present what's waiting, or say
        // there's nothing.
        match self.proposals.iter().find(|p| p.state == crate::booking::State::NeedsYou).cloned() {
            Some(p) => {
                let busy = self.busy_slots(now, now + 30 * 86_400);
                let assessed = crate::booking::assess(&p, &busy, now, &cfg, &self.home_zone());
                let mins = p.times.first().map(|s| s.mins).unwrap_or(30);
                let alternatives = crate::booking::could_offer(&busy, now, mins, &cfg, 3, &self.home_zone());
                crate::booking::to_decide(&p, &assessed, &alternatives)
            }
            None => "I don't have a proposed time to work through. Tell me who proposed what — \
                     \"Sam proposed Tuesday at 2pm or Wednesday at 10am for the review\" — and I'll \
                     check it against your calendar."
                .into(),
        }
    }

    /// Carry out your answer to the waiting proposal. Accept writes it to your
    /// calendar; decline marks it; a counter lays out times you could offer.
    /// Atlas never sends the reply — that stays with you.
    fn answer_proposal(
        &mut self,
        i: usize,
        state: crate::booking::State,
        now: u64,
        cfg: &crate::booking::BookingConfig,
    ) -> String {
        use crate::booking::{Fit, State};
        let p = self.proposals[i].clone();
        match state {
            State::Accepted => {
                let busy = self.busy_slots(now, now + 30 * 86_400);
                let assessed = crate::booking::assess(&p, &busy, now, cfg, &self.home_zone());
                let slot = assessed
                    .iter()
                    .find(|a| matches!(a.verdict, Fit::Free | Fit::Awkward))
                    .map(|a| a.slot);
                match slot {
                    Some(s) => {
                        let title = p
                            .about
                            .clone()
                            .unwrap_or_else(|| format!("meeting with {}", p.from));
                        let when = crate::calendar::When {
                            start: s.start,
                            end: s.start + s.mins as u64 * 60,
                            all_day: false,
                        };
                        let id = self.calendar.add_in(
                            &title,
                            when,
                            None,
                            crate::earned::Space::Personal,
                            now,
                        );
                        let whenn = self.calendar.event(id).map(|e| e.say_when_in(&self.home_zone())).unwrap_or_default();
                        self.proposals[i].state = State::Accepted;
                        let _ = self.calendar.save(&self.store);
                        let _ = self.store.save("proposals", &self.proposals);
                        format!(
                            "Done — \"{title}\" is on your calendar for {whenn}. I haven't replied to \
                             {}; say the word and I'll draft it, but I don't send on your behalf.",
                            p.from
                        )
                    }
                    None => "None of their times actually clear against your calendar, so accepting \
                             would book a clash. Better to offer another — say \"offer another time\"."
                        .into(),
                }
            }
            State::Declined => {
                self.proposals[i].state = State::Declined;
                let _ = self.store.save("proposals", &self.proposals);
                format!(
                    "Marked {}'s proposal declined. Nothing's been sent — that reply is yours to make.",
                    p.from
                )
            }
            State::CounterOffered => {
                let busy = self.busy_slots(now, now + 30 * 86_400);
                let mins = p.times.first().map(|s| s.mins).unwrap_or(30);
                let alternatives = crate::booking::could_offer(&busy, now, mins, cfg, 3, &self.home_zone());
                self.proposals[i].state = State::CounterOffered;
                let _ = self.store.save("proposals", &self.proposals);
                if alternatives.is_empty() {
                    "I couldn't find a clear slot inside your hours to offer instead — your next two \
                     weeks are full at those times.".into()
                } else {
                    let times: Vec<String> = alternatives
                        .iter()
                        .map(|s| {
                            let e = crate::calendar::Event {
                                id: 0,
                                title: String::new(),
                                start: s.start,
                                end: s.start + s.mins as u64 * 60,
                                all_day: false,
                                place: None,
                                note: None,
                                space: crate::earned::Space::Personal,
                                kind: crate::calendar::EventKind::Meeting,
                                repeat: crate::calendar::Repeat::Once,
                                remind_before_mins: None,
                                source: crate::calendar::Source::Atlas,
                                phone_key: None,
                                created: now,
                                except: Vec::new(),
                                zone: None,
                            };
                            e.say_when_in(&self.home_zone())
                        })
                        .collect();
                    format!(
                        "Here's what you could offer {} instead: {}. Nothing's sent — pick one and \
                         I'll draft the reply for you to send.",
                        p.from,
                        times.join("; ")
                    )
                }
            }
            State::NeedsYou | State::WentStale => {
                let opts = "\"accept the meeting\", \"decline the meeting\", or \"offer another time\"";
                format!("I couldn't tell if that was an accept, a decline, or a counter — say {opts}.")
            }
        }
    }

    /// Which registered project a request is about, if any — a registered name
    /// mentioned anywhere wins; otherwise a "project X"/"on X"/"in X" phrase
    /// names one (new or existing).
    /// Review a page's design against the house style.
    ///
    /// The honest half of taste, made usable: it reads the markup — a file you
    /// name, or HTML you paste — and reports where it's off the spacing scale,
    /// using typed-in colours instead of tokens, or failing accessibility. It
    /// never says whether the design is *good*; that's yours to judge or a
    /// stronger model's. A clean review means "consistent and accessible", not
    /// "right".
    pub(super) fn design_review(&self, what: &str) -> String {
        let arg = what.trim();
        if arg.is_empty() {
            return "Point me at a page — \"review the design of index.html\" — or paste the markup."
                .into();
        }

        // Paste vs path: markup has a tag in it; anything else is treated as a
        // file to read.
        let (html, source) = if arg.contains('<') && arg.contains('>') {
            (arg.to_string(), "the markup you gave me".to_string())
        } else {
            match std::fs::read_to_string(arg) {
                Ok(text) => (text, format!("\"{arg}\"")),
                Err(e) => {
                    return format!(
                        "I couldn't read {arg}: {e}. Name an HTML file I can reach, or paste the \
                         markup."
                    )
                }
            }
        };

        let rules = self.tools_cfg().taste.clone();
        let findings = crate::taste::review(&html, &rules);
        let mut out = crate::taste::spoken(&findings);

        let blocking = crate::taste::blocking(&findings);
        if !blocking.is_empty() {
            out.push_str(&format!(" In {source}:"));
            for f in &blocking {
                out.push_str(&format!("\n  • {}", f.detail));
            }
        }
        let advisory: Vec<&crate::taste::Finding> =
            findings.iter().filter(|f| f.severity == crate::taste::Severity::Advisory).collect();
        if !advisory.is_empty() {
            out.push_str("\nWorth a look:");
            for f in &advisory {
                out.push_str(&format!("\n  • {}", f.detail));
            }
        }
        out
    }

    /// Learn a whole body of knowledge at once — a paste, or a file.
    ///
    /// This is how the knowledge base grows vast: a document, a page of notes,
    /// a reference sheet is broken into many discrete facts, each tagged and
    /// indexed and folded into the one book with merge-on-restate, so nothing
    /// duplicates and everything is recalled the same fast way. Reference
    /// knowledge — kept, never an evictable guess.
    pub(super) fn learn_knowledge(&mut self, what: &str) -> String {
        let arg = what.trim().trim_start_matches(':').trim();
        if arg.is_empty() {
            return "Give me something to learn — paste some text after \"learn this\", or name a \
                    file with \"learn from …\"."
                .into();
        }
        let now = crate::store::now();
        // A whole folder of reference material, imported in one pass (bounded so
        // a large tree can't stall the machine) — the way a knowledge base grows
        // vast without pasting a hundred documents by hand.
        if std::fs::metadata(arg).map(|m| m.is_dir()).unwrap_or(false) {
            return self.learn_folder(arg, now);
        }
        // A file named: read for what it is -- a PDF or Word file isn't text
        // (1 Oct 2026: "learn from book.pdf" failed to read as text and the
        // path itself was learned as a fact). A name that isn't there is said.
        let low = arg.to_lowercase();
        let path_shaped = arg.contains('/') || arg.contains('\\')
            || [".pdf", ".docx", ".txt", ".md", ".epub", ".rtf", ".html"].iter().any(|e| low.ends_with(e));
        let p = std::path::Path::new(arg);
        if path_shaped && !p.is_file() {
            return format!("I can't find a file called {arg}. Give me its full path.");
        }
        if p.is_file() && (low.ends_with(".pdf") || low.ends_with(".docx")) {
            let text = if low.ends_with(".pdf") {
                match std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| crate::pdftext::read(&b).map(|pdf| pdf.text)) {
                    Ok(t) => t,
                    Err(e) => return format!("I couldn't read {arg}: {e}."),
                }
            } else {
                match crate::unpack::docx_text(p) {
                    Ok(t) => t,
                    Err(e) => return format!("I couldn't read {arg}: {e}."),
                }
            };
            return self.keep_on_the_reading_shelf(p, &text);
        }
        // A readable file path, or the text itself.
        let (text, whence) = match std::fs::read_to_string(arg) {
            // A long text file is a document to look things up in, not a few
            // facts about you: it goes on the reading shelf, whole.
            Ok(t) if t.len() > READING_SHELF_OVER => return self.keep_on_the_reading_shelf(p, &t),
            Ok(t) => (t, format!("\"{arg}\"")),
            Err(_) => (arg.to_string(), "what you gave me".to_string()),
        };
        let chunks = crate::facts::into_facts(&text);
        if chunks.is_empty() {
            return "There wasn't anything in that I could turn into facts to remember.".into();
        }
        let mut learned = 0u32;
        for chunk in &chunks {
            self.facts.learn(crate::facts::reference_fact(chunk, now), now);
            learned += 1;
        }
        self.facts.trim(crate::facts::MEMORY_BUDGET_BYTES, now);
        let _ = self.facts.save(&self.store);
        format!(
            "Learned {learned} thing{} from {whence}. Ask \"what do you know about …\" and I'll have it.",
            if learned == 1 { "" } else { "s" }
        )
    }

    /// A document kept whole in the reading folder, where search finds it a
    /// chunk at a time and says where (`recall::add_readings`), rather than
    /// cut into "facts" about you.
    fn keep_on_the_reading_shelf(&mut self, from: &std::path::Path, text: &str) -> String {
        if text.trim().is_empty() {
            return "I opened it and there's no text in it I can read -- if it's a scan, say \"read\" and the file, and I'll read the pages.".into();
        }
        let dir = crate::roots::data_sub("reading");
        let _ = std::fs::create_dir_all(&dir);
        let name = from.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
        let kept = dir.join(format!("{name}.txt"));
        if let Err(e) = std::fs::write(&kept, text) {
            return format!("I read it but couldn't keep it ({e}).");
        }
        self.reload_library();
        let words = text.split_whitespace().count();
        format!("Kept {name} ({words} words) on my reading shelf. Ask me about it and I'll answer from it and say where it says so.")
    }

    /// Import every readable text file under a folder in one pass.
    ///
    /// Bounded on purpose — a cap on files read and a per-file size limit, and
    /// it skips the folders that are never knowledge (`.git`, `target`,
    /// `node_modules`) and any file it can't read as text. That is what lets you
    /// point Atlas at a whole notes folder and have it learn the lot without a
    /// deep tree stalling the machine or a binary being turned into nonsense
    /// facts.
    fn learn_folder(&mut self, dir: &str, now: u64) -> String {
        const MAX_FILES: usize = 200;
        const MAX_BYTES: u64 = 5 * 1024 * 1024;
        const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".obsidian", ".venv"];
        let mut files = 0usize;
        let mut learned = 0u32;
        let mut skipped = 0usize;
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(p) = stack.pop() {
            if files >= MAX_FILES {
                break;
            }
            let Ok(entries) = std::fs::read_dir(&p) else { continue };
            for e in entries.flatten() {
                let path = e.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if !SKIP_DIRS.contains(&name) {
                        stack.push(path);
                    }
                    continue;
                }
                if files >= MAX_FILES {
                    break;
                }
                if path.metadata().map(|m| m.len() > MAX_BYTES).unwrap_or(true) {
                    skipped += 1;
                    continue;
                }
                match std::fs::read_to_string(&path) {
                    Ok(t) => {
                        for chunk in crate::facts::into_facts(&t) {
                            self.facts.learn(crate::facts::reference_fact(&chunk, now), now);
                            learned += 1;
                        }
                        files += 1;
                    }
                    // Not text (binary, non-UTF8): left alone rather than
                    // turned into garbage facts.
                    Err(_) => skipped += 1,
                }
            }
        }
        if files == 0 {
            return "I couldn't read any text files in that folder.".into();
        }
        self.facts.trim(crate::facts::MEMORY_BUDGET_BYTES, now);
        let _ = self.facts.save(&self.store);
        let tail = if skipped > 0 {
            format!(" ({skipped} file{} weren't text and were left alone.)", if skipped == 1 { "" } else { "s" })
        } else {
            String::new()
        };
        format!(
            "Learned {learned} thing{} from {files} file{} in that folder.{tail} Ask \"what do you know about …\" and I'll have it.",
            if learned == 1 { "" } else { "s" },
            if files == 1 { "" } else { "s" }
        )
    }

    /// Explain the staged change as behaviour, not code.
    ///
    /// The self-fix path leads its confirmation with a one-line behaviour
    /// summary; this is the fuller form, for when you want it — what the change
    /// will now do, what it no longer promises, what it touched, and what's
    /// worth watching — read from the tests it adds and drops, with no diff. It
    /// only reads what's already staged; nothing here lands or discards.
    pub(super) fn plain_change(&self) -> String {
        match &self.pending_change_effect {
            Some(effect) => {
                let mut out = crate::plainchange::written(effect);
                out.push_str(&format!("\n{}", crate::plainchange::ask(effect)));
                out
            }
            None => "Nothing of mine is staged right now, so there's no change to explain. Set me on \
                     a fix first and I'll tell you what it'll do before you land it."
                .into(),
        }
    }

    /// Make an animation: "animate a bouncing ball, 600x400, for 3 seconds".
    ///
    /// Draws it as a self-contained SVG with the local model, checks that it
    /// renders and matches the size/duration asked for, saves it where you can
    /// open it, and reports honestly. The check is the reliable half — it does
    /// not, and does not pretend to, judge whether the motion looks good; that
    /// is what opening the file is for. Nothing is sent anywhere; it's a file.
    pub(super) fn animate(&mut self, what: &str) -> String {
        let idea = what.trim();
        if idea.is_empty() {
            return "What should I animate? Try \"animate a bouncing ball, 600x400, for 3 seconds\"."
                .into();
        }
        // "animate a bouncing ball in 3d" is a moving 3-D scene, not an SVG.
        let low = idea.to_lowercase();
        if ["3d", "3-d", "three d", "3 d ", "three-dimensional"].iter().any(|w| low.contains(w)) {
            return self.scene(idea);
        }
        let Some(llm) = self.llm.clone() else {
            return "I can draw an animation, but I need a model to draft it and none is configured."
                .into();
        };
        let spec = crate::motion::MotionSpec::from_words(idea);

        // Draw it and iterate against the check — the model drafts, the check
        // decides, up to the shared fix-round budget. Nothing here judges
        // whether it looks good; that's what opening the file is for.
        let rounds_budget = self.tools_cfg().build.max_fix_rounds;
        let outcome = crate::motion::draw_loop(&spec, llm.as_ref(), rounds_budget, |s| {
            crate::motion::check(s, &spec)
        });
        let (svg, findings, rounds, clean) = match outcome {
            crate::motion::Outcome::NoDraft(why) => {
                return format!("I tried, but {why}. Ask me to try again.")
            }
            crate::motion::Outcome::Drawn { svg, rounds, notes } => (svg, notes, rounds, true),
            crate::motion::Outcome::Struggled { svg, rounds, findings } => {
                (svg, findings, rounds, false)
            }
        };

        // Save it either way — even a flawed draft is worth opening — under a
        // name that says whether it passed the checks.
        let dir = crate::roots::data_sub("animations");
        let _ = std::fs::create_dir_all(&dir);
        let name = if clean { "animation" } else { "animation.draft" };
        let path = dir.join(format!("{name}.svg"));
        if let Err(e) = std::fs::write(&path, &svg) {
            return format!("I drew it, but couldn't save it: {e}.");
        }
        self.last_animation = Some((path.clone(), spec.clone(), crate::store::now()));

        let mut said = crate::motion::spoken(&findings);
        if clean && rounds > 0 {
            said.push_str(&format!(
                " Took {rounds} fix{} to get there.",
                if rounds == 1 { "" } else { "es" }
            ));
        }
        said.push_str(&format!(" Saved to {}.", path.display()));

        // If a rasteriser is configured and the SVG itself is sound, render a
        // PNG still next to it and check what came out. No rasteriser → the SVG
        // stands on its own (it renders in any browser), said plainly rather
        // than pretended.
        let render_cmd = self.tools_cfg().build.render_svg_command.clone();
        if clean && !render_cmd.trim().is_empty() {
            let png = dir.join("animation.png");
            let expect = crate::motion::Expect {
                kind: crate::motion::RenderKind::Png,
                width: spec.width,
                height: spec.height,
            };
            match crate::motion::render(&path, &png, &render_cmd, &expect) {
                Ok(render_findings) => {
                    if crate::motion::blocking(&render_findings).is_empty() {
                        said.push_str(&format!(" Rendered a PNG to {}.", png.display()));
                    } else {
                        said.push_str(" I rendered it, but the result didn't check out:");
                    }
                    for f in &render_findings {
                        said.push_str(&format!("\n  • {}", f.detail));
                    }
                }
                Err(why) => said.push_str(&format!(" (No PNG: {why}.)")),
            }
        }

        // Played and saved as a GIF (and an MP4 with ffmpeg) when there's a
        // browser to play it in — Edge on any Windows machine. `filmstrip`.
        if clean {
            let tc = self.tools_cfg();
            match crate::filmstrip::find_browser(tc.vars.get("browser").map(|s| s.as_str())) {
                Some(browser) => {
                    let plan = crate::filmstrip::Plan::for_svg(&svg, &spec, 12);
                    let ffmpeg = tc.vars.get("ffmpeg").cloned().unwrap_or_else(|| "ffmpeg".into());
                    match crate::filmstrip::film(&svg, &plan, &browser, Some(&ffmpeg), &dir, name) {
                        Ok(made) => said.push_str(&format!(" {}", made.say())),
                        Err(e) => said.push_str(&format!(" (No GIF: {e}.)")),
                    }
                }
                None => said.push_str(" (No GIF: that needs Edge or Chrome to play it in, and I didn't find either.)"),
            }
        }

        for f in crate::motion::blocking(&findings) {
            said.push_str(&format!("\n  • {}", f.detail));
        }
        said
    }

    /// "Make it faster", "make it red", "a bit slower and bigger" -- said
    /// within the hour after an animation was drawn, an edit to that one
    /// (`motion::refine`), done here with no model, checked the same way,
    /// and saved beside it as the next version.
    pub(super) fn refine_animation(&mut self, said: &str, t: u64) -> Option<String> {
        let (path, spec, at) = self.last_animation.clone()?;
        if t.saturating_sub(at) > 3600 {
            return None;
        }
        let low = said.to_lowercase();
        let about_it = ["make it", "make the animation", "now make it", "can you make it", "and make it", "faster", "slower", "speed it up", "slow it down"]
            .iter()
            .any(|p| low.trim_start().starts_with(p));
        if !about_it {
            return None;
        }
        let svg = std::fs::read_to_string(&path).ok()?;
        let r = crate::motion::refine(&svg, said)?;
        let mut spec = spec;
        if let Some(d) = r.duration_secs {
            spec.duration_secs = d;
        }
        if let Some((w, h)) = r.size {
            spec.width = w;
            spec.height = h;
        }
        let findings = crate::motion::check(&r.svg, &spec);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("animation");
        let base = stem.split(".v").next().unwrap_or(stem).to_string();
        let n = stem.rsplit(".v").next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(1) + 1;
        let next = path.with_file_name(format!("{base}.v{n}.svg"));
        if let Err(e) = std::fs::write(&next, &r.svg) {
            return Some(format!("I changed it, but couldn't save it: {e}."));
        }
        self.last_animation = Some((next.clone(), spec, t));
        let mut out = format!("Made it {}. Saved to {} (the one before is still there).", r.changes.join(", "), next.display());
        let problems = crate::motion::blocking(&findings);
        if problems.is_empty() {
            out.push_str(" It still checks out: renders, right size, right length.");
        } else {
            for f in problems {
                out.push_str(&format!("\n  • {}", f.detail));
            }
        }
        Some(out)
    }

    /// Draw a 3-D scene: "draw a 3d scene of a red ball on a box".
    ///
    /// A model drafts the scene as JSON (shapes, colours, a sun, a camera);
    /// Atlas draws it itself — a still and a turntable GIF — and in Blender
    /// too when Blender is installed. Checked: it reads, it has something in
    /// view, the picture isn't just sky. Not checked: whether it looks right.
    pub(super) fn scene(&mut self, what: &str) -> String {
        let idea = what.trim();
        if idea.is_empty() {
            return "What should I draw? Try \"draw a 3d scene of a red ball on a blue box\".".into();
        }
        let Some(llm) = self.llm.clone() else {
            return "I can draw a 3-D scene, but I need a model to describe it first and none is configured.".into();
        };
        let rounds = self.tools_cfg().build.max_fix_rounds;
        // Models Eric has put in the models folder can be placed by name.
        let models = crate::roots::data_sub("models");
        let known = crate::scene3d::model_files(&models);
        let ask = if known.is_empty() {
            idea.to_string()
        } else {
            format!("{idea}\n\nModel files you may place (shape \"mesh\"): {}", known.join(", "))
        };
        let scene = match crate::scene3d::draft_scene(&ask, llm.as_ref(), rounds, Some(&models)) {
            Ok(s) => s,
            Err(e) => return format!("I tried, but the scene never came together: {e}."),
        };

        let dir = crate::roots::data_sub("animations");
        let tc = self.tools_cfg();
        let blender = crate::scene3d::find_blender(tc.vars.get("blender").map(|s| s.as_str()));
        match crate::scene3d::make(&scene, &dir, "scene", 24, blender.as_deref()) {
            Ok(made) => made.say(),
            Err(e) => format!("I described it but couldn't draw it: {e}."),
        }
    }

    /// Explain code in plain English: "explain this: <code>" or "explain
    /// src/foo.rs".
    ///
    /// Writes a non-coder explanation with the local model and iterates it
    /// against the plain-language check — no leaked code, the right length,
    /// jargon flagged. The check is the reliable half: it never claims the
    /// explanation is *correct* about the code, only that it reads like an
    /// explanation. Reading only — it changes nothing.
    pub(super) fn explain_code(&mut self, what: &str) -> String {
        let arg = what.trim();
        // Strip a lead-in like "this:" / "this code:" so the rest is the code.
        let arg = arg
            .strip_prefix("this code:")
            .or_else(|| arg.strip_prefix("this:"))
            .or_else(|| arg.strip_prefix("this code"))
            .or_else(|| arg.strip_prefix("this"))
            .unwrap_or(arg)
            .trim();
        if arg.is_empty() {
            return "What should I explain? Point me at a file — \"explain src/foo.rs\" — or paste \
                    the code."
                .into();
        }

        // What to explain, in order: something I built recently, a change
        // waiting to be implemented, a file you named, or code you pasted.
        let low = arg.to_lowercase();
        let (code, source) = if references_a_build(&low) {
            match self.latest_build_code() {
                Ok(p) => p,
                Err(e) => return e,
            }
        } else if references_a_queued_change(&low)
            || self.names_a_ready_change(&low)
        {
            match self.queued_change_code(&low) {
                Some(p) => p,
                None => return "There's nothing waiting to be implemented right now.".into(),
            }
        } else {
            // A single-line, path-shaped argument that names a real file is
            // read; anything else is treated as pasted code.
            let looks_like_path = !arg.contains('\n') && arg.split_whitespace().count() == 1;
            if looks_like_path && std::path::Path::new(arg).is_file() {
                match std::fs::read_to_string(arg) {
                    Ok(text) => (text, format!("\"{arg}\"")),
                    Err(e) => return format!("I couldn't read {arg}: {e}."),
                }
            } else if !looks_like_code(arg) {
                // "explain code the quarterly budget" isn't code: the model
                // explained it anyway, as if it were (the laptop's self-test,
                // 30 Sep 2026). A path that isn't there is said as such.
                if looks_like_path {
                    return format!("I can't find a file called {arg}. Give me its full path, or paste the code.");
                }
                // "explain how a heat pump works" reached here by the
                // model's choice (1 Oct 2026 model ranking): it's a question,
                // so it's answered as one rather than refused as not-code.
                if let Some(llm) = self.llm.clone() {
                    let system = "Answer the question plainly in two or three spoken sentences. No lists, no markdown, no follow-up question.";
                    if let Ok(text) = llm.complete(system, &format!("Explain {arg}")) {
                        let text = crate::phonemodel::without_thinking(&text).trim().to_string();
                        if !text.is_empty() {
                            return text;
                        }
                    }
                }
                return format!("\"{arg}\" doesn't look like code. Point me at a file -- \"explain src/foo.rs\" -- or paste the code.");
            } else {
                (arg.to_string(), "the code you gave me".to_string())
            }
        };

        let Some(llm) = self.llm.clone() else {
            return "I can explain it, but I need a model to write the explanation and none is \
                    configured."
                .into();
        };

        // The depth dial, read from the request ("like I'm five", "in detail").
        let depth = crate::explain::Depth::from_words(&low);
        let rounds_budget = self.tools_cfg().build.max_fix_rounds;
        let outcome = crate::explain::explain_loop(&code, llm.as_ref(), rounds_budget, depth, |t| {
            crate::explain::check_at(t, depth)
        });
        let (text, findings, rounds) = match outcome {
            crate::explain::Outcome::NoDraft(why) => {
                return format!("I tried to explain {source}, but {why}.")
            }
            crate::explain::Outcome::Explained { text, rounds, notes } => (text, notes, rounds),
            crate::explain::Outcome::Struggled { text, rounds, findings } => (text, findings, rounds),
        };

        let mut said = text;
        said.push_str("\n\n");
        said.push_str(&crate::explain::spoken(&findings));
        if rounds > 0 {
            said.push_str(&format!(
                " (Took {rounds} rewrite{} to read plainly.)",
                if rounds == 1 { "" } else { "s" }
            ));
        }
        for f in crate::explain::blocking(&findings) {
            said.push_str(&format!("\n  • {}", f.detail));
        }
        said
    }

    /// The most recent thing Atlas built, as code to explain.
    fn latest_build_code(&self) -> std::result::Result<(String, String), String> {
        let dir = crate::roots::data_sub("builds");
        let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let m = e.metadata().ok()?.modified().ok()?;
                p.is_file().then_some((p, m))
            })
            .collect();
        files.sort_by_key(|(_, m)| *m);
        match files.last() {
            Some((p, _)) => {
                let code = std::fs::read_to_string(p)
                    .map_err(|e| format!("I found what I built but couldn't read it: {e}."))?;
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                Ok((code, format!("what I built ({name})")))
            }
            None => Err("I haven't built anything recently — ask me to build something first, \
                         then I'll explain it."
                .into()),
        }
    }

    /// Does the request name a change that's ready to implement?
    fn names_a_ready_change(&self, low: &str) -> bool {
        self.workshop.projects.iter().flat_map(|p| p.ready()).any(|c| {
            let t = c.title.to_lowercase();
            !t.is_empty() && low.contains(&t)
        })
    }

    /// A change waiting to be implemented, as code to explain: the one named in
    /// the request, or the most recent if none is named.
    fn queued_change_code(&self, low: &str) -> Option<(String, String)> {
        let ready: Vec<&crate::workshop::Change> =
            self.workshop.projects.iter().flat_map(|p| p.ready()).collect();
        let picked = ready
            .iter()
            .find(|c| {
                let t = c.title.to_lowercase();
                !t.is_empty() && low.contains(&t)
            })
            .copied()
            .or_else(|| ready.iter().max_by_key(|c| c.created).copied())?;
        let code = picked
            .files
            .iter()
            .map(|f| format!("// {}\n{}", f.path, f.content))
            .collect::<Vec<_>>()
            .join("\n\n");
        Some((code, format!("the \"{}\" change waiting to be implemented", picked.title)))
    }

    fn detect_project(&self, what: &str) -> Option<String> {
        let low = what.to_ascii_lowercase();
        // A registered project named anywhere in the request.
        for p in &self.workshop.projects {
            let n = p.name.to_ascii_lowercase();
            if !n.is_empty() && low.contains(&n) {
                return Some(p.name.clone());
            }
        }
        // "the X project" / "on X" / "in X" — take the word after the lead.
        for lead in ["the ", "on ", "in ", "for ", "project "] {
            if let Some(i) = low.find(lead) {
                let rest = &what[i + lead.len()..];
                let word: String =
                    rest.split_whitespace().next().unwrap_or("").chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
                // "the date parser" shouldn't name a project "date"; only take
                // it when the phrase is "<name> project" or the lead was
                // "project".
                let followed_by_project = rest.to_lowercase().contains("project");
                if !word.is_empty() && (lead == "project " || followed_by_project) {
                    return Some(word);
                }
            }
        }
        None
    }

    /// Write a fresh master document for a project — what it is, where it
    /// lives, and its recent implemented changes — and return where it landed.
    fn write_project_master_doc(&self, project: &str) -> Option<String> {
        let p = self.workshop.resolve(project)?;
        let mut doc = format!("# {} — master document\n\n", p.name);
        if !p.folder.is_empty() {
            doc.push_str(&format!("Folder: {}\n\n", p.folder));
        }
        doc.push_str("## Implemented changes\n\n");
        let mut any = false;
        for c in p.changes.iter().filter(|c| c.state == crate::workshop::State::Implemented) {
            any = true;
            doc.push_str(&format!("- **{}** — {}\n", c.title, c.what));
        }
        if !any {
            doc.push_str("- (none yet)\n");
        }
        if !p.outstanding().is_empty() {
            doc.push_str("\n## Outstanding\n\n");
            for t in p.outstanding() {
                doc.push_str(&format!("- {}\n", t.title));
            }
        }
        let dir = crate::roots::data_sub("projects");
        let _ = std::fs::create_dir_all(&dir);
        let slug: String =
            p.name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
        let path = dir.join(format!("{slug}.master.md"));
        std::fs::write(&path, doc).ok().map(|_| path.display().to_string())
    }
}

/// Does pasted text look like code rather than words? Brackets, semicolons,
/// operators or keywords, and more than a few of them.
fn looks_like_code(text: &str) -> bool {
    let marks = text.chars().filter(|c| matches!(c, '{' | '}' | '(' | ')' | ';' | '=' | '<' | '>' | '[' | ']')).count();
    let words = [" fn ", "def ", "function", "return", "import ", "class ", "let ", "const ", "var ", "#include", "=>", "->", "public ", "SELECT "];
    let keyword = words.iter().any(|w| text.contains(w));
    marks >= 3 || (keyword && marks >= 1) || text.contains('\n') && marks >= 1
}
