//! Commands for other people and your other devices: invites and pairing, the
//! gate, sharing, trust, handoffs, Telegram, nearby, the mesh and groups.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

pub(super) fn run_invite(cfg: &Config) {
    use atlas::kin::{self, Pairings};

    let Some(their_name) = first_bare_arg("invite") else {
        println!(
            "Who are you inviting? Try:\n\n  \
             atlas invite \"Friend's Name\" --as \"Your Name\" --host your-tailscale-name\n\n\
             \"Your Name\" is what they'll see this came from. \"your-tailscale-name\" is the \
             address Tailscale gave your machine — open the Tailscale app to find it, it looks \
             something like eric-laptop."
        );
        return;
    };
    let Some(my_name) = flag_value("--as") else {
        println!("I need to know what to call you — add --as \"Your Name\".");
        return;
    };
    let Some(my_host) = flag_value("--host") else {
        println!(
            "I need your Tailscale address to put in the invite — add --host your-tailscale-name.\n\
             Open the Tailscale app on this machine to find it."
        );
        return;
    };
    // The port the daemon will actually be listening on.
    //
    // This was `unwrap_or(kin::DEFAULT_PORT)` and never consulted
    // `tools.kin.port`, while `run_daemon` binds
    // `if tc.kin.port != 0 { tc.kin.port } else { DEFAULT_PORT }`. The port
    // is baked into the invite code, so changing `kin.port` in tools.yaml
    // produced pairings that complete cleanly on both ends and then never
    // connect -- the worst kind of failure to debug, because every visible
    // step succeeded.
    let port = flag_value("--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| listening_port(&cfg));

    let dir = pairings_dir();
    let mut pairings = Pairings::load(&dir);
    let token = match atlas::server::new_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("couldn't generate a secure token: {e}");
            leave(2);
        }
    };
    let Some(code) = kin::invite(&mut pairings, &their_name, &my_name, &my_host, port, &token) else {
        println!("Your name or host can't contain the character '|' — try again without it.");
        return;
    };
    if let Err(e) = pairings.save(&dir) {
        eprintln!("couldn't save the pairing: {e}");
        leave(2);
    }

    println!("{their_name} can already reach you — I've remembered them.\n");
    println!("Send them exactly this, however you'd normally message them:\n");
    println!("{code}\n");
    println!("Once they run `atlas accept` with it, you're paired.");
}

pub(super) fn run_accept(cfg: &Config) {
    use atlas::kin::{self, Pairings};

    let Some(code) = first_bare_arg("accept") else {
        println!(
            "Paste what someone sent you:\n\n  atlas accept \"ATLAS-KIN-1:...\"\n\n\
             You'll also need to tell me who you are and your own Tailscale address the first \
             time, so I can hand back a way for them to reach you too:\n\n  \
             atlas accept \"...\" --as \"Your Name\" --host your-tailscale-name"
        );
        return;
    };

    let dir = pairings_dir();
    let mut pairings = Pairings::load(&dir);

    // Whether a return block will actually be owed is knowable before
    // accepting at all -- decode first and check, rather than trying to
    // accept with empty placeholders and attempting to guess afterwards
    // whether the empty result meant "nothing owed" or "couldn't build it".
    // Those look identical from outside `kin::accept`, so guessing after the
    // fact cannot be made reliable; asking first can be.
    let first_time = match kin::decode_invite(&code) {
        Ok(inv) => !pairings.has_peer(&inv.from_name),
        Err(e) => {
            println!("Couldn't accept that: {}", e.plain());
            return;
        }
    };

    let my_name = flag_value("--as");
    let my_host = flag_value("--host");
    // The port the daemon will actually be listening on.
    //
    // This was `unwrap_or(kin::DEFAULT_PORT)` and never consulted
    // `tools.kin.port`, while `run_daemon` binds
    // `if tc.kin.port != 0 { tc.kin.port } else { DEFAULT_PORT }`. The port
    // is baked into the invite code, so changing `kin.port` in tools.yaml
    // produced pairings that complete cleanly on both ends and then never
    // connect -- the worst kind of failure to debug, because every visible
    // step succeeded.
    let port = flag_value("--port")
        .and_then(|p| p.parse().ok())
        .unwrap_or_else(|| listening_port(&cfg));

    if first_time && (my_name.is_none() || my_host.is_none()) {
        println!(
            "This is the first time you've heard from them, so they don't have a way to reach \
             you back yet. Run this again with:\n\n  \
             atlas accept \"...\" --as \"Your Name\" --host your-tailscale-name"
        );
        return;
    }

    // Past this point either it's a repeat pairing (name/host genuinely
    // unneeded) or both were supplied -- either way `kin::accept` has
    // everything it needs.
    let (my_name, my_host) = (my_name.unwrap_or_default(), my_host.unwrap_or_default());

    match kin::accept(&mut pairings, &code, &my_name, &my_host, port) {
        Ok(accepted) => {
            if let Err(e) = pairings.save(&dir) {
                eprintln!("couldn't save the pairing: {e}");
                leave(2);
            }
            println!("Paired with {}.", accepted.from);
            match accepted.return_block {
                Some(block) => println!("\nSend this back so they can reach you too:\n\n{block}\n"),
                None => println!("You can already reach each other — nothing further to do."),
            }
        }
        Err(e) => println!("Couldn't accept that: {}", e.plain()),
    }
}

/// Run one gate for real. The command strings in `craft::ladder` are fixed,
/// known toolchain invocations -- never anything derived from user input --
/// so this is a much narrower trust surface than "run an arbitrary command",
/// closer in shape to `SignalListener::probe_devices` shelling out to a
/// specific, audited program.
pub(super) fn run_gate(dir: &std::path::Path, gate: &atlas::craft::Gate) -> atlas::craft::Ran {
    let mut parts = gate.command.split_whitespace();
    let Some(program) = parts.next() else {
        return atlas::craft::Ran {
            command: gate.command.clone(),
            tells: gate.tells,
            passed: false,
            output: "empty command".into(),
        };
    };
    let output = atlas::tools::command(program).args(parts).current_dir(dir).output();
    match output {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            atlas::craft::Ran {
                command: gate.command.clone(),
                tells: gate.tells,
                passed: o.status.success(),
                output: text.trim().to_string(),
            }
        }
        Err(e) => atlas::craft::Ran {
            command: gate.command.clone(),
            tells: gate.tells,
            passed: false,
            output: format!("couldn't run '{}': {e}", gate.command),
        },
    }
}

/// Long jobs that report back rather than being polled. There's no real
/// process-liveness check for an arbitrary command in this tree yet, so
/// this deliberately doesn't try to guess whether something is still
/// running -- a script says `atlas watching done <id>` at the end of
/// itself (`your-build && atlas watching done 3`), the same shape as any
/// webhook callback, and that's the honest signal this can act on.
/// `atlas share "<note>" --to <Friend>`
///
/// The first real caller of `household::share_with_friend`, which built a
/// `Handoff` that nothing sent for as long as it existed. One-way, carrying
/// nothing about your household — the friend's Atlas takes it in as something
/// you sent, not as part of your world.
///
/// Deliberately reuses the pairing you already have rather than inventing a
/// second way to reach someone: `atlas invite` / `atlas accept` filled in the
/// contact list, and this uses it. No pairing, no sending.
pub(super) fn run_share(args: &[String]) {
    let what = args
        .iter()
        .take_while(|a| !a.starts_with("--"))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    if what.trim().is_empty() {
        println!("atlas share \"<what you want to send>\" --to <friend>");
        return;
    }
    let Some(their_name) = flag_value("--to") else {
        println!("Who to? atlas share \"...\" --to <friend>");
        return;
    };

    let pairings = atlas::kin::Pairings::load(&pairings_dir());
    // Case-insensitively, the same as everywhere else a person types a peer's
    // name -- `kin.rs` learned this once already and it should not have to be
    // relearned per command.
    let Some(contact) = pairings
        .contacts
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(their_name.trim()))
    else {
        let known: Vec<&str> = pairings.contacts.iter().map(|c| c.name.as_str()).collect();
        if known.is_empty() {
            println!(
                "You haven't paired with anyone yet. `atlas invite \"{}\"` starts that.",
                their_name.trim()
            );
        } else {
            println!("I don't have {} — I can reach: {}", their_name.trim(), known.join(", "));
        }
        return;
    };

    // Ask first, unless you've said not to. A share leaves this machine, so
    // the default is to stop and confirm — except for a contact you've marked
    // trusted, which is the whole point of trusting them: routine sends to the
    // people you send to often should not make you type `--yes` every time.
    // `--yes` is the confirmation; being trusted is standing confirmation.
    let trusted = pairings.is_trusted(&contact.name);
    if !trusted && !has_flag("--yes") {
        let item = match flag_value("--file") {
            Some(p) => std::path::Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "that file".into()),
            None => "this".into(),
        };
        println!(
            "This would send {item} to {name}, and it leaves your machine. \
             Add --yes to send it, or `atlas trust {name}` so I stop asking for them.",
            name = contact.name
        );
        return;
    }

    let my_name = flag_value("--as")
        .or_else(|| {
            let h = atlas::household::Household::load(&atlas::roots::store());
            if h.is_set() {
                Some(h.name)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "a friend".into());

    // A file gets a longer timeout than a note. Eight megabytes over a
    // Tailscale link to a laptop that may be on a phone hotspot is not a
    // ten-second job, and a timeout that fires mid-transfer looks to the
    // sender exactly like the friend refusing it.
    match flag_value("--file") {
        None => {
            let handoff = atlas::household::share_with_friend(&what, &my_name);
            match atlas::kin::PeerLink::from_state(&pairings, &atlas::chat::Chats::default()).sealing_as(atlas::peerkey::Identity::load_or_create(&atlas::kin::where_pairings_live()).ok()).hand_note(&contact.name, &handoff, None) {
                Ok(()) => {
                    let tail = if trusted {
                        format!(" You trust {}, so I didn't ask.", contact.name)
                    } else {
                        String::new()
                    };
                    println!("Sent to {}. Nothing else went with it.{tail}", contact.name)
                }
                Err(e) => println!("{e}"),
            }
        }
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            if !path.is_file() {
                println!("There's no file at {}.", path.display());
                return;
            }
            // The covering line is optional when there is a file: `atlas
            // share --file report.pdf --to Priya` is a complete sentence on
            // its own, and making you type something as well would just get
            // you "here" every time.
            let note = if what.trim().is_empty() {
                format!(
                    "{}",
                    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                )
            } else {
                what.clone()
            };
            let handoff = atlas::household::share_with_friend(&note, &my_name);
            match atlas::kin::PeerLink::from_state(&pairings, &atlas::chat::Chats::default()).sealing_as(atlas::peerkey::Identity::load_or_create(&atlas::kin::where_pairings_live()).ok())
                .waiting(std::time::Duration::from_secs(120))
                .hand_note(&contact.name, &handoff, Some(&path))
            {
                Ok(()) => {
                    let tail = if trusted {
                        format!(" You trust {}, so I didn't ask.", contact.name)
                    } else {
                        String::new()
                    };
                    println!(
                        "Sent {} to {}. Nothing else went with it.{tail}",
                        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                        contact.name
                    )
                }
                Err(e) => println!("{e}"),
            }
        }
    }
}

/// `atlas trust [<name> | --off <name>]`
///
/// Who Atlas may send to without stopping to ask each time. With no name it
/// lists them; with a name it trusts; with `--off` it stops. Trust is the
/// standing form of the `--yes` that `atlas share` otherwise requires — you
/// pay the confirmation once, for the people you send to often, instead of on
/// every share. A share to anyone not on this list still asks first.
///
/// The name is read from the passed args rather than the whole argv, so
/// `--off <name>` parses correctly: a boolean flag has no value to skip, and
/// the shared `first_bare_arg` helper would treat the name as `--off`'s value
/// and drop it.
pub(super) fn run_trust(args: &[String]) {
    let dir = pairings_dir();
    let mut pairings = atlas::kin::Pairings::load(&dir);
    let off = args.iter().any(|a| a == "--off");
    let name = args.iter().find(|a| !a.starts_with("--")).cloned();

    let Some(name) = name else {
        // List.
        let names = pairings.trusted_names();
        if names.is_empty() {
            println!(
                "You haven't marked anyone trusted. `atlas trust <name>` lets me send to \
                 them without asking each time."
            );
        } else {
            println!("I'll send to these without asking first:");
            for n in names {
                println!("  {n}");
            }
        }
        return;
    };
    let name = name.trim().to_string();

    if off {
        if pairings.distrust(&name) {
            if let Err(e) = pairings.save(&dir) {
                eprintln!("couldn't save the trusted list: {e}");
                leave(2);
            }
            println!("Okay — I'll ask again before sending anything to {name}.");
        } else {
            println!("{name} wasn't on your trusted list, so nothing changed.");
        }
        return;
    }

    // Trusting someone you haven't paired with is harmless but almost always a
    // typo, so say so rather than silently trusting a name that can never
    // match a contact.
    let known = pairings.contacts.iter().any(|c| c.name.eq_ignore_ascii_case(&name));
    pairings.trust(&name);
    if let Err(e) = pairings.save(&dir) {
        eprintln!("couldn't save the trusted list: {e}");
        leave(2);
    }
    if known {
        println!(
            "Done — I'll send to {name} without asking. `atlas trust --off {name}` undoes it."
        );
    } else {
        println!(
            "Noted — I'll trust {name} once you've paired with them. \
             `atlas invite \"{name}\"` starts that. `atlas trust --off {name}` undoes this."
        );
    }
}

/// `atlas hand <path> [--for "<what you want done>"] [--business <name>]`
///
/// Give Atlas a file that already lives on this machine — any type, any size.
/// Taken in by reference through `tray::hand_local`: no copy is made and no
/// cap applies, so a multi-gigabyte archive is handed over as cheaply as a
/// photo, and forgetting the tray item later never touches your original.
/// `--for` is the one line Atlas reads as *your* request about the file;
/// `--business` files it on the business side of the firewall.
pub(super) fn run_hand(args: &[String]) {
    let Some(path) = args.iter().find(|a| !a.starts_with("--")).cloned() else {
        println!("atlas hand <path>   — give me a file to look at, any type or size.");
        return;
    };
    let path = std::path::PathBuf::from(&path);
    if !path.is_file() {
        println!("There's no file at {}.", path.display());
        return;
    }
    let space = match flag_value("--business") {
        Some(b) => atlas::earned::Space::Business(b),
        None => atlas::earned::Space::Personal,
    };
    let asked = flag_value("--for");
    let store = atlas::roots::store();
    let mut tray = atlas::tray::Tray::load(&store);
    match tray.hand_local(&path, &space, "this machine", asked.as_deref(), atlas::store::now()) {
        Ok(id) => {
            if keep(tray.save(&store), "the tray") {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                println!(
                    "Got {name} — it's in the tray as {id}. I'll read it where it sits; \
                     nothing was copied."
                );
            }
        }
        Err(e) => println!("{e}"),
    }
}

/// `atlas handoffs list | keep <id> | drop <id>`
///
/// The waiting list of things friends have handed over. `keep` is the
/// deliberate act that moves one into the tray — which is the point at which
/// Atlas may look at it. Arriving does not earn that; a peer credential
/// should not be able to make your Atlas go and fetch a URL.
pub(super) fn run_handoffs(args: &[String]) {
    let store = atlas::roots::store();
    let mut inbox = atlas::household::Inbox::load(&store);

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        None | Some("list") => {
            if inbox.items.is_empty() {
                println!("Nothing from anyone.");
                return;
            }
            for i in &inbox.items {
                match &i.file {
                    None => println!("  {}  from {} — {}", i.id, i.from, i.what),
                    // The size, so you can tell a screenshot from a video
                    // before deciding. Nothing has looked inside it.
                    // Bytes below a kilobyte, because "0 KB" next to a real
                    // file reads as an error rather than as a small file.
                    Some(f) => println!(
                        "  {}  from {} — {} ({}, {}, not opened)",
                        i.id,
                        i.from,
                        i.what,
                        f.name,
                        if f.size < 1024 {
                            format!("{} bytes", f.size)
                        } else if f.size < 1024 * 1024 {
                            format!("{} KB", f.size / 1024)
                        } else {
                            format!("{:.1} MB", f.size as f64 / (1024.0 * 1024.0))
                        }
                    ),
                }
            }
            println!("\n`atlas handoffs keep <id>` to take one in, `drop <id>` to bin it.");
        }
        Some("keep") => {
            let Some(id) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
                println!("atlas handoffs keep <id>");
                return;
            };
            let Some(got) = inbox.take(id) else {
                println!("Nothing waiting with that number.");
                return;
            };
            let mut tray = atlas::tray::Tray::load(&store);
            // `from` records the friend, not a device: "where did this come
            // from" should answer with the person, since that is the thing
            // you will want to know about something you did not send
            // yourself.
            //
            // This is the moment the bytes become something Atlas may read.
            // Up to here a file has only been written to the doorstep and
            // named on a list. `tray::hand_file` is what puts it where the
            // tray's normal rules apply.
            let handed = match &got.file {
                None => tray.hand(&got.what, &atlas::earned::Space::Personal, &got.from, got.at),
                Some(f) => match std::fs::read(store.root().join(&f.stored_at)) {
                    Ok(bytes) => tray.hand_file(
                        &f.name,
                        &bytes,
                        &atlas::earned::Space::Personal,
                        &got.from,
                        // The covering line is *theirs*, not yours, so it
                        // does not go in `asked` -- that field is documented
                        // as "the only text on an item Atlas treats as
                        // coming from you", and a sentence a friend wrote is
                        // exactly what must not end up there.
                        None,
                        got.at,
                        store.root(),
                    ),
                    Err(e) => Err(format!("the file wasn't where I left it: {e}")),
                },
            };
            match handed {
                Ok(tid) => {
                    let saved = tray.save(&store).and_then(|()| inbox.save(&store));
                    match saved {
                        Ok(()) => {
                            // The doorstep copy goes once the tray has its
                            // own. Leaving both means the same file twice on
                            // disk and a stale one to wonder about later.
                            if let Some(f) = &got.file {
                                let _ = std::fs::remove_file(store.root().join(&f.stored_at));
                            }
                            println!("Kept it — it's in the tray as {tid}.");
                        }
                        Err(e) => println!("Couldn't save that: {e}"),
                    }
                }
                Err(e) => println!("{e}"),
            }
        }
        Some("drop") => {
            let Some(id) = args.get(1).and_then(|s| s.parse::<u64>().ok()) else {
                println!("atlas handoffs drop <id>");
                return;
            };
            match inbox.take(id) {
                Some(got) => match inbox.save(&store) {
                    Ok(()) => {
                        // Dropped means gone, including the bytes. A "drop"
                        // that leaves the file on disk is a lie about what
                        // just happened.
                        if let Some(f) = &got.file {
                            let _ = std::fs::remove_file(store.root().join(&f.stored_at));
                        }
                        println!("Dropped the one from {}.", got.from);
                    }
                    Err(e) => println!("Couldn't save that: {e}"),
                },
                None => println!("Nothing waiting with that number."),
            }
        }
        Some(other) => println!("Don't know `handoffs {other}`. Try list, keep, or drop."),
    }
}

/// Read what a Telegram bot has been sent, and sort it the way the inbox is.
///
/// The half `messaging.rs` never had. That module had triage, weighting,
/// a folder, a note on a person and a spoken form -- `sort`, `folder_for`,
/// `note_on`, `spoken`, `work_spend` -- and **nothing could supply it a
/// message**, so the daemon called `messaging::spoken(&[], ..)` and got back
/// "0 messages, all group chat": a count of an inbox nothing had read, said
/// as fact.
///
/// This is online and secondary, which is the whole reason it is a separate
/// module and off by default. Everything primary in Atlas works unplugged.
/// This cannot -- the messages are on somebody else's server -- so a machine
/// with no network loses this and keeps the rest.
///
/// `token` is its own subcommand because the token belongs in the vault and
/// nowhere else, and typing it as an argument to the reading command is how
/// it ends up in a shell history.
pub(super) fn run_telegram(cfg: &Config, args: &[String]) {
    let tcfg = cfg.tools.as_ref().map(|t| t.telegram.clone()).unwrap_or_default();
    let store = atlas::roots::store();
    let mut vault = atlas::vault::Vault::load(&atlas::roots::install_state());
    let now = atlas::store::now();

    if args.first().map(|s| s.as_str()) == Some("token") {
        if vault.state() != atlas::vault::State::Open {
            eprintln!("The vault is locked, and the token goes in it. Open it first.");
            return;
        }
        print!("Paste the token from BotFather: ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut line = String::new();
        if std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line).is_err() {
            return;
        }
        let token = line.trim();
        // Checked for shape before it is kept. The mistake people make is
        // pasting half the line, and catching it here beats it becoming
        // "couldn't reach Telegram" a day later.
        if !atlas::telegram::looks_like_a_token(token) {
            eprintln!(
                "That doesn't look like a bot token. They're digits, a colon, then about \
                 thirty-five letters and numbers -- 123456789:AA..."
            );
            return;
        }
        match vault.put(atlas::telegram::TOKEN, atlas::vault::Kind::ApiKey, token, now) {
            Ok(()) => match vault.save(&atlas::roots::install_state()) {
                Ok(()) => println!("Kept. `atlas telegram` reads what's arrived."),
                // Never claim it kept something it did not.
                Err(e) => eprintln!("I took it but couldn't save it ({e}) -- it won't survive a restart."),
            },
            Err(e) => eprintln!("couldn't keep it: {e}"),
        }
        return;
    }

    if !tcfg.enabled {
        println!("Reading Telegram is switched off. Turn on `telegram.enabled` in your settings.");
        println!("\n{}", atlas::telegram::HOW_TO_SET_UP);
        return;
    }
    if vault.state() != atlas::vault::State::Open {
        eprintln!("The vault is locked and the token is in it. Open it first.");
        return;
    }
    let token = match vault.get(atlas::telegram::TOKEN, now) {
        Ok(t) => t,
        Err(_) => {
            println!("I haven't got a bot token yet.\n\n{}", atlas::telegram::HOW_TO_SET_UP);
            return;
        }
    };

    let since: Option<u64> = store.load::<Option<u64>>(atlas::telegram::READ_UP_TO);
    let fresh = match atlas::telegram::fetch(&token, since, &tcfg) {
        Ok(m) => m,
        Err(why) => {
            eprintln!("{why}");
            return;
        }
    };
    if fresh.is_empty() {
        println!("Nothing new.");
        return;
    }

    // How far we got, before anything else can fail. Telegram hands the same
    // messages back until it is told, so losing this means reading them all
    // again -- and a crash after printing is a crash that repeats itself.
    // Reported rather than discarded. If this does not stick, Telegram hands
    // the same messages back next time and Atlas reads them out again --
    // forever, and silently, because nothing downstream can tell. A write
    // whose result is thrown away and then confirmed is the shape
    // `no_confident_nothings.rs` exists to catch, and it caught this.
    let mut trouble: Vec<String> = Vec::new();
    if let Some(n) = atlas::telegram::read_up_to(&fresh) {
        if let Err(e) = store.save(atlas::telegram::READ_UP_TO, &Some(n)) {
            trouble.push(format!(
                "I couldn't record how far I got ({e}), so you'll see these again next time"
            ));
        }
    }

    // Kept, so the daemon answers "any messages?" from something that was
    // actually read rather than from an empty slice.
    let mut kept: Vec<atlas::messaging::Message> = store.load(atlas::telegram::KEPT);
    kept.extend(fresh.iter().cloned());
    if let Err(e) = store.save(atlas::telegram::KEPT, &kept) {
        // Said, because the next thing you ask is "any messages?" and the
        // answer would come back as though nothing had ever been read.
        trouble.push(format!("I couldn't keep them ({e}), so asking me later won't find them"));
    }

    let mcfg = cfg.tools.as_ref().map(|t| t.messaging.clone()).unwrap_or_default();
    println!("{}", atlas::messaging::spoken(&fresh, &mcfg.your_names));
    for t in &trouble {
        // Before the messages rather than after: a warning under a list is a
        // warning read second, and this one changes what the list means.
        eprintln!("({t}.)");
    }
    for m in &fresh {
        let sort = atlas::messaging::sort(m, &mcfg.your_names);
        let where_from = m.group.clone().unwrap_or_else(|| m.from.clone());
        println!("  [{}] {where_from}: {}", sort.plain(), m.text);
    }
}

/// Who else is running Atlas on this network.
///
/// The missing half of `Path::SameNetwork`. Two Atlases could already talk --
/// `kin.rs` is the door, `elsewhere.rs` is the asking, and every request
/// carries a token -- but reaching one needed a hand-typed address in
/// `elsewhere.known`, on a home network that hands out a different one after
/// a reboot.
///
/// Read-only, and it grants nothing. What comes back is a name and a port;
/// the token is still the whole of the access decision and still comes from
/// the other machine. Atlas prints the config entry rather than writing it,
/// because an entry that only needs one more field is an invitation to paste
/// a token in without thinking about what it lets in.
pub(super) fn run_nearby(cfg: &Config) {
    let ncfg = cfg.tools.as_ref().map(|t| t.nearby.clone()).unwrap_or_default();
    println!("Asking who's on this network. Nothing is connected to.\n");
    match atlas::nearby::look(&ncfg) {
        Ok(found) => println!("{}", atlas::nearby::spoken(&found, &ncfg)),
        // The socket's own words. A machine with no network at all fails
        // here, and saying so beats an empty list that reads as "nobody is
        // there".
        Err(why) => eprintln!("I couldn't ask: {why}"),
    }
}

/// What a private network would give you, and what it takes to have one.
///
/// `mesh.rs` had six correct, tested, user-facing things to say about this
/// and nothing said any of them: `honest`, `what_it_adds`, `works_without`,
/// `setup_steps`, `YOU_APPROVE_THE_DEVICE` and `WHAT_ID_DO` were reached by
/// tests alone. `mesh.kind` was the setting underneath — a string nothing
/// parsed, so `kind: tailscale` and `kind: banana` were the same setting.
///
/// Advice, not a transport. `mesh` stays on `CAPABILITY_UNWIRED` because
/// `choose` picks between four routes and only the cloud folder is built, and
/// this command opens by saying so. Setting up Tailscale after reading this
/// is worth doing on its own terms; believing Atlas will then use it is not
/// something this command is allowed to let you believe.
pub(super) fn run_mesh(cfg: &Config) {
    let mcfg = cfg.tools.as_ref().map(|t| t.mesh.clone()).unwrap_or_default();
    for line in atlas::mesh::what_a_private_network_would_give_you(&mcfg) {
        println!("{line}");
        println!();
    }
}

/// `policy::gate`, plus the one thing it doesn't know about: `identity.rs`'s
/// grace window. Off by default (`IdentityConfig::enabled` is `false`), so
/// this changes nothing unless it's turned on. When it is, the *first*
/// approval for an action named in `verify_for` still asks exactly as
/// before -- this only skips asking again within the grace window, which
/// is the whole point ("prove yourself rarely and it feels like nothing").
///
/// `Hello` is always reported `Unavailable` here -- no Windows Hello
/// detection exists anywhere in this tree yet, and the module's own doc is
/// explicit that unavailable must never be read as "assume it's him". That
/// makes every ask on this path `Gate::AskAloud`, which is exactly
/// `policy::gate`'s own spoken-yes ask -- so nothing here invents a second
/// prompt, it only decides whether the existing one is skippable.
pub(super) fn gate_with_identity(cfg: &Config, intent: &Intent, approver: &dyn Approver) -> atlas::error::Result<()> {
    let id_cfg = cfg.tools.as_ref().map(|t| t.identity.clone()).unwrap_or_default();
    let kind = atlas::session::kind_of(intent);
    let store = atlas::roots::store();

    // A guest's restriction is not optional and does not wait on
    // `IdentityConfig::enabled` -- it's the one check here that applies
    // whether or not the rest of this function is turned on.
    // From the install's own state, never from `store.root()` -- which is
    // now the *active person's* directory. Reading the registry from inside a
    // profile would mean a second person's Atlas could not find the list it
    // is a member of.
    // Handed over, and the action is one of the things Atlas will not do for
    // somebody who is not you. Checked before the profile role and before
    // `IdentityConfig::enabled`, because a handover is the one statement
    // about who is at the machine that Atlas actually has -- you said it.
    //
    // A guest *profile* and a handed-over *install* are different situations
    // with the same answer: your friend at your laptop has not switched
    // profiles and will not, so the role check alone protected nothing in the
    // case it was written for.
    let handed = atlas::handover::Handover::load(&atlas::roots::install_state());
    if handed.stance.handed_over() && atlas::handover::refuses(kind) {
        return Err(atlas::error::AtlasError::ApprovalRequired(
            atlas::handover::refusal(kind),
        ));
    }

    let profiles = atlas::profiles::Profiles::load(atlas::roots::state_dir().as_path());
    if !profiles.role().may(kind) {
        return Err(atlas::error::AtlasError::ApprovalRequired(format!(
            "guests can't {}",
            kind.replace('_', " ")
        )));
    }

    let watched = id_cfg.enabled && id_cfg.verify_for.iter().any(|k| k == kind);
    let now = atlas::store::now();

    if watched {
        let identity = atlas::identity::Identity::load(&store);
        if identity.within_grace(&id_cfg, now) {
            return Ok(());
        }
    }

    policy::gate(intent, approver)?;

    if watched {
        // Deliberately NOT `Proof::Verified`.
        //
        // What just succeeded is `policy::gate` -- a spoken or typed yes.
        // `identity.rs` states the rule this broke in its own doc: *"Only a
        // real verification extends the grace window -- a spoken yes confirms
        // the action, not your identity."* Recording it as a verification
        // meant one "yes" bought a four-hour window in which nothing was
        // asked again, on exactly the actions someone thought worth watching.
        //
        // Nothing in this tree can perform a real verification yet: `Hello`
        // is always `Unavailable` and there is no Windows Hello detection.
        // So the grace window stays shut, every watched action keeps asking,
        // and that is the correct behaviour until a real proof exists --
        // `identity::Gate` and `Proof::Verified` are waiting on that, not on
        // this line.
        let mut identity = atlas::identity::Identity::load(&store);
        identity.record(atlas::identity::Proof::SpokenYes, now);
        if let Err(e) = identity.save(&store) {
            eprintln!("couldn't record that approval: {e}");
        }
    }
    Ok(())
}

/// `atlas group` -- group chats you own: who's in them and who may post.
///
/// Changes are saved here and sent to every member's Atlas by the running
/// Atlas, signed by this one's key, so each of them can check it was you.
pub(super) fn run_group(args: &[String]) {
    let store = atlas::roots::store();
    let dir = atlas::kin::where_pairings_live();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_default();
    let rest = |i: usize| args.get(i..).map(|a| a.join(" ")).unwrap_or_default();
    let done = match arg(0).as_str() {
        "" | "list" => {
            let (views, addable) = atlas::groups::views(&store, &dir);
            if views.is_empty() {
                println!("No groups with an owner yet. `atlas group new <name> <people, comma separated>` makes one.");
            }
            for v in views {
                println!(
                    "\n{}{} -- {}",
                    v.name,
                    if v.release_channel { " (release channel)" } else { "" },
                    if v.mine { "yours".to_string() } else { format!("made by {}", v.owner) }
                );
                for (who, role, _) in v.seats {
                    println!("  {who}: {}", role.plain());
                }
            }
            if !addable.is_empty() {
                println!("\nPeople you can add: {}", addable.join(", "));
            }
            return;
        }
        // atlas group new "Friends" Sam, Maya      atlas group release "Atlas updates" Sam, Maya
        "new" => atlas::groups::act(&store, &dir, "new", &arg(1), &rest(2), ""),
        "release" => atlas::groups::act(&store, &dir, "new", &arg(1), &rest(2), "release"),
        // atlas group add "Friends" Sam [reader]
        "add" => atlas::groups::act(&store, &dir, "add", &arg(1), &arg(2), &arg(3)),
        "remove" => atlas::groups::act(&store, &dir, "remove", &arg(1), &arg(2), ""),
        // atlas group role "Friends" Sam reader
        "role" => atlas::groups::act(&store, &dir, "role", &arg(1), &arg(2), &arg(3)),
        "rename" => atlas::groups::act(&store, &dir, "rename", &arg(1), "", &rest(2)),
        "adopt" => Err("Giving an older group an owner renames and messages it, so it needs Atlas itself \
                        running: use the Groups page on the hub.".into()),
        other => Err(format!(
            "I don't know `atlas group {other}`. Try: list, new <name> <people>, release <name> <people>, \
             add <group> <person> [reader], remove <group> <person>, role <group> <person> <member|reader>, \
             rename <group> <new name>."
        )),
    };
    match done {
        Ok(s) => println!("{s}"),
        Err(e) => {
            eprintln!("{e}");
            leave(1);
        }
    }
}
