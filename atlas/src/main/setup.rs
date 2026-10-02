//! Setting Atlas up and looking after it: craft, doctor, fixes, the voice lab
//! and enrolment, adapting, the walkthrough, quick input, startup, backends,
//! WireGuard, home, getting pieces, add-ons, edits, refusals, reclaiming space
//! and the call check.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

pub(super) fn run_craft() {
    use atlas::craft::{self, Next};

    let Some(dir) = first_bare_arg("craft") else {
        println!(
            "Which project? Try:\n\n  atlas craft path/to/the/project\n\n\
             I look for Cargo.toml or a Python project file in that folder to \
             know which ladder to run."
        );
        return;
    };
    let dir = std::path::PathBuf::from(&dir);
    if !dir.is_dir() {
        println!("{} isn't a folder I can see.", dir.display());
        return;
    }
    let Some(lang) = atlas::craft::lang_of_dir(&dir) else {
        println!(
            "I can't tell what this project is — no Cargo.toml, no pyproject.toml, \
             setup.py, setup.cfg, or requirements.txt in {}.",
            dir.display()
        );
        return;
    };

    println!("Running the {} ladder in {} — cheapest checks first.\n", lang.plain(), dir.display());

    let mut ran: Vec<craft::Ran> = Vec::new();
    loop {
        let todo = craft::still_worth_running(lang, &ran);
        let Some(gate) = todo.first() else { break };
        print!("  {} ... ", gate.command);
        let _ = io::stdout().flush();
        let r = run_gate(&dir, gate);
        println!("{}", if r.passed { "ok" } else { "failed" });
        ran.push(r);
        // Stop as soon as something blocking has failed -- running the rest
        // would only produce noise from code the earlier failure already
        // explains. `still_worth_running` encodes exactly this on the next
        // loop, but there is no point paying for a gate whose result cannot
        // change what happens next.
        if let Next::Fix { .. } = craft::read_ladder(lang, &ran) {
            if ran.last().map(|r| !r.passed && r.tells.blocks_later()).unwrap_or(false) {
                break;
            }
        }
    }

    match craft::read_ladder(lang, &ran) {
        Next::Good => println!("\nEverything passed."),
        Next::WorksWithNotes(notes) => {
            println!("\nIt works. Worth a look, not blocking:");
            for n in notes {
                println!("  - {n}");
            }
        }
        Next::CannotCheck { program, gate } => {
            println!("\nCouldn't check it: {program} isn't installed on this computer (needed for `{}`).", gate.command);
        }
        Next::Fix { gate, output } => {
            println!("\n{} — {}\n", gate.on_fail, gate.command);
            // The tool's own words, verbatim -- see craft.rs on why a
            // paraphrase loses the line number, which is the useful part.
            let head: String = output.lines().take(20).collect::<Vec<_>>().join("\n");
            println!("{head}");
            if output.lines().count() > 20 {
                println!("... ({} more lines)", output.lines().count() - 20);
            }
        }
    }
}

pub(super) fn run_doctor(cfg: &Config, plat: &dyn Platform) {
    println!("atlas doctor\n");
    let mut findings = doctor::run(cfg, cfg.tools.as_ref(), plat);
    // The machine itself. Absent until today, which is how a readings stub
    // survived every clean doctor run there has ever been: a check that never
    // looks at a thing cannot fail on it.
    findings.extend(doctor::machine_findings());
    findings.push(doctor::hearing_finding(cfg.tools.as_ref()));
    let mut bad = 0;
    for f in &findings {
        let mark = if f.ok { "  ok " } else { bad += 1; "FAIL " };
        println!("{mark}{:<18} {}", f.label, f.detail);
    }
    println!();
    // Source code for the developer's dry runs, never shown to the person
    // setting Atlas up: Eric's rule is no code in what he sees. Set
    // ATLAS_DEV to get it.
    if std::env::var_os("ATLAS_DEV").is_some() {
        if let Ok(m) = plat.monitors() {
            println!("Paste into src/main.rs so dry runs match this desk:\n");
            println!("{}", doctor::monitor_fixture(&m));
        }
    }
    if bad == 0 {
        println!("All checks passed.");
    } else {
        println!("{bad} problem(s). Fix these before expecting anything to work.");
    }
}

/// `atlas fix <folder> <what you wanted> -- <test command>` — work a failing
/// test with the model until it passes (`fixloop`), in a copy of the folder.
/// Shows the tested diff; `--land` puts it in the folder, originals kept.
pub(super) fn run_fix(cfg: &Config, args: &[String]) {
    let Some(split) = args.iter().position(|a| a == "--") else {
        return println!("atlas fix <folder> <what you wanted> -- <test command> [--land]");
    };
    let land = args.iter().any(|a| a == "--land");
    let head: Vec<&String> = args[..split].iter().filter(|a| *a != "--land").collect();
    let test: Vec<String> = args[split + 1..].iter().filter(|a| *a != "--land").cloned().collect();
    let Some(folder) = head.first() else { return println!("which folder?") };
    let goal = head[1..].iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
    let Some(tc) = cfg.tools.as_ref() else { return println!("no config/tools.yaml") };
    let Some(llm) = model_connection(tc) else {
        return println!("There's no model to work it with. Set one up (the models folder, or `llm:` in tools.yaml) and try again.");
    };
    let job = atlas::fixloop::Job { folder: std::path::PathBuf::from(folder.as_str()), test, goal };
    let mut counsel = atlas::fixloop::ModelCounsel::new(llm.as_ref());
    match atlas::fixloop::run(&job, &mut counsel, &atlas::roots::tmp_dir().join("fix"), &tc.strategy, &tc.handoff, &tc.consult) {
        Ok(o) => {
            for s in &o.steps {
                println!("  {s}");
            }
            if o.solved {
                println!("\nFixed and tested in a copy ({}), {} attempt(s):\n\n{}", o.copy.display(), o.attempts, o.diff);
                if land {
                    match atlas::fixloop::land(&o, &job.folder) {
                        Ok(f) => println!("Landed: {} (each original kept as .before).", f.join(", ")),
                        Err(e) => println!("{e}"),
                    }
                } else {
                    println!("Nothing in your folder has changed. Run it again with --land to put this in.");
                }
            } else if let Some(b) = o.brief {
                println!("\nDidn't get there. The write-up, for whoever picks it up:\n\n{b}");
            }
        }
        Err(e) => println!("I couldn't work on it: {e}. If that's the model, is its server running? Nothing in your folder changed."),
    }
}

/// `atlas wake-word`, `atlas hearing`, `atlas voices` — teaching Atlas your
/// phrase, your room and the voices that aren't you, from recordings. The
/// hub's calendar page does the same with buttons.
pub(super) fn run_voice_lab(cfg: &Config, words: &[String]) {
    let store = atlas::roots::store();
    let read = |p: &String| -> Option<(Vec<i16>, u32)> {
        match std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| atlas::diarize::read_wav(&b)) {
            Ok(x) => Some(x),
            Err(e) => {
                println!("{p}: {e}");
                None
            }
        }
    };
    match (words[0].as_str(), words.get(1).map(|s| s.as_str())) {
        ("wake-word", Some("forget")) => match atlas::wakeword::forget(&store) {
            Ok(()) => println!("Forgotten. I'm back to listening for the phrase through speech-to-text."),
            Err(e) => println!("couldn't forget it: {e}"),
        },
        ("wake-word", Some("test")) => {
            let Some(m) = atlas::wakeword::load(&store) else { return println!("No phrase taught yet: atlas wake-word <take.wav> (three times).") };
            for p in &words[2..] {
                if let Some((s, r)) = read(p) {
                    let c = atlas::wakeword::closest(&s, r, &m);
                    println!("{p}: {} (closest {:.2}, heard at or under {:.2})", if atlas::wakeword::heard(&s, r, &m) { "heard" } else { "not heard" }, c.map(|x| x.0).unwrap_or(f32::INFINITY), m.threshold);
                }
            }
        }
        ("wake-word", Some(_)) => {
            for p in &words[1..] {
                if let Some((s, r)) = read(p) {
                    match atlas::wakeword::add_take(&store, &s, r) {
                        Ok(said) => println!("{p}: {said}"),
                        Err(e) => println!("{p}: {e}"),
                    }
                }
            }
        }
        ("hearing", Some(room)) if words.len() >= 3 => {
            let current = cfg.tools.as_ref().map(|t| t.endpoint.vad_params()).unwrap_or_default();
            let (Ok(a), Ok(b)) = (std::fs::read(room), std::fs::read(&words[2])) else { return println!("I couldn't open one of those files.") };
            match atlas::vadcal::from_recordings(&a, &b, current, &atlas::roots::config_dir()) {
                Ok(o) => println!("{}", atlas::vadcal::Outcome::say(&o)),
                Err(e) => println!("{e}"),
            }
        }
        ("voices", Some(_)) => {
            for p in &words[1..] {
                if let Some((s, r)) = read(p) {
                    match atlas::speaker::learn_background(&s, r, &store) {
                        Ok(n) => println!("{p}: learned from {n} stretch{} of speech. {}", if n == 1 { "" } else { "es" }, if atlas::speaker::background(&store).ready() { "That's enough to tell voices apart." } else { "" }),
                        Err(e) => println!("{p}: {e}"),
                    }
                }
            }
            if !atlas::speaker::background(&store).ready() {
                println!("{}", atlas::speaker::still_learning(&atlas::speaker::background(&store)));
            }
        }
        _ => {
            println!("atlas wake-word <take.wav> [...]     teach your phrase (three takes)");
            println!("atlas wake-word test <clip.wav>      would that have woken me?");
            println!("atlas wake-word forget               back to speech-to-text");
            println!("atlas hearing <room.wav> <you.wav>   tune the speech detector to your room");
            println!("atlas voices <recording.wav> [...]   learn what voices that aren't you sound like");
        }
    }
}

/// What Atlas turned down, and what the shape of it says.
///
/// The half of the record that nothing kept until now. With only the trades it
/// found, "Atlas hasn't traded this week" has two readings — careful, or
/// broken — and no way to tell them apart.
pub(super) fn run_refusals(args: &[String]) {
    let store = atlas::roots::store();
    let turned_down = atlas::refusals::Refusals::load(&store);
    let pairs: Vec<String> = match args.first() {
        Some(p) => vec![p.to_uppercase()],
        None => {
            let mut all: Vec<String> = turned_down.by_pair.iter().map(|(p, _)| p.clone()).collect();
            for (p, _) in &turned_down.proposed {
                if !all.contains(p) {
                    all.push(p.clone());
                }
            }
            all.sort();
            all
        }
    };
    if pairs.is_empty() {
        println!("I haven't been asked about a market yet, so there's nothing to show.");
        println!("Every `atlas market ...` adds to this, whether it finds a trade or not.");
        return;
    }
    for pair in &pairs {
        println!("{}", turned_down.spoken(pair));
        if let Some((_, causes)) = turned_down.by_pair.iter().find(|(p, _)| p == pair) {
            for c in causes {
                println!("  {:>5}  {}", c.times, c.label);
                for e in &c.examples {
                    println!("         e.g. {e}");
                }
            }
        }
        println!();
    }
}

pub(super) fn run_reclaim(cfg: &Config, go: bool) {
    let now = atlas::store::now();
    // The places Atlas may look, passed in rather than discovered, so how far
    // it can reach is a decision written down here rather than a side effect.
    let roots = atlas::reclaim::roots_from_env();
    if roots.is_empty() {
        eprintln!("I couldn't work out where your home folder is, so I haven't looked anywhere.");
        return;
    }

    println!("Looking at the whole disk. Nothing is moved unless you ask.");
    println!();

    // Two lists, because they are two different decisions.
    let mut found = atlas::reclaim::survey(&roots, now);
    let looked = atlas::reclaim::whole_disk(&roots, now);
    let apps = atlas::reclaim::installed_apps(&roots, now);

    if found.is_empty() && looked.is_empty() && apps.is_empty() {
        println!("{}", atlas::reclaim::spoken(&found));
        // Still say what I'm costing you. "Nothing to reclaim" from a program
        // sitting on 900 MB of its own is the answer that makes you stop
        // trusting the other one.
        report_own_footprint(cfg);
        return;
    }
    if !found.is_empty() {
        println!("I can clear these myself -- they rebuild:");
        for c in found.iter().take(15) {
            println!("  {}", c.line());
        }
    }
    if !looked.is_empty() || !apps.is_empty() {
        println!();
        println!("Yours to judge. I won't touch any of these:");
        for c in looked.iter().chain(apps.iter()).take(25) {
            println!("  {}", c.line());
        }
    }
    let all: Vec<atlas::reclaim::Candidate> =
        found.iter().chain(looked.iter()).chain(apps.iter()).cloned().collect();
    println!();
    println!("{}", atlas::reclaim::report(&all));
    // Atlas's own folder, counted in the same breath as everybody else's.
    report_own_footprint(cfg);

    if !go {
        // The list on its own is the useful part. Acting is a separate
        // decision, made by typing something different.
        println!("
Nothing has been moved. `atlas reclaim --do-it` moves these to Atlas's");
        println!("trash, where they stay for 30 days and can be put back.");
        return;
    }

    let tc = cfg.tools.as_ref();
    let trash = atlas::safety::Trash::new(
        tc.map(|t| t.trash.clone()).unwrap_or_default().resolved(&atlas::roots::install_root()),
    );
    // Only ever the list Atlas may move. The reported half never reaches
    // here, and `reclaim` re-checks every entry regardless.
    found.retain(|c| c.kind.atlas_may_move());
    let (moved, refused) = atlas::reclaim::reclaim(&found, &trash);
    println!("
Moved {} to the trash. They can be put back for 30 days.", moved.len());
    // Never reported as a whole success: a partial reclaim announced as a
    // complete one sends you looking for space that was never freed.
    for (path, why) in &refused {
        println!("  couldn't move {}: {why}", path.display());
    }

    // And what is already stranded in there.
    //
    // Until 18 Sep, `Trash::expire` deleted a folder's ledger entry and then
    // failed to delete the folder — `remove_file` does not remove a
    // directory, and the error was discarded. Every folder ever reclaimed on
    // that code is still in `data/trash` with nothing pointing at it: `undo`
    // cannot find it, `expire` will never look at it again, and the space
    // `reclaim` reported freeing was never freed.
    //
    // Named and measured, never deleted. This is a holding pen for things
    // somebody decided to get rid of, and removing whatever Atlas did not
    // recognise in there is the wrong instinct in the one folder where being
    // wrong cannot be undone.
    let stray = trash.unaccounted();
    if !stray.is_empty() {
        let total: u64 = stray.iter().map(|(_, b)| *b).sum();
        println!(
            "\nThere's {} MB in my trash I have no record of — most likely folders \
             reclaimed before 18 Sep, whose records expired while the folders stayed. \
             Nothing can put these back, so they're yours to delete:",
            total / (1024 * 1024)
        );
        for (path, bytes) in stray.iter().take(10) {
            println!("  {} ({} MB)", path.display(), bytes / (1024 * 1024));
        }
        if stray.len() > 10 {
            println!("  ...and {} more", stray.len() - 10);
        }
    }
}

/// What Atlas's own data folder is costing you, and whether that is in budget.
///
/// `atlas reclaim` looked at your whole disk and named other people's caches
/// while saying nothing at all about Atlas's own folder. A tool that tells you
/// to clear 4 GB of npm cache and does not mention its own 900 MB is the one
/// shape of disk report nobody should ship.
///
/// The numbers existed. `retention::survey` walks the data folder,
/// `retention::usage` groups it by what the file is for, and
/// `Usage::total_mb` is the answer to "how much" — and that method had no
/// production caller at all. It *looked* like it had one until 19 Sep, because
/// `install::total_mb` shared its bare name and the deadness scan reads bare
/// names; renaming that one to `download_mb` is what made this visible.
///
/// Read-only on purpose. `atlas reclaim --do-it` moves things it found on your
/// disk; Atlas's own folder is pruned by the hourly housekeeping pass against
/// the budget in `retention:`, and having two things delete from the same
/// folder on two different rules is how a backup disappears.
fn report_own_footprint(cfg: &Config) {
    let root = atlas::roots::data_dir();
    if !root.exists() {
        return;
    }
    let items = atlas::retention::survey(&root);
    if items.is_empty() {
        return;
    }
    let usage = atlas::retention::usage(&items);
    let rcfg = cfg.tools.as_ref().map(|t| t.retention.clone()).unwrap_or_default();

    println!();
    println!(
        "And me: {} MB in my own folder, against a {} MB budget.",
        usage.total_mb(),
        rcfg.total_budget_mb
    );
    // The breakdown, because "900 MB" invites deleting the folder and the
    // split says which part of it is actually growing.
    //
    // The words are `Class`'s own, not guesses at them: `Scratch` is the wav
    // of the turn you are in, `Captures` is screenshots and webcam frames.
    // Calling `Captures` "recordings" — which the first draft of this did —
    // reads as audio and points at the wrong half of the folder.
    let mb = |b: u64| b / (1024 * 1024);
    println!(
        "  recordings and working files {} MB, screenshots {} MB, logs {} MB, \
         notes {} MB, what I've learned {} MB",
        mb(usage.scratch),
        mb(usage.captures),
        mb(usage.logs),
        mb(usage.notes),
        mb(usage.state)
    );
    if usage.unknown > 0 {
        // Its own line rather than folded into a total, because this is the
        // part Atlas will never delete: a file it cannot date is a file it
        // has no basis for evicting.
        println!("  and {} MB I can't date, so I leave it alone", mb(usage.unknown));
    }
    if usage.not_ours > 0 {
        println!("  plus {} MB in my trash and backups, which I don't prune here", mb(usage.not_ours));
    }
    if atlas::retention::irreducible(&usage, &rcfg) {
        println!(
            "  Notes and what I've learned alone are over the budget — clearing recordings \
             and screenshots won't bring me back under it. Raise `retention.total_budget_mb`."
        );
    } else if usage.total_mb() > rcfg.total_budget_mb {
        println!("  Over budget. The hourly housekeeping pass brings this down on its own.");
    }
}

/// Speak the same line in each shortlisted voice, so you can choose by ear.
///
/// The whole point is that you cannot pick a voice from a description. Atlas
/// says a real sentence — one of the things it actually says — in each
/// candidate, and you keep the one you want to hear every day.
pub(super) fn run_audition(cfg: &Config) {
    let Some(tc) = cfg.tools.as_ref() else {
        eprintln!("no config/tools.yaml, so there's nothing to speak with.");
        return;
    };
    let voice = Voice::new(tc);
    println!("Auditioning {} voices.\n", atlas::tts::SHORTLIST.len());
    println!("{}\n", atlas::tts::KOKORO_HAS_NO_RANGE);

    for (id, why) in atlas::tts::SHORTLIST {
        println!("  {id} — {why}");
        // Spoken through the configured engine with the settings you already
        // have, so what you hear is what you would get.
        let line = format!(
            "This is {}. Your nine o'clock post is over length by twelve characters.",
            id.rsplit('_').next().unwrap_or(id)
        );
        match voice.say_as(id, &line) {
            Ok(()) => {}
            // Never silently skip: a voice you did not hear is one you cannot
            // choose, and a silent audition looks like a voice you disliked.
            Err(e) => eprintln!("      (couldn't speak that one: {e})"),
        }
    }
    println!("\nSet the one you want as `voice_settings.voice` in config/tools.yaml.");
}

/// Record a few samples and learn the voice in them.
///
/// Deliberately several: one recording captures one mood, one distance and one
/// microphone. `min_samples` in `tools.yaml` is what `voiceid` will trust.
pub(super) fn run_enrol_voice(cfg: &Config, plat: &dyn Platform) {
    let Some(tc) = cfg.tools.as_ref() else {
        eprintln!("no config/tools.yaml, so there's nothing to record with.");
        return;
    };
    let vars = tc.vars.clone();
    if !atlas::speaker::available(&tc.speaker, &vars) {
        // Said before anything is recorded, rather than after three samples.
        eprintln!("{}", atlas::speaker::NO_ENCODER);
        eprintln!("{}", atlas::speaker::still_learning(&atlas::speaker::background(&atlas::roots::store())));
        return;
    }
    // The microphone this machine has, as the other doors pick it (29 Sep
    // 2026: this recorded from tools.yaml's guess).
    let tc_owned = pick_the_microphone(cfg, plat, tc);
    let tc = &tc_owned;
    let voice = Voice::new(tc);
    let store = atlas::roots::store();
    let mut id = atlas::voiceid::VoiceId::load(&store);
    let needed = tc.voice_id.min_samples.max(1);

    println!("Teaching Atlas your voice. {needed} short recordings.\n");
    while id.enrolled() < needed {
        println!("{}", atlas::voiceid::enrollment_prompt(id.enrolled(), needed));
        print!("[enter when ready] ");
        let _ = io::stdout().flush();
        let mut l = String::new();
        if matches!(io::stdin().read_line(&mut l), Ok(0) | Err(_)) {
            return;
        }
        if voice.listen().is_err() {
            eprintln!("(didn't catch that — try again)");
            continue;
        }
        match voice.voiceprint() {
            Some(print) => match id.enroll(&print) {
                Ok(n) => println!("  sample {n} of {needed}."),
                Err(e) => eprintln!("(couldn't use that one: {e})"),
            },
            None => eprintln!("(the encoder didn't return anything usable — try again)"),
        }
    }
    match id.save(&store) {
        Ok(()) => println!("\n{}", atlas::voiceid::enrollment_prompt(id.enrolled(), needed)),
        // Never claim it learned something it did not keep.
        Err(e) => eprintln!("\nI learned it but couldn't save it ({e}) — it won't survive a restart."),
    }
}

/// `atlas adapt` — work out what this particular computer has.
///
/// The other half of the two-layer config split `adapt.rs` describes and
/// nothing implemented: `config/*.yaml` is the recipe that ships to anyone,
/// `config/machine.yaml` is what this kitchen actually has. Until this
/// existed, the second file was never written, so the first one carried a
/// hardcoded Chrome path and a Realtek microphone name and was wrong for
/// every machine but one.
pub(super) fn run_adapt(cfg: &Config, plat: &dyn Platform, args: &[String]) {
    let dir = atlas::roots::config_dir();
    let dir = dir.as_path();

    if args.first().map(|s| s.as_str()) == Some("show") {
        match atlas::adapt::Machine::load(dir) {
            Some(m) => {
                println!("{}", m.summary());
                let wanted: Vec<String> = cfg.apps.apps.keys().cloned().collect();
                println!("Apps found: {:.0}% of the {} configured.", m.coverage(&wanted) * 100.0, wanted.len());
                match plat.monitors() {
                    Ok(mons) if !m.matches(&mons) => println!(
                        "The displays have changed since this was written — run `atlas adapt` again."
                    ),
                    _ => {}
                }
                for n in &m.notes {
                    println!("  - {n}");
                }
            }
            None => println!("Nothing worked out yet. Run `atlas adapt`."),
        }
        return;
    }

    if args.first().map(|s| s.as_str()) == Some("fixture") {
        match atlas::adapt::Machine::load(dir) {
            // The point of this is a dry run whose monitor geometry matches
            // the real desk, rather than the invented one in `fake_monitors`.
            Some(m) => print!("{}", atlas::adapt::monitor_fixture(&m)),
            None => println!("Nothing worked out yet. Run `atlas adapt`."),
        }
        return;
    }

    let app_names: Vec<String> = cfg.apps.apps.keys().cloned().collect();

    // How an app is found: take whatever the generic layer guessed, expand
    // its %VARS%, and see whether that is really a file here. If it is not,
    // fall back to looking for the bare executable name on PATH — which is
    // what makes a Linux or macOS machine work at all, since a Windows
    // "C:/Program Files/..." guess is never going to resolve on one.
    let find_app = |name: &str| -> Option<atlas::adapt::AppFound> {
        let spec = cfg.apps.apps.get(name)?;
        if spec.store {
            // A Store app id is not a path and cannot be checked by looking
            // for a file. Taken as given rather than reported missing.
            return Some(atlas::adapt::AppFound { launch: spec.launch.clone(), store: true });
        }
        let expanded = atlas::doctor::expand_env(&spec.launch);
        if std::path::Path::new(&expanded).is_file() {
            return Some(atlas::adapt::AppFound {
                launch: atlas::adapt::portable(&expanded),
                store: false,
            });
        }
        let bare = expanded
            .rsplit(['/', '\\'])
            .next()
            .map(|s| s.trim_end_matches(".exe").to_string())
            .unwrap_or_default();
        for candidate in [expanded.as_str(), bare.as_str(), name] {
            if candidate.is_empty() {
                continue;
            }
            if let Some(found) = atlas::tools::which(candidate) {
                return Some(atlas::adapt::AppFound {
                    launch: atlas::adapt::portable(&found),
                    store: false,
                });
            }
        }
        None
    };

    // The external tools, taken from the config rather than a list typed in
    // here — a hardcoded list is one more thing to forget to update.
    let mut tool_names: Vec<String> = vec!["ffmpeg".into(), "ffplay".into()];
    if let Some(t) = cfg.tools.as_ref() {
        for v in t.vars.values() {
            let looks_like_a_program = v.ends_with(".exe")
                || v.ends_with(".cmd")
                || v.ends_with(".bat")
                || (!v.contains('/') && !v.contains('\\') && !v.contains(' ') && !v.contains(':'));
            if looks_like_a_program {
                tool_names.push(v.clone());
            }
        }
    }
    tool_names.sort();
    tool_names.dedup();

    let find_tool = |t: &str| -> Option<String> {
        let expanded = atlas::doctor::expand_env(t);
        if std::path::Path::new(&expanded).is_file() {
            return Some(atlas::adapt::portable(&expanded));
        }
        atlas::tools::which(&expanded).map(|p| atlas::adapt::portable(&p))
    };

    // Microphones and speakers, asked of the machine rather than guessed.
    // An ffmpeg that isn't installed yields empty lists, and empty is the
    // honest answer — `first_run_message` says "no microphone, so we're
    // typing for now" rather than pretending.
    let inputs: Vec<String> = atlas::audio::probe("ffmpeg", true)
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.name)
        .collect();
    let outputs: Vec<String> = atlas::audio::probe("ffmpeg", false)
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.name)
        .collect();

    let m = atlas::adapt::detect(
        plat,
        &app_names,
        &find_app,
        inputs,
        outputs,
        &find_tool,
        &tool_names,
        atlas::store::now(),
    );

    println!("{}", atlas::adapt::first_run_message(&m, &app_names));
    match m.save(dir) {
        Ok(()) => println!("Written to config/machine.yaml. That file is yours — don't share it."),
        Err(e) => println!("Couldn't write config/machine.yaml: {e}"),
    }
}

/// `atlas carry` — what travels with the work, decided against the budget.
///
/// The packing rule is `workingset::pack`'s, unchanged: most valuable first,
/// shrink before dropping. What this adds is the part that was missing — real
/// paths, real byte counts, and a record on disk of what went, so that
/// `missing` and `back` have something true to answer from.
/// `atlas walkthrough` — Atlas finds the page, you make the change.
///
/// Wired 14 Sep 2026. This module sat built and tested and unreachable since
/// it landed, and it is the one in the `confirmed`/`consent`/`delegate`/
/// `afterme` family that needs no ruling from Eric, because **it is the half
/// that does not act** — until 24 Sep 2026, when Eric ruled that Atlas may make
/// the change itself after the read-back and his yes; `atlas_clicks` is now a
/// real setting, off until he turns it on. Before that it was `#[serde(skip)]` over a
/// function that returns false, so no config file can turn it into something
/// that touches his accounts; it opens a page and tells him where the switch
/// is. He proposed this shape himself, and it is also the answer to "one
/// instruction at a time, not multi-step reports" -- a walk is a queue of
/// single instructions that survives being put down halfway.
pub(super) fn run_walkthrough(cfg: &Config, args: &[String]) {
    use atlas::confirmed::{self, Asked, Change, Run, Step};
    use atlas::walkthrough::{self, Walk};

    let store = atlas::roots::store();
    let wcfg = cfg.tools.as_ref().map(|t| t.walkthrough.clone()).unwrap_or_default();
    // `confirmed.rs` was a complete, tested module nothing could reach. It
    // was written for Atlas making a security change itself, which nothing
    // here does -- but the read-back is the half that does the work, and it
    // is worth exactly as much when you are the one about to click. This is
    // the one place in the tree where a security change actually gets
    // started, and until 19 Sep 2026 it started with no read-back and no yes.
    let ccfg = cfg.tools.as_ref().map(|t| t.confirmed.clone()).unwrap_or_default();
    if !wcfg.enabled {
        println!("Walkthroughs are switched off (walkthrough.enabled: false in tools.yaml).");
        return;
    }

    let mut walk: Option<Walk> = store.load("walkthrough");
    let sites: Vec<String> = args.iter().skip(1).cloned().collect();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("prepare") | Some("turn-off") | Some("turn-on") if sites.is_empty() => {
            println!("Which accounts? e.g. `atlas walkthrough prepare google apple`.");
        }
        Some(kind @ ("prepare" | "turn-off" | "turn-on")) => {
            let w = match kind {
                "prepare" => walkthrough::to_prepare(&sites),
                "turn-on" => walkthrough::to_turn_on(&sites),
                _ => walkthrough::to_turn_off(&sites),
            };
            if w.stops.is_empty() {
                println!(
                    "I don't know where the setting lives on any of those yet, so I'd be \
                     sending you to a homepage to hunt -- which is the thing this is for \
                     avoiding. Nothing started."
                );
                return;
            }
            let skipped = sites.len() - w.stops.len();
            if kind == "turn-off" {
                println!("{}", walkthrough::before_turning_off(w.stops.len()));
                println!();
                println!("{}", walkthrough::FAMILY_ACCESS);
                println!();
            }
            if skipped > 0 {
                println!("{skipped} I don't have an exact page for -- left out rather than guessed.");
            }
            // Read back first, one at a time, when you've asked for that.
            // Six accounts picked in one command is still six yeses -- a
            // single yes covering six security changes is how the wrong one
            // gets made, and you would have no way to tell which.
            let change = match kind {
                "turn-off" => Change::TurnOffTwoFactor,
                "turn-on" => Change::TurnOnTwoFactor,
                _ => Change::GenerateRecoveryCodes,
            };
            let asking: Vec<Asked> = w
                .stops
                .iter()
                .map(|stop| Asked {
                    site: stop.site.clone(),
                    account: String::new(),
                    change: change.clone(),
                })
                .filter(|a| confirmed::needs_reading_back(a, &ccfg))
                .collect();
            if !asking.is_empty() {
                let run = Run::new(asking);
                println!("{}", confirmed::before_a_run(&run.items));
                println!();
                if let Some(a) = run.current() {
                    println!("{}", confirmed::saying_it_back(a));
                }
                println!("  `atlas walkthrough yes` or `atlas walkthrough no`.");
                keep(store.save("walkthrough_pending", &Some(w)), "store");
                keep(store.save("walkthrough_confirming", &Some(run)), "store");
                return;
            }
            println!("{}", w.say());
            if wcfg.open_pages {
                if let Some(stop) = w.current() {
                    println!("  page: {}", stop.url);
                }
            }
            println!("  `atlas walkthrough next` when it's done, `skip` to come back to it.");
            keep(store.save("walkthrough", &Some(w)), "store");
        }
        Some(said @ ("yes" | "no")) => {
            let mut run: Option<Run> = store.load("walkthrough_confirming");
            let Some(r) = run.as_mut() else {
                println!("Nothing waiting on a yes or no.");
                return;
            };
            let Some(asked) = r.current().cloned() else {
                println!("Nothing waiting on a yes or no.");
                return;
            };
            match confirmed::answer(said, &asked) {
                Step::Go { .. } => {
                    // Your yes, and Atlas presses it if you've let it
                    // (Settings: "Atlas makes the change"). Exactly one
                    // labelled control, never past a password box, nothing
                    // typed. Anything else comes back to you with the page.
                    let mut worked = true;
                    if wcfg.atlas_clicks {
                        let url = atlas::walkthrough::where_2fa_lives(&asked.site).map(|(u, _)| u).unwrap_or("");
                        let bcfg = cfg.tools.as_ref().map(|t| t.browser.clone()).unwrap_or_default();
                        let vars = cfg.tools.as_ref().map(|t| t.vars.clone()).unwrap_or_default();
                        let outcome = if url.is_empty() {
                            Ok(atlas::confirmed::Pressed::NoWordsForIt)
                        } else {
                            atlas::browser::Browser::start(&bcfg, &vars).and_then(|mut b| {
                                b.open(url)?;
                                let p = b.press_the_one_at(&asked.change, Some(url));
                                b.close();
                                p
                            })
                        };
                        match outcome {
                            Ok(p) => {
                                worked = p == atlas::confirmed::Pressed::Done;
                                println!("{}", p.say(&asked, url));
                            }
                            Err(e) => {
                                worked = false;
                                println!("I couldn't open my browser to do it ({e}). It's yours from here: {url}");
                            }
                        }
                    }
                    let rec = confirmed::record(&asked, worked, said, atlas::store::now());
                    let mut trail: Vec<atlas::confirmed::Record> = store.load("security_changes");
                    trail.push(rec);
                    keep(store.save("security_changes", &trail), "store");
                    r.record(worked);
                    println!("Right. {}", confirmed::how_to_undo(&asked.change));
                }
                Step::Dropped => {
                    r.skip();
                    println!("Left alone.");
                }
                // Anything ambiguous is a no for now, not a yes.
                Step::Unclear { say } | Step::Cannot(say) => {
                    println!("{say}");
                    return;
                }
                Step::ReadBack { say } => {
                    println!("{say}");
                    return;
                }
            }
            if !r.finished() {
                if let Some(next) = r.current() {
                    println!();
                    println!("{}", confirmed::saying_it_back(next));
                    println!("  `atlas walkthrough yes` or `atlas walkthrough no`.");
                }
                keep(store.save("walkthrough_confirming", &run), "store");
                return;
            }
            println!();
            println!("{}", r.summary());
            let confirmed_sites: Vec<String> = r
                .done
                .iter()
                .filter(|(_, ok)| *ok)
                .filter_map(|(what, _)| what.split(" — ").next().map(|s| s.to_string()))
                .collect();
            keep(store.save("walkthrough_confirming", &None::<Run>), "store");
            let pending: Option<Walk> = store.load("walkthrough_pending");
            keep(store.save("walkthrough_pending", &None::<Walk>), "store");
            let Some(mut w) = pending else { return };
            // Only the ones you actually said yes to. A walk that still
            // visited the pages you declined would make the question
            // decorative.
            w.stops.retain(|stop| confirmed_sites.iter().any(|s| s == &stop.site));
            if w.stops.is_empty() {
                println!("Nothing left to walk through.");
                return;
            }
            println!();
            println!("{}", w.say());
            if wcfg.open_pages {
                if let Some(stop) = w.current() {
                    println!("  page: {}", stop.url);
                }
            }
            println!("  `atlas walkthrough next` when it's done, `skip` to come back to it.");
            keep(store.save("walkthrough", &Some(w)), "store");
        }
        Some("next") | Some("skip") => {
            let Some(w) = walk.as_mut() else {
                println!("No walkthrough going. `atlas walkthrough prepare <site>...` starts one.");
                return;
            };
            let skipping = args.first().map(|s| s.to_lowercase()).as_deref() == Some("skip");
            let more = if skipping { w.skip() } else { w.next() };
            let (done, total) = w.progress();
            if more {
                println!("{}", w.say());
                if wcfg.open_pages {
                    if let Some(stop) = w.current() {
                        println!("  page: {}", stop.url);
                    }
                }
                println!("  {done} of {total} done.");
                keep(store.save("walkthrough", &walk), "store");
            } else {
                let left = w.remaining().len();
                if left == 0 {
                    println!("{} — all {total} done.", w.what_for);
                } else {
                    println!(
                        "{} — end of the list, {left} still open. `atlas walkthrough` shows \
                         which.",
                        w.what_for
                    );
                }
                keep(store.save("walkthrough", &walk), "store");
            }
        }
        Some("stop") => {
            if walk.is_some() {
                let none: Option<Walk> = None;
                keep(store.save("walkthrough", &none), "store");
                println!("Dropped it. Nothing was changed on any account.");
            } else {
                println!("Nothing going.");
            }
        }
        None | Some("where") => match &walk {
            None => {
                println!("No walkthrough going.");
                println!("  `atlas walkthrough prepare <site>...`  — set up a way back in first");
                println!("  `atlas walkthrough turn-off <site>...` — turn two-factor off");
            }
            Some(w) => {
                let (done, total) = w.progress();
                println!("{}", w.say());
                if wcfg.open_pages {
                    if let Some(stop) = w.current() {
                        println!("  page: {}", stop.url);
                    }
                }
                println!("  {done} of {total} done.");
                for s in w.remaining().iter().skip(1).take(4) {
                    println!("  still to do: {}", s.site);
                }
            }
        },
        Some(other) => {
            println!("I don't know `{other}`. Try prepare, turn-off, next, skip, stop.");
        }
    }
}

/// `atlas type` — the place to type when voice isn't working.
///
/// Wired 14 Sep 2026. Its own doc names the gap exactly: voice fails, and
/// "opening Notepad to talk to your assistant is absurd." The module was
/// built, tested and unreachable, which meant the fallback for a broken mic
/// was itself unreachable -- the failure mode this codebase keeps finding,
/// landing on the one feature whose entire job is to work when something
/// else has stopped.
///
/// A real hotkey surface belongs to the panel process, not a CLI. What this
/// gives is the same state machine driven from the keyboard Eric already has
/// in front of him, so the capability is reachable today rather than waiting
/// on the window work.
pub(super) fn run_quickinput(cfg: &Config, args: &[String]) {
    use atlas::quickinput::{Action, QuickInput, Surface};

    let qcfg = cfg.tools.as_ref().map(|t| t.quick_input.clone()).unwrap_or_default();
    if !qcfg.enabled {
        println!("The typing surface is switched off (quick_input.enabled: false in tools.yaml).");
        return;
    }
    // `surface` had no reader, so choosing the overlay silently got you the
    // console anyway. The borderless Win32 overlay isn't built (see the
    // module doc), so the honest thing is to say so rather than pretend the
    // setting did nothing — two values, two truthful outcomes.
    if qcfg.surface == Surface::Overlay {
        println!(
            "The overlay surface isn't built yet — set quick_input.surface: console in \
             tools.yaml to use the console one."
        );
        return;
    }

    let mut q = QuickInput::new(qcfg);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // A sentence on the command line goes straight through; that is the same
    // door as the panel's submit, without the typing.
    let typed: String = args.join(" ");
    if !typed.trim().is_empty() {
        q.hotkey(None, now);
        for c in typed.chars() {
            q.typed(c, now);
        }
        match q.submit() {
            Action::Submit(text) => println!("Sending: {text}"),
            Action::Hide => println!("Nothing to send."),
            other => println!("{}", other.plain()),
        }
        return;
    }

    match q.hotkey(None, now) {
        Action::Show | Action::ShowWithReason(_) => {
            println!("{}", q.placeholder());
            println!("  (type a line and press return, or Ctrl-C to close)");
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_ok() {
                for c in line.trim_end().chars() {
                    q.typed(c, now);
                }
                // An empty line means "go away", which is `escape`, not an
                // empty `submit`. The module draws that distinction itself
                // and it is worth keeping on this surface too.
                if line.trim().is_empty() {
                    q.escape();
                    println!("Nothing typed. Closed.");
                } else {
                    match q.submit() {
                        Action::Submit(text) => println!("Sending: {text}"),
                        _ => println!("Nothing typed. Closed."),
                    }
                }
                debug_assert!(!q.is_open(), "the box must not be left open behind a finished turn");
            }
        }
        other => println!("{}", other.plain()),
    }
}

/// `atlas startup on|off|status` — Atlas starting itself when you log in.
///
/// Eric's ruling, 17 Sep 2026: yes, in the background. So `on` with no mode
/// means `--daemon`.
///
/// **It prints the exact command and what it will do before running it**, and
/// that is not decoration. Registering a logon task means this machine will
/// start something that holds the microphone every time you sign in, and the
/// person agreeing to that should be able to see what is being registered
/// rather than trusting a sentence. `startup show` prints it and runs nothing.
pub(super) fn run_startup(args: &[String]) {
    use atlas::startup::{self, Mode};

    // `current_exe`, not argv[0] and not a relative path. A logon task starts
    // in `system32`, so anything relative resolves there -- see the module
    // note in `startup.rs` for why that used to be fatal and no longer is.
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            println!("I couldn't work out where my own program file is ({e}), so I can't");
            println!("register anything that would still find it at logon.");
            return;
        }
    };

    let rest = args.get(1..).map(|r| r.join(" ")).unwrap_or_default();
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("on") => {
            let mode = Mode::parse(&rest).unwrap_or(Mode::Background);
            let plan = startup::register(&exe, mode);
            println!("This will {}.\n", plan.what);
            println!("  {}\n", plan.as_typed());
            // On Windows the task is made from its XML definition (no time
            // limit, runs on battery): written first, printed so it can be
            // read before it's used.
            if cfg!(windows) {
                match startup::write_task_file(&exe, mode) {
                    Ok(path) => println!("  (the task's definition: {})\n", path.display()),
                    Err(e) => {
                        println!("{e}\nNothing was changed.");
                        return;
                    }
                }
            }
            match startup::run(&plan) {
                Ok(true) => {
                    let _ = startup::remember(&atlas::roots::state_dir(), true);
                    println!("Done. Atlas will start when you log in, and {}.", mode.plainly());
                    println!("Turn it off again with `atlas startup off`.");
                }
                Ok(false) => {
                    println!("That command ran and refused. Nothing was changed.");
                    println!("Run it yourself to see what it said -- I deliberately don't");
                    println!("swallow its output and guess.");
                }
                Err(e) => println!("{e}\nNothing was changed."),
            }
        }
        Some("off") => {
            let plan = startup::remove();
            println!("This will {}.\n", plan.what);
            println!("  {}\n", plan.as_typed());
            // The sign-in list entry the window falls back to when Task
            // Scheduler says no (`startup::turn_on`), gone too.
            if cfg!(windows) {
                let _ = startup::run(&startup::run_entry_remove());
            }
            let _ = startup::remember(&atlas::roots::state_dir(), false);
            match startup::run(&plan) {
                Ok(true) => println!("Done. Atlas will not start on its own any more."),
                // `/Delete` on a task that is not there reports failure, and
                // that is the same outcome the person asked for.
                Ok(false) => println!("It wasn't registered, so there was nothing to remove."),
                Err(e) => println!("{e}\nNothing was changed."),
            }
        }
        Some("status") | None => {
            let plan = startup::whether_registered();
            match startup::run(&plan) {
                Ok(true) => println!("Atlas starts when you log in."),
                Ok(false) => {
                    println!("Atlas does not start on its own.");
                    println!();
                    println!("  atlas startup on            in the background (wake word,");
                    println!("                              scheduled work, offers)");
                    println!("  atlas startup on listening  the wake word and nothing else");
                }
                Err(e) => println!("I couldn't ask: {e}"),
            }
        }
        Some("show") => {
            // Everything that would be run, and nothing run.
            for plan in [
                startup::register(&exe, Mode::Background),
                startup::register(&exe, Mode::Listening),
                startup::remove(),
                startup::whether_registered(),
            ] {
                println!("to {}:\n  {}\n", plan.what, plan.as_typed());
            }
            if !cfg!(windows) {
                println!("and this goes in {}:\n", startup::unit_path().display());
                println!("{}", startup::unit_file(&exe, Mode::Background));
            }
        }
        Some(other) => {
            println!("I don't know \"{other}\" -- try on, off, status or show.");
        }
    }
}

// acts on anything.
pub(super) fn run_backends(args: &[String]) {
    // `roots::store()`, not `Store::new("data/state")`. That literal is a
    // relative path, so the Store it builds is rooted wherever the process
    // happened to be standing -- the bug that made Atlas start with an empty
    // memory when launched from a shortcut. It was mine: this function was
    // dropped by the 17 Sep merge and I restored it by hand with the literal
    // in it. `tests/one_install_root.rs` named the line and the fix.
    let store = atlas::roots::store();
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            let router = atlas::backends::Router::load(&store);
            let learned = router.what_ive_learned();
            if learned.is_empty() {
                println!(
                    "Nothing learned yet — I record which apps I can read as I read them, and it \
                     takes a few reads of an app before there's anything worth saying."
                );
                return;
            }
            println!("What I've learned about reading your apps:");
            for line in learned {
                println!("  {line}");
            }
        }
        Some("forget") => {
            let Some(app) = args.get(1) else {
                println!("Which app should I forget what I learned about? atlas backends forget <app>");
                return;
            };
            let mut router = atlas::backends::Router::load(&store);
            router.forget_app(app);
            match router.save(&store) {
                Ok(()) => println!("Forgotten what I'd learned about {app} — I'll learn it fresh."),
                Err(e) => println!("Couldn't save that: {e}"),
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try status or forget <app>."),
    }
}

/// Which Atlas this is, and which of your own devices belong to it. See
/// `household.rs`'s own doc: two installs know nothing about each other
/// unless the same person paired them, device to device, with a code.

/// `atlas wireguard` — your own server, reached over WireGuard, fenced so
/// your devices reach its model server and nothing else.
///
///   atlas wireguard            the layout, the fence, and what stays yours
///   atlas wireguard configs    make the keys and the three configs
///   atlas wireguard check      which devices have connected, and when
///   atlas wireguard tidy       delete the configs holding private keys
pub(super) fn run_wireguard(cfg: &Config, args: &[String]) {
    use atlas::wireguard::Device;
    let tools = cfg.tools.clone().unwrap_or_default();
    let wcfg = tools.mesh.wireguard.clone();
    let plan = match atlas::wireguard::Plan::from_config(&wcfg) {
        Ok(p) => p,
        Err(e) => {
            println!("I can't lay out the tunnel: mesh.wireguard.subnet — {e}.");
            return;
        }
    };
    let dir = atlas::roots::data_sub("wireguard");
    let tool = atlas::wireguard::wg_tool(&wcfg);
    let vars = tools.vars.clone();
    match args.first().map(String::as_str) {
        None | Some("plan") => {
            println!("Your own server, over WireGuard:");
            for d in [Device::Server, Device::Laptop, Device::Phone] {
                println!("  the {} is {}", d.label(), plan.address(d));
            }
            println!();
            println!("What the server lets through from the tunnel:");
            for r in atlas::wireguard::fence(&plan) {
                println!("  - {}", r.what);
            }
            println!();
            println!("{}", atlas::wireguard::ONE_TUNNEL_ON_A_PHONE);
            println!();
            if plan.endpoint.is_empty() {
                println!("I don't have the server's outside address yet (mesh.wireguard.endpoint).");
            } else {
                println!("Your devices will find the server at {}.", plan.endpoint);
            }
            println!();
            println!("Yours to do:");
            for (i, step) in atlas::wireguard::YOURS.iter().enumerate() {
                println!("  {}. {step}", i + 1);
            }
            println!();
            println!("Mine: `atlas wireguard configs` makes the keys, the three configs and the");
            println!("server's fence; `atlas wireguard check` says who has connected.");
        }
        Some("configs") => {
            if plan.endpoint.is_empty() {
                println!("I need the server's outside address first — the name your home address");
                println!("answers to and the port — in mesh.wireguard.endpoint.");
                return;
            }
            let make = || atlas::wireguard::make_keys(&tool, &vars);
            let (server, laptop, phone) = match (make(), make(), make()) {
                (Ok(a), Ok(b), Ok(c)) => (a, b, c),
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                    println!("I couldn't make the keys: {e}");
                    println!("WireGuard's own tool makes them. Install WireGuard, then try again.");
                    return;
                }
            };
            let laptop_conf = atlas::wireguard::device_conf(&plan, Device::Laptop, &laptop, &server.public);
            let phone_conf = atlas::wireguard::device_conf(&plan, Device::Phone, &phone, &server.public);
            let (Ok(laptop_conf), Ok(phone_conf)) = (laptop_conf, phone_conf) else {
                println!("I couldn't write the device configs.");
                return;
            };
            let public = serde_json::json!({
                "server": server.public, "laptop": laptop.public, "phone": phone.public,
            });
            let files = [
                ("server.conf", atlas::wireguard::server_conf(&plan, &server, &laptop.public, &phone.public)),
                ("laptop.conf", laptop_conf),
                ("phone.conf", phone_conf),
                ("fence-linux.nft", atlas::wireguard::nftables(&plan, "wg0")),
                ("fence-windows.txt", atlas::wireguard::windows_rules(&plan).join("\n") + "\n"),
                ("public-keys.json", serde_json::to_string_pretty(&public).unwrap_or_default()),
            ];
            if let Err(e) = std::fs::create_dir_all(&dir) {
                println!("I couldn't make {}: {e}", dir.display());
                return;
            }
            for (name, body) in &files {
                if let Err(e) = std::fs::write(dir.join(name), body) {
                    println!("I couldn't write {name}: {e}");
                    return;
                }
            }
            println!("Made in {}:", dir.display());
            println!("  server.conf, laptop.conf, phone.conf — one for each WireGuard");
            println!("  the server's fence, for Linux and for Windows — your devices reach its model");
            println!("  server on port {} and nothing else", plan.model_port);
            println!();
            println!("The three .conf files hold private keys. Import each one, then run");
            println!("`atlas wireguard tidy` and I'll delete them. The phone's goes over by AirDrop or");
            println!("Files into the WireGuard app — not by email or a chat.");
        }
        Some("check") => {
            let names: std::collections::HashMap<String, String> =
                std::fs::read_to_string(dir.join("public-keys.json"))
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .and_then(|v| v.as_object().cloned())
                    .map(|m| {
                        m.into_iter()
                            .filter_map(|(k, v)| v.as_str().map(|p| (p.to_string(), k)))
                            .collect()
                    })
                    .unwrap_or_default();
            if names.is_empty() {
                println!("I haven't made the configs here, so I don't know whose key is whose.");
                return;
            }
            let show = atlas::tools::ExternalTool {
                args: vec!["show".into(), "all".into(), "latest-handshakes".into()],
                ..tool
            };
            let out = match show.run(&vars, None) {
                Ok(o) => o,
                Err(e) => {
                    println!("I couldn't ask WireGuard: {e}");
                    println!("On Windows that needs Atlas running as administrator.");
                    return;
                }
            };
            let seen = atlas::wireguard::handshakes(&out);
            let now = atlas::store::now();
            for name in ["laptop", "phone"] {
                let key = names.iter().find(|(_, n)| n.as_str() == name).map(|(k, _)| k.clone());
                let last = key.and_then(|k| seen.iter().find(|(p, _)| *p == k).map(|(_, t)| *t));
                println!("{}", atlas::wireguard::connection(name, last, now));
            }
        }
        Some("tidy") => {
            let mut gone = 0;
            for name in ["server.conf", "laptop.conf", "phone.conf"] {
                if std::fs::remove_file(dir.join(name)).is_ok() {
                    gone += 1;
                }
            }
            println!(
                "Deleted {gone} config file{} holding private keys. The public keys stay, so \
                 `atlas wireguard check` still knows who's who.",
                if gone == 1 { "" } else { "s" }
            );
        }
        Some(other) => {
            println!("I don't know `atlas wireguard {other}`. Try: atlas wireguard [configs|check|tidy]");
        }
    }
}

/// The window a double-click opens: set up the first time, "Atlas is
/// running" after that. Moves Atlas into its standard home first when it was
/// opened from somewhere that isn't an install.
pub(super) fn run_home(double_clicked: bool, first: atlas::firstlaunch::First) {
    // Problems here are shown in a message box when Atlas was double-clicked:
    // it has no console, and until 28 Sep 2026 they went to one that wasn't
    // there (running Atlas Setup.exe again over a running Atlas did nothing,
    // silently).
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            atlas::firstlaunch::show_problem(&format!("I couldn't find myself on disk: {e}"));
            return;
        }
    };
    let exe_dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let told = std::env::var_os("ATLAS_HOME").map(|v| !v.is_empty()).unwrap_or(false);
    let home = atlas::firstlaunch::standard_home();
    match atlas::firstlaunch::where_to_live(
        &exe_dir,
        atlas::roots::looks_like_an_install(&exe_dir),
        told,
        home.as_deref(),
    ) {
        atlas::firstlaunch::Where::MoveTo(home) => {
            let words = first.words();
            let args: Vec<&str> = words.iter().map(|w| w.as_str()).collect();
            let installed = home.join(atlas::firstlaunch::INSTALLED_NAME);
            let open_installed = || match atlas::firstlaunch::spawn_quietly(&installed, &args) {
                Ok(child) => atlas::unwaited::dont_wait(child),
                Err(e) => atlas::firstlaunch::show_problem(&format!("I couldn't open the Atlas in {}: {e}", home.display())),
            };
            match atlas::firstlaunch::replacing_in(&exe, &home) {
                // Already there, byte for byte: just open it.
                atlas::firstlaunch::Replacing::Same => {
                    open_installed();
                    return;
                }
                // Never a silent downgrade.
                atlas::firstlaunch::Replacing::Older { installed: newer, mine } => {
                    let q = format!(
                        "The Atlas already on this computer is version {newer}, which is newer than this one ({mine}). \
                         Replace it with this older version? Your notes and settings are kept either way."
                    );
                    if !atlas::firstlaunch::ask_yes_no(&q) {
                        open_installed();
                        return;
                    }
                }
                atlas::firstlaunch::Replacing::Install => {}
            }
            match atlas::firstlaunch::move_in_over(&exe, &home, true, std::time::Duration::from_secs(20)) {
                // Carry on from the new home, in a fresh process, so every
                // path below is decided from there.
                Ok(moved) => match atlas::firstlaunch::spawn_quietly(&moved, &args) {
                    Ok(child) => atlas::unwaited::dont_wait(child),
                    Err(e) => atlas::firstlaunch::show_problem(&format!("I moved into {} but couldn't open from there: {e}", home.display())),
                },
                Err(e) => atlas::firstlaunch::show_problem(&format!(
                    "{e}\n\nIf Atlas is still running, close it (or restart the computer) and open this again."
                )),
            }
            return;
        }
        atlas::firstlaunch::Where::Here => {}
    }
    let dir = atlas::roots::config_dir();
    if atlas::firstlaunch::settings_missing(&dir) {
        let _ = atlas::firstlaunch::write_default_config(&dir);
    }
    let configured = Config::load(&dir).ok().and_then(|c| c.tools.map(|t| t.server.port)).unwrap_or(8787);
    // Opening Atlas once it's set up makes sure the background Atlas is
    // running (no console: it's started with no window of its own), then
    // opens the hub. Closing the window never stops it (Eric, 28 Sep 2026).
    // "Running" is Atlas's own lock and its hub's own answer, not whatever
    // holds the port (`firstlaunch::atlas_running`).
    let opening = atlas::firstlaunch::what_opening_does(
        atlas::firstlaunch::is_set_up(&atlas::roots::install_root()),
        atlas::firstlaunch::atlas_running(&atlas::roots::install_root()),
        first,
    );
    // Where the running Atlas's hub really is (it may have opened beside a
    // taken port: `server::open_hub`); the setting when it isn't answering.
    let port = atlas::firstlaunch::hub_port_at(&atlas::roots::install_root(), configured);
    if opening.start_background {
        // Watched for a few seconds: one that stops at once says why instead
        // of leaving a hub nothing answers (29 Sep 2026).
        if let Err(why) = atlas::firstlaunch::start_background_watched(
            &exe,
            &atlas::roots::install_root(),
            std::time::Duration::from_secs(4),
        ) {
            atlas::firstlaunch::show_problem(&why);
        }
    }
    let first = opening.first;
    #[cfg(feature = "desktop-ui")]
    {
        let place = match atlas::setupwin::here(&atlas::roots::install_root(), &exe, port) {
            Ok(p) => p,
            Err(e) => {
                atlas::firstlaunch::show_problem(&format!("I couldn't get ready: {e}"));
                return;
            }
        };
        if double_clicked {
            atlas::firstlaunch::let_go_of_the_console();
        }
        if let Err(e) = atlas::setupwin::run(place, first) {
            atlas::firstlaunch::show_problem(&format!("I couldn't open my window: {e}"));
        }
    }
    // No desktop UI in this build: there is no setup window to open. The config
    // is written above; the hub (and, on a phone, the shell over it) is the
    // face. Say where things stand rather than opening nothing.
    #[cfg(not(feature = "desktop-ui"))]
    {
        let _ = (double_clicked, port, &exe, &first);
        println!("Atlas is set up. This build has no desktop window — open the hub in a browser.");
    }
}

/// `atlas kokoro-check [words]` — load Kokoro (`kokoro`), say the words (or
/// a sentence of its own) into `data/tmp/kokoro-check.wav`, and print how
/// long loading and speaking took against how long the speech lasts. Exit
/// status 0 only when it spoke. What the Windows build runs to prove the
/// library opens on real Windows (`.github/workflows/windows.yml`).
pub(super) fn run_kokoro_check(words: &[String]) -> i32 {
    let root = atlas::roots::install_root();
    let ready = match atlas::kokoro::check(&root) {
        Ok(r) => r,
        Err(m) => {
            println!("Can't: {}. `atlas get kokoro` fetches it.", m.plain());
            return 1;
        }
    };
    let text = if words.is_empty() {
        "Your nine o'clock post is over length by twelve characters.".to_string()
    } else {
        words.join(" ")
    };
    let threads = atlas::kokoro::thread_count();
    let t = std::time::Instant::now();
    let k = match atlas::kokoro::Kokoro::load(&ready.runtime, &ready.model, threads) {
        Ok(k) => k,
        Err(why) => {
            println!("Can't: {why}.");
            return 1;
        }
    };
    let load_ms = t.elapsed().as_millis();
    let (voice, sid) = atlas::kokoro::voice_or_default(atlas::kokoro::DEFAULT_VOICE);
    let t = std::time::Instant::now();
    let samples = match k.synth(&text, sid, 1.0) {
        Ok(s) => s,
        Err(why) => {
            println!("Can't: {why}.");
            return 1;
        }
    };
    let synth_ms = t.elapsed().as_millis() as f64;
    let secs = samples.len() as f64 / k.sample_rate() as f64;
    if secs < 0.3 {
        println!("Kokoro loaded but said nothing ({secs:.2} s of audio).");
        return 1;
    }
    let out = atlas::roots::data_dir().join("tmp").join("kokoro-check.wav");
    let _ = std::fs::create_dir_all(out.parent().unwrap_or(&root));
    let _ = std::fs::write(&out, atlas::kokoro::to_wav(&samples, k.sample_rate()));
    println!(
        "Kokoro ({voice}, {threads} threads): loaded in {load_ms} ms; {secs:.2} s of speech made in {synth_ms:.0} ms \
         ({:.2}x its own length). Saved to {}.",
        synth_ms / 1000.0 / secs,
        out.display()
    );
    0
}

/// `atlas get` — fetch the voice pieces here, in the terminal. The same code
/// the setup window uses; what the launcher's first-run setup calls.
pub(super) fn run_get(which: Option<&str>) {
    let root = atlas::roots::install_root();
    let tools = atlas::getpieces::Tools::default();
    let mut problems = 0;
    let Some((what, pieces)) = atlas::getpieces::set(which) else {
        println!("I don't know that set. `atlas get` (the voice), `atlas get seeing`, `atlas get pictures`, `atlas get photos`, `atlas get hearing`, `atlas get voiceid`, or `atlas get kokoro`.");
        return;
    };
    println!("Getting {what}. Safe to run again —");
    println!("it picks up where it stopped and skips what's already here.");
    println!();
    if let Err(why) = atlas::getpieces::room_for(&pieces, &root, atlas::getpieces::free_bytes(&root)) {
        println!("{why}");
        return;
    }
    for piece in pieces {
        if atlas::getpieces::have(&piece, &root) {
            println!("  [have] {}", piece.name);
            continue;
        }
        println!("  [get ] {} ({} MB)...", piece.name, piece.megabytes());
        let last = std::cell::Cell::new(0u64);
        let report = |done: u64, total: u64| {
            let pct = if total == 0 { 0 } else { done * 100 / total };
            if pct >= last.get() + 10 {
                last.set(pct - pct % 10);
                println!("         {pct}%");
            }
        };
        match atlas::getpieces::fetch(&piece, &root, &tools, &report) {
            Ok(()) => println!("  [ ok ] {}", piece.name),
            Err(e) => {
                problems += 1;
                println!("  [FAIL] {e}");
            }
        }
    }
    println!();
    if problems == 0 {
        println!("Everything's here.");
    } else {
        println!("{problems} piece(s) didn't arrive. Run this again to pick up where it stopped.");
    }
}

/// `atlas plugins` -- the add-ons on this install, and your decisions about them.
///
/// Approving happens here or on the hub's Add-ons page, and nowhere else:
/// there is deliberately no spoken command and no message that approves an
/// add-on, so nothing that can talk to Atlas can give one permissions.
pub(super) fn run_plugins(cfg: &Config, args: &[String]) {
    use atlas::plugins;
    let dir = plugins::plugins_dir();
    let store = atlas::roots::store();
    let arg = |i: usize| args.get(i).map(|s| s.as_str()).unwrap_or("");
    let said = match arg(0) {
        "" | "list" => {
            let all = plugins::scan(&dir, &cfg.commands, &plugins::Approvals::load(&store));
            if all.is_empty() {
                println!("No add-ons. `atlas plugins add <file>` adds one; it does nothing until you approve it.");
                return;
            }
            for p in &all {
                describe_plugin(p);
            }
            return;
        }
        "approve" => {
            let id = arg(1);
            let all = plugins::scan(&dir, &cfg.commands, &plugins::Approvals::load(&store));
            let Some(p) = all.iter().find(|p| p.id == id) else {
                eprintln!("There's no add-on called \"{id}\". `atlas plugins` lists them.");
                leave(1);
            };
            describe_plugin(p);
            if p.manifest.is_none() {
                eprintln!("It can't be approved as it is.");
                leave(1);
            }
            println!("\nApprove {} to do the above? Type yes to approve.", p.name());
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            if line.trim().to_lowercase() != "yes" {
                println!("Not approved. It stays off.");
                return;
            }
            plugins::approve(&store, &dir, &cfg.commands, id, &p.sha256)
        }
        "revoke" => plugins::revoke(&store, arg(1), arg(2)),
        "trust" => plugins::trust_step(&store, &dir, &cfg.commands, arg(1), &args.get(2..).map(|a| a.join(" ")).unwrap_or_default()),
        "untrust" => plugins::untrust_step(&store, arg(1), &args.get(2..).map(|a| a.join(" ")).unwrap_or_default()),
        "remove" => {
            let tc = cfg.tools.as_ref();
            let trash = atlas::safety::Trash::new(
                tc.map(|t| t.trash.clone()).unwrap_or_default().resolved(&atlas::roots::install_root()),
            );
            plugins::remove(&store, &dir, arg(1), &trash)
        }
        "send" | "share" => send_plugin(&dir, arg(1), &args.get(2..).map(|a| a.join(" ")).unwrap_or_default()),
        "offers" => {
            let offers = plugins::Offers::load(&store).items;
            if offers.is_empty() {
                println!("Nobody has shared an add-on with you.");
            }
            for o in offers {
                println!(
                    "\n#{} {} -- {} (says it's by {})",
                    o.offer,
                    o.name,
                    match &o.in_group { Some(g) => format!("shared by {} in {g}", o.from), None => format!("sent by {}", o.from) },
                    o.author
                );
                for k in &o.permissions {
                    println!("  would be allowed to {}", plugins::permission(k).map(|p| p.plain).unwrap_or(k));
                }
            }
            return;
        }
        "take" => {
            let n: u64 = arg(1).trim_start_matches('#').parse().unwrap_or(0);
            let Some(o) = plugins::Offers::load(&store).items.into_iter().find(|o| o.offer == n) else {
                eprintln!("There's no offer #{n}. `atlas plugins offers` lists them.");
                leave(1);
            };
            println!("{} from {} would be allowed to:", o.name, o.from);
            for k in &o.permissions {
                println!("  {}", plugins::permission(k).map(|p| p.plain).unwrap_or(k));
            }
            println!("Add it and allow that? Type yes.");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            if line.trim().to_lowercase() != "yes" {
                println!("Left it where it is.");
                return;
            }
            plugins::take_offer(&store, &dir, &cfg.commands, n, &o.sha256)
        }
        "decline" => plugins::decline_offer(&store, arg(1).trim_start_matches('#').parse().unwrap_or(0)),
        "off" => plugins::set_off(&store, arg(1), true),
        "on" => plugins::set_off(&store, arg(1), false),
        "add" => plugins::add_from(std::path::Path::new(arg(1)), &dir, &cfg.commands),
        other => Err(format!(
            "I don't know `atlas plugins {other}`. Try: list, add <file>, approve <id>, revoke <id> <permission>, \
             trust <id> <step>, untrust <id> <step>, off <id>, on <id>, remove <id>, share <id> <person or group>, \
             offers, take <offer>, decline <offer>."
        )),
    };
    match said {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("{e}");
            leave(1);
        }
    }
}

/// Hand an add-on to a paired Atlas. It arrives there switched off, marked as
/// coming from you.
/// Hand an add-on to a paired person, or to everyone in a group you can
/// reach. It arrives on their shelf of things shared with them -- never
/// installed by being sent.
fn send_plugin(dir: &std::path::Path, id: &str, to: &str) -> Result<String, String> {
    let (file_name, bytes, name) = atlas::plugins::to_share(dir, id)?;
    let pairings = atlas::kin::Pairings::load(&atlas::kin::where_pairings_live());
    let chats = atlas::chat::Chats::load(&atlas::roots::store());
    let key = to.trim().strip_prefix("the ").unwrap_or(to.trim());
    let key = key.strip_suffix(" group").unwrap_or(key).trim();
    let (people, covering) = match chats.group_named(key) {
        Some(g) => (g.members.clone(), format!("{}{}", atlas::plugins::SHARED_IN, g.name)),
        None => (vec![key.to_string()], format!("the add-on {name}")),
    };
    let named = std::env::temp_dir().join(&file_name);
    std::fs::write(&named, &bytes).map_err(|e| format!("couldn't prepare it: {e}"))?;
    let mut reached = Vec::new();
    for who in &people {
        let Some(contact) = pairings.contacts.iter().find(|c| c.name.eq_ignore_ascii_case(who)).cloned() else { continue };
        let h = atlas::household::share_with_friend(&covering, "me");
        if atlas::kin::PeerLink::from_state(&pairings, &atlas::chat::Chats::default()).sealing_as(atlas::peerkey::Identity::load_or_create(&atlas::kin::where_pairings_live()).ok()).hand_note(&contact.name, &h, Some(&named)).is_ok() {
            reached.push(who.clone());
        }
    }
    let _ = std::fs::remove_file(&named);
    if reached.is_empty() {
        return Err(format!("couldn't reach anyone in {to} to share \"{name}\" with"));
    }
    Ok(format!(
        "Shared \"{name}\" with {}. It waits on their Add-ons page until they choose to add it.",
        reached.join(", ")
    ))
}

fn describe_plugin(p: &atlas::plugins::Plugin) {
    println!("\n{} ({}) -- {}", p.name(), p.id, p.status.plain());
    if let Some(m) = &p.manifest {
        println!("  by {}{}", m.author, if m.description.is_empty() { String::new() } else { format!(": {}", m.description) });
        if m.permissions.is_empty() {
            println!("  asks to do nothing beyond talking back");
        }
        for k in &m.permissions {
            let plain = atlas::plugins::permission(k).map(|x| x.plain).unwrap_or("");
            let mark = if p.granted.contains(k) { "allowed" } else { "asks" };
            println!("  [{mark}] {k}: {plain}");
        }
        for f in &p.flows {
            let starts = if f.triggers.is_empty() { "nothing starts it".to_string() } else { format!("say \"{}\"", f.triggers.join("\" or \"")) };
            println!("  \"{}\" -- {starts}", f.name);
            for s in &f.steps {
                println!("      {}", s.command);
            }
        }
    }
    if let Some(who) = &p.sent_by {
        println!("  sent to you by {who} (your paired Atlas)");
    }
    for q in &p.questions {
        match (&q.always_asks, q.trusted) {
            (Some(why), _) => println!("  asks before \"{}\" every time: {why}", q.command),
            (None, true) => println!("  won't ask before \"{}\" (you said) -- `atlas plugins untrust {} {}`", q.command, p.id, q.command),
            (None, false) => println!("  asks before \"{}\" -- `atlas plugins trust {} {}` to stop that", q.command, p.id, q.command),
        }
    }
    for t in &p.trouble {
        println!("  note: {t}");
    }
    println!("  file fingerprint {}", &p.sha256.get(..16).unwrap_or(""));
}

/// `atlas edits` -- your own edits to the shipped config files, which Atlas
/// keeps through every update, and giving one up.
pub(super) fn run_edits(args: &[String]) {
    use atlas::yourchanges::{all_kept, forget, shown};
    let dir = atlas::roots::config_dir();
    match args.first().map(|s| s.as_str()).unwrap_or("list") {
        "list" => {
            let (kept, problems) = all_kept(&dir);
            for p in &problems {
                println!("! {p}");
            }
            if kept.is_empty() {
                println!("No edits of yours to the shipped config files. Hand edits are moved here the next time Atlas starts.");
                return;
            }
            for e in &kept {
                let yours = if e.change.removed { "(you removed it)".to_string() } else { shown(e.change.yours.as_ref()) };
                let mut line = format!("{} {}: {}", e.file, e.change.dotted(), yours);
                if e.change.unsure {
                    line.push_str("  [unsure it was you]");
                }
                if e.default_moved() {
                    line.push_str(&format!(
                        "  [the default changed: was {}, now {}]",
                        shown(e.change.was.as_ref()),
                        shown(e.shipped_now.as_ref())
                    ));
                }
                println!("{line}");
            }
            println!("\n`atlas edits revert <file> <setting>` goes back to the shipped default.");
        }
        "revert" => {
            let (file, path) = (args.get(1).cloned().unwrap_or_default(), args.get(2).cloned().unwrap_or_default());
            match forget(&dir, &file, &path) {
                Ok(true) => println!("{file} {path} is back to what Atlas ships, from the next start."),
                Ok(false) => println!("There's no edit of yours at {file} {path}. `atlas edits` lists them."),
                Err(e) => {
                    eprintln!("{e}");
                    leave(1);
                }
            }
        }
        other => {
            eprintln!("I don't know `atlas edits {other}`. Try: list, revert <file> <setting>.");
            leave(1);
        }
    }
}

/// `atlas call check [seconds]`: record your microphone and what the laptop
/// plays for a few seconds, and say how loud each was — so you can tell,
/// before a real call, that both sides of call notes can hear. The files are
/// deleted afterwards.
pub(super) fn run_call_check(args: &[String]) {
    if args.first().map(|s| s.as_str()) == Some("now") {
        match atlas::callwatch::call_now() {
            Some(app) => println!("You're on {app}."),
            None => println!("No call app is holding the microphone."),
        }
        return;
    }
    if args.first().map(|s| s.as_str()) != Some("check") {
        println!("`atlas call check` records a few seconds of both sides to check they work.");
        return;
    }
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5).clamp(1, 60);
    let scratch = atlas::roots::RunScratch::new("atlas-call-check");
    let dir = scratch.path().to_path_buf();
    let mine = dir.join("you.wav");
    let theirs = dir.join("them.wav");
    println!("Recording {secs} seconds of your microphone and of what this laptop plays...");
    let a = atlas::callrec::start(atlas::callrec::Side::Yours, &mine, false);
    let b = atlas::callrec::start(atlas::callrec::Side::Theirs, &theirs, false);
    std::thread::sleep(std::time::Duration::from_secs(secs));
    for (what, r, path) in [("Your microphone", a, &mine), ("What the laptop plays", b, &theirs)] {
        match r {
            Err(e) => println!("  {what}: couldn't record — {e}"),
            Ok(rec) => match rec.finish() {
                Err(e) => println!("  {what}: {e}"),
                Ok(got) => {
                    // Loudest moment, against a floor above hiss: about 3%.
                    let peak = std::fs::read(path)
                        .ok()
                        .map(|b| b.get(44..).unwrap_or(&[]).chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs()).max().unwrap_or(0))
                        .unwrap_or(0);
                    let peak = if peak > 1000 { 1 } else { 0 };
                    println!(
                        "  {what}: {got:.1} seconds recorded, {}",
                        if peak > 0 { "and there was sound in it." } else { "but it was silent." }
                    );
                }
            },
        }
    }
    drop(scratch);
}
