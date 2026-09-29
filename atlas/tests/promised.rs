//! Everything a delivered package promised is still in the tree.
//!
//! Why this exists: the first package from the improvements chat (10 Sep
//! 2026, "work off the tick") went out as a zip and only its first pass ever
//! reached the tree. Its second and third passes and four test files were
//! missing for two weeks, and nothing noticed until Eric asked. Nothing
//! checked that what a package said it delivered had actually landed.
//!
//! So each package adds its rows here before it is called done: the file,
//! a piece of code that only exists if the work does, and what it is. A merge
//! that drops any of it fails this test, by name. Rows are never deleted to
//! make it pass — a promise that's withdrawn is withdrawn in the handoff, in
//! words, and then its row goes.

const PROMISED: &[(&str, &str, &str, &str)] = &[
    // ---- 10 Sep 2026: work off the tick (first pass) ----
    ("10 Sep", "src/crew.rs", "pub fn settle(&mut self, t: u64) -> Vec<News>", "the crew: slow work off the tick"),
    ("10 Sep", "src/crew.rs", "Vanished", "an errand that dies without a word is Vanished"),
    ("10 Sep", "src/crew.rs", "pub fn why_waiting(", "a queue that isn't moving says why"),
    ("10 Sep", "src/store.rs", "if existing == body.as_bytes()", "Store::save skips identical bytes"),
    ("10 Sep", "src/daemon.rs", "fn take_crew_news(", "crew news collected every tick"),
    ("10 Sep", "src/wants.rs", "pub fn machine_from(", "recommendations against the measured machine"),
    // ---- 10 Sep 2026: second pass, crew efficiency (rebuilt 26 Sep) ----
    ("10 Sep", "src/crew.rs", "pub enum Needs", "work says what it costs"),
    ("10 Sep", "src/crew.rs", "MostlyWaiting", "the bulkhead for waiting work"),
    ("10 Sep", "src/crew.rs", "cores.saturating_sub(1)", "a core left for the person at the machine"),
    ("10 Sep", "src/crew.rs", "Blocker::Memory", "the memory margin"),
    ("10 Sep", "src/crew.rs", "Blocker::Battery", "the battery floor"),
    ("10 Sep", "src/crew.rs", "pub const AGE_UP_SECS", "urgency with ageing"),
    ("10 Sep", "src/crew.rs", "Taken::Joined", "the same work done once"),
    ("10 Sep", "src/crew.rs", "pub fn recently_finished(", "waited and ran, measured"),
    ("10 Sep", "src/crew.rs", "pub fn longest_wait(", "the one number that says the crew is too small"),
    ("10 Sep", "src/crew.rs", "pub fn wants_attention(", "no hand left idle for two seconds"),
    ("10 Sep", "src/daemon.rs", "fn crew_job(", "each crew errand's cost, decided in one place"),
    ("10 Sep", "src/settings.rs", "crew.keep_free_mb", "the memory margin is a setting"),
    ("10 Sep", "src/settings.rs", "crew.battery_floor_percent", "the battery floor is a setting"),
    ("10 Sep", "src/hublive.rs", "\"Work in hand\"", "the crew on the Status page"),
    // ---- 10 Sep 2026: third pass, idle cost (rebuilt 26 Sep) ----
    ("10 Sep", "src/daemon.rs", "pub fn persist_after(", "no full save every two seconds"),
    ("10 Sep", "src/awareness.rs", "pub fn how_long_to_wait(", "folder scans back off when nothing changes"),
    ("10 Sep", "src/daemon.rs", "tools_resolved: std::sync::Arc<", "tools config resolved once, shared"),
    ("10 Sep", "src/brain.rs", "pub fn with_keep_alive(", "the model kept warm as fit measured"),
    ("10 Sep", "src/tools.rs", "pub fn poll_gap(", "adaptive polling of running tools"),
    ("10 Sep", "src/health.rs", "fn read_power(", "the battery is actually read"),
    // ---- 10 Sep 2026: its four test files ----
    ("10 Sep", "tests/crew.rs", "fn stopped_is_not_failed", "tests/crew.rs"),
    ("10 Sep", "tests/crew_efficiency.rs", "fn the_slot_count_bounds_thinking_not_waiting", "tests/crew_efficiency.rs"),
    ("10 Sep", "tests/idle_cost.rs", "fn eight_quiet_hours_cost_about_sixteen_walks", "tests/idle_cost.rs"),
    ("10 Sep", "tests/wants.rs", "fn memory_held_by_things_you_arent_using_counts_as_free", "tests/wants.rs"),
    // ---- 26 Sep 2026: round 11, the seventeen ----
    ("round 11", "src/workday.rs", "pub fn read_first(", "the working day"),
    ("round 11", "src/marketdays.rs", "", "market days"),
    ("round 11", "src/cliphist.rs", "", "clipboard history"),
    ("round 11", "src/screentext.rs", "", "text off the screen"),
    ("round 11", "src/mailbook.rs", "", "the mail address book"),
    ("round 11", "src/waitingfor.rs", "", "waiting-for"),
    ("round 11", "src/launcher.rs", "", "the launcher"),
    ("round 11", "src/tradeday.rs", "", "the trading day"),
    ("round 11", "src/meetprep.rs", "", "meeting prep"),
    ("round 11", "src/snippets.rs", "", "snippets"),
    ("round 11", "src/findfile.rs", "", "find a file"),
    ("round 11", "src/pdfkit.rs", "", "PDFs"),
    ("round 11", "src/people.rs", "", "people"),
    ("round 11", "src/feeds.rs", "", "feeds"),
    ("round 11", "src/receipts.rs", "", "receipts"),
    ("round 11", "src/habits.rs", "", "habits"),
    ("round 11", "src/srs.rs", "", "spaced repetition"),
    ("round 11", "src/translation.rs", "", "translation"),
    ("round 11", "src/chords.rs", "", "key chords"),
    ("round 11", "tests/round11.rs", "", "round 11's tests"),
    // ---- the third chat's line, 23e -> 25h (merged 26 Sep 2026) ----
    ("23 Sep (third chat)", "src/settingswin.rs", "pub fn keep_setting(", "settings in Atlas's own window, kept the moment they change"),
    ("23 Sep (third chat)", "src/hubwin.rs", "", "the hub inside Atlas's window (WebView2)"),
    ("23 Sep (third chat)", "src/webview2_loader.rs", "", "atlas.exe starts without WebView2Loader.dll beside it"),
    // The 20 Sep mock-up, kept as history: the hub design is design/hub/.
    ("23 Sep (third chat)", "design/superseded/command-deck.html", "", "the command deck mock-up, superseded"),
    ("24 Sep (third chat)", "src/localclock.rs", "pub fn offset_secs(", "your clock, not UTC's"),
    ("24 Sep (third chat)", "src/daemon.rs", "pub fn pick_up_settings(", "settings apply without a restart"),
    ("24 Sep (third chat)", "src/speaking.rs", "", "the mark follows Atlas's real voice"),
    ("24 Sep (third chat)", "src/picture_talk.rs", "pub fn ask_until(", "say what a picture shows, stoppable"),
    ("24 Sep (third chat)", "src/callwatch.rs", "", "notice a call"),
    ("24 Sep (third chat)", "src/callrec.rs", "", "record a call, the others only after they say yes"),
    ("24 Sep (third chat)", "src/callnotes.rs", "pub fn write_up(", "write up who said what"),
    ("24 Sep (third chat)", "src/profiles.rs", "ONLY_YOU_MAY_ASK", "the envelope is yours alone"),
    ("25 Sep (third chat)", "src/daemon.rs", "fn hand_off_as(", "window replies, write-ups and the picture reader go through the crew"),
    ("25 Sep (third chat)", "src/council.rs", "pub fn room_for(", "the build and security rooms (wshobson/agents)"),
    ("25 Sep (third chat)", "THIRD_PARTY_NOTICES.md", "", "MIT attribution for what was taken"),
    ("25 Sep (third chat)", "src/council.rs", "retest_rooms", "a no says what would make it OK, and is retested"),
    ("25 Sep (third chat)", "src/activity.rs", "pub fn check_with_backups(", "the activity log checked against every backup"),
    ("25 Sep (third chat)", "src/trace.rs", "pub fn keep_example(", "graded examples kept, scrubbed"),
    ("25 Sep (third chat)", "src/recall.rs", "SearchCheck", "search measures itself when its model changes"),
    ("25 Sep (third chat)", "src/delegate.rs", "pub fn type_into_window(", "typing read back before Enter"),
    ("25 Sep (third chat)", "src/platform/idle.rs", "pub fn idle_of_yours(", "Atlas's own typing isn't counted as yours"),
    ("25 Sep (third chat)", "src/resume.rs", "", "a restart picks work back up"),
    ("25 Sep (third chat)", "src/phases.rs", "", "project work in phases kept on disk"),
    ("25 Sep (third chat)", "src/next_up.rs", "", "what needs you first"),
    ("25 Sep (third chat)", "src/knowhow.rs", "", "procedures as runbooks that learn"),
    ("25 Sep (third chat)", "src/hollowcode.rs", "", "hollow finds never-called code and made-up packages"),
    ("25 Sep (third chat)", "src/twofactor.rs", "", "two-factor codes, read out or found, and two-factor on or off"),
    ("25 Sep (third chat)", "src/webrun.rs", "", "signing in and making accounts in Atlas's own browser"),
    ("25 Sep (third chat)", "src/astype.rs", "", "correcting as you type, and learning adaptively"),
    ("25 Sep (third chat)", "src/server.rs", "pub fn guesses_worth_mentioning(", "wrong tokens at the hub are slowed"),
    // ---- the three-chat merge itself: decisions that must hold ----
    ("26 Sep (merge)", "src/localclock.rs", "pub fn set_home_zone(", "one home zone: your setting, else this machine's clock"),
    ("26 Sep (merge)", "src/tz.rs", "pub fn machine(", "unset time zone means this machine's clock"),
    ("26 Sep (merge)", "src/calendar.rs", "fn occurrences_on(", "weekly repeats on your clock without shifting twice"),
    ("26 Sep (merge)", "src/calendar.rs", "pub fn resolve_when_in(", "the third chat's calendar form, over round 9's parser"),
    ("26 Sep (merge)", "src/http.rs", "pub fn whole_reply(", "one fix for Chrome keeping the line open"),
    ("26 Sep (merge)", "src/activity.rs", "pub fn verify_seal(", "the Merkle seal, beside the chained one"),
    ("26 Sep (merge)", "src/hub.rs", "data-theme=paper", "Warm Paper by name"),
    // Eric, 26 Sep, after the merge: "one of the chats when it was doing a
    // merge still didn't pick up the hub design and was still defaulting to
    // the old one". The design locked on 20-21 Sep, in the tree and in the hub.
    ("26 Sep (Eric)", "design/hub/SPEC.md", "", "the locked hub design, kept beside the code"),
    ("26 Sep (Eric)", "design/hub/locked-2026-09-21/Main.dc.html", "", "the design's own artboards"),
    ("26 Sep (Eric)", "src/hub.rs", "fn sidebar_html(", "the design's labelled sidebar"),
    ("26 Sep (Eric)", "src/hub.rs", "fn brief_html(", "Home's Brief, carrying what's waiting on you"),
    ("26 Sep (Eric)", "src/hub.rs", "pub fn outstanding_page(", "Outstanding: tried, stopped, needs"),
    ("26 Sep (Eric)", "src/hub.rs", "pub struct NowView", "Now: the thought process as a stream"),
    ("26 Sep (Eric)", "src/look_paint.rs", "pub const WARM_PAPER", "Atlas's own windows wear the design too"),
    ("26 Sep (ruling)", "src/appearance.rs", "/// Warm Paper: Eric's ruling of 26 Sep 2026.", "Warm Paper is the default colourway"),
    ("26 Sep (ruling)", "src/appearance.rs", "pub fn settings_html(", "Settings → How it looks changes it"),
    ("26 Sep (ruling)", "src/hub.rs", "(\"paper\", \"Paper\")", "the Aa menu offers Paper"),
    ("26 Sep (ruling)", "src/activity.rs", "pub fn check(", "both seals kept: the chained one"),
    ("26 Sep (merge)", "src/plugins.rs", "\"two_factor\"", "the third chat's commands decided for add-ons"),
    ("26 Sep (merge)", "src/crew.rs", "pub fn set_margins(", "crew settings apply without a restart"),
];

#[test]
fn everything_a_package_promised_is_still_in_the_tree() {
    let lib = std::fs::read_to_string("src/lib.rs").unwrap();
    let mut missing = Vec::new();
    for (package, file, needle, what) in PROMISED {
        let Some(text) = crate::common::read_source_path(file) else {
            missing.push(format!("{package}: {what} — {file} is gone"));
            continue;
        };
        if !needle.is_empty() && !text.contains(needle) {
            missing.push(format!("{package}: {what} — `{needle}` is no longer in {file}"));
        }
        // A module file that exists but isn't declared is compiled by
        // nothing: present on disk, absent from Atlas.
        if let Some(m) = file.strip_prefix("src/").and_then(|f| f.strip_suffix(".rs")) {
            if !m.contains('/') && m != "lib" && m != "main" && !lib.contains(&format!("pub mod {m};")) {
                missing.push(format!("{package}: {what} — {file} exists but lib.rs doesn't declare it"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "delivered work has gone missing from the tree:\n  {}\n\
         Put it back, or withdraw the promise in the handoff in words first.",
        missing.join("\n  ")
    );
}

#[test]
fn each_promised_test_file_is_in_the_gate() {
    // A test file the gate doesn't run is a test that can go red unseen.
    // Autotests are off: a file runs only if tests/all.rs includes it or
    // Cargo.toml declares it.
    let cargo = std::fs::read_to_string("Cargo.toml").unwrap();
    // Line endings removed: a Windows checkout has CRLF.
    let all = std::fs::read_to_string("tests/all.rs").unwrap().replace('\r', "");
    for (_, file, _, _) in PROMISED {
        if let Some(name) = file.strip_prefix("tests/").and_then(|f| f.strip_suffix(".rs")) {
            let included = all.contains(&format!("#[path = \"{name}.rs\"]\nmod {name};"));
            let declared = cargo.contains(&format!("path = \"tests/{name}.rs\""));
            assert!(included || declared, "{file} is in neither tests/all.rs nor Cargo.toml, so nothing runs it");
        }
    }
}
