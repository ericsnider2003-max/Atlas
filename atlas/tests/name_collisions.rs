//! The deadness the shared `calls()` rule cannot see: **name collisions
//! across modules.**
//!
//! `tests/common/mod.rs` answers "does anything call this" by looking for the
//! bare name — `foo(` anywhere, or `::foo` anywhere. That is deliberately
//! crude and it biases toward *under*-reporting, which is the safe direction
//! for the ceilings built on it. But it has a blind spot with teeth: **235 of
//! the 1072 functions it scans share a bare name with a function in another
//! module.** `spoken` is defined in 44 of them. One live `spoken(` call
//! anywhere in the tree makes all 44 look alive.
//!
//! ## What this guard scans, and what it deliberately does not (17 Sep)
//!
//! It used to match `pub fn` at column zero. It now reads **1,101 free public
//! functions** -- indented ones inside nested `mod` blocks included, and
//! `pub(crate)` too -- and skips **1,993 methods on purpose**.
//!
//! The exclusion is not a blind spot left standing; it is the rule's domain.
//! "Has a module-qualified caller" means somebody writes
//! `accounts::advice(...)`, which is how this codebase reaches a free
//! function. **A method is never reached that way** -- it is `book.advice()`
//! -- so `strict` is false for every method by construction.
//!
//! Trimming without that exclusion was tried first and produced **1,422**
//! entries: essentially every method sharing a name with anything else. That
//! is the rule firing where it has no meaning, and a list of 1,422 nobody can
//! act on is worse than the 139 it would have replaced. The method-collision
//! class is real and still unmeasured here; it needs type resolution rather
//! than a wider net, and saying so is more use than a list of noise.
//!
//! Inline `#[cfg(test)]` blocks are no longer read as the program, for the
//! same reason as `dead_capabilities.rs`: a test writing `foo::bar(...)` is
//! not the program reaching `bar` by its address.
//!
//! Measured on 16 Sep 2026: the loose rule finds 329 functions with no
//! cross-module caller. A module-qualified rule finds **465**. The difference
//! is this file's **136** entries — functions called by nothing, that look
//! called because some unrelated module happens to define the same word.
//!
//! Some are a sharper case still: `brief::run`, `craft::ladder` and
//! `finance::allowed` appear in the tree ONLY inside comments that discuss
//! them. A first pass at this guard, written in a scripting language against
//! the raw text, counted those as reachable. The implementation below skips
//! comment lines, which is why it found them where the sketch of it did not —
//! a reminder that the guard is the thing that is right, not the sketch of it.
//! (`pipeline::review` was a fourth until 20 Sep, when the self-improvement
//! loop's Build stage gave it a real caller in `Daemon::attempt_own_fix`.)
//!
//! Three that make the point, each verified by hand before being listed:
//!
//! * **`delivery::send`** — the only `send(` calls in the tree are channel
//!   sends in `input.rs` and `crew.rs`. The worst case of the class, because
//!   `send(` is everywhere in any threaded program.
//! * **`accounts::spoken`** — the one module that imports from `accounts`
//!   (`goingaway.rs`) imports `Account, SecondFactor, Stakes`, not `spoken`.
//!   The bare hits are `untrusted`'s method and `fxday::spoken`.
//! * **`consult::classify`** — the hits are `policy::classify` and
//!   `reclaim::classify`.
//!
//! ## Why a new guard instead of fixing `calls()`
//!
//! Because `calls()` is shared by `wiring.rs`, `capability_wiring.rs`,
//! `dead_capabilities.rs` and `new_capabilities_are_wired.rs`, and tightening
//! it moves all four ceilings at once, in one change, by an amount nobody
//! could review. This tree has a documented history of ratchets moving for
//! reasons unrelated to the code, and a four-guard simultaneous jump is how
//! that happens again.
//!
//! So the strict rule lives here with its own list. The existing guards keep
//! their behaviour and their history; this one names what they cannot see, and
//! like every other list in this tree it may not grow silently.
//!
//! ## What to do when this fails
//!
//! **A name appeared:** a function now has no module-qualified caller. Either
//! wire it, or add it here with the reason.
//!
//! **A name disappeared:** it gained a real caller. Delete the line. That is
//! the direction this list is supposed to move.

mod common;

use std::collections::{BTreeSet, HashSet};

/// Functions with a bare-name caller somewhere and no module-qualified one.
///
/// Sorted, so a diff reads as "these were cleared, these appeared".
const NAME_COLLISION_ONLY: &[&str] = &[
    // --- 18 Sep ---------------------------------------------------------
    // `mesh::choose` joined this list when its only caller went. That caller
    // was in `Intent::Sync`, passed four hardcoded literals, and threw the
    // answer away; wiring sync to a real carrier removed it, and what is left
    // is a function kept alive only by other modules defining `choose`.
    // `delegate::interpret` is the same shape: reached only through the
    // night's `delegate` brain, which is not wired.

    // --- 17 Sep: two arrived when the scan stopped reading test blocks ------
    //
    // Both verified by hand, as every entry here is meant to be. Neither is a
    // bug; both are deliberately production-uncalled, which is exactly what
    // this list is for.
    //
    // `fixtures::path` -- a bar-series builder for tests. Its one caller is a
    //   `use super::super::fixtures::path` inside `market/structure.rs`'s own
    //   test block. Production never reaches it, and should not.
    "answering::describe",
    "anticipate::matches",
    "anticipate::suggested",
    "asia::overnight",
    "audio::announce",
    "audio::changed",
    "awareness::describe",
    "backlog::now_secs",
    // booking::assess came off 21 Sep 2026: Intent::Booking now calls it
    // module-qualified to assess a meeting proposal, so it has a real caller.
    "brief::ask",
    "brief::budget",
    "brief::due",
    "brief::gather",
    "brief::run",
    // 26 Sep merge: the call write-up's own step (callnotes.rs), called only there;
    // collides with friends::free_name (ours).
    "callnotes::free_name",
    // 26 Sep merge: the call write-up's transcript step (callnotes.rs), called
    // there and by tests/call_notes.rs; collides with diarize::who_said_what (ours).
    "callnotes::who_said_what",
    // capture::kind_of came off 18 Sep 2026. It was reachable only because
    // "kind_of" is also a free function elsewhere; `Intent::Capture` now
    // calls it module-qualified to decide whether a caught thought is a task,
    // which is what `capture.tasks_become_work` turns into a backlog item.
    "checks::needs_approval",
    "checks::reversible",
    "clipboard::classify",
    "cloudsync::result",
    "consolidate::compact",
    // consolidate::trim went on 25 Sep 2026: the daemon now calls
    // `make_room` (H10: squeeze first, then drop, keeping a line for what
    // went), which reaches `trim` through `trim_with_stones` in its own module.
    "consolidate::trim",
    "consolidate::worth_keeping",
    "consult::settled",
    "contents::parse",
    "credentials::spoken",
    "credentials::written",
    "daily::thread",
    "delegate::interpret",
    "delivery::spoken",
    "diagnose::detail",
    "dictate::ask_which",
    "dictate::parse",
    "dictate::render",
    "editcraft::ladder",
    "enrol::read",
    "events::gaps",
    // explain::check (added 21 Sep 2026) is the fixed-Normal convenience over
    // `check_at`. The `atlas explain` handler always reads an explicit depth
    // from the request ("like I'm five", "in detail") and calls `check_at` with
    // it, so the Normal-only wrapper has no production caller and should not be
    // given a forced one -- it looks alive only because other modules define
    // `check`.
    "explain::check",
    "faithful::check",
    // `feeds::text_of` left 29 Sep 2026: `social::watchlist` now calls it as
    // `crate::feeds::text_of` to read Mastodon posts and feed titles, a real
    // module-qualified caller.
    "files::join",
    // finance::review and finance::summary came off 19 Sep 2026: `atlas money`
    // calls both module-qualified, so the scan can see which `review` and
    // which `summary` it reached.
    // 29 Sep: `filing::suggest`'s caller is now `filing::plan_folder`, in its
    // own module, which `atlas file` and "tidy my desktop" both reach by its
    // full name; `atlas file` used to call `suggest` itself. Verified by hand.
    "filing::suggest",
    "finance::allowed",
    "fixtures::path",
    "flow::expand",
    "fxday::day_of",
    "gaze::spoken",
    "gaze::verdict",
    // `getpieces::catalogue` joined 28 Sep 2026, the shape
    // `feeds::text_of` had: it is live -- `getpieces::setup_pieces` and the
    // `voice` piece set call it bare -- but its one module-qualified caller
    // was `setupwin::setup_pieces`, and that moved into `getpieces` so the
    // phone builds compile (69b6aba). `tts` and `palette` also define a
    // `catalogue`, which is all this scan can see.
    "grading::check",
    "grading::spoken",
    "handoff::should_ask",
    "handoff::spoken",
    "hollow::audit",
    // `hotkeys::heard_as` joined 26 Sep 2026 with 25k (the keys, fixed on
    // the laptop). It is live -- the key hook calls it bare, to say which
    // key it heard -- and `update_courier` has a private `heard_as` of its
    // own; nothing names the hotkeys one with its module.
    "hotkeys::heard_as",
    "http::request",
    "identity::explain",
    "improve::progress",
    // install::total_mb came off 19 Sep 2026 by being renamed to
    // `download_mb`. It collided with `retention`'s `total_mb` method, and
    // `atlas install` now has a real caller -- so the ambiguity would have
    // been a false green on the other one rather than a note here.
    "knowhow::shipped",
    "language::args",
    "language::plan",
    "learned::spoken",
    "ledger::spoken",
    "ledger::summarise",
    "levels::gaps",
    "mail::categories",
    "mail::trace",
    "mend::refusal",
    "mend::should_ask",
    "messaging::interrupts",
    // Both of these are `src/platform/mod.rs`, and both have real production
    // callers -- `platform::here()` and `platform::what_am_i()`. The scan
    // takes a module's name from its file, so a `mod.rs` is called `mod`, and
    // a caller writing the directory name it actually has cannot match. The
    // artifact is the scan's, not the code's; `what_am_i` joined `here` on
    // 19 Sep 2026 when `atlas catalog` and `capability::on_platform_full`
    // began asking which machine they are on.
    // 28 Sep 2026: called bare inside its own module by `start_once`, the
    // phone's one way to start its hub; `server::HubDoor` also has a `serve`.
    "mobile::serve",
    // 29 Sep 2026: which screen, in words and in pixels -- `platform/mod.rs`
    // again (see `mod::virtual_key` below), called as
    // `crate::platform::describe_screen` / `screens_asked_for` /
    // `monitor_under` (daemon, brain) and `super::builtin_among` (win).
    "mod::builtin_among",
    "mod::describe_screen",
    "mod::here",
    "mod::monitor_under",
    "mod::screens_asked_for",
    // --- 26 Sep (round 11) -------------------------------------------------
    // `platform::virtual_key` lives in `platform/mod.rs`, which this scan
    // names `mod`; its callers write `crate::platform::virtual_key` (chords)
    // and `super::virtual_key` (win), which reach it but don't spell `mod::`.
    // Not a collision -- nothing else defines the name -- just the one file
    // whose module name isn't its stem.
    "mod::virtual_key",
    "mod::what_am_i",
    "modes::suggested",
    // 26 Sep 2026: its only caller was trading-system code that left personal Atlas on 26 Sep 2026 (Eric: personal Atlas keeps general trading knowledge and nothing specific to his own trading system). General market knowledge, kept for the next trading caller.
    "multiframe::agreement",
    "multiframe::read_frame",
    "ocr::args",
    "ocr::tidy",
    "otherside::spoken",
    "otherside::written",
    "params::report",
    // plainchange::{ask,explain,spoken,written} came off 21 Sep 2026: the
    // plain_change intent is wired and its handler calls them module-qualified.
    "portable::coverage",
    "proactive::detect",
    "profiles::slug",
    "prose::spoken",
    "recall::words_of",
    "reclaim::classify",
    "recovery::suggest",
    "reference::is_stale",
    "reference::quoted",
    "reference::worth_keeping",
    "register::formality",
    "retention::classify",
    "returning::how_long",
    "revise::slug",
    "roots::backups_dir",
    // `roots::logs_dir` was here until 23 Sep. `atlas logs` and the doctor's
    // "log" line (`drain`) call it by its module now.
    "roots::trash_dir",
    "route::plan",
    // `selfgrant::may_land` was here until 18 Sep. It has a real
    // module-qualified caller now, from the merge of the two trees.
    "server::render",
    "server::route",
    // 26 Sep merge: the settings page's "back to default" (settingswin.rs);
    // collides with redact::Scrubber::put_back (ours).
    "settingswin::put_back",
    // signin::spoken came off 19 Sep 2026: `atlas access` calls it
    // module-qualified, so the scan can see which `spoken` it reached.
    // speech::split came off 28 Sep 2026: `Voice::prepare` and
    // `Voice::speak_kokoro` call it module-qualified.
    "stale::spoken",
    "stance::kind_of",
    "stance::spoken",
    "subject::confirm",
    "subject::wants",
    // sync::can_open, sync::merge and sync::spoken came off 18 Sep 2026 --
    // `Daemon::carry_to_your_other_devices` calls all three by name when it
    // reads a bundle out of the shared folder.
    "sync::drifting",
    "system::describe",
    "thread::now_secs",
    "timebox::size_of",
    "together::net",
    // 26 Sep merge: cleans a graded example before it is kept (trace.rs,
    // keep_example); collides with redact::Scrubber::scrub (ours).
    "trace::scrub",
    "tts::adjust",
    "tts::interpret",
    "tune::summary",
    "unsub::judge",
    // 28 Sep 2026: its caller is `update_apply::update_tick` in the same
    // module, bare; `server::HubDoor::take_news` shares the name.
    "update_apply::take_news",
    "verify::report",
    "vision::whole_picture",
    "voiceover::how_long",
    "wanted::decide",
    "watching::describe",
    "words::find",
    "words::strength",
    // 25 Sep 2026 (round 9, merging 5dda9b2): the courier's `yourchanges::diff`
    // is called bare inside its own module and by `hand_edits_survive_updates`
    // through `use`; the bare name reads as a hit because this tree also has
    // the `diff` module (`crate::diff::lines_changed`), which master doesn't.
    "yourchanges::diff",
];

fn read_tree(dir: &str) -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            if let Ok(text) = std::fs::read_to_string(&path) {
                // A split module's pieces are one module (27 Sep 2026).
                common::push_module(out, &path, stem, text);
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new(dir), &mut out);
    out
}

/// Everything one file reaches by a path that names the module.
///
/// Built once per file rather than re-scanned per candidate. The first version
/// of this guard did the naive thing -- rescan the whole tree for every
/// function -- and took over a minute, which is exactly how a guard stops
/// being run. A slow guard is a guard someone eventually skips.
struct Reach {
    /// Every `a::b` written out in this file, as `(a, b)`.
    paths: HashSet<(String, String)>,
    /// `(module, name)` pairs brought in by a `use` from that module.
    imported: HashSet<(String, String)>,
    /// Bare names this file calls at all.
    called: HashSet<String>,
}

fn word_at(bytes: &[u8], mut start: usize, end: usize) -> (usize, usize) {
    while start > 0 {
        let c = bytes[start - 1];
        if c.is_ascii_alphanumeric() || c == b'_' {
            start -= 1;
        } else {
            break;
        }
    }
    (start, end)
}

fn index(text: &str) -> Reach {
    let mut paths = HashSet::new();
    let mut imported = HashSet::new();
    let mut called = HashSet::new();

    for line in text.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let bytes = line.as_bytes();

        let mut i = 0;
        while let Some(rel) = line[i..].find("::") {
            let at = i + rel;
            let (lhs_start, lhs_end) = word_at(bytes, at, at);
            let rhs_start = at + 2;
            let mut rhs_end = rhs_start;
            while rhs_end < bytes.len() {
                let c = bytes[rhs_end];
                if c.is_ascii_alphanumeric() || c == b'_' {
                    rhs_end += 1;
                } else {
                    break;
                }
            }
            if lhs_start < lhs_end && rhs_start < rhs_end {
                paths.insert((
                    line[lhs_start..lhs_end].to_string(),
                    line[rhs_start..rhs_end].to_string(),
                ));
            }
            i = at + 2;
        }

        let mut j = 0;
        while let Some(rel) = line[j..].find('(') {
            let at = j + rel;
            let (s, e) = word_at(bytes, at, at);
            if s < e {
                called.insert(line[s..e].to_string());
            }
            j = at + 1;
        }
    }

    // `use ...::module::{a, b};` and `use ...::module::name;`. Walked over the
    // raw text because a brace list wraps across lines.
    let mut from = 0;
    while let Some(rel) = text[from..].find("use ") {
        let at = from + rel;
        let tail = &text[at..];
        let stop = tail.find(';').unwrap_or_else(|| tail.len().min(600));
        let stmt = &tail[..stop];
        if let Some(brace) = stmt.find('{') {
            let head = &stmt[..brace];
            let segs: Vec<&str> = head.trim_end().trim_end_matches("::").split("::").collect();
            if let Some(module) = segs.last() {
                let module = module.trim().to_string();
                if let Some(close) = stmt[brace..].find('}') {
                    for item in stmt[brace + 1..brace + close].split(',') {
                        let name = item.trim().split(" as ").next().unwrap_or("").trim();
                        if !name.is_empty() {
                            imported.insert((module.clone(), name.to_string()));
                        }
                    }
                }
            }
        } else {
            let mut segs: Vec<&str> =
                stmt.trim_start_matches("use ").trim().split("::").collect();
            if segs.len() >= 2 {
                let name = segs.pop().unwrap().trim().to_string();
                let module = segs.pop().unwrap().trim().to_string();
                imported.insert((module, name));
            }
        }
        from = at + 4;
    }

    Reach { paths, imported, called }
}

impl Reach {
    /// Does this file reach `module::name` by a path that names the module?
    ///
    /// Two shapes count and nothing else: `module::name` written out, or a
    /// `use` bringing exactly that name from exactly that module plus a bare
    /// call. A bare call with no such `use` does NOT count -- that is the whole
    /// point of this file.
    fn reaches(&self, module: &str, name: &str) -> bool {
        let key = (module.to_string(), name.to_string());
        self.paths.contains(&key) || (self.imported.contains(&key) && self.called.contains(name))
    }
}

fn unwired_modules() -> BTreeSet<String> {
    std::fs::read_to_string("tests/wiring.rs")
        .expect("tests/wiring.rs")
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"').and_then(|r| r.split('"').next()).map(String::from))
        .collect()
}

/// Functions a bare-name search believes are alive and a module-qualified
/// search cannot reach.
fn current() -> BTreeSet<String> {
    let src = read_tree("src");
    let unwired = unwired_modules();

    // Production text only. A `#[cfg(test)] mod tests` block writing
    // `foo::bar(...)` is a module-qualified *test* caller, and this guard's
    // whole question is whether anything in the **program** reaches a
    // function by its real address. `dead_capabilities.rs` and
    // `new_capabilities_are_wired.rs` took the same correction earlier today.
    let prod: Vec<(String, String)> = src
        .iter()
        .map(|(m, t)| (m.clone(), common::split_production_and_tests(t).0))
        .collect();
    let indexed: Vec<(String, Reach, String)> =
        prod.iter().map(|(m, body)| (m.clone(), index(body), body.clone())).collect();

    let mut out = BTreeSet::new();
    for (module, body) in &prod {
        if unwired.contains(module) || module == "main" || module == "lib" {
            continue;
        }
        // Free functions only, and the `impl` tracking below is the whole
        // reason this guard could not simply be trimmed like the others.
        //
        // The rule here is "has a **module-qualified** caller" -- somebody
        // writing `accounts::advice(...)`. That is how this codebase reaches a
        // free function, and a bare-name-only hit is real evidence of a
        // collision. **It cannot apply to a method.** A method is reached as
        // `book.advice()` and never as `accounts::advice(...)`, so `strict` is
        // false for every method by construction.
        //
        // Trimming without this exclusion was tried first and produced 1,422
        // entries -- essentially every method whose name appears anywhere else
        // in the tree. That is not the defect this guard names; it is the rule
        // being applied where it has no meaning, and a 1,422-line list nobody
        // could act on is worse than the 139-line one it replaced.
        //
        // So: indented **free** functions (inside a nested `mod`) are now
        // seen, and `pub(crate)` too. Methods stay out, and the residue is
        // named in the module doc as what this guard still cannot see.
        let mut depth: i32 = 0;
        let mut impl_at: Option<i32> = None;
        for line in body.lines() {
            let raw = line;
            let ts = raw.trim_start();
            if impl_at.is_none() && (ts.starts_with("impl ") || ts.starts_with("impl<")) {
                impl_at = Some(depth);
            }
            let inside_impl = impl_at.is_some();
            depth += raw.matches('{').count() as i32 - raw.matches('}').count() as i32;
            if let Some(a) = impl_at {
                if depth <= a {
                    impl_at = None;
                }
            }
            if inside_impl {
                continue;
            }
            let t = line.trim_start();
            let rest = match t.strip_prefix("pub fn ") {
                Some(r) => r,
                None => match t.strip_prefix("pub(crate) fn ") {
                    Some(r) => r,
                    None => continue,
                },
            };
            let name: String =
                rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if name.is_empty() || name.ends_with("_for_test") {
                continue;
            }
            let loose = indexed.iter().any(|(m, r, _)| {
                m != module && (r.called.contains(&name) || r.paths.iter().any(|(_, n)| n == &name))
            });
            let strict = indexed.iter().any(|(m, r, _)| m != module && r.reaches(module, &name));
            if loose && !strict {
                out.insert(format!("{module}::{name}"));
            }
        }
    }
    out
}

#[test]
fn no_new_function_hides_behind_another_modules_name() {
    let found = current();
    let known: BTreeSet<String> = NAME_COLLISION_ONLY.iter().map(|s| s.to_string()).collect();
    let added: Vec<&String> = found.difference(&known).collect();
    assert!(
        added.is_empty(),
        "these have no module-qualified caller, and look alive only because another module \
         defines the same bare name:\n  {}\n\nWire one, or add it to NAME_COLLISION_ONLY \
         with the reason. The list may grow; it may not grow silently.",
        added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn a_function_that_got_a_real_caller_comes_off_the_list() {
    let found = current();
    let known: BTreeSet<String> = NAME_COLLISION_ONLY.iter().map(|s| s.to_string()).collect();
    let cleared: Vec<&String> = known.difference(&found).collect();
    assert!(
        cleared.is_empty(),
        "these are listed as reachable only by name collision and now have a real \
         module-qualified caller:\n  {}\n\nGood -- delete those lines.",
        cleared.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn the_list_is_sorted_and_free_of_duplicates() {
    let mut sorted = NAME_COLLISION_ONLY.to_vec();
    sorted.sort_unstable();
    assert_eq!(NAME_COLLISION_ONLY, &sorted[..], "NAME_COLLISION_ONLY is not sorted");
    let unique: BTreeSet<&&str> = NAME_COLLISION_ONLY.iter().collect();
    assert_eq!(unique.len(), NAME_COLLISION_ONLY.len(), "duplicate entry");
}

#[test]
fn the_strict_rule_is_actually_stricter_than_the_loose_one() {
    // A guard whose rule accidentally became equivalent to the shared one would
    // pass forever while checking nothing. This pins that the two disagree, and
    // in which direction.
    assert!(
        !NAME_COLLISION_ONLY.is_empty(),
        "the strict rule now finds nothing the loose rule misses -- either the tree was cleaned \
         up (delete this guard and say so) or calls_qualified stopped being strict"
    );
}
