//! Moving your things between machines: carrying files, the remote laptop,
//! sync and setting it up.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

// ===========================================================================
// Away from the laptop — `atlas carry`, `atlas remote`, `atlas mobile`,
// `atlas sync-setup <provider>`.
//
// Four modules (`workingset`, `remote`, `companion`/`ios`/`android`,
// `cloudsync`) sat in `UNWIRED_BASELINE` for the same reason: each is the
// *thinking* half of something whose other half is a phone app nobody has
// built. That is a real reason for the phone side to be missing and no reason
// at all for the laptop side to be unreachable. Packing a working set against
// real files on this disk, queueing a request and probing a real paired
// address to see whether the machine that must run it is up, and answering
// "what will this actually do on my phone" are all things the laptop can do
// today, alone.
//
// What is deliberately NOT faked here: nothing invents a phone. `carry back`
// takes the files you actually brought home and compares them against what
// was actually packed; `remote ask` reports reachability from a real TCP
// probe of a real paired contact, or says plainly that there is no contact to
// probe. Where the answer needs a device that isn't here, the command says so
// instead of printing a plausible sentence.
// ===========================================================================

/// Worth shrinking for the trip rather than leaving behind.
///
/// Extension-based on purpose: the alternative is opening every file to see
/// what it is, which costs more than the decision is worth and is wrong on
/// exactly the files (a `.dat` that is really a JPEG) nobody carries anyway.
fn carry_shrinkable(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some(
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "mp4"
                | "mov" | "mkv" | "avi" | "webm" | "wav" | "flac" | "aiff"
        )
    )
}

/// A real file on this disk, measured rather than described.
fn carry_file(
    path: &std::path::Path,
    because: atlas::workingset::Why,
) -> Option<atlas::workingset::Carried> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some(atlas::workingset::Carried {
        path: path.display().to_string(),
        name: path.file_name()?.to_string_lossy().into_owned(),
        bytes: meta.len(),
        because,
        shrinkable: carry_shrinkable(path),
    })
}

/// Seconds since the epoch that a path was last written, or 0 if unknown.
pub(super) fn mtime_secs(path: &std::path::Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(super) fn run_carry(cfg: &Config, args: &[String]) {
    use atlas::workingset::{self, Carried, Changed, Why};

    let store = atlas::roots::store();
    let wcfg = cfg
        .tools
        .as_ref()
        .map(|t| t.working_set.clone())
        .unwrap_or_default();

    if !wcfg.enabled {
        println!("Carrying work is switched off (working_set.enabled: false in tools.yaml).");
        return;
    }

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            let taking: Vec<Carried> = store.load("working_set");
            let left: Vec<(String, String)> = store.load("working_set_left");
            if taking.is_empty() && left.is_empty() {
                println!("Nothing packed. `atlas carry pack <file>...` to work one out.");
                return;
            }
            let total: u64 = taking.iter().map(|c| c.bytes).sum();
            println!(
                "Carrying {} file{}, {}MB of a {}MB budget.",
                taking.len(),
                if taking.len() == 1 { "" } else { "s" },
                total / 1_048_576,
                wcfg.budget_mb
            );
            for c in &taking {
                println!(
                    "  {:>7}KB  {}  ({})",
                    c.bytes / 1024,
                    c.name,
                    match c.because {
                        Why::Needed => "needed",
                        Why::Made => "Atlas made it",
                        Why::YouHadItOpen => "you had it open",
                        Why::YouMentionedIt => "you mentioned it",
                        Why::NearSomethingNeeded => "near something needed",
                    }
                );
            }
            for (name, why) in &left {
                println!("  left behind: {name} — {why}");
            }
            println!();
            println!("atlas carry missing <name>   what to do about one that didn't come");
            println!("atlas carry back <folder>    bring changed copies home");
        }
        Some("pack") => {
            let named: Vec<std::path::PathBuf> = args[1..]
                .iter()
                .filter(|a| !a.starts_with("--"))
                .map(std::path::PathBuf::from)
                .collect();
            if named.is_empty() {
                println!("Which files? atlas carry pack notes.md draft.mp4");
                return;
            }

            let mut files: Vec<Carried> = Vec::new();
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

            for p in &named {
                match carry_file(p, Why::Needed) {
                    Some(c) => {
                        seen.insert(c.path.clone());
                        files.push(c);
                    }
                    None => println!("(skipping {} — not a readable file)", p.display()),
                }
            }
            if files.is_empty() {
                println!("None of those are files I can read. Nothing packed.");
                return;
            }

            // Everything sitting beside something needed. `pack` weights these
            // lowest, so they fill whatever space is left and are the first
            // thing dropped — which is the correct treatment for "it was in
            // the same folder".
            for p in &named {
                let Some(parent) = p.parent() else { continue };
                let Ok(entries) = std::fs::read_dir(if parent.as_os_str().is_empty() {
                    std::path::Path::new(".")
                } else {
                    parent
                }) else {
                    continue;
                };
                for e in entries.flatten() {
                    let path = e.path();
                    let key = path.display().to_string();
                    if seen.contains(&key) {
                        continue;
                    }
                    if let Some(c) = carry_file(&path, Why::NearSomethingNeeded) {
                        seen.insert(key);
                        files.push(c);
                    }
                }
            }

            let packed = workingset::pack(&files, &wcfg);
            println!("{}", workingset::spoken(&packed));
            println!();
            for c in &packed.taking {
                println!("  {:>7}KB  {}", c.bytes / 1024, c.name);
            }
            for (name, why) in &packed.leaving {
                println!("  left: {name} — {why}");
            }

            keep(store.save("working_set", &packed.taking), "store");
            keep(store.save("working_set_left", &packed.leaving), "store");
            keep(store.save("working_set_packed_at", &atlas::store::now()), "store");
            println!();
            println!("Recorded. `atlas carry back <folder>` when you get home.");
        }
        Some("missing") => {
            let Some(name) = args.get(1) else {
                println!("Which one? atlas carry missing draft.mp4");
                return;
            };
            let left: Vec<(String, String)> = store.load("working_set_left");
            match left.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                Some((n, why)) => println!("{}", workingset::not_here(n, why)),
                None => {
                    let taking: Vec<Carried> = store.load("working_set");
                    if taking.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
                        println!("{name} did come with us — it's in the carried set.");
                    } else {
                        println!("{name} wasn't in the last pack either way.");
                    }
                }
            }
        }
        Some("back") => {
            let Some(folder) = args.get(1) else {
                println!("Which folder did the changed copies come home in? atlas carry back ~/from-phone");
                return;
            };
            let taking: Vec<Carried> = store.load("working_set");
            if taking.is_empty() {
                println!("Nothing was packed, so nothing can come back.");
                return;
            }
            let packed_at: u64 = store.load("working_set_packed_at");

            // What actually came home: files in that folder whose names match
            // something that went out. Matched by name rather than path
            // because the phone will not have reproduced the laptop's folder
            // layout, and pretending otherwise would silently match nothing.
            let mut changed: Vec<Changed> = Vec::new();
            let Ok(entries) = std::fs::read_dir(folder) else {
                println!("Can't read {folder}.");
                return;
            };
            for e in entries.flatten() {
                let path = e.path();
                let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                    continue;
                };
                let Some(origin) = taking.iter().find(|c| c.name == name) else {
                    continue;
                };
                let Ok(meta) = std::fs::metadata(&path) else { continue };
                if !meta.is_file() {
                    continue;
                }
                changed.push(Changed {
                    path: origin.path.clone(),
                    at: mtime_secs(&path),
                    bytes: meta.len(),
                });
            }

            if changed.is_empty() {
                println!("Nothing in {folder} matches anything that went out.");
                return;
            }

            // The other half of the clash test, measured rather than assumed:
            // a carried file whose copy *here* has been written since the pack
            // moved on while you were away.
            let also_here: Vec<String> = taking
                .iter()
                .filter(|c| mtime_secs(std::path::Path::new(&c.path)) > packed_at)
                .map(|c| c.path.clone())
                .collect();

            let (clean, clashes) = workingset::returning(&changed, &also_here);
            println!(
                "{clean} of {} land without a question.",
                changed.len()
            );
            if clashes.is_empty() {
                println!("Nothing changed at both ends.");
            } else {
                println!("Changed in both places — you choose which wins:");
                for p in &clashes {
                    println!("  {p}");
                }
            }
            println!();
            println!("Nothing has been copied. This is the report; the copy is yours to make.");
        }
        Some(other) => {
            println!("I don't know \"{other}\" — try list, pack, missing or back.")
        }
    }
}

/// Read a `How` out of the string `remote.tell_me` holds.
///
/// The config field is a string because it is hand-edited YAML; this is the
/// one place that has to agree with it, so an unrecognised value falls back to
/// the same default the type does rather than failing the command.
fn remote_how(s: &str) -> atlas::remote::How {
    use atlas::remote::How;
    match s.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "quietly" => How::Quietly,
        "dont_tell_me" | "don't_tell_me" | "nothing" => How::DontTellMe,
        _ => How::InYourEar,
    }
}

/// Is the machine that has to run this actually up?
///
/// A real TCP probe of a real paired contact, not a guess. Returns `None` when
/// there is nobody paired to probe — which is a different answer from "not
/// reachable" and is printed as one.
fn laptop_reachable(dir: &std::path::Path, to: Option<&str>) -> Option<(String, bool)> {
    let pairings = atlas::kin::Pairings::load(dir);
    let contact = match to {
        Some(name) => pairings
            .contacts
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))?,
        None => pairings.contacts.first()?,
    };
    let address = format!("{}:{}", contact.host, contact.port);
    Some((contact.name.clone(), atlas::watch::reachable(&address, 800)))
}

/// `atlas remote` — the queue of things that need the other machine.
///
/// The phone half of this does not exist. The queue, the states, the
/// give-up rule and the "was that worth interrupting you for" gate all do, and
/// all of them are the laptop's side of the exchange — which is the side this
/// binary runs on.
pub(super) fn run_remote(cfg: &Config, dir: &std::path::Path, args: &[String]) {
    use atlas::remote::{self, How, Needs, Queue, State};

    let store = atlas::roots::store();
    let rcfg = cfg.tools.as_ref().map(|t| t.remote.clone()).unwrap_or_default();
    let mut q: Queue = store.load("remote_queue");
    let now = atlas::store::now();

    let flagged = |f: &str| args.iter().any(|a| a == f);
    let id_arg = || args.get(1).and_then(|n| n.parse::<u64>().ok());

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            let waiting = q.waiting_count();
            println!(
                "{waiting} waiting, {} in the queue altogether.",
                q.requests.len()
            );
            for r in q.for_the_laptop() {
                println!(
                    "  {}  {}  ({})",
                    r.id,
                    r.what,
                    match r.needs {
                        Needs::TheLaptop => "needs the laptop",
                        Needs::Anything => "could run anywhere",
                    }
                );
            }
            for r in q.given_up(now, &rcfg) {
                let days = now.saturating_sub(r.asked_at) / 86_400;
                println!("  {}", remote::never_ran(r, days));
            }
            println!();
            println!("atlas remote ask <what>          queue it");
            println!("atlas remote start|done|failed|drop <number>");
        }
        Some("ask") => {
            let what = atlas::cli::plain_words(&args[1..], &["--to", "--secs"]);
            if what.trim().is_empty() {
                println!("Ask for what? atlas remote ask \"render the draft\"");
                return;
            }
            let needs = if flagged("--anywhere") { Needs::Anything } else { Needs::TheLaptop };
            let how = if flagged("--quietly") {
                How::Quietly
            } else if flagged("--dont-tell-me") {
                How::DontTellMe
            } else {
                remote_how(&rcfg.tell_me)
            };

            let to = atlas::cli::flag_value(args, "--to");

            let id = q.ask(&what, needs, how, now);
            match laptop_reachable(dir, to) {
                Some((name, up)) => {
                    println!("{}", remote::handing_off(&what, up));
                    println!("(probed {name} just now — {}.)", if up { "up" } else { "not answering" });
                }
                None => {
                    // Honest about the gap rather than picking a branch: with
                    // nothing paired there is no machine to probe, and saying
                    // "queued, it'll go when we're back in touch" would be a
                    // sentence about a relationship that doesn't exist.
                    println!("Queued as #{id}. Nothing is paired yet, so there's no other machine to reach —");
                    println!("`atlas invite` first, and this will go the moment one answers.");
                }
            }
            if remote::worth_waking_for(needs, flagged("--urgent")) {
                println!("Marked urgent and it needs the laptop — worth waking it if you have wake-on-LAN.");
            }
            keep(store.save("remote_queue", &q), "store");
            println!("#{id}.");
        }
        Some("start") => match id_arg() {
            Some(id) => {
                // The gate `confirm_side_effects` was written for. It ships
                // on, and until 19 Sep 2026 nothing read it: `atlas remote
                // start 3` marked a request running with no check of what it
                // was. A safety switch that is on by default and connected to
                // nothing is the worst of the three states.
                let asking = q
                    .requests
                    .iter()
                    .find(|r| r.id == id)
                    .filter(|r| r.state == State::Waiting)
                    .filter(|r| remote::needs_your_yes(&r.what, &rcfg) && !flagged("--yes"))
                    .map(|r| remote::asking_before_it_runs(id, &r.what));
                match asking {
                    Some(said) => println!("{said}"),
                    None if q.set(id, State::Running) => {
                        keep(store.save("remote_queue", &q), "store");
                        println!("#{id} running.");
                    }
                    None => println!("No request #{id}."),
                }
            }
            None => println!("Which one? atlas remote start 3"),
        },
        Some(verb @ ("done" | "failed")) => {
            let Some(id) = id_arg() else {
                println!("Which one? atlas remote {verb} 3 \"what happened\"");
                return;
            };
            let failed = verb == "failed";
            let result = atlas::cli::plain_words(&args[2..], &["--secs"]);
            let took = atlas::cli::flag_value(args, "--secs")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or_else(|| {
                    q.requests
                        .iter()
                        .find(|r| r.id == id)
                        .map(|r| now.saturating_sub(r.asked_at))
                        .unwrap_or(0)
                });

            if !q.set(id, if failed { State::Failed } else { State::Done }) {
                println!("No request #{id}.");
                return;
            }
            keep(store.save("remote_queue", &q), "store");
            let Some(r) = q.requests.iter().find(|r| r.id == id) else { return };
            match remote::finished(r, took, if result.is_empty() { "—" } else { &result }) {
                Some(said) => println!("{said}"),
                None => println!(
                    "#{id} {} quietly — not worth interrupting you for, by your own rule.",
                    if failed { "failed" } else { "finished" }
                ),
            }
        }
        Some("drop") => match id_arg() {
            Some(id) if q.set(id, State::Dropped) => {
                keep(store.save("remote_queue", &q), "store");
                println!("#{id} dropped.");
            }
            Some(id) => println!("No request #{id}."),
            None => println!("Which one? atlas remote drop 3"),
        },
        Some(other) => println!("I don't know \"{other}\" — try list, ask, start, done, failed or drop."),
    }
}

/// `atlas sync-setup <provider>` — the folder two devices meet in.
///
/// With no provider named this still opens the hub, which is what it did
/// before. Naming one gets the actual steps, split into what Atlas does and
/// what only you can do, plus an honest answer about whether the free tier is
/// enough for the traffic your settings imply.
/// `atlas sync key ...` and `atlas sync read ...`.
///
/// Separate from `sync-setup`, which is about a cloud provider. This is about
/// the key, and it is deliberately usable when nothing else is: `read` needs
/// a file and a phrase, and touches no daemon, no vault and no config.
pub(super) fn run_sync(cfg: &Config, args: &[String]) {
    use atlas::sync;
    let store = atlas::roots::store();
    let now = atlas::store::now();
    let value = |f: &str| atlas::cli::flag_value(args, f);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("key") => match args.get(1).map(|s| s.to_lowercase()).as_deref() {
            Some("new") => {
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                if kept.is_set() && !args.iter().any(|a| a == "--replace") {
                    println!(
                        "This device already has a household key. `atlas sync key show` \
                         prints it. Making a new one makes every bundle written under the \
                         old one unreadable -- `--replace` if that is what you mean."
                    );
                    return;
                }
                let phrase = sync::new_key_phrase();
                let keeping = sync::KeptKey::keeping(&phrase, now);
                match store.save(sync::KEY_FILE, &keeping) {
                    Ok(()) => {
                        println!("Your household key:");
                        println!();
                        println!("    {phrase}");
                        println!();
                        println!("Write it down now. It is the only thing that opens a sealed");
                        println!("bundle -- on your other devices, and on this one after a");
                        println!("reinstall. I can show it again while this device still works");
                        println!("(`atlas sync key show`), and not after that.");
                        println!();
                        println!("{}", keeping.at_rest_says());
                        println!();
                        println!("On your other devices:  atlas sync key set {phrase}");
                        println!("Then turn it on:        sync.encrypt_bundles: true in tools.yaml");
                    }
                    Err(e) => println!("I made a key and couldn't keep it: {e}"),
                }
            }
            Some("set") => {
                let typed: String = args[2..].join(" ");
                let typed = value("--key").map(|k| k.to_string()).unwrap_or(typed);
                if typed.trim().is_empty() {
                    println!("atlas sync key set <phrase>   -- the phrase from `atlas sync key new`");
                    return;
                }
                // Checked before it is kept, and an existing key kept unless
                // `--replace` -- in `sync::set_key`, shared with the Sync page.
                let replace = args.iter().any(|a| a == "--replace");
                let typed = typed.replace("--replace", "");
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                if kept.is_set() && !replace && kept.phrase().ok().as_deref().map(str::trim) != Some(typed.trim()) {
                    println!(
                        "This device already has a household key. Using another one makes every \
                         bundle sealed under the old one unreadable here -- `--replace` if that is \
                         what you mean."
                    );
                    return;
                }
                match sync::set_key(&store, &typed, replace, now) {
                    Ok(said) => println!("{said}"),
                    Err(why) => println!("{why}"),
                }
            }
            Some("show") => {
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                match kept.phrase() {
                    Ok(p) => {
                        println!("{p}");
                        println!();
                        println!("{}", kept.at_rest_says());
                    }
                    Err(why) => println!("{why}"),
                }
            }
            Some("forget") => {
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                if !kept.is_set() {
                    println!("There is no household key on this device.");
                    return;
                }
                if !args.iter().any(|a| a == "--yes") {
                    println!(
                        "That leaves this device unable to open any sealed bundle, and \
                         unable to write one. If the phrase is written down you can put it \
                         back with `atlas sync key set`. Add --yes if you mean it."
                    );
                    return;
                }
                match store.save(sync::KEY_FILE, &sync::KeptKey::default()) {
                    Ok(()) => println!("Forgotten. Sealed bundles here are now closed to me."),
                    Err(e) => println!("I couldn't forget it: {e}"),
                }
            }
            _ => {
                let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                let on = cfg
                    .tools
                    .as_ref()
                    .map(|t| t.sync.encrypt_bundles)
                    .unwrap_or(false);
                println!("Sealing is {}.", if on { "on" } else { "off" });
                println!(
                    "This device {}.",
                    if kept.is_set() { "has a household key" } else { "has no household key" }
                );
                if kept.is_set() {
                    println!("{}", kept.at_rest_says());
                }
                println!();
                println!("  atlas sync key new             make one, and print it once");
                println!("  atlas sync key set <phrase>    use the same key as another device");
                println!("  atlas sync key show            print the phrase this device holds");
                println!("  atlas sync key forget --yes    remove it from this device");
                println!("  atlas sync read <file>         open a bundle by hand");
            }
        },

        Some("read") => {
            let Some(path) = args.get(1).filter(|p| !p.starts_with("--")) else {
                println!("atlas sync read <file.bundle> [--card <recovery card file>]");
                println!("                            [--key <phrase>]");
                return;
            };
            let raw = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    println!("I couldn't read {path}: {e}");
                    return;
                }
            };
            // Three ways to have the key, in the order that asks least of
            // you. A recovery card is a file you can copy anywhere, which is
            // the answer to "I will lose a piece of paper": `--card` takes
            // the file rather than making you read the phrase off it and
            // type it.
            let from_card = |path: &str| -> Option<String> {
                std::fs::read_to_string(path).ok().and_then(|t| sync::phrase_in_card(&t))
            };
            let phrase = value("--key")
                .map(|k| k.to_string())
                .or_else(|| value("--card").and_then(&from_card))
                .or_else(|| {
                    let kept: sync::KeptKey = store.load(sync::KEY_FILE);
                    kept.is_set().then(|| kept.phrase().unwrap_or_default())
                })
                // The card this machine wrote, if it is still where it was
                // put. Last, so an explicit key always wins.
                .or_else(|| from_card(&sync::card_path().display().to_string()));
            let key = match phrase.as_deref().map(sync::key_from_phrase) {
                Some(Ok(k)) => Some(k),
                Some(Err(why)) => {
                    println!("{why}");
                    return;
                }
                None => None,
            };
            if let Some(env) = sync::peek(&raw) {
                println!("Sealed bundle from {} ({}), written at {}.",
                    env.from_device, env.belongs_to, env.made_at);
                println!();
            }
            match sync::read_bundle_for(&raw, key.as_deref(), sync::Reader::Command) {
                Ok(b) => {
                    println!(
                        "{} thing{} from {} ({}).",
                        b.events.len(),
                        if b.events.len() == 1 { "" } else { "s" },
                        b.from_device,
                        b.from_name
                    );
                    println!();
                    match serde_json::to_string_pretty(&b) {
                        Ok(text) => println!("{text}"),
                        Err(e) => println!("(I opened it and couldn't print it: {e})"),
                    }
                }
                Err(why) => println!("{why}"),
            }
        }

        _ => {
            println!("atlas sync key    -- the household key sealed bundles use");
            println!("atlas sync read   -- open a bundle by hand");
            println!();
            println!("Carrying things between devices is `sync` spoken to Atlas, or the");
            println!("daemon doing it by itself. This command is the key and the reader.");
        }
    }
}

pub(super) fn run_sync_setup(cfg: &Config, args: &[String]) -> bool {
    use atlas::cloudsync::{self, Provider};

    // `atlas sync-setup compare` — the providers side by side. `compare` is
    // not a provider name, so it is handled before the provider parse.
    if args.first().map(|a| a.eq_ignore_ascii_case("compare")).unwrap_or(false) {
        println!("{}", atlas::cloudsync::compare_providers());
        return true;
    }

    let ccfg = cfg.tools.as_ref().map(|t| t.cloud.clone()).unwrap_or_default();

    // `cloud.provider` ships `onedrive` and was read by nothing: this took
    // the provider from the command line or gave up, so `atlas sync-setup`
    // with nothing after it opened the hub and the line in your file decided
    // nothing at all. Named on the command line still wins -- setting up a
    // second provider once should not mean editing the file first.
    let name = match args.first() {
        Some(a) => a.clone(),
        None => ccfg.provider.clone(),
    };
    let p = match name.trim().to_ascii_lowercase().replace(['-', '_'], "").as_str() {
        "onedrive" => Provider::OneDrive,
        "icloud" | "iclouddrive" => Provider::ICloud,
        "dropbox" => Provider::Dropbox,
        "googledrive" | "gdrive" | "google" => Provider::GoogleDrive,
        "folder" | "whatever" | "any" => Provider::Whatever,
        _ => return false,
    };

    // Per-provider setup guidance, reading the real on_windows/on_ios/hint
    // notes: `atlas sync-setup <provider>` (laptop) or `... <provider> phone`.
    let device = if args.iter().any(|a| a.eq_ignore_ascii_case("phone")) {
        atlas::cloudsync::Setup::Phone
    } else {
        atlas::cloudsync::Setup::Laptop
    };
    println!("{}", atlas::cloudsync::setup_guidance(p, device));

    let already = args.iter().any(|a| a == "--installed");

    // Whether the folder is already known is a fact on disk, not a guess: an
    // empty `cloud.folder` means setup has never found one.
    let found_at = if ccfg.folder.trim().is_empty() {
        None
    } else if std::path::Path::new(&ccfg.folder).exists() {
        Some(ccfg.folder.clone())
    } else {
        None
    };
    println!("{}", cloudsync::setting_up(p, found_at.as_deref()));
    println!();
    // Said at set-up rather than buried, because this is the moment the
    // decision is being made. The sentence it replaces claimed the opposite.
    println!("{}", cloudsync::ABOUT_BUNDLES);
    println!();

    // The size question, answered from the settings that actually drive it.
    // `days_kept` comes from the working set's own retention because
    // `cloud.carry_files` is what routes those files here — the two settings
    // are the same decision seen from two sides, and taking the number from
    // anywhere else would make the estimate about a different system.
    let days_kept = cfg
        .tools
        .as_ref()
        .map(|t| t.working_set.keep_days)
        .unwrap_or(14);
    let events_per_day = 200;
    let needed = cloudsync::space_needed_mb(events_per_day, days_kept, ccfg.carry_files);
    let (fits, why) = cloudsync::free_tier_is_enough(p, needed);
    println!(
        "Space: about {needed}MB for {days_kept} days of history{}. {}{}",
        if ccfg.carry_files { ", carrying files" } else { "" },
        if fits { "" } else { "Doesn't fit: " },
        why
    );
    println!();

    println!("On the laptop:");
    for s in cloudsync::laptop_steps(p, already) {
        match &s.why_you {
            None => println!("  Atlas does it: {}", s.what),
            Some(why) => println!("  You do it: {} — {why}", s.what),
        }
    }
    println!();
    println!("On the phone:");
    for s in cloudsync::phone_steps(p) {
        match &s.why_you {
            None => println!("  Atlas does it: {}", s.what),
            Some(why) => println!("  You do it: {} — {why}", s.what),
        }
    }
    println!();
    println!("If Atlas can't find the folder, look in:");
    for place in cloudsync::where_to_look(p) {
        println!("  {place}");
    }
    true
}
