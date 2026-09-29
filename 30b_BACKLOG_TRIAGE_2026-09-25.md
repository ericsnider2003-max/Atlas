# The built-but-unused backlog, sorted (25 September 2026)

All 253 functions that had tests but no caller in the running program, each read and put in one bin. What happened to them is in doc 30.

| Bin | Count | What was done |
|---|---|---|
| WIRE | 9 | 8 wired. `improve::automatic_cost_mb` was left, because its place is the hub, which is paused. |
| DUPLICATE | 40 | 28 removed, with their tests pointed at the production path. The rest were kept because they are heavily used in tests or sit in files that were being edited. |
| DEAD | 39 | 34 removed. The "always true/false" ones stated a rule; those rules are recorded in doc 30. |
| TEST-SUPPORT | 19 | Kept on purpose. |
| NEEDS-RULING | 145 | Untouched: yours to decide. About 22 are trading, about 18 are security, vault or consent, and the rest are halves of features never built. |
| ALREADY-CALLED | 1 | `hub::crumbs`: the scan missed a call written as `.map(crumbs)`. |

| item | bin | evidence |
|---|---|---|
| `adapt::leaks_a_username` | WIRE | doctor::run (src/doctor.rs:20) should flag a generic-layer config/*.yaml containing /Users/<name>; main.rs only calls adapt::portable (main.rs:6495) |
| `categories::consent_line` | WIRE | daemon.rs:2283 builds "{default_say} Go ahead?" with no category; consent_line names what is being approved (policy.rs:321 already has category_of) |
| `connectivity::invalidate` | WIRE | daemon.rs:6081 research-failed branch records i.failed(...) but leaves the Online cache standing; call self.connectivity.invalidate() |
| `daily::note_day` | WIRE | daemon.rs:3511 day-roll block never increments days_watched, so Rhythm::quiet_hour is None forever (daily.rs:164) |
| `improve::automatic_cost_mb` | WIRE | hublive.rs:443 Recommendations page lists improve::automatic() but not what it costs; add the total |
| `panel::place_anyway` | WIRE | daemon.rs:3983 parks pending_panel after AskFirst but no answer handler reads it; add a branch beside pending_offer (daemon.rs:1716) using place_anyway on yes |
| `perf::may_scan` | WIRE | Daemon::observe (daemon.rs:8285) rescans the index with no battery/size check; gate the indexing cfg on Throttle::may_scan(self.power, entries) |
| `prose::check_phrases` | WIRE | daemon.rs:5153 review runs prose::check only; extend with check_phrases(what) — its Kind::Likely fixes go to the "unsure" question, not auto-applied |
| `sandbox::discard` | WIRE | Build errand creates a timestamped sandbox (daemon.rs:9642) and never removes it after writing out_dir; discard after build_loop |
| `awake::on_waking` | DUPLICATE | Superseded "old shape"; daemon uses awake::on_waking_checked (daemon.rs uses awake::on_waking_checked) |
| `awake::woke` | DUPLICATE | Daemon speaks awake::woke_checked instead |
| `channel::stands_alone` | DUPLICATE | channel.rs:176 calls check_result directly on the result path; this is a bool wrapper |
| `diagnose::yours` | DUPLICATE | diagnose::report (diagnose.rs:225) already extracts Remedy::Yours for the Status page (hublive.rs:459) |
| `faithful::plain_report` | DUPLICATE | lead_with_the_problem does this for overnight.rs and channel.rs |
| `freshness::shelf_for_source` | DUPLICATE | facts.rs maps Checkable::YouSaid to Shelf::Yours itself; reference/recall use shelf_for |
| `gaze::in_use` | DUPLICATE | Live sign set is handshape::Vocabulary (daemon.rs Vocabulary::load) |
| `handshape::about_the_recordings` | DUPLICATE | hublive.rs uses as_demonstrated/needs_deciding directly |
| `handshape::recognise` | DUPLICATE | Wrapper for recognise_moving(hand, Motion::default()); handloop uses the moving form |
| `hub::list_page` | DUPLICATE | Thin wrapper for list_page_at(None, ...); hublive uses list_page_at (4 calls) |
| `install::required` | DUPLICATE | install.rs:119/352 filter pieces by !optional inline |
| `layout_prefs::has_duplicate` | DUPLICATE | Hub dashboard editing is dash::Dashboard::apply (dash.rs:381), not layout_prefs |
| `layout_prefs::move_to` | DUPLICATE | dash::Dashboard::apply Move::To/Up/Down (dash.rs:383-385) |
| `layout_prefs::resize` | DUPLICATE | dash::Dashboard::apply Move::Widen (dash.rs:388) |
| `mail::how_to_connect` | DUPLICATE | `atlas mail setup` (main.rs:4409-4425) is the live text, and it contradicts this one on Outlook (OAuth vs app password) |
| `mend::worth_trying` | DUPLICATE | selfwork.rs calls mend::paper_overs directly |
| `modes::detail_level` | DUPLICATE | Daemon computes the cap from modes::sentences_for + register.length() (daemon.rs:1957-1960, 2360-2363) |
| `ocr::region_capture_args` | DUPLICATE | words::capture_args (words.rs:664, used from main.rs) is the live gdigrab builder |
| `persona::is_flattery` | DUPLICATE | persona.spoken → strip_filler removes FILLER_OPENERS incl. "great question" (persona.rs:68, daemon.rs:2368) |
| `persona::shape_for` | DUPLICATE | Daemon caps max_spoken_sentences with register.length() then calls persona.spoken (daemon.rs:2360-2368) |
| `policy::classify_with_history` | DUPLICATE | Wrapper for classify_with_policy, which daemon calls directly (daemon.rs:2178, 7406, 7509) |
| `posix::works_here` | DUPLICATE | Platform answer comes from capability::on_platform_summary/portable::honest_summary (daemon.rs ~4033) |
| `quickinput::voice_failed` | DUPLICATE | Voice-failure fallback is input::Tiers / self.degrade (daemon.rs:15620) |
| `quickinput::voice_worked` | DUPLICATE | tiers.succeeded() (daemon.rs:15617) plays this role |
| `references::is_ambiguous` | DUPLICATE | daemon.rs:1605 matches Resolution::Ambiguous directly |
| `roots::models_dir` | DUPLICATE | Models path comes from config tools.models.dir (main.rs:3835); roots.rs's own doc warns against second paths |
| `safety::describe_last` | DUPLICATE | Undo path names the restored file from trash.undo_last (daemon.rs:3710-3717) |
| `session::resolve_approval` | DUPLICATE | Daemon resolves Pending::Approval inline with is_yes (daemon.rs:1614-1660) |
| `stance::support_strength` | DUPLICATE | stance::assess + brief (daemon.rs:5086) produce the verdict; this score has no reader |
| `tier::slow` | DUPLICATE | Daemon matches Tier::Think directly (daemon.rs:2038, 2068) |
| `timing::worst` | DUPLICATE | Recent::worst_stage (timing.rs:193) is the aggregate used by why_slow |
| `trace::broke` | DUPLICATE | record_model_call sets c.failed directly (daemon.rs:14672) |
| `tray::needs_tool` | DUPLICATE | Tray read path dispatches by Sort to read_photo/listen_to, which check their tools (daemon.rs:12820-12834) |
| `tts::download_urls` | DUPLICATE | Voice download URLs live in getpieces.rs:95-103 |
| `tts::voice_file` | DUPLICATE | Wrapper for voice_file_for, used at voice.rs:561/764 and doctor.rs:474 |
| `tune::full_pass` | DUPLICATE | Daemon runs tune::examine and filters itself (daemon.rs:3777-3781) |
| `viewing::weave` | DUPLICATE | Wrapper for weave_seen, which daemon calls (daemon.rs:11227) |
| `voice::wake_heard_in` | DUPLICATE | Same expression inline in the wake loop (voice.rs:892, 925) |
| `words::same_line_as` | DUPLICATE | words.rs:259 calls shares_a_line directly |
| `workspace::input_allowed` | DUPLICATE | may_dictate_into uses workspace::input_blocked_apps (daemon.rs:13180); differs on unknown apps |
| `callrec::silent` | TEST-SUPPORT | Injected via Notes.starter as the no-device recorder in tests; production uses callrec::start (callnotes.rs:156) |
| `connectivity::unpin` | TEST-SUPPORT | Pair of Connectivity::set, which nothing in production calls either; used for deterministic tests |
| `daemon::pending_offer` | TEST-SUPPORT | Read-only accessor; daemon reads the field directly (daemon.rs:1718) |
| `faithful::not_checked` | TEST-SUPPORT | Step constructor; production uses Step::did/skipped (daemon.rs, overnight.rs); tests build NotChecked steps |
| `fixtures::box_range` | TEST-SUPPORT | Synthetic bar series; used only by market #[cfg(test)] mods (claims.rs:705, regime.rs:395) and tests — market |
| `fixtures::from_path` | TEST-SUPPORT | Same (market/levels.rs:432 test mod) |
| `fixtures::ramp` | TEST-SUPPORT | Same (market/bars.rs:728 test mod) |
| `fixtures::zigzag` | TEST-SUPPORT | Same (market/claims.rs:797 test mod) |
| `flow::optional` | TEST-SUPPORT | Step builder; production flows come from flow::Library, tests build steps in code |
| `flow::producing` | TEST-SUPPORT | Step builder, as above |
| `flow::retrying` | TEST-SUPPORT | Step builder, as above |
| `mock::clipboard_now` | TEST-SUPPORT | Fake OS inspection (src/platform/mock.rs:81) |
| `mock::focus_on` | TEST-SUPPORT | Fake OS setup |
| `mock::set_clipboard` | TEST-SUPPORT | Fake OS setup |
| `mock::set_window_text` | TEST-SUPPORT | Fake OS setup |
| `mock::with_slow_app` | TEST-SUPPORT | Fake OS setup |
| `nudge::offering` | TEST-SUPPORT | Goal builder; no production Goal tracking exists |
| `uia::disabled` | TEST-SUPPORT | Node builder for UIA fixtures |
| `uia::valued` | TEST-SUPPORT | Node builder for UIA fixtures |
| `accounts::asked_to_weaken` | NEEDS-RULING | Refusal guard for "turn off 2FA"; wiring adds a security refusal path — security, owner decides |
| `accounts::instead` | NEEDS-RULING | Spoken alternative to weakening 2FA; pairs with asked_to_weaken — security wording, owner decides |
| `anticipate::enable` | NEEDS-RULING | No setting/intent toggles anticipator rules (daemon only calls due(), daemon.rs:7626); decide if user can switch individual rules |
| *[row removed 28 Sep 2026: trading-system material]* |
| `awake::running_state` | NEEDS-RULING | Needs session-lock/display-off signals no platform reads (daemon builds Power with lid unknown, daemon.rs:8683) — hardware |
| `awake::work_continues` | NEEDS-RULING | Method on Running, which only running_state produces — same missing lock/display signals |
| `calendar::for_phone` | NEEDS-RULING | Needs a phone app writing EventKit/CalendarProvider; no /calendar route in server.rs:652-746 — hardware/app |
| `calendar::merge_from_phone` | NEEDS-RULING | Same phone bridge; nothing ever sets phone_key except None (daemon.rs:10318) |
| `capture::found` | NEEDS-RULING | No "find that note" intent exists; wiring = new spoken recall feature |
| `capture::never_revisited` | NEEDS-RULING | Would add an unprompted "you never looked at these" line — new speech |
| `categories::media_decision` | NEEDS-RULING | No media export/overwrite pipeline exists; approval policy for media is the owner's |
| `chain::what_stands` | NEEDS-RULING | brief.rs:444 builds a Chain but never runs one; reporting irreversible steps needs a chain executor |
| `confirmed::read_back` | NEEDS-RULING | Security-page changes; main.rs uses saying_it_back/needs_reading_back directly — security, owner decides |
| `consent::announcement_named` | NEEDS-RULING | Call-recording announcement choice — consent wording, owner decides |
| `consent::script` | NEEDS-RULING | What Atlas posts in a call chat — speaks to third parties |
| `consolidate::knew_once` | NEEDS-RULING | Needs tombstones, which the daemon never keeps (only consolidate::trim, daemon.rs:2544) |
| `consolidate::make_room` | NEEDS-RULING | Would replace trim at daemon.rs:2544 but lossily compacts stored claims and needs a new persisted stones field — owner decides |
| `consolidate::once_knew` | NEEDS-RULING | Same missing tombstone store as knew_once |
| `consult::costs_an_attempt` | NEEDS-RULING | Consultation is created then discarded (daemon.rs:13976 `Some(_)`); driving it means another AI in a window — outside the machine |
| `consult::is_ready` | NEEDS-RULING | Same undriven consultation |
| `consult::report_result` | NEEDS-RULING | Same undriven consultation |
| `consult::unproductive` | NEEDS-RULING | Same undriven consultation |
| `craft::as_goal` | NEEDS-RULING | Feeds goal::Goal, whose only purpose is unattended long jobs — autonomy decision |
| `credentials::needs_you_awake` | NEEDS-RULING | Access page (hublive.rs:521-533) could show it, but it Box::leaks per call (credentials.rs:148); fix first, then owner decides |
| `daily::find_dropped` | NEEDS-RULING | Daemon.dropped (daemon.rs:543) has no writer and no intent; "bring back what I dropped" is unbuilt |
| `daily::noticed` | NEEDS-RULING | Unprompted "you're usually done by N" line; only meaningful after note_day is wired |
| `daily::picking_back_up` | NEEDS-RULING | Same unbuilt dropped-items feature |
| `decide::can_explain` | NEEDS-RULING | work_a_decision (daemon.rs:6404) is stateless per turn; multi-turn decision working (lean/set_aside) unbuilt, speaks recommendations |
| `decide::lean` | NEEDS-RULING | Same; producing a recommendation is the owner's call |
| `decide::set_aside` | NEEDS-RULING | Same held-Decision feature |
| `delegate::refused` | NEEDS-RULING | Step::Confirm stops the job (daemon.rs:12355) and no yes/no handler calls confirmed()/refused(); resuming acts in other apps |
| `diagnose::self_fixable` | NEEDS-RULING | "Ones Atlas may act on unasked" — autonomous remediation |
| `editcraft::check_cuts` | NEEDS-RULING | Needs an edit timeline Atlas does not read; content-advice feature unbuilt |
| `editcraft::is_scheduling_rather_than_capturing` | NEEDS-RULING | Unprompted habit critique; needs per-day capture counts nothing records |
| `editcraft::judge_deal` | NEEDS-RULING | Money/affiliate-deal advice — finance-adjacent speech |
| `editcraft::profile_note` | NEEDS-RULING | Creator-profile advice line, no input source; new speech |
| `editcraft::too_many_effects` | NEEDS-RULING | Needs transition list from an editor project; unbuilt |
| `enrol::is_final` | NEEDS-RULING | Account signup flow (acts outside the machine); daemon only calls permitted/domain_from |
| `enrol::split_login` | NEEDS-RULING | Reads vault login pairs — security |
| `enrol::spoken_result` | NEEDS-RULING | Signup result speech; signup execution unbuilt |
| `enrol::vault_write` | NEEDS-RULING | Writes credentials to vault — security |
| `events::collisions` | NEEDS-RULING | market/events.rs — trading |
| `facts::dangling` | NEEDS-RULING | Reporting dangling links is new speech/hub content; facts::Book is live (daemon.rs) so small if wanted |
| `facts::settles` | NEEDS-RULING | Conflict-resolution rule for facts; wiring changes which fact wins in answers |
| `files::after_scan` | NEEDS-RULING | No scanning pipeline exists (files::convert only answers, daemon.rs:16107); feature unbuilt |
| `files::pdf_is_really_a_scan` | NEEDS-RULING | No PDF text extraction in src; unbuilt |
| `files::safe_to_unpack` | NEEDS-RULING | Nothing unpacks archives (files.rs:105 comment); unbuilt |
| `files::scan_steps` | NEEDS-RULING | Same scanning pipeline |
| `finance::needs_credentials` | NEEDS-RULING | Finance |
| `fit::limits` | NEEDS-RULING | Plain "what I can't do here" lines; daemon has fit::plan_here/describe — adding speech is owner's call (small) |
| `fit::worth_replanning` | NEEDS-RULING | Needs a stored previous Machine to compare; re-planning changes models chosen |
| `fxday::position_of` | NEEDS-RULING | FX trading day — trading |
| `goal::machine_checks` | NEEDS-RULING | Goal exists only via craft::as_goal; unattended long-job loop unbuilt — autonomy |
| `goal::runnable_unattended` | NEEDS-RULING | Same; gate for running while you sleep |
| `grading::recommended_setup` | NEEDS-RULING | Resolve colour-management advice; grading::spoken is the live path — new advice content |
| `grading::the_mistake` | NEEDS-RULING | Same grading advice content |
| `handoff::read_answer` | NEEDS-RULING | Parses another AI's reply; consultation is never driven (daemon.rs:13976) |
| `handoff::write_brief` | NEEDS-RULING | Brief for an outside helper; same undriven hand-off |
| `handshape::adopt` | NEEDS-RULING | Teaching a new gesture from the camera — camera/hardware feature |
| `handshape::worked_out` | NEEDS-RULING | Same gesture-teaching feature |
| `hearing::record_turn` | NEEDS-RULING | Daemon never holds a Hearing (only main.rs calibration CLI); ear selection needs audio hardware |
| `hub::index_rows` | NEEDS-RULING | No index page (businesses/partners/projects) exists yet; hub navigation design is the owner's |
| `improve::hints` | NEEDS-RULING | No whisper prompt/hints plumbing exists; would change transcription input |
| *[row removed 28 Sep 2026: trading-system material]* |
| `input::down` | NEEDS-RULING | Push-to-talk key state; there is no global hotkey (daemon.rs:15615) — needs Windows keyboard hook |
| `input::is_talking` | NEEDS-RULING | Same push-to-talk state |
| `input::up` | NEEDS-RULING | Same push-to-talk state |
| *[row removed 28 Sep 2026: trading-system material]* |
| `language::good_enough` | NEEDS-RULING | No per-turn language confidence is tracked in the daemon (only model_facts/insert_whisper_vars) |
| `language::live_line` | NEEDS-RULING | Live multilingual panel line; needs the unbuilt translation turn pipeline |
| `language::notes` | NEEDS-RULING | Multilingual call notes; same pipeline |
| `language::suggestion` | NEEDS-RULING | Offers switching to a bigger speech model — changes model, owner decides |
| `ledger::keep_for` | NEEDS-RULING | Tax record-keeping advice — finance |
| `ledger::relevant_rule` | NEEDS-RULING | Tax/trading rules — finance/trading |
| `levels::bounce_rate` | NEEDS-RULING | src/market/levels.rs:423 — trading |
| `levels::other_targets` | NEEDS-RULING | src/levels.rs:201 stop/target — trading |
| `live::closed` | NEEDS-RULING | Live bar feed — trading |
| `live::is_forming` | NEEDS-RULING | Live bar feed — trading |
| `live::settled_bars` | NEEDS-RULING | Live bar feed — trading |
| `mail::may_touch` | NEEDS-RULING | No code moves or labels mail (brief.rs:390 only classifies); guard belongs in a mailbox-mutation path the owner hasn't approved |
| `mail::rehearsal` | NEEDS-RULING | "Say go and I'll sort them" — acts on your mailbox |
| `mail::what_the_trail_says` | NEEDS-RULING | Alias-leak advice; needs alias tracking nothing records |
| `messaging::filed` | NEEDS-RULING | Unprompted "X looks like work" line; new speech |
| `nudge::track` | NEEDS-RULING | Daemon never tracks nudge Goals; a goal-nudging feature speaks unprompted |
| `online::dispatch_task` | NEEDS-RULING | Sends tasks to an online worker — leaves the machine |
| `opportunity::atlas_can_judge` | NEEDS-RULING | Whole opportunity module has no production caller; unbuilt, includes money axis |
| `opsec::approaching` | NEEDS-RULING | Unprompted countdown line about the user's restriction end date — speech on a sensitive topic |
| `opsec::no_longer_applies` | NEEDS-RULING | Same end-date announcement |
| `overlay::window_style` | NEEDS-RULING | Native overlay window flags; overlaywin.rs does not build the layered window — Windows UI work |
| `overnight::delegation_for` | NEEDS-RULING | Overnight delegation drives another AI in a window while you sleep |
| `overnight::morning_detail` | NEEDS-RULING | daemon.rs:8621 speaks only morning_brief; storing the long account needs somewhere to read it (no reader exists) — decide where it shows |
| `overnight::spend_turns` | NEEDS-RULING | Same overnight delegation budget |
| `panel::place_on_second` | NEEDS-RULING | "Put it on the other screen" intent doesn't exist; panels aren't rendered (wants_panel has no reader) |
| `panel::window_args` | NEEDS-RULING | Launching a chromeless browser window — panel rendering is unbuilt |
| `person::learn_from_edit` | NEEDS-RULING | Needs before/after of user edits to Atlas output; no such capture path |
| `person::touched` | NEEDS-RULING | No project attribution per turn; learning-about-you data, owner decides |
| `pipeline::record_refinement` | NEEDS-RULING | Self-modification loop Refine stage (daemon.rs:14175); autonomy over own code |
| `probe::target_for` | NEEDS-RULING | Moving your windows to answer a question — acts on your desktop |
| `prose::is_your_style` | NEEDS-RULING | Live as-you-type correction in other apps is unbuilt |
| `prose::leave_alone` | NEEDS-RULING | Same live-correction learner |
| `prose::may_correct_in` | NEEDS-RULING | Same; guard for typing into other windows |
| `prose::you_undid` | NEEDS-RULING | Same live-correction learner |
| `publish::schedule` | NEEDS-RULING | Scheduling public posts — acts outside the machine |
| `publishing::rules` | NEEDS-RULING | Format advice lines; new content-advice speech |
| `quickinput::backspace` | NEEDS-RULING | Overlay typing box not built (main.rs:6999 says so); console path reads whole lines |
| `quickinput::parse_hotkey` | NEEDS-RULING | No RegisterHotKey anywhere in src — needs Windows hotkey work |
| `recovery::honest_weakness` | NEEDS-RULING | Vault recovery wording — security |
| `recovery::visible_if_used` | NEEDS-RULING | Vault recovery property — security |
| `reference::correct_it` | NEEDS-RULING | Kept-reference store isn't held by daemon (uses reference::chosen/nothing_found); correction semantics are owner's |
| `reference::gone_off` | NEEDS-RULING | Same kept store |
| `reference::never_used` | NEEDS-RULING | Same kept store |
| `reference::trading_mb` | NEEDS-RULING | Trading reference shelf — trading |
| `register::announcement_for` | NEEDS-RULING | Call-recording announcement text — consent |
| `revise::repeat_rate` | NEEDS-RULING | Metric could go on a hub page; Mending is live (daemon) — owner decides where |
| `revise::stale_notes` | NEEDS-RULING | Surfacing stale corrections is new content |
| `route::stuck_spoken` | NEEDS-RULING | New spoken line for route::plan Stuck; daemon handles plan itself |
| `route::switching` | NEEDS-RULING | New mid-task narration line |
| `routine::starting` | NEEDS-RULING | Routines aren't run automatically (brief.rs:466 only checks due); running them acts |
| *[row removed 28 Sep 2026: trading-system material]* |
| `selfaudit::as_thought` | NEEDS-RULING | Would feed recommendations straight into the self-modification pipeline (daemon.rs:7899) — autonomy |
| `selfgrant::raise_it` | NEEDS-RULING | Self-change recommendation speech; selfgrant::asking_for is the live line |
| `selfgrant::raised_again` | NEEDS-RULING | Repeat-reminder speech |
| `selfgrant::seen_you` | NEEDS-RULING | Would make self-grants expire; Granted is rebuilt per landing with days_present 0 (selfwork.rs:877-887) — authority decision |
| `server::delay_ms` | NEEDS-RULING | server::Failures is never used; auth backoff on the 401 path (server.rs:1219) is a security change |
| `server::with_peers` | NEEDS-RULING | Opens a door to trusted peers — security |
| `session::overlap_hours` | NEEDS-RULING | src/market/session.rs:273 — trading |
| `signin::may_fill` | NEEDS-RULING | Password autofill gate — security |
| `signin::quiet` | NEEDS-RULING | Unused-credential reminders — security |
| `strategy::what_was_learned` | NEEDS-RULING | Belongs in the outside-help brief, which is never sent (daemon.rs:13976) |
| `sync::how_to_carry` | NEEDS-RULING | Choosing cable/cloud/Apple sync needs device detection nothing does — hardware/network |
| `thread::fold_input` | NEEDS-RULING | daemon.rs:2461 "folds" the thread to the string "N earlier exchanges"; a real summary needs a model call per fold — cost/latency decision |
| `timebox::warn_up_front` | NEEDS-RULING | Would add "this'll take about ten minutes" speech |
| `tts::audition_line` | NEEDS-RULING | Voice audition flow (download and play voices) unbuilt; network + audio |
| `tune::mechanism_for` | NEEDS-RULING | daemon.rs:3778 passes tune::Survey::default(), so examine() returns nothing and this would never fire; needs a real machine survey first |
| `tune::storage_plan` | NEEDS-RULING | Moves models/captures/video to another drive |
| `uia::by_name` | NEEDS-RULING | Finding controls to click in other apps (UIA) — acting in other windows |
| `undo::mark_undone` | NEEDS-RULING | reverse() only asks "Undo X?" (undo.rs:257); no confirmed-undo execution exists to mark done |
| *[row removed 28 Sep 2026: trading-system material]* |
| `workspace_view::why_this_took_so_long` | NEEDS-RULING | Hub item detail page doesn't exist; new hub content |
| `asking::query` | DEAD | Production deliberately searches each term separately (daemon.rs:2934 find_files); a joined query is the thing it avoids |
| `awake::needs_the_screen` | DEAD | Constant `false`; documents a rule, nothing branches on it |
| `categories::leaves_the_machine` | DEAD | Gate works off category_of().default_decision() (policy.rs:321); brain.rs:150 deliberately uses its own name |
| `categories::uploads_your_content` | DEAD | Only ExternalAiCreative; nothing asks it separately (earned.rs:516 matches the variant directly) |
| `content::kept_rate` | DEAD | Metric method; how_its_going (main.rs) does not use it and nothing ranks by it |
| `council::first_angle` | DEAD | Ties council to otherside vocabulary; no production reader |
| `decide::names_a_winner` | DEAD | Check for model text picking a winner; no model-drafted decision path exists |
| `editcraft::better_than_post` | DEAD | Transition predicate; reply_to (daemon) never reads transitions |
| `editcraft::free_in_post` | DEAD | Transition predicate, no reader |
| `endpoint::saved_against_fixed` | DEAD | Statistic vs fixed-window recording; nothing reports it |
| `enrol::avoids_ambiguous` | DEAD | Constant `true`; documents a rule |
| `firewall::coming_back` | DEAD | Constant Allowed; documents one-way boundary |
| `firstrun::essential` | DEAD | Constant `false` |
| `freshness::still_true` | DEAD | Known is rebuilt per read (recall.rs, tier.rs); no persisted Known to reconfirm |
| `gaze::identity_is_enough_for_sensitive_work` | DEAD | Constant `false`; handover.rs:5 cites it in docs only |
| `gguf::vocab_size` | DEAD | models.rs reads Gguf but never needs vocab size |
| `grade::clears` | DEAD | main.rs:8768 uses SafeArea::caption_band; no caption positions are checked |
| `household::implies_belonging` | DEAD | Constant `false` |
| `hub::works_without_voice` | DEAD | Documents which pages run standalone; run_hub serves them without consulting it |
| `identity::reason` | DEAD | Gate accessor; main.rs matches Gate variants itself |
| `index::by_class` | DEAD | No caller wants entries by class; search/recent are used |
| `lanes::push_urgent` | DEAD | Nothing marks work urgent; queue uses push/push_online (daemon.rs:7634) |
| `learned::reconsider` | DEAD | Daemon holds no Learned store — it passes Learned::default() (daemon.rs:6379), so there is nothing to reconsider |
| `mesh::free` | DEAD | Constant `true` |
| `metrics::one_line` | DEAD | main.rs:6256 prints render(); no status surface wants the one-liner |
| `overlay::click_through` | DEAD | Style predicate; overlaywin.rs never uses WS_EX flags from here |
| `overlay::stays_out_of_the_way` | DEAD | Style predicate, as above |
| `panel::transient` | DEAD | Only Waking; nothing auto-dismisses panels |
| `presence::away_for` | DEAD | No reader wants the away duration; presence changes are used directly (daemon.rs:11651) |
| `signin::can_change_security` | DEAD | Constant `false` |
| `speech::heard_text` | DEAD | say_interruptibly's Delivery is discarded (daemon.rs:15872); the unsaid part is parked via remaining_text |
| `sync::can_talk` | DEAD | Kind predicate; sync::Kind unused in production |
| `sync::works_alone` | DEAD | Kind predicate, as can_talk |
| `timebox::keep_going` | DEAD | No path asks "keep going?"; brief.rs:436 starts a Box_ and never stalls it into a question |
| `timing::total_ms` | DEAD | Recent uses atlas_ms; total including uncontrolled stages has no reader |
| `tts::needs_gpu` | DEAD | Constant `false` |
| `vault::usable_unattended` | DEAD | Constant `false` |
| `workspace_view::drift_spoken` | DEAD | Input estimate_drift is always None: nothing writes Item.spent_mins (only read, workspace_view.rs:499) |
| `workspace_view::estimate_drift` | DEAD | Same — spent_mins has no writer |
| `hub::crumbs` | ALREADY-CALLED | Not dead: passed as a fn value at src/hub.rs:1429 `.map(crumbs)` inside shell_with — the list's caller scan missed it |
