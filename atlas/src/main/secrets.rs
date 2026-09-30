//! The vault and who is who: first run, the vault, handing over, the recovery
//! key, accounts, profiles, codes, access and after-me.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

/// What Atlas can work out about this machine by itself, for `firstrun`.
/// Real checks only: an app "found" means its executable actually exists on
/// disk, not that config merely names it -- config presence is exactly the
/// trap this codebase keeps naming (`voice::ToolsConfig` et al).
fn firstrun_found(cfg: &Config, plat: &dyn Platform) -> atlas::firstrun::Found {
    let mut apps = Vec::new();
    let mut missing_apps = Vec::new();
    for (name, spec) in &cfg.apps.apps {
        if spec.store || std::path::Path::new(&spec.launch).exists() {
            apps.push(name.clone());
        } else {
            missing_apps.push(name.clone());
        }
    }
    let monitors = plat.monitors().map(|m| m.len()).unwrap_or(1);
    let models_dir = std::path::Path::new(&cfg.tools.as_ref().map(|t| t.models.dir.clone()).unwrap_or_default()).to_path_buf();
    let missing_tools: Vec<String> = atlas::infer::whats_missing(&models_dir, &atlas::infer::Kind::all())
        .into_iter()
        .map(|(k, _)| k.file().to_string())
        .collect();
    atlas::firstrun::Found {
        apps,
        missing_apps,
        monitors,
        // Asked of the machine. This was `Vec::new()` with a comment saying
        // real enumeration needed an OS command Atlas didn't run anywhere --
        // which was half true: it ran one, hardcoded to Windows dshow, so
        // the setup wizard told every Linux and macOS user they had no
        // microphone. Empty is still the honest answer when ffmpeg is
        // missing or nothing is plugged in, and firstrun's Microphones step
        // already degrades correctly on it.
        microphones: atlas::audio::probe("ffmpeg", true)
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.name)
            .collect(),
        missing_tools,
    }
}

/// Walks the setup wizard for real, at the terminal: says what `next()`
/// says, does what `Look` asks (using real data, not a script), plays real
/// audio for `Audition`, and reads real answers back for `Ask`. Every
/// public piece of `firstrun.rs` gets exercised here -- `record`, `resume`,
/// `is_skip`, `which_monitor`, `Found`'s own report methods -- not just
/// `next()` in isolation.
pub(super) fn run_firstrun(cfg: &Config, plat: &dyn Platform) {
    let store = atlas::roots::store();
    let mut fr = atlas::firstrun::FirstRun::load(&store);

    if fr.finished && !fr.deferred.is_empty() {
        println!(
            "Already set up, with {} thing{} you put off. Picking one back up.",
            fr.deferred.len(),
            if fr.deferred.len() == 1 { "" } else { "s" }
        );
        if let Some(step) = fr.resume() {
            let _ = step; // fr.next() below recomputes what to ask from ORDER
        }
    } else if fr.finished {
        println!("Already set up. There's nothing left deferred.");
        return;
    }

    loop {
        let found = firstrun_found(cfg, plat);
        use atlas::firstrun::Move;
        match fr.next(&found) {
            Move::Say(s) => println!("{s}"),
            Move::Finished(s) => {
                println!("{s}");
                keep(fr.save(&store), "first-run progress");
                break;
            }
            Move::Look { step, what } => {
                println!("({what})");
                match step {
                    atlas::firstrun::Step::FindApps => {
                        fr.record(step, &found.report_apps(), false);
                    }
                    atlas::firstrun::Step::Missing => {
                        fr.record(step, &found.report_missing(), false);
                    }
                    atlas::firstrun::Step::Microphones => {
                        // Honest, not a guess -- see firstrun_found's own
                        // comment on why real enumeration isn't wired yet.
                        fr.record(step, "microphone detection isn't wired yet", true);
                    }
                    _ => {}
                }
            }
            Move::Audition => {
                run_audition(cfg);
                print!("Which voice did you like? ");
                io::stdout().flush().ok();
                let mut line = String::new();
                io::stdin().read_line(&mut line).ok();
                let line = line.trim().to_string();
                let skipped = atlas::firstrun::is_skip(&line);
                fr.record(atlas::firstrun::Step::Voice, &line, skipped);
            }
            Move::Ask { step, question, examples } => {
                println!("{question} ({})", examples.join(", "));
                let mut line = String::new();
                io::stdin().read_line(&mut line).ok();
                let line = line.trim().to_string();
                if atlas::firstrun::is_skip(&line) {
                    fr.record(step, "skipped", true);
                } else if step == atlas::firstrun::Step::Monitors {
                    let which = atlas::firstrun::which_monitor(&line).unwrap_or("current");
                    fr.record(step, which, false);
                } else {
                    fr.record(step, &line, false);
                }
            }
        }
        keep(fr.save(&store), "first-run progress");
    }
}

/// Who's using this install right now. Not a setting -- a separate state
/// directory per profile, so a guest's session can never quietly read the
/// owner's memory, drafts, or approval history. See `profiles.rs`'s own
/// doc for why this exists even though "your friends should run their own
/// copy" is the better answer when that's possible.
/// The passphrase, which is the only thing here that proves who is typing.
///
/// Without this command there was no way to set one outside the spoken
/// `unlock` intent -- so on a fresh install `atlas handover back` could never
/// succeed, because a vault with no passphrase has its first unlock *set*
/// one and that is not proof of anything. A way in that cannot be reached is
/// not a way in.
pub(super) fn run_vault(args: &[String]) {
    let state = atlas::roots::install_state();
    // Handed over, and no passphrase has ever been set.
    //
    // The hole this closes: `handover::take_back` refuses a vault with no
    // passphrase, because a first unlock *chooses* one rather than checking
    // it. But nothing stopped whoever is holding the laptop from setting that
    // first passphrase right here and then taking the handover back with it,
    // which turns the refusal into a two-step instruction.
    //
    // `run_vault` is not behind `gate_with_identity` and should not be --
    // most of what it does is refused by the vault itself, which asks for a
    // passphrase nobody else has. This one branch is the exception, because
    // it is the only one that hands out authority instead of checking it.
    //
    // Changing an existing passphrase is not blocked: that already requires
    // the current one.
    {
        let handed = atlas::handover::Handover::load(&state);
        let has = atlas::vault::Vault::load(&state).has_a_passphrase();
        let asked = args.first().map(|a| a.to_lowercase());
        if atlas::handover::would_hand_out_the_way_back(
            handed.stance.handed_over(),
            has,
            asked.as_deref(),
        ) {
            println!("{}", atlas::handover::not_yours_to_set());
            return;
        }
    }
    let cfg = Config::load(&atlas::roots::config_dir())
        .ok()
        .and_then(|c| c.tools)
        .map(|t| t.vault)
        .unwrap_or_default();
    let mut vault = atlas::vault::Vault::load(&state);
    let now = atlas::store::now();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            if vault.has_a_passphrase() {
                println!("The vault has a passphrase set.");
                // Said here rather than only at set time, because the person
                // who needs to hear it is the one who set a passphrase months
                // ago and never made a recovery key.
                if vault.has_a_recovery_key() {
                    println!(
                        "There's a recovery key for it — the one you wrote down. `atlas vault recovery` makes a new one and retires that one."
                    );
                } else {
                    println!();
                    println!(
                        "There is NO recovery key. If the passphrase goes, so does everything in here — and a handover could never be taken back."
                    );
                    println!("  `atlas vault recovery` makes one. It takes ten seconds.");
                    println!();
                }
            } else {
                println!("No passphrase set yet. `atlas vault passphrase` sets one.");
                println!(
                    "Until then an unlock proves nothing — the first one chooses the \
                     passphrase, whoever types it."
                );
            }
            let kept = vault.list();
            if kept.is_empty() {
                println!("Nothing stored in it.");
            } else {
                println!("{} things stored:", kept.len());
                for (name, kind) in kept {
                    println!("  {name} — {}", kind.plain());
                }
            }
            let weak = vault.weakly_sealed();
            if !weak.is_empty() {
                println!(
                    "{} of them were sealed before the real cipher existed and should be \
                     replaced: {}",
                    weak.len(),
                    weak.join(", ")
                );
            }
        }
        Some("passphrase") | Some("set") | Some("change") => {
            let first = !vault.has_a_passphrase();
            if first {
                println!("Setting the vault passphrase for the first time.");
                println!(
                    "Long beats complicated — a sentence you would not forget, at least \
                     twelve characters. Atlas cannot recover it and neither can anyone \
                     else; that is what makes the vault worth having."
                );
            }
            let old = if first {
                String::new()
            } else {
                match ask_quietly("Current passphrase: ") {
                    Some(p) => p,
                    None => {
                        println!("Nothing typed — unchanged.");
                        return;
                    }
                }
            };
            let Some(new) = ask_quietly("New passphrase: ") else {
                println!("Nothing typed — unchanged.");
                return;
            };
            let Some(again) = ask_quietly("Again: ") else {
                println!("Nothing typed — unchanged.");
                return;
            };
            // The deciding is `vault::set_passphrase`, shared with the hub's
            // Accounts page; what is left here is the terminal's half.
            match atlas::vault::set_passphrase(&mut vault, &old, &new, &again, &cfg, now) {
                Ok((said, issued)) => {
                    if keep(vault.save(&state), "the vault") {
                        println!("{said}");
                        if let Some(code) = issued {
                            show_recovery_key(&code);
                        }
                    }
                }
                Err(why) => println!("{why}"),
            }
            vault.lock();
        }
        // The ways back in that live outside Atlas: an envelope, pieces
        // with people, a key file. Recorded so Atlas can say each one's
        // weakness and remind you to check them (Eric, B5).
        Some("way-back") => {
            let people = atlas::roots::store();
            let mut routes: Vec<atlas::recovery::Setup> = people.load("recovery");
            let rcfg = Config::load(&atlas::roots::config_dir())
                .ok()
                .and_then(|c| c.tools)
                .map(|t| t.recovery)
                .unwrap_or_default();
            match args.get(1).map(|s| s.as_str()) {
                Some("add") => {
                    let rest: Vec<String> = args.iter().skip(2).cloned().collect();
                    let Some(route) = atlas::recovery::route_from(&rest) else {
                        println!("Which kind? envelope, split <pieces> <needed>, person, or keyfile — then where or who.");
                        return;
                    };
                    let skip = if matches!(route, atlas::recovery::Route::SplitBetweenPeople { .. }) { 3 } else { 1 };
                    let with = rest.iter().skip(skip).cloned().collect::<Vec<_>>().join(" ");
                    let s = atlas::recovery::Setup { route, with, set_up_at: now, last_checked: None, used_at: None };
                    println!("Kept. {}", atlas::recovery::described(&s));
                    println!("{}", atlas::recovery::TEST_IT);
                    routes.push(s);
                    keep(people.save("recovery", &routes), "the ways back in");
                }
                Some("checked") => {
                    let n: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                    match routes.get_mut(n.saturating_sub(1)) {
                        Some(r) if n > 0 => {
                            r.last_checked = Some(now);
                            println!("Marked checked: {}", r.route.plain());
                            keep(people.save("recovery", &routes), "the ways back in");
                        }
                        _ => println!("Which one? The numbers are in the list."),
                    }
                }
                _ => {
                    if routes.is_empty() {
                        println!("No ways back in are recorded yet. {}", atlas::recovery::NOT_ATLAS);
                    }
                    for (i, r) in routes.iter().enumerate() {
                        println!("{}. {}", i + 1, atlas::recovery::described(r));
                    }
                    if !routes.is_empty() {
                        println!("{}", atlas::recovery::spoken(&routes, &rcfg, now));
                    }
                }
            }
        }
        Some("recovery") | Some("recovery-key") | Some("newkey") => {
            if !vault.has_a_passphrase() {
                println!(
                    "There's no passphrase on this vault yet, so there's nothing to make a second way into. `atlas vault passphrase` first."
                );
                return;
            }
            if vault.has_a_recovery_key() {
                println!("This makes a NEW recovery key and retires the one you have.");
                println!("If you still have the old one written down somewhere, it stops working.");
                println!();
            }
            let Some(phrase) = ask_quietly("Passphrase: ") else {
                println!("Nothing typed — unchanged.");
                return;
            };
            if let Err(why) = vault.open(&phrase, now, &cfg) {
                println!("{why}");
                return;
            }
            match vault.issue_recovery_key(now, &cfg) {
                Ok(code) => {
                    if keep(vault.save(&state), "the vault") {
                        show_recovery_key(&code);
                    }
                }
                Err(why) => println!("{why}"),
            }
            vault.lock();
        }
        Some("recover") | Some("forgot") | Some("use-recovery") => {
            if !vault.has_a_recovery_key() {
                println!(
                    "There's no recovery key on this vault. One is made when you set the passphrase, or with `atlas vault recovery`."
                );
                println!(
                    "Without one and without the passphrase, what's in the vault cannot be recovered -- not by me and not by anyone. That is what the encryption is."
                );
                return;
            }
            println!("Type the recovery key you wrote down. Case and dashes don't matter.");
            let Some(code) = ask_quietly("Recovery key: ") else {
                println!("Nothing typed — unchanged.");
                return;
            };
            if let Err(why) = vault.open_with_recovery_key(&code, now, &cfg) {
                println!("{why}");
                return;
            }
            // Said with the name of the door it came through, because the
            // next sentence asks the person to change their passphrase and
            // that instruction only makes sense if they know they got in
            // without one.
            match vault.opened_with() {
                Some(how) => println!("That's it — the vault is open, on {}.", how.plain()),
                None => println!("That's it — the vault is open."),
            }
            println!();
            // Not optional, and not a suggestion. Coming in this way means the
            // passphrase is gone, so leaving without setting a new one leaves
            // a vault whose only key is the piece of paper that was just used
            // -- which is the situation this whole feature exists to prevent,
            // reached from the other side.
            println!("Set a new passphrase now. The old one is gone either way.");
            let Some(new) = ask_quietly("New passphrase: ") else {
                println!(
                    "Nothing typed. The vault is shut again and the recovery key still works -- but the passphrase is still gone, so do this again when you can."
                );
                vault.lock();
                return;
            };
            let Some(again) = ask_quietly("Again: ") else {
                println!("Nothing typed — the passphrase is unchanged.");
                vault.lock();
                return;
            };
            if new != again {
                println!("Those two didn't match. Nothing changed, and the recovery key still works.");
                vault.lock();
                return;
            }
            match vault.set_passphrase_from_recovery(&new, now, &cfg) {
                Ok(()) => {
                    if keep(vault.save(&state), "the vault") {
                        println!("Set. Everything in the vault is still there.");
                        println!();
                        println!(
                            "That recovery key still works. `atlas vault recovery` replaces it with a new one if you would rather the used one stopped working."
                        );
                    }
                }
                Err(why) => println!("{why}"),
            }
            vault.lock();
        }
        Some("forget-recovery") => {
            let Some(phrase) = ask_quietly("Passphrase: ") else {
                println!("Nothing typed — unchanged.");
                return;
            };
            if let Err(why) = vault.open(&phrase, now, &cfg) {
                println!("{why}");
                return;
            }
            if vault.forget_recovery_key() {
                if keep(vault.save(&state), "the vault") {
                    println!("Gone. The passphrase is the only way in now.");
                    println!("If it goes, so does everything in here.");
                }
            } else {
                println!("There wasn't one.");
            }
            vault.lock();
        }
        Some(other) => {
            println!("I don't know `atlas vault {other}`.");
            println!("  atlas vault             — what's in it");
            println!("  atlas vault passphrase  — set or change the passphrase");
            println!("  atlas vault recovery    — make a new recovery key");
            println!("  atlas vault recover     — get in with the recovery key");
        }
    }
}

/// Handing the machine to somebody else, and taking it back.
///
/// The two halves are guarded differently on purpose, and that asymmetry is
/// the whole design -- see `handover.rs`. Entering narrows what Atlas will
/// do, so anyone may do it, including the person you handed it to. Leaving
/// grants, so it costs the vault passphrase, which is the one thing in this
/// tree that proves who is typing.
pub(super) fn run_handover(args: &[String]) {
    let state = atlas::roots::install_state();
    let mut h = atlas::handover::Handover::load(&state);
    let now = atlas::store::now();
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => println!("{}", h.spoken(now)),
        Some("to") | Some("start") | Some("on") => {
            let note = args[1..].join(" ");
            let said = h.hand_over(&note, now);
            keep(h.save(&state), "the handover");
            println!("{said}");
            // Said *after* it is done, never before, and never as a question.
            // Entering a handover is the one thing that must not be slowed
            // down or argued with -- you are standing there with somebody
            // waiting for the laptop. But a handover you cannot take back is
            // worth one sentence.
            let vault = atlas::vault::Vault::load(&state);
            if !vault.has_a_passphrase() {
                println!();
                println!(
                    "One thing: there's no passphrase on this vault, so nothing can \
                     prove you're you when you want this back. Right now the only way \
                     out is `atlas handover back` refusing, and me telling you to set \
                     one -- which you can't do until it's yours again."
                );
                println!("Set one with `atlas vault` next time it's yours.");
            } else if !vault.has_a_recovery_key() {
                println!();
                println!(
                    "(No recovery key on this vault. If the passphrase won't come to \
                     you, there's no second way back. `atlas vault recovery` makes one.)"
                );
            }
        }
        Some("back") | Some("mine") | Some("off") => {
            if !h.stance.handed_over() {
                println!("{}", h.spoken(now));
                return;
            }
            let cfg = Config::load(&atlas::roots::config_dir())
                .ok()
                .and_then(|c| c.tools)
                .map(|t| t.vault)
                .unwrap_or_default();
            let mut vault = atlas::vault::Vault::load(&state);
            if !vault.has_a_passphrase() {
                println!(
                    "There's no passphrase on this vault yet, so unlocking it wouldn't prove \
                     anything -- the first unlock sets one. Set one while this is yours, and \
                     then a handover can be taken back."
                );
                return;
            }
            let phrase = match ask_quietly("Passphrase: ") {
                Some(p) => p,
                None => {
                    println!("Nothing typed — it's still handed over.");
                    return;
                }
            };
            // Checked, counted, saved and locked again in
            // `handover::take_back_with` -- the same sequence "I'm back" and
            // the hub's Accounts page go through.
            println!("{}", atlas::handover::take_back_with(&state, &mut vault, &phrase, &cfg, now));
        }
        Some(other) => {
            println!("I don't know `atlas handover {other}`.");
            println!("  atlas handover              — what it's doing now");
            println!("  atlas handover to <note>    — somebody else has it");
            println!("  atlas handover back         — yours again, needs the passphrase");
            println!();
            println!("  Out loud, to a running Atlas: \"I'm back\" does the same thing —");
            println!("  it asks for the passphrase in writing rather than taking a spoken one.");
        }
    }
}

/// Put a recovery key in front of somebody, once.
///
/// Deliberately loud, deliberately in the way, and it waits for a keypress.
/// This string exists exactly once and is never recoverable afterwards, so a
/// line of output that scrolls past among others is the same as not printing
/// it at all.
fn show_recovery_key(code: &str) {
    println!();
    println!("  ================================================================");
    println!("    YOUR RECOVERY KEY — write this down now");
    println!("  ================================================================");
    println!();
    println!("      {code}");
    println!();
    println!("  This is the only time it will be shown. It is not stored anywhere");
    println!("  and cannot be printed again -- a new one can be made, which stops");
    println!("  this one working.");
    println!();
    println!("  What it is for: if you forget the passphrase, this gets you back");
    println!("  into the vault, and it takes a handover back. Without it, a");
    println!("  forgotten passphrase is final.");
    println!();
    println!("  Where it goes: on paper, or on a phone or drive that is not this");
    println!("  machine. Anyone holding it has the vault, so it does not belong");
    println!("  in a file on this computer, and it does not belong on a sticky");
    println!("  note on this screen.");
    println!();
    print!("  Press Enter once you have written it down. ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    println!();
}

/// `atlas accounts` — the book of what you have accounts with.
///
/// Written because `daemon.rs` says, out loud, "`atlas accounts` is where they
/// go" when it has nothing to tell you about travel. `accounts.rs` has had a
/// `Book`, an audit and advice for weeks, reachable by nothing.
pub(super) fn run_accounts(args: &[String]) {
    let store = atlas::roots::store();
    let mut book = atlas::accounts::Book::load(&store);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            if book.accounts.is_empty() {
                println!("Nothing in the book yet.");
                println!("  atlas accounts add <site>   — start keeping track of one");
                return;
            }
            let advice = atlas::accounts::audit(&book.accounts);
            println!("{}", atlas::accounts::spoken(&book.accounts, &advice));
            println!();
            for a in &book.accounts {
                println!(
                    "  {}  — {}{}",
                    a.site,
                    a.second_factor.plain(),
                    if a.has_recovery_codes { ", recovery codes saved" } else { "" }
                );
            }
        }
        Some("add") => {
            let site = args[1..].join(" ");
            if site.trim().is_empty() {
                println!("Which site? `atlas accounts add github.com`");
                return;
            }
            if book.note(&site) {
                keep(book.save(&store), "the account book");
                println!("Added {site}. I've guessed how much it matters from what it is;");
                println!("`atlas accounts` says what I'd change about it.");
            } else {
                println!("{site} is already in the book.");
            }
        }
        Some("forget") => {
            let site = args[1..].join(" ");
            if book.forget(&site) {
                keep(book.save(&store), "the account book");
                println!("Forgotten {site}.");
            } else {
                println!("Nothing in the book called {site}.");
            }
        }
        // The verb that was missing, and its absence was not a silence.
        //
        // `Book::set_recovery_codes` had no caller anywhere, so
        // `has_recovery_codes` was false for every account that ever existed
        // -- and `goingaway::would_lock_you_out` clears an account off the
        // list exactly when that flag is true. Atlas told you every
        // second-factor account would strand you abroad, confidently, every
        // time you asked. A missing CLI verb producing a wrong answer.
        Some("codes") => {
            let rest: Vec<&String> = args[1..].iter().collect();
            let has = !rest.last().map(|s| s.as_str()) .is_some_and(|s| {
                matches!(s.to_lowercase().as_str(), "no" | "none" | "false" | "off")
            });
            let site: String = if has {
                rest.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
            } else {
                rest[..rest.len() - 1].iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ")
            };
            if site.trim().is_empty() {
                println!("Which site? `atlas accounts codes github.com`");
                println!("  ... and `atlas accounts codes github.com no` to take it back.");
                return;
            }
            match book.record_codes(&store, &site, has) {
                atlas::accounts::Recorded::Changed if has => {
                    println!("Noted — {site}'s recovery codes are somewhere you can reach.");
                    println!("It won't be counted as one that strands you when you travel.");
                }
                atlas::accounts::Recorded::Changed => {
                    println!("Noted — {site} has no reachable recovery codes.");
                }
                atlas::accounts::Recorded::AlreadySo => {
                    println!("That was already what I had for {site}.");
                }
                atlas::accounts::Recorded::NoSuchSite => {
                    println!("Nothing in the book called {site}.");
                    println!("  atlas accounts add {site}");
                }
                // Said rather than swallowed: the whole point of this verb is
                // that the answer it changes is one you rely on later.
                atlas::accounts::Recorded::CouldNotWrite(why) => {
                    println!("I couldn't write the account book, so that hasn't stuck: {why}");
                }
            }
        }
        Some(other) => {
            println!("I don't know `atlas accounts {other}`.");
            println!("  atlas accounts             — what you have, and what I'd change");
            println!("  atlas accounts add <site>  — start keeping track of one");
            println!("  atlas accounts forget <s>  — stop");
            println!("  atlas accounts codes <s>   — its recovery codes are reachable");
        }
    }
}

pub(super) fn run_profiles(args: &[String]) {
    let root = atlas::roots::state_dir();
    let mut profiles = atlas::profiles::Profiles::load(&root);
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            println!("{}", profiles.summary());
            for p in &profiles.profiles {
                let role = match p.role {
                    atlas::profiles::Role::Owner => "owner",
                    atlas::profiles::Role::Guest => "guest",
                };
                let active = if profiles.active.as_deref() == Some(&p.id) { " (active)" } else { "" };
                println!("  {} — {role}{active}", p.name);
            }
        }
        Some("add") => {
            let guest = args.iter().any(|a| a == "--guest");
            let name: String =
                args[1..].iter().filter(|a| a.as_str() != "--guest").cloned().collect::<Vec<_>>().join(" ");
            if name.trim().is_empty() {
                println!("atlas profiles add <name> [--guest]");
                return;
            }
            let role =
                if guest { atlas::profiles::Role::Guest } else { atlas::profiles::Role::Owner };
            match profiles.add(name.trim(), role) {
                Ok(p) => {
                    keep(profiles.save(&root), "the profiles");
                    println!("Added {} as {}.", p.name, if guest { "a guest" } else { "an owner" });
                }
                Err(e) => println!("Couldn't add that: {e}"),
            }
        }
        Some("switch") => {
            let name = args[1..].join(" ");
            let Some(target) = profiles.match_name(&name).map(|p| p.id.clone()) else {
                println!("No profile matches \"{name}\".");
                return;
            };
            match profiles.switch(&target, atlas::store::now()) {
                Ok(switch) => {
                    let display_name =
                        profiles.get(&target).map(|p| p.name.clone()).unwrap_or(target);
                    if !keep(profiles.save(&root), "the profiles") {
                        println!("You are still {display_name} for this session only.");
                        return;
                    }
                    println!("{}", switch.say(&display_name));
                    // Named rather than claimed. These are what lives in a
                    // person's own directory and therefore what a restart
                    // will be reading from theirs instead of yours -- it is
                    // the list of what *is* separate, not a list of things
                    // this command just did.
                    println!(
                        "Theirs, not yours, from the next start: {}.",
                        switch.must_forget().join(", ")
                    );
                }
                Err(e) => println!("Couldn't switch: {e}"),
            }
        }
        Some("remove") => {
            let name = args[1..].join(" ");
            let Some(id) = profiles.match_name(&name).map(|p| p.id.clone()) else {
                println!("No profile matches \"{name}\".");
                return;
            };
            match profiles.remove(&id) {
                Ok(()) => {
                    if keep(profiles.save(&root), "the profiles") {
                        println!("Removed.");
                    }
                }
                Err(e) => println!("Couldn't remove that: {e}"),
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try list, add, switch or remove."),
    }
}

/// The names of the accounts, which is what the codes side works in.
pub(super) fn account_names(book: &atlas::accounts::Book) -> Vec<String> {
    book.accounts.iter().map(|a| a.site.clone()).collect()
}

/// Ten one-time strings on paper, and whether you actually have them.
///
/// `codes.check_days_before` was a threshold on a trip nothing recorded, and
/// `used_one`, `logins_available` and `Set::running_low` had no callers --
/// the module could say what was missing and there was no way to tell it
/// anything had changed.
pub(super) fn run_codes(cfg: &Config, args: &[String]) {
    use atlas::codes::{self, Set};

    let store = atlas::roots::store();
    let ccfg = cfg.tools.as_ref().map(|t| t.codes.clone()).unwrap_or_default();
    let mut sets: Vec<Set> = store.load("codes");
    let book: atlas::accounts::Book = store.load(atlas::accounts::FILE);
    let now = atlas::store::now();
    let flag = |name: &str| atlas::cli::flag_value(args, name);
    let site_arg = || args.get(1).filter(|s| !s.starts_with("--")).cloned();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            if sets.is_empty() {
                println!("No recovery codes recorded.");
            } else {
                for s in &sets {
                    println!(
                        "  {:<20} {} of {} left{}{}",
                        s.site,
                        s.left(),
                        s.issued,
                        if s.in_hand { ", printed and on you" } else { ", NOT in hand" },
                        if s.running_low(&ccfg) { " -- running low" } else { "" }
                    );
                    if let Some(w) = &s.kept_where {
                        println!("      kept: {w}");
                    }
                }
            }
            println!();
            println!(
                "{} logins you could actually use from a machine that isn't yours.",
                codes::logins_available(&sets)
            );
            println!();
            println!("{}", codes::before_you_go(&sets, &account_names(&book), &ccfg));
            println!();
            println!("atlas codes have <site> --issued 10 [--where \"the blue folder\"]");
            println!("atlas codes in-hand <site>        you've printed them and they're on you");
            println!("atlas codes used <site>           you spent one");
            println!("atlas codes where <site>          where that service hides them");
            println!();
            println!("{}", codes::THE_LIMIT);
        }
        Some("have") => {
            let Some(site) = site_arg() else {
                println!("Which site? atlas codes have google --issued 10");
                return;
            };
            let issued = flag("--issued").and_then(|n| n.parse::<u32>().ok()).unwrap_or(10);
            let kept = flag("--where").map(|s| s.to_string());
            sets.retain(|s| !s.site.eq_ignore_ascii_case(&site));
            sets.push(Set {
                site: site.clone(),
                issued,
                used: 0,
                at: now,
                kept_where: kept,
                // Never assumed. Generating a set and printing it are two
                // different days, and the number that matters counts only
                // what you have said is on you.
                in_hand: false,
            });
            keep(store.save("codes", &sets), "store");
            println!("{issued} recorded for {site}. None spent.");
            println!("`atlas codes in-hand {site}` once they're printed and in your bag --");
            println!("until then they don't count towards what you could use from a hotel.");
        }
        Some("in-hand") => match site_arg() {
            Some(site) => {
                match sets.iter_mut().find(|s| s.site.eq_ignore_ascii_case(&site)) {
                    Some(s) => {
                        s.in_hand = true;
                        keep(store.save("codes", &sets), "store");
                        println!("{site}: printed and on you. {} logins.", codes::logins_available(&sets));
                    }
                    None => println!("I haven't got a set recorded for {site}."),
                }
            }
            None => println!("Which site? atlas codes in-hand google"),
        },
        Some("used") => match site_arg() {
            Some(site) => match codes::used_one(&mut sets, &site) {
                Some(left) => {
                    keep(store.save("codes", &sets), "store");
                    println!("{left} left for {site}.");
                    if let Some(s) = sets.iter().find(|s| s.site.eq_ignore_ascii_case(&site)) {
                        if s.running_low(&ccfg) {
                            println!("That's running low -- print a fresh set before you go.");
                            if let Some((url, called, _)) = codes::where_to_get(&site) {
                                println!("  {site} calls them \"{called}\": {url}");
                            }
                        }
                    }
                }
                None => println!("I haven't got a set recorded for {site}."),
            },
            None => println!("Which site? atlas codes used google"),
        },
        Some("where") => match site_arg() {
            Some(site) => match codes::where_to_get(&site) {
                Some((url, called, where_to_look)) => {
                    println!("{site} calls them \"{called}\".");
                    println!("  {url}");
                    println!("  {where_to_look}");
                }
                None => println!("I don't know where {site} hides them -- left out rather than guessed."),
            },
            None => println!("Which site? atlas codes where google"),
        },
        Some(other) => println!("I don't know `atlas codes {other}`. Try `atlas codes list`."),
    }
}

/// Which sites Atlas can sign into, and taking that away.
///
/// The module said "access is per-site and revocable from one page" and none
/// of it worked: nothing called `Access::grant`, so there was never a grant;
/// nothing loaded or saved an `Access`, so one could not have survived a
/// restart; the access page was handed an empty list of sites; and the revoke
/// buttons it rendered posted to routes that did not exist.
///
/// This is the half a person starts from. The revoking is on the page, which
/// is where the module always said it would be.
pub(super) fn run_access(cfg: &Config, args: &[String]) {
    use atlas::signin::{self, Access, Allowed};

    let store = atlas::roots::store();
    let mut access = Access::load(&store);
    let now = atlas::store::now();
    let flag = |name: &str| atlas::cli::flag_value(args, name);
    let scfg = cfg.tools.as_ref().map(|t| t.signin.clone()).unwrap_or_default();

    let say_list = |a: &Access| {
        let rows = signin::hub_rows(a, now);
        if rows.is_empty() {
            println!("Nothing. I can't sign you into anything.");
        } else {
            for (label, detail, domain, needs_attention) in rows {
                println!("  {}{label} — {detail}", if needs_attention { "! " } else { "  " });
                println!("      {domain}");
            }
        }
        println!();
        println!("{}", signin::spoken(a, now));
    };

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            if !scfg.enabled {
                println!("Signing in is switched off (`signin.enabled: true` in tools.yaml).");
                println!("What is below is what it would have, not what it is doing.");
                println!();
            }
            say_list(&access);
            println!();
            println!("atlas access give <site> --as <account> --entry <vault entry>");
            println!("                        [--name \"what you call it\"] [--and-use]");
            println!("atlas access take <site>          take it back, now");
            println!("atlas access take-all             all of it");
            println!("atlas access changed <site> --as <account> --entry <new vault entry>");
            println!();
            println!("{}", signin::BANKS_ARE_SITES_TOO);
        }
        Some("give") => {
            let Some(site) = args.get(1).filter(|s| !s.starts_with("--")).cloned() else {
                println!("Which site? atlas access give mybank.com --as eric --entry \"bank login\"");
                return;
            };
            let Some(entry) = flag("--entry").map(|s| s.to_string()) else {
                println!("Which vault entry holds the login? --entry \"bank login\"");
                println!("Atlas never stores the password here -- a grant points at the vault.");
                return;
            };
            // Checked rather than taken on trust. A grant pointing at an entry
            // that does not exist fails at the worst moment, on a sign-in
            // page, with a message about the vault rather than about the typo.
            // Names and kinds are readable while the vault is sealed, on
            // purpose, so this works without unlocking anything.
            let vault = atlas::vault::Vault::load(&atlas::roots::install_state());
            let names: Vec<&str> = vault.list().into_iter().map(|(n, _)| n).collect();
            if !names.iter().any(|n| n.eq_ignore_ascii_case(entry.trim())) {
                println!("There's nothing in the vault called \"{entry}\".");
                if names.is_empty() {
                    println!("The vault is empty, so there is nothing to point a grant at yet.");
                } else {
                    println!("What is in there: {}", names.join(", "));
                }
                println!("Nothing granted -- a grant pointing at an entry that isn't there fails");
                println!("on a sign-in page, with a message about the vault rather than the typo.");
                return;
            }
            let account = flag("--as").map(|s| s.to_string()).unwrap_or_default();
            let name = flag("--name")
                .map(|s| s.to_string())
                .unwrap_or_else(|| signin::registered_domain(&site));
            let allowed = if args.iter().any(|a| a == "--and-use") {
                Allowed::SignInAndUse
            } else {
                Allowed::SignIn
            };
            access.grant(&site, &account, &name, allowed, entry.trim(), now);
            keep(access.save(&store), "store");
            println!(
                "Granted: {} on {}{}.",
                match allowed {
                    Allowed::SignInAndUse => "sign in and act as you",
                    Allowed::SignIn => "sign in",
                    Allowed::Nothing => "nothing",
                },
                signin::registered_domain(&site),
                if account.is_empty() { String::new() } else { format!(" as {account}") }
            );
            println!("Take it back any time on the Access page, or `atlas access take {site}`.");
            println!();
            println!("{}", signin::SIGNIN_IS_NOT_SETTINGS);
        }
        Some("take") => match args.get(1) {
            Some(site) if access.revoke(site) => {
                keep(access.save(&store), "store");
                println!("Gone. Your password hasn't changed -- I simply don't have it any more.");
            }
            Some(site) => println!("I didn't have access to {site}."),
            None => println!("Which site? atlas access take mybank.com"),
        },
        Some("take-all") => {
            let n = access.revoke_all();
            keep(access.save(&store), "store");
            println!("{n} gone. None of your passwords changed.");
        }
        Some("changed") => {
            let Some(site) = args.get(1).filter(|s| !s.starts_with("--")).cloned() else {
                println!("Which site? atlas access changed mybank.com --as eric --entry \"new bank login\"");
                return;
            };
            let account = flag("--as").map(|s| s.to_string()).unwrap_or_default();
            let Some(entry) = flag("--entry").map(|s| s.to_string()) else {
                println!("Which vault entry holds the new login? --entry \"bank login\"");
                return;
            };
            if access.superseded_by(&site, &account, entry.trim()) {
                keep(access.save(&store), "store");
                println!("Pointed at \"{entry}\". I'll stop saying the password looks changed.");
            } else {
                println!("I haven't got a grant for {site}{}.", if account.is_empty() {
                    String::new()
                } else {
                    format!(" as {account}")
                });
            }
        }
        Some(other) => println!("I don't know `atlas access {other}`. Try `atlas access list`."),
    }
}

/// If something happens to you.
///
/// `afterme.rs` was complete, tested and unreachable: `gaps` wants a place, a
/// list of people told and something counting the days, and nothing in the
/// tree held any of those, so nothing could call it. `after_me:` sat in
/// `tools.yaml` being parsed into a field nobody read.
///
/// What was missing was never the logic — it was somewhere to keep what you
/// have actually arranged, and a way to say it. Nothing here is a secret:
/// a kind of place, who was told, what they were told, and what is counting.
pub(super) fn run_afterme(cfg: &Config, args: &[String]) {
    use atlas::afterme::{self, Arrangement, Instruction, When};

    let store = atlas::roots::store();
    let acfg: afterme::AfterMeConfig =
        cfg.tools.as_ref().map(|t| t.after_me.clone()).unwrap_or_default();
    let mut a: Arrangement = store.load(afterme::RECORD);
    let now = atlas::store::now();
    let flag = |name: &str| atlas::cli::flag_value(args, name);
    let flagged = |f: &str| args.iter().any(|a| a == f);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("show") => {
            if !acfg.enabled {
                println!("The envelope arrangement is switched off. `after_me.enabled: true` in");
                println!("config/tools.yaml turns it on -- nothing below is acted on until it is.");
                println!();
            }
            print!("{}", a.spoken(&acfg, now));
            println!();
            println!("{}", afterme::THE_SHAPE);
            println!();
            println!("atlas afterme where yours|bank|sealed|split");
            println!("atlas afterme told <name> --location \"the safe in the study\" [--days 180]");
            println!("atlas afterme confirmed <name>       they've said yes");
            println!("atlas afterme timer platforms|phone|person|atlas");
            println!("atlas afterme reviewed               you've checked it still holds");
        }
        Some("shape") => {
            println!("{}", afterme::THE_SHAPE);
            println!();
            for (w, why) in afterme::suggest_for_you() {
                println!("  {why}");
                println!("      ({})", w.the_catch());
            }
        }
        Some("where") => match args.get(1).and_then(|w| afterme::where_from(w)) {
            Some(w) => {
                a.kind = Some(w);
                keep(store.save(afterme::RECORD, &a), "store");
                println!("Noted: {}", w.the_catch());
                if w.openable_without_you() {
                    println!("Worth knowing: somebody could open that without you finding out.");
                }
            }
            None => println!("Which? yours, bank, sealed or split."),
        },
        Some("timer") => match args.get(1).and_then(|w| afterme::timer_from(w)) {
            Some(t) => {
                a.timer = Some(t);
                keep(store.save(afterme::RECORD, &a), "store");
                println!("Counting: {}", t.why());
            }
            None => println!("Which? platforms, phone, person or atlas."),
        },
        Some("told") => {
            let Some(person) = args.get(1).filter(|p| !p.starts_with("--")).cloned() else {
                println!("Told who? atlas afterme told Sam --location \"the safe in the study\"");
                return;
            };
            let location = flag("--location").map(|s| s.to_string()).unwrap_or_else(|| {
                // Their words, not yours: what this person is given is a
                // place they could find, and `after_me.location` is the one
                // you already wrote down.
                acfg.location.clone()
            });
            if location.trim().is_empty() {
                println!("Where is it, in words they'd understand?");
                println!("  atlas afterme told {person} --location \"the safe in the study\"");
                return;
            }
            let when = if flagged("--only-if-i-say") {
                When::OnlyIfYouSaySo
            } else {
                When::OutOfContact {
                    days: flag("--days")
                        .and_then(|d| d.parse::<u32>().ok())
                        .unwrap_or(acfg.after_days),
                }
            };
            let then_what = flag("--then")
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Open it and follow what's inside.".to_string());
            let i = Instruction {
                person: person.clone(),
                location,
                when,
                then_what,
                they_know: false,
            };
            println!("What {person} would be told, and nothing more:");
            println!("  \"{}\"", i.as_told());
            a.tell(i);
            keep(store.save(afterme::RECORD, &a), "store");
            println!();
            println!("Say it to them, then `atlas afterme confirmed {person}`.");
            println!("Until they've agreed, this isn't an arrangement -- it's a note to yourself.");
        }
        Some("confirmed") => match args.get(1) {
            Some(person) if a.they_agreed(person) => {
                a.reviewed_at = now;
                keep(store.save(afterme::RECORD, &a), "store");
                println!("{person} knows and has agreed.");
            }
            Some(person) => println!("{person} hasn't been told anything yet."),
            None => println!("Who? atlas afterme confirmed Sam"),
        },
        Some("reviewed") => {
            a.reviewed_at = now;
            keep(store.save(afterme::RECORD, &a), "store");
            println!("Noted. I'll ask again in {} days.", acfg.review_every_days);
        }
        Some(other) => println!("I don't know `atlas afterme {other}`. Try `atlas afterme`."),
    }
}
