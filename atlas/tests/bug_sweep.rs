//! Finding the bugs the existing guards cannot see.
//!
//! Atlas already has several detectors and they are good at what they do:
//! `hollow` judges an answer, `hollowcode` judges code, `no_quiet_nothings`
//! reads the source for stub shapes, `wiring` checks modules and named
//! capabilities are reachable, `guards` pins load-bearing lines in place.
//!
//! What none of them ask is whether the *wiring between layers* is real.
//! Every bug this file catches was found by hand this session, after the
//! existing guards had all passed:
//!
//! * A speech command whose arguments named `{tts_model}` while Settings
//!   wrote to `voice_settings.voice`. Two voices configured, and the one you
//!   chose lost. Every guard passed.
//! * `speed`, `variation` and `sentence_gap` exposed in Settings, stored, and
//!   passed to nothing. Three sliders that moved and changed nothing you
//!   could hear. Every guard passed.
//! * Documentation describing voice as "waiting on whisper" and speaker
//!   verification as unbuilt, when both were real in the code.
//!
//! ## Why these four and not forty
//!
//! A first pass at this produced 894 "uncalled functions" and 331 "unused
//! enum variants". Both numbers are true and both are useless: a detector
//! that reports nine hundred things is a detector somebody switches off, and
//! this codebase has a standing rule against crying wolf for exactly that
//! reason.
//!
//! So each detector here is either **zero-tolerance** — it should never fire,
//! and firing means a real fault — or **ratcheted** against a measured
//! baseline that may only shrink. The same discipline as `UNWIRED_BASELINE`,
//! applied to three more classes of gap.

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Shared reading
// ---------------------------------------------------------------------------

/// YAML with its comments removed.
///
/// Learned the hard way twice in this repo. The source scanner spent a week
/// reading trailing comments as code, and the first run of the placeholder
/// check below reported `{tts_model}` as a live fault when the only surviving
/// mention was in a comment explaining that it had been removed.
fn config_without_comments() -> String {
    let raw = std::fs::read_to_string("config/tools.yaml").expect("config/tools.yaml");
    let mut out = String::with_capacity(raw.len());
    for line in raw.lines() {
        let mut in_quotes = false;
        let mut cut = line.len();
        for (i, c) in line.char_indices() {
            match c {
                '"' => in_quotes = !in_quotes,
                // A `#` inside a quoted string is data, not a comment.
                '#' if !in_quotes => {
                    cut = i;
                    break;
                }
                _ => {}
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// Every source file in the tree, including the subdirectories.
///
/// This used to read `src/` non-recursively, and that was not a small
/// omission. `src/market/` and `src/platform/` were invisible to every sweep
/// in this file, in both directions:
///
/// * three functions were **counted dead while being called** — by
///   `src/market/*` or `src/platform/*`, which the sweep could not see. Those
///   are false accusations, and a detector that cries wolf gets its ceiling
///   raised rather than its findings fixed.
/// * nineteen genuinely dead functions **inside** those directories were
///   never counted at all. `platform/` is where the Windows layer lives, so
///   the blind spot covered exactly the code least likely to be exercised
///   here.
///
/// Modules are keyed by **path**, not by file stem, and that matters as soon
/// as the walk goes into subdirectories: `src/levels.rs` and
/// `src/market/levels.rs` are two different modules that share a stem, as
/// are `src/session.rs` and `src/market/session.rs`. Keyed by stem they
/// merge, and a function in one is treated as "called by its own module"
/// when the caller is really the other — which hides exactly the thing this
/// sweep exists to find.
// `calls`, `whole_word` and `after` used to live here, for the summed
// dead-capability ceiling that stood below. They went with it: they are still
// in `tests/dead_capabilities.rs`, which is now the only place that decides
// what "nothing calls this" means. Two copies of that rule in two files was
// how the two numbers came to disagree in the first place.

fn sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // "market/levels", not "levels".
            let name = path
                .strip_prefix("src")
                .unwrap_or(&path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push((name, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new("src"), &mut out);
    // A split module's pieces are one module (27 Sep 2026); none yet.
    crate::common::fold_split_modules(out)
}

/// Every `{placeholder}` appearing in the config.
fn placeholders(yaml: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let b: Vec<char> = yaml.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '{' {
            let mut j = i + 1;
            let mut word = String::new();
            while j < b.len() && (b[j].is_ascii_lowercase() || b[j] == '_' || b[j].is_ascii_digit()) {
                word.push(b[j]);
                j += 1;
            }
            if j < b.len() && b[j] == '}' && !word.is_empty() {
                out.insert(word);
            }
            i = j;
        }
        i += 1;
    }
    out
}

/// Names the code supplies at run time, via `vars.insert("name", ...)`.
///
/// **The quote does not have to be on the same line as `insert(`.** This used
/// to split on the literal `insert("`, which sees
/// `vars.insert("lang_opt".into(), ..)` and misses
///
/// ```ignore
/// vars.insert(
///     "task_opt".into(),
/// ```
///
/// -- the same call, wrapped by rustfmt because its arguments got long. That
/// wrap made `task_opt` look supplied by nothing and the guard reported a
/// placeholder that resolves perfectly well at run time. A guard that fires on
/// line width teaches its reader to silence it, and the cheap silencer here
/// would have been to add `task_opt` to a whitelist -- which would have left
/// the next wrapped `insert` undetected for real. So: skip whitespace after
/// the paren before looking for the quote.
fn supplied_at_runtime(src: &[(String, String)]) -> HashSet<String> {
    let mut out = HashSet::new();
    for (_, text) in src {
        for part in text.split("insert(").skip(1) {
            let rest = part.trim_start();
            let Some(body) = rest.strip_prefix('"') else { continue };
            if let Some(end) = body.find('"') {
                let name = &body[..end];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out
}

/// Names defined in the `vars:` block of the config.
fn declared_vars(yaml: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut inside = false;
    for line in yaml.lines() {
        if line.starts_with("vars:") {
            inside = true;
            continue;
        }
        if inside {
            if !line.starts_with("  ") && !line.trim().is_empty() {
                break;
            }
            if let Some(colon) = line.find(':') {
                let key = line[..colon].trim();
                if !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                    out.insert(key.to_string());
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 1. A command that names something that does not exist
// ---------------------------------------------------------------------------

#[test]
fn every_placeholder_in_a_command_resolves_to_something() {
    // Zero tolerance, because the failure is invisible. A mistyped
    // `{voice_fil}` does not fail to compile, does not fail to load, and does
    // not fail any existing guard. It fails the first time Atlas tries to
    // speak, as silence, which is the hardest fault in this system to chase.
    let yaml = config_without_comments();
    let src = sources();
    let known: HashSet<String> =
        declared_vars(&yaml).union(&supplied_at_runtime(&src)).cloned().collect();

    let mut unresolved: Vec<String> = placeholders(&yaml)
        .into_iter()
        .filter(|p| !known.contains(p))
        .collect();
    unresolved.sort();
    assert!(
        unresolved.is_empty(),
        "these appear in a command and are defined nowhere -- neither in `vars:` nor supplied \
         by the code. Each one runs as a literal and fails only when that command is used:\n  {}",
        unresolved.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// 2. A setting that changes nothing
// ---------------------------------------------------------------------------

/// Settings that take effect in Rust rather than by being passed to a command.
///
/// An explicit list, and deliberately so. The first two attempts at this test
/// tried to work it out automatically — "is the field read anywhere outside
/// `settings.rs`" — and both could not fail. Removing `{speed}` from the
/// speech command left `vars.insert("speed", ...)` in place and a `self.speed`
/// read inside `VoiceSettings::args()`, a function only the tests call. The
/// test passed while the setting reached nothing, which is the exact bug it
/// exists to catch.
///
/// There are seven settings. A list of seven that someone must add to is
/// better than a heuristic that cannot fail, and adding to it forces the one
/// useful question: *how does this setting actually take effect?*
const APPLIED_IN_CODE: &[(&str, &str)] = &[
    ("self_work.source_dir", "selfwork's source lookup reads it first (selfwork.rs, `cfg.source_dir`), before looking anywhere else"),
    ("push_to_talk.key", "hotkeys::Keys::from_settings hands it to the keyboard hook at start-up"),
    ("quick_input.hotkey", "hotkeys::Keys::from_settings registers it with Windows at start-up"),
    ("persona.tone", "persona::spoken shapes the reply"),
    ("sound.speak_replies", "Daemon::say asks SoundConfig::may_speak_now, with whether the turn was typed"),
    ("sound.volume", "Voice::speak scales the synthesised WAV before it plays (sound::scale_wav)"),
    ("sound.quiet_from", "the start of that window"),
    ("sound.quiet_to", "the end of that window"),
    ("sound.popups", "Daemon::reach_you holds a note in the outbox unless SoundConfig::may_pop_up"),
    ("wake.phrase", "the wake listener matches this phrase"),
    ("persona.wit", "persona decides whether a dry remark fits"),
    ("persona.max_spoken_sentences", "persona::shape truncates the reply"),
    ("models.memory_budget_mb", "the model loader refuses to exceed it"),
    ("models.draft", "models::server_args adds -md and the draft flags when the file exists"),
    ("models.speculate", "models::server_args adds --spec-type for a draft-free mode the build supports"),
    // 30 Sep 2026: "Better answers" (`deepbrain`).
    ("models.talk", "Registry::choose_for starts the talking server on the model it names; Daemon::follow_the_talk_setting restarts it when it changes"),
    ("research.searxng_url", "research asks that SearXNG first and falls back to the built-in search"),
    // 29 Sep 2026: your own handles, set from the Social page.
    ("workday.social.youtube_channel", "social::glue's refresh reads that channel through the YouTube Data API"),
    ("workday.social.bluesky_handle", "social::glue's refresh reads that profile from Bluesky's public API"),
    ("identity.trusted_devices", "kin checks it before accepting a knock"),
    ("voice_settings.voice", "becomes {voice_file}, which the speech command uses"),
    // Offered since 28 Sep 2026.
    ("tts_engine.engine", "Voice::speak speaks through kokoro when it names Kokoro, and the configured command otherwise"),
    ("crew.keep_free_mb", "crew::Limits::keep_free_mb: nothing that thinks starts below it"),
    ("crew.battery_floor_percent", "crew::Limits::battery_floor_percent holds whole-machine chores on battery"),
    // Offered since 27 Sep 2026, set from the Sync page's first step.
    ("sync.folder", "the sync run, the invitation (household::leave_invitation) and the join (take_invitation) all read tools_cfg().sync.folder"),
    ("household.device_name", "the join form is filled with it and falls back to it; household::init names this device with it"),
    // Offered since 29 Sep 2026 (opportunity hunting).
    ("hunt.top_n", "hunting::brief_items and the voice list take the best top_n from HuntState::top"),
    ("hunt.max_requests_per_day", "HuntConfig::budget caps a day's requests at it (never above hunt::HARD_CEILING); hunting::tick skips a source that would pass it"),
];

#[test]
fn every_setting_in_the_hub_reaches_something() {
    // The bug this session found by hand: `voice_settings.speed`, `.variation`
    // and `.sentence_gap` were listed in Settings, stored, and saved when you
    // changed them — while the speech command was
    // `["-m", "{tts_model}", "-f", "{out_wav}"]`. Three sliders that moved and
    // changed nothing you could hear, and every existing guard passed.
    let yaml = config_without_comments();
    let reachable = placeholders(&yaml);
    let settings = std::fs::read_to_string("src/settings.rs").expect("src/settings.rs");

    let mut dead = Vec::new();
    for part in settings.split("key: \"").skip(1) {
        let Some(end) = part.find('"') else { continue };
        let key = &part[..end];
        if !key.contains('.') {
            continue;
        }
        let field = key.rsplit('.').next().unwrap_or("");
        if reachable.contains(field) {
            continue;
        }
        if APPLIED_IN_CODE.iter().any(|(k, _)| *k == key) {
            continue;
        }
        dead.push(key.to_string());
    }
    dead.sort();
    dead.dedup();
    assert!(
        dead.is_empty(),
        "these are offered in Settings and reach nothing — changing them does nothing at all.\n\
         Either pass the value to a command as a placeholder, or add it to APPLIED_IN_CODE \
         saying how it takes effect:\n  {}",
        dead.join("\n  ")
    );
}

#[test]
fn the_applied_in_code_list_does_not_describe_settings_that_are_gone() {
    // A list entry for a setting nobody offers any more is a claim that
    // something works, kept alive past the thing it described.
    let settings = std::fs::read_to_string("src/settings.rs").expect("src/settings.rs");
    let mut stale = Vec::new();
    for (key, _) in APPLIED_IN_CODE {
        if !settings.contains(&format!("key: \"{key}\"")) {
            stale.push(*key);
        }
    }
    assert!(stale.is_empty(), "these are excused but no longer exist: {stale:?}");
}

#[test]
fn every_excused_setting_says_how_it_takes_effect() {
    for (key, how) in APPLIED_IN_CODE {
        assert!(
            how.len() > 12,
            "{key} is excused without saying how it works, which is how a list like this rots"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Documentation that describes a worse Atlas than the one that exists
// ---------------------------------------------------------------------------

/// Documentation that still says something is unbuilt which is wired.
///
/// Measured on 9 Sep 2026. May only shrink.
///
/// Matched as a substring of "file: line", so a fragment identifying the doc
/// and the claim is enough. Each entry is a doc to fix, not a rule.
///
/// **This was `STALE_DOC_CLAIMS: usize = 7` on this side until the second
/// 17 Sep merge, and the improvements chat's version is better.** A count
/// says seven documents are wrong and names none of them, so the only way to
/// act on it is to re-derive the list; and the cheap way to make it pass is
/// to write `8`. A named list puts the document you are excusing in the diff,
/// which is the thing a number can never do. Both trees flagged the same
/// seven lines, which is what made the swap safe to take wholesale.
const STALE_DOC_BASELINE: &[&str] = &[
    // not a claim that anything is unbuilt -- it says the opposite, and the
    // matcher is reading "waiting on" beside a wired module name. A false
    // positive kept rather than silenced, so the matcher's weakness stays
    // visible.
    "BUILD_PLAN.md: **Done, waiting on hardware:**",
    // same false positive, same line shape.
    "BUILD_PLAN.md: **Done, waiting on a device:**",
    // `memory.rs` exists and is wired, but the section is about a
    // *different* memory -- the long-horizon one that survives
    // consolidation. Genuinely still open; the heading collides with a
    // module name.
    "OPEN_ITEMS.md: ## 3. Memory",
    // `pipeline.rs` is wired as a stage machine; the section is about Atlas
    // driving the whole loop unattended, which is not. Same heading
    // collision.
    "OPEN_ITEMS.md: ## 4. Self-driving pipeline",
    // true. `awareness.rs` is built and the Layer 2 wiring it describes is
    // not done.
    "GAPS.md: needs Layer 2 awareness",
    // a historical argument *for deleting* the vault, kept because the
    // reasoning is still worth reading. The vault was built instead,
    // deliberately.
    "AUDIT.md: ## 4.",
];

#[test]
fn documentation_does_not_describe_a_worse_atlas_than_the_one_that_exists() {
    // A doc claiming a worse state than reality is a smaller version of the
    // same problem as one claiming a better state, and this is the codebase
    // where that distinction is supposed to matter most. It also has a
    // practical cost: work gets planned against the docs, and features get
    // rebuilt because nobody knew they were finished.
    let unwired: HashSet<String> = std::fs::read_to_string("tests/wiring.rs")
        .expect("tests/wiring.rs")
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix('"').and_then(|r| r.split('"').next()).map(|s| s.to_string())
        })
        .collect();
    let modules: Vec<String> = sources().into_iter().map(|(n, _)| n).collect();

    let mut claims = Vec::new();
    let Ok(dir) = std::fs::read_dir("docs") else { return };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        // A raw dump of every module's own doc comment, not authored
        // status-tracking prose -- it quotes historical narrative text
        // ("the capability page used to say...") and function names that
        // happen to contain "waiting" as part of describing what they fix,
        // both of which this scan correctly can't tell apart from a real
        // stale claim. What this guard checks for doesn't apply to a
        // verbatim source-comment reproduction.
        //
        // Matched by prefix, not by exact filename. The exemption was
        // originally pinned to `MODULE_REFERENCE_2026-09-12.md`, and the
        // next regeneration of the same file under the next day's date
        // (`MODULE_REFERENCE_2026-09-13.md`) walked straight back into the
        // guard with the same two false findings the exemption was written
        // for -- an exemption that expires the moment the file it describes
        // is regenerated is not an exemption, it is a delayed failure. The
        // property that earns it is how the file is produced, which the
        // name says and the date does not.
        if file.starts_with("MODULE_REFERENCE_") {
            continue;
        }
        // Same reasoning, different generator. `CAPABILITIES.md` is written
        // by `capability::as_markdown()` from `capability::all()`, and
        // `tests/catalogue.rs` fails if the file on disk and that function
        // disagree — so it cannot fall behind the code, which is the only
        // thing this guard is for.
        //
        // It trips anyway, and the reason is worth writing down rather than
        // silencing. This scan's idea of "wired" is `tests/wiring.rs`'s:
        // something outside the module names it. The catalogue's `Planned`
        // is the stricter one `tests/capability_wiring.rs` keeps: nothing
        // reaches what the module is *for*. `certainty` is named from main
        // and nothing ever asks it whether Atlas is sure, so it is wired by
        // the first rule and not built by the second. The catalogue is the
        // one telling the truth. Exempting it here is not excusing a stale
        // document; it is declining to let the weaker of two definitions
        // overrule the stronger.
        //
        // The old hand-written version of this file never tripped, because
        // it claimed those same capabilities were *working*.
        if file == "CAPABILITIES.md" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines() {
            let l = line.to_lowercase();
            let says_unbuilt = ["not built", "isn't built", "unbuilt", "waiting on", "not wired"]
                .iter()
                .any(|p| l.contains(p));
            if !says_unbuilt {
                continue;
            }
            // Only counts when it names a module that is actually wired, and
            // names it as a whole word. Without the word boundary, "waiting on
            // hardware" matched `hardware`-adjacent module names and "the path
            // you're waiting on" matched `path`-adjacent ones -- two false
            // findings that would have been baselined in as real.
            if modules.iter().any(|m| m.len() > 4 && !unwired.contains(m) && names_word(&l, m)) {
                claims.push(format!("{file}: {}", line.trim()));
            }
        }
    }
    // Both directions, which is the point of a named list over a count.
    let unexplained: Vec<&String> =
        claims.iter().filter(|c| !STALE_DOC_BASELINE.iter().any(|b| c.contains(b))).collect();
    assert!(
        unexplained.is_empty(),
        "documentation has fallen further behind the code -- these claim something \
         is unbuilt that is wired:\n  {}\n\nFix the doc, or add it to \
         STALE_DOC_BASELINE with why it is still there.",
        unexplained.iter().map(|c| c.as_str()).collect::<Vec<_>>().join("\n  ")
    );
    let fixed: Vec<&&str> = STALE_DOC_BASELINE
        .iter()
        .filter(|b| !claims.iter().any(|c| c.contains(**b)))
        .collect();
    assert!(
        fixed.is_empty(),
        "these are baselined as stale documentation and are not stale any more. \
         Delete them from STALE_DOC_BASELINE -- a baseline that outlives what it \
         describes is how a list stops meaning anything:\n  {fixed:?}"
    );
}

/// Does `haystack` mention `word` as a whole word?
///
/// `contains` alone matches a module name buried inside an unrelated one,
/// which turns a precise detector into a noisy one at exactly the moment it
/// is being trusted enough to have a baseline written down.
fn names_word(haystack: &str, word: &str) -> bool {
    let boundary = |c: char| !(c.is_alphanumeric() || c == '_');
    let mut from = 0;
    while let Some(at) = haystack[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        let before_ok = start == 0 || haystack[..start].chars().next_back().is_some_and(boundary);
        let after_ok = end >= haystack.len() || haystack[end..].chars().next().is_some_and(boundary);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
        if from >= haystack.len() {
            break;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// 4. Capabilities that exist and are never used
// ---------------------------------------------------------------------------

/// Free `pub fn`s in wired modules that no other module calls.
///
/// Measured on 9 Sep 2026. May only shrink.
///
/// This is a *ratchet*, not a target. Some of these are genuinely fine — a
/// public helper written for a caller that is coming, an API kept deliberately
/// for the hub. What matters is the direction: the number must not grow
/// quietly, because that is how `integrations::mark`, `voiceid::check` and
/// `hollow::judge` all ended up complete, tested and unreachable while every
/// guard passed.
// Raised once, and only for a merge.
//
// 235 was measured on the secondary session's tree, which did not contain
// `gaze`, `handshape`, `handtrack`, `handloop`, `infer` or `frames`; the main
// tree's count did not contain `speaker`, `reclaim` or `filing`. The union of
// two trees is legitimately larger than either, and no single number measured
// on one side was ever right for both — the same situation as
// `UNWIRED_BASELINE`, which merged to a figure lower than either side rather
// than to one side's.
//
// This is the one adjustment that is allowed to be upward, it is recorded as
// debt rather than as a new normal, and the ratchet resumes from here: it may
// only shrink.
//
// Raised a second time, on 11 Sep 2026, and for a different reason: this test
// was folded into `docs/island.py` for the first time that day (previously it
// could only run through `cargo test`, which needed crates.io and had never
// once been reachable). The true count was 284 the moment it could first be
// measured -- this is not new debt from that day's work, it is old debt that
// had no way to be seen until then. Wiring `live`, `stale`, `asia`,
// `standdown` and one other module's capabilities that same day (see
// `tests/capability_wiring.rs`, a sibling guard folded in alongside this one)
// brought it from 285 to 284; the remaining 284 span roughly thirty unrelated
// modules -- `speaker`, `kin`, `persona`, `workspace_view`, `audio`, `routine`,
// `diagnose`, `checks`, `reclaim` among them -- each needing its own wiring
// decision rather than a guess. Named, not fixed, because guessing at thirty
// unrelated integration points in one pass is how this codebase's other
// hollow capabilities got built in the first place. The ratchet resumes from
// here: it may only shrink.
// Raised a third time, same day, for a third reason: wiring a module in at
// all is not the same thing as wiring in every function inside it. `prose`
// and `person` went from fully invisible (excluded by `UNWIRED_BASELINE` and
// `CAPABILITY_UNWIRED`, so this sweep never looked at their insides) to
// reachable, which is real progress -- but each is a small file with several
// public functions, and only one from each was actually called at the call
// site that made them reachable. The other four were there the whole time;
// they just could not be counted while the module itself was exempt. Named
// exactly, not folded into the "roughly thirty modules" note above, because
// each is a one-line question with a real answer, not a design decision:
// `prose::may_correct_in` (which apps auto-correct is allowed in -- built for
// live dictation, which nothing calls yet), `prose::check_phrases` (a second,
// unused entry point into the same checker), `person::learn_from_edit` (the
// half of learning-from-corrections that needs a wired draft-editing flow to
// feed it, which doesn't exist -- see `BUILD_PLAN`), `person::beyond_me` and
// `person::hard_day` (both written for a self-assessment surface that isn't
// wired either). 284 -> 289. The ratchet resumes from here: it may only
// shrink.
// Raised a fourth time, same day, for the same reason as the third: fixing
// the bug in `ReviewPost`'s prose check (it was calling `apply_certain` and
// `spoken` in a shape built for live dictation, which silently discards
// `Certain` fixes rather than reporting them -- wrong for a review, whose
// whole point is telling you what's there) meant no longer calling either
// function. Both were already going to need a real caller of their own
// eventually -- `apply_certain` for the live-dictation auto-correct feature
// this was never actually wired to, `spoken` for whatever surface it's
// meant to announce a correction on. 289 -> 290. The ratchet resumes from
// here: it may only shrink.
// Raised a fifth time, 13 Sep 2026, for the same shape of reason as the
// third and fourth: wiring `returning` pulled its own free functions into
// view for the first time (it had been exempt, so this sweep never looked
// inside it), and only part of the module got a real caller that day. The
// `Address`/greeting layer (`address_change`, `confirm_address`, `greet`,
// `how_long`) is genuinely wired into `daemon.rs`'s away-handling now. The
// deeper design -- `welcome()`'s Straight/Offer/Nothing decision and
// `full_brief()`, the answer to "yes" on an offered brief -- is not: the
// existing away-briefing path predates `returning.rs` and produces a
// pre-formatted string, not the structured `Happened` list `welcome()` and
// `full_brief()` need, so wiring them for real means restructuring that
// path, not a one-line call. Named as its own future integration rather
// than guessed at here. 290 -> 291. The ratchet resumes from here: it may
// only shrink.
// Lowered a first time, 13 Sep 2026: wiring `household`'s device-pairing
// flow for real (`encode_pairing`/`decode_pairing`, `new_pairing`, `meets`,
// `saw_another`, all now called from `atlas household pair/join` in
// `main.rs`) paid down three of the functions the previous raise named
// specifically as still owed. `share_with_friend` is the one piece of that
// same cluster still unwired -- a different feature (handing a note or
// file to a friend's Atlas, not device pairing) that needs its own
// integration point. 291 -> 288. The ratchet resumes from here: it may
// only shrink.
// Lowered a second time, 13 Sep 2026, paying down exactly what the fifth
// raise named as owed. `returning::welcome` (the Straight/Offer/Nothing
// decision) and `returning::full_brief` (the answer to yes on an offered
// brief) now have real callers: `daemon.rs`'s away path builds the
// structured `Happened` list they take -- merged from the journal and the
// outbox -- instead of the pre-formatted string it used to build, and a bare
// yes on the turn after an offer runs `full_brief` against that exact list.
// That is -2.
//
// It is -1, not -2, and the difference is worth being straight about.
// `returning::how_long` used to be called from `daemon.rs` directly, for the
// "a quiet overnight still gets one line" floor. That floor is `welcome`'s
// own decision and is no longer duplicated at the call site, so `how_long`'s
// only caller is now `welcome`, inside its own module -- and this sweep
// counts a function as dead unless something *outside* its module calls it.
// It is genuinely exercised on every return; it just is not reachable from
// outside any more. Giving it an outside caller would mean inventing a use
// for it to satisfy a counter, which is the thing this file exists to catch,
// so it is left counted and explained instead. 288 -> 287.
//
// 287 -> 292 (14 Sep, second pass). Wiring `adapt.rs` made
// `adapt::first_run_message` reachable, which is -1. Making microphone
// enumeration work on more than one platform is +5, and the +5 is worth
// being straight about because four of them are not really new code:
//
//   audio::parse_devices        was always here
//   audio::parse_alsa           new -- Linux
//   audio::parse_avfoundation   new -- macOS
//   audio::listing_command      new -- picks the command per platform
//   audio::parse_listing        new -- picks the parser per platform
//
// `probe` calls all five, and `probe` is called from `main.rs`, so every one
// of them runs in production. They count as dead because this sweep asks
// whether something *outside* the module names them, and after this change
// nothing does -- `parse_devices` even flipped to dead purely because the
// stale comment in `main.rs` that used to mention it by name ("nothing calls
// it") was deleted, being no longer true.
//
// The alternative is making them private, and that costs something real: a
// Linux machine can only exercise the ALSA parser, so the Windows and macOS
// ones would lose their direct tests and be verified on no machine at all
// rather than on one. Two platforms' parsers tested beats a smaller number,
// so they stay public, counted, and explained.
//
// 292 -> 318 -> 319 (14 Sep, third pass). Almost none of this is new code.
// The sweep was reading `src/` non-recursively, so `src/market/` and
// `src/platform/` were invisible in both directions, and it matched callers
// by plain substring, so any function whose name is a suffix of another's
// counted as called by its sibling. Fixing both raised the true count from
// 292 to 318 -- that is 26 functions that were dead the whole time and could
// not be seen, including `vault::seal_bytes` hiding behind
// `vault::unseal_bytes`. Two genuinely superseded functions
// (`vault::seal_for_real`, `unseal_for_real`) were deleted in the same pass,
// and `index::default_roots` was wired.
//
// The +1 to 319 is `upgrade::check_with`, which exists so that `check`'s
// at-risk branch can be exercised: the branch guards an invariant that keeps
// it from ever firing against the real lists, and an unexercised branch
// protecting an invariant is exactly what this file exists to catch.
//
// **The number is no longer the interesting artefact.**
// `tests/dead_capabilities.rs` splits the same set four ways and names, one
// by one, every function that nothing calls at all. A new one of those fails
// the build by name. This ceiling is now the coarse backstop underneath it,
// and the two are asserted to agree exactly.
//
// 319 -> 314 (14 Sep, fourth pass). Nothing was deleted for this one: the
// detector learned that a function can be *used* without being *called*.
// `daemon.rs` reaches `nudge::link_broke` by writing
// `.map(crate::nudge::link_broke)` -- passed by name, no parenthesis -- and a
// rule that only looked for an opening bracket reported it, and five others,
// as never called while a real call site sat three characters away. The
// others were `mail::categories`, `mail::trace`, `triage::triage`,
// `brief::budget` and `daily::thread`.
//
// Comment lines are skipped now too, since every dead function in this tree
// is named in the prose explaining why it is dead -- a note about dead code
// was enough to make the code look alive.
//
// The ratchet resumes from here: it may only shrink.
//
// 314 -> 314 (14 Sep, the notes index). Worth writing down precisely because
// the number did not move while real work was done, and a ceiling that sits
// still looks like a session where nothing happened. Three functions left the
// set -- `contents::what_to_open`, `nudge::drifted` and `nudge::what_i_know_of`
// all have production callers now, and the last of those was a named orphan.
// Three entered it in the same pass: `contents::says_for`, `line_for` and
// `line_for_folder`, which `from_folder` calls and nothing outside the module
// does. That is the honest shape of this count -- it is every function not
// reached from outside its own module, helpers included, so building a
// feature out of small private pieces holds it flat even when the dead
// capability it unblocked is gone. The named-orphan list in
// `tests/dead_capabilities.rs` is the number that actually moved: 9 -> 8.
// 314 -> 312 (14 Sep, the flight recorder). Three functions left the set:
// `trace::to_line` and `trace::from_lines` described a file format nothing
// wrote or read, and `nudge::trace_line` was a named orphan whose entry said
// it was waiting on "an intent for asking" -- `Intent::ModelTrace` is that
// intent. One entered it: `daemon::model_in_use`, a private helper that reads
// the model's name out of the request body it is actually sent in, called
// only from `record_model_call`. Net two, measured rather than estimated.
// The `DEAD_CAPABILITY_CEILING` that stood here is **removed**, at Eric's
// call, because it did not work.
//
// It summed four groups from `tests/dead_capabilities.rs`, one of which --
// `helper_tested`, meaning "its own module calls it and a test covers it" --
// is live, tested, running code. So the number went *up* when the brief
// redesign split the gathering into ten well-named private functions, and it
// would have gone *down* if all ten were inlined into one unreadable one. A
// ratchet that rewards the worse version of the same code is worse than no
// ratchet, because it is obeyed.
//
// It had a second fault on top: being a sum, it could move for four unrelated
// reasons and never said which.
//
// What replaces it, in `tests/dead_capabilities.rs`: the exact `ORPHANS` list
// by name, and exact (not headroomed) counts for `test_only` and
// `helper_untested`. Three numbers that each mean one thing.

#[test]
fn the_ceilings_are_not_quietly_raised() {
    // The ratchet's own ratchet. A ceiling that can be edited upward as
    // casually as the code that breaks it is not a ratchet at all -- this repo
    // has the receipts, in `tests/ceiling.rs`, for exactly that happening.
    //
    // `DEAD_CAPABILITY_CEILING` used to be checked here too. It was removed
    // rather than protected: guarding a number that measures the wrong thing
    // only makes the wrong thing harder to fix.
    // `STALE_DOC_CLAIMS` used to be checked here as well, as a number. It is
    // now `STALE_DOC_BASELINE`, a named list, and a list needs no guard
    // against being quietly raised: adding a line to it names the document
    // you are excusing, in the diff, which is the thing a number could never
    // do. The assertion that replaced this one is in
    // `documentation_does_not_describe_a_worse_atlas_than_the_one_that_exists`
    // -- it fails both when the list grows *and* when an entry on it stops
    // being true.
    let text = std::fs::read_to_string("tests/bug_sweep.rs").expect("this file");
    assert!(
        text.contains("May only shrink."),
        "the note saying these may only shrink has been removed"
    );
    assert!(
        text.contains("const STALE_DOC_BASELINE: &[&str]"),
        "the stale-doc list went back to being a count"
    );
}

// ---------------------------------------------------------------------------
// 5. The machine Atlas is actually on
// ---------------------------------------------------------------------------

#[test]
fn atlas_measures_the_machine_rather_than_assuming_one() {
    // `fit.rs` had the budget arithmetic, the tiers and the plan, and nothing
    // ever built a `Machine` — so Atlas ran identically on a 64GB desktop and
    // an 8GB laptop, and `UNWIRED_BASELINE` listing `fit` meant no guard said
    // so. These are the two knobs that decide whether Atlas stays out of your
    // way.
    let m = atlas::fit::measure();
    assert!(m.total_ram_mb > 0, "memory read as zero, so every plan is the smallest one");
    assert!(m.cpu_cores >= 1, "no cores measured");

    let plan = atlas::fit::plan_for(&m);
    assert!(plan.concurrency >= 1, "a plan that runs nothing at a time runs nothing");
    assert!(!plan.because.trim().is_empty(), "the plan doesn't say why it chose what it chose");
}

#[test]
fn a_bigger_machine_is_allowed_to_do_more() {
    // The point of measuring. If every machine got the same plan, measuring
    // would be decoration.
    use atlas::fit::{plan_for, Machine};
    let small = Machine { total_ram_mb: 4096, free_ram_mb: 2048, cpu_cores: 4, ..Default::default() };
    let big = Machine { total_ram_mb: 65536, free_ram_mb: 48000, cpu_cores: 16, ..Default::default() };
    let (a, b) = (plan_for(&small), plan_for(&big));
    assert!(b.tier >= a.tier, "a 64GB machine planned no higher than a 4GB one");
    assert!(
        b.concurrency > a.concurrency || b.keep_model_warm,
        "the bigger machine gained nothing it could use"
    );
}

#[test]
fn a_machine_too_small_for_a_model_still_works() {
    // Degrade, never fail. Too little memory for a language model means the
    // rule-based paths do the work, not that Atlas stops.
    use atlas::fit::{plan_for, Machine};
    let tiny = Machine { total_ram_mb: 2048, free_ram_mb: 512, cpu_cores: 2, ..Default::default() };
    let p = plan_for(&tiny);
    assert!(p.concurrency >= 1, "a small machine was planned to do nothing at all");
}

#[test]
fn integrated_graphics_are_not_counted_as_spare_memory() {
    // Shared memory reported as VRAM is not extra memory. Counting it would
    // plan a model this machine cannot actually hold.
    use atlas::fit::Machine;
    let igpu = Machine { total_ram_mb: 16384, free_ram_mb: 8000, vram_mb: 2048, ..Default::default() };
    assert_eq!(igpu.usable_vram_mb(), 0, "shared graphics memory was counted as usable VRAM");
}

// ---------------------------------------------------------------------------
// 6. What Atlas is holding on disk
// ---------------------------------------------------------------------------

#[test]
fn atlas_can_say_what_it_is_holding_that_nothing_uses() {
    // `fit::what_to_drop` existed and was called by nothing, so Atlas could
    // not answer "what have you downloaded that I don't need?" — which is the
    // question you ask before deciding whether to spend a gigabyte on a
    // better voice.
    use atlas::fit::what_to_drop;
    let installed = vec![
        ("models/en_US-old-voice.onnx".to_string(), 63u64),
        ("models/ggml-base.en.bin".to_string(), 142u64),
    ];
    let referenced = vec!["ggml-base.en.bin".to_string()];
    let drop = what_to_drop(&installed, &referenced);
    assert_eq!(drop.len(), 1, "it offered to drop something in use, or missed one: {drop:?}");
    assert!(drop[0].what.contains("old-voice"));
    assert!(!drop[0].costs_you.is_empty(), "it didn't say what dropping it costs");
}

#[test]
fn something_named_in_the_config_is_never_offered_up() {
    // Read from the config rather than from access times, deliberately. A
    // model named in tools.yaml is one Atlas will reach for next time it needs
    // it — dropping it on an access-time rule would uninstall the thing you
    // are about to use.
    use atlas::fit::what_to_drop;
    let installed = vec![("models/in-use.onnx".to_string(), 99u64)];
    let referenced = vec!["in-use.onnx".to_string()];
    assert!(what_to_drop(&installed, &referenced).is_empty());
}

#[test]
fn atlas_only_surveys_the_folders_it_owns() {
    // The line that makes this safe to leave running. Atlas tidies `models/`,
    // `tools/` and `data/` — the folders it installs into and generates. A
    // tool that offers to delete from folders it does not own is one you
    // cannot leave unattended.
    let fit = std::fs::read_to_string("src/fit.rs").expect("src/fit.rs");
    let surveyed = fit
        .split("for dir in [")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .unwrap_or("");
    assert!(surveyed.contains("models"), "it stopped surveying its own models: {surveyed}");
    assert!(surveyed.contains("tools"), "it stopped surveying its own tools: {surveyed}");
    for outside in ["\"C:", "Users", "Documents", "Downloads", "/home", "AppData"] {
        assert!(
            !surveyed.contains(outside),
            "it reaches outside the folders Atlas owns: {surveyed}"
        );
    }
}
