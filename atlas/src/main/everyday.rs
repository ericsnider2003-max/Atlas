//! The everyday commands: files, pictures and the screen, shared and client
//! lists, notes, documents, the calendar, tasks, the household, the index,
//! backups, mail, watching, being away, the catalogue and the budget.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

/// Suggest where loose files should go, and move them only if told to.
///
/// Two steps, like `reclaim`: plain shows what it would do, `--do-it` acts.
/// Every move goes through `system::judge` — the roots check, the master
/// switch, the reversibility rules — so filing cannot reach anywhere Atlas is
/// not already permitted to work.
pub(super) fn run_file(cfg: &Config, go: bool) {
    let Some(tc) = cfg.tools.as_ref() else {
        eprintln!("no config/tools.yaml, so I don't know which folders I may work in.");
        return;
    };
    let sys = tc.system.clone();
    let Some(home) = atlas::doctor::lookup_env("USERPROFILE")
        .or_else(|| atlas::doctor::lookup_env("HOME"))
    else {
        eprintln!("I couldn't work out where your home folder is.");
        return;
    };
    let root = std::path::PathBuf::from(&home).join("Filed");
    let now = atlas::store::now();

    // Loose files in the folders things land in. Not a whole-disk sweep:
    // filing everywhere would move things you deliberately put somewhere.
    let mut moves = 0usize;
    let mut left = 0usize;
    let mut planned: Vec<(std::path::PathBuf, atlas::filing::Suggestion)> = Vec::new();
    for folder in ["Downloads", "Desktop", "Documents"] {
        let dir = std::path::PathBuf::from(&home).join(folder);
        // The same reading "tidy my desktop" makes (`filing::plan_folder`).
        for (path, s) in atlas::filing::plan_folder(&dir, &root, now) {
            match &s {
                atlas::filing::Suggestion::Move { .. } => moves += 1,
                atlas::filing::Suggestion::Leave { .. } => left += 1,
            }
            println!("  {}", s.line(&path));
            planned.push((path, s));
        }
    }
    println!();
    println!("{}", atlas::filing::spoken(moves, left));

    if !go {
        println!();
        println!("Nothing has been moved. `atlas file --do-it` files these.");
        return;
    }

    // No trash here, and that is the fix rather than an omission.
    //
    // ## What this did until 17 Sep 2026
    //
    // The `Verdict::Go` arm created the destination's parent directory and
    // then called `trash.take(from, "filed")`. **Nothing ever moved the file
    // to `to`.** Every file the walker picked from Downloads, Desktop **and
    // Documents** ended up in `data/trash/<id>-<name>`, and stdout said
    // `filed <path>`.
    //
    // Two things made that worse than a no-op. The trashed files then sat
    // inside `data/`, where the hourly retention pass surveys — and
    // `retention::classify` matched on extensions, so a filed `.png` was
    // `Captures` with a 24-hour limit and a filed `.wav` was `Scratch` with a
    // ten-minute one, deleted permanently with `remove_file` while the trash
    // ledger still listed them. So `atlas file --do-it` said "filed" and then
    // destroyed the person's documents within a day. Both halves are fixed;
    // this is the half that moves the file.
    //
    // The comment that used to be here said the trash made "a wrong home
    // recoverable rather than a hunt", which is a sound instinct about the
    // wrong operation: the trash is for removing something, and a misfiled
    // file is recoverable because the line below says where it went.
    //
    // The move itself is `filing::file_one` (29 Sep 2026), shared with "tidy
    // my desktop": judged every time, never over a file already there, and a
    // cross-drive copy removes the original only once it is whole.
    for (from, s) in &planned {
        if atlas::filing::as_change(from, s).is_none() {
            continue;
        }
        match atlas::filing::file_one(from, s, &sys) {
            Ok(to) => println!("  filed {} -> {}", from.display(), to.display()),
            Err(why) => println!("  skipped {}: {why}", from.display()),
        }
    }
}

/// Read the words in a picture, or off the screen, in Atlas's own code.
///
/// The one command in this round that can be run today with nothing installed
/// but the two model files — no tesseract, no account, no second program.
/// `atlas picture <file> [question]` or `atlas picture screen [question]`:
/// ask the picture reader about a picture, the same reader "look at my
/// screen" uses — for checking it on this machine once `atlas get pictures`
/// has fetched it. A screenshot taken here is deleted once it's been read.
pub(super) fn run_picture(cfg: &Config, args: &[String]) {
    let tools = cfg.tools.clone().unwrap_or_default();
    let root = atlas::roots::store().install_root();
    if let Err(why) = atlas::picture_talk::ready(&tools.picture_talk, &root) {
        println!("I can't read pictures yet: {why}.");
        return;
    }
    let Some(what) = args.first() else {
        println!("atlas picture <file> [question]   or   atlas picture screen [question]");
        return;
    };
    let said = args[1..].join(" ");
    let question = atlas::picture_talk::question_for(&said);
    let (image, taken) = if what == "screen" {
        let Some(tool) = tools.capture_screen.clone() else {
            println!("There's no screen capture set up on this machine.");
            return;
        };
        let dir = std::path::PathBuf::from(&tools.work_dir);
        let _ = std::fs::create_dir_all(&dir);
        let shot = dir.join(format!("screen_{}.png", atlas::store::now()));
        let mut vars = tools.vars.clone();
        vars.insert("out_png".into(), shot.display().to_string());
        if let Err(e) = tool.run(&vars, None) {
            let _ = std::fs::remove_file(&shot);
            println!("I couldn't take the picture: {e}");
            return;
        }
        (shot, true)
    } else {
        (std::path::PathBuf::from(what), false)
    };
    let small = atlas::picture_talk::smaller(&image);
    let started = std::time::Instant::now();
    let answer = atlas::picture_talk::ask_until(
        &tools.picture_talk,
        &root,
        small.as_deref().unwrap_or(&image),
        &question,
        &|| false,
    );
    if let Some(p) = &small {
        let _ = std::fs::remove_file(p);
    }
    if taken {
        let _ = std::fs::remove_file(&image);
    }
    match answer {
        Ok(a) => println!("{a}"),
        Err(e) => println!("I couldn't read it: {e}"),
    }
    println!("({:.0} s, asked: {question})", started.elapsed().as_secs_f32());
}

pub(super) fn run_screen(cfg: &Config, args: &[String]) {
    let tools = cfg.tools.clone().unwrap_or_default();
    let models = std::path::PathBuf::from(&tools.models.dir);
    if !atlas::words::Reader::installed(&models) {
        let missing = atlas::infer::whats_missing(&models, &atlas::infer::Kind::for_reading());
        println!("{}", atlas::infer::spoken(&missing));
        println!();
        println!("Menu item 8 in ATLAS.bat fetches them. About 36MB, once.");
        return;
    }
    let mut reader = match atlas::words::Reader::open(&models) {
        Ok(r) => r,
        Err(e) => {
            println!("{e}");
            return;
        }
    };
    let wcfg = tools.words;

    let read = match args.first().map(|s| s.as_str()) {
        Some("grab") => {
            // atlas screen grab <x> <y> <w> <h>
            let nums: Vec<i64> = args[1..].iter().filter_map(|a| a.parse().ok()).collect();
            if nums.len() != 4 || nums[2] <= 0 || nums[3] <= 0 {
                println!("atlas screen grab <x> <y> <width> <height>");
                println!("A rectangle, not the whole desktop: reading 2560x1392 finds a great");
                println!("deal you didn't mean and takes ten times as long.");
                return;
            }
            let (w, h) = (nums[2] as usize, nums[3] as usize);
            let grab =
                atlas::words::capture_args(nums[0] as i32, nums[1] as i32, w as u32, h as u32);
            match atlas::words::pixels_from(&tools.video.ffmpeg, &tools.vars, &grab)
                .and_then(|px| {
                    atlas::words::whole_picture(px.len(), w, h)?;
                    reader.look(&px, w, h, &wcfg)
                }) {
                Ok(r) => r,
                Err(e) => {
                    println!("{e}");
                    return;
                }
            }
        }
        Some(path) => {
            match atlas::words::read_file(&mut reader, &tools.video.ffmpeg, &tools.vars, path, &wcfg)
            {
                Ok(r) => r,
                Err(e) => {
                    println!("{e}");
                    return;
                }
            }
        }
        None => {
            println!("atlas screen <picture file>");
            println!("atlas screen grab <x> <y> <width> <height>");
            return;
        }
    };

    println!("{}", read.spoken());
    if read.words() > 0 {
        println!();
        println!("{}", read.text());
    }
    if !read.worth_acting_on() && read.words() > 0 {
        println!();
        println!("I'd not act on that. {}", atlas::words::NO_CAPITALS);
    }
}

/// The line between your own work and a business you share.
///
/// Four things, and `check` is the important one: it answers "would this be
/// allowed?" without anything actually crossing. A boundary nobody can
/// interrogate is a boundary nobody has reason to trust, and the first time
/// you find out what it does should not be the time it matters.
pub(super) fn run_shared(args: &[String]) {
    // The same state folder everything else in Atlas uses.
    let store = atlas::roots::store();
    let mut wall = atlas::firewall::Firewall::load(&store);
    let now = atlas::store::now();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            println!("{}", wall.spoken());
            for h in wall.waiting() {
                println!("  {}  {}  -> {}  ({})", h.id, h.what, h.into, h.why);
            }
            println!();
            println!("atlas shared release <number>   let that one through");
            println!("atlas shared drop <number>      it should never have been going");
            println!("atlas shared check <business> <name>   would this be allowed?");
        }
        Some("release") => match args.get(1).and_then(|n| n.parse::<u64>().ok()) {
            Some(id) => match wall.release(id) {
                Ok(said) | Err(said) => {
                    println!("{said}");
                    keep(wall.save(&store), "the shared wall");
                }
            },
            None => println!("Which one? atlas shared release 3"),
        },
        Some("drop") => match args.get(1).and_then(|n| n.parse::<u64>().ok()) {
            Some(id) => match wall.forget(id) {
                Ok(said) | Err(said) => {
                    println!("{said}");
                    keep(wall.save(&store), "the shared wall");
                }
            },
            None => println!("Which one? atlas shared drop 3"),
        },
        Some("check") => {
            let (Some(into), Some(what)) = (args.get(1), args.get(2)) else {
                println!("atlas shared check <business> <name of the thing>");
                return;
            };
            // Asked as your own work, which is the case worth checking. A
            // business's own material was never going to be stopped.
            //
            // `would_stop`, not `check`. `check` is the enforcement path: it
            // takes `&mut self`, allocates an id and pushes a `Held`. This
            // subcommand called it, printed "Nothing has actually moved", and
            // then SAVED the wall — so every invocation of a command named
            // *check* created a real hold on disk, and asking the same
            // question three times left three entries in `atlas shared list`.
            match wall.would_stop(&atlas::earned::Space::Personal, into) {
                None => println!("That would go through."),
                Some(why) => {
                    println!("That would stop here — {why}.");
                    println!("Nothing has moved and nothing is held — this is just the answer.");
                    // The third leg, not just the first two: block and pause
                    // are the lines above, and this is what "notify" actually
                    // says. The text is real rather than a stub; what it does
                    // not have behind it is a hold, because nothing crossed.
                    let n = wall.would_say(into, what, &why, now);
                    println!("If it did cross, I'd say: \"{}\" — {}", n.title, n.body);
                }
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try list, release, drop or check."),
    }
}

/// One list, `earned::Space`-generic -- your own tasks and every business's
/// shelf are the same store. See `shared_task.rs`'s own doc for why this is
/// one type rather than two.
/// `atlas clients` — who your clients are, and bringing them in from a
/// phone's contacts or a business card (`.vcf`).
pub(super) fn run_clients(args: &[String]) {
    let store = atlas::roots::store();
    let mut list = atlas::clients::ClientList::load(&store);
    let now = atlas::store::now();
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            println!("Your clients ({}):", list.len());
            for c in list.all() {
                println!("  {} <{}>{}", c.name_or_address(), c.address, if c.phone.is_empty() { String::new() } else { format!("  {}", c.phone) });
            }
            println!();
            println!("atlas clients add <address> [name]     add one");
            println!("atlas clients import <file.vcf>        bring them in from contacts");
            println!("atlas clients export <file.vcf>        write them out for your phone");
            println!("atlas clients dupes                    who looks like the same person twice");
        }
        Some("add") => match args.get(1) {
            Some(address) if address.contains('@') => {
                let name = args[2..].join(" ");
                list.add(address, &name, "", now);
                keep(list.save(&store), "your clients");
                println!("Added {address}.");
                for d in list.likely_duplicates().iter().filter(|d| d.contains(&address.to_lowercase())) {
                    println!("  Worth a look: {d}");
                }
            }
            _ => println!("atlas clients add <address> [name]"),
        },
        Some("import") => match args.get(1).map(|p| std::fs::read_to_string(p)) {
            Some(Ok(text)) => match list.import_vcf(&text, now) {
                Ok((added, skipped, notes)) => {
                    keep(list.save(&store), "your clients");
                    println!("Brought in {added} client{}.", if added == 1 { "" } else { "s" });
                    if skipped > 0 {
                        println!("Skipped {skipped} card{} with no email address — a client is recognised by address.", if skipped == 1 { "" } else { "s" });
                    }
                    for n in notes {
                        println!("  {n}");
                    }
                }
                Err(e) => println!("That file didn't read as contacts: {e}"),
            },
            Some(Err(e)) => println!("I couldn't open that file: {e}"),
            None => println!("atlas clients import <file.vcf>"),
        },
        Some("export") => match args.get(1) {
            Some(path) => match std::fs::write(path, list.to_vcf()) {
                Ok(()) => println!("Wrote {} client{} to {path}.", list.len(), if list.len() == 1 { "" } else { "s" }),
                Err(e) => println!("I couldn't write {path}: {e}"),
            },
            None => println!("atlas clients export <file.vcf>"),
        },
        Some("dupes") | Some("duplicates") => {
            let d = list.likely_duplicates();
            if d.is_empty() {
                println!("Nobody on the list looks like the same person twice.");
            } else {
                println!("These look like one person under two entries (nothing has been merged):");
                println!("({})", list.weights_note());
                for line in d {
                    println!("  {line}");
                }
            }
        }
        Some(other) => println!("I don't know 'clients {other}'. Try: atlas clients"),
    }
}

/// `atlas my-key` / `seal-file` / `open-file` — files only one person can
/// open (`agefile`, the age v1 format, which the real `age` tool reads too).
/// Your own key lives in the vault; the `age1…` line you give people is kept
/// in the clear beside it, because it is meant to be handed out.
pub(super) fn run_agefile(words: &[String]) {
    let store = atlas::roots::store();
    let state = atlas::roots::install_state();
    let now = atlas::store::now();
    let vcfg = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).map(|t| t.vault).unwrap_or_default();
    const NAME: &str = "age identity";
    let unlock = |vault: &mut atlas::vault::Vault| -> bool {
        if !vault.has_a_passphrase() {
            println!("Your key is kept in the vault, and the vault has no passphrase yet — `atlas vault passphrase` first.");
            return false;
        }
        let Some(phrase) = ask_quietly("Vault passphrase: ") else { return false };
        match vault.open(&phrase, now, &vcfg) {
            Ok(()) => true,
            Err(why) => {
                println!("{why}");
                false
            }
        }
    };
    match words.first().map(|s| s.as_str()) {
        Some("my-key") => {
            let known: String = store.load("age-recipient");
            if !known.is_empty() {
                println!("{known}");
                println!("Give that line to anyone who should be able to send you files only you can open.");
                return;
            }
            let mut vault = atlas::vault::Vault::load(&state);
            if !unlock(&mut vault) {
                return;
            }
            // Already one in the vault (the public half was lost, say): read
            // it back rather than making a second key nobody has.
            if let Ok(identity) = vault.get(NAME, now) {
                vault.lock();
                return match atlas::agefile::recipient_of(&identity) {
                    Ok(r) => {
                        keep(store.save("age-recipient", &r), "your public key");
                        println!("{r}");
                    }
                    Err(why) => println!("The key in your vault didn't read: {why}"),
                };
            }
            let (identity, recipient) = atlas::agefile::new_identity();
            if let Err(why) = vault.put(NAME, atlas::vault::Kind::Note, &identity, now) {
                return println!("I couldn't keep the key: {why}");
            }
            if keep(vault.save(&state), "the vault") && keep(store.save("age-recipient", &recipient), "your public key") {
                println!("{recipient}");
                println!("Made you a key. That line is the public half — give it out freely. The secret half is in your vault.");
            }
            vault.lock();
        }
        Some("seal-file") => {
            // atlas seal-file <file> to <age1…> [age1…]
            let (Some(path), rest) = (words.get(1), &words[2.min(words.len())..]) else {
                return println!("atlas seal-file <file> to <age1… key> [more keys]");
            };
            let mut to: Vec<String> = rest.iter().filter(|w| *w != "to" && *w != "and").cloned().collect();
            let mine: String = store.load("age-recipient");
            if !mine.is_empty() && !to.contains(&mine) {
                to.push(mine); // so you can open what you sent
            }
            let plain = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => return println!("I couldn't read {path}: {e}"),
            };
            match atlas::agefile::seal(&plain, &to) {
                Ok(sealed) => {
                    let out = format!("{path}.age");
                    match std::fs::write(&out, sealed) {
                        Ok(()) => println!("Sealed to {} key{}: {out}. Only those keys open it — with Atlas, or with `age -d`.", to.len(), if to.len() == 1 { "" } else { "s" }),
                        Err(e) => println!("I couldn't write {out}: {e}"),
                    }
                }
                Err(why) => println!("{why}"),
            }
        }
        _ => {
            let Some(path) = words.get(1) else {
                return println!("atlas open-file <file.age>");
            };
            let sealed = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => return println!("I couldn't read {path}: {e}"),
            };
            let mut vault = atlas::vault::Vault::load(&state);
            if !unlock(&mut vault) {
                return;
            }
            let identity = match vault.get(NAME, now) {
                Ok(i) => i,
                Err(_) => return println!("There's no key of yours in the vault yet — `atlas my-key` makes one."),
            };
            vault.lock();
            match atlas::agefile::open(&sealed, &identity) {
                Ok(plain) => {
                    let out = path.strip_suffix(".age").map(String::from).unwrap_or_else(|| format!("{path}.opened"));
                    let out = if std::path::Path::new(&out).exists() { format!("{out}.opened") } else { out };
                    match std::fs::write(&out, plain) {
                        Ok(()) => println!("Opened: {out}"),
                        Err(e) => println!("I couldn't write {out}: {e}"),
                    }
                }
                Err(why) => println!("I couldn't open it: {why}."),
            }
        }
    }
}

/// `atlas notes <recording.wav>` — who said what (`vad` finds the speech,
/// `diarize` groups it by voice, your enrolled voiceprint names you). Uses
/// the same speaker encoder and speech-to-text tools as the voice loop; says
/// which one is missing rather than pretending.
pub(super) fn run_notes(cfg: &Config, args: &[String], merge_voices: bool, people: Option<usize>) {
    let Some(path) = args.first() else {
        return println!("atlas notes <recording.wav> [--people N] [--merge-voices]");
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return println!("I couldn't open {path}: {e}"),
    };
    let (samples, rate) = match atlas::diarize::read_wav(&bytes) {
        Ok(x) => x,
        Err(e) => return println!("{path}: {e}"),
    };
    let tc = cfg.tools.clone().unwrap_or_default();
    let mut vars = tc.vars.clone();
    // Removed when this command ends (`RunScratch`).
    let scratch = atlas::roots::RunScratch::new("atlas-notes");
    let work = scratch.path().to_path_buf();
    let seg_wav = work.join("segment.wav");
    vars.insert("in_wav".into(), seg_wav.display().to_string());
    vars.insert("work_dir".into(), work.display().to_string());
    // An installed encoder is used when there is one. Otherwise Atlas's own
    // (`speaker::recording_embeddings`): supervectors against the machine's
    // background model, or the recording's own when there's none yet,
    // centred on the recording.
    let external = atlas::speaker::which(&tc.speaker, &vars) == atlas::speaker::Encoder::External;
    let own: Vec<Option<Vec<f32>>> = if external {
        Vec::new()
    } else {
        atlas::speaker::recording_embeddings(&samples, rate, &atlas::speaker::background(&atlas::roots::store()))
            .into_iter()
            .map(|(_, e)| e)
            .collect()
    };
    let can_embed = external || own.iter().flatten().count() >= 2;
    let can_hear = tc.stt.available(&vars);
    if !can_embed {
        println!("(Too little speech in this to tell voices apart — every line is \"Someone\".)");
    } else if !external {
        println!("(Voices told apart by Atlas's own encoder — a classical one, weaker than a trained model.)");
    }
    if !can_hear {
        println!("(Speech-to-text isn't installed, so these are the times people spoke, without the words.)");
    }
    let write = |s: &[i16]| std::fs::write(&seg_wav, atlas::audio::wav_bytes(s, rate)).is_ok();
    // Called once per stretch of speech, in order — the same order
    // `recording_embeddings` produced them in.
    let mut next = own.into_iter();
    let mut embed = |s: &[i16]| -> Option<Vec<f32>> {
        if external {
            (write(s)).then(|| atlas::speaker::embed(&tc.speaker, &vars).ok()).flatten()
        } else {
            next.next().flatten()
        }
    };
    let mut hear = |s: &[i16]| -> Option<String> {
        (can_hear && write(s)).then(|| tc.stt.run(&vars, None).ok().map(|raw| atlas::voice::clean_transcript(&raw))).flatten()
    };
    // Your print is only comparable when it came from the same encoder
    // against the same background; a per-recording background is not the
    // machine's, so with the built-in encoder "You" is not claimed here.
    let you = if external { atlas::voiceid::VoiceId::load(&atlas::roots::store()).print.map(|p| p.centroid) } else { None };
    let lines = if external {
        atlas::diarize::who_said_what(&samples, rate, &mut embed, &mut hear, you.as_deref(), tc.voice_id.accept)
    } else {
        atlas::diarize::who_said_what_grouped(&samples, rate, &mut embed, &mut hear, None, tc.voice_id.builtin_accept, atlas::speaker::GROUP_CENTRED_AT)
    };
    // A second look on each speaker's pooled speech (`diarize::
    // merge_same_voices`), only when asked for: measured on sixty calls it
    // puts a split voice back together, and on calls it wasn't tuned on it
    // also put two people under one name once in twenty. Splitting one
    // person is the safer mistake for notes, so it stays opt-in.
    let lines = if merge_voices {
        atlas::diarize::merge_same_voices(&samples, rate, lines, atlas::diarize::SAME_VOICE_LAMBDA)
    } else {
        lines
    };
    // Someone who spoke once, swallowed by the nearest voice, is given back a
    // name of their own (`diarize::split_strangers`) — on by default, because
    // on calls it wasn't tuned on it mixed fewer people and split no more.
    let lines = atlas::diarize::split_strangers(&samples, rate, lines, atlas::diarize::STRANGER_MARGIN);
    // Told how many people there were: that settles it (`diarize::to_count`).
    let lines = match people {
        Some(n) => atlas::diarize::to_count(&samples, rate, lines, n),
        None => lines,
    };
    let _ = std::fs::remove_dir_all(&work);
    if lines.is_empty() {
        return println!("I didn't find any speech in {path}.");
    }
    for l in &lines {
        println!("{}", l.say());
    }
}

/// `atlas doc` — a page both of your machines can edit while apart, that
/// comes back together without a clash to settle (`yata`). The edits ride in
/// the sync log, so the next `atlas sync` carries them.
pub(super) fn run_doc(args: &[String]) {
    let store = atlas::roots::store();
    // Read, never written from here: the running Atlas is the sync log's one
    // writer. Edits go to its inbox (`yata::queue`) and it takes them in on
    // its next tick.
    let log: atlas::sync::Log = store.load("synclog");
    let site = if log.device.trim().is_empty() {
        let named = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).map(|t| t.sync.name).unwrap_or_default();
        if named.trim().is_empty() { "this device".to_string() } else { named.trim().to_string() }
    } else {
        log.device.clone()
    };
    let inbox = store.data_dir().join("doc-inbox");
    match (args.first().map(|s| s.as_str()), args.get(1)) {
        (Some("show"), Some(name)) => {
            let d = atlas::yata::current(name, &site, &log.events, &inbox);
            print!("{}", d.text());
            if d.waiting() > 0 {
                println!("\n({} edits are waiting on others that haven't arrived yet.)", d.waiting());
            }
        }
        (Some("set"), Some(name)) | (Some("add"), Some(name)) => {
            let adding = args[0] == "add";
            let new_text = match args.get(2) {
                Some(path) if std::path::Path::new(path).is_file() => match std::fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(e) => return println!("I couldn't read {path}: {e}"),
                },
                _ => args[2..].join(" "),
            };
            let mut d = atlas::yata::current(name, &site, &log.events, &inbox);
            let target = if adding {
                let now_text = d.text();
                let sep = if now_text.is_empty() || now_text.ends_with('\n') { "" } else { "\n" };
                format!("{now_text}{sep}{}\n", new_text.trim_end())
            } else {
                new_text
            };
            let before = d.text();
            let ops = d.set_text(&target);
            if ops.is_empty() {
                return println!("No change.");
            }
            match atlas::yata::queue(&inbox, name, &ops) {
                Ok(()) => println!(
                    "Saved \"{name}\" ({} character edits, {} lines changed). It goes to your other machines with the next sync.",
                    ops.len(),
                    atlas::diff::lines_changed(&before, &d.text())
                ),
                Err(e) => return println!("I couldn't save that: {e}"),
            }
            // No Atlas running to take the inbox in: take it in here, under
            // the same one-at-a-time lock the daemon holds, so there is still
            // only ever one writer of the sync log.
            let now = atlas::store::now();
            let lock = atlas::onlyone::OnlyOne::at(&store.data_dir());
            if lock.take(now).is_ok() {
                let mut log: atlas::sync::Log = store.load("synclog");
                if log.device.trim().is_empty() {
                    log = atlas::sync::Log::new(&site);
                }
                let taken = atlas::yata::take_queued(&inbox, &mut log, now);
                if !taken.is_empty() && store.save("synclog", &Some(log)).is_ok() {
                    for f in taken {
                        let _ = std::fs::remove_file(f);
                    }
                }
                lock.release();
            }
        }
        (Some("list"), _) | (None, _) => {
            let names = atlas::yata::all_names(&log.events, &inbox);
            if names.is_empty() {
                println!("No shared pages yet. atlas doc set <name> <text or file>");
            }
            for n in names {
                let t = atlas::yata::current(&n, &site, &log.events, &inbox).text();
                println!("{n} — {} lines", t.lines().count());
            }
        }
        _ => println!("atlas doc list | show <name> | set <name> <text or file> | add <name> <line>"),
    }
}

/// Your `time_zone` setting as a zone; UTC when unset.
fn your_zone() -> atlas::tz::Zone {
    let set = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).map(|t| t.time_zone).unwrap_or_default();
    atlas::tz::home(&set)
}

/// `atlas calendar import|export` — `.ics`, the file every other calendar
/// speaks, so an invite from anyone lands here and yours can go anywhere.
pub(super) fn run_calendar(args: &[String]) {
    let store = atlas::roots::store();
    let mut cal = atlas::calendar::Calendar::load(&store);
    let now = atlas::store::now();
    let zone = your_zone();
    if zone.is_utc() {
        if let Some(z) = atlas::tz::suggest() {
            println!("(No time zone is set, so times are shown in UTC. This computer says {} — pick it under Settings → Time zone.)", z.name);
        }
    }
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("import") => match args.get(1).map(|p| std::fs::read_to_string(p)) {
            Some(Ok(text)) => match cal.import_ics(&text, now, &zone) {
                Ok((n, unknown)) => {
                    keep(cal.save(&store), "your calendar");
                    println!("{n} event{} added or updated.", if n == 1 { "" } else { "s" });
                    for id in &unknown {
                        println!("  (The file names a time zone I don't know, \"{id}\"; those times were read as {}.)", zone.name);
                    }
                    for e in cal.occurrences_between(now, now + 14 * 86_400).iter().take(5) {
                        println!("  {} — {}", e.title, e.say_when_in(&zone));
                    }
                }
                Err(e) => println!("That file didn't read as a calendar: {e}"),
            },
            Some(Err(e)) => println!("I couldn't open that file: {e}"),
            None => println!("atlas calendar import <file.ics>"),
        },
        Some("export") => match args.get(1) {
            Some(path) => match std::fs::write(path, cal.to_ics(now)) {
                Ok(()) => println!("Wrote {} event{} to {path}.", cal.len(), if cal.len() == 1 { "" } else { "s" }),
                Err(e) => println!("I couldn't write {path}: {e}"),
            },
            None => println!("atlas calendar export <file.ics>"),
        },
        _ => {
            println!("atlas calendar import <file.ics>   bring in an invite or another calendar");
            println!("atlas calendar export <file.ics>   write yours out for any other calendar");
        }
    }
}

pub(super) fn run_tasks(args: &[String]) {
    let store = atlas::roots::store();
    let mut tasks = atlas::shared_task::Tasks::load(&store);
    let now = atlas::store::now();

    let print_task = |t: &atlas::shared_task::Task| {
        let mark = if t.done { "x" } else { " " };
        let shared = if t.shared_from_personal { "  (shared from personal)" } else { "" };
        println!("  [{mark}] {}  {}{shared}", t.id, t.description);
    };

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            println!("Your own tasks, most pressing first:");
            for (t, why) in tasks.in_order(&atlas::earned::Space::Personal, now) {
                print_task(t);
                if !why.is_empty() {
                    println!("        {why}");
                }
            }
            println!();
            println!("atlas tasks add <description>          add one of your own");
            println!("atlas tasks done <id>                   mark it done");
            println!("atlas tasks share <id> <business>       share one into a business's shelf");
            println!("atlas tasks for <business>               show that business's shelf");
            println!("atlas tasks roster <business> <peer>     let a paired Atlas see that business");
            println!("atlas tasks unroster <business> <peer>   take that access away");
        }
        Some("add") => {
            let description = args[1..].join(" ");
            if description.trim().is_empty() {
                println!("atlas tasks add <description>");
                return;
            }
            // "… by Friday", "… due tomorrow at 5": the part after the last
            // " by "/" due " is read with the calendar's own day reader; if
            // it names a day, that is the due date and it comes off the
            // description. Nothing it cannot read becomes a date.
            let low = description.to_ascii_lowercase();
            let cut = [" by ", " due "].iter().filter_map(|k| low.rfind(k).map(|i| (i, k.len()))).max();
            let (text, due) = match cut {
                // Read on your clock and kept as the real moment, like every
                // other time the calendar reads (`resolve_when_in`).
                Some((i, k)) => match atlas::calendar::resolve_when_in(&description[i + k..], now, &atlas::localclock::zone()) {
                    Some(w) => (description[..i].trim().to_string(), Some(w.start)),
                    None => (description.trim().to_string(), None),
                },
                None => (description.trim().to_string(), None),
            };
            let id = tasks.add(atlas::earned::Space::Personal, &text, due, now);
            keep(tasks.save(&store), "your tasks");
            match due {
                Some(d) => println!("Added as {id}, due {}.", atlas::digest::iso_utc(d)),
                None => println!("Added as {id}."),
            }
        }
        Some("done") => match args.get(1).and_then(|n| n.parse::<u64>().ok()) {
            Some(id) if tasks.complete(id) => {
                keep(tasks.save(&store), "your tasks");
                println!("Done.");
            }
            Some(_) => println!("That's already done, or there's no task with that id."),
            None => println!("Which one? atlas tasks done 3"),
        },
        Some("share") => {
            let (Some(id), Some(business)) =
                (args.get(1).and_then(|n| n.parse::<u64>().ok()), args.get(2))
            else {
                println!("atlas tasks share <id> <business>");
                return;
            };
            let mut wall = atlas::firewall::Firewall::load(&store);
            match tasks.share_into_business(id, business, &mut wall, now) {
                atlas::firewall::Crossing::Allowed => {
                    println!("Shared -- that was already {business}'s own.");
                }
                atlas::firewall::Crossing::Stopped { held, why } if held > 0 => {
                    println!("That stops at the line -- {why}.");
                    println!("Held as {held}. Run `atlas shared release {held}` to let it through,");
                    println!("then `atlas tasks release {held}` to actually finish the share.");
                    // The third leg, on the crossing that actually happened.
                    //
                    // `firewall::note` is the "notify" half of block/pause/
                    // notify, and its only caller was `atlas shared check` —
                    // the dry run. So Atlas built the notification for a
                    // crossing that never occurred and built nothing for the
                    // one that did. A real thing stopped at the boundary told
                    // Eric nothing beyond whatever scrollback he happened to
                    // be looking at.
                    if let Some(h) = wall.get(held) {
                        let n = atlas::firewall::note(h);
                        println!("{} — {}", n.title, n.body);
                    }
                }
                atlas::firewall::Crossing::Stopped { why, .. } => println!("Couldn't share that: {why}."),
            }
            keep(tasks.save(&store), "your tasks");
            keep(wall.save(&store), "the shared wall");
        }
        Some("release") => match args.get(1).and_then(|n| n.parse::<u64>().ok()) {
            Some(held_id) => {
                let wall = atlas::firewall::Firewall::load(&store);
                match tasks.complete_release(held_id, &wall, now) {
                    Some(new_id) => {
                        keep(tasks.save(&store), "your tasks");
                        println!("Finished -- now on the business's shelf as {new_id}.");
                    }
                    None => println!(
                        "Nothing to finish -- either that hold was never a task share, or it \
                         hasn't been released yet with `atlas shared release {held_id}`."
                    ),
                }
            }
            None => println!("Which held item? atlas tasks release 3"),
        },
        Some("for") => match args.get(1) {
            Some(business) => {
                println!("{business}'s shelf:");
                for (t, _) in tasks.in_order(&atlas::earned::Space::Business(business.clone()), now) {
                    print_task(t);
                }
            }
            None => println!("atlas tasks for <business>"),
        },
        Some("roster") => {
            let (Some(business), Some(peer)) = (args.get(1), args.get(2)) else {
                println!("atlas tasks roster <business> <peer>");
                return;
            };
            let pairings = atlas::kin::Pairings::load(store.root());
            let mut roster = atlas::roster::Roster::load(&store);
            match roster.add(business, peer, &pairings) {
                Ok(()) => {
                    keep(roster.save(&store), "the roster");
                    println!("{peer} can now see {business}'s shared shelf.");
                }
                Err(e) => println!("Couldn't add them: {}", e.plain()),
            }
        }
        Some("unroster") => {
            let (Some(business), Some(peer)) = (args.get(1), args.get(2)) else {
                println!("atlas tasks unroster <business> <peer>");
                return;
            };
            let mut roster = atlas::roster::Roster::load(&store);
            if roster.remove(business, peer) {
                keep(roster.save(&store), "the roster");
                println!("{peer} can no longer see {business}'s shared shelf.");
            } else {
                println!("{peer} wasn't on {business}'s roster.");
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try list, add, done, share, release, for, roster or unroster."),
    }
}

/// Which Atlas this is, and which of your own devices belong to it. See
/// `household.rs`'s own doc: two installs know nothing about each other
/// unless the same person paired them, device to device, with a code.
pub(super) fn run_household(args: &[String]) {
    let store = atlas::roots::store();
    // The sync folder, for handing the household key to a device being
    // paired. Read here rather than passed in, like `trade_cfgs` and
    // `trading_cfg` do -- `run_household` is reached from a dispatch that
    // has no `&Config` to give it.
    let sync_folder = || -> String {
        Config::load(&atlas::roots::config_dir())
            .ok()
            .and_then(|c| c.tools.map(|t| t.sync.folder))
            .unwrap_or_default()
    };
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            let h = atlas::household::Household::load(&store);
            if !h.is_set() {
                println!("No household yet. atlas household init <name>");
                return;
            }
            println!("{} — {} device(s), made {}", h.name, h.devices.len(), h.made_at);
            for d in &h.devices {
                println!("  {d}");
            }
        }
        Some("proof") => {
            let id_cfg = Config::load(&atlas::roots::config_dir())
                .ok()
                .and_then(|c| c.tools.map(|t| t.identity))
                .unwrap_or_default();
            let identity = atlas::identity::Identity::load(&store);
            match atlas::identity::grace_remaining(&identity, &id_cfg, atlas::store::now()) {
                Some(secs) => println!("Proven for another {} minutes.", secs / 60),
                None => println!("Not currently proven — the next watched action will ask."),
            }
        }
        Some("init") => {
            let h = atlas::household::Household::load(&store);
            if h.is_set() {
                println!(
                    "Already set up as {}. There's no re-init — a second household id for the \
                     same devices is exactly the confusion this file exists to prevent.",
                    h.name
                );
                return;
            }
            let name = args[1..].join(" ");
            if name.trim().is_empty() {
                println!("atlas household init <name>");
                return;
            }
            match atlas::household::init(&store, &name, "", atlas::store::now()) {
                Ok(_) => {
                    println!("{}", atlas::household::THEIRS_IS_THEIRS);
                    println!("\n{}", atlas::household::SEPARATE_BY_DEFAULT);
                }
                Err(why) => println!("{why}"),
            }
        }
        Some("pair") => {
            let h = atlas::household::Household::load(&store);
            if !h.is_set() {
                println!("Set up a household first: atlas household init <name>");
                return;
            }
            let device = args[1..].join(" ");
            if device.trim().is_empty() {
                println!("atlas household pair <name for the new device>");
                return;
            }
            let now = atlas::store::now();
            let folder = sync_folder();

            // The short way, when both machines can see the same folder:
            // everything bulky goes in the folder sealed, and the person
            // types ten characters. The household key goes with it, so
            // joining and being able to read sealed bundles are one act.
            if !folder.trim().is_empty() {
                let short = atlas::household::new_invite_code();
                let kept: atlas::sync::KeptKey = store.load(atlas::sync::KEY_FILE);
                let phrase = kept.is_set().then(|| kept.phrase().ok()).flatten();
                match atlas::household::leave_invitation(
                    std::path::Path::new(folder.trim()),
                    &h.id,
                    &h.name,
                    &short,
                    phrase.as_deref(),
                    now,
                    atlas::household::INVITE_WAIT_SECS,
                ) {
                    Ok(_) => {
                        println!("On {}, within 15 minutes:", device.trim());
                        println!();
                        println!("    atlas household join {short} \"{}\"", device.trim());
                        println!();
                        println!("Or open the hub there, go to Your devices, and type:");
                        println!();
                        println!("    {short}");
                        println!();
                        if phrase.is_some() {
                            println!(
                                "The household key goes with it, so sealed bundles from here \
                                 will open there straight away."
                            );
                        }
                        println!(
                            "Nothing in {} says whose it is or what is in it -- the code is \
                             the only thing that opens it, and it clears itself either way.",
                            folder.trim()
                        );
                        return;
                    }
                    Err(why) => println!(
                        "(I couldn't leave an invitation in your sync folder: {why} -- \
                         falling back to the long code.)"
                    ),
                }
            }

            // No shared folder: the old way, which carries everything in the
            // code itself and is why it is long.
            let code = match atlas::server::new_token() {
                Ok(t) => t,
                Err(e) => {
                    println!("Couldn't generate a pairing code: {e}");
                    return;
                }
            };
            let pairing = atlas::household::new_pairing(&code, now);
            match atlas::household::encode_pairing(&h.id, &h.name, &pairing) {
                Some(block) => {
                    println!(
                        "On {}, within 3 minutes, run:\n  atlas household join \"{block}\" \"{}\"",
                        device.trim(),
                        device.trim()
                    );
                    // The household key goes with it, if there is one and a
                    // folder both can see. Sealed under the pairing code,
                    // which is a one-use token with real entropy, and gone
                    // when it is taken or when the three minutes are up.
                    //
                    // Without this the new device is paired and still cannot
                    // read a single sealed bundle until somebody copies a key
                    // file by hand -- which is the step this whole design is
                    // trying not to ask for.
                    let kept: atlas::sync::KeptKey = store.load(atlas::sync::KEY_FILE);
                    let folder = sync_folder();
                    if kept.is_set() && !folder.trim().is_empty() {
                        match kept.phrase().and_then(|phrase| {
                            atlas::sync::leave_handoff(
                                std::path::Path::new(folder.trim()),
                                &h.id,
                                &code,
                                &phrase,
                                now,
                                atlas::sync::HANDOFF_WAIT_SECS,
                            )
                            .map_err(|e| e)
                        }) {
                            Ok(_) => println!(
                                "\nI've left the key for it in your sync folder, sealed under \
                                 that code. It'll pick it up when it joins (give your cloud \
                                 folder a few minutes to carry it), and the file goes either \
                                 way after fifteen minutes."
                            ),
                            Err(why) => println!("\n(I couldn't leave the key for it: {why})"),
                        }
                    } else if kept.is_set() {
                        println!(
                            "\nYour sync folder isn't set, so I can't hand the key over \
                             automatically. Set it on both machines and pair again, or copy \
                             {} across.",
                            atlas::sync::card_path().display()
                        );
                    }
                }
                None => println!("Couldn't build a pairing code -- the household name has a '|' in it."),
            }
        }
        Some("join") => {
            let Some(code) = args.get(1) else {
                println!("atlas household join <code> <this device's name>");
                return;
            };
            let device = args[2..].join(" ");
            let mine = atlas::household::Household::load(&store);

            // A short code first: ten characters, with everything else
            // waiting in the folder both machines can see. The long block
            // still works below, for a pairing started before this existed
            // and for two machines with no folder in common.
            let short = atlas::household::take_invitation(
                std::path::Path::new(sync_folder().trim()),
                code,
                atlas::store::now(),
            );
            if let Ok(inside) = short {
                if mine.is_set() && mine.id != inside.for_household {
                    println!(
                        "This device already belongs to {} -- joining {} would mean two \
                         households on one machine, which this file exists to prevent.",
                        mine.name, inside.name
                    );
                    return;
                }
                if device.trim().is_empty() {
                    println!("atlas household join <code> <this device's name>");
                    return;
                }
                let joined = atlas::household::Household {
                    id: inside.for_household.clone(),
                    name: inside.name.clone(),
                    made_at: atlas::store::now(),
                    devices: vec![device.trim().to_string()],
                };
                match joined.save(&store) {
                    Ok(()) => println!("Joined. This device now belongs to {}.", joined.name),
                    Err(e) => {
                        println!("Couldn't save that: {e}");
                        return;
                    }
                }
                match inside.key_phrase {
                    Some(phrase) => {
                        let keeping =
                            atlas::sync::KeptKey::keeping(&phrase, atlas::store::now());
                        match store.save(atlas::sync::KEY_FILE, &keeping) {
                            Ok(()) => {
                                println!(
                                    "The household key came with it, so sealed bundles from \
                                     your other machine open here."
                                );
                                if let Ok(card) = atlas::sync::write_card(&phrase) {
                                    println!("Written down in {}.", card.display());
                                }
                            }
                            Err(e) => println!("I got the key and couldn't keep it: {e}"),
                        }
                    }
                    None => println!(
                        "(No household key came with it -- the other machine isn't sealing \
                         what it carries.)"
                    ),
                }
                return;
            }

            let Some((their_id, their_name, pairing)) = atlas::household::decode_pairing(code) else {
                // The short path's reason is the useful one here: a mistyped
                // ten-character code is far more likely than a mangled block.
                println!("{}", short.unwrap_err());
                return;
            };
            let meeting = atlas::household::meets(
                &mine.id,
                &their_id,
                mine.is_set() && mine.id == their_id,
            );
            println!("{}", atlas::household::saw_another(&meeting));
            match meeting {
                atlas::household::Meeting::Mine => {
                    println!("Already paired -- nothing to do.");
                }
                atlas::household::Meeting::NotMine if mine.is_set() => {
                    println!(
                        "This device already belongs to {} -- joining {their_name} would mean \
                         two households on one machine, which this file exists to prevent.",
                        mine.name
                    );
                }
                _ => {
                    if !pairing.still_good(atlas::store::now()) {
                        println!("That code has expired -- ask for a fresh one with `atlas household pair`.");
                        return;
                    }
                    if device.trim().is_empty() {
                        println!("atlas household join <code> <this device's name>");
                        return;
                    }
                    let joined = atlas::household::Household {
                        id: their_id,
                        name: their_name,
                        made_at: atlas::store::now(),
                        devices: vec![device.trim().to_string()],
                    };
                    match joined.save(&store) {
                        Ok(()) => {
                            println!("Joined. This device now belongs to {}.", joined.name);
                            // And the key, if the other side left one. This
                            // is the whole point of doing it here: the code
                            // that proved the pairing is the same code that
                            // opens the key, so there is nothing further to
                            // type and nothing to copy.
                            let folder = sync_folder();
                            if folder.trim().is_empty() {
                                println!(
                                    "Your sync folder isn't set here yet. Set `sync.folder` to \
                                     the folder the other machine uses and pair again, and \
                                     I'll pick up the key with it."
                                );
                            } else {
                                match atlas::sync::take_handoff(
                                    std::path::Path::new(folder.trim()),
                                    &joined.id,
                                    &pairing.code,
                                    atlas::store::now(),
                                ) {
                                    Ok(phrase) => {
                                        let keeping = atlas::sync::KeptKey::keeping(
                                            &phrase,
                                            atlas::store::now(),
                                        );
                                        match store.save(atlas::sync::KEY_FILE, &keeping) {
                                            Ok(()) => {
                                                println!(
                                                    "I picked up the household key as well, so \
                                                     sealed bundles from your other machine will \
                                                     open here."
                                                );
                                                if let Ok(card) =
                                                    atlas::sync::write_card(&phrase)
                                                {
                                                    println!(
                                                        "Written down in {}.",
                                                        card.display()
                                                    );
                                                }
                                            }
                                            Err(e) => println!(
                                                "I got the key and couldn't keep it: {e}"
                                            ),
                                        }
                                    }
                                    // Not an error worth alarming anyone
                                    // with: most pairings have no key to
                                    // carry, because sealing is off.
                                    Err(why) => println!("(No key came across: {why})"),
                                }
                            }
                        }
                        Err(e) => println!("Couldn't save that: {e}"),
                    }
                }
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try status, proof, init, pair or join."),
    }
}

/// List and restore backups, checking whose household a backup actually
/// belongs to before anything is copied over your own.
/// `atlas trace` — every model call this install has made.
///
/// The flight recorder, read back. Whether the local model is good enough, or
/// a question needs the bigger one, is a measurement — and this is where the
/// measurement lives. Never the words of a prompt: see
/// `trace::STORES_NO_CONTENT`.
/// `atlas search check`: how well search finds the right note, measured the
/// same way Atlas measures it whenever its meaning model changes — known
/// questions (yours in `search-questions.txt`, plus some made from the notes),
/// scored by words alone and by meaning. Meaning uses only vectors already
/// made with the current model, so this never runs the encoder over the lot.
pub(super) fn run_search_check() {
    let store = atlas::roots::store();
    let tools = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).unwrap_or_default();
    let notes = tools.research.clone().resolved(&store.install_root()).notes_dir;
    let mut lib = atlas::recall::library_from_dir(std::path::Path::new(&notes));
    if lib.is_empty() {
        println!("There are no notes in {notes} to search, so there's nothing to measure.");
        return;
    }
    let mut questions = atlas::recall::questions_written(
        &std::fs::read_to_string(store.root().join("search-questions.txt")).unwrap_or_default(),
    );
    questions.extend(atlas::recall::questions_from(&lib, 40));
    if questions.is_empty() {
        println!("The notes are too short to make questions from.");
        return;
    }
    let now = atlas::store::now();
    let words = atlas::recall::measure(&lib, &questions, None, &tools.recall, now);
    let fp = atlas::meaning::fingerprint(&tools.meaning, &tools.vars);
    let remembered = atlas::meaning::Remembered::load(&store);
    let same_model = !fp.is_empty() && remembered.model() == fp;
    let mut have = 0usize;
    if same_model {
        for p in lib.pieces.iter_mut() {
            p.embedding = remembered.get(&p.title, &p.text).cloned();
            have += p.embedding.is_some() as usize;
        }
    }
    let usable = tools.recall.semantic && have > 0 && atlas::meaning::available(&tools.meaning, &tools.vars);
    let meaning = usable.then(|| {
        let embed = |q: &str| atlas::meaning::embed(&tools.meaning, &tools.vars, q).ok();
        atlas::recall::measure(&lib, &questions, Some(&embed), &tools.recall, now)
    });
    let check = atlas::recall::SearchCheck { at: now, model: if usable { fp } else { String::new() }, words, meaning };
    let kept: Vec<atlas::recall::SearchCheck> = store.load("search_checks");
    println!("{}", check.said(kept.last()));
    if !usable {
        println!(
            "Meaning search wasn't measured: {}.",
            if !tools.recall.semantic {
                "it's switched off"
            } else if !same_model || have == 0 {
                "no note has a vector from the current model yet (Atlas makes them in the background)"
            } else {
                "the meaning model isn't installed"
            }
        );
    }
}

pub(super) fn run_trace(args: &[String]) {
    // The store, not `data/logs`. `Daemon::trace_path` writes into the store
    // root, and the first version of this command read a different directory
    // — so `atlas trace` reported an empty log while the daemon was filling
    // one two folders away. `tests/flight_recorder.rs` holds the two together.
    let path = atlas::trace::log_path(atlas::roots::store().root());
    let t = atlas::trace::load(&path);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            println!("{}", t.spoken());
            if t.calls.is_empty() {
                println!("{}", path.display());
                return;
            }
            // Per module, because "what is actually using the model" is never
            // what you expect -- the module header says so and this is the
            // line that shows it.
            let mut by: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
            for c in &t.calls {
                let e = by.entry(c.asked_by.as_str()).or_default();
                e.0 += 1;
                if !c.ok() {
                    e.1 += 1;
                }
            }
            for (who, (n, bad)) in &by {
                let rate = t.failure_rate(who);
                println!(
                    "  {who}: {n} call{}, {bad} failed ({:.0}%)",
                    if *n == 1 { "" } else { "s" },
                    rate * 100.0
                );
            }
            let mut models: std::collections::BTreeSet<&str> =
                t.calls.iter().map(|c| c.model.as_str()).collect();
            models.remove("none");
            for m in models {
                println!("  {m}: typically {}ms", t.typical_ms(m));
            }
            let corrections = t.caused_corrections().len();
            if corrections > 0 {
                println!("  {corrections} call(s) you went on to correct");
            }
            println!("{}", path.display());
        }
        // `atlas trace grades`: how each kind of call is doing, graded
        // without a model — a read-back that found a chatbot's voice, a
        // figure missing from its sources, a seat that wouldn't commit, your
        // corrections.
        Some("grades") => {
            let card = t.scorecard();
            if card.is_empty() {
                println!("Nothing graded yet.");
            }
            for s in &card {
                println!("  {}", s.said());
            }
            let kept = atlas::trace::examples(&path).len();
            println!(
                "The call log keeps no words ({}). The words of graded calls are kept apart, scrubbed: \
                 {kept} so far (`atlas trace examples`). Below {} grades a kind is counted, not rated.",
                atlas::trace::STORES_NO_CONTENT,
                atlas::trace::ENOUGH_TO_MEASURE
            );
        }
        Some("examples") => {
            // The graded calls whose words were kept: what a new model can be
            // tested against, one line of JSON each, in the file named below.
            let all = atlas::trace::examples(&path);
            if all.is_empty() {
                println!("No graded examples kept yet.");
            }
            let mut by: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
            for e in &all {
                let n = by.entry(e.asked_by.as_str()).or_default();
                if e.good { n.0 += 1 } else { n.1 += 1 }
            }
            for (who, (good, bad)) in by {
                println!("  {who}: {good} good, {bad} bad");
            }
            println!("Kept in {}. Emails, phone numbers, long numbers, web-address queries and \
                      anything after \"password\" are taken out; names and addresses are not.",
                     atlas::trace::examples_path(&path).display());
        }
        Some("compact") => {
            let before = t.calls.len();
            match atlas::trace::compact(&path, atlas::trace::KEEP) {
                Ok(kept) if kept == before => println!("Nothing to drop — {before} calls."),
                Ok(kept) => println!("Kept the newest {kept} of {before}."),
                Err(e) => println!("Couldn't rewrite the log: {e}"),
            }
        }
        Some("failures") => {
            let bad: Vec<&atlas::trace::Call> = t.calls.iter().filter(|c| !c.ok()).collect();
            if bad.is_empty() {
                println!("No failed calls on record.");
                return;
            }
            for c in bad.iter().rev().take(20) {
                println!(
                    "  {} {} — {}",
                    c.at,
                    c.asked_by,
                    c.failed.as_deref().unwrap_or("(no reason recorded)")
                );
            }
        }
        Some(other) => println!("Don't know `atlas trace {other}`. Try status, failures or compact."),
    }
}

/// `atlas index` — the notes index, from outside the daemon.
///
/// The daemon notices drift hourly and offers to fix it. This is the same
/// thing on demand, and it is also the only way to look at the index without
/// waiting an hour to be told about it.
pub(super) fn run_index(args: &[String]) {
    let cfg = Config::load(&atlas::roots::config_dir()).ok();
    let dir: std::path::PathBuf = cfg
        .as_ref()
        .and_then(|c| c.tools.as_ref())
        .map(|t| t.research.notes_dir.clone())
        .unwrap_or_else(|| atlas::roots::notes_dir().to_string_lossy().into_owned())
        .into();
    let folder = dir.to_string_lossy().to_string();
    let master = atlas::contents::master_path(&dir);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            let Some(c) = atlas::contents::load(&folder, &master) else {
                println!("No index yet. `atlas index rebuild` writes one from {folder}.");
                return;
            };
            println!("{}", atlas::nudge::what_i_know_of(&c));
            println!("Index: {}", master.display());
            let d = atlas::contents::drift(&c, &atlas::contents::names_on_disk(&dir));
            println!("{}", d.plain());
            // Named, not just counted. A count tells you something is wrong;
            // the names tell you whether it matters.
            for n in d.unlisted.iter().take(10) {
                println!("  not in the index: {n}");
            }
            for n in d.missing.iter().take(10) {
                println!("  promised, not there: {n}");
            }
            let blank = c.useless_lines().len();
            if blank > 0 {
                println!("{blank} line(s) say nothing the filename didn't.");
            }
            if c.needs_splitting() {
                println!(
                    "Past {} lines — worth splitting into folders.",
                    atlas::contents::MAX_LINES
                );
            }
            if !d.is_clean() {
                println!("\natlas index rebuild");
            }
        }
        Some("rebuild") => {
            if !dir.is_dir() {
                println!("There's no notes folder at {folder} yet, so there's nothing to index.");
                return;
            }
            let before = atlas::contents::load(&folder, &master)
                .map(|c| atlas::contents::drift(&c, &atlas::contents::names_on_disk(&dir)));
            match atlas::contents::rebuild(&dir) {
                Ok(c) => {
                    match before {
                        Some(d) if !d.is_clean() => println!(
                            "Rebuilt: {} added, {} dropped.",
                            d.unlisted.len(),
                            d.missing.len()
                        ),
                        Some(_) => println!("Rebuilt. It already matched."),
                        None => println!("Wrote the first index."),
                    }
                    println!("{}", atlas::nudge::what_i_know_of(&c));
                    println!("{}", master.display());
                }
                Err(e) => println!("Couldn't write the index: {e}"),
            }
        }
        Some("show") => match atlas::contents::load(&folder, &master) {
            Some(c) => print!("{}", c.as_markdown()),
            None => println!("No index yet. `atlas index rebuild` writes one."),
        },
        Some(other) => {
            println!("Don't know `atlas index {other}`. Try status, show or rebuild.");
        }
    }
}

pub(super) fn run_backups(args: &[String]) {
    let store = atlas::roots::store();
    let cfg = Config::load(&atlas::roots::config_dir()).ok();
    let backup_cfg =
        cfg.as_ref().and_then(|c| c.tools.as_ref()).map(|t| t.backup.clone()).unwrap_or_default();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            // `backups`, not `list_backups`: an unreadable backup folder must
            // not read as "you have none". `list_backups` returns an empty
            // Vec whether the folder is empty or unreadable, so a permissions
            // error here printed "No backups yet." -- the answer most likely
            // to make someone stop worrying at exactly the wrong moment, which
            // is the distinction `backups` was written to keep.
            let backups = match atlas::safety::backups(&backup_cfg) {
                Ok(b) => b,
                Err(why) => {
                    println!("{why}");
                    return;
                }
            };
            if backups.is_empty() {
                println!("No backups yet.");
                return;
            }
            for (i, b) in backups.iter().enumerate() {
                let files = b.files.map(|f| f.to_string()).unwrap_or_else(|| "?".into());
                println!("  {i}: {} — {files} files, {} bytes", b.at, b.bytes);
            }
            println!("\natlas backups restore <n>");
        }
        Some("restore") => {
            let Some(n) = args.get(1).and_then(|s| s.parse::<usize>().ok()) else {
                println!("atlas backups restore <n> -- see `atlas backups list` for the number");
                return;
            };
            let backups = atlas::safety::list_backups(&backup_cfg);
            let Some(b) = backups.get(n) else {
                println!("There's no backup numbered {n}.");
                return;
            };
            let mine = atlas::household::Household::load(&store);
            let trash_cfg = cfg
                .as_ref()
                .and_then(|c| c.tools.as_ref())
                .map(|t| t.trash.clone())
                .unwrap_or_default();
            let trash =
                atlas::safety::Trash::new(trash_cfg.resolved(&atlas::roots::install_root()));
            match atlas::safety::restore(&b.path, store.root(), &trash, &mine) {
                Ok(count) => println!("Restored {count} file(s)."),
                Err(e) => println!("Couldn't restore that: {e}"),
            }
        }
        Some(other) => println!("I don't know \"{other}\" — try list or restore."),
    }
}

/// `atlas mail` — what Atlas can see of your mailboxes, and how to add one.
///
/// Written because `config/tools.yaml` told people to run `atlas mail setup`
/// for the Azure app registration and there was no `mail` command at all. The
/// IMAP and SMTP sides have been built for days; the way in had never been.
pub(super) fn run_mail(args: &[String]) {
    let cfg = Config::load(&atlas::roots::config_dir())
        .ok()
        .and_then(|c| c.tools)
        .map(|t| t.mail)
        .unwrap_or_default();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("status") => {
            if !cfg.enabled {
                println!("Mail is switched off. Turn it on with `mail.enabled: true` in config/tools.yaml.");
            }
            if cfg.accounts.is_empty() {
                println!("No mailboxes configured yet. `atlas mail setup` walks through adding one.");
                return;
            }
            println!("{} mailbox{}:", cfg.accounts.len(), if cfg.accounts.len() == 1 { "" } else { "es" });
            for a in &cfg.accounts {
                let provider = atlas::mail::Provider::from_address(&a.address);
                let how = match atlas::mail::credential_source(a) {
                    Ok(name) => format!("credential in the vault as \"{name}\""),
                    Err(why) => format!("cannot connect: {why}"),
                };
                println!("  {} ({})  {}, {how}", a.name, a.address, provider.plain());
            }
            println!();
            println!("Nothing here has ever connected to a real server from this machine.");
            println!("The first real run has to happen where it can reach the provider.");
        }
        Some("setup") => {
            println!("Adding a mailbox");
            println!("=================");
            println!();
            println!("Both kinds go in `mail.accounts` in config/tools.yaml, one entry each.");
            println!();
            println!("{}", atlas::mail::Provider::Gmail.how_to_connect());
            println!("Yahoo, Fastmail, or any ordinary IMAP server work the same way:");
            println!("  1. Make an app password with the provider (not your normal password).");
            println!("  2. Put it in the vault:  atlas vault");
            println!("  3. Add the account, naming that vault entry in `password_from_vault`.");
            println!();
            println!("{}", atlas::mail::Provider::Outlook.how_to_connect());
            println!("  1. Go to the Azure portal -> App registrations -> New registration.");
            println!("  2. Any name. Account types: personal + work/school.");
            println!("  3. Add a platform: Mobile and desktop, and tick the device-flow box.");
            println!("  4. Under API permissions add IMAP.AccessAsUser.All and SMTP.Send.");
            println!("  5. Copy the Application (client) ID into `client_id`, set `oauth: true`.");
            println!();
            println!("Then `atlas mail` shows what it can see, and the first connection");
            println!("happens on a machine that can actually reach the provider.");
        }
        Some(other) => {
            println!("I don't know `atlas mail {other}`.");
            println!("  atlas mail        — what mailboxes are configured");
            println!("  atlas mail setup  — how to add one");
        }
    }
}

pub(super) fn run_watching(args: &[String]) {
    let store = atlas::roots::store();
    let mut w = atlas::watching::Watcher::load(&store);
    let cfg = Config::load(&atlas::roots::config_dir())
        .ok()
        .and_then(|c| c.tools.map(|t| t.watching))
        .unwrap_or_default();
    let now = atlas::store::now();

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            let running = w.running();
            if running.is_empty() {
                println!("Nothing being watched.");
            }
            for j in &running {
                println!("  #{} {} — {} in", j.id, j.name, j.ran_for(now) / 60);
            }
        }
        Some("start") => {
            let name = args[1..].join(" ");
            if name.trim().is_empty() {
                println!("atlas watching start <name>");
                return;
            }
            let id = w.watch(name.trim(), name.trim(), now);
            match w.save(&store) {
                Ok(()) => println!("Watching #{id}: {}", name.trim()),
                Err(e) => println!("Couldn't save that: {e}"),
            }
        }
        Some("done") | Some("failed") => {
            let failed = args.first().map(|s| s.to_lowercase()).as_deref() == Some("failed");
            let Some(id) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
                println!("atlas watching done <id> [last line of output]");
                return;
            };
            let last_line = args[2..].join(" ");
            let outcome =
                if failed { atlas::watching::Outcome::Failed } else { atlas::watching::Outcome::Finished };
            w.update(id, outcome, &last_line, now);
            if let Err(e) = w.save(&store) {
                println!("Couldn't save that: {e}");
                return;
            }
            for line in w.to_report(&cfg, now) {
                println!("{line}");
            }
            keep(w.save(&store), "what I am watching");
        }
        Some("report") => {
            let lines = w.to_report(&cfg, now);
            if lines.is_empty() {
                println!("Nothing new to report.");
            }
            for line in lines {
                println!("{line}");
            }
            if let Err(e) = w.save(&store) {
                println!("Couldn't save that: {e}");
            }
        }
        Some("prune") => {
            let n = w.prune(&cfg, now);
            match w.save(&store) {
                Ok(()) => println!("Stopped watching {n} job(s) that had run too long."),
                Err(e) => println!("Couldn't save that: {e}"),
            }
        }
        Some(other) => {
            println!("I don't know \"{other}\" — try list, start, done, failed, report or prune.")
        }
    }
}

/// Going somewhere your texts will not arrive.
///
/// `going_away.remind_days_before` is "remind you this many days before a
/// trip you've told it about", and there was no way to tell it about a trip.
/// `periodic_nudge` takes `days_since_last` and nothing kept a last. Both
/// were waiting on the same small thing: a date, written down.
pub(super) fn run_away(cfg: &Config, args: &[String]) {
    use atlas::goingaway::{self, Away};

    let store = atlas::roots::store();
    let acfg = cfg.tools.as_ref().map(|t| t.going_away.clone()).unwrap_or_default();
    let ccfg = cfg.tools.as_ref().map(|t| t.codes.clone()).unwrap_or_default();
    let mut away: Away = store.load(goingaway::AWAY_RECORD);
    let book: atlas::accounts::Book = store.load(atlas::accounts::FILE);
    let sets: Vec<atlas::codes::Set> = store.load("codes");
    let now = atlas::store::now();
    let flag = |name: &str| atlas::cli::flag_value(args, name);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("on") => {
            let Some(when) = args.get(1).and_then(|d| goingaway::leaving_on(d)) else {
                println!("When? A date, written the one way: atlas away on 2026-11-03");
                return;
            };
            if when < now {
                println!("That date has already gone. Nothing changed.");
                return;
            }
            away.leaving_at = when;
            away.going_to = flag("--to").map(|s| s.to_string()).unwrap_or_default();
            keep(store.save(goingaway::AWAY_RECORD, &away), "store");
            let days = away.days_until(now).unwrap_or(0);
            println!("Noted: {days} days from now.");
            if !acfg.enabled {
                println!("Getting your accounts ready is switched off, so I won't raise it");
                println!("on my own -- `going_away.enabled: true` in tools.yaml.");
            } else {
                println!("I'll start raising it {} days out.", acfg.remind_days_before);
            }
        }
        Some("clear") => {
            away.leaving_at = 0;
            away.going_to.clear();
            keep(store.save(goingaway::AWAY_RECORD, &away), "store");
            println!("No trip. I'll go back to the periodic check.");
        }
        None | Some("check") => {
            away.last_checked = now;
            keep(store.save(goingaway::AWAY_RECORD, &away), "store");

            match away.days_until(now) {
                Some(days) => println!(
                    "You're going{} in {days} day{}.",
                    if away.going_to.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" to {}", away.going_to.trim())
                    },
                    if days == 1 { "" } else { "s" }
                ),
                None if away.on_a_trip() => println!("The trip you told me about has passed."),
                None => println!("No trip set. `atlas away on 2026-11-03` when you know."),
            }
            println!();
            if book.accounts.is_empty() {
                println!("I don't know about any of your accounts yet, so I can't say which");
                println!("would lock you out. `atlas accounts` is where they go.");
            } else {
                println!("{}", goingaway::spoken(&book.accounts));
            }
            println!();
            println!("{}", atlas::codes::before_you_go(&sets, &account_names(&book), &ccfg));
            println!();
            println!("{}", goingaway::WHY_NOT_OFF);
        }
        Some(other) => println!("I don't know `atlas away {other}`. Try `atlas away check`."),
    }
}

/// The catalogue, out loud.
///
/// Atlas's own inventory of itself: what it does, what state each thing is
/// in, which files it lives in, and — the part that was missing — what each
/// of those does on a platform other than this one.
///
/// It reads from `capability::all()` and `portable::how` and nothing else, so
/// there is no second copy of the answer to keep in step. `atlas catalog
/// --module sync` is the direction Atlas needs when it is working on itself:
/// a file is open, and the question is what breaks if this goes wrong.
pub(super) fn run_catalog(args: &[String]) {
    use atlas::capability;
    use atlas::portable::Platform;

    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(|s| s.to_lowercase())
    };

    if args.iter().any(|a| a == "--markdown") {
        // The generated form of docs/CAPABILITIES.md. `tests/catalogue.rs`
        // compares the file on disk with this, so regenerating is how that
        // document stays true rather than how it gets rewritten.
        print!("{}", capability::as_markdown());
        return;
    }

    if let Some(m) = arg("--module") {
        let uses = capability::what_uses(&m);
        if uses.is_empty() {
            println!(
                "Nothing in the catalogue claims src/{m}.rs. That is either plumbing — config, \
                 storage, the platform layer — or a capability nobody wrote down."
            );
            return;
        }
        println!("src/{m}.rs is part of:");
        for c in uses {
            println!("  {:<12} {:<52} {}", c.id, c.what, c.state.plain());
        }
        return;
    }

    let platform = match arg("--platform").as_deref() {
        Some("windows") => Some(Platform::Windows),
        Some("mac") | Some("macos") => Some(Platform::Mac),
        Some("linux") => Some(Platform::Linux),
        Some("ios") | Some("iphone") => Some(Platform::Ios),
        Some("android") => Some(Platform::Android),
        Some("web") | Some("browser") => Some(Platform::Web),
        Some(other) => {
            println!("I don't know a platform called \"{other}\". Try windows, mac, linux, ios, android or web.");
            return;
        }
        None => None,
    };

    match platform {
        Some(p) => print!("{}", capability::on_platform_full(p)),
        None => {
            // No platform asked for: answer for the one it is on, since
            // "here" is the question nine times out of ten.
            let here = atlas::platform::what_am_i();
            println!("{}", capability::summary());
            print!("{}", capability::on_platform_full(here));
            // The five-platform answer on one line, which is the question
            // Atlas is built to survive: it is meant to run on Windows, a
            // Mac, Linux, an iPhone and an Android phone, and until now
            // nothing said how much of it each of those would get.
            let tally: Vec<String> = [
                Platform::Windows, Platform::Mac, Platform::Linux,
                Platform::Ios, Platform::Android, Platform::Web,
            ]
            .iter()
            .map(|p| format!("{} {}", p.bare(), capability::on(*p).len()))
            .collect();
            println!("\nOf {} things, possible on: {}", capability::all().len(), tally.join(" · "));

            let (claimed, modules) = capability::module_coverage();
            println!(
                "{claimed} of {modules} source files are claimed by something in this list. \
                 The rest is plumbing, or not written down yet."
            );
        }
    }
}

/// `atlas selftest [--no-model]`: Atlas asks itself for everything it can
/// do, on this machine, without doing anything it can't take back
/// (`atlas::selftest`). The outer run makes a scratch copy of the install
/// and runs the test in a child with that copy as its home, so every write
/// lands in the copy; the report comes back to `data/selftest/`.
pub(super) fn run_selftest(args: &[String]) {
    let no_model = args.iter().any(|a| a == "--no-model");
    let out = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    if !args.iter().any(|a| a == "--inside") {
        let real = atlas::roots::install_root();
        let reports = atlas::selftest::reports_dir(&real);
        let _ = std::fs::create_dir_all(&reports);
        // The scratch folder lives inside data/, which isn't linked or
        // copied into itself: copy first, then point the child at it.
        let tmp = std::env::temp_dir().join(format!("atlas-selftest-{}", std::process::id()));
        match atlas::selftest::scratch_copy(&real, &tmp) {
            Ok(missed) => {
                for m in missed {
                    println!("note: couldn't link {m} into the test copy; anything in it will read as missing.");
                }
            }
            Err(e) => {
                eprintln!("I couldn't make the test copy of this install: {e}");
                leave(1);
            }
        }
        println!("Testing every command on this machine, on a copy of your install. Nothing is sent, moved or approved.");
        let exe = std::env::current_exe().unwrap_or_else(|_| "atlas".into());
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("selftest").arg("--inside").arg("--out").arg(&reports).env("ATLAS_HOME", &tmp).env_remove("ATLAS_CONFIG");
        if no_model {
            cmd.arg("--no-model");
        }
        let status = cmd.status();
        let _ = std::fs::remove_dir_all(&tmp);
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("The test stopped early ({s}). What it got through is in {}.", reports.display());
                leave(1);
            }
            Err(e) => {
                eprintln!("I couldn't start the test: {e}");
                leave(1);
            }
        }
        return;
    }
    // Inside: this process's home is the copy.
    let Some(out) = out else {
        eprintln!("selftest --inside needs --out <folder>");
        leave(2);
    };
    let cfg = match Config::load(&atlas::roots::config_dir()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("I couldn't read the settings in the test copy: {e}");
            leave(1);
        }
    };
    let real_plat = atlas::platform::here();
    let plat = atlas::selftest::SafePlatform::wrapping(real_plat.as_ref());
    let llm = if no_model { None } else { cfg.tools.as_ref().and_then(model_connection) };
    let with_model = llm.is_some();
    let started = atlas::store::now();
    let state = atlas::roots::state_dir();
    let total = atlas::selftest::sentences(&atlas::intent::ToolBook::new(&cfg.commands)).len();
    let mut n = 0;
    let rows = atlas::selftest::run_all(&cfg, &plat, llm, &state, started, &mut |r| {
        n += 1;
        let mark = if r.verdict.is_a_fault() { "!!" } else { "  " };
        println!("{mark} {n:>3}/{total} {:<22} {}", r.command, r.verdict.plain());
    });
    let _ = std::fs::create_dir_all(&out);
    let stamp = atlas::hubpages::ymd((started / 86_400) as i64);
    let name = format!("report-{}-{:02}-{:02}-{}", stamp.0, stamp.1, stamp.2, started % 86_400);
    let md = atlas::selftest::report(&rows, &format!("{}-{:02}-{:02}", stamp.0, stamp.1, stamp.2), with_model);
    let _ = std::fs::write(out.join(format!("{name}.md")), &md);
    let _ = std::fs::write(out.join("latest.md"), &md);
    let _ = std::fs::write(out.join(format!("{name}.json")), serde_json::to_string_pretty(&rows).unwrap_or_default());
    println!();
    println!("{}", atlas::selftest::summary(&rows));
    println!("The report: {}", out.join("latest.md").display());
}
