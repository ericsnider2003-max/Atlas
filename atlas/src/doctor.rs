//! `atlas doctor` — find out what is actually on this machine.
//!
//! This exists because every path in the original spec set was a guess that
//! nobody could check. Instead of guessing harder, ask the machine: enumerate
//! the real monitors, hunt for the real executables, probe for the real tools,
//! then print config you can paste.

use crate::config::Config;
use crate::platform::{Monitor, Platform};
use crate::tools::{which, Vars};
use crate::voice::ToolsConfig;
use std::path::{Path, PathBuf};

pub struct Finding {
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

pub fn run(cfg: &Config, tools: Option<&ToolsConfig>, plat: &dyn Platform) -> Vec<Finding> {
    let mut f = Vec::new();

    // --- which install am I? ---
    //
    // First, because every other line below is relative to it and because
    // this is the question that had no answer. Everything Atlas remembered
    // used to hang off the current working directory, so a shortcut with the
    // wrong "Start in", Task Scheduler, or `atlas update` run from anywhere
    // brought Atlas up with an empty `data/state` and said nothing at all.
    // "Atlas started with no memory" and "Atlas started with your memory"
    // looked identical from the outside. Now it says which folder and why.
    let fresh = crate::roots::first_run_here();
    // Matched rather than printed, because the four answers send you to four
    // different places.
    let advice = match crate::roots::how() {
        crate::roots::Chosen::Told => {
            "ATLAS_HOME is set, so this is where I look no matter where you start me from."
        }
        crate::roots::Chosen::BesideTheProgram => {
            "Found beside atlas.exe, which is the arrangement that survives a \
             shortcut, Task Scheduler and `atlas update`."
        }
        crate::roots::Chosen::AboveTheProgram => {
            "This is a build tree, not an install -- atlas.exe is somewhere \
             under target/. Fine for development; set ATLAS_HOME if you meant \
             a real install."
        }
        crate::roots::Chosen::WhereYouAreStanding => {
            "The folder you started me from holds an install and atlas.exe's \
             own folder does not. That works, but it means where you start me \
             from decides what I remember -- put atlas.exe in this folder, or \
             set ATLAS_HOME."
        }
        crate::roots::Chosen::FreshBesideTheProgram => {
            "Nothing here looks like an install yet."
        }
    };
    f.push(Finding {
        label: "install".into(),
        // A fresh start is not a fault -- it is what a first run looks like.
        // It is only a fault if you were not expecting one, which is exactly
        // why it is worth a line.
        ok: !fresh,
        detail: if fresh {
            format!(
                "{} -- nothing here looks like an install yet, so this run \
                 starts one. If you expected your notes and settings to be \
                 here, you are in the wrong folder: set ATLAS_HOME, or start \
                 Atlas from the folder holding atlas.exe.",
                crate::roots::install_root().display()
            )
        } else {
            format!("{} -- {advice}", crate::roots::install_root().display())
        },
    });

    // --- monitors ---
    match plat.monitors() {
        Ok(m) if m.is_empty() => f.push(Finding {
            label: "monitors".into(),
            ok: false,
            detail: "none detected".into(),
        }),
        Ok(m) => {
            for mon in &m {
                f.push(Finding {
                    label: format!("monitor {}", mon.id),
                    ok: true,
                    detail: format!(
                        "{}x{} at ({},{}){}",
                        mon.width,
                        mon.height,
                        mon.x,
                        mon.y,
                        if mon.primary { " [primary]" } else { "" }
                    ),
                });
            }
            let roles = crate::layout::resolve_roles(&cfg.layouts, &m);
            for (role, mon) in &roles {
                f.push(Finding {
                    label: format!("role '{role}'"),
                    ok: true,
                    detail: format!("-> monitor {} at x={}", mon.id, mon.x),
                });
            }
        }
        Err(e) => f.push(Finding {
            label: "monitors".into(),
            ok: false,
            detail: e.to_string(),
        }),
    }

    // --- app executables ---
    let roots = search_roots();
    for (name, spec) in &cfg.apps.apps {
        let configured = expand_env(&spec.launch);
        // A Store app is launched by id through the shell. Checking an app id
        // against the filesystem will always fail, and that failure means
        // nothing.
        if spec.store {
            let known = store_package(name).is_some();
            f.push(Finding {
                label: format!("app '{name}'"),
                ok: known,
                detail: if known {
                    format!("{configured} (Microsoft Store app)")
                } else {
                    format!("{configured} — configured as a Store app, but no matching package is installed")
                },
            });
            continue;
        }
        if Path::new(&configured).is_file() || which(&configured).is_some() {
            f.push(Finding {
                label: format!("app '{name}'"),
                ok: true,
                detail: configured,
            });
            continue;
        }
        let target = spec
            .process_names
            .first()
            .cloned()
            .unwrap_or_else(|| format!("{name}.exe"));

        // Exact filename first, then anything containing the app's name.
        // Installers are inconsistent — an app can ship as Claude.exe,
        // AnthropicClaude.exe, or a Store alias — and "not found" when the
        // thing is plainly on the taskbar is a useless answer.
        let hits = find_exes(&roots, &target, name);
        match hits.split_first() {
            Some((best, rest)) => {
                let extra = if rest.is_empty() {
                    String::new()
                } else {
                    format!(" (also: {})", rest.iter().take(2).cloned().collect::<Vec<_>>().join(", "))
                };
                f.push(Finding {
                    label: format!("app '{name}'"),
                    ok: false,
                    detail: format!("configured path is wrong. FOUND INSTEAD: {best}{extra}"),
                })
            }
            None => f.push(Finding {
                label: format!("app '{name}'"),
                ok: false,
                detail: format!(
                    "not at {configured}, and nothing matching '{name}' under {} search roots.                      If it's on your taskbar: right-click the icon, right-click its name,                      Properties, and send me the Target field.",
                    roots.len()
                ),
            }),
        }
    }

    // --- external tools ---
    if let Some(t) = tools {
        let vars: Vars = t.vars.clone();
        for (label, tool) in [
            ("record (mic)", &t.record),
            ("stt", &t.stt),
            ("tts", &t.tts),
            ("play", &t.play),
        ] {
            let (cmd, _) = tool.resolved(&vars);
            match which(&cmd) {
                Some(p) => f.push(Finding { label: label.into(), ok: true, detail: p }),
                None => f.push(Finding {
                    label: label.into(),
                    ok: false,
                    detail: format!("'{cmd}' not on PATH"),
                }),
            }
        }
        // Model files are the other half of a working voice loop.
        for (k, v) in &t.vars {
            if !(k.ends_with("_model") || k.ends_with("model")) {
                continue;
            }
            // Some "models" are files on disk; others are tags a runtime
            // resolves itself (Ollama's "llama3.1:8b"). Checking the second
            // kind against the filesystem always reports a failure that
            // isn't one.
            if !looks_like_a_path(v) {
                f.push(Finding {
                    label: format!("model '{k}'"),
                    ok: true,
                    detail: format!("{v} (resolved by the runtime, not a file)"),
                });
                continue;
            }
            // `stt_model: "models/ggml-base.en.bin"` and
            // `whisper: "tools/whisper/whisper-cli.exe"` are install-relative
            // by declaration. Stat'd bare, every one of them reported missing
            // from any other folder.
            let exists = crate::roots::under_install(v).is_file();
            f.push(Finding {
                label: format!("model '{k}'"),
                ok: exists,
                detail: if exists { v.clone() } else { format!("missing: {v}") },
            });
        }
    } else {
        f.push(Finding {
            label: "tools.yaml".into(),
            ok: false,
            detail: "not loaded — voice and screen capture are unavailable".into(),
        });
    }

    // Can Atlas reach you when you are not at the desk?
    //
    // Reported rather than assumed. A machine with no notifier still works —
    // everything is held and spoken when you return — but that is a different
    // system from the one you think you have, and finding out by missing
    // something is the wrong way to learn it.
    // Checked by looking for the binary, not by looking at the config. A
    // configured command that is not installed is the same as no notifier,
    // and reporting "ok" for it would be a check that cannot fail — which is
    // the failure mode this whole file exists to avoid.
    let n = tools.map(|t| t.notify.clone()).unwrap_or_default();
    let vars = tools.map(|t| t.vars.clone()).unwrap_or_default();
    let (ok, detail) = match (&n.enabled, n.tool.as_ref()) {
        (false, _) => (false, "notifications are switched off in your settings".to_string()),
        // No external command is the shipped default now that PowerShell is
        // gone: Atlas draws its own panel. That is working, not missing — this
        // reported FAIL for the normal healthy configuration until the window
        // existed to back it up.
        (true, None) if crate::window::can_open() => {
            (true, "on-screen alerts in Atlas's own window".to_string())
        }
        (true, None) => (
            false,
            format!("{} There's no display here to draw one on.", crate::notify::NO_NOTIFIER),
        ),
        (true, Some(t)) if t.available(&vars) => {
            (true, format!("on-screen alerts via {}", t.command))
        }
        (true, Some(t)) => (
            false,
            format!(
                "'{}' is configured for on-screen alerts but isn't installed here. \
                 Nothing is lost — anything that happens while you're away is held \
                 and said when you're back — but you won't hear about it until then.",
                t.command
            ),
        ),
    };
    f.push(Finding { label: "notify".into(), ok, detail });

    // Can Atlas see?
    //
    // Checked by looking for the model files, not by reading the setting.
    // "Seeing is switched on" and "seeing can happen" are different claims,
    // and reporting the first as the second is a check that cannot fail —
    // which is the failure this whole file exists to avoid.
    let v = tools.map(|t| t.vision.clone()).unwrap_or_default();
    // Through the same resolver the runtime uses. Read bare, this was the
    // config's relative `"models"`, so `atlas doctor` run from anywhere but
    // the install folder reported every vision model missing when they were
    // all present -- a check that fails for a reason that has nothing to do
    // with what it is checking.
    let models = crate::models::Registry::dir_for(&tools.map(|t| t.models.clone()).unwrap_or_default());
    let seeing_missing = crate::infer::whats_missing(&models, &crate::infer::Kind::for_seeing());
    let (vok, vdetail) = match (v.enabled, seeing_missing.len()) {
        (false, _) => (
            false,
            "recognising what it sees is switched off in your settings".to_string(),
        ),
        (true, 0) => (
            true,
            "faces, things, and anything you've shown it".to_string(),
        ),
        // Some of it. Said as what still works rather than as a failure,
        // because three models out of four is a feature that half runs and
        // the half that runs is worth having.
        (true, n) if n < crate::infer::Kind::for_seeing().len() => (
            true,
            format!(
                "partly — {}",
                crate::infer::spoken(&seeing_missing).to_lowercase()
            ),
        ),
        (true, _) => (false, crate::infer::spoken(&seeing_missing)),
    };
    f.push(Finding { label: "vision".into(), ok: vok, detail: vdetail });

    // Reading the screen. A third separate install, and the one that used to
    // report itself as "waiting on tesseract" — a wait that could never end,
    // because nothing in Atlas ever fetched tesseract.
    let reading_missing = crate::infer::whats_missing(&models, &crate::infer::Kind::for_reading());
    f.push(Finding {
        label: "reading".into(),
        ok: reading_missing.is_empty(),
        detail: if reading_missing.is_empty() {
            "reading words off the screen needs nothing else installed".to_string()
        } else {
            crate::infer::spoken(&reading_missing)
        },
    });

    // And whether the hand models are there, which is a separate install and
    // used to be reported only by the feature refusing to start.
    let hands_missing = crate::infer::whats_missing(&models, &crate::infer::Kind::for_hands());
    f.push(Finding {
        label: "hands".into(),
        ok: hands_missing.is_empty(),
        detail: if hands_missing.is_empty() {
            "hand tracking has what it needs".to_string()
        } else {
            crate::infer::spoken(&hands_missing)
        },
    });

    // Can Atlas answer anything it has no phrase for? That's the language
    // model, and on a friend's machine (27 Sep 2026) it was the one thing
    // nothing here reported: every question got "I can't answer that here"
    // and this list said nothing about why.
    let (lok, ldetail) = match tools {
        Some(t) if t.llm.is_some() => (true, "using the model connection written in your settings".to_string()),
        _ => {
            let mcfg = tools.map(|t| t.models.clone()).unwrap_or_default();
            let (registry, _) = crate::models::Registry::scan_reporting(&models);
            let model = registry.choose_for(&mcfg, u64::MAX).map(|m| m.id.clone());
            let server = crate::models::server_tool(&mcfg)
                .map(|t| Path::new(&t.command).is_file() || which(&t.command).is_some())
                .unwrap_or(false);
            match (model, server) {
                (Some(m), true) => (true, format!("{m}, started when it's needed")),
                (Some(m), false) => (
                    false,
                    format!("{m} is here but the program that runs it (llama-server) isn't — start Atlas again and its setup fetches it"),
                ),
                (None, _) => (
                    false,
                    "no language model yet, so only the things on the command list work — start Atlas again \
                     and its setup fetches it (about 3 GB)"
                        .to_string(),
                ),
            }
        }
    };
    f.push(Finding { label: "talking".into(), ok: lok, detail: ldetail });

    // And whether you can be reached away from the machine at all.
    let ph = tools.map(|t| t.phone.clone()).unwrap_or_default();
    let (pok, pdetail) = match crate::phone::configured(&ph) {
        Ok(()) => (true, format!("phone alerts to {}{}", ph.host, ph.path)),
        // Not being set up is not a fault -- most people won't want this --
        // so it reports what happens instead rather than reading as broken.
        Err(crate::phone::NotSet::Disabled) => (true, crate::phone::NO_PHONE.to_string()),
        Err(e) => (false, e.plain()),
    };
    f.push(Finding { label: "phone".into(), ok: pok, detail: pdetail });

    // Can Atlas tell your voice from anyone else's?
    //
    // Reported as three separate states, because they need different things
    // done about them and one "voice-lock: off" line would hide which.
    // Crucially, an unavailable voice-lock is never shown as a failure: it is
    // how Atlas has always behaved, and dressing it up as broken would train
    // you to ignore the line that matters.
    let sp = tools.map(|t| t.speaker.clone()).unwrap_or_default();
    let vid = tools.map(|t| t.voice_id.clone()).unwrap_or_default();
    let voice_record = crate::voiceid::VoiceId::load(&crate::roots::store());
    let enrolled = voice_record.enrolled();
    let builtin = crate::speaker::which(&sp, &vars) == crate::speaker::Encoder::BuiltIn;
    let (vok, vdetail) = if !crate::speaker::available(&sp, &vars) {
        (
            true,
            format!(
                "{} {}",
                crate::speaker::NO_ENCODER,
                crate::speaker::still_learning(&crate::speaker::background(&crate::roots::store()))
            ),
        )
    } else if enrolled < vid.min_samples {
        (
            true,
            format!(
                "an encoder is installed but I haven't learned your voice yet \
                 ({enrolled} of {} samples). Run `atlas enrol-voice`.",
                vid.min_samples
            ),
        )
    } else if !vid.enabled {
        (true, "I know your voice, but voice-lock is switched off in your settings".into())
    } else {
        // With the lock on and in use, say where the fixed thresholds sit
        // against this voice's own accepted scores — the evidence the
        // open adaptive-threshold ruling needs, gathered where the person
        // deciding will actually see it.
        match voice_record.thresholds_report(&vid) {
            Some(report) => {
                (true, format!("voice-lock on, {enrolled} samples enrolled; {report}"))
            }
            None => (true, format!("voice-lock on, {enrolled} samples enrolled")),
        }
    };
    let vdetail = if builtin && crate::speaker::available(&sp, &vars) {
        format!("{vdetail} (Atlas's own encoder: classical, weaker than a trained one)")
    } else {
        vdetail
    };
    f.push(Finding { label: "voice-lock".into(), ok: vok, detail: vdetail });

    // The vault on your sign-in (`loginseal`): off, on and made, or on and
    // waiting for the next passphrase unlock to make the copy.
    let vcfg = tools.map(|t| t.vault.clone()).unwrap_or_default();
    let sealed = crate::vault::Vault::load(&crate::roots::install_state()).sealed_to_this_login();
    let (sok, sdetail) = if !vcfg.open_on_this_login {
        (true, "off — the vault opens only with your passphrase or recovery key, so scheduled mail checks wait for you".to_string())
    } else if !crate::loginseal::available() {
        (false, crate::loginseal::NOT_HERE.to_string())
    } else if sealed {
        (true, "sealed to your Windows sign-in: scheduled mail checks can open logins and API keys while you're signed in".to_string())
    } else {
        (true, "switched on; the sign-in copy is made the next time you unlock with your passphrase".to_string())
    };
    f.push(Finding { label: "vault on sign-in".into(), ok: sok, detail: sdetail });

    // --- a typed passphrase stays off the screen ---
    //
    // On Windows this really switches the console's echo off and back on to
    // find out (round 10: the in-process read replaced a PowerShell pipe).
    let (hok, how) = crate::typed::how_typing_is_hidden();
    f.push(Finding { label: "typed passphrase".into(), ok: hok, detail: how.to_string() });

    // Can recall search by meaning, or only by words?
    //
    // Three states for the same reason voice-lock gets three: they need
    // different things done. Off is a choice and reads as one; on with an
    // encoder is working; on WITHOUT an encoder is the gap `recall.semantic`'s
    // own config comment warns about, and this is the line that names it
    // rather than leaving word-only results to be mistaken for the search
    // not working. Like voice-lock, none of these is a failure — word search
    // is complete on its own.
    let rc = tools.map(|t| t.recall.clone()).unwrap_or_default();
    let mc = tools.map(|t| t.meaning.clone()).unwrap_or_default();
    let (mok, mdetail) = if !crate::recall::needs_a_model(&rc) {
        (true, "searching notes by words; meaning search is switched off in your settings".into())
    } else if !crate::meaning::available(&mc, &vars) {
        (true, crate::meaning::NO_ENCODER.to_string())
    } else {
        (true, "meaning search is on and the encoder is installed".into())
    };
    f.push(Finding { label: "meaning-search".into(), ok: mok, detail: mdetail });

    // Your own server over WireGuard: is the tunnel laid out, and do the model
    // slots stay inside the fence? Doc 13 §1 — personal Atlas uses the server
    // for models only. The server's firewall holds that line; this catches a
    // setting on this side that could only ever be refused, and names it.
    let mesh = tools.map(|t| t.mesh.clone()).unwrap_or_default();
    let models_cfg = tools.map(|t| t.models.clone()).unwrap_or_default();
    let listen_problem = (!models_cfg.listen_on.trim().is_empty())
        .then(|| crate::server::bind_address(&models_cfg.listen_on).err())
        .flatten();
    if crate::mesh::Mesh::from_setting(&mesh.kind) == Some(crate::mesh::Mesh::Wireguard)
        || listen_problem.is_some()
    {
        let (ok, detail) = match crate::wireguard::Plan::from_config(&mesh.wireguard) {
            Err(e) => (false, format!("the tunnel can't be laid out: {e}")),
            Ok(plan) => {
                let mut problems: Vec<String> = Vec::new();
                if let Some(e) = listen_problem {
                    problems.push(format!(
                        "models.listen_on: {e} — the model server stays on this machine only"
                    ));
                }
                let slots = [
                    ("your model", tools.and_then(|t| t.llm.as_ref())),
                    ("your second model", tools.and_then(|t| t.llm_secondary.as_ref())),
                ];
                for (slot, llm) in slots {
                    if let Some(l) = llm {
                        if let Some(p) = crate::wireguard::model_door_problem(
                            &plan,
                            slot,
                            &l.tool.command,
                            &l.tool.args,
                        ) {
                            problems.push(p);
                        }
                    }
                }
                if !problems.is_empty() {
                    (false, problems.join("; "))
                } else if plan.endpoint.is_empty() {
                    (
                        true,
                        "WireGuard chosen; I still need the server's outside address \
                         (mesh.wireguard.endpoint) before I can write the configs"
                            .into(),
                    )
                } else {
                    (
                        true,
                        format!(
                            "WireGuard to {}; your devices may reach its model server on port {} \
                             and nothing else",
                            plan.endpoint, plan.model_port
                        ),
                    )
                }
            }
        };
        f.push(Finding { label: "own-server".into(), ok, detail });
    }

    // Which engine is driving speech, and does the config agree with itself?
    //
    // An engine and an executable that disagree is a fault that only ever
    // shows up as silence, which is the hardest kind to chase. `tts.rs` had
    // `is_consistent` for exactly this and nothing called it.
    let eng = tools.map(|t| t.tts_engine.clone()).unwrap_or_default();
    let vset = tools.map(|t| t.voice_settings.clone()).unwrap_or_default();
    let voice_file = eng.voice_file_for(&vset.voice);
    let have_voice = std::path::Path::new(&voice_file).is_file();
    // The half that was missing: whether the engine's *program* is there at
    // all. Only the voice file was checked, so a shipped config naming
    // `tools/kokoro/speak.cmd` — which no installer downloads and which
    // tools.yaml itself says you must write by hand — reported nothing
    // wrong, and Atlas was simply silent.
    let exe_path = crate::roots::under_install(&eng.exe);
    let have_exe = exe_path.is_file();
    // Kokoro is spoken inside Atlas since 28 Sep 2026 (`kokoro`): no program
    // of its own, just its library and model, downloaded from Sound & voice.
    // Until they're here Atlas speaks in piper, which is said, not silent.
    let kokoro = (eng.engine == crate::tts::Engine::Kokoro)
        .then(|| crate::kokoro::check(&crate::roots::install_root()));
    let (eok, edetail) = if let Some(k) = kokoro {
        let (voice, _) = crate::kokoro::voice_or_default(&vset.voice);
        match k {
            Ok(_) => (true, format!("Kokoro, spoken inside Atlas, as {voice}")),
            Err(m) => (false, format!("Kokoro is chosen, but {}; until it is, Atlas speaks in piper", m.plain())),
        }
    } else if !eng.is_consistent() {
        (
            false,
            format!(
                "config says the engine is {} but the executable is '{}' — those disagree, \
                 and that shows up as silence rather than an error",
                eng.engine.name(),
                eng.exe
            ),
        )
    } else if !have_exe {
        (
            false,
            format!(
                "{} is configured, speaking as {}, but {} isn't there. Nothing \
                 installs it — either put it there, or set tts_engine.engine to \
                 one whose program you have.",
                eng.engine.name(),
                vset.voice,
                exe_path.display()
            ),
        )
    } else if !have_voice {
        (false, format!("{}, but the voice file is missing: {voice_file}", eng.engine.name()))
    } else {
        (true, format!("{}, speaking as {}", eng.engine.name(), vset.voice))
    };
    f.push(Finding { label: "speech engine".into(), ok: eok, detail: edetail });

    // What this machine can actually run, and what Atlas decided because of it.
    //
    // `fit.rs` had all of this and nothing ever built a `Machine`, so Atlas
    // ran identically on a 64GB desktop and an 8GB laptop. Reported rather
    // than silent: the two numbers that decide whether Atlas stays out of your
    // way are how many things it does at once and whether it holds a model in
    // memory between turns.
    // What Atlas is holding on disk that nothing refers to.
    //
    // Worth stating what this is *not*: Atlas surveys `models/` and `tools/`,
    // the two folders it installs into, and `data/`, which it generates. It
    // does not tidy the rest of your disk and is not trying to be a disk
    // cleaner. A tool that offers to delete from folders it does not own is a
    // tool you cannot leave running unattended.
    let cfg_text = std::fs::read_to_string(crate::roots::config_file("tools.yaml")).unwrap_or_default();

    // --- settings you changed that went nowhere ---
    //
    // The settings layer exists because every toggle used to report success
    // and write nothing. A preference that names a key `tools.yaml` no longer
    // has is that same failure one layer up, so it gets its own line rather
    // than being dropped during loading.
    if !cfg.settings_that_went_nowhere.is_empty() {
        f.push(Finding {
            label: "settings that didn't apply".into(),
            ok: false,
            detail: format!(
                "you changed {} in Settings, and there is no such setting any more: {}. \
                 They are still in config/settings.yaml doing nothing -- delete those \
                 lines, or say so and I will.",
                if cfg.settings_that_went_nowhere.len() == 1 { "something" } else { "some things" },
                cfg.settings_that_went_nowhere.join(", ")
            ),
        });
    }

    // --- hand edits that didn't apply ---
    //
    // `config/local` holds the edits you made to shipped files, kept out of
    // them so an update cannot undo them. One that lands nowhere is the same
    // silent failure as a setting that does, so it is said out loud.
    if !cfg.edits_that_went_nowhere.is_empty() {
        f.push(Finding {
            label: "your config edits that didn't apply".into(),
            ok: false,
            detail: cfg.edits_that_went_nowhere.join("; "),
        });
    }

    // --- add-ons that are off without you having switched them off ---
    //
    // Changed since you approved it, broken, or written for another Atlas:
    // each is something you'd otherwise only discover by saying its words
    // and getting nothing.
    let addons = crate::plugins::scan(
        &crate::plugins::plugins_dir(),
        &cfg.commands,
        &crate::plugins::Approvals::load(&crate::roots::store()),
    );
    let off: Vec<String> = addons
        .iter()
        .filter(|p| {
            !matches!(
                p.status,
                crate::plugins::Status::Active | crate::plugins::Status::Disabled | crate::plugins::Status::Waiting
            )
        })
        .map(|p| format!("{}: {}", p.name(), p.status.plain()))
        .collect();
    if !off.is_empty() {
        f.push(Finding { label: "add-ons that are off".into(), ok: false, detail: off.join("; ") });
    }
    let waiting = addons.iter().filter(|p| p.status == crate::plugins::Status::Waiting).count();
    if waiting > 0 {
        f.push(Finding {
            label: "add-ons waiting for you".into(),
            ok: true,
            detail: format!(
                "{waiting} add-on{} waiting for your approval -- the hub's Add-ons page shows what each wants",
                if waiting == 1 { " is" } else { "s are" }
            ),
        });
    }

    // --- settings that reach nothing ---
    //
    // Editing one of these changes nothing and, until now, nothing said so.
    // Worth a line of its own because the failure is indistinguishable from
    // the setting not working: you turn `triage.draft_replies` on, nothing
    // happens, and the natural conclusion is that triage is broken rather
    // than that the block was discarded before anything saw it.
    let dead = crate::config::settings_that_do_nothing(&cfg_text);
    if !dead.is_empty() {
        let named: Vec<String> =
            dead.iter().take(6).map(|(k, why)| format!("{k} ({why})")).collect();
        f.push(Finding {
            label: "settings that do nothing".into(),
            ok: false,
            detail: format!(
                "{} section(s) in your tools.yaml reach no code: {}{}",
                dead.len(),
                named.join("; "),
                if dead.len() > 6 { ", and more" } else { "" }
            ),
        });
    }
    // --- what this machine will never do ---
    //
    // `portable.warn_up_front` is "say what won't work here before it's
    // needed rather than after", and there was no `portable:` block for it to
    // arrive in until 19 Sep 2026. Silent on a machine with no walls: a line
    // saying "everything here is possible" teaches people to skip the section
    // it is printed in.
    if let Some(said) = crate::capability::heads_up(
        crate::platform::what_am_i(),
        &tools.map(|t| t.portable.clone()).unwrap_or_default(),
    ) {
        f.push(Finding {
            label: "not possible on this machine".into(),
            ok: true,
            detail: said,
        });
    }

    let spare = crate::fit::spare_weight(&cfg_text);
    let spare_mb: u64 = spare.iter().map(|t| t.frees_mb).sum();
    f.push(Finding {
        label: "spare weight".into(),
        ok: true,
        detail: if spare.is_empty() {
            "nothing installed that your config doesn't use".into()
        } else {
            format!(
                "{spare_mb} MB nothing refers to: {}",
                spare
                    .iter()
                    .take(3)
                    .map(|t| format!("{} ({} MB, {})", t.what, t.frees_mb, t.costs_you))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        },
    });

    // --- what Atlas did: is the record the one it wrote? ---
    let journal = crate::activity::Journal::load(&crate::roots::store());
    let (ok, detail) = match journal.verify_seal() {
        Ok(d) => (true, d),
        Err(why) => (false, why),
    };
    f.push(Finding { label: "activity record".into(), ok, detail });

    // --- the log, as the few things that happened (`drain`) ---
    let log_text = std::fs::read_to_string(crate::roots::logs_dir().join("atlas.log")).unwrap_or_default();
    let (d, lines) = crate::drain::read_log(&log_text);
    if lines > 0 {
        let warnings: Vec<&crate::drain::Template> = d.by_count().into_iter().filter(|t| t.words.first().is_some_and(|w| w == "WARN")).collect();
        let detail = match warnings.first() {
            Some(w) => format!(
                "{lines} lines, {} kinds; the commonest warning ({}x): {}. `atlas logs --warn` lists them.",
                d.templates.len(),
                w.count,
                w.text().trim_start_matches("WARN ")
            ),
            None => format!("{lines} lines, {} kinds, no warnings.", d.templates.len()),
        };
        f.push(Finding { label: "log".into(), ok: true, detail });
    }

    // --- your clock: is a time zone set, and does it read? ---
    if let Some(t) = tools {
        let set = t.time_zone.trim();
        let now = crate::store::now() as i64;
        let (ok, detail) = if set.is_empty() || set == "UTC" {
            let hint = crate::tz::suggest()
                .map(|z| format!(" This computer says {} — pick it under Settings → Time zone.", z.name))
                .unwrap_or_default();
            (true, format!("not set, so every time is UTC.{hint}"))
        } else {
            match crate::tz::Zone::named(set) {
                Some(z) => (true, format!("{} — {} now, {:+.1} hours from UTC.", z.name, z.abbreviation_at(now), z.offset_at(now) as f64 / 3600.0)),
                None => (false, format!("\"{set}\" isn't a zone I know, so times are UTC until it's fixed.")),
            }
        };
        f.push(Finding { label: "time zone".into(), ok, detail });
    }

    // --- your standing watches: does each one read? ---
    if let Some(t) = tools {
        for spec in &t.automations {
            let (ok, detail) = match crate::automation::Automation::from_spec(spec) {
                Ok(a) => (true, format!("\"{}\" — watching: {}", a.name, spec.when.trim())),
                Err(why) => (false, format!("\"{}\" is ignored until fixed: {why}", spec.name.trim())),
            };
            f.push(Finding { label: "watch".into(), ok, detail });
        }
    }

    let m = crate::fit::measure();
    let plan = crate::fit::plan_as_set(&m, &tools.map(|t| t.fit.clone()).unwrap_or_default());
    f.push(Finding {
        label: "this machine".into(),
        ok: true,
        detail: format!(
            "{} — {} at a time, model {} between turns. {}",
            plan.tier.name(),
            plan.concurrency,
            if plan.keep_model_warm { "stays loaded" } else { "unloads" },
            plan.because
        ),
    });

    // --- a personal path in the generic settings ---
    //
    // `config/*.yaml` ships to anyone; `machine.yaml` is this machine's own.
    // A `C:/Users/<name>/` in the generic layer is broken for everybody but
    // that person, and handing Atlas to a friend would carry it with it
    // (`adapt::leaks_a_username`, which had no caller).
    {
        let dir = crate::roots::config_dir();
        let mut leaks = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let path = e.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                if !name.ends_with(".yaml") || name == "machine.yaml" {
                    continue;
                }
                if let Some(who) = std::fs::read_to_string(&path).ok().as_deref().and_then(crate::adapt::leaks_a_username) {
                    leaks.push(format!("{name} has {who}'s own folder in it"));
                }
            }
        }
        f.push(Finding {
            label: "shareable settings".into(),
            ok: leaks.is_empty(),
            detail: if leaks.is_empty() {
                "No one's own folders in the settings that would go to someone else.".into()
            } else {
                format!("{} — that belongs in machine.yaml, or as %USERPROFILE%.", leaks.join("; "))
            },
        });
    }

    // --- the activity log ---
    //
    // Atlas's own record of what it did, checked seal by seal against the
    // heads written beside it. A log that has been edited says so here, in
    // the one place you'd look when something doesn't add up.
    {
        let store = crate::roots::store();
        let journal = crate::activity::Journal::load(&store);
        let backup = crate::config::Config::load(&crate::roots::config_dir())
            .ok()
            .and_then(|c| c.tools)
            .map(|t| t.backup)
            .unwrap_or_default()
            .resolved(&store.install_root());
        let (sealed, from) = crate::activity::check_with_backups(&journal, store.root(), &backup);
        f.push(Finding {
            label: "activity log".into(),
            ok: matches!(sealed, crate::activity::Sealed::Intact { .. }),
            detail: crate::activity::said_with_backups(&sealed, from),
        });
    }

    f
}

/// Emit a paste-ready Rust literal for main.rs's dry-run fixture, so dry runs
/// mirror the real desk.
/// What the machine says about itself, as doctor findings.
///
/// Added because `atlas doctor` reported every tool, model and monitor and
/// said nothing at all about memory or disk — so the readings stub survived
/// a clean doctor run for the entire life of the module. A check that never
/// looks at a thing cannot fail on it.
pub fn machine_findings() -> Vec<Finding> {
    let r = crate::health::read_machine();
    let mut out = Vec::new();
    for name in ["memory", "disk"] {
        let (ok, detail) = match name {
            "memory" if r.ram_total_gb > 0.0 => (
                true,
                format!("{:.1} GB installed, {:.1} GB in use", r.ram_total_gb, r.ram_used_gb),
            ),
            "disk" if r.disk_total_gb > 0.0 => (
                true,
                format!("{:.0} GB free of {:.0} GB", r.disk_free_gb, r.disk_total_gb),
            ),
            _ => (false, "read came back empty — the instrument is not wired".to_string()),
        };
        out.push(Finding { label: name.to_string(), ok, detail });
    }
    out
}

pub fn monitor_fixture(m: &[Monitor]) -> String {
    let mut s = String::from("fn fake_monitors() -> Vec<Monitor> {\n    vec![\n");
    for mon in m {
        s.push_str(&format!(
            "        Monitor {{ id: {}, x: {}, y: {}, width: {}, height: {}, primary: {} }},\n",
            mon.id, mon.x, mon.y, mon.width, mon.height, mon.primary
        ));
    }
    s.push_str("    ]\n}\n");
    s
}

/// Is this app installed from the Store? Returns the package family name.
fn store_package(app: &str) -> Option<String> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let packages = Path::new(&local).join("Packages");
    let key = app.to_lowercase();
    std::fs::read_dir(packages).ok()?.flatten().find_map(|e| {
        let n = e.file_name().to_string_lossy().to_string();
        n.to_lowercase().starts_with(&key).then_some(n)
    })
}

/// A file path, or a name something else resolves?
fn looks_like_a_path(v: &str) -> bool {
    v.contains('/')
        || v.contains('\\')
        || v.rsplit('.').next().map(|e| {
            matches!(e, "bin" | "gguf" | "onnx" | "safetensors" | "pt" | "exe" | "dll")
        }).unwrap_or(false)
}

fn search_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["LOCALAPPDATA", "APPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)", "ProgramData"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .map(PathBuf::from)
        .collect();

    // Two places installers hide things that the roots above miss at shallow
    // depth: per-user program installs, and Store app aliases.
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(&local).join("Programs"));
        roots.push(PathBuf::from(&local).join("Microsoft").join("WindowsApps"));
    }
    roots.retain(|p| p.is_dir());
    roots.dedup();
    roots
}

/// Find candidate executables for an app.
///
/// Returns exact filename matches first, then anything whose name contains the
/// app's key. Bounded depth, because an unbounded walk of Program Files takes
/// minutes and the answer is almost always near the top.
pub fn find_exes(roots: &[PathBuf], exact: &str, app_key: &str) -> Vec<String> {
    let exact = exact.to_lowercase();
    let key = app_key.to_lowercase();
    let mut exacts = Vec::new();
    let mut fuzzy = Vec::new();

    for root in roots {
        walk(root, &exact, &key, 0, 5, &mut exacts, &mut fuzzy);
        if exacts.len() > 4 {
            break;
        }
    }
    exacts.extend(fuzzy);
    exacts.dedup();
    exacts
}

fn walk(
    dir: &Path,
    exact: &str,
    key: &str,
    depth: u32,
    max: u32,
    exacts: &mut Vec<String>,
    fuzzy: &mut Vec<String>,
) {
    if depth > max || exacts.len() > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut subdirs = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            let skip = p
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| SKIP_DIRS.contains(&n.to_lowercase().as_str()))
                .unwrap_or(false);
            if !skip {
                subdirs.push(p);
            }
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()).map(|n| n.to_lowercase()) else {
            continue;
        };
        if !name.ends_with(".exe") {
            continue;
        }
        if name == exact {
            exacts.push(p.display().to_string());
        } else if name.contains(key) && !IGNORE.iter().any(|i| name.contains(i)) {
            fuzzy.push(p.display().to_string());
        }
    }
    for d in subdirs {
        walk(&d, exact, key, depth + 1, max, exacts, fuzzy);
    }
}

/// Directories that are large, irrelevant, and slow to walk.
const SKIP_DIRS: &[&str] =
    &["cache", "caches", "temp", "tmp", "logs", "node_modules", "packages", "cache2", "crashpad"];

/// Names that match an app key but are never the app itself.
const IGNORE: &[&str] = &["uninstall", "crashpad", "setup", "updater", "installer", "helper"];

pub fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' {
            if let Some(close) = chars[i + 1..].iter().position(|c| *c == '%') {
                let name: String = chars[i + 1..i + 1 + close].iter().collect();
                // Windows env names are case-insensitive: the config says
                // %PROGRAMFILES% and Windows reports "ProgramFiles".
                if let Some(v) = lookup_env(&name) {
                    out.push_str(&v);
                    i += close + 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Case-insensitive environment lookup.
pub fn lookup_env(name: &str) -> Option<String> {
    if let Ok(v) = std::env::var(name) {
        return Some(v);
    }
    let want = name.to_lowercase();
    std::env::vars().find(|(k, _)| k.to_lowercase() == want).map(|(_, v)| v)
}
