//! The guards, listed, so losing one fails a build.
//!
//! Three merges in a row have silently dropped a guard. `atlas metrics` lost
//! its dispatch twice, leaving `docs/METRICS.md` frozen while still reading
//! like current data. `UNWIRED_CEILING` vanished once, after which the unwired
//! list grew from 27 to 93 with every test green.
//!
//! Each time, the guard was working right up until it wasn't there, and its
//! absence was invisible precisely because a guard that doesn't exist can't
//! fail. The tests below are the thing that notices.
//!
//! This file is itself listed. If someone deletes it, the wiring guard's
//! `ORDINARY_TESTS` allowlist won't cover it, so `selfgrant` treats it as
//! out of Atlas's reach — but a human deleting it is still possible, and
//! nothing here can stop that. What it can do is make the deletion deliberate.


/// Every guard, and the string that proves it's still wired.
///
/// `(file, needle, what breaks without it)`
const GUARDS: &[(&str, &str, &str)] = &[
    (
        "tests/wiring.rs",
        "UNWIRED_CEILING",
        "the unwired list can grow without limit",
    ),
    (
        "tests/declared.rs",
        "fn no_two_source_files_are_byte_identical",
        "a rename done as a copy leaves the original behind, undeclared, and the \
         suite stops compiling -- this shipped twice (mending/revise, links/integrations)",
    ),
    (
        "tests/declared.rs",
        "fn declared_in",
        "the test-import guard goes back to checking the filesystem instead of \
         lib.rs, which is what let an undeclared-but-present module break the build",
    ),
    (
        "src/daemon.rs",
        "fn research",
        "Intent::Research goes back to answering \"Research isn't built yet\" while \
         a complete research module and a configured search/fetch tool sit unused",
    ),
    (
        "src/health.rs",
        "if !unread.is_empty()",
        "an unread machine goes back to answering All fine -- assess() only speaks \
         above zero, so no findings is exactly what an unread machine produces",
    ),
    (
        "src/health.rs",
        "fn read_disk",
        "disk goes back to being an empty stub off Windows, which Atlas targets",
    ),
    (
        "src/health.rs",
        "fn read_memory_bsd",
        "memory reads zero on macOS, which has no /proc",
    ),
    (
        "src/vault.rs",
        "seal_aead(value.as_bytes(), key)",
        "credentials go back to being stored under the repeating-key XOR -- real \
         encryption available again becomes permission to store rather than the \
         thing actually used",
    ),
    (
        "src/vault.rs",
        "pub const REAL_CRYPTO: bool = true;",
        "the vault is back to the stand-in cipher",
    ),
    (
        "src/vault.rs",
        "Algorithm::Argon2id",
        "key derivation reverts to the affine stretch that collapsed to one step",
    ),
    (
        "src/vault.rs",
        "XChaCha20Poly1305",
        "the authenticated cipher is gone -- tampering becomes undetectable again",
    ),
    (
        "src/vault.rs",
        "pub salt: Vec<u8>",
        "the per-install salt is gone, so one passphrase gives one key everywhere",
    ),
    (
        "src/vault.rs",
        "pub fn weakly_sealed",
        "secrets stored under the stand-in cipher stop being named and go back \
         to looking protected",
    ),
    (
        "src/daemon.rs",
        "crate::nudge::link_broke",
        "a connection that has started failing goes back to the log only",
    ),
    (
        "src/nudge.rs",
        "pub fn may_raise",
        "a nudge raised outside consider() bypasses the back-off and mute rules",
    ),
    (
        "src/integrations.rs",
        "pub fn dependencies",
        "the connections board is created empty again, so every reader of it \
         reads nothing while looking perfectly healthy",
    ),
    (
        "src/daemon.rs",
        "crate::integrations::mark",
        "answers stop admitting which of their sources was down -- the whole \
         point of the module, and it sat uncalled for a week",
    ),
    (
        "src/brain.rs",
        "pub model: Reached",
        "model reachability goes back to being recoverable only by string-matching \
         the phrase \"Model unreachable\"",
    ),
    (
        "tests/wiring.rs",
        "fn every_module_is_reachable_from_the_entrypoint",
        "modules can be written that nothing can ever call",
    ),
    (
        "tests/wiring.rs",
        "fn strip_config_only",
        "a config field counts as wiring again, hiding most of the backlog",
    ),
    (
        "src/main.rs",
        "fn run_metrics",
        "docs/METRICS.md freezes while still looking authoritative",
    ),
    (
        "src/main.rs",
        "Some(\"metrics\")",
        "nothing dispatches `atlas metrics`",
    ),
    (
        "src/selfgrant.rs",
        "ITS_OWN_LIMITS",
        "Atlas can edit the rules that constrain it",
    ),
    (
        "src/selfgrant.rs",
        "ORDINARY_TESTS",
        "Atlas can rewrite the tests that hold its limits",
    ),
    (
        "src/selfgrant.rs",
        "fn severity",
        "reordering the Reach enum silently changes what Atlas may do to itself",
    ),
    (
        "src/integrations.rs",
        "fn mark",
        "an answer built on a broken source stops saying so, and gets acted on anyway",
    ),
    (
        "src/integrations.rs",
        "Silence is not health",
        "an integration nobody has called in a week starts being reported as fine",
    ),
    (
        "src/trace.rs",
        "STORES_NO_CONTENT",
        "the log starts keeping the words of every prompt, unencrypted, next to your notes",
    ),
    (
        "src/trace.rs",
        "pub fn append",
        "nothing writes a model call down again, and every reader in trace.rs \
         goes back to computing over a list only a test has ever filled",
    ),
    (
        "src/daemon.rs",
        "fn record_model_call",
        "the flight recorder stops recording, so \"is the local model good \
         enough\" goes back to being an opinion rather than a measurement",
    ),
    (
        "src/daemon.rs",
        "decision.model != brain::Reached::NotNeeded",
        "every phrase the parser settles is logged as a model call, so the log \
         says Atlas asks the model about everything and busiest() stops \
         meaning anything",
    ),
    (
        "src/daemon.rs",
        "crate::trace::log_path(self.store.root())",
        "the model-call log goes back to one fixed path shared by every \
         install and every test, which is how two tests came to see each \
         other's calls",
    ),
    (
        "config/commands.yaml",
        "intent: model_trace",
        "there is no way to ask what the model has been doing, which is the \
         caller nudge::trace_line spent its whole existence waiting for",
    ),
    (
        "src/contents.rs",
        "fn drift",
        "a stale index stops being detected, and Atlas trusts it and stops looking",
    ),
    (
        "src/contents.rs",
        "fn from_folder",
        "nothing builds an index out of a real folder again, and every other \
         function in contents.rs goes back to being complete, tested and \
         unreachable -- an index nothing writes cannot drift",
    ),
    (
        "src/daemon.rs",
        "fn load_index",
        "the daemon starts with an empty index, so what Atlas knows it has is \
         nothing, and the drift check compares a folder against a blank claim",
    ),
    (
        "src/daemon.rs",
        "Some(c) => self.contents = c",
        "the index is re-derived from the folder instead of read back, which \
         makes it agree with the folder by construction -- it can never be \
         wrong, so the whole drift check silently stops meaning anything",
    ),
    (
        "config/commands.yaml",
        "\"rebuilding the index\",",
        "the exact phrase nudge::drifted offers as its relief stops parsing, so \
         saying yes to the offer agrees to nothing happening. The trailing \
         comma is load-bearing: the comment above that line quotes the phrase \
         too, and a bare needle passed on the prose while the phrase itself was \
         gone -- the same way comments made dead code look alive in bug_sweep",
    ),
    (
        "config/commands.yaml",
        "intent: rebuild_index",
        "there is no command to rebuild the notes index at all, from the nudge \
         or from a person asking",
    ),
    (
        "src/tts.rs",
        "fn speed_is_inverted",
        "every speed setting silently inverts when the engine changes",
    ),
    (
        "src/safety.rs",
        "safe_to_write",
        "an unreadable trash ledger goes back to being overwritten instead of refused",
    ),
    (
        "src/safety.rs",
        "LedgerState",
        "the trash ledger stops distinguishing empty from unreadable, and a bad read can erase everything already held",
    ),
    (
        "src/retention.rs",
        "Class::Unknown",
        "a file Atlas cannot date goes back to being dated to 1970, which is exactly what marks it as first to delete",
    ),
    (
        "src/finance.rs",
        "Skip the row rather than book it as zero",
        "an unparseable amount goes back to silently booking as a zero transaction",
    ),
    (
        "src/daemon.rs",
        "MAX_SLEEP_SECS",
        "the loop goes back to sleeping for the full backed-off interval, and a signal or typed command can sit unnoticed for up to three minutes on battery",
    ),
    (
        "src/kin.rs",
        "RESERVED",
        "a name or host containing the delimiter can corrupt an invite silently instead of being refused at encode time",
    ),
    (
        "src/main.rs",
        "first_time && (my_name.is_none()",
        "someone accepting their first pairing is told nothing further is owed, when a return block was actually needed",
    ),
    (
        "src/server.rs",
        "set_nonblocking",
        "the signal listener goes back to blocking, and one tick with nobody calling freezes the whole daemon",
    ),
    (
        "src/kin.rs",
        "DEFAULT_PORT",
        "the signal door and the settings hub share a port, and a peer's saved address stops working the next restart",
    ),
    (
        "src/kin.rs",
        "fn as_nudge",
        "a message from another Atlas gains a path to becoming a command instead of only ever a nudge",
    ),
    (
        "src/kin.rs",
        "fn as_waiting",
        "a note handed over by a friend's Atlas gains a path to somewhere other than the waiting list -- the tray, an intent, a memory write -- and arriving becomes enough to make Atlas act on it",
    ),
    (
        "src/kin.rs",
        "MAX_HANDOFFS_PER_WINDOW",
        "handed-over notes lose their own rate limit, and either flood unchecked or share the signal budget so that sending notes starves out anything urgent",
    ),
    (
        "src/server.rs",
        "fn route_handoff",
        "handed-over content stops having a routing surface of its own, and either falls into the signal path or into the general action space",
    ),
    (
        "src/kin.rs",
        "MAX_HANDOFF_FILE_BYTES",
        "a peer can hand over a file of any size, and the whole of it is read into memory by a listener that services one connection at a time",
    ),
    (
        "src/kin.rs",
        "safe_name(n)",
        "a filename chosen by whoever is sending is used to build a path unchanged, and a handoff can write outside the folder it is meant to land in",
    ),
    (
        "src/server.rs",
        "POST /handoff ",
        "the per-endpoint body cap goes away, and either file handoffs are silently truncated back to 4KB or every peer-reachable endpoint gains a multi-megabyte body cap it has no use for",
    ),
    (
        "src/household.rs",
        "HANDOFF_FOLDER",
        "a file from a peer lands in the tray folder on arrival, which is where Atlas reads -- so receiving one becomes enough to make Atlas open it",
    ),
    (
        "tests/a_document_says_when_it_stopped_being_true.rs",
        "const CURRENT",
        "docs/ goes back to holding forty-seven files of which four are true, with no way \
         to tell which is which from the outside -- being out of date is not a claim, so \
         no other guard can catch it",
    ),
    (
        "tests/a_document_says_when_it_stopped_being_true.rs",
        "fn a_document_called_current_carries_no_tree_size_number",
        "an evergreen document picks up one sentence of state on the way past and quietly \
         becomes a dated one -- this is how COST.md got \"891 tests\" in it",
    ),
    (
        "tests/metrics.rs",
        "fn what_metrics_calls_unreachable_is_a_module_name_and_not_a_piece_of_a_comment",
        "docs/METRICS.md goes back to listing chopped-up fragments of the comments in \
         UNWIRED_BASELINE as module names, and saying 18 modules are unreachable when one is",
    ),
    (
        "src/main.rs",
        "fn unwired_from_wiring_test",
        "atlas metrics loses the unwired list entirely and METRICS.md stops reporting it",
    ),
    (
        "tests/bug_sweep.rs",
        "walk(&path, out)",
        "the dead-capability sweep stops descending into src/market and src/platform, and because its assertion is `<= ceiling` a detector that suddenly sees less passes silently -- which is the failure mode a ratchet cannot catch on its own",
    ),
    (
        // Moved twice, and the second move is the point. It was in
        // bug_sweep.rs and dead_capabilities.rs; the ceiling removal took one
        // copy, and then new_capabilities_are_wired.rs turned out to hold a
        // third, byte-identical. Two lists that must agree about which
        // functions are dead were agreeing because two copies happened to
        // match. It lives in tests/common/mod.rs now and both files read it.
        "tests/common/mod.rs",
        "pub fn calls",
        "the sweep goes back to substring matching, so any function whose name is a suffix of another's counts as called by its sibling -- twelve were hidden that way, including two in the vault",
    ),
    (
        "tests/dead_capabilities.rs",
        "const ORPHANS",
        "the enumerated list of functions nothing calls goes back to being a bare number, and a new one stops failing the build by name",
    ),
    // 29 Sep 2026: the pick moved into the library (`hearing::pick_microphone`)
    // so the running Atlas can pick again when a headset connects; start-up
    // (`main.rs`) and the running Atlas both call it.
    (
        "src/hearing.rs",
        "let choice = hearing.decide(w, &tc.hearing, now);",
        "the microphone goes back to being picked by how its name looks rather than by whether it can actually hear you -- a laptop shut on a stand behind two monitors has the most built-in-looking name there is",
    ),
    (
        "src/main.rs",
        "atlas::hearing::pick_microphone(&devices, &mut hearing, tc, &whereabouts",
        "start-up stops using the pick that listens for which microphone hears you",
    ),
    (
        "src/voice.rs",
        "self.listen_until_you_stop(",
        "every turn goes back to recording for a fixed eight seconds, cutting you off mid-word and making you wait seven seconds after \"yes\"",
    ),
    (
        "src/audio.rs",
        "fn level_db",
        "nothing measures how loud a window of audio was, so endpoint.rs has nothing to decide from and the fixed stopwatch is the only option again",
    ),
    (
        "tests/new_capabilities_are_wired.rs",
        "const KNOWN",
        "the backlog of built-but-never-called capabilities goes back to growing silently -- a count alone cannot tell \"cleared three, added three\" from \"did nothing\"",
    ),
    (
        "src/index.rs",
        "return default_roots()",
        "file indexing goes back to having zero usable roots on Linux and macOS, because the shipped allowlist is written in %USERPROFILE% and nothing there sets it",
    ),
    (
        "src/config.rs",
        "adapt::Machine::load",
        "config/machine.yaml stops being read, so everything `atlas adapt` works out about this computer is written to a file nothing loads -- which is the state adapt.rs sat in for its whole existence",
    ),
    (
        "src/audio.rs",
        "fn listing_command",
        "device enumeration goes back to asking every platform the Windows question, so mic auto-detection silently does nothing on Linux and macOS and falls back to a Windows device name that cannot work there",
    ),
    (
        "src/settings.rs",
        "fn build",
        "the settings registry goes back to declaring a default beside each live value, which is the arrangement that had the hub reporting three settings as unchanged when they were changed",
    ),
    (
        "src/kin.rs",
        "MAX_PER_WINDOW",
        "a misbehaving or compromised peer can flood this channel until urgent stops meaning anything",
    ),
    (
        "src/server.rs",
        "route_signal",
        "a peer credential gains a path into the general action space -- Say, Approve, Deny, HubSet",
    ),
    (
        "src/audio.rs",
        "laptop_screen_active",
        "a mic sealed inside a shut laptop lid becomes offerable again because Windows still lists it",
    ),
    (
        "src/main.rs",
        "probe_devices",
        "voice mode goes back to trusting a static guessed device name instead of what is actually plugged in",
    ),
    (
        "src/recall.rs",
        "relative_floor",
        "a weak, contradictory hit rides along beside a strong one because it clears the absolute floor",
    ),
    (
        "src/recall.rs",
        "fn clarity",
        "two equally good, contradictory hits get handed over silently instead of flagged",
    ),
    (
        "src/revise.rs",
        "REPEATS_NEEDED",
        "Atlas rebuilds its own instructions from a single bad day",
    ),
    (
        "src/revise.rs",
        "fn proposal",
        "Atlas rewrites its own instructions without showing you the change",
    ),
    (
        "src/hollow.rs",
        "ADMITS_INCOMPLETE",
        "the detector for code that tells a person it isn't built stops existing",
    ),
    (
        "src/hollow.rs",
        "fn judge_readings",
        "an all-zero measurement can pass as a healthy machine again",
    ),
    (
        "src/doctor.rs",
        "fn machine_findings",
        "doctor stops looking at memory and disk, which is how the stub hid",
    ),
    (
        "src/brief.rs",
        "NEVER_SENDS",
        "the morning run can send mail instead of drafting it",
    ),
    (
        "src/selfwork.rs",
        "pub fn what_holds_it_back",
        "a fix Atlas made can land without checking that it is a fix -- a \
         deleted test, a silenced warning or a widened type makes the check \
         pass and removes the thing that would have noticed",
    ),
    (
        "src/selfwork.rs",
        "strip_prefix(root)",
        "the allowlist is handed an absolute path again, which starts with \
         none of may_touch -- so either every change is refused, or worse, \
         the refusal stops meaning anything",
    ),
    (
        "src/selfwork.rs",
        "pub fn land",
        "nothing puts a passing fix on the machine, so \"fixed then and \
         there\" is a sentence describing what would have changed",
    ),
    (
        "src/daemon.rs",
        "fn park_for_you",
        "being asked for approval while you are away goes back to \"I'll wait\" \
         with nothing recorded -- a promise Atlas does not keep, and \
         Blocker::NeedsYourDecision goes back to having no producer at all",
    ),
    (
        "src/mend.rs",
        "pub fn about_approval",
        "nothing builds a mend::Question again, so the only thing that can \
         produce a needs-your-decision item has no caller",
    ),
    (
        "src/store.rs",
        "if existing == body.as_bytes()",
        "every save writes and renames again whether or not anything changed \
         -- thirteen files and twenty-six filesystem operations on every idle \
         tick, to put back bytes that were already there",
    ),
    (
        "src/models.rs",
        "pub fn budget_bytes",
        "how much memory a model may use goes back to being a number typed \
         into a config file rather than the memory fit.rs measures -- and a \
         budget larger than the machine produces a model that will not load",
    ),
    (
        "src/daemon.rs",
        "fn which_model",
        "nothing scans the models folder, so models.rs is complete and \
         unreachable again and Atlas cannot say what it could run",
    ),
    (
        "config/tools.yaml",
        "memory_budget_mb: 0",
        "the shipped config hardcodes a memory measurement again -- it stops \
         being true the moment a browser closes or Atlas runs on the other \
         machine",
    ),
    (
        "src/daemon.rs",
        "crate::revise::standing",
        "nothing Atlas has been told twice reaches the model again, so a \
         lesson is written to a place nothing reads -- the exact failure \
         revise.rs's first rule is written against",
    ),
    (
        "src/revise.rs",
        "MAX_STANDING",
        "the standing instructions grow with every correction until they \
         crowd out the thing you just said",
    ),
    (
        "src/daemon.rs",
        "fn got_it_wrong",
        "nothing builds a Correction again, so Mending::heard never fires, no \
         Edit is produced, and repeat_rate divides by an empty list",
    ),
    (
        "src/daemon.rs",
        "self.trace.blame",
        "a bad answer can no longer be traced back to the model call that \
         produced it -- the correction loop the flight recorder was built to \
         close",
    ),
    (
        "src/council.rs",
        "pub fn parse_opinion",
        "nothing turns what a seat said back into an Opinion, and tally, \
         Verdict, strongest_dissent and spoken all go back to computing over a \
         list only a test has ever filled",
    ),
    (
        "src/council.rs",
        "pub fn hardware_room",
        "the council goes back to arguing in the abstract -- five seats that \
         have not seen the machine are five opinions, which is the one thing a \
         council is meant not to be",
    ),
    (
        "src/daemon.rs",
        "fn ask_the_room",
        "council.rs is complete and unreachable again, and the decision about \
         when a room is worth five model calls is unmade again",
    ),
    (
        "src/daemon.rs",
        "\"council\",",
        "the room's five model calls stop being recorded, so what a council \
         costs becomes invisible until the model budget goes",
    ),
    (
        "src/brief.rs",
        "pub fn gather",
        "the brief goes back to being buildable only from an inbox, which this \
         build has no reader for -- so it computes over two empty slices and \
         always says nothing needs you",
    ),
    (
        "src/voice.rs",
        "pub brief: crate::brief::BriefConfig",
        "BriefConfig leaves ToolsConfig and there is nowhere to turn the \
         morning run on again: it can only be Default::default() at its call \
         site, which is how it stayed switched off for its whole existence",
    ),
    (
        "src/daemon.rs",
        "fn brief_now",
        "nothing assembles the brief out of what this machine already holds, \
         and brief.rs is complete and unreachable again",
    ),
    (
        "src/brief.rs",
        "needs_network",
        "a brief built with the router down leads with an email you cannot \
         open, which is worse than leading with the second thing",
    ),
    (
        "src/council.rs",
        "fn blind_prompts",
        "seats can see each other's answers, which is the whole failure a council avoids",
    ),
    (
        "src/council.rs",
        "suspiciously_unanimous",
        "a leading question that produced agreement gets reported as confidence",
    ),
    (
        "src/checks.rs",
        "NEVER",
        "Atlas can be told to dump saved credentials or run a registry cleaner",
    ),
    (
        "src/checks.rs",
        "fn is_refused",
        "the no-list exists but nothing checks against it",
    ),
    (
        "src/checks.rs",
        "fn first_pass",
        "an unattended pass stops guaranteeing a restore point before it changes anything",
    ),
    (
        "src/nudge.rs",
        "NEVER_NUDGES_ABOUT",
        "Atlas can nudge its way from your stated habits into medical opinion",
    ),
    (
        "src/nudge.rs",
        "fn is_medical",
        "the medical boundary exists but nothing checks against it",
    ),
    (
        "src/nudge.rs",
        "g.asked_why = true",
        "the one-shot becomes a repeat, which is what nagging is",
    ),
    (
        "src/policy.rs",
        "fn classify_with_policy",
        "nothing decides what needs your say-so",
    ),
    (
        "src/finance.rs",
        "MOVES_MONEY",
        "Atlas will click anything on a banking page",
    ),
    (
        "src/enrol.rs",
        "const PAYMENT",
        "a signup that wants a card no longer stops the run",
    ),
    (
        "src/enrol.rs",
        "const HUMAN_CHECK",
        "Atlas stops handing robot checks over to you",
    ),
    (
        "src/signals.rs",
        "fn gather",
        "the self-audit goes back to reading an empty vector",
    ),
    (
        "tests/guards.rs",
        "const GUARDS",
        "nothing notices the next guard that goes missing",
    ),    (
        "src/daemon.rs",
        "self.hub_server.take()",
        "the hub stops being served by the running Atlas, and every page but \
         settings goes back to answering that it needs Atlas running",
    ),
    (
        "src/server.rs",
        "fn poll_once",
        "the hub can only be served by a loop that blocks, which is why it was \
         settings-only in the first place",
    ),
    (
        "src/hub.rs",
        "pointerdown",
        "dragging stops working on touch — a dashboard you can only rearrange \
         with a mouse cannot be rearranged on a phone",
    ),
    (
        "src/hub.rs",
        "fn shell_with",
        "settings and the waiting count stop being one click from every page",
    ),
    (
        "src/hub.rs",
        "fn access_page_full",
        "the access page goes back to a site list nobody fills, so it renders \
         nothing on a machine with a live browser session and a vault -- and a \
         security page that says nothing reads as nothing to worry about",
    ),
    (
        "tests/tray.rs",
        "fn nothing_turns_what_was_fetched_into_something_atlas_does",
        "a fetched page becomes an instruction, and any site on the internet \
         can drive Atlas in Eric's name",
    ),
    (
        "src/daemon.rs",
        "std::fs::remove_file(frame)",
        "frames stop being deleted after they are read, so watching an hour of \
         video leaves a folder of screenshots behind -- the exact cost Eric \
         said made screenshots the wrong answer",
    ),
    (
        "src/daemon.rs",
        "select='gt(scene,",
        "frames get sampled on a timer again: hundreds of near-identical \
         pictures of a static slide, and still a missed frame at the one second \
         something appeared",
    ),
    (
        "src/hollowcode.rs",
        "!control.iter().any(|c| head.starts_with(c))",
        "every guard clause in every codebase gets flagged as an unfinished \
         function, and a tool that cries wolf is switched off on its first run",
    ),
    (
        "src/hollowcode.rs",
        "can't turn it into working Rust",
        "Atlas starts claiming it can port code between languages -- a \
         translation that compiles and quietly means something else is worse \
         than no translation, and nobody could tell which one they got",
    ),
    (
        "src/daemon.rs",
        "if text.trim().is_empty()",
        "frames that read as nothing get deleted along with the readable ones, \
         so a chart, a photo or someone pointing at something vanishes -- \
         partial context arrived at from the other direction",
    ),
    (
        "src/daemon.rs",
        "fn keep_thumbnail",
        "kept frames go back to full size, which is the disk cost that made \
         screenshots the wrong answer in the first place",
    ),
    (
        "src/daemon.rs",
        "self.eyes.blind()",
        "an unread camera gets reported as an empty room -- one of those means \
         \"speak freely, he's gone\" and the other means \"you have no idea\"",
    ),
    (
        "src/daemon.rs",
        "fn gesture_answers",
        "a hand stops being routed through the same path a spoken yes takes, \
         so a misread gesture can reach something a word could not",
    ),
    (
        "src/infer.rs",
        "if pixels.len() != want",
        "a wrong-sized frame reaches the inference engine and panics inside \
         it, taking the whole daemon down over a resize bug",
    ),
    (
        "src/infer.rs",
        "fn whats_missing",
        "a missing model file goes back to the feature silently doing nothing \
         instead of saying which one-off download is needed",
    ),
    (
        "src/frames.rs",
        "let _ = self.child.kill();",
        "a camera process outlives the tracker, so the webcam light stays on \
         after Eric said stop -- the most visible way this could misbehave",
    ),
    (
        "src/frames.rs",
        "fn from_capture_args",
        "the continuous feed stops reusing the capture command that already \
         knows this machine's camera, making two places to get it right and \
         one to silently rot",
    ),
    (
        "src/handloop.rs",
        "sure < HAND_FLOOR",
        "the expensive landmark model runs on an empty room, which is the \
         commonest way this kind of pipeline wastes a laptop",
    ),
    (
        "src/handshape.rs",
        "!p.z.is_finite()",
        "a not-a-number depth passes a check on x and y and then poisons \
         anything reasoning about how far away a finger is",
    ),
    (
        "src/handloop.rs",
        "std::thread::Builder::new()",
        "hand tracking goes back onto the daemon tick, which sleeps up to two \
         seconds -- not a slow pointer, a broken one, and no amount of \
         prediction rescues it",
    ),
    (
        "src/handloop.rs",
        "if let Some(t) = self.joined.take()",
        "stopping stops asking rather than waiting, so a tracking thread can \
         outlive the request and keep moving the mouse after Eric said stop",
    ),
    (
        "src/handloop.rs",
        "if let Some((px, py)) = track.where_now(at)",
        "the pointer stops being predicted between detections, so it only \
         moves as often as the model runs",
    ),
    (
        "src/handshape.rs",
        "self.hand.span()",
        "gesture thresholds stop being measured against the hand's own size, \
         so every one is right at one arm's length and wrong at another -- the \
         commonest way hand gestures come out unreliable",
    ),
    (
        "src/handshape.rs",
        "if self.gestures.is_empty()",
        "features get computed for gestures nothing is bound to, which is \
         exactly the processing Eric asked not to pay for",
    ),
    (
        "src/handshape.rs",
        "Progress::Holding",
        "a held gesture stops reporting progress, so the wait becomes an \
         invisible dead interface instead of visible deliberate delay",
    ),
    (
        "src/handtrack.rs",
        "budgeted.max(wanted)",
        "the rate floor overrides the CPU budget again, letting an expensive \
         detector past its share of the core to hold a frame rate -- the \
         opposite of not slowing the machine down",
    ),
    (
        "src/handtrack.rs",
        "fn where_now",
        "the pointer stops being predicted between detections, so it lags then \
         jumps to catch up, and the jump is what makes people give up on \
         gesture control within a minute",
    ),
    (
        "src/daemon.rs",
        "self.pace\n            .took(",
        "detection cost stops being measured, so pacing goes back to an \
         assumption about a machine and a model neither of which is known here",
    ),
    (
        "src/daemon.rs",
        "pub fn watch_hands",
        "there is no way to turn hand control on at all -- steering could only \
         ever be entered from inside the steering handler, which only ran once \
         already steering",
    ),
    (
        "src/gaze.rs",
        "state.open_for < OPEN_FRAMES",
        "an instant flat palm means relaxing your hand after a drag summons \
         Atlas, every single time -- the collision Atlas's own registry warns \
         about, in Atlas's own vocabulary",
    ),
    (
        "src/gaze.rs",
        "fn snags",
        "new gestures stop being checked against the ones in use, so a \
         collision is found after a week of it firing at the wrong moment \
         rather than before it goes in",
    ),
    (
        "src/daemon.rs",
        "fn outline_under",
        "the ring around what your hand is over disappears, and a hand \
         pointing at something with no mark on screen is a hand you have to \
         guess with",
    ),
    (
        "src/daemon.rs",
        "fn note_what_was_clicked",
        "a pinch goes back to \"clicked at 840,220\" instead of \"selected \
         Send\" -- a click Atlas cannot name is one that can never be trusted \
         with anything that matters",
    ),
    (
        "src/hublive.rs",
        "fn account_safety",
        "what would lock you out stops being checked -- being unable to get \
         back in is worse than being easy to get into, and nobody checks it \
         until the day it matters",
    ),
    (
        "src/gaze.rs",
        "1.0 - h.x",
        "the camera image stops being mirrored, so your right hand moves the \
         pointer left and the whole thing feels broken in a way nobody can \
         describe",
    ),
    (
        "src/gaze.rs",
        "state.missing < GONE_AFTER",
        "one missed frame ends a drag and flings whatever you were carrying \
         across the desk",
    ),
    (
        "src/platform/mod.rs",
        "fn rect_of",
        "nothing can ask where a window was, so nothing can put it back -- \
         which is why every gesture had to be irreversible or refused",
    ),
    (
        "src/gaze.rs",
        "fn why_look",
        "the camera goes back to a timer -- watching the room when nothing \
         needs watching, and blind at the one moment a hand is held up",
    ),
    (
        "src/gaze.rs",
        "fn what_happened",
        "gestures go back to a remote control -- step through panels one at a \
         time instead of reaching out and taking hold of the one you want",
    ),
    (
        "src/daemon.rs",
        "reason.hands_may_steer()",
        "a hand can command outside steering mode -- a misread gesture that \
         answers a question is a wrong answer to a known question, one that \
         issues a command is something nobody asked for",
    ),
    (
        "src/gaze.rs",
        "min_identity_confidence",
        "one number comes to mean both \"there is a face here\" and \"this is \
         the right face\", which is how a stranger becomes a session",
    ),
    (
        "src/viewing.rs",
        "fn where_to_look",
        "watching goes back to scene changes alone, and a video whose picture \
         does not change gets zero frames -- measured on a real clip, not \
         hypothetical",
    ),
    (
        "src/viewing.rs",
        "fn one_per_moment",
        "every requested moment yields two near-identical frames, doubling the \
         text recognition and showing the same picture twice",
    ),
    (
        "src/viewing.rs",
        "fn same_screen",
        "a cursor moving over a slide becomes eight separate moments, and an \
         account that repeats itself is one nobody finishes reading",
    ),
    (
        "src/viewing.rs",
        "fn weave",
        "the screens and the transcript become two documents, neither of which \
         is the video, leaving the reader to do the stitching that is the point",
    ),
    (
        "src/tray.rs",
        "fn safe_name",
        "a filename from a phone can write outside the tray folder -- ../ in a \
         filename is the oldest trick there is",
    ),
    (
        "src/tray.rs",
        "fn fingerprint",
        "files are identified by name again, so two different photos both \
         called IMG_0042 collapse into one",
    ),
    (
        "src/server.rs",
        "self.max_upload",
        "every endpoint on the port gets the upload allowance, so any request \
         can ask Atlas to hold twenty megabytes",
    ),
    (
        "src/tray.rs",
        "fn forget_old_finished",
        "trimming stops being limited to finished items, and something handed \
         over that never got an answer is silently dropped",
    ),
    (
        "src/earned.rs",
        "if personally == Rope::AskFirst",
        "a brand-new business becomes where Atlas quietly earns its first \
         licence, with the one person who would notice a wrong tone not looking",
    ),
    (
        "src/earned.rs",
        ".min(Rope::DoAndSay)",
        "business work stops being said out loud, so a partner carries a risk \
         they never agreed to and nobody hears about it",
    ),
    (
        "src/earned.rs",
        "earned.min(ceiling)",
        "a track record can buy reach the cost of being wrong should never \
         allow -- a perfect run would earn the right to act alone on money",
    ),
    (
        "src/earned.rs",
        "fn what_would_earn_more",
        "\"not confident enough\" goes back to having no reason attached, which \
         is what makes a system feel arbitrary rather than careful",
    ),
    (
        "src/daemon.rs",
        "self.earned.note(kind",
        "the track record stops being written, so every kind of work sits at \
         ask-first forever and confidence can never be earned",
    ),
    (
        "src/hub.rs",
        "pub fn note(self)",
        "short page names lose the sentence that made them safe to shorten, and \
         the menu becomes thirteen words with no explanation anywhere",
    ),
    (
        "src/dash.rs",
        "pub fn note(self)",
        "the same, for cards: a two-word heading with nothing under it",
    ),
    (
        "src/settings.rs",
        "GROUP_ORDER",
        "the categories go back to alphabetical, which put a twenty-item bucket \
         called \"Acting\" first because of its A",
    ),
    (
        "src/hublive.rs",
        "fn holds",
        "the access page lists the whole catalogue, claiming Atlas holds a mail \
         password nobody ever gave it",
    ),
    (
        "src/hublive.rs",
        "hub::with_palette",
        "the palette stops appearing on pages, and which pages it is missing \
         from becomes a thing you find out by reaching for it",
    ),
    (
        "src/palette.rs",
        "fn score",
        "matching moves into the browser, where it becomes a second set of \
         rules that disagrees with the plain /hub/find page",
    ),
    (
        "src/hub.rs",
        "action='/hub/find'",
        "the palette stops working without script, so a slow or broken page \
         load leaves no way to reach anything by typing",
    ),
    (
        "src/accounts.rs",
        "fn undescribed",
        "an account Atlas has been told nothing about passes as a clean bill of \
         health, because the audit finds nothing wrong with what it does not know",
    ),
    (
        "src/accounts.rs",
        "_ => return None,",
        "a misspelled second-factor field falls back to a default, silently \
         recording \"password only\" against an account that has more",
    ),
    (
        "src/dash.rs",
        "fn reconciled",
        "a card added to Atlas after you saved a layout ships invisible, and a \
         card removed from Atlas either renders nothing or resets everything \
         you arranged",
    ),
    (
        "src/hub.rs",
        "value='{what}'",
        "the move buttons go, leaving dragging as the only way to rearrange — \
         which fails WCAG 2.5.7 and is unusable on a phone",
    ),
    (
        "src/hub.rs",
        "'/hub/dash'",
        "the drag path stops posting where the buttons post, making two \
         mechanisms that can disagree",
    ),
    (
        "src/daemon.rs",
        "self.timing.add(timed)",
        "the timing window stops being filled, and an unmeasured Atlas answers \
         \"nothing has been slow\" exactly like a fast one",
    ),
    (
        "src/daemon.rs",
        "fn execute_timed",
        "the typed prompt stops being timed, so only spoken turns are ever measured",
    ),
    (
        "src/daemon.rs",
        "if let Some(sig) = self.timing.got_slower()",
        "selfaudit::Kind::GotSlower goes back to being a signal nothing can raise",
    ),
    (
        "src/daemon.rs",
        "crate::asking::prepare",
        "a spoken question is searched raw again — mostly scaffolding, so the one \
         word that mattered gets diluted, and a question that points instead of \
         naming gets guessed at rather than asked about",
    ),
    (
        "src/daemon.rs",
        "self.index.missed",
        "\"found nothing\" and \"could not look\" become the same answer again",
    ),
    (
        "src/voice.rs",
        "fn last_speak_split_ms",
        "synthesis and playback fold together, so a long answer reads as a slow assistant",
    ),
    (
        "src/voice.rs",
        "const UNMEASURED",
        "an unmeasured stage becomes a measured zero, which is how a skipped step looks fast",
    ),
    (
        "src/daemon.rs",
        "self.outbox.save(&self.store)",
        "held notes stop surviving a restart, while NO_NOTIFIER and doctor both \
         keep telling you nothing is lost",
    ),
    (
        "src/daemon.rs",
        "let waiting = self.outbox.ready(t, &cfg);",
        "the outbox is drained at the start of a turn that can still end without \
         saying anything, which loses exactly what it promised to keep",
    ),
    (
        "src/daemon.rs",
        "after_wake_word: how == Arrival::Directed",
        "the wake word is detected and thrown away before the addressing check, \
         so saying \"Atlas\" then \"hello\" is judged as overheard and ignored",
    ),
    (
        "src/persona.rs",
        "pub fn social_reply",
        "greeting Atlas goes back to silence or \"I didn't catch that. Go ahead?\"",
    ),
    (
        "src/hollow.rs",
        "Why::ZeroDressedAsFine",
        "the detector goes blind to the exact bug it was written for -- a fluent \
         answer whose every number is zero",
    ),
    (
        "src/daemon.rs",
        "pub fn hollow_answers",
        "the hollow detector goes back to being called by nothing, which is the \
         bug it exists to find",
    ),
    (
        "src/daemon.rs",
        "Route::Phone =>",
        "the phone route goes and Atlas is back to holding everything when you \
         have left the building -- which means hearing nothing until you return",
    ),
    (
        "src/phone.rs",
        "NotSet::NeedsTls",
        "an https push address with no server name stops being refused up \
         front and fails as a confusing connection error at the moment \
         something urgent needed sending",
    ),
    (
        "src/phone.rs",
        "if note.private || !cfg.include_detail",
        "message bodies start landing on a lock screen anyone can read",
    ),
    (
        "src/window.rs",
        "fn can_open",
        "Atlas launches a window process on a machine with no display and reads \
         the child dying as a delivered message",
    ),
    (
        "src/main.rs",
        "Some(\"window\")",
        "the panel subcommand goes, and every window Atlas tries to open starts \
         a child that immediately fails -- macOS needs the window on a process \
         main thread, which is why it is a separate process at all",
    ),
    (
        "src/daemon.rs",
        "fn show_panel",
        "the brief, the outstanding list and the thought process stop appearing \
         on screen and go back to being spoken once and gone",
    ),
    (
        "src/notify.rs",
        "pub fn how_to_say",
        "the headphones/call/public routing goes, and Atlas is back to either \
         announcing into a room it cannot see or holding everything",
    ),
    (
        "tests/no_quiet_nothings.rs",
        "fn line_comment_at",
        "the source scanner goes back to reading trailing comments and block \
         comments as code, and truncating any line with a URL in it",
    ),
    (
        "tests/no_quiet_nothings.rs",
        "fn default_stubs_in",
        "the single-line `-> T { T::default() }` stub -- the exact bug this file \
         was written about -- stops being detected again",
    ),
    (
        "src/notify.rs",
        "pub fn shown",
        "discretion goes back to being a routing decision based on sensing the \
         room -- which cannot work (no sensor is wired) and is the wrong \
         question anyway (a coffee shop is always crowded)",
    ),
    (
        "src/daemon.rs",
        "pub fn reach_you",
        "everything Atlas wants to tell you goes back to the speakers or the log \
         -- one reaches an empty room, the other reaches nobody",
    ),
    (
        "src/daemon.rs",
        "let _ = self.outbox.collect(_t, &cfg);",
        "held notes are never handed back, which makes \"held\" a nicer word for \
         dropped -- and worse than no alerting, because you believe you were told",
    ),
    (
        "src/notify.rs",
        "pub fn reached_you",
        "whether something was delivered collapses back into whether it was \
         attempted",
    ),
    (
        "src/daemon.rs",
        "pub fn reload_library",
        "the searchable library is never populated, so every note Atlas writes \
         becomes write-only and recall's ranking runs over nothing",
    ),
    (
        "src/daemon.rs",
        "let from_notes = match &intent {",
        "the notes lookup drops below the policy gate, where an Unknown intent \
         never reaches it -- the search runs but no question ever gets there",
    ),
    (
        "src/daemon.rs",
        "self.voice_id.check(&print",
        "voice-lock goes back to being a complete, tested module with no caller -- \
         which the wiring guard cannot see, because recall::cosine makes voiceid \
         count as reachable while speaker identification has no path to it",
    ),
    (
        "src/daemon.rs",
        "crate::voiceid::Handling::Confirm => Decision::RequireApproval",
        "a voice that only might be yours stops turning a consequential action \
         into a question, and a standing grant carries when the person who gave \
         it is exactly what's in doubt",
    ),
    (
        "src/daemon.rs",
        "!matches!(crate::policy::classify(&intent), Decision::AutoProceed)",
        "consequential is read from classify_with_policy instead of classify, so \
         a prior approval silently downgrades the voice check",
    ),
    (
        "src/speaker.rs",
        "pub const NO_ENCODER",
        "an unavailable voice-lock stops being reported and starts looking like \
         an active one",
    ),
    (
        "src/main.rs",
        "fn run_enrol_voice",
        "there is no way to teach Atlas your voice, so every verdict is \
         NotEnrolled forever and the whole feature decides nothing",
    ),
    (
        "src/voice.rs",
        "v.insert(\"voice_file\".into()",
        "the voice you chose in Settings stops reaching the speech command, and \
         loses to whatever {tts_model} names",
    ),
    (
        "src/voice.rs",
        "eng.engine.speed_value(vs.speed)",
        "speed stops being converted per engine -- piper's length scale and \
         everyone else's multiplier mean opposite things, so changing engine \
         silently inverts every speed you have ever set",
    ),
    (
        "src/doctor.rs",
        "eng.is_consistent()",
        "a config whose engine and executable disagree stops being caught, and \
         that fault only ever shows up as silence",
    ),
    (
        "src/reclaim.rs",
        "pub const NEVER",
        "the never-touch list goes, and an allowlisted folder name inside \
         Documents becomes deletable",
    ),
    (
        "src/reclaim.rs",
        "trash.take(",
        "reclaim starts deleting outright instead of moving to the trash, and \
         every mistake stops being reversible",
    ),
    (
        "src/reclaim.rs",
        "if forbidden(&c.path)",
        "reclaim trusts the caller's list without rechecking it -- the survey \
         and the removal are separated by a person reading a list, and this is \
         the last cheap place to catch a mistake",
    ),
    (
        "src/reclaim.rs",
        ".split(['/', '\\\\'])",
        "path checking goes back to components(), which only knows one \
         platform's separator -- a Windows path read anywhere else arrives as \
         a single component and matches nothing on the never-touch list",
    ),
    (
        "src/system.rs",
        "system.enabled: true",
        "the master-switch refusal stops naming the setting that would allow it, \
         and a careful assistant reads as a broken one",
    ),
    (
        "src/system.rs",
        "`system.file_roots`",
        "the out-of-roots refusal stops naming how to permit the folder",
    ),
    (
        "src/main.rs",
        "atlas::system::judge(&change, &sys)",
        "filing stops going through the safety gate and can move files outside \
         the folders Atlas is permitted to work in",
    ),
    (
        "src/reclaim.rs",
        "pub fn atlas_may_move",
        "the line between what Atlas reports and what it may move disappears -- \
         reporting is safe anywhere, moving is not",
    ),
    (
        "src/vision.rs",
        "pub enum Sight",
        "a look that did not happen goes back to being reportable as an empty \
         room, which is what tells presence to speak freely about private things",
    ),
    (
        "src/vision.rs",
        "the model and the name list don't match",
        "a model with a different number of names is read with this one's names \
         -- every answer confidently wrong and nothing reporting a problem",
    ),
    (
        "src/vision.rs",
        "A face is not a password",
        "the rule stops being written where the next person changing this file \
         will read it, and a photograph held to a webcam becomes a key",
    ),
    (
        "src/infer.rs",
        "pub fn at",
        "callers go back to reading result number zero and calling it the answer \
         -- which is how a detector that runs perfectly never finds anything",
    ),
    (
        "src/infer.rs",
        "pub enum Layout",
        "every model gets the one layout again; the wrong one is not an error, \
         it is a model that runs perfectly and sees nothing recognisable",
    ),
    (
        "src/handshape.rs",
        "pub fn in_the_frame",
        "landmarks go back to the model's own pixels while everything downstream \
         multiplies them by the width of the screen -- the pointer leaves the \
         display on the first sighting",
    ),
    (
        "src/handloop.rs",
        "pub fn where_the_hand_is",
        "whether a hand is there goes back to being read off the box \
         measurements rather than the scores",
    ),
    (
        "src/vision.rs",
        "album.which_thing(",
        "the album can be added to and never read -- \"this is my mug\" is \
         accepted, stored, and never recognised again",
    ),
    (
        "src/market/structure.rs",
        "pub confirmed_at: usize",
        "a swing goes back to being knowable at the bar it happened on rather \
         than the bar it was confirmed on -- which on H4 is eight hours of \
         hindsight, and it reads as a brilliant backtest",
    ),
    (
        "src/market/bars.rs",
        "pub(crate) fn all_close",
        "the full series becomes readable from outside the module, and a reader \
         can take the whole thing and look at the end of it -- the mistake AsOf \
         exists to make unexpressible",
    ),
    (
        "src/levels.rs",
        "pub fn cost_share",
        "the spread stops being charged against the reward, and a target that \
         costs more than it pays looks like a trade",
    ),
    (
        "src/levels.rs",
        "wrong_if",
        "a level stops carrying the price at which it is wrong, which is what \
         makes it a level rather than a hope",
    ),
    (
        "src/main.rs",
        "Some(\"market\")",
        "nothing dispatches `atlas market`, so the market-state work becomes \
         two more modules that compile and never run",
    ),
    (
        "src/untrusted.rs",
        "no route into the parser",
        "text Atlas reads gets a way to become an intent, and any document it \
         is handed can issue instructions in Eric's name",
    ),
    (
        "src/main.rs",
        "Some(\"read\")",
        "nothing dispatches `atlas read`, so the rule that what Atlas reads \
         may never instruct it has no way to be exercised",
    ),
    (
        "src/live.rs",
        "only_if_it_closes_here",
        "a break that has not happened yet stops being flagged as conditional, \
         and a live reading starts disagreeing with the backtest of the same \
         moment",
    ),
    (
        "src/words.rs",
        "looks_like_odds",
        "the reader's scores get softmaxed a second time, which leaves every \
         word right and every confidence beside it wrong -- and because \
         confidence is what filters, it shows up as Atlas being unable to read \
         the screen at all",
    ),
    (
        "src/words.rs",
        "fn whole_picture",
        "a truncated grab is read as a whole one, and Atlas quotes the top of a \
         screen as the whole of it without ever saying a third went missing",
    ),
    (
        "src/words.rs",
        ".map(|s| Strip { strength: strength(map, width, &s), ..s })",
        "boxes get scored after they are grown rather than before, which \
         averages in the background they just swallowed and silently throws \
         away the smallest text first",
    ),
    (
        "src/main.rs",
        "Some(\"screen\")",
        "nothing dispatches `atlas screen`, so the in-house reader goes back to \
         being code that compiles and is never reached",
    ),
    (
        "src/market/claims.rs",
        "let v = match claim.kind {",
        "the referee stops matching the claim vocabulary exhaustively, so a kind \
         can be added without a checker -- a claim that compiles and is never \
         ruled on, in front of a live market",
    ),
    (
        "src/market/claims.rs",
        "pub fn cannot_say",
        "'the bars could not settle this' collapses into 'the bars say no', \
         which teaches Atlas that its readings fail in exactly the conditions \
         where they were never tested",
    ),
    // 28 Sep 2026: the two `src/ladder.rs` guards went with the module, which
    // left personal Atlas (tests/personal_atlas_is_its_own.rs).
    (
        "src/market/bars.rs",
        "Clamping would answer a different question quietly",
        "asking for a bar that has not happened starts clamping to the end \
         instead of refusing, which is how a replay driver's off-by-one becomes \
         a profitable-looking backtest",
    ),
    (
        "src/market/feed.rs",
        "pub fn no_future_bars",
        "a bar stamped in the future stops being refused at the door, and AsOf \
         cannot catch it -- it stops a reader looking past what it was given and \
         cannot make what it was given honest",
    ),
    (
        "src/fxday.rs",
        "offset_hours(Zone::NewYork",
        "the FX day boundary goes back to a fixed UTC hour, which puts every \
         prior-day high and low an hour out for half the year -- and does it \
         silently, because the levels stay plausible and stay near price",
    ),
    (
        "src/fxday.rs",
        "(all.len() >= 2).then",
        "the day still in progress starts being handed out as the prior day, \
         and a high that can still move gets treated as a level",
    ),
    (
        "src/levels.rs",
        "crate::fxday::levels(view)",
        "yesterday's high and low stop being candidates, and the two \
         most-watched lines on an FX chart after the figure go missing",
    ),
    (
        "src/stale.rs",
        "return Ok(Going::Stopped);",
        "a bar that spans the stop and the target starts being read as the \
         target, which is how a backtest invents money -- nothing in a bar \
         says which side it touched first",
    ),
    (
        "src/stale.rs",
        "flatters \\",
        "a view that starts mid-trade stops being refused, and the best of the \
         last few bars gets reported as the best of the trade, which flatters \
         every stale position there is",
    ),
    (
        "src/refusals.rs",
        "if total < ENOUGH_TO_LOOK",
        "the shape of three refusals starts being read as a pattern, which is \
         how a reader talks itself into moving a limit on noise",
    ),
    (
        "src/main.rs",
        "turned_down.declined(&pair, no.label(), no.plain()",
        "what Atlas turned down stops being kept, and 'it hasn't traded this \
         week' goes back to having two readings -- careful, or broken -- with \
         no way to tell them apart",
    ),
    (
        "src/rollover.rs",
        "3 => Some(Rollover { at, triple: true })",
        "Wednesday's rollover goes back to charging one night instead of \
         three, and a swing trade opened on a Wednesday silently costs three \
         times what the arithmetic says",
    ),
    (
        "src/levels.rs",
        "crate::rollover::thin(now)",
        "trades get proposed into the 17:00 New York turn, where every desk \
         squares at once and the fill comes back several pips wide -- a losing \
         trade with a perfectly good reading behind it",
    ),
    (
        "src/asia.rs",
        "if night.still_forming",
        "a range whose high and low can both still move starts being handed \
         out as a level, which is the same mistake as reporting today as the \
         prior day",
    ),
    (
        "src/asia.rs",
        "all.pop();",
        "the day in progress rejoins the sample the overnight range is \
         measured against -- and because the short-day filter can remove it \
         first, popping after the filter takes a completed day instead, \
         silently and only on the series where today is short",
    ),
    (
        "src/levels.rs",
        "crate::asia::levels(view)",
        "the overnight high and low stop being candidates, and the two lines \
         London opens into go missing from the target list",
    ),
    (
        "src/standdown.rs",
        "Blackout::InsideTheBar",
        "the news check goes back to asking an interval question about an \
         instant, which calls the H4 bar containing payrolls clean -- the least \
         clean bar of the week, waved through in the reassuring direction",
    ),
    (
        "src/standdown.rs",
        "no_clock_is_clear: false",
        "bars with no timestamps start being treated as safe, so a reader that \
         cannot tell whether it is standing in front of a release decides it \
         isn't",
    ),
    (
        "src/levels.rs",
        "crate::standdown::standing_down(",
        "the calendar goes back to being correct, complete and never asked -- \
         which is indistinguishable, from the results, from not having one",
    ),
    (
        "src/levels.rs",
        "candidates.retain(|(at, _)| (at - price).abs() <= reach)",
        "the round-number grid can offer a target further off than the market \
         has moved in the whole window, and it wins on reward every time -- a \
         fifty-pip target off a four-pip range at seven times the risk",
    ),
    (
        "src/levels.rs",
        "NoTrade::StandingDown",
        "refusing to act and failing to read collapse into one answer, and the \
         record can no longer tell a working rule from a gap in the analysis",
    ),
    (
        "src/main.rs",
        "atlas::standdown::spoken(&view",
        "`atlas market` stops saying there's a release in front of it unless a \
         direction was asked for, which is the one line worth reading on the \
         twelve minutes a month it is true",
    ),
    (
        "src/together.rs",
        "unknown.push(p.pair.clone())",
        "a pair Atlas doesn't recognise contributes nothing to the exposure \
         totals and makes the account look safer than it is",
    ),
    (
        "src/main.rs",
        "inbox.took_in(r, 500)",
        "nothing Atlas reads is kept, so 'what have you been fed' has no answer \
         and the rule holds where nobody can see it holding",
    ),
    (
        "src/live.rs",
        "self.forming = None;",
        "a bar that closed leaves the candle that was drawing it behind, and the \
         same price action is counted twice",
    ),
    (
        "src/firewall.rs",
        "never what was in it",
        "the held list starts keeping the contents of what it blocked, which \
         carries that content across the boundary into the one file nobody \
         thinks of as sensitive",
    ),
    (
        "src/main.rs",
        "Some(\"shared\")",
        "nothing dispatches `atlas shared`, so the boundary cannot be asked \
         what it would do until the day it matters",
    ),
    // The four below came from the improvements chat in the second 17 Sep
    // merge. They cover `handover` and the rewritten `vault` -- modules this
    // side did not have when the rest of this list was written, so nothing
    // here was pinning them. All four needles were confirmed present in this
    // tree before the entries were added; an entry for a line that is not
    // there is a guard that fails for its own reasons.
    (
        "src/main.rs",
        "atlas::handover::refuses(kind)",
        "a handed-over Atlas stops refusing anything, so the friend holding your \
         laptop can post as you, read your mail and unlock your vault",
    ),
    (
        "src/vault.rs",
        "pub fn proved_it",
        "openness goes back to being treated as proof, and on a vault with no \
         passphrase set the first unlock -- by anyone, with any twelve \
         characters -- takes the machine back",
    ),
    (
        "src/daemon.rs",
        "crate::vault::Vault::load(&crate::roots::install_state())",
        "the vault stops being read from disk and every run starts with an empty \
         one, so nothing is remembered and the passphrase is whatever is typed \
         first that run",
    ),
    (
        "tests/handed_over.rs",
        "fn the_one_gate_every_action_passes_through_asks_about_the_handover",
        "the gate wiring stops being checked at all, and the handover module \
         becomes a complete, tested thing that nothing consults",
    ),
    (
        "tests/commands_are_distinct.rs",
        "fn no_two_commands_claim_the_same_phrase",
        "two commands can share a phrase again, and one of the pair silently \
         never fires while still appearing in the config and the docs",
    ),

];

#[test]
fn every_guard_is_still_present() {
    let mut missing = Vec::new();
    for (path, needle, consequence) in GUARDS {
        // Through `read_source_path`, so a row naming `src/daemon.rs` still
        // finds its needle once daemon.rs is split into `src/daemon/*.rs`.
        match shared_rule::read_source_path(path) {
            None => missing.push(format!("{path} is gone entirely — {consequence}")),
            Some(text) => {
                if !text.contains(needle) {
                    missing.push(format!("{path} no longer contains `{needle}` — {consequence}"));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "a guard has gone missing, most likely in a merge:\n  {}\n\n\
         Restore it, or delete its entry from GUARDS deliberately.",
        missing.join("\n  ")
    );
}

#[test]
fn the_manifest_covers_every_guard_file() {
    // A guard file with no entry here is a guard that can vanish quietly.
    let guarded: std::collections::BTreeSet<&str> = GUARDS.iter().map(|(f, _, _)| *f).collect();
    let expected = [
        "tests/wiring.rs",
        "tests/guards.rs",
        "src/selfgrant.rs",
        "src/policy.rs",
        "src/finance.rs",
        "src/enrol.rs",
        "src/main.rs",
        "src/audio.rs",
        "src/brain.rs",
        "src/brief.rs",
        "src/checks.rs",
        "src/contents.rs",
        "src/council.rs",
        "src/daemon.rs",
        "src/dash.rs",
        "src/doctor.rs",
        "src/health.rs",
        "src/hollow.rs",
        "src/hub.rs",
        "src/integrations.rs",
        "src/kin.rs",
        "src/nudge.rs",
        "src/recall.rs",
        "src/retention.rs",
        "src/revise.rs",
        "src/safety.rs",
        "src/server.rs",
        "src/signals.rs",
        "src/trace.rs",
        "src/tts.rs",
        "src/vault.rs",
        "src/voice.rs",
        "tests/declared.rs",
        "src/accounts.rs",
        "src/earned.rs",
        "src/hollowcode.rs",
        "src/hublive.rs",
        "src/notify.rs",
        "src/palette.rs",
        "src/persona.rs",
        "src/phone.rs",
        "src/settings.rs",
        "src/tray.rs",
        "src/viewing.rs",
        "src/window.rs",
        "tests/no_quiet_nothings.rs",
        "tests/tray.rs",
        "src/gaze.rs",
        "src/handshape.rs",
        "src/handtrack.rs",
        "src/handloop.rs",
        "src/infer.rs",
        "src/frames.rs",
        "src/platform/mod.rs",
        "src/reclaim.rs",
        "src/speaker.rs",
        "src/system.rs",
    ];
    let unguarded: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|f| !guarded.contains(f))
        .collect();
    assert!(
        unguarded.is_empty(),
        "guard files with no manifest entry, so they can vanish quietly: {unguarded:?}"
    );
}

#[test]
fn every_entry_says_what_breaks() {
    // A manifest that only lists filenames tells whoever hits this failure
    // nothing about whether to restore or remove the entry.
    for (path, needle, consequence) in GUARDS {
        assert!(
            consequence.len() > 20,
            "{path}/{needle} has no useful consequence written down"
        );
        assert!(
            !consequence.ends_with('.'),
            "{path}/{needle}: keep consequences as clauses, they read into the message"
        );
    }
}

#[test]
fn no_guard_is_listed_twice() {
    // This shipped. A scripted edit matched its anchor four times and pasted
    // the same nine spec-seam guards in four times over; a second one doubled
    // five more. Every one of them passed, because a guard listed twice checks
    // the same string twice and agrees with itself.
    //
    // What it costs is not correctness but the count: the manifest is what
    // says how much of this codebase is actually held down, and 218 entries
    // guarding 213 things overstates it by exactly the amount nobody notices.
    // A guard file is a place where quiet duplication is *invisible*, which
    // is the same property that makes it worth guarding.
    let mut seen: Vec<(&str, &str)> = Vec::new();
    let mut twice: Vec<(&str, &str)> = Vec::new();
    for (path, needle, _) in GUARDS {
        if seen.contains(&(path, needle)) {
            twice.push((path, needle));
        } else {
            seen.push((path, needle));
        }
    }
    assert!(
        twice.is_empty(),
        "these guards are listed more than once, which inflates the count without \
         guarding anything more: {twice:?}"
    );
}

// ---------------------------------------------------------------------------
// The index and the rule it replaces must agree
// ---------------------------------------------------------------------------

#[path = "common/mod.rs"]
mod shared_rule;

/// **The only thing that makes `common::called_names` safe to use.**
///
/// Three guards were rewritten on 17 Sep to ask a prepared set instead of
/// rescanning the tree per candidate, taking 131 seconds off every
/// verification run. That is worth having and it is worth nothing if the fast
/// answer differs from the slow one, because the difference would show up as
/// a ceiling moving — and this tree has a documented history of ratchets
/// moving for reasons that had nothing to do with the code.
///
/// So: over every source file and every name defined anywhere in the tree,
/// the two must return the same answer. Not a sample. Every pair.
#[test]
fn the_index_agrees_with_the_rule_it_replaces() {
    use std::collections::BTreeSet;

    let mut files: Vec<String> = Vec::new();
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                if let Ok(t) = std::fs::read_to_string(&p) {
                    out.push(t);
                }
            }
        }
    }
    walk(std::path::Path::new("src"), &mut files);
    assert!(files.len() > 100, "expected the whole src tree, got {}", files.len());

    // Every name worth asking about: every function this tree defines.
    let mut names: BTreeSet<String> = BTreeSet::new();
    for t in &files {
        for line in t.lines() {
            let trimmed = line.trim_start();
            for prefix in ["pub fn ", "fn "] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let n: String =
                        rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    if !n.is_empty() {
                        names.insert(n);
                    }
                    break;
                }
            }
        }
    }
    assert!(names.len() > 500, "expected a lot of names, got {}", names.len());

    // Only names that occur as a SUBSTRING of this file can possibly be
    // called in it -- both the rule and the index need the characters to be
    // present. Skipping the rest is not sampling: it removes pairs where both
    // answers are false by construction. Without the prune this test took
    // 136 seconds, which is more than the 131 it exists to save, and a proof
    // that costs more than the thing it protects does not get run either.
    let mut disagreements = Vec::new();
    let mut compared = 0usize;
    for (i, text) in files.iter().enumerate() {
        let indexed = shared_rule::called_names(text);
        for name in &names {
            if !text.contains(name.as_str()) {
                continue;
            }
            compared += 1;
            let slow = shared_rule::calls(text, name);
            let fast = indexed.contains(name.as_str());
            if slow != fast && disagreements.len() < 10 {
                disagreements.push(format!(
                    "file {i}, name {name:?}: rule says {slow}, index says {fast}"
                ));
            }
        }
    }

    assert!(
        disagreements.is_empty(),
        "the index and the rule disagree on {} pair(s) out of {compared}:\n  {}",
        disagreements.len(),
        disagreements.join("\n  ")
    );
    // A prune that pruned everything would pass while proving nothing.
    assert!(
        compared > 40_000,
        "only {compared} pairs compared -- the prune is too aggressive. It was 42,751 when \
         measured on 17 Sep 2026; a large drop means the prune is now hiding pairs rather than \
         skipping ones that are false by construction."
    );
}
