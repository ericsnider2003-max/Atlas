//! **Settings that can be changed and change nothing.**
//!
//! The dead-capability guards ask whether *code* is reachable. This one asks
//! the same question of *configuration*, and the answer turned out to be
//! worse, because a dead config key is a lie told to the person holding the
//! file. A dead function sits there quietly. A setting named
//! `may_keep_screen_on` that nothing reads actively tells its reader that
//! Atlas has a behaviour it does not have — and it will be believed, because
//! why else would it be in the file.
//!
//! Counted on 16 Sep 2026, over every `pub` field of every `*Config` struct:
//!
//! ```text
//! config fields declared                693
//! read somewhere                        574
//! never read                            119
//!   of which, in an UNWIRED module        9   expected; nothing runs
//!   of which, in a WIRED module         110   a setting that does nothing
//! ```
//!
//! Plus **18 keys sitting in the shipped `config/*.yaml` files that no code
//! anywhere names at all** — not a struct field that goes unread, but a line
//! in a config file that no struct even has a slot for.
//!
//! `voice` is the worst single module at 20. That is the module that owns the
//! transcription toolchain, so its settings are exactly the ones someone would
//! reach for when dictation behaves oddly.
//!
//! ## What these lists are not
//!
//! Not all bugs. Several are honest scaffolding for a capability that is built
//! and waiting on a ruling or a device, and a config slot arriving before its
//! reader is a reasonable order to build in. The defect is the silence: until
//! now nothing recorded which was which, so every one of the 119 looked
//! identical to a reader — and to the next session.
//!
//! ## What to do when this fails
//!
//! **A key appeared:** either read it where it was meant to be read, or add it
//! here with the reason it cannot be read yet. **A key disappeared:** it got a
//! reader. Delete the line; that is the direction this list should move.

use std::collections::{BTreeSet, HashSet};

/// Config fields in WIRED modules that nothing reads.
///
/// The ones that matter. Each is a setting a person can change today, in a
/// module that is running, with no effect whatsoever.
const DEAD_IN_WIRED: &[&str] = &[
    // --- 19 Sep 2026 ------------------------------------------------------
    //
    // Twenty-eight `#[serde(skip)]` fields came off this list together. They
    // were never settings: no config file can set one, so "a person can
    // change this today and nothing happens" was false about every one of
    // them, and counting them here made the number mean two things at once.
    //
    // Fifteen were invariants the code already enforces -- a type that cannot
    // express the forbidden thing, or an explicit refusal -- and those fields
    // are deleted, with a test at the place that does the enforcing. A bool
    // nobody reads is not a boundary.
    //
    // Thirteen are promises about capabilities this tree has not built, and
    // they are in PROMISES_ABOUT_WHAT_IS_NOT_BUILT below with what is
    // missing beside each. `a_pinned_invariant_is_not_counted_as_a_setting`
    // keeps the two apart from here on.
    // --- 18 Sep 2026, second sitting ---------------------------------------
    //
    // Twelve came off together, all of them settings a person could change
    // today that did nothing. Four were worse than inert -- the shipped
    // default said one thing and the code did another:
    //
    //   opsec::always_strip_metadata   shipped true; `publishing::export_args`
    //                                  emitted no `-map_metadata`, so an
    //                                  exported clip carried the original's
    //                                  GPS and camera serial while
    //                                  `Risk::Metadata` told you stripping
    //                                  happened by default.
    //   capture::never_ask_on_capture  shipped true ("ask nothing at capture
    //                                  time, this is the whole point") and
    //                                  the capture path asked anyway.
    //   retention::logs_mb             shipped 4; the log rotated at 2.
    //   viewing::longest_minutes       shipped 90; a three-hour file was
    //                                  scanned and OCR'd in full.
    //
    // The rest were the familiar shape: a literal beside the config that
    // happened to equal it (`compact_approvals(200)`, `session_turns` as
    // `max_turns: 40`, `>= 8` in `content::learn`, `>= 5` in
    // `Carried::worth_mentioning`), or a reader that took the whole set when
    // the config named a subset (`reference::for_trading()`).
    //
    // presence::discreet_with_strangers is the one to read the reasoning on:
    // `notify::route` had already ruled that discretion is not a routing
    // decision, so it is wired where that ruling put it -- `Note::shown`, the
    // knock -- rather than as a second mechanism beside it. `reach_you` also
    // ignored the camera entirely and guessed presence from idle time.
    // --- 18 Sep -------------------------------------------------------------
    //
    // overnight::attempts_each -- attempts on a single problem before moving
    // on. The wired brain (`ask_you_later`) writes a problem up rather than
    // attempting it, so there is nothing to count. It is read the day a
    // solving brain is wired, and not before.
    //
    // voice::ToolsConfig.mesh came off 19 Sep 2026. Its only reader had been a
    // `mesh::choose` call in `Intent::Sync` that passed four hardcoded
    // literals and discarded the answer; wiring sync to a real carrier
    // removed it, and the section went unread. `atlas mesh` reads it now --
    // not to route anything, which is still not built, but to say what a
    // private network would give you, which of the four kinds you chose, what
    // it costs, and what you would have to do yourself. `mesh.kind` was a
    // string nothing parsed, so `tailscale` and `banana` were the same
    // setting. `mesh.prefer_direct` went the other way, to
    // PROMISES_ABOUT_WHAT_IS_NOT_BUILT: its only reader is `choose`, and
    // `choose` picks between three routes that do not exist and one that does.

    // clipboard::ClipboardConfig.reply_to_clipboard came off on 20 Sep 2026
    // when the write-back was wired: a clipboard request is now answered and,
    // when this is on, the answer is set on `Daemon::clipboard_writeback` for
    // the platform layer to put back on the clipboard -- the mirror of how
    // `clipboard_text` is set on the way in. It had been the classic shape
    // here: a setting describing something nothing did.
    // cloudsync::CloudConfig.check_every_hours came off on 23 Sep 2026: the
    // tick now runs `cloudsync::still_syncing` on the sync folder at this
    // cadence — folder still there, still writable, and something new from
    // your other devices within twice the interval.
    // cloudsync::CloudConfig.encrypt_before_writing came off on 18 Sep 2026
    // by being deleted. It was `#[serde(skip)]`, pinned true, read by
    // nothing, and it stated a guarantee the tree did not keep: bundles were
    // written with `serde_json::to_string_pretty` into a folder a cloud
    // provider copies to their servers, while `cloudsync::WHY_ONEDRIVE` said
    // "I encrypt before anything is written either way". Sealing is built
    // now -- `sync.encrypt_bundles`, off by default, `sync::seal` -- so the
    // switch lives where the writing happens and this field would have been
    // a second answer to the same question.
    // codes::CodesConfig.check_days_before and
    // goingaway::AwayConfig.remind_days_before came off 19 Sep 2026, and they
    // were the same omission twice. Both are "how many days before a trip",
    // and there was no way to tell Atlas about a trip -- no date anywhere in
    // the tree, so both were thresholds on a number nothing computed.
    // `goingaway::Away` is that date, and `atlas away on <date>` is how it
    // gets there.
    // queue_while_offline classified 22 Sep 2026 (b): its reader would be the
    // phone-side capture path, and `Phone::capture` has no production caller --
    // there is no phone client and no sync that writes the mirror, which
    // `companion::unbuilt` states in as many words. Read when that end exists.
    "companion::CompanionConfig.queue_while_offline",
    // reply_timeout_secs classified 22 Sep 2026 (c, UNSURE -- left unwired):
    // its siblings settle_ms/max_exchanges/opening are read inside consult.rs
    // (settled/next/open), but nothing times out a wait -- the StillComing ->
    // Move::Wait path takes no elapsed time. A reader means a wait-loop driver
    // that threads elapsed seconds into `next`, which would change behaviour,
    // so this is reported rather than edited.
    "consult::ConsultConfig.reply_timeout_secs",
    // effects_per_minute / eye_jump_limit classified 22 Sep 2026 (b): their
    // readers-in-waiting are `too_many_effects` (`per_minute > 6.0`) and
    // `Cut::seamless` (`jump() < 0.2`), which carry these exact literals but
    // have no production caller -- only the brand-reply path (`what_they_asked`
    // / `reply_to`) runs. Wiring here would be a reader nothing reaches.
    "editcraft::EditCraftConfig.effects_per_minute",
    // files::FilesConfig.look_inside_archives came off 19 Sep 2026 by being
    // deleted. It shipped **true** for a behaviour nothing here performs:
    // `index::AssetClass` classifies a .zip and stops, and `safe_to_unpack`
    // -- the guard for the unpacker -- has no caller. A switch shipped on,
    // for a thing nobody does, reads as a thing you have, and believing Atlas
    // has looked inside your archives is what stops you checking. Same shape
    // as `cloudsync.encrypt_before_writing` the day before.
    // finance::FinanceConfig.category_jump came off 19 Sep 2026, and wiring
    // it meant writing the check it names. It says "flag a category whose
    // spend rises more than this fraction month on month" and nothing did
    // that: `finance::review` checks large transactions, duplicates and
    // repeat billing and never compares two months, while `money::new_or_grown`
    // compares months one standing charge at a time -- with 15% hardcoded,
    // against the 50% in the shipped file. A threshold for a check nobody
    // wrote is the worst kind of dead setting: the file reads as though the
    // check exists. `money::buckets_that_jumped` is that check.
    // crush_limit / skin_tolerance_degrees classified 22 Sep 2026 (b):
    // `grading::check` carries these literals (`crushed_black > 0.02`,
    // `skin_off_line.abs() > 6.0`) but has no production caller -- the daemon's
    // grading branch says there is no grade-tree reader, so nothing builds a
    // `Measured`. Wired when something produces grade notes.
    "grading::GradingConfig.crush_limit",
    "grading::GradingConfig.skin_tolerance_degrees",
    // hearing::HearingConfig.switch_margin came off 21 Sep 2026. Its own doc
    // said "a new ear must beat the current one by this much to be worth
    // switching", and rule 4 of the module -- don't flap -- names two guards
    // for it: "clearly better" and "has to last". Only the second was wired;
    // `settle_secs` made a change wait, but `ideal` picked the single
    // highest-scoring desk mic with no reference to the one in use, so two
    // webcams that hear you equally well traded the ear on every recalibration
    // wobble. `decide` now gates a desk-to-desk switch on the margin: a
    // different desk mic must beat the current one's score by `switch_margin`
    // before it even becomes the ear Atlas wants.
    // The three `household::HouseholdConfig` fields came off 19 Sep 2026, and
    // only one of them by being wired.
    //
    // `device_name` is read now: the hub's join form is pre-filled with it
    // and falls back to it when the box is left empty, so a person who named
    // this machine in their config is not asked to name it again on a phone
    // keyboard.
    //
    // `household` and `discoverable` were deleted. The household's id lives
    // in the store, made once at first run; a copy of it in a text file is a
    // thing people move between machines, which is how two devices claim the
    // same household without either having been invited. And nothing here
    // announces itself on a local network -- `mesh` is unwired and says so --
    // so a switch for it read as a behaviour you had turned off. Both are in
    // `config::NO_FIELD_TO_LAND_IN`, which is how a file still setting either
    // gets told rather than ignored.
    // The four `install::InstallConfig` fields came off 19 Sep 2026.
    // `InstallConfig` had no field anywhere in `ToolsConfig` and no block in
    // `tools.yaml`, so none of them could be set at all.
    //
    // `tools_dir` and `models_dir` are the ones that cost something:
    // `atlas install` stats every piece under the install folder, so on the
    // machine those settings exist for -- the one where the models live on
    // another drive -- it reported every piece as missing. The feature looked
    // broken rather than unconfigured.
    //
    // `include_optional` turned up a second defect while being wired: the
    // list of pieces and the "MB to fetch" under it were counting different
    // sets, so with the optional ones left out it printed "4801MB to fetch"
    // beside a 341MB download. `what_to_fetch` is the fix.
    //
    // `carry_on_after_failure` is in PROMISES_ABOUT_WHAT_IS_NOT_BUILT: it is
    // how a downloader should behave, and nothing here downloads anything.
    // Both messaging entries came off 19 Sep 2026, and they went opposite
    // ways, which is the useful part.
    //
    // `your_names` is what `messaging::sort` compares a group message against
    // to decide whether it is addressed to you. It was pinned to
    // `#[serde(skip)]` that morning, because there was no messaging reader
    // and a list of the names you go by, in a tree that never sees a message
    // you were named in, has no effect but to make you believe something is
    // watching.
    //
    // It came back the same afternoon, and that is the shape worth noticing.
    // `telegram.rs` reads a bot's messages, so `sort` runs on something
    // somebody sent and the list has something to be found in. **A promise
    // about what is not built is meant to end this way** -- the missing
    // capability arrives and the pinned field becomes a setting again. A
    // `PROMISES` list that only ever grows is a list of things nobody
    // intends to build.
    //
    // `platforms` stayed settable and got a real reader. Atlas cannot count
    // an inbox it has not read, but it can say which of the platforms you
    // named could ever work, what each needs from you, and which are closed
    // whatever you do -- and two of the six are closed. That is
    // `messaging::what_you_asked_for`, reached from `Intent::Mail` when you
    // ask about messages. The difference between the two is whether an
    // honest answer needs a message to exist.
    // money::MoneyConfig.work_words came off 19 Sep 2026. It decides which
    // lines are the business rather than the household, and nothing sorted
    // anything: `finance::parse_csv` had no caller, `money::sort_one` had no
    // caller, and the daemon's money branch summarised an empty slice.
    "overnight::OvernightConfig.attempts_each",
    // panel::PanelConfig.waking_secs came off on 23 Sep 2026: the tick clears
    // a transient panel once it has been up this long (`panel::faded`), which
    // also gives `Panel::transient()` its first reader.
    // The three `person::PersonConfig` thresholds came off 19 Sep 2026. They
    // were thresholds on something nobody produced: `Noticed` has four
    // variants, each with a sentence and a rule about repeating, and nothing
    // in the tree ever built one. `person::noticing` is the producer it never
    // had, `Person::working_now` keeps the count of late nights that
    // `late_nights_before_saying` is a threshold on, and `Said` is what makes
    // `quiet_days` mean something -- it needs a memory of what was said.
    // portable::PortableConfig.warn_up_front came off 19 Sep 2026, and it was
    // a worse case than a field nothing read: `PortableConfig` had no field
    // anywhere in `ToolsConfig`, so there was no `portable:` block a person
    // could write and nothing that would have read one. The catalogue is what
    // made it answerable -- a wall is only worth warning about if you can say
    // which of the things Atlas does hits it. `atlas doctor` says so now, and
    // says nothing at all on a machine with no walls.
    // reference::ReferenceConfig.warn_when_stale came off 19 Sep 2026, and
    // wiring it turned up a worse thing than the dead setting. `nothing_found`
    // is the sentence Atlas says instead of inventing a number, and it
    // answered "my futures contract specs covers tick size, tick value..."
    // about a shelf with `as_of` empty -- nothing in this tree fetches
    // reference material, so it holds none of them. A grounded answer that is
    // not grounded is worse than an ungrounded one, because you stop checking
    // it. `Shelf::held` is the distinction; the setting decides whether the
    // age of a shelf that *is* held gets said out loud.
    // remote::RemoteConfig.confirm_side_effects came off 19 Sep 2026. It
    // ships **on** -- "ask before running something that changes things" --
    // and nothing read it, so `atlas remote start 3` marked a request running
    // with no look at what the request was. On-by-default and connected to
    // nothing is the worst of the three states: the file says you are
    // protected and you are not.
    // selfaudit::SelfAuditConfig.every_days and .act_without_asking came off
    // 19 Sep 2026. Both were inert for one reason: `recommend` was only ever
    // reached from `Intent::WorkOnYourself`, which runs because you asked, so
    // "how often to look" described a looking nothing did and "act on the
    // best one without asking" described an asking that was the only way in.
    // The tick now looks on the cadence, keeping the clock in the store
    // rather than in memory -- a cadence of days in a process restarted daily
    // would have been the same setting doing nothing, one layer down.
    // signin::SignInConfig.fill_without_asking came off 19 Sep 2026, and the
    // hole under it was bigger than the field. `Access::may_fill` -- the
    // lookalike-domain check this whole module exists for -- had no caller
    // anywhere in the running program, so `Intent::SignIn` announced "signing
    // you into X" having checked only that the feature was on. `may_start` is
    // the half of that decision that does not need a browser page, and the
    // spoken sign-in goes through it; `fill_without_asking` decides whether
    // that is an announcement or a question.
    // vault::VaultConfig.kdf_rounds came off 19 Sep 2026 by being deleted.
    // The vault moved to Argon2id, whose cost is memory and passes rather
    // than a round count, and the field has been ignored since -- while
    // `tools.yaml` shipped it with a comment promising what the number bought
    // you. `config::NO_FIELD_TO_LAND_IN` carries it as a nested key now, so
    // `atlas doctor` names it to anyone whose own file still sets it.
    // voice::ToolsConfig.after_me came off 19 Sep 2026. `afterme.rs` was a
    // complete, tested module nothing could reach, because `gaps` wants a
    // place, people told and something counting the days and nothing held
    // any of those. `atlas afterme` keeps the arrangement now, and the
    // daemon reads `review_every_days` to nudge you to check it still holds
    // -- which is the only part of an arrangement like this that belongs on
    // a clock.
    // voice::ToolsConfig.android and .ios stay on this list, and the reason
    // is not an oversight: **there is no Android or iOS build**. The blocks
    // came out of `config/tools.yaml` on 19 Sep 2026 -- every line in them,
    // `enabled` and `model` included, was a switch for an app that does not
    // exist -- and the decisions they recorded moved into `src/android.rs`
    // and `src/ios.rs` as `#[serde(skip)]` specifications, which
    // `PROMISES_ABOUT_WHAT_IS_NOT_BUILT` below now carries with what is
    // missing beside each.
    //
    // These two are what is left: the sections themselves. They get a reader
    // on the day there is a phone build and not before, and `atlas catalog
    // --platform ios` is where to see how far off that is.
    "voice::ToolsConfig.android",
    // booking came off 21 Sep 2026: the Intent::Booking handler reads
    // `self.tools_cfg().booking` — its `enabled` gate, `hours`, and
    // `notice_hours` all judge a proposed time now.
    // voice::ToolsConfig.consult and .strategy came off 26 Sep 2026 with 25j.
    // voice::ToolsConfig.household came off with them: the section is read
    // now, for `device_name`.
    // voice::ToolsConfig.hub came off 18 Sep 2026. The dashboard pages are
    // served from main.rs and nothing consulted `hub.enabled`, so there was
    // no way to turn the hub off from config. Same shape as server.enabled.
    "voice::ToolsConfig.ios",
    // voice::ToolsConfig.retention and .together (and a third section since
    // removed from personal Atlas) came off 18 Sep 2026. All three were the same shape: code built `XConfig::default()`
    // at the call site while the user's section sat unread. `retention` was
    // the one that mattered -- it is the pass that deletes files to stay in
    // budget, and it was ignoring the budget.
    "voice::ToolsConfig.layout_prefs",
    // `voice::ToolsConfig.lifecycle` was here until 17 Sep, and it was the
    // best example on the list of what this guard is for: the whole
    // `LifecycleConfig` block was being parsed out of `tools.yaml` and thrown
    // away, because the daemon built `Supervisor::default()` rather than
    // `Supervisor::new(cfg)`. So a person could set a memory budget and an
    // idle timeout and change nothing at all. The merge wired it; see
    // `tests/helpers_are_governed.rs`, which pins the wiring rather than the
    // algorithm for exactly that reason.

    // voice::ToolsConfig.sync came off 18 Sep 2026 -- `Intent::Sync` reads it
    // to find the folder to carry through.
    // voice::ToolsConfig.trading and TradingConfig.levels
    // came off 18 Sep 2026. Same shape as retention/together: `atlas
    // market` built `Rules::default()` at the call site, and `atlas market` carried a
    // third copy as bare literals -- `structure::recent(&view, 120, 2)` and
    // `levels(&view, 2, 50.0)` are `structure_bars`, `pivot_reach` and
    // `level_span_pips` written out again. `main::trading_cfg` reads the
    // section now and every one of those sites takes its number from it.
    // workingset::WorkingSetConfig.always_carry_current classified 22 Sep 2026
    // (b): it packs the current task "whenever devices meet", and no rendezvous
    // event exists -- no phone client, no meeting detection. The only packing
    // path is manual (`atlas carry pack`). Read when devices can meet.
    "workingset::WorkingSetConfig.always_carry_current",
    // The three `workspace_view::WorkspaceConfig` fields came off 19 Sep
    // 2026. `WorkspaceConfig` had no field anywhere in `ToolsConfig` and no
    // block in `tools.yaml`, so none of them could be set at all -- and
    // `workspace_page_live` took `views.first()`, which happens to be "Now",
    // which is also what `default_view` ships as. The hardcoded behaviour and
    // the shipped default agreed, so nothing looked wrong until somebody
    // changed the setting. That is the worst version of this defect: it is
    // invisible to everyone except the one person who tried to use it.
];

/// Config fields belonging to modules that are themselves unwired.
///
/// Expected rather than alarming -- the module does not run, so its settings
/// are not read. Listed so that wiring the module surfaces them.
const DEAD_IN_UNWIRED: &[&str] = &[
    // overnight's settings left this list on 18 Sep 2026 when the module was
    // wired to the tick. `apply_while_asleep` is now read as a refusal guard
    // in `run_the_night`; `attempts_each` is in DEAD_IN_WIRED with its reason.
];

/// Keys present in `config/*.yaml` that no source file names at all.
///
/// Worse than an unread struct field: there is no slot for these. They are
/// text in a shipped config file that nothing has ever looked for.
const YAML_KEYS_WITH_NO_READER: &[&str] = &[
    // target_lufs, max_true_peak_db and min_posts_for_direction came off
    // 18 Sep 2026. All three were the same story: the value in the file
    // matched a constant in the module exactly -- `grade::TARGET_LUFS`,
    // `grade::MAX_TRUE_PEAK_DB`, `posts.len() < 6` -- so the advice was right
    // and the file had nothing to do with it. `GradeConfig` and `ReachConfig`
    // exist now and both sections parse into them.
    // Classified 22 Sep 2026, all (b): every key below belongs to a section
    // with no reader. `layouts.yaml:right_third` is a spare fractional rect
    // that no app selects with `layout:`/`standalone_layout:`. The rest sit
    // under sections `config.rs::NO_FIELD_TO_LAND_IN` already names: `fit`
    // (force_tier, replan_on_change -- no FitConfig), `improve`
    // (distil_from_hosted, learn_route_reliability, learn_vocabulary,
    // precompute_overnight -- no ImproveConfig, the loop is unbuilt), `knowhow`
    // (learn_new_snags), `wanted` (learn_by_topic), `chain`
    // (confirm_before_sending), and `cdp` (launch_args -- superseded by the
    // live `browser:` block). None can be wired without adding a config type,
    // which is not a small change and most await an unbuilt feature.
    "layouts.yaml:right_third",
    "tools.yaml:confirm_before_sending",
    "tools.yaml:distil_from_hosted",
    "tools.yaml:launch_args",
    "tools.yaml:learn_by_topic",
    "tools.yaml:learn_new_snags",
    "tools.yaml:learn_route_reliability",
    "tools.yaml:learn_vocabulary",
    "tools.yaml:precompute_overnight",
    "tools.yaml:replan_on_change",
];

/// Promises about behaviour that does not exist yet.
///
/// The 18 Sep sitting found that `DEAD_IN_WIRED` was counting two different
/// things as one. A setting that does nothing is a person changing a value
/// and getting no result. These are not that: they are `#[serde(skip)]`
/// fields, which **no config file can set at all**. Calling them settings and
/// putting them on that list said something false about them, in both
/// directions — the count was inflated by things nobody could change, and
/// each one looked like an oversight rather than a decision.
///
/// What they actually are: a guarantee written down in the struct, for a
/// capability the tree has not built. `booking.may_send_alone` is `false` and
/// nothing reads it because **nothing here can send anything**. The field is
/// the only place recording what the answer will be on the day a sender
/// exists.
///
/// That makes them worth keeping and worth naming honestly, which is what the
/// second column is for: the capability that is missing. When one of these is
/// built, its field gets a reader and this guard says so.
///
/// The other kind — an invariant the types or an explicit refusal already
/// enforce — was deleted rather than listed, with a test put at the place
/// that does the enforcing. A bool nobody reads is not a boundary.
const PROMISES_ABOUT_WHAT_IS_NOT_BUILT: &[(&str, &str)] = &[
    (
        "accounts::AccountsConfig.may_change_security",
            "nothing here writes to a site's settings. `Change` has four variants \
         and all four edit Atlas's own record of what your 2FA is.",
    ),
    (
        "calendar::CalendarConfig.sync_native",
            "the native phone calendar is read and written by the phone app \
         through EventKit (iOS) / CalendarProvider (Android), which isn't in \
         this tree -- same boundary as the Android client. This flag is what \
         that app checks before syncing; `calendar::merge_from_phone` and \
         `for_phone` are the in-tree seam it drives.",
    ),
    (
        "android::AndroidConfig.holds_credentials",
            "there is no Android client in this tree at all -- `android.rs` \
         describes abilities a phone app would have.",
    ),
    (
        "android::AndroidConfig.always_listening",
        "there is no Android build. `atlas catalog --platform android` says so, and \
         `atlas mobile android` says what one would be allowed to do and what the wake \
         word costs in battery. This records that it would ship off.",
    ),
    (
        "android::AndroidConfig.can_act_in_apps",
        "there is no Android build. The accessibility service is the mechanism, and \
         this records that it would be a thing you turn on knowing what it means.",
    ),
    (
        "android::AndroidConfig.read_notifications",
        "there is no Android build. Recorded off, because a notification reader is \
         the broadest permission on the phone.",
    ),
    (
        "android::AndroidConfig.replace_assistant",
        "there is no Android build. Recorded off: taking the home button is a thing \
         you choose, not a thing an install does to you.",
    ),
    (
        "mesh::MeshConfig.prefer_direct",
        "nothing in this tree reaches another device directly. Its only reader is \
         `choose`, which picks between SameNetwork, Mesh, Cable and Cloud -- and only \
         Cloud is built (a folder both machines can see). This records the decision: \
         given a direct route and a relay, take the direct one.",
    ),
    (
        "install::InstallConfig.carry_on_after_failure",
        "nothing here downloads anything. `plan`, `before` and `after` describe the \
         work and report on it; the fetching is the setup script's or yours. This \
         records that one flaky download should not cost you the other five.",
    ),
    (
        "ios::IosConfig.background_work",
        "there is no iOS build. `portable::how(Ios, Background)` is `Awkward` -- a few \
         minutes after you switch away, then suspended -- and this records the intent \
         to use what there is.",
    ),
    (
        "ios::IosConfig.ask_permissions_when_needed",
        "there is no iOS build. Recorded on: asking for the camera, photos, calendar \
         and contacts all at once at first launch is how you get told no to all of it.",
    ),
    (
        "booking::BookingConfig.may_book_alone",
            "nothing writes to a calendar. `assess` and `could_offer` have no \
         production caller.",
    ),
    (
        "booking::BookingConfig.may_send_alone",
            "nothing sends anything. The same absence as the line above.",
    ),
    (
        "companion::CompanionConfig.mirrors_secrets",
            "nothing writes the phone mirror -- `companion::unbuilt` says so in \
         as many words.",
    ),
    (
        "editcraft::EditCraftConfig.may_send_replies",
            "`reply_to` returns a string that `daemon` prints. There is no brand \
         reply path to unlock.",
    ),
    (
        "enrol::EnrolConfig.never_answer_human_checks",
            "`enrol::read` would consult it and has no production caller -- \
         nothing drives an `enrol::Run`.",
    ),
    (
        "ios::IosConfig.holds_credentials",
            "there is no iOS client in this tree, the same as android above.",
    ),
    (
        "messaging::MessagingConfig.chatter_interrupts",
            "`messaging::interrupts` is the rule and nothing calls it, because no \
         platform adapter produces a `Message`.",
    ),
    (
        "messaging::MessagingConfig.draft_only",
            "there is no send path to gate, and no message source to draft from.",
    ),
    (
        "money::MoneyConfig.gives_advice",
            "every function in `money` describes what already happened. There is \
         no recommendation generator to switch off.",
    ),
    (
        "recovery::RecoveryConfig.atlas_can_use_these",
            "nothing consumes a `recovery::Route`. The vault opens with a \
         passphrase or a recovery key, both typed by a person.",
    ),
];

/// Every `#[serde(skip)]` field, with whether anything reads it.
///
/// Read from the source rather than remembered, because the whole point of
/// separating these is that they are a different kind of thing and nobody
/// should have to keep the distinction in their head.
fn pinned_fields(src: &[(String, String)]) -> Vec<(String, bool)> {
    let read = fields_read_anywhere(src);
    // A module that does not run is already accounted for by
    // `DEAD_IN_UNWIRED`, and saying its fields are promises about something
    // unbuilt would be true of the whole module rather than of the field.
    let unwired = unwired_modules();
    let mut out = Vec::new();
    for (module, body) in src {
        if unwired.contains(module) {
            continue;
        }
        let lines: Vec<&str> = body.lines().collect();
        let mut current_struct = String::new();
        let mut skipped = false;
        for line in &lines {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("pub struct ") {
                current_struct =
                    rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                skipped = false;
                continue;
            }
            if t.starts_with("#[serde(skip") {
                skipped = true;
                continue;
            }
            if let Some(f) = t.strip_prefix("pub ") {
                let field: String =
                    f.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if !field.is_empty()
                    && f[field.len()..].trim_start().starts_with(':')
                    && current_struct.contains("Config")
                {
                    if skipped {
                        out.push((
                            format!("{module}::{current_struct}.{field}"),
                            read.contains(&field),
                        ));
                    }
                    skipped = false;
                }
                continue;
            }
            if !t.starts_with("///") && !t.starts_with("//") && !t.is_empty() {
                skipped = false;
            }
        }
    }
    out
}

#[test]
fn a_pinned_invariant_is_not_counted_as_a_setting() {
    // A `#[serde(skip)]` field cannot be set by anybody, so calling it a
    // setting that does nothing is false twice over. Each one is either an
    // invariant the code already enforces -- in which case delete it and put
    // a test where the enforcing happens -- or a promise about something not
    // built, in which case it goes on the list above with what is missing.
    let src = sources();
    let listed: BTreeSet<&str> =
        PROMISES_ABOUT_WHAT_IS_NOT_BUILT.iter().map(|(k, _)| *k).collect();
    let dead: BTreeSet<&str> = DEAD_IN_WIRED.iter().copied().collect();

    let mut unaccounted = Vec::new();
    let mut wrongly_counted = Vec::new();
    for (key, is_read) in pinned_fields(&src) {
        if is_read {
            // It has a reader now. That is the good direction, and it means
            // the capability arrived -- take it off the list.
            if listed.contains(key.as_str()) {
                wrongly_counted.push(format!("{key} (now read -- delete the line)"));
            }
            continue;
        }
        if dead.contains(key.as_str()) {
            wrongly_counted.push(format!("{key} (on DEAD_IN_WIRED, which is for settings)"));
        }
        if !listed.contains(key.as_str()) {
            unaccounted.push(key);
        }
    }

    assert!(
        wrongly_counted.is_empty(),
        "these are pinned invariants being counted as settings a person can change:\n  {}",
        wrongly_counted.join("\n  ")
    );
    assert!(
        unaccounted.is_empty(),
        "these fields are `#[serde(skip)]` and unread, so they are promises rather than \
         settings:\n  {}\n\nEither delete the field and put a test where the guarantee is \
         actually enforced, or add it to PROMISES_ABOUT_WHAT_IS_NOT_BUILT with the capability \
         that is missing.",
        unaccounted.join("\n  ")
    );
}

#[test]
fn every_promise_says_what_is_missing() {
    for (key, why) in PROMISES_ABOUT_WHAT_IS_NOT_BUILT {
        assert!(
            why.len() > 30,
            "{key} is listed with no account of what is not built: {why:?}"
        );
    }
    // And the scan that finds them still finds them.
    let found = pinned_fields(&sources());
    assert!(
        found.len() >= PROMISES_ABOUT_WHAT_IS_NOT_BUILT.len(),
        "the pinned-field scan found {} fields, which is fewer than the list it is meant to \
         police",
        found.len()
    );
}

fn sources() -> Vec<(String, String)> {
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
            if let Ok(t) = std::fs::read_to_string(&path) {
                out.push((stem, t));
            }
        }
    }
    let mut out = Vec::new();
    walk(std::path::Path::new("src"), &mut out);
    out
}

fn unwired_modules() -> BTreeSet<String> {
    std::fs::read_to_string("tests/wiring.rs")
        .expect("tests/wiring.rs")
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"').and_then(|r| r.split('"').next()).map(String::from))
        .collect()
}

/// Every `pub` field of every `*Config` struct, as `(module, struct, field)`.
fn config_fields(src: &[(String, String)]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (module, body) in src {
        let mut lines = body.lines().peekable();
        while let Some(line) = lines.next() {
            let t = line.trim_start();
            let Some(rest) = t.strip_prefix("pub struct ") else { continue };
            let name: String =
                rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if !name.contains("Config") {
                continue;
            }
            // The struct body, to its closing brace at the same indent.
            for inner in lines.by_ref() {
                if inner.starts_with('}') {
                    break;
                }
                let it = inner.trim_start();
                let Some(f) = it.strip_prefix("pub ") else { continue };
                let field: String =
                    f.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                if field.is_empty() || !f[field.len()..].trim_start().starts_with(':') {
                    continue;
                }
                out.push((module.clone(), name.clone(), field));
            }
        }
    }
    out
}

/// Every `.field` name read anywhere in the tree, computed once.
///
/// **This guard was mine and it was 15.6s**, for the same reason the three
/// older ones were 131s between them: `read_anywhere` walked the whole source
/// tree once per config field, 693 times over. Building the set once turns the
/// question into a hash lookup and the guard into 0.3s.
///
/// Deliberately generous, unchanged from the slow version: any `.field`
/// anywhere counts, including in tests and including a same-named field on an
/// unrelated struct. That biases hard toward calling a key ALIVE, so
/// everything the guard reports is unambiguous.
fn fields_read_anywhere(src: &[(String, String)]) -> HashSet<String> {
    let mut out = HashSet::new();
    for (_, body) in src {
        for line in body.lines() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            // A declaration is not a read.
            let is_decl = t.starts_with("pub ") && t.contains(':');
            let b = line.as_bytes();
            let mut i = 0;
            while let Some(rel) = line[i..].find('.') {
                let at = i + rel;
                let start = at + 1;
                let mut e = start;
                while e < b.len() {
                    let c = b[e];
                    if c.is_ascii_alphanumeric() || c == b'_' {
                        e += 1;
                    } else {
                        break;
                    }
                }
                if start < e {
                    let name = &line[start..e];
                    // Skip a numeric tuple index and a float's fraction.
                    if !name.chars().next().unwrap_or('0').is_ascii_digit()
                        && !(is_decl && t.contains(&format!("{name}:")))
                    {
                        out.insert(name.to_string());
                    }
                }
                i = at + 1;
            }
        }
    }
    out
}

fn current() -> (BTreeSet<String>, BTreeSet<String>) {
    let src = sources();
    let unwired = unwired_modules();
    let read = fields_read_anywhere(&src);
    let (mut wired_dead, mut unwired_dead) = (BTreeSet::new(), BTreeSet::new());
    for (module, st, field) in config_fields(&src) {
        if read.contains(&field) {
            continue;
        }
        let entry = format!("{module}::{st}.{field}");
        if unwired.contains(&module) {
            unwired_dead.insert(entry);
        } else {
            wired_dead.insert(entry);
        }
    }
    (wired_dead, unwired_dead)
}

#[test]
fn no_setting_in_a_running_module_quietly_stops_being_read() {
    let (found, _) = current();
    // Two lists, because there are two kinds of unread field and calling
    // them one thing is what this guard was doing wrong until 19 Sep 2026.
    // DEAD_IN_WIRED is for settings: a person changes the value and nothing
    // happens. PROMISES_ABOUT_WHAT_IS_NOT_BUILT is for `#[serde(skip)]`
    // fields, which no file can set at all --
    // `a_pinned_invariant_is_not_counted_as_a_setting` polices those, and
    // they are subtracted here so this one keeps meaning the first thing.
    let known: BTreeSet<String> = DEAD_IN_WIRED
        .iter()
        .map(|s| s.to_string())
        .chain(PROMISES_ABOUT_WHAT_IS_NOT_BUILT.iter().map(|(k, _)| k.to_string()))
        .collect();

    let added: Vec<&String> = found.difference(&known).collect();
    assert!(
        added.is_empty(),
        "these settings are in modules that run, and nothing reads them -- a person can change \
         them today and nothing will happen:\n  {}\n\nRead it where it was meant to be read, or \
         add it to DEAD_IN_WIRED with the reason it cannot be read yet.",
        added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );

    // Only the settings half is a ratchet in this direction: a promise
    // gaining a reader is handled by the pinned-invariant guard, which knows
    // what to say about it.
    let settings: BTreeSet<String> = DEAD_IN_WIRED.iter().map(|s| s.to_string()).collect();
    let cleared: Vec<&String> = settings.difference(&found).collect();
    assert!(
        cleared.is_empty(),
        "these settings now have a reader:\n  {}\n\nGood -- delete those lines.",
        cleared.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn the_unwired_modules_settings_stay_accounted_for() {
    let (_, found) = current();
    let known: BTreeSet<String> = DEAD_IN_UNWIRED.iter().map(|s| s.to_string()).collect();

    let added: Vec<&String> = found.difference(&known).collect();
    assert!(
        added.is_empty(),
        "new unread settings in unwired modules:\n  {}",
        added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );

    let cleared: Vec<&String> = known.difference(&found).collect();
    assert!(
        cleared.is_empty(),
        "these are now read -- probably because their module got wired:\n  {}\n\nDelete the \
         lines, and check UNWIRED_BASELINE in the same change.",
        cleared.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn no_yaml_key_is_shipped_without_a_reader() {
    // A key in a config file that no struct even has a slot for. There is no
    // typo-check that catches this: it loads, it parses, it does nothing.
    let src = sources();
    let all: String = src.iter().map(|(_, b)| b.as_str()).collect::<Vec<_>>().join("\n");
    let known: BTreeSet<String> = YAML_KEYS_WITH_NO_READER.iter().map(|s| s.to_string()).collect();

    // The whole YAML corpus, so a key read by another config file — a `vars:`
    // entry interpolated as `{key}` into a request template, or a layout name
    // referenced by an app — is not mistaken for one nothing reads. Checking
    // only `.rs` source was blind to both mechanisms, and reported four keys
    // (`left_half`, `right_half`, `llm_model`, `webcam_device`) as dead when
    // they are read every run. A guard with false positives teaches the next
    // person to distrust it, which is the one thing this guard cannot afford.
    let yaml_all: String = {
        let mut s = String::new();
        if let Ok(dir) = std::fs::read_dir("config") {
            for entry in dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                    if let Ok(t) = std::fs::read_to_string(&path) {
                        s.push('\n');
                        s.push_str(&t);
                    }
                }
            }
        }
        s
    };

    let mut found = BTreeSet::new();
    let Ok(dir) = std::fs::read_dir("config") else { return };
    for entry in dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        // The nearest section header above this line — the last line at
        // indentation zero that ends in a colon. `vars` and `layouts` are the
        // two whose children are read by value elsewhere rather than by name
        // in source.
        let mut parent = String::new();
        for line in text.lines() {
            if !line.is_empty()
                && !line.starts_with(|c: char| c == ' ' || c == '#')
            {
                if let Some(c) = line.find(':') {
                    parent = line[..c].trim().to_string();
                }
            }
            let t = line.trim_start();
            if t.starts_with('#') || !line.starts_with(|c: char| c == ' ' || c.is_alphabetic()) {
                continue;
            }
            let Some(colon) = t.find(':') else { continue };
            let key = &t[..colon];
            if key.is_empty() || !key.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()) {
                continue;
            }
            if all.contains(key) {
                continue;
            }
            // A `vars:` child is an interpolation variable — read wherever a
            // template says `{key}`, never as a bare identifier in `.rs`.
            if parent == "vars" && yaml_all.contains(&format!("{{{key}}}")) {
                continue;
            }
            // A layout name is read by value: an app selects it with
            // `layout: <name>` or `standalone_layout: <name>`.
            if file == "layouts.yaml"
                && (yaml_all.contains(&format!("layout: {key}"))
                    || yaml_all.contains(&format!("standalone_layout: {key}")))
            {
                continue;
            }
            found.insert(format!("{file}:{key}"));
        }
    }

    let added: Vec<&String> = found.difference(&known).collect();
    assert!(
        added.is_empty(),
        "these keys are in a shipped config file and no source file names them at all:\n  {}",
        added.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
}

#[test]
fn all_three_lists_are_sorted_and_free_of_duplicates() {
    for (list, name) in [
        (DEAD_IN_WIRED, "DEAD_IN_WIRED"),
        (DEAD_IN_UNWIRED, "DEAD_IN_UNWIRED"),
        (YAML_KEYS_WITH_NO_READER, "YAML_KEYS_WITH_NO_READER"),
    ] {
        let mut sorted = list.to_vec();
        sorted.sort_unstable();
        assert_eq!(list, &sorted[..], "{name} is not sorted");
        let unique: BTreeSet<&&str> = list.iter().collect();
        assert_eq!(unique.len(), list.len(), "{name} has a duplicate");
    }
}
