//! Updating Atlas: installing and undoing an update, feedback, the update
//! command, releases (signing and announcing) and the install page.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

/// `atlas update` — say what replacing the binary would do, before it does.
///
/// The whole command is a report. It moves nothing, downloads nothing and
/// asks for nothing, because the thing that was missing was never the
/// mechanics of copying a file — it was any way to know, in advance and
/// specifically, that your notes and your machine settings are not part of
/// what gets replaced.
/// `atlas update install`: your yes to the release that's here. Checked
/// again and staged; the running Atlas restarts itself into it, or the next
/// start installs it.
fn update_install(root: &std::path::Path) {
    let store = atlas::roots::store();
    let a = atlas::update_courier::Available::load(&store);
    let Some(platform) = atlas::release::this_platform() else {
        println!("Atlas doesn't ship updates for this kind of device.");
        return;
    };
    if a.notice.is_none() {
        println!("There's no update waiting. `atlas update` says what's been heard.");
        return;
    }
    atlas::update_apply::say_yes(&store, &a.version);
    match atlas::update_apply::stage_update(&store, root, platform) {
        Ok(v) => println!(
            "Atlas {v} is checked against your release key and ready. The background Atlas restarts itself into it \
             within a minute; if Atlas isn't running, the next start installs it. It has to pass its health check \
             first, and the version you're on now is kept."
        ),
        Err(why) => println!("{why}"),
    }
}

/// `atlas update undo`: back one version, with your yes typed here.
fn update_undo(root: &std::path::Path) {
    let Some((previous, _)) = atlas::update_apply::previous_build(root) else {
        println!("There's no previous version kept here to go back to.");
        return;
    };
    print!("Go back from Atlas {} to {previous}? {} won't be offered again. Type yes to go back: ", atlas::upgrade::version(), atlas::upgrade::version());
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() || !answer.trim().eq_ignore_ascii_case("yes") {
        println!("Left as it is.");
        return;
    }
    let Ok(running) = std::env::current_exe() else {
        println!("I couldn't tell where this program is, so nothing changed.");
        return;
    };
    let yes = atlas::release::LocalApproval::given_by_the_person_at_this_device();
    match atlas::update_apply::undo_update(&atlas::roots::store(), root, &running, yes) {
        Ok(v) => println!("Back on Atlas {v}. The background Atlas restarts itself onto it within a minute."),
        Err(why) => println!("{why}"),
    }
}

/// Who sends this Atlas its updates: (their key, your name for them), and
/// whether that's you. From your release channel's signed list.
fn update_sender() -> Option<(String, Option<String>, bool)> {
    atlas::feedback::release_sender(&atlas::roots::store(), &pairings_dir())
}

/// `atlas feedback`: tell whoever sends you Atlas that something's wrong --
/// when you decide it is -- and hear back what they did about it. On the
/// releaser's side, the feedback your friends sent, and your answers.
///
///   atlas feedback                      what you've sent and what's come in
///   atlas feedback send                 write it, see it, send it
///   atlas feedback reply <n> seen|fixing|fixed <version>|wont [note...]
pub(super) fn run_feedback(args: &[String]) {
    use atlas::feedback::{self, FeedbackStatus};
    let store = atlas::roots::store();
    let now = atlas::store::now();
    match args.first().map(|s| s.as_str()) {
        Some("send") => {
            let Some((_, name, mine)) = update_sender() else {
                println!("This Atlas isn't in anyone's release channel, so there's no one to send feedback to.");
                return;
            };
            let words = ask_line("What's wrong? Say it in your own words: ");
            let attach = match atlas::update_apply::last_failure(&store) {
                Some(f) => {
                    println!("\nAn update failed here earlier. This is what was written down about it (your name and home folder are already taken out):\n");
                    println!("{}", feedback::feedback_preview(&feedback::Feedback { attached: Some(f.clone()), ..Default::default() }).lines().skip(2).collect::<Vec<_>>().join("\n"));
                    ask_line("\nAttach it? Type yes to attach: ").eq_ignore_ascii_case("yes").then_some(f)
                }
                None => None,
            };
            let f = match feedback::compose_feedback(&words, attach, now) {
                Ok(f) => f,
                Err(why) => {
                    println!("{why}");
                    return;
                }
            };
            println!("\nThis is exactly what will be sent:\n\n{}", feedback::feedback_preview(&f));
            if mine {
                // Your own Atlas: straight into your own list.
                let _ = feedback::heard_feedback(&store, "you", &serde_json::to_string(&f).unwrap_or_default());
                println!("This is your own Atlas, so it's in your own feedback list (`atlas feedback`).");
                return;
            }
            let Some(to) = name else {
                println!("I can't reach whoever sends you updates -- you aren't paired with them -- so nothing was sent.");
                return;
            };
            if !ask_line(&format!("Send this to {to}? Type yes to send: ")).eq_ignore_ascii_case("yes") {
                println!("Not sent.");
                return;
            }
            feedback::queue_feedback(&store, f, &to);
            println!("Sending it to {to}. The running Atlas delivers it when it can reach them, and tells you when they answer.");
        }
        Some("reply") => {
            let n = args.get(1).and_then(|n| n.parse::<usize>().ok());
            let word = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let version = args.get(3).map(|s| s.as_str());
            let (Some(n), Some(status)) = (n, FeedbackStatus::from_words(word, version)) else {
                println!("atlas feedback reply <n> seen | fixing | fixed <version> | wont [a note for them]");
                return;
            };
            let skip = if matches!(status, FeedbackStatus::Fixed(_)) { 4 } else { 3 };
            let note = args.iter().skip(skip).cloned().collect::<Vec<_>>().join(" ");
            match feedback::answer_feedback(&store, n, status.clone(), &note, now) {
                Ok(to) => println!("Marked {}; {to} will hear it the next time Atlas reaches them.", status.plain()),
                Err(why) => println!("{why}"),
            }
        }
        _ => {
            let sent = feedback::feedback_sent(&store);
            let got = feedback::feedback_inbox(&store);
            if sent.is_empty() && got.is_empty() {
                println!("No feedback sent or received yet. `atlas feedback send` tells whoever sends you Atlas that something's wrong.");
                return;
            }
            if !got.is_empty() {
                println!("Feedback from your friends:");
                for (i, f) in got.iter().enumerate() {
                    println!(
                        "  {}. {} (Atlas {}, {}): \"{}\"{} -- {}",
                        i + 1,
                        f.from,
                        f.version,
                        f.platform,
                        f.words,
                        if f.attached.is_some() { " [update failure attached]" } else { "" },
                        f.status.plain()
                    );
                }
                println!("  `atlas feedback reply <n> seen|fixing|fixed <version>|wont [note]` answers one; `atlas update failures brief <version>` makes a fix brief.\n");
            }
            if !sent.is_empty() {
                println!("What you've sent:");
                for f in &sent {
                    println!("  to {}: \"{}\" -- {}", f.to, f.words, f.status.plain());
                    for (_, note) in &f.replies {
                        println!("      they said: {note}");
                    }
                }
            }
        }
    }
}

/// `atlas update failures [brief <version>]`: every report of an update
/// that failed -- on your own devices and friends' -- grouped by version, and
/// a written brief for fixing one.
fn update_failures(flag: Option<&str>, version: Option<&str>) {
    let store = atlas::roots::store();
    let reports = atlas::update_apply::failure_reports(&store);
    if flag == Some("brief") {
        let Some(v) = version else {
            println!("atlas update failures brief <version>");
            return;
        };
        match atlas::update_apply::fix_brief(&reports, v) {
            Some(brief) => {
                let out = format!("atlas-{v}-fix-brief.md");
                match std::fs::write(&out, &brief) {
                    Ok(()) => println!("Wrote {out}: what failed, where, and how to close it. Hand it to a coding session, or say \"work on yourself\" to Atlas on the machine that holds the source."),
                    Err(e) => println!("couldn't write {out}: {e}\n\n{brief}"),
                }
            }
            None => println!("No reports about Atlas {v}."),
        }
        return;
    }
    if reports.is_empty() {
        println!("No update has failed anywhere that's told me.");
        return;
    }
    let mut versions: Vec<&str> = reports.iter().map(|r| r.version.as_str()).collect();
    versions.dedup();
    for v in versions {
        let these: Vec<_> = reports.iter().filter(|r| r.version == v).collect();
        println!("Atlas {v}: {} report(s)", these.len());
        for r in these {
            let why: Vec<&str> = r.reasons.iter().map(|x| x.as_str()).filter(|x| !x.starts_with("(passed)")).collect();
            println!(
                "  {} ({}), at its {}: {}{}",
                if r.from.is_empty() { "?" } else { &r.from },
                r.platform,
                r.stage,
                why.join("; "),
                match r.because {
                    atlas::update_apply::FailedBecause::ThisMachine => "  [that machine, not the build]",
                    atlas::update_apply::FailedBecause::TheBuild => "",
                }
            );
        }
    }
    println!("\n`atlas update failures brief <version>` writes a brief for fixing one. A failing build stays blocked and isn't handed out; the fixed release is offered as usual.");
}

/// `atlas update auto on|ask|off|default`.
fn update_auto(to: Option<&str>) {
    use atlas::update_apply::AutoUpdate;
    let (mode, said) = match to {
        Some("on" | "automatic") => (Some(AutoUpdate::Automatic), "Updates install by themselves at a quiet moment."),
        Some("ask") => (Some(AutoUpdate::Ask), "I'll ask before installing each update."),
        Some("off") => (Some(AutoUpdate::Off), "Updates are off. `atlas update` still says what's out."),
        Some("default") => (None, "Back to the default: automatic on your own devices, ask on friends'."),
        _ => {
            println!("atlas update auto on | ask | off | default");
            return;
        }
    };
    match atlas::update_apply::choose_mode(&atlas::roots::store(), mode) {
        Ok(()) => println!("{said}"),
        Err(e) => println!("I couldn't keep that: {e}"),
    }
}

pub(super) fn run_update(args: &[String]) {
    // The install root, not the working directory.
    //
    // This was `Path::new(".")`. Run `atlas update` from a Start Menu
    // shortcut, from Task Scheduler (working directory `system32`), or from
    // any terminal that is not sitting in the install folder, and every entry
    // in `upgrade::YOURS` missed — so each was labelled
    // `Fate::Regenerated` ("Not there, and will be made from scratch on first
    // run. **Not a problem**"), `Report::safe()` returned true, and the output
    // read *"Safe to update. 0 things of yours stay exactly where they are."*
    //
    // This module exists for one job: answering "what survives" with
    // evidence. It could not tell "not there" apart from "I looked in the
    // wrong place", and the reassuring answer was the wrong one. `roots.rs`
    // names this exact scenario — `atlas update` run from anywhere — as the
    // bug it was written to kill, and `tests/one_install_root.rs` did not
    // catch it because `"."` is not a `data/…` literal.
    let root = atlas::roots::install_root();
    // Step 2: installing a release that's arrived, going back, and the
    // automatic setting (`update_apply`).
    match args.first().map(|s| s.as_str()) {
        Some("install") => return update_install(&root),
        Some("undo") => return update_undo(&root),
        Some("auto") => return update_auto(args.get(1).map(|s| s.as_str())),
        Some("failures") => return update_failures(args.get(1).map(|s| s.as_str()), args.get(2).map(|s| s.as_str())),
        _ => {}
    }
    let report = atlas::upgrade::check(&root);

    println!(
        "atlas {} (data format {})",
        atlas::upgrade::version(),
        atlas::upgrade::DATA_FORMAT
    );
    println!();
    println!("{}", report.spoken());
    println!("\n{}", atlas::update_courier::status(&atlas::roots::store(), atlas::store::now()));
    let avail = atlas::update_courier::Available::load(&atlas::roots::store());
    if avail.notice.is_some() {
        println!(
            "  For this device: {} ({} MB, fingerprint {}). Installing it from here is the next step \
             of the courier; until then it waits.",
            avail.file,
            avail.size / (1024 * 1024),
            avail.sha256.get(..16).unwrap_or("")
        );
    }
    // The trial of the last update, and what happened to updates here (O1).
    if let Some(t) = atlas::upgrade::current_trial(&root) {
        println!(
            "\n{} is on trial after replacing {}: {} of {} starts so far without getting through. \
             If it reaches {}, {} comes back by itself.",
            t.new, t.previous, t.starts, atlas::upgrade::TRIAL_STARTS, atlas::upgrade::TRIAL_STARTS, t.previous
        );
    }
    let history = atlas::upgrade::update_history(&root);
    if !history.is_empty() {
        println!("\nWhat happened to updates here (latest last):");
        for line in history.iter().rev().take(5).rev() {
            let (at, what) = line.split_once(' ').unwrap_or(("", line));
            let when = at.parse::<u64>().map(|t| atlas::freshness::ago(atlas::store::now().saturating_sub(t))).unwrap_or_default();
            println!("  {when}  {what}");
        }
    }
    {
        let store = atlas::roots::store();
        let mode = match atlas::update_apply::chosen_mode(&store) {
            Some(atlas::update_apply::AutoUpdate::Automatic) => "automatic (your choice)",
            Some(atlas::update_apply::AutoUpdate::Ask) => "ask first (your choice)",
            Some(atlas::update_apply::AutoUpdate::Off) => "off (your choice)",
            None => "the default: automatic on your own devices, ask on friends'",
        };
        println!("\nInstalling updates: {mode}. `atlas update auto on|ask|off|default` changes it.");
        if let Some(p) = atlas::update_apply::pending(&store) {
            println!("Atlas {} is checked and waiting to go in at the next start.", p.version);
        }
        if let Some((v, _)) = atlas::update_apply::previous_build(&root) {
            println!("Atlas {v} is kept; `atlas update undo` goes back to it.");
        }
    }
    let kept = atlas::yourchanges::kept_count(&atlas::roots::config_dir());
    println!(
        "\nYour own edits to the shipped config files: {kept} kept in config/{}, laid back over \
         whatever the next version ships. Hand edits made since the last start are moved there \
         the next time Atlas starts.",
        atlas::yourchanges::LOCAL_DIR
    );
    println!();

    if report.safe() {
        println!("To update: stop Atlas, replace the binary, start it again.");
        println!(
            "Keep the old one as {} first and you can go back without downloading anything.",
            atlas::upgrade::keep_old_at(&root, &atlas::upgrade::this_tag()).display()
        );
        println!("Nothing above marked \"kept\" is touched by any of that.");
        if args.first().map(|s| s.as_str()) == Some("--why") {
            println!();
            println!(
                "The reason this is a report rather than a downloader: Atlas is one \n\
                 binary plus two config layers. The generic layer ships; the machine \n\
                 layer and everything under data/ are written here and are not part of \n\
                 any download. `atlas adapt` rebuilds the machine layer if you ever do \n\
                 lose it, and it takes a few seconds."
            );
        }
    } else {
        println!("Move or copy the things marked AT RISK before updating.");
    }
}

/// `atlas install-page <Atlas.ipa|Atlas.apk> [--friends] [--minutes N] [--port N]`
/// -- put a phone build on a page the phone installs it from. For an iPhone,
/// this is the only way on without a Mac; for Android it saves emailing the APK.
///
/// The page, its manifest and the app are served on 127.0.0.1 only, under a
/// random path, for `--minutes` (default 20). Tailscale carries them to the
/// phone over HTTPS on port 8443: `serve` (only your own devices, the default)
/// or, with `--friends`, `funnel` (anyone with the link, until the time runs
/// out). Either way it is switched off again at the end.
pub(super) fn run_install_page(_words: &[String]) {
    use atlas::ota;
    // Read the raw arguments: `main` strips every `--flag` out of the words,
    // which would leave `--minutes 5`'s "5" looking like the file.
    let raw: Vec<String> = std::env::args().skip_while(|a| a != "install-page").skip(1).collect();
    let mut file = None;
    let (mut minutes, mut port, mut public) = (20u64, 9741u16, false);
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--friends" => public = true,
            "--minutes" => minutes = it.next().and_then(|v| v.parse().ok()).unwrap_or(minutes).clamp(1, 240),
            "--port" => port = it.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            other if !other.starts_with("--") => file = Some(other.to_string()),
            other => println!("(ignoring {other})"),
        }
    }
    let Some(file) = file else {
        println!("atlas install-page <Atlas.ipa | Atlas.apk> [--friends] [--minutes 20]");
        println!("  Puts a phone build on a page the phone installs it from, over your Tailscale.");
        println!("  --friends  let a friend's iPhone (not on your tailnet) open it, until the time runs out");
        return;
    };
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => return println!("I can't read {file}: {e}"),
    };
    let package = match ota::Package::read(&bytes) {
        Ok(p) => p,
        Err(e) => return println!("{file} can't go on the page: {e}"),
    };
    let phone = match &package {
        ota::Package::Ios(ipa) => {
            println!("{} {} (build {}), {}", ipa.title, ipa.version, ipa.build, ipa.bundle_id);
            println!("Installs on {} device(s); works until {}.", ipa.devices.len(), ipa.expires.as_deref().unwrap_or("an unknown date"));
            if ipa.devices.is_empty() || ipa.expired(&ota::now_iso()) {
                return println!("This build can't install on anything: {}. Make a new build first.",
                    if ipa.devices.is_empty() { "it lists no devices" } else { "it has expired" });
            }
            "iPhone, in Safari"
        }
        ota::Package::Android(apk) => {
            println!("Atlas for Android, {:.1} MB, signed. SHA-256 {}", apk.bytes_len as f64 / 1048576.0, apk.sha256);
            "Android phone, in Chrome"
        }
    };

    let tool = atlas::phonelink::tailscale_tool();
    let vars = atlas::tools::Vars::new();
    let run = |a: &[String]| atlas::tools::ExternalTool { args: a.to_vec(), ..tool.clone() }.run(&vars, None).map_err(|e| e.to_string());
    let status = run(&["status".into(), "--json".into()]);
    let net = match status.as_ref().ok().and_then(|j| atlas::phonelink::read_status(j)) {
        Some(n) if n.running && !n.dns_name.is_empty() => n,
        Some(n) if n.running => return println!("{}", atlas::phonelink::say(&atlas::phonelink::Serve::NeedsHttps)),
        _ => {
            let why = atlas::phonelink::serve_outcome(status.map(|_| String::new()), "");
            let why = if why == atlas::phonelink::Serve::NeedsHttps { atlas::phonelink::Serve::NotRunning } else { why };
            return println!("{}", atlas::phonelink::say(&why));
        }
    };
    let token = ota::fresh_token();
    let (on, off) = ota::tailscale_args(port, public);
    let url = ota::page_url(&net.dns_name, &token);
    match atlas::phonelink::serve_outcome(run(&on), &url) {
        atlas::phonelink::Serve::Published { .. } => {}
        other => return println!("{}", atlas::phonelink::say(&other)),
    }
    println!();
    println!("Open this on the {phone}{}:", if public { "" } else { " (the phone needs Tailscale on)" });
    println!("  {url}");
    if let Some((n, dark)) = atlas::phonelink::qr_modules(&url) {
        // Two rows per line with half blocks, so the code fits a terminal.
        for y in (0..n).step_by(2) {
            let line: String = (0..n)
                .map(|x| match (dark[y * n + x], y + 1 < n && dark[(y + 1) * n + x]) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    (false, false) => ' ',
                })
                .collect();
            println!("  {line}");
        }
    }
    if public {
        println!("This link works from anywhere until the page closes, in {minutes} minutes. Only send it to people you mean to.");
    }
    println!("Waiting (Ctrl+C stops it early; the Tailscale switch then stays on until `tailscale {} --https=8443 off`).", on[0]);
    let base = format!("https://{}:{}", net.dns_name, ota::HTTPS_PORT);
    let served = ota::serve_install(&bytes, &package, port, &token, Some(&base), minutes, &mut |s: &str| println!("  {s}"));
    let _ = run(&off);
    match served {
        Ok(0) => println!("Closed. No phone downloaded the app."),
        Ok(n) => println!("Closed. The app was downloaded {n} time(s). Tailscale's switch is off again."),
        Err(e) => println!("The page couldn't run: {e}. Tailscale's switch is off again."),
    }
}

/// The name the release key is kept under in the vault.
use atlas::release::RELEASE_KEY_NAME;

/// `atlas release keygen | sign | announce | show` -- the build side of the
/// update courier: the key that signs every release, and the notice that
/// reaches every friend's Atlas through your release channel.
pub(super) fn run_release(args: &[String]) {
    use atlas::release;
    let state = atlas::roots::install_state();
    let cfg = Config::load(&atlas::roots::config_dir()).ok().and_then(|c| c.tools).map(|t| t.vault).unwrap_or_default();
    let now = atlas::store::now();
    let open_vault = || -> Option<atlas::vault::Vault> {
        let mut vault = atlas::vault::Vault::load(&state);
        if !vault.has_a_passphrase() {
            println!("The release key lives in your vault, and the vault has no passphrase yet. `atlas vault passphrase` first.");
            return None;
        }
        let phrase = ask_quietly("Vault passphrase: ")?;
        if let Err(why) = vault.open(&phrase, now, &cfg) {
            println!("{why}");
            return None;
        }
        Some(vault)
    };
    match args.first().map(|s| s.as_str()).unwrap_or("show") {
        "show" => {
            println!(
                "This build trusts {}.",
                if release::anchor_configured() {
                    format!("the release key {}", release::seed_hex(&release::RELEASE_PUBLIC_KEY))
                } else {
                    "no release key yet -- so it accepts no updates. `atlas release keygen` makes one.".into()
                }
            );
            println!("{}", atlas::update_courier::status(&atlas::roots::store(), now));
        }
        "keygen" => {
            let Some(mut vault) = open_vault() else { return };
            let made = match release::make_release_key(&mut vault, now) {
                Ok(m) => m,
                Err(why) => {
                    println!("{why}");
                    vault.lock();
                    return;
                }
            };
            if !keep(vault.save(&state), "the vault") {
                return;
            }
            vault.lock();
            if let Err(e) = atlas::roots::store().save(release::KEY_CARD, &made.card) {
                println!("(Atlas couldn't keep the key card for the Updates page: {e}. It's printed below; that copy is the one to send.)\n");
            }
            println!("Made your release key. It's in your vault and never leaves it.\n");
            println!("Put these two lines in release-keys.txt before building the copies you hand out:\n");
            println!("{}\n", made.card);
            println!("  ================================================================");
            println!("    YOUR RELEASE RECOVERY KEY -- write this down now, keep it offline");
            println!("  ================================================================\n");
            println!("      {}\n", made.recovery);
            println!("  It is not stored anywhere and won't be shown again. It is the only way to");
            println!("  replace the release key if the vault is lost or the key is stolen -- so it");
            println!("  must never be on this computer.");
        }
        "sign" => {
            // atlas release sign 1.4.0 windows-x86_64=target/atlas.exe macos-aarch64=out/Atlas.dmg
            let Some(version) = args.get(1) else {
                println!("atlas release sign <version> <platform>=<file> [<platform>=<file> ...]");
                return;
            };
            let mut files = Vec::new();
            for a in &args[2..] {
                let Some((platform, path)) = a.split_once('=') else {
                    println!("{a}: write it as <platform>=<file>, e.g. windows-x86_64=atlas.exe");
                    return;
                };
                let bytes = match std::fs::read(path) {
                    Ok(b) => b,
                    Err(e) => {
                        println!("couldn't read {path}: {e}");
                        return;
                    }
                };
                let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                files.push((platform.to_string(), name, bytes));
            }
            let Some(mut vault) = open_vault() else { return };
            let seed = match vault.get(RELEASE_KEY_NAME, now) {
                Ok(s) => release::seed_from_hex(&s),
                Err(why) => {
                    println!("{why} -- `atlas release keygen` makes the key.");
                    return;
                }
            };
            vault.lock();
            let Some(seed) = seed else {
                println!("The release key in your vault is damaged.");
                return;
            };
            let key = release::signing_key_from_seed(&seed);
            if release::anchor_of(&key) != release::RELEASE_PUBLIC_KEY {
                println!(
                    "Warning: this build doesn't carry your release key, so copies built from it would \
                     refuse this release. Bake the key in (see `atlas release keygen`) before handing \
                     out copies."
                );
            }
            // The releaser's own store, where the hub's "Sign and send" keeps it too.
            let seq_store = atlas::roots::store();
            let last: u64 = seq_store.load(release::LAST_SEQUENCE);
            let sequence = last + 1;
            let manifest = match release::manifest_for(
                sequence,
                version,
                atlas::upgrade::DATA_FORMAT,
                now,
                now + 30 * 86_400,
                &files,
            ) {
                Ok(m) => m,
                Err(r) => {
                    println!("{}", r.plain());
                    return;
                }
            };
            let signed = release::seal_manifest(&key, &manifest);
            let out = format!("atlas-release-{version}.json");
            if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&signed).unwrap_or_default()) {
                println!("couldn't write {out}: {e}");
                return;
            }
            // The number is what makes "newer" mean something; if it can't be
            // kept, the next release would reuse it and be refused as already
            // installed. Said, not glossed.
            if let Err(e) = seq_store.save(release::LAST_SEQUENCE, &sequence) {
                println!("Signed {out}, but couldn't record release number {sequence} ({e}). Don't sign another until that's fixed -- it would reuse the number.");
                return;
            }
            // Put aside for friends' Atlases to fetch over their pairings, by
            // fingerprint, once the notice is announced.
            let state = atlas::roots::store().root().to_path_buf();
            for (_, name, bytes) in &files {
                if let Err(e) = atlas::update_courier::keep_for_friends(&state, bytes) {
                    println!("Signed, but {name} couldn't be put aside for friends to fetch: {e}");
                }
            }
            println!("Signed release {sequence} ({version}) for {} platform(s): {out}", files.len());
            println!("Next notice promised within 30 days. `atlas release announce {out}` posts it.");
        }
        "announce" => {
            let Some(path) = args.get(1) else {
                println!("atlas release announce <atlas-release-VERSION.json>");
                return;
            };
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    println!("couldn't read {path}: {e}");
                    return;
                }
            };
            if serde_json::from_str::<release::SignedManifest>(&text).is_err()
                && serde_json::from_str::<release::SignedRotation>(&text).is_err()
            {
                println!("{path} isn't a signed release notice or key change (`atlas release sign` or `rotate` makes one).");
                return;
            }
            let dir = atlas::roots::store().root().join(atlas::update_courier::OUTBOX);
            let dest = dir.join(std::path::Path::new(path).file_name().unwrap_or_default());
            match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&dest, &text)) {
                Ok(()) => println!("Queued. The running Atlas posts it in every release channel you own."),
                Err(e) => println!("couldn't queue it: {e}"),
            }
        }
        // Stop handing out a build, after reading feedback about it.
        "hold" => {
            let Some(v) = args.get(1) else {
                println!("atlas release hold <version>");
                return;
            };
            let store = atlas::roots::store();
            let builds: Vec<String> = atlas::update_apply::failure_reports(&store)
                .into_iter()
                .filter(|r| &r.version == v && !r.sha256.is_empty())
                .map(|r| r.sha256)
                .collect();
            if builds.is_empty() {
                println!("I don't know which build Atlas {v} is -- no failure report names it. Nothing held.");
                return;
            }
            for b in &builds {
                atlas::update_apply::hold_release(&store, b);
            }
            println!("Held: Atlas {v} isn't handed out to anyone until a fixed release replaces it.");
        }
        // A planned change of release key, signed by the key it replaces
        // (spec §20 gap G). Every device that trusts the old key moves to the
        // new one when it hears this in the release channel.
        "rotate" | "recover" => {
            let recovering = args.first().map(|s| s.as_str()) == Some("recover");
            let store = atlas::roots::store();
            let mut installed = release::Installed::load(&store);
            let signer = if recovering {
                println!("Recovery: for when the release key is lost or stolen. Type the recovery key you wrote down.");
                let Some(typed) = ask_quietly("Recovery key: ") else { return };
                let Some(seed) = release::seed_from_hex(&typed.replace([' ', '-'], "")) else {
                    println!("That isn't a recovery key (64 hex characters, spaces allowed).");
                    return;
                };
                let key = release::signing_key_from_seed(&seed);
                if release::anchor_of(&key) != release::RECOVERY_PUBLIC_KEY {
                    println!("That isn't the recovery key this build carries, so devices would refuse it. Nothing changed.");
                    return;
                }
                key
            } else {
                let Some(mut vault) = open_vault() else { return };
                let seed = vault.get(RELEASE_KEY_NAME, now).ok().and_then(|s| release::seed_from_hex(&s));
                vault.lock();
                let Some(seed) = seed else {
                    println!("There's no release key in your vault to change from. `atlas release keygen` makes one.");
                    return;
                };
                release::signing_key_from_seed(&seed)
            };
            // Recovery jumps well past anything a thief may have signed; a
            // planned change is exactly the next one.
            let number = installed.trust.rotations + if recovering { 100 } else { 1 };
            let new_seed = release::new_seed();
            let new_public = release::anchor_of(&release::signing_key_from_seed(&new_seed));
            let rotation = release::Rotation {
                number,
                new_anchor: release::seed_hex(&new_public),
                reason: if recovering { "recovery".into() } else { "planned".into() },
            };
            let signed = release::seal_rotation(&signer, &rotation);
            let Some(mut vault) = open_vault() else { return };
            if let Ok(old) = vault.get(RELEASE_KEY_NAME, now) {
                let _ = vault.put(&format!("{RELEASE_KEY_NAME} (before change {number})"), atlas::vault::Kind::ApiKey, &old, now);
            }
            if let Err(why) = vault.put(RELEASE_KEY_NAME, atlas::vault::Kind::ApiKey, &release::seed_hex(&new_seed), now) {
                println!("{why} Nothing changed.");
                return;
            }
            if !keep(vault.save(&state), "the vault") {
                return;
            }
            vault.lock();
            let out = format!("atlas-rotation-{number}.json");
            if let Err(e) = std::fs::write(&out, serde_json::to_string_pretty(&signed).unwrap_or_default()) {
                println!("The new key is in your vault, but I couldn't write {out}: {e}");
                return;
            }
            // This device trusts the new key at once; the others when they hear it.
            match release::apply_rotation(&installed.trust, &signed) {
                Ok(t) => {
                    installed.trust = t;
                    if let Err(e) = installed.save(&store) {
                        println!("(This device couldn't record the new key ({e}); it takes it when it hears the announcement.)");
                    }
                }
                Err(r) => println!("(This device didn't take it: {})", r.plain()),
            }
            println!("Made a new release key (change {number}); the old one is kept in your vault, renamed.");
            println!("`atlas release announce {out}` posts the change in your release channels.");
            println!("Bake the new key into src/release.rs before building copies for new devices:\n");
            println!("pub const RELEASE_PUBLIC_KEY: [u8; 32] = {};", release::as_rust_array(&new_public));
        }
        other => println!("I don't know `atlas release {other}`. Try: show, keygen, sign <version> <platform>=<file>..., announce <file>, rotate, recover, hold <version>."),
    }
}
