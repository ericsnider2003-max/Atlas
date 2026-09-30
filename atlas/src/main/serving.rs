//! `atlas` with no command: the daemon, the voice loop, and the hub's answers
//! while it runs (`handle`, looking, the dashboard cards).
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

/// The model connection, worked out once for whichever door you came in by.
///
/// Was inline in `run_daemon` alone, which is part of why `atlas --voice` never
/// had one: the other front door could not reach it without copying fifty
/// lines, so it didn't, and answered five intents out of fifty-six instead.
///
/// `tools.llm` always wins: if you have written a connection you mean it, and
/// silently overriding it would be the worst kind of helpful. But with no
/// `tools.llm` and a `models` folder that has something in it, Atlas can
/// build the connection itself -- it knows which model fits this machine and,
/// from the file's own chat template, exactly how that model's prompt has to
/// be wrapped. That last part is the bit worth having: a hand-written body
/// with the wrong control tokens still gets answers, just worse ones, and
/// nothing tells you.
///
/// This is what "no Ollama in the path" means in practice. `ShellLlm` was
/// always capable of talking to llama-server; what was missing was anything
/// that knew what to say to it.
pub(super) fn model_connection(tc: &atlas::voice::ToolsConfig) -> Option<std::sync::Arc<dyn Llm>> {
    // In the library since 27 Sep 2026, so the phone core builds the same
    // connection (it had none at all: OPEN_GAPS P.7).
    atlas::models::connection(tc)
}

pub(super) fn run_daemon(cfg: &Config, plat: &dyn Platform, unattended: bool) {
    let Some(tc) = cfg.tools.as_ref() else {
        eprintln!("config/tools.yaml is missing — the daemon is voice-first and needs it.");
        leave(2);
    };

    // Before anything touches state. Two long-running daemons both reading
    // data/state, changing it in memory, and writing the whole thing back
    // would mean the second one quietly erasing whatever the first learned.
    //
    // Deliberately not applied to the interactive prompt above -- a quick
    // one-off command while --daemon is already running in the background is
    // exactly the kind of thing Atlas should still answer, and locking that
    // out too would make the CLI useless whenever the daemon is up.
    let only = atlas::onlyone::OnlyOne::at(&atlas::roots::data_dir());
    // Patiently: a lock that reads abandoned may be an Atlas that was only
    // asleep with the laptop and is about to beat (`onlyone::WOKE_GRACE_SECS`).
    match only.take_patiently(std::time::Duration::from_secs(atlas::onlyone::WOKE_GRACE_SECS), &atlas::store::now) {
        Err(why) => {
            eprintln!("{why}");
            atlas::firstlaunch::note_start_problem(&atlas::roots::install_root(), &why);
            leave(1);
        }
        Ok(found) => {
            if !matches!(found, atlas::onlyone::Found::Free) {
                println!("{}", found.plain());
            }
        }
    }

    // The words on the desktop while Atlas speaks (`overlaywin`). Always
    // started on Windows: it reads its own switch every few seconds, so
    // turning it on in Settings works without a restart, and it ends itself
    // when this process does.
    if cfg!(windows) {
        if let Ok(exe) = std::env::current_exe() {
            if let Ok(child) = atlas::firstlaunch::spawn_quietly(&exe, &["overlay"]) {
                atlas::unwaited::dont_wait(child);
            }
        }
    }

    // The microphone this machine really has, for the daemon too (29 Sep
    // 2026): everything below reads this one configuration.
    let tc_owned = pick_the_microphone(cfg, plat, tc);
    let mut cfg_owned = cfg.clone();
    cfg_owned.tools = Some(tc_owned);
    let cfg = &cfg_owned;
    let tc = cfg.tools.as_ref().expect("just set");
    let voice = Voice::new(tc);
    let store = atlas::roots::store();

    // Read here, before `store` is moved into the daemon below. The hub is
    // bound further down and needs it then; taking it now is cheaper than
    // opening a second store, and keeps to the one-store rule
    // `tests/one_install_root.rs` enforces.
    let hub_token = atlas::server::token_for(&store);

    // A model connection Atlas worked out for itself, when there is no
    // hand-written one.
    //
    // `tools.llm` always wins: if you have written a connection you mean it,
    // and silently overriding it would be the worst kind of helpful. But with
    // no `tools.llm` and a `models` folder that has something in it, Atlas can
    // build the connection itself -- it knows which model fits this machine
    // and, from the file's own chat template, exactly how that model's prompt
    // has to be wrapped. That last part is the bit worth having: a
    // hand-written body with the wrong control tokens still gets answers, just
    // worse ones, and nothing tells you.
    //
    // This is what "no Ollama in the path" means in practice. `ShellLlm` was
    // always capable of talking to llama-server; what was missing was anything
    // that knew what to say to it.
    let llm = model_connection(tc);

    // Somewhere to type a passphrase, so that "I'm back" has a prompt to
    // summon. Without it the phrase is answered honestly and names
    // `atlas handover back` instead -- see `daemon::take_it_back`.
    let mut d = Daemon::new(cfg, plat, llm, store, Proactive::new(tc.proactive.clone()))
        .starting_the_model_server()
        .with_typed_prompt(Box::new(atlas::typed::Console))
        .watch_settings(atlas::roots::config_dir());
    d.autonomy = if unattended { Autonomy::Unattended } else { Autonomy::Supervised };

    // The door to another Atlas -- closed unless you have actually named
    // someone to trust. Binding failure here (port in use, etc.) is loud
    // rather than silently leaving the door shut, because a peer that thinks
    // it can reach you and can't is a worse failure than Atlas not starting.
    // Peers come from two places: whoever you hand-typed into tools.yaml
    // under kin.peers, and whoever `atlas invite` / `atlas accept` has
    // paired you with in data/state/kin_peers.yaml. Running one of those
    // commands *is* the deliberate act of trust -- requiring a further
    // `enabled: true` in a file you were never asked to open would be
    // exactly the extra step this was built to remove. tools.yaml's
    // `kin.enabled` stays meaningful for the hand-edited list alone: typing
    // a token into a config file does not carry the same built-in
    // deliberateness, so that path still asks for it explicitly.
    let auto_paired = atlas::kin::Pairings::load(&pairings_dir()).peers;
    let manual_peers: Vec<atlas::kin::Peer> =
        if tc.kin.enabled { tc.kin.peers.iter().cloned().map(Into::into).collect() } else { Vec::new() };
    let mut all_peers = auto_paired;
    for p in manual_peers {
        if !all_peers.iter().any(|x| x.name == p.name) {
            all_peers.push(p);
        }
    }
    // The same trusted devices may also send notices through the hub's own
    // address (Eric, B7), which is the one a phone or tablet reaches over
    // Tailscale. Notices only: a peer token still can't open a hub page.
    let hub_peers = all_peers.clone();

    // Opened with nobody paired too, since 25 Sep: a friend link made from
    // the command line, or before this start, must find the door there when
    // your friend uses it. With nobody paired and no link out it lets nobody
    // in -- every door on it refuses a token it doesn't know, and the friend
    // door refuses any secret you didn't make (`kin::Door::receive_friend`).
    {
        let port = if tc.kin.port != 0 { tc.kin.port } else { atlas::kin::DEFAULT_PORT };
        let names: Vec<String> = all_peers.iter().map(|p| p.name.clone()).collect();
        match atlas::server::SignalListener::bind(port, all_peers.clone()) {
            Ok(l) => {
                if names.is_empty() {
                    println!("The door for friends is open on port {port}, with nobody let in yet.");
                } else {
                    println!("Listening for {} on port {port} (friends reach it sealed; your own devices over your own network).", names.join(", "));
                }
                d = d.with_signal_listener(l);
                // Atlas's own Tor: how friends reach this Atlas, and it theirs.
                match d.start_tor() {
                    Ok(()) => println!("Starting Tor so friends can reach you from anywhere."),
                    Err(e) => eprintln!("{e}"),
                }
            }
            Err(e) => {
                eprintln!("couldn't open the door friends reach you on: {e}");
                eprintln!("continuing without it, and trying again each minute -- everything else still works.");
                d.log.warn(&format!("couldn't open the door friends reach you on (port {port}): {e}; trying again each minute"));
                d = d.with_signal_door_later(port, all_peers);
            }
        }
    }

    // The hub, served by this Atlas. Until now the only serve loop was
    // `run_hub`'s settings-only mode, so every page except Settings and Access
    // answered "needs the full Atlas running" -- and there was no other mode
    // in which to run it. Failing to open it is not a reason to refuse to
    // start: everything else still works without a browser.
    // `token_for`, not `new_token`: the same token every run, so the address
    // below can be bookmarked. It used to be minted fresh at every start,
    // which made the dashboard reachable only by copying it out of this
    // console window — see `server::token_for` for why that was the defect
    // rather than the design.
    //
    // Opened by `server::open_hub` (28 Sep 2026): a port that's taken is
    // tried again in the background, and after a while the hub opens on a
    // port beside it rather than not at all -- it used to be bound once, and
    // a taken port meant no hub for the whole session with nothing said.
    // Where it really is, and which Atlas it is, goes in `data/state`
    // (`server::DOOR_FILE`) for `atlas hub`, the phone link, Setup and "Open
    // Atlas"; the icon by the clock is told too.
    let mut hub_address: Option<String> = None;
    let configured_port = tc.server.port;
    match hub_token.and_then(|token| {
        // Your setting, not a copy of it with the answer changed. This used
        // to force `enabled = true` here, which made `server.enabled: false`
        // a switch that did nothing.
        let scfg = tc.server.clone();
        let id = atlas::server::new_token()?;
        let state = atlas::roots::state_dir();
        let (t, i) = (token.clone(), id.clone());
        let on_open: Box<dyn Fn(u16) + Send> = Box::new(move |port| {
            if let Err(e) = atlas::server::record_door(&state, &atlas::server::Door { port, id: i.clone() }) {
                eprintln!("couldn't note where the hub is ({e}); `atlas hub` may give the usual port");
            }
            atlas::notifyicon::tray_hub_address(&atlas::server::hub_url(port, &t, "/hub"));
            atlas::notifyicon::tray_hub_note((port != configured_port).then(|| {
                format!("Hub on port {port} (its usual {configured_port} is taken)")
            }));
        });
        // Its own threads read the connections; the loop answers
        // (`server::HubDoor`).
        atlas::server::open_hub(&scfg, &token, &id, hub_peers.clone(), atlas::server::Retry::default(), on_open)
            .map(|s| (s, token))
    }) {
        Ok((server, token)) => {
            for line in server.take_news() {
                println!("{line}");
            }
            // The configured port when it's still being waited for: the
            // bookmark's address, and the one it will most likely open on.
            let port = if server.port() != 0 { server.port() } else { configured_port };
            println!("your dashboard: {}", atlas::server::hub_url(port, &token, "/hub"));
            println!("  same address every time -- bookmark it. `atlas hub` prints it again.");
            hub_address = Some(atlas::server::hub_url(port, &token, "/hub"));
            d.hub_server = Some(server);
        }
        Err(e) => {
            eprintln!("couldn't open the dashboard: {e}");
            eprintln!("everything else still works -- talk to me here instead.");
            atlas::notifyicon::tray_hub_note(Some(format!("No hub: {e}")));
        }
    }

    // Before the loop starts, so a Ctrl-C at any point after this is a
    // shutdown rather than a kill. Installed here and not inside `Daemon::run`
    // because a process-wide signal handler belongs to the program, not to a
    // library type -- `run` is called by tests too, and a test that installs
    // signal handlers is a test that changes how the test runner exits.
    atlas::goodbye::listen();

    let audio_ok = atlas::input::audio_available(Some(tc)).unwrap_or(false);
    let keyboard = Keyboard::spawn();
    let mut throttle = Throttle::new(tc.perf.clone());

    println!(
        "atlas running ({}). wake phrase: {}.",
        if unattended { "unattended" } else { "supervised" },
        tc.wake.as_ref().map(|w| w.phrase.clone()).unwrap_or_else(|| "<none>".into())
    );
    if !audio_ok {
        println!("audio unavailable — run `atlas doctor`. You can still type commands here.");
        // Written down as well (29 Sep 2026): started from the icon or at
        // sign-in there's no console, so printed alone it reached nobody.
        d.log.warn("the voice tools aren't all there (listening, speech-to-text or speaking): the hub's Health page says which");
    }
    // Push-to-talk and the typing box, from anywhere in Windows (H1).
    let keys = atlas::hotkeys::Keys::from_settings(&tc.push_to_talk, &tc.quick_input);
    match atlas::hotkeys::start(&keys) {
        Ok(h) => {
            if let Some(line) = keys.said(&tc.push_to_talk, &tc.quick_input) {
                println!("{line}");
            }
            for p in &h.problems {
                println!("({p})");
                d.log.warn(p);
            }
            d.hotkeys = Some(h);
        }
        Err(why) => {
            println!("(no push-to-talk or typing-box key: {why})");
            d.log.warn(&format!("no push-to-talk or typing-box key: {why}"));
        }
    }
    println!("type at any time; Ctrl-C to stop — I'll save first and let go of the lock.");
    // Atlas's icon by the clock (Windows): with no window open, how you know
    // it's running and where you open, pause or quit it (Eric, 28 Sep 2026).
    // Held until the run loop ends; dropping it takes the icon away.
    let _tray = if cfg!(windows) && tc.desktop.tray_icon {
        match std::env::current_exe() {
            Ok(exe) => match atlas::notifyicon::show_icon(exe, hub_address.clone().unwrap_or_default()) {
                Ok(icon) => Some(icon),
                Err(e) => {
                    println!("(no icon by the clock: {e})");
                    d.log.warn(&format!("no icon by the clock: {e}"));
                    None
                }
            },
            Err(_) => None,
        }
    } else {
        None
    };
    // A panic that gets past every `crash::caught` inside the loop used to
    // end the background Atlas with nothing said and nothing to bring it
    // back until the next sign-in -- and with its lock still held, so "Open
    // Atlas" couldn't start another for two and a half minutes either. Now
    // the way out still runs (state, helpers, the lock) and Atlas starts
    // itself again, unless it keeps crashing (`crash::may_start_again`). The
    // crash note is already written; the new start says it.
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        d.run(&voice, &voice, &keyboard, &mut throttle, audio_ok, &atlas::store::now)
    }));
    if ran.is_err() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| d.shut_down()));
        drop(_tray);
        if atlas::crash::may_start_again(&atlas::roots::state_dir(), atlas::store::now()) {
            if let Ok(exe) = std::env::current_exe() {
                // The same way it was started (`--unattended` stays).
                let args: Vec<String> = std::env::args().skip(1).collect();
                let args: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
                if let Ok(child) = atlas::firstlaunch::spawn_quietly(&exe, &args) {
                    atlas::unwaited::dont_wait(child);
                }
            }
        }
        // Not `leave`: a crash counts against a build on trial.
        std::process::exit(101);
    }
}

/// Which microphone Atlas listens with, picked from what this machine
/// actually has rather than the guess in `tools.yaml` (`mic_device`).
///
/// One place for both doors (29 Sep 2026). This lived inside `voice_loop`
/// only, so the background Atlas -- what setup starts and what Eric runs --
/// recorded from the shipped "Microphone Array (Realtek(R) Audio)" on a
/// laptop whose microphone is Intel Smart Sound. Every wake-word clip
/// failed, Atlas dropped to push-to-talk, and the held key recorded nothing
/// and said nothing.
pub(super) fn pick_the_microphone(cfg: &Config, plat: &dyn Platform, tc: &atlas::voice::ToolsConfig) -> atlas::voice::ToolsConfig {
    // Pick the mic from what is actually there, rather than trusting the
    // guess sitting in tools.yaml -- that guess is what sent someone chasing
    // a Realtek device name on a laptop with Intel audio and a shut lid.
    let mut tc_owned = tc.clone();
    match atlas::audio::probe_devices("ffmpeg") {
        Ok(devices) => {
            // The laptop's own screen, asked of Windows directly (29 Sep 2026:
            // "is a screen in the laptop role" was true whenever any screen
            // existed, so a shut lid never counted as shut and the mic under
            // it stayed a candidate). The role guess stands where that can't
            // be told.
            let laptop_active = plat.built_in_screen_on().unwrap_or_else(|| plat
                .monitors()
                .map(|monitors| {
                    let roles = atlas::layout::resolve_roles(&cfg.layouts, &monitors);
                    atlas::layout::monitor_for_role(&cfg.layouts, &roles, "laptop").is_ok()
                })
                // No monitors readable is not evidence the lid is down --
                // default to trusting the built-in mic rather than silently
                // excluding it on a guess.
                .unwrap_or(true));
            // Which ear to listen with, measured rather than guessed.
            //
            // `audio::choose` picks on the device's *name* -- is it built in,
            // does it look like a headset. `hearing.rs` picks on whether the
            // microphone can actually hear you, by recording a second from
            // each and reading the level back, and it remembers the answer
            // across runs so the measurement happens rarely. It also refuses
            // to switch ears mid-conversation over a flicker, which the
            // name-based pick had no concept of.
            //
            // It was complete and tested and nothing called it. Part of why
            // is that it needs a device list, and device listing only worked
            // on Windows until earlier today.
            let hstore = atlas::roots::store();
            let mut hearing = atlas::hearing::Hearing::load_from(&hstore);
            hearing.observe_devices(&devices);
            // What your voice has measured on each one, over its room
            // (`leveller`): the better judge, once known (30 Sep 2026).
            hearing.learn_levels(&atlas::leveller::remembered());

            if hearing.needs_calibration(&tc.hearing, atlas::store::now()) {
                println!("Checking which microphone actually hears you...");
                // Every microphone listened to at once rather than one after
                // another (Eric, H13e: "it has to be quick") -- one second in
                // all, however many there are.
                let listening: Vec<(String, std::thread::JoinHandle<Option<f32>>)> = hearing
                    .candidates
                    .clone()
                    .into_iter()
                    .map(|cand| {
                        let args = atlas::hearing::calibration_args(&atlas::audio::ffmpeg_name_for(&devices, &cand.name), 1);
                        let h = std::thread::spawn(move || {
                            atlas::tools::command("ffmpeg")
                                .args(&args)
                                .stdin(std::process::Stdio::null())
                                .output()
                                .ok()
                                .and_then(|o| atlas::hearing::mean_volume(&String::from_utf8_lossy(&o.stderr)))
                        });
                        (cand.name, h)
                    })
                    .collect();
                for (name, h) in listening {
                    let measured = h.join().ok().flatten();
                    match measured {
                        Some(db) => hearing.record_level(&name, db, atlas::store::now()),
                        // A device that cannot be opened is not a device that
                        // is quiet, and recording it as quiet would rule it
                        // out permanently. Left unmeasured instead.
                        None => eprintln!("  (couldn't measure {})", atlas::hearing::short(&name)),
                    }
                }
            }

            // What Atlas actually knows about where you are. Every field
            // here is a real signal or an admitted absence -- nothing is
            // invented to make the decision look better informed.
            let presence_readable = plat.monitors().is_ok();
            // At the desk means a screen is on here: the laptop's own, or
            // the monitors it's plugged into (29 Sep 2026). "The laptop's
            // screen is on" alone read Eric -- lid closed behind two
            // monitors, AirPods connected -- as away from the desk, and away
            // with a headset means listening through the AirPods, which
            // drops them to call-quality sound for everything.
            let screens_on = plat.monitors().map(|m| !m.is_empty()).unwrap_or(false);
            let whereabouts = atlas::hearing::Where {
                at_desk: laptop_active || screens_on,
                presence_unknown: !presence_readable,
                headset_connected: hearing.candidates.iter().any(|c| c.bluetooth),
                // No signal for either of these yet: nothing reports whether
                // the phone is in use, and nothing watches playback. False
                // rather than a guess -- `Where`'s own `presence_unknown`
                // field exists because this module already knew the
                // difference between "no" and "don't know", and inventing a
                // yes here would be the worse error.
                phone_active: false,
                audio_playing: false,
            };

            // The same pick the running Atlas makes again later
            // (`hearing::pick_microphone`, 29 Sep 2026).
            match atlas::hearing::pick_microphone(&devices, &mut hearing, tc, &whereabouts, laptop_active, atlas::store::now()) {
                Some(p) => {
                    tc_owned.vars.insert("mic_name".into(), p.name.clone());
                    tc_owned.vars.insert("mic_device".into(), p.device.clone());
                    println!("Using \"{}\" -- {}.", atlas::hearing::short(&p.name), p.why);
                    if p.costs_quality {
                        println!("  (that one costs audio quality while it listens.)");
                    }
                }
                None => eprintln!("(no usable microphone found on this machine)"),
            }
            for deaf in hearing.deaf_devices(&tc.hearing) {
                println!("  ({} can't hear you from here.)", atlas::hearing::short(&deaf.name));
            }
            let _ = hearing.save_to(&hstore);
        }
        Err(e) => {
            eprintln!("(couldn't list audio devices, using what's in tools.yaml: {e})");
        }
    }
    // The camera this machine really has, not the shipped guess (29 Sep 2026).
    let configured = tc_owned.vars.get("webcam_device").cloned().unwrap_or_default();
    // The one pointed at you: matching the microphone that hears you, and
    // never the one inside a shut lid (30 Sep 2026).
    let mic_now = tc_owned.vars.get("mic_name").cloned().unwrap_or_default();
    let lid_open = plat.built_in_screen_on().unwrap_or(true);
    if let Some(cam) = atlas::audio::pick_camera_for(&atlas::audio::probe_cameras("ffmpeg"), &configured, &mic_now, lid_open) {
        if cam != configured {
            println!("Camera: \"{cam}\".");
            tc_owned.vars.insert("webcam_device".into(), cam);
        }
    }
    tc_owned
}

pub(super) fn voice_loop(
    cfg: &Config,
    plat: &dyn Platform,
    parser: &Parser,
    approver: &dyn Approver,
    hands_free: bool,
) {
    let Some(tc) = cfg.tools.as_ref() else {
        eprintln!("config/tools.yaml is missing — nothing to talk to.");
        leave(2);
    };
    if !tc.enabled {
        eprintln!("tools.yaml has enabled: false");
        leave(2);
    }
    let tc_owned = pick_the_microphone(cfg, plat, tc);
    // The mic Atlas just picked has to reach the daemon, not only the
    // recorder. `Voice` borrows `tc_owned`; `Daemon::new` borrows a whole
    // `Config`. Handing the daemon the original `cfg` would give it a
    // `tools` block without the `mic_device` var worked out above -- two
    // views of the configuration, one of them stale. So the clone is made
    // at the `Config` level and both read the same one.
    let mut cfg_owned = cfg.clone();
    cfg_owned.tools = Some(tc_owned);
    let tc_live = cfg_owned.tools.as_ref().expect("just set");
    let voice = Voice::new(tc_live);

    // The same single-instance lock `run_daemon` takes, and for the same
    // reason: this door now reads and writes `data/state` too, and two
    // long-running Atlases both loading it, changing it in memory and
    // writing the whole thing back means the second quietly erasing whatever
    // the first learned. Before this, `atlas --voice` touched almost no state,
    // so it did not need the lock; it does now.
    let only = atlas::onlyone::OnlyOne::at(&atlas::roots::data_dir());
    // Patiently: a lock that reads abandoned may be an Atlas that was only
    // asleep with the laptop and is about to beat (`onlyone::WOKE_GRACE_SECS`).
    match only.take_patiently(std::time::Duration::from_secs(atlas::onlyone::WOKE_GRACE_SECS), &atlas::store::now) {
        Err(why) => {
            eprintln!("{why}");
            atlas::firstlaunch::note_start_problem(&atlas::roots::install_root(), &why);
            leave(1);
        }
        Ok(found) => {
            if !matches!(found, atlas::onlyone::Found::Free) {
                println!("{}", found.plain());
            }
        }
    }

    // The real door, at last.
    //
    // `atlas --voice` went to `handle`, which covers **five** of the 56
    // `Intent` variants and answers everything else with "not wired to an
    // action yet". `Daemon::execute` has 89 `Intent::` arms. So the command
    // whose name most obviously means "talk to Atlas" was a legacy stub
    // answering about a tenth of what Atlas can do -- while `prompt_line`,
    // the *typed* door, had been using the daemon for weeks. Nothing said so
    // at the point someone would type the wrong one.
    //
    // `handle` stays, as `prompt_line` documents it: the fallback for a
    // machine with no `config/tools.yaml`. It is unreachable from here,
    // because this function has already exited if `tools` is missing.
    let llm = model_connection(tc_live);
    let mut d = Daemon::new(
        &cfg_owned,
        plat,
        llm,
        atlas::roots::store(),
        Proactive::new(tc_live.proactive.clone()),
    )
    .starting_the_model_server()
    .with_typed_prompt(Box::new(atlas::typed::Console))
    .watch_settings(atlas::roots::config_dir());
    d.autonomy = Autonomy::Supervised;

    if hands_free {
        match tc_live.wake.as_ref() {
            Some(w) if w.enabled => println!("listening for \"{}\". Ctrl-C to stop.", w.phrase),
            _ => {
                eprintln!("wake is not enabled in tools.yaml — set wake.enabled: true");
                leave(2);
            }
        }
    } else {
        println!("voice loop. press Enter to talk, Ctrl-C to stop.");
    }
    loop {
        if hands_free {
            if let Err(e) = voice.wait_for_wake() {
                eprintln!("(wake failed: {e})");
                return;
            }
            println!("[heard wake word]");
        } else {
            print!("[enter to listen] ");
            let _ = io::stdout().flush();
            let mut l = String::new();
            // No keyboard at all is the end, not a press of Enter: read as
            // Enter, the loop recorded and acted on clip after clip with
            // nobody there (29 Sep 2026).
            if matches!(io::stdin().read_line(&mut l), Ok(0) | Err(_)) {
                return;
            }
        }
        match voice.listen() {
            Ok(heard) => {
                println!("heard: {heard}");
                // The same two steps `prompt_line` takes, in the same order:
                // parse, gate, then let the daemon do it. Running the gate
                // here as well as inside the daemon means a gated action is
                // refused by whichever is stricter, which is the direction an
                // error should go.
                let intent = parser.parse(&heard);
                let reply = match gate_with_identity(&cfg_owned, &intent, approver) {
                    Err(e) => format!("{e}"),
                    Ok(()) => d.execute_timed(&intent, &heard),
                };
                println!("{reply}");
                if let Err(e) = voice.speak(&reply) {
                    eprintln!("(tts failed: {e})");
                }
            }
            Err(e) => eprintln!("(listen failed: {e})"),
        }
    }
}

/// One turn. Returns the line to print and speak — kept short on purpose,
/// because a spoken confirmation that runs three sentences is unusable.
pub(super) fn handle(
    cfg: &Config,
    plat: &dyn Platform,
    parser: &Parser,
    approver: &dyn Approver,
    line: &str,
) -> String {
    // Known phrases resolve instantly and cannot be misread. Only speech that
    // matches nothing goes to the model, and only if one is configured.
    // The same connection every other door uses -- a hand-written `llm:`,
    // or the model Atlas runs itself -- rather than `llm:` alone, which since
    // 26 Sep 2026 ships unset.
    let conn = cfg.tools.as_ref().and_then(|t| model_connection(t));
    let decision = match conn {
        Some(llm) => {
            // The CLI path gets the character too. It is the same Atlas --
            // there is no reason the one-shot command line should sound like
            // a different program from the one you speak to. No thread to read the
            // moment from, so the register is worked out from the sentence
            // alone.
            let persona = cfg.tools.as_ref().map(|t| t.persona.clone()).unwrap_or_default();
            let register = atlas::register::read(line, &atlas::register::Moment::default());
            Brain { llm: llm.as_ref(), fallback: parser, voice: Some((&persona, register)) }
                .decide(line, &brain::context(cfg, plat))
        }
        None => {
            let i = parser.parse(line);
            let say = brain::default_say(&i);
            brain::Decision { intent: i, say, model: brain::Reached::NotNeeded }
        }
    };
    let intent = decision.intent;

    // Not understood, with no model to ask: said, not "blocked".
    if matches!(intent, Intent::Unknown(_)) {
        return "I can't answer that here — general questions need my language model, and there isn't one \
                running. Start Atlas again and its setup fetches it (about 3 GB)."
            .into();
    }
    if let Err(e) = gate_with_identity(cfg, &intent, approver) {
        return format!("{e}");
    }

    match &intent {
        Intent::WorkspaceOn => describe(workspace::workspace_on(cfg, plat), "workspace online"),
        Intent::WorkspaceOff => describe(workspace::workspace_off(cfg, plat), "workspace down"),
        Intent::ViewDisplay => look(cfg, "screen", line),
        Intent::CaptureWebcam => look(cfg, "webcam", line),
        Intent::Say(_) | Intent::Ask(_) => decision.say,
        // This is the no-`tools.yaml` fallback only — every other door
        // builds a real daemon now. It still has to answer in English.
        other => format!(
            "I understood that as {} — but without config/tools.yaml I can only \
             start and stop your workspace, look at the screen or camera, and talk.",
            other.plain()
        ),
    }
}

/// Capture, then actually look at it if a vision model is configured.
pub(super) fn look(cfg: &Config, what: &str, question: &str) -> String {
    let Some(tc) = cfg.tools.as_ref() else {
        return "no tools.yaml, cannot capture".into();
    };
    let v = Voice::new(tc);
    let shot = if what == "webcam" { v.capture_webcam() } else { v.capture_screen() };
    let path = match shot {
        Ok(p) => p,
        Err(e) => return format!("capture failed: {e}"),
    };
    match tc.llm.as_ref() {
        Some(lc) if lc.vision_request.is_some() => {
            let llm = ShellLlm { cfg: lc.clone(), vars: tc.vars.clone() };
            match llm.look(&path, question) {
                Ok(answer) => answer,
                Err(e) => format!("captured {path}, but couldn't look at it: {e}"),
            }
        }
        _ => format!("captured {path}. No vision model configured, so I can't see it yet."),
    }
}

pub(super) fn describe(r: atlas::error::Result<workspace::Report>, ok_msg: &str) -> String {
    match r {
        Ok(r) if r.ok() => format!("{ok_msg}."),
        Ok(r) => {
            let fails: Vec<String> = r.failed.iter().map(|(a, w)| format!("{a} ({w})")).collect();
            format!("partial — placed {}; failed {}", r.placed.join(", "), fails.join(", "))
        }
        Err(e) => format!("error: {e}"),
    }
}

/// Replaced by whatever `atlas doctor` prints on the target laptop.
pub(super) fn fake_monitors() -> Vec<Monitor> {
    // A generic two-monitor desk, used only for dry runs before `atlas setup`
    // (or `atlas firstrun`, the same thing)
    // has seen the real machine. Nobody's actual monitor ids belong here.
    vec![
        Monitor { id: 1, x: 0,    y: 0, width: 2560, height: 1392, primary: true },
        Monitor { id: 2, x: 2560, y: 0, width: 2560, height: 1392, primary: false },
    ]
}

/// The hub, and nothing else.
///
/// Deliberately the smallest possible amount of Atlas: bind the loopback
/// listener, serve pages, apply setting changes. If this can't start, nothing
/// else was going to either, and it says why.

/// What each dashboard card has to show in settings-only mode.
///
/// Named one at a time rather than left blank. A card that renders an empty
/// box because the daemon isn't running looks exactly like a card reporting
/// that nothing is wrong, and that confusion is the whole reason `hollow.rs`
/// exists. Each says which thing it is waiting on.
pub(super) fn dash_bodies() -> Vec<(atlas::dash::Card, String)> {
    use atlas::dash::Card;
    Card::all()
        .into_iter()
        .map(|c| {
            let why = match c {
                Card::Machine => "Reads memory and disk while Atlas is running.",
                Card::Outstanding | Card::Stuck | Card::Today => {
                    "Comes from the running daemon's own list."
                }
                Card::Activity => "Comes from the journal the daemon writes.",
                Card::Connections => "Checked live, not from a cache.",
                Card::Ideas => "Written by the nightly self-audit.",
                Card::Trust => "Built from how Atlas's own work turns out.",
                Card::Handed => "Filled by anything you send from another device.",
                Card::Projects => "Read from the projects the running Atlas keeps.",
            };
            (
                c,
                format!(
                    "<p class=note>Nothing here yet — this is settings-only mode. {why}</p>"
                ),
            )
        })
        .collect()
}
