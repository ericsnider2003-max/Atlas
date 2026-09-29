//! The hub from the command line: serving it, the install report, metrics,
//! the hub's address, the phone page and the phone.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

pub(super) fn run_hub(cfg: &Config) {
    use atlas::hub;
    use atlas::server::{Action, Reply, Server, ServerConfig};

    let tools = cfg.tools.clone().unwrap_or_default();
    // The one place that ignores the switch: you have typed `atlas settings`,
    // and a command that opens a page is not the same thing as a dashboard a
    // daemon leaves listening.
    let scfg = ServerConfig { enabled: true, ..ServerConfig::default() };
    // Loopback only, and a token even so — anything else on the machine can
    // reach a local port.
    // From the operating system, not the clock. If there's no entropy source
    // it stops rather than falling back — a token quietly generated from a
    // timestamp is worse than not starting, because everything downstream
    // assumes it's strong and you'd never know.
    // The install's own token, so this prints the same address the daemon
    // prints and the one you already bookmarked.
    let token = match atlas::server::token_for(&atlas::roots::store()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("couldn't make a secure token: {e}");
            leave(2);
        }
    };

    let server = match Server::bind(&scfg, &token) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("couldn't open the settings page: {e}");
            eprintln!("something else may already be using port {}.", scfg.port);
            leave(2);
        }
    };

    // WITH the token. These two lines printed addresses with no `?t=` on
    // them, and every request carries a token with no exemptions
    // (`token_matches` returns false for `None`), so the server answered
    // `Denied` to both. The token was generated three lines up and printed
    // nowhere at all.
    //
    // That made `atlas settings` unusable -- and it is menu item 3 in
    // `ATLAS.bat`, labelled "works even when Atlas won't", the recovery path
    // for when the voice or the daemon is broken. The one door that is
    // supposed to work when nothing else does answered Denied.
    println!(
        "Settings are at {}",
        atlas::server::hub_url(server.port(), &token, "/hub/settings")
    );
    println!("Access is at    {}", atlas::server::hub_url(server.port(), &token, "/hub/access"));
    println!("Leave this window open. Close it when you're done.");
    println!();
    println!("This is the fallback for when Atlas itself won't start. Normally: open");
    println!("Atlas and press Hub or Settings -- the same pages, in Atlas's own window.");

    let mut settings = atlas::settings::registry(&tools);
    let store = atlas::roots::store();
    let commands = cfg.commands.clone();
    let trash = atlas::safety::Trash::new(tools.trash.clone().resolved(&atlas::roots::install_root()));
    // The arrangement is yours and lives on disk. Read once at start rather
    // than per request, written back whenever a move actually changes it.
    let mut layout = atlas::dash::Layout::load(&store);
    // Reading is the default. Arranging is a mode you turn on, so a stray
    // drag while you are reading cannot rearrange anything.
    let mut arranging = false;

    let mut handle = |action: Action| -> Reply {
        match action {
            // Your switch, honoured.
            //
            // `hub.enabled` shipped `true` and was read by nothing, so there
            // was no way to turn the dashboard off from config -- the exact
            // sentence `config::PARSED_AND_NEVER_READ` recorded against it.
            // The pages are served here, so this is where the answer belongs.
            //
            // Plain HTML rather than a 404: you turned it off, and being told
            // so is more use than a browser error.
            Action::Hub(_) | Action::HubQ(..) if !tools.hub.enabled => Reply::html(
                "<h1>The hub is switched off</h1><p>Set <code>hub.enabled: true</code> \
                 in your tools.yaml to bring it back.</p>",
            ),
            Action::Hub(page) => match page {
                hub::Page::Dashboard => {
                    Reply::html(hub::dashboard_page(&layout, &dash_bodies(), arranging))
                }
                hub::Page::Settings => Reply::html(hub::settings_page(&settings)),
                hub::Page::Access => Reply::html(hub::access_page(&[])),
                // Both are files on disk, so they work with Atlas stopped --
                // which is when you would most want to take a permission away.
                hub::Page::AddOns => Reply::html(hub::addons_page_with(
                    &atlas::plugins::scan(
                        &atlas::plugins::plugins_dir(),
                        &commands,
                        &atlas::plugins::Approvals::load(&store),
                    ),
                    &atlas::plugins::Offers::load(&store).items,
                    &[],
                    &[],
                )),
                hub::Page::Edits => {
                    let (kept, problems) = atlas::yourchanges::all_kept(&atlas::roots::config_dir());
                    Reply::html(hub::edits_page(&kept, &problems))
                }
                hub::Page::Groups => {
                    let (views, addable) =
                        atlas::groups::views(&store, &atlas::kin::where_pairings_live());
                    Reply::html(hub::groups_page(&views, &addable))
                }
                // Never `{:?}` on an enum: that reaches the screen as a
                // variable name, which is a code leak in a product.
                other => Reply::html(hub::shell(
                    other.label(),
                    &format!(
                        "<p class=nothing>{} needs Atlas itself running. Start Atlas \
                         and open this page again — this window only knows about \
                         settings.</p>",
                        hub::esc(other.label())
                    ),
                )),
            },
            Action::DashArrange(on) => {
                arranging = on;
                Reply::redirect("/hub")
            }
            Action::DashMove(m) => {
                // Only write when something actually moved. A file rewritten
                // on every click is a file that gets corrupted on the one
                // click that happens during a power cut.
                if layout.apply(&m) {
                    if let Err(e) = layout.save(&store) {
                        eprintln!("couldn't save the dashboard layout: {e}");
                    }
                }
                Reply::redirect("/hub")
            }
            // Validated, then written, through the same path as the running
            // Atlas. Until 27 Sep 2026 this validated, said "is now on", and
            // wrote nothing -- in the window whose whole job is changing a
            // setting when Atlas won't start -- and put the sentence into the
            // page unescaped.
            Action::HubSet { key, value } => {
                let said = settings.set_and_keep(&key, &value, &atlas::roots::config_dir());
                hub::back_with(&format!("{}#set-{key}", hub::Page::Settings.href()), "", &said)
            }
            // What a button did, said on the page it came back to.
            Action::HubQ(page, q) => {
                let said = hub::form_fields(&q).into_iter().find(|(k, _)| k == "said").map(|(_, v)| v);
                let page = match page {
                    hub::Page::Settings => hub::settings_page(&settings),
                    hub::Page::Access => hub::access_page(&[]),
                    other => hub::shell(
                        other.label(),
                        &format!(
                            "<h1>{}</h1><p class=nothing>This page needs Atlas itself running. Start Atlas \
                             and open it again — this window only knows about settings.</p>",
                            hub::esc(other.label())
                        ),
                    ),
                };
                Reply::html(hub::with_said(page, said.as_deref()))
            }
            Action::HubBack(page, said) => hub::back_with(page.href(), "", &said),
            // Said rather than silently shown the settings page. This window
            // is the one you get when Atlas itself is not running, and it
            // does not hold the grants -- pressing revoke here and being
            // shown a different page would look exactly like it worked.
            Action::AddOn { what, id, key, sha } => hub::after_button(
                hub::Page::AddOns,
                atlas::plugins::hub_action(
                    &store,
                    &atlas::plugins::plugins_dir(),
                    &commands,
                    &trash,
                    &what,
                    &id,
                    &key,
                    &sha,
                ),
            ),
            Action::Friend { .. } => Reply::html(hub::shell(
                "Atlas",
                "<p class=note>Adding a friend needs Atlas itself running -- its door is what their \
                 Atlas knocks on. Start Atlas and open the Friends page.</p>",
            )),
            Action::GroupChange { what, group, who, role } => hub::after_button(
                hub::Page::Groups,
                atlas::groups::act(&store, &atlas::kin::where_pairings_live(), &what, &group, &who, &role),
            ),
            Action::ForgetEdit { file, path } => hub::after_button(
                hub::Page::Edits,
                atlas::yourchanges::forget(&atlas::roots::config_dir(), &file, &path).map(|_| String::new()),
            ),
            Action::RevokeAccess(_) | Action::RevokeAllAccess => Reply::html(hub::shell(
                "Atlas",
                "<p class=note>Taking access away needs Atlas itself running — this window \
                 only knows about settings. Start Atlas and open the Access page.</p>",
            )),
            Action::Appearance { what, to } => {
                let mut a: hub::Appearance = store.load(hub::APPEARANCE_KEY);
                if a.choose(&what, &to) {
                    if let Err(e) = store.save(hub::APPEARANCE_KEY, &a) {
                        eprintln!("couldn't keep that appearance choice: {e}");
                    }
                    Reply::redirect("/hub")
                } else if let Some(done) = atlas::appearance::choose(&what, &to) {
                    // From Settings → How it looks; a colourway chosen there
                    // clears the Aa menu's theme, which would otherwise win.
                    match done {
                        Ok(_) if what == "look.theme" && !a.theme.is_empty() => {
                            a.theme.clear();
                            if let Err(e) = store.save(hub::APPEARANCE_KEY, &a) {
                                eprintln!("couldn't keep that appearance choice: {e}");
                            }
                        }
                        Ok(_) => {}
                        Err(e) => eprintln!("couldn't keep that appearance choice: {e}"),
                    }
                    Reply::redirect(&format!("{}#how-it-looks", hub::Page::Settings.href()))
                } else {
                    Reply::redirect("/hub")
                }
            }
            _ => Reply::html(hub::settings_page(&settings)),
        }
    };

    // --- Answering other Atlases asking who is here ---
    //
    // Beside the hub rather than anywhere else, because the two are the same
    // fact: the announcement says "there is a door at this port", and this is
    // the process holding that door open. Started when the door opens and
    // gone when it closes, so the announcement cannot outlive the thing it
    // announces.
    //
    // Off unless you turned it on -- see `nearby.announce`, and the reason is
    // that the network you are on is not always your own. A failure here is
    // printed and not fatal: discovery is a convenience, and a machine that
    // cannot answer probes still serves every request it is sent.
    let ncfg = tools.nearby.clone();
    if ncfg.announce {
        let me = atlas::roots::install_root()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "atlas".into());
        let door = scfg.port;
        std::thread::spawn(move || {
            // Never stops: it lives as long as the hub does, and the hub is
            // the loop below. A stop that is always false is honest about
            // that rather than pretending to a lifecycle this has not got.
            if let Err(e) = atlas::nearby::answer_probes(&me, door, &ncfg, &|| false) {
                eprintln!("nearby: not answering probes ({e}). The hub is unaffected.");
            }
        });
    }

    // On threads of its own, as the running Atlas serves it (28 Sep 2026):
    // `serve_once` took one connection at a time, so a page, its icons and
    // its manifest queued behind each other and one silent connection held
    // every other one for its whole deadline. Every answer is still worked
    // out here, one at a time.
    let door = match server.threaded() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("hub: {e}");
            return;
        }
    };
    loop {
        door.wait_and_answer(1000, &mut handle);
    }
}

/// What's present, what isn't, and what to do about it.
///
/// Size, not checksums: pinning hashes breaks every time an upstream release
/// is rebuilt, and a truncated download is the failure that actually happens.
pub(super) fn report_install(cfg: &Config) {
    use atlas::install::{
        after, before, what_to_fetch, state_of, wanted, where_it_lands, State, COSTS_NOTHING,
    };

    // `install.tools_dir` and `.models_dir` are for the machine where the
    // models live on another drive, and nothing read either -- so this
    // reported every piece missing on exactly the machine they existed for.
    // `include_optional` is why the list is `wanted` rather than `pieces`.
    let icfg = cfg.tools.as_ref().map(|t| t.install.clone()).unwrap_or_default();
    let want = wanted(&icfg);

    let found: Vec<(&'static str, Option<u64>)> = want
        .iter()
        .map(|p| {
            // Anchored: `Piece.path` is install-relative by declaration,
            // like `upgrade::YOURS`. Stat'd bare, `atlas install` reported
            // every piece missing from anywhere but the install folder.
            let bytes = std::fs::metadata(atlas::roots::under_install(where_it_lands(p, &icfg)))
                .ok()
                .map(|m| m.len());
            (p.name, bytes)
        })
        .collect();

    println!("{COSTS_NOTHING}\n");
    for p in &want {
        let bytes = found.iter().find(|(n, _)| *n == p.name).and_then(|(_, b)| *b);
        let mark = match state_of(p, bytes) {
            State::Present => "have",
            State::Missing if p.optional => "    ",
            State::Missing => "need",
            State::HalfDownloaded => "part",
        };
        println!("  [{mark}] {:<22} {}", p.name, if bytes.is_some() { "" } else { p.without_it });
    }
    // `what_to_fetch`, not `plan`: the list above and the number below it have to
    // be about the same pieces. `plan` walks every one, so with
    // `include_optional` off this printed "4801MB to fetch" beside a 341MB
    // download.
    println!("\n{}", before(&what_to_fetch(&want, &found)));
    println!(
        "{}MB altogether if none of it were here.",
        atlas::install::download_mb(icfg.include_optional)
    );
    let left_out = atlas::install::pieces().len() - want.len();
    if left_out > 0 {
        println!(
            "{left_out} optional piece{} left out -- `install.include_optional: true` adds them.",
            if left_out == 1 { "" } else { "s" }
        );
    }
    if icfg.tools_dir.trim() != "tools" || icfg.models_dir.trim() != "models" {
        println!(
            "Looking in {} and {}, which is where you said.",
            icfg.tools_dir.trim(),
            icfg.models_dir.trim()
        );
    }

    let results: Vec<(&'static str, bool)> = want
        .iter()
        .map(|p| {
            let bytes = found.iter().find(|(n, _)| *n == p.name).and_then(|(_, b)| *b);
            (p.name, state_of(p, bytes) == State::Present)
        })
        .collect();
    println!("{}", after(&results));
}

/// Regenerate `docs/METRICS.md`. Lost in two merges now; `tests/metrics.rs`
/// asserts this exists so the third time fails a build instead of quietly
/// leaving a stale file that still looks authoritative.
pub(super) fn run_metrics() {
    let root = std::path::Path::new(".");
    let unwired = unwired_from_wiring_test(root);
    let m = atlas::metrics::gather(root, unwired);
    let rendered = m.render();
    let out = root.join("docs/METRICS.md");
    match std::fs::write(&out, &rendered) {
        Ok(()) => println!("{rendered}\nwritten to {}", out.display()),
        Err(e) => {
            eprintln!("could not write {}: {e}", out.display());
            println!("{rendered}");
        }
    }
}

fn unwired_from_wiring_test(root: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join("tests/wiring.rs")) else {
        return Vec::new();
    };
    let Some(start) = text.find("UNWIRED_BASELINE") else { return Vec::new() };
    let rest = &text[start..];
    let Some(open) = rest.find("= &[").map(|i| i + 3) else { return Vec::new() };
    let Some(close) = rest.find("];") else { return Vec::new() };
    // Line by line, not `split(',')`.
    //
    // The comma version shipped and put its output straight into
    // `docs/METRICS.md`, where the "written but not reachable" section listed
    // things like "`tested (28`" and "`19`" and half a paragraph of a comment
    // about `afterme`. Every entry in that list is followed by a justifying
    // comment, and comments contain commas, so splitting on commas chops the
    // prose into pieces and each piece that is not empty becomes a module
    // name. The count was 18; the real one is 1.
    //
    // This is the third time this exact parser has been written and the
    // second time it has been wrong the same way -- `tests/capability_wiring.rs`
    // had it too, where a comment ending in a full stop swallowed `"mesh"`.
    // The rule that works: a baselined name is a quoted string alone on its
    // own line, and a line whose first non-space characters are `//` is prose.
    rest[open + 1..close]
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.starts_with("//") {
                return None;
            }
            let name = t.strip_prefix('"')?.split('"').next()?;
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

pub(super) fn run_mobile(cfg: &Config, args: &[String]) {
    use atlas::companion::{self, CompanionConfig, Phone, Piece};

    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("android") => {
            println!("On Android:");
            for a in atlas::android::abilities() {
                println!(
                    "  [{}] {}{}",
                    a.can.plain(),
                    a.what,
                    if a.needs_deliberate_permission { "  (needs a permission you grant in Settings)" } else { "" }
                );
                println!("      {}", a.detail);
            }
            println!();
            println!("Permissions worth understanding before you grant them:");
            for (name, what, why) in atlas::android::serious_permissions() {
                println!("  {name} — {what}");
                println!("      {why}");
            }
            println!();
            println!(
                "Wake word running all day costs about {:.1}% of the battery an hour.",
                atlas::android::wake_word_battery_percent_per_hour()
            );
        }
        Some("mirror") => {
            let store = atlas::roots::store();
            let ccfg: CompanionConfig = cfg
                .tools
                .as_ref()
                .map(|t| t.companion.clone())
                .unwrap_or_default();
            let phone: Phone = store.load("phone_mirror");
            let now = atlas::store::now();

            println!("{}", phone.state(now, &ccfg));
            println!();

            // The config is a list of strings; this is the only place that
            // turns them into the type that knows what is safe to lose in a
            // taxi. An unrecognised name is named rather than skipped — a
            // typo in `mirror:` would otherwise silently mirror nothing.
            println!("What your config says to mirror:");
            for name in &ccfg.mirror {
                let piece = match name.trim().to_ascii_lowercase().as_str() {
                    "outstanding" => Some(Piece::Outstanding),
                    "projects" => Some(Piece::Projects),
                    "notes" => Some(Piece::Notes),
                    "last_brief" => Some(Piece::LastBrief),
                    "code_counts" => Some(Piece::CodeCounts),
                    "thread" => Some(Piece::Thread),
                    _ => None,
                };
                match piece {
                    Some(p) if !p.safe_on_a_phone() => println!(
                        "  {name} — NOT safe on a phone. Atlas will not mirror it whatever this says."
                    ),
                    Some(p) => println!(
                        "  {name} — travels, {}",
                        if p.writable() { "and you can change it there" } else { "read-only there" }
                    ),
                    None => println!("  {name} — not something Atlas knows how to mirror (typo?)"),
                }
            }

            println!();
            println!("Never leaves the laptop, whatever the config says:");
            for (what, why) in companion::never_travels() {
                println!("  {what} — {why}");
            }

            println!();
            println!("How the two would talk:");
            for (how, detail, works_offline) in companion::how_they_talk() {
                println!("  {how}{} — {detail}", if works_offline { "" } else { " (needs both online)" });
            }
        }
        Some("back") => {
            let store = atlas::roots::store();
            let phone: Phone = store.load("phone_mirror");
            let pending: Vec<companion::Pending> =
                phone.waiting().into_iter().cloned().collect();
            // What moved here while the phone was away, from the same record
            // the carry set keeps. Nothing is invented: with no phone paired
            // this is an empty list against an empty list, and `merge` says so.
            let changed_here: Vec<String> = {
                let taking: Vec<atlas::workingset::Carried> = store.load("working_set");
                let packed_at: u64 = store.load("working_set_packed_at");
                taking
                    .iter()
                    .filter(|c| mtime_secs(std::path::Path::new(&c.path)) > packed_at)
                    .map(|c| c.name.clone())
                    .collect()
            };
            let m = companion::merge(&pending, &changed_here);
            let away_days = phone.mirror_age_days(atlas::store::now()).unwrap_or(0);
            let said = companion::on_return(&m, away_days);
            if said.trim().is_empty() {
                println!("Nothing waiting from a phone.");
            } else {
                println!("{said}");
            }
        }
        Some("ios") | None => {
            println!("{}", atlas::ios::first_run());
            println!();
            println!("On an iPhone or iPad:");
            for a in atlas::ios::abilities() {
                println!("  [{}] {}", a.can.plain(), a.what);
                println!("      {}", a.detail);
            }
            println!();
            println!("Flatly not possible on iOS:");
            for a in atlas::ios::cannot() {
                println!("  {} — {}", a.what, a.detail);
            }
            println!();
            println!("Ways to start it:");
            for (how, detail) in atlas::ios::ways_to_start() {
                println!("  {how} — {detail}");
            }
            println!();
            println!("The phone is better at: {}", atlas::ios::phone_is_better_at().join(", "));
            println!("The laptop is better at: {}", atlas::ios::laptop_is_better_at().join(", "));
            println!();
            println!(
                "Offline, on the phone: transcription {}, the big model {}.",
                if atlas::ios::works_offline("transcription") { "works" } else { "doesn't" },
                if atlas::ios::works_offline("the big model") { "works" } else { "doesn't" }
            );
            println!();
            println!("atlas mobile android   the same list for Android");
            println!("atlas mobile mirror    what would actually travel");
        }
        Some(other) => println!("I don't know \"{other}\" — try ios, android, mirror or back."),
    }
}

/// `atlas hub` — where Atlas can be reached from, said once.
///
/// **This does not open a browser, and that is Eric's ruling, 17 Sep 2026:**
/// *"the hub should not be a browser. You say that if Atlas has a dependency
/// on the internet."* The first version of this command launched Chrome, which
/// walked straight back into the design position the panels were rebuilt to
/// get away from. `tests/capability_wiring.rs` already records it against
/// `look`: rendering the panels as HTML "needs an external browser or a
/// bundled web engine, and the ruling for this system is in-house and
/// self-contained, so the design is painted natively by `look_paint`/`window`
/// instead."
///
/// So the server is for the PHONE, which is the job `server.rs` was written
/// for -- a phone cannot run the native window, and loopback plus a VPN is the
/// honest way to reach a desktop from one. On the desktop the surface is the
/// native window, and this command says so rather than papering over the gap.
///
/// The address still matters and still had a real defect: the token was
/// regenerated on every start, so the phone's saved URL broke on every desktop
/// reboot. `server::token_for` fixes that, which is why this prints one fixed
/// address instead of a different one each run.
pub(super) fn run_hub_address(args: &[String]) {
    let store = atlas::roots::store();
    let token = match atlas::server::token_for(&store) {
        Ok(t) => t,
        Err(e) => {
            println!("I couldn't read or make the hub token: {e}");
            return;
        }
    };
    // The configured port, because `tools.yaml` can move it and a printed
    // address that ignores the setting is wrong in exactly the cases where
    // someone changed it on purpose.
    let configured = Config::load(&atlas::roots::config_dir())
        .ok()
        .and_then(|c| c.tools.map(|t| t.server.port))
        .unwrap_or(8787);
    // Where the running Atlas's hub really answers, when it does (it opens
    // beside a taken port rather than not at all: `server::open_hub`).
    let port = atlas::server::hub_port(&atlas::roots::state_dir(), configured);

    let url = atlas::server::hub_url(port, &token, "/hub");
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("address") | Some("url") => println!("{url}"),
        None => {
            println!("On this machine: open Atlas from the Start menu or the desktop and press");
            println!("Hub or Settings -- or say \"show me the hub\" or \"show me settings\". The hub");
            println!("shows inside Atlas's own window, no browser. `atlas home hub` opens it too.");
            println!();
            println!("From your phone:  {url}");
            println!();
            println!("  Loopback only, so that address is reachable from another device only");
            println!("  through a VPN that terminates on this machine. It is the same address");
            println!("  every time now -- save it once. It used to change on every restart,");
            println!("  which is why saving it never worked.");
            println!();
            println!("It only answers while Atlas is running.");
        }
        Some(other) => println!(
            "I don't know \"{other}\" -- try `atlas hub` or `atlas hub address`.\n\
             There is no `open`: opening a browser is not how Atlas shows you things."
        ),
    }
}

/// `atlas phone` — put Atlas on your phone over Tailscale and print the link
/// (the window shows the same thing as a code to scan); `atlas phone off`
/// takes it back off.
pub(super) fn run_phone(cfg: &Config, args: &[String]) {
    let tool = atlas::phonelink::tailscale_tool();
    let vars = atlas::tools::Vars::new();
    if args.first().map(|s| s.as_str()) == Some("off") {
        match atlas::phonelink::unpublish(&tool, &vars) {
            Ok(()) => match atlas::roots::store().save(atlas::phonelink::LINK_KEY, &String::new()) {
                Ok(()) => println!("Atlas is off your phone. `atlas phone` puts it back."),
                Err(e) => println!(
                    "Atlas is off your phone, but I couldn't forget its old link ({e}), so the \
                     hub may still show it."
                ),
            },
            Err(e) => println!("That didn't take: {e}"),
        }
        return;
    }
    // The hub's real port, when the running Atlas's hub answers on another
    // than the configured one (`server::open_hub`).
    let port = atlas::server::hub_port(&atlas::roots::state_dir(), cfg.tools.as_ref().map(|t| t.server.port).unwrap_or(8787));
    let token = match atlas::server::token_for(&atlas::roots::store()) {
        Ok(t) => t,
        Err(e) => {
            println!("I couldn't read or make the hub token: {e}");
            return;
        }
    };
    let outcome = atlas::phonelink::publish(port, &token, &tool, &vars);
    println!("{}", atlas::phonelink::say(&outcome));
    if let atlas::phonelink::Serve::Published { url } = &outcome {
        if let Err(e) = atlas::roots::store().save(atlas::phonelink::LINK_KEY, url) {
            println!("(I couldn't keep the link for the hub's devices page: {e})");
        }
        println!();
        println!("{url}");
        println!();
        println!("On the phone: open it, then Share → Add to Home Screen, and Atlas is an app.");
    }
}
