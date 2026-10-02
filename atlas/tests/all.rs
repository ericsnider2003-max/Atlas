//! The consolidated integration-test binary.
//!
//! Every self-contained, state-isolated `tests/*.rs` file is included here
//! as a module via `#[path]`, so the whole suite links the crate **once**
//! instead of once per file. The files stay exactly where they are — the
//! content guards that scan `tests/` read them in place, unchanged — this
//! file only changes how they are compiled. A new self-contained test file
//! needs a `mod` line here (autotests is off, so nothing is auto-discovered).
//!
//! Two kinds of file keep their own `[[test]]` target in Cargo.toml instead:
//!   1. Eight that declare their own `mod common;` (a file submodule can't be
//!      resolved through a `#[path]` include).
//!   2. Files that drive the *global* install state — the vault and the
//!      handover, which `roots::install_state()` resolves to one shared path
//!      per process (a `OnceLock`). Run in one process they would stomp each
//!      other's handover/lock state and refuse each other's turns; a process
//!      per file keeps that state to itself, exactly as before.
#![allow(clippy::all)]

// Shared helpers for the included files, reached as `crate::common::...` --
// chiefly `source_of`, which reads a module's source wherever its files live
// (27 Sep 2026, ahead of the daemon.rs / main.rs split). An included file
// still can't declare its own `mod common;`; it uses this one.
mod common;

#[path = "kokoro_voice.rs"]
mod kokoro_voice;
#[path = "one_way_to_do_each_small_thing.rs"]
mod one_way_to_do_each_small_thing;
#[path = "reading_a_module_wherever_it_lives.rs"]
mod reading_a_module_wherever_it_lives;
#[path = "an_update_that_fails_goes_back.rs"]
mod an_update_that_fails_goes_back;
#[path = "installing_a_release.rs"]
mod installing_a_release;
#[path = "feedback_from_friends.rs"]
mod feedback_from_friends;
#[path = "nothing_from_any_version_was_lost.rs"]
mod nothing_from_any_version_was_lost;
#[path = "a_backup_that_holds_everything.rs"]
mod a_backup_that_holds_everything;
#[path = "a_bar_knows_when_it_opened_and_closed.rs"]
mod a_bar_knows_when_it_opened_and_closed;
#[path = "a_brief_without_an_inbox.rs"]
mod a_brief_without_an_inbox;
#[path = "a_conversation_not_a_script.rs"]
mod a_conversation_not_a_script;
#[path = "talking_freely.rs"]
mod talking_freely;
#[path = "the_second_scan_holds.rs"]
mod the_second_scan_holds;
#[path = "other_programs_tools.rs"]
mod other_programs_tools;
#[path = "reading_pages_well.rs"]
mod reading_pages_well;
#[path = "mail_through_himalaya.rs"]
mod mail_through_himalaya;
#[path = "reading_without_the_picture_reader.rs"]
mod reading_without_the_picture_reader;
#[path = "speculative_decoding.rs"]
mod speculative_decoding;
#[path = "a_document_says_when_it_stopped_being_true.rs"]
mod a_document_says_when_it_stopped_being_true;
#[path = "a_flow_is_a_run_not_a_pile_of_pushes.rs"]
mod a_flow_is_a_run_not_a_pile_of_pushes;
#[path = "a_group_has_an_owner.rs"]
mod a_group_has_an_owner;
#[path = "a_judgment_says_how_sure_it_is.rs"]
mod a_judgment_says_how_sure_it_is;
#[path = "a_month_against_your_own_months.rs"]
mod a_month_against_your_own_months;
#[path = "a_note_on_who_messaged.rs"]
mod a_note_on_who_messaged;
#[path = "a_private_network_you_do_not_have.rs"]
mod a_private_network_you_do_not_have;
#[path = "a_question_that_survives_you_leaving.rs"]
mod a_question_that_survives_you_leaving;
#[path = "a_reader_for_the_messages.rs"]
mod a_reader_for_the_messages;
#[path = "a_recording_does_not_outlive_its_transcript.rs"]
mod a_recording_does_not_outlive_its_transcript;
#[path = "a_reply_is_read_as_bytes_and_bounded.rs"]
mod a_reply_is_read_as_bytes_and_bounded;
#[path = "a_room_that_disagrees.rs"]
mod a_room_that_disagrees;
#[path = "a_save_that_fails_is_not_silent.rs"]
mod a_save_that_fails_is_not_silent;
#[path = "a_stage_that_cannot_be_reached.rs"]
mod a_stage_that_cannot_be_reached;
#[path = "a_switch_that_does_nothing.rs"]
mod a_switch_that_does_nothing;
#[path = "a_window_title_is_not_an_instruction.rs"]
mod a_window_title_is_not_an_instruction;
#[path = "access_and_settings.rs"]
mod access_and_settings;
#[path = "accounts_book.rs"]
mod accounts_book;
#[path = "adapt.rs"]
mod adapt;
#[path = "add_ons_do_only_what_you_allowed.rs"]
mod add_ons_do_only_what_you_allowed;
#[path = "adding_a_friend_is_one_step.rs"]
mod adding_a_friend_is_one_step;
#[path = "addressing_publish.rs"]
mod addressing_publish;
#[path = "afterme_companion.rs"]
mod afterme_companion;
#[path = "an_objection_is_never_too_early.rs"]
mod an_objection_is_never_too_early;
#[path = "android_mesh_household.rs"]
mod android_mesh_household;
#[path = "animation.rs"]
mod animation;
#[path = "answering.rs"]
mod answering;
#[path = "asia.rs"]
mod asia;
#[path = "asking.rs"]
mod asking;
#[path = "asking_atlas_to_work_on_itself.rs"]
mod asking_atlas_to_work_on_itself;
#[path = "asking_twice_is_noticed_through_the_daemon.rs"]
mod asking_twice_is_noticed_through_the_daemon;
#[path = "asking_which_one_you_meant.rs"]
mod asking_which_one_you_meant;
#[path = "atlas_can_be_told_to_stop.rs"]
mod atlas_can_be_told_to_stop;
#[path = "attention_grants_delegate.rs"]
mod attention_grants_delegate;
#[path = "audio.rs"]
mod audio;
#[path = "audio_enumeration.rs"]
mod audio_enumeration;
#[path = "awake_rhythm.rs"]
mod awake_rhythm;
#[path = "away_voiceover.rs"]
mod away_voiceover;
#[path = "backlog_uia.rs"]
mod backlog_uia;
#[path = "before_you_go.rs"]
mod before_you_go;
#[path = "brain_and_wake.rs"]
mod brain_and_wake;
#[path = "browser_http.rs"]
mod browser_http;
#[path = "budget.rs"]
mod budget;
#[path = "bug_sweep.rs"]
mod bug_sweep;
#[path = "build_from_a_description.rs"]
mod build_from_a_description;
#[path = "capability_honesty.rs"]
mod capability_honesty;
#[path = "capability_routine.rs"]
mod capability_routine;
#[path = "capture_content.rs"]
mod capture_content;
#[path = "catalogue.rs"]
mod catalogue;
#[path = "ceiling.rs"]
mod ceiling;
#[path = "channel.rs"]
mod channel;
#[path = "checks.rs"]
mod checks;
#[path = "cli_args.rs"]
mod cli_args;
#[path = "clipboard_rehearse.rs"]
mod clipboard_rehearse;
#[path = "clipboard_writeback.rs"]
mod clipboard_writeback;
#[path = "cloudsync.rs"]
mod cloudsync;
#[path = "codes_panel.rs"]
mod codes_panel;
#[path = "commands_are_distinct.rs"]
mod commands_are_distinct;
#[path = "commissioning.rs"]
mod commissioning;
#[path = "confirmed.rs"]
mod confirmed;
#[path = "connections_wired.rs"]
mod connections_wired;
#[path = "consent_routes.rs"]
mod consent_routes;
#[path = "consolidate.rs"]
mod consolidate;
#[path = "consult.rs"]
mod consult;
#[path = "contextual_recall.rs"]
mod contextual_recall;
#[path = "contents_trace_voice.rs"]
mod contents_trace_voice;
#[path = "council_brief.rs"]
mod council_brief;
#[path = "craft.rs"]
mod craft;
#[path = "craft_wired.rs"]
mod craft_wired;
#[path = "credentials.rs"]
mod credentials;
#[path = "crew.rs"]
mod crew;
#[path = "crew_efficiency.rs"]
mod crew_efficiency;
#[path = "daemon.rs"]
mod daemon;
#[path = "daily_booking.rs"]
mod daily_booking;
#[path = "dash.rs"]
mod dash;
#[path = "dead_config.rs"]
mod dead_config;
#[path = "dead_methods.rs"]
mod dead_methods;
#[path = "decide.rs"]
mod decide;
#[path = "declared.rs"]
mod declared;
#[path = "delivery_edit.rs"]
mod delivery_edit;
#[path = "design_taste.rs"]
mod design_taste;
#[path = "diagnose_reads_a_symptom.rs"]
mod diagnose_reads_a_symptom;
#[path = "dictating.rs"]
mod dictating;
#[path = "digest.rs"]
mod digest;
#[path = "earned.rs"]
mod earned;
#[path = "enrol.rs"]
mod enrol;
#[path = "entity_resolution.rs"]
mod entity_resolution;
#[path = "every_intent_reaches_the_daemon.rs"]
mod every_intent_reaches_the_daemon;
#[path = "every_way_in.rs"]
mod every_way_in;
#[path = "explaining_code.rs"]
mod explaining_code;
#[path = "facts.rs"]
mod facts;
#[path = "faithful.rs"]
mod faithful;
#[path = "files_remote.rs"]
mod files_remote;
#[path = "filing.rs"]
mod filing;
#[path = "filing_actually_files.rs"]
mod filing_actually_files;
#[path = "finance_consent.rs"]
mod finance_consent;
#[path = "finance_fields.rs"]
mod finance_fields;
#[path = "finding_files.rs"]
mod finding_files;
#[path = "finding_the_other_machine.rs"]
mod finding_the_other_machine;
#[path = "firewall.rs"]
mod firewall;
#[path = "fit_knowhow.rs"]
mod fit_knowhow;
#[path = "fitting_a_new_machine.rs"]
mod fitting_a_new_machine;
#[path = "fixing_itself_then_and_there.rs"]
mod fixing_itself_then_and_there;
#[path = "flight_recorder.rs"]
mod flight_recorder;
#[path = "flow_refs.rs"]
mod flow_refs;
#[path = "frames.rs"]
mod frames;
#[path = "freshness.rs"]
mod freshness;
#[path = "fxday.rs"]
mod fxday;
#[path = "gaze.rs"]
mod gaze;
#[path = "gguf_models.rs"]
mod gguf_models;
#[path = "goal.rs"]
mod goal;
#[path = "grade_reach.rs"]
mod grade_reach;
#[path = "group_naming_through_the_daemon.rs"]
mod group_naming_through_the_daemon;
#[path = "hand_edits_survive_updates.rs"]
mod hand_edits_survive_updates;
#[path = "handing_to_a_friend.rs"]
mod handing_to_a_friend;
#[path = "handloop.rs"]
mod handloop;
#[path = "handoff.rs"]
mod handoff;
#[path = "hands_read_the_model.rs"]
mod hands_read_the_model;
#[path = "handshape.rs"]
mod handshape;
#[path = "handtrack.rs"]
mod handtrack;
#[path = "health_modes.rs"]
mod health_modes;
#[path = "health_readings.rs"]
mod health_readings;
#[path = "hearing.rs"]
mod hearing;
#[path = "hedging_its_own_record.rs"]
mod hedging_its_own_record;
#[path = "helpers_are_governed.rs"]
mod helpers_are_governed;
#[path = "holding_a_message_until_they_appear.rs"]
mod holding_a_message_until_they_appear;
#[path = "hollowcode.rs"]
mod hollowcode;
#[path = "how_much_you_know.rs"]
mod how_much_you_know;
#[path = "hub_is_not_code.rs"]
mod hub_is_not_code;
#[path = "hub_navigation.rs"]
mod hub_navigation;
#[path = "hub_workspace_extra.rs"]
mod hub_workspace_extra;
#[path = "idle_cost.rs"]
mod idle_cost;
#[path = "improve.rs"]
mod improve;
#[path = "index_coverage.rs"]
mod index_coverage;
#[path = "infer.rs"]
mod infer;
#[path = "input_identity_ocr.rs"]
mod input_identity_ocr;
#[path = "input_perf_probe.rs"]
mod input_perf_probe;
#[path = "install.rs"]
mod install;
// Unix only: it builds tiny projects driven by shell scripts.
#[cfg(unix)]
#[path = "integrated_verification.rs"]
mod integrated_verification;
#[path = "integration.rs"]
mod integration;
#[path = "integration_wired.rs"]
mod integration_wired;
#[path = "integrations.rs"]
mod integrations;
#[path = "interrupt_mail.rs"]
mod interrupt_mail;
#[path = "ios.rs"]
mod ios;
#[path = "it_can_actually_speak.rs"]
mod it_can_actually_speak;
#[path = "it_can_do_what_it_says.rs"]
mod it_can_do_what_it_says;
#[path = "it_knows_it_crashed.rs"]
mod it_knows_it_crashed;
#[path = "it_says_which_half_it_looks_at.rs"]
mod it_says_which_half_it_looks_at;
#[path = "it_starts_itself.rs"]
mod it_starts_itself;
#[path = "kin.rs"]
mod kin;
#[path = "kin_server_boundary.rs"]
mod kin_server_boundary;
#[path = "knowing_the_map.rs"]
mod knowing_the_map;
#[path = "lanes_research.rs"]
mod lanes_research;
#[path = "language.rs"]
mod language;
#[path = "languages.rs"]
mod languages;
#[path = "leaving_a_group.rs"]
mod leaving_a_group;
#[path = "leaving_a_mode.rs"]
mod leaving_a_mode;
#[path = "ledger.rs"]
mod ledger;
#[path = "levels.rs"]
mod levels;
#[path = "levels_do_not_get_slower_the_longer_the_file.rs"]
mod levels_do_not_get_slower_the_longer_the_file;
#[path = "list_capabilities_in_full.rs"]
mod list_capabilities_in_full;
#[path = "listening_until_you_stop.rs"]
mod listening_until_you_stop;
#[path = "live.rs"]
mod live;
#[path = "machine_detection.rs"]
mod machine_detection;
#[path = "market_feed_spikes.rs"]
mod market_feed_spikes;
#[path = "measuring.rs"]
mod measuring;
#[path = "meaning_search.rs"]
mod meaning_search;
#[path = "mend.rs"]
mod mend;
#[path = "messaging_between_people.rs"]
mod messaging_between_people;
#[path = "metrics.rs"]
mod metrics;
#[path = "mind_hub.rs"]
mod mind_hub;
#[path = "model_call_over_a_real_process.rs"]
mod model_call_over_a_real_process;
#[path = "money_grading.rs"]
mod money_grading;
#[path = "multiframe.rs"]
mod multiframe;
#[path = "no_confident_nothings.rs"]
mod no_confident_nothings;
#[path = "no_quiet_nothings.rs"]
mod no_quiet_nothings;
#[path = "not_acting_on_a_guess.rs"]
mod not_acting_on_a_guess;
#[path = "nothing_is_left_in_the_process_table.rs"]
mod nothing_is_left_in_the_process_table;
#[path = "notify.rs"]
mod notify;
#[path = "nudge.rs"]
mod nudge;
#[path = "off_the_tick.rs"]
mod off_the_tick;
#[path = "offline_and_cdp.rs"]
mod offline_and_cdp;
#[path = "one_name_one_record.rs"]
mod one_name_one_record;
#[path = "one_way_to_write_a_date.rs"]
mod one_way_to_write_a_date;
#[path = "online_delegation_is_off_by_default.rs"]
mod online_delegation_is_off_by_default;
#[path = "onlyone.rs"]
mod onlyone;
#[path = "opportunity.rs"]
mod opportunity;
#[path = "opsec.rs"]
mod opsec;
#[path = "opsec_recovery_undo.rs"]
mod opsec_recovery_undo;
#[path = "overlay.rs"]
mod overlay;
#[path = "overnight.rs"]
mod overnight;
#[path = "pairing.rs"]
mod pairing;
#[path = "palette.rs"]
mod palette;
#[path = "panels.rs"]
mod panels;
#[path = "person.rs"]
mod person;
#[path = "persona_thread.rs"]
mod persona_thread;
#[path = "phase0.rs"]
mod phase0;
#[path = "phase1.rs"]
mod phase1;
#[path = "phase2.rs"]
mod phase2;
#[path = "phase3.rs"]
mod phase3;
#[path = "phone.rs"]
mod phone;
#[path = "pipeline.rs"]
mod pipeline;
#[path = "plain_change.rs"]
mod plain_change;
#[path = "plainly_publishing.rs"]
mod plainly_publishing;
#[path = "portable.rs"]
mod portable;
#[path = "profiles.rs"]
mod profiles;
#[path = "promised.rs"]
mod promised;
#[path = "prompt_reaches_actions.rs"]
mod prompt_reaches_actions;
#[path = "prose_unsub.rs"]
mod prose_unsub;
#[path = "provider_fallback.rs"]
mod provider_fallback;
#[path = "read_receipts.rs"]
mod read_receipts;
#[path = "reading_it_back.rs"]
mod reading_it_back;
#[path = "recall.rs"]
mod recall;
#[path = "recall_cutoff.rs"]
mod recall_cutoff;
#[path = "recall_wired.rs"]
mod recall_wired;
#[path = "recap_reads_the_conversation_back.rs"]
mod recap_reads_the_conversation_back;
#[path = "reclaim.rs"]
mod reclaim;
#[path = "recommend_names_the_slowest_stage.rs"]
mod recommend_names_the_slowest_stage;
#[path = "refile_corrects_a_captured_note.rs"]
mod refile_corrects_a_captured_note;
#[path = "reference.rs"]
mod reference;
#[path = "refusals.rs"]
mod refusals;
#[path = "register_character.rs"]
mod register_character;
#[path = "remembering.rs"]
mod remembering;
#[path = "reminders_and_wants.rs"]
mod reminders_and_wants;
#[path = "decision_list_remainder.rs"]
mod decision_list_remainder;
#[path = "errand_pause.rs"]
mod errand_pause;
#[path = "personal_atlas_is_its_own.rs"]
mod personal_atlas_is_its_own;
#[path = "own_server_wireguard.rs"]
mod own_server_wireguard;
#[path = "launcher_line_endings.rs"]
mod launcher_line_endings;
#[path = "easy_setup.rs"]
mod easy_setup;
#[path = "atlas_runs_with_nothing_open.rs"]
mod atlas_runs_with_nothing_open;
#[path = "the_hub_is_always_there.rs"]
mod the_hub_is_always_there;
#[path = "native_settings.rs"]
mod native_settings;
#[path = "hub_in_the_window.rs"]
mod hub_in_the_window;
#[path = "the_hub_design_is_the_locked_one.rs"]
mod the_hub_design_is_the_locked_one;
#[path = "your_clock.rs"]
mod your_clock;
#[path = "phone_app.rs"]
mod phone_app;
#[path = "phone_link.rs"]
mod phone_link;
#[path = "idle_but_on.rs"]
mod idle_but_on;
#[path = "ports_live.rs"]
mod ports_live;
#[path = "ports_round3.rs"]
mod ports_round3;
#[path = "round4.rs"]
mod round4;
#[path = "round5.rs"]
mod round5;
#[path = "round6.rs"]
mod round6;
#[path = "round7.rs"]
mod round7;
#[path = "round8.rs"]
mod round8;
#[path = "round9.rs"]
mod round9;
#[path = "round10.rs"]
mod round10;
#[path = "round11.rs"]
mod round11;
#[path = "social.rs"]
mod social;
#[path = "wants.rs"]
mod wants;
#[path = "workday_through_the_daemon.rs"]
mod workday_through_the_daemon;
#[path = "round11_on_windows.rs"]
mod round11_on_windows;
#[path = "research_browser_fallback.rs"]
mod research_browser_fallback;
#[path = "research_off_the_tick.rs"]
mod research_off_the_tick;
#[path = "research_wired.rs"]
mod research_wired;
#[path = "resources.rs"]
mod resources;
#[path = "retention_bounds.rs"]
mod retention_bounds;
#[path = "retrospective.rs"]
mod retrospective;
#[path = "returning_otherside.rs"]
mod returning_otherside;
#[path = "returning_wired.rs"]
mod returning_wired;
#[path = "reviewing_what_atlas_did_unprompted.rs"]
mod reviewing_what_atlas_did_unprompted;
#[path = "revise.rs"]
mod revise;
#[path = "rollover.rs"]
mod rollover;
#[path = "roster.rs"]
mod roster;
#[path = "safety_ledger.rs"]
mod safety_ledger;
#[path = "sandbox_lost_update.rs"]
mod sandbox_lost_update;
#[path = "self_check.rs"]
mod self_check;
#[path = "self_finishing_backlog.rs"]
mod self_finishing_backlog;
#[path = "selfaudit.rs"]
mod selfaudit;
#[path = "selfgrant.rs"]
mod selfgrant;
#[path = "selfwork.rs"]
mod selfwork;
#[path = "sensing.rs"]
mod sensing;
#[path = "server_safety.rs"]
mod server_safety;
#[path = "settings_actually_stick.rs"]
mod settings_actually_stick;
#[path = "shakedown.rs"]
mod shakedown;
#[path = "shared_task.rs"]
mod shared_task;
#[path = "shipped_config.rs"]
mod shipped_config;
#[path = "signal_listener.rs"]
mod signal_listener;
#[path = "signals.rs"]
mod signals;
#[path = "signin.rs"]
mod signin;
#[path = "speech_activity.rs"]
mod speech_activity;
#[path = "stale.rs"]
mod stale;
#[path = "stance_route.rs"]
mod stance_route;
#[path = "standdown.rs"]
mod standdown;
#[path = "stop_everything_stops_everything.rs"]
mod stop_everything_stops_everything;
#[path = "strategy.rs"]
mod strategy;
#[path = "subject_attribute.rs"]
mod subject_attribute;
#[path = "subject_system.rs"]
mod subject_system;
#[path = "subsystems.rs"]
mod subsystems;
#[path = "swings_incremental.rs"]
mod swings_incremental;
#[path = "sync.rs"]
mod sync;
#[path = "taking_it_back.rs"]
mod taking_it_back;
#[path = "telling_it_twice.rs"]
mod telling_it_twice;
#[path = "text_that_is_not_ascii.rs"]
mod text_that_is_not_ascii;
#[path = "the_budget_knows_what_it_does_not_know.rs"]
mod the_budget_knows_what_it_does_not_know;
#[path = "the_calendar_says_when_it_does_not_know.rs"]
mod the_calendar_says_when_it_does_not_know;
#[path = "the_ears_and_the_voice_actually_run.rs"]
mod the_ears_and_the_voice_actually_run;
#[path = "the_envelope.rs"]
mod the_envelope;
#[path = "the_flag_nothing_could_set.rs"]
mod the_flag_nothing_could_set;
#[path = "the_grants_gate_is_wired.rs"]
mod the_grants_gate_is_wired;
#[path = "the_guides_tell_the_truth.rs"]
mod the_guides_tell_the_truth;
#[path = "the_heartbeat_does_not_stop.rs"]
mod the_heartbeat_does_not_stop;
#[path = "the_hub_is_reachable.rs"]
mod the_hub_is_reachable;
#[path = "the_hub_works_offline.rs"]
mod the_hub_works_offline;
#[path = "the_log_has_levels.rs"]
mod the_log_has_levels;
#[path = "the_mirror_knows_it_was_never_filled.rs"]
mod the_mirror_knows_it_was_never_filled;
#[path = "the_night_actually_runs.rs"]
mod the_night_actually_runs;
#[path = "the_oldest_vault_still_opens.rs"]
mod the_oldest_vault_still_opens;
#[path = "the_printed_address_actually_opens.rs"]
mod the_printed_address_actually_opens;
#[path = "the_hub_answers_every_click.rs"]
mod the_hub_answers_every_click;
#[path = "the_project_workshop.rs"]
mod the_project_workshop;
#[path = "the_three_it_promised.rs"]
mod the_three_it_promised;
#[path = "the_trash_is_not_where_space_goes_to_die.rs"]
mod the_trash_is_not_where_space_goes_to_die;
#[path = "the_trash_survives_a_bad_read.rs"]
mod the_trash_survives_a_bad_read;
#[path = "the_wiring_pass_20sep.rs"]
mod the_wiring_pass_20sep;
#[path = "tier.rs"]
mod tier;
#[path = "times_with_other_people.rs"]
mod times_with_other_people;
#[path = "timing.rs"]
mod timing;
#[path = "timing_wired.rs"]
mod timing_wired;
#[path = "the_microphone_has_its_own_thread.rs"]
mod the_microphone_has_its_own_thread;
#[path = "speaking_off_the_loop.rs"]
mod speaking_off_the_loop;
#[path = "together.rs"]
mod together;
#[path = "tool_timeout.rs"]
mod tool_timeout;
#[path = "tools_cfg_paths.rs"]
mod tools_cfg_paths;
#[path = "tray.rs"]
mod tray;
#[path = "triage_chain.rs"]
mod triage_chain;
#[path = "tts.rs"]
mod tts;
#[path = "tune.rs"]
mod tune;
#[path = "two_atlases_one_folder.rs"]
mod two_atlases_one_folder;
#[path = "typing_is_a_conversation_too.rs"]
mod typing_is_a_conversation_too;
#[path = "untrusted.rs"]
mod untrusted;
#[path = "utf8_boundaries.rs"]
mod utf8_boundaries;
#[path = "vault_crypto.rs"]
mod vault_crypto;
#[path = "vault_walk.rs"]
mod vault_walk;
#[path = "viewing.rs"]
mod viewing;
#[path = "vision.rs"]
mod vision;
#[path = "voice_and_tools.rs"]
mod voice_and_tools;
#[path = "voice_is_the_real_door.rs"]
mod voice_is_the_real_door;
#[path = "voice_lock.rs"]
mod voice_lock;
#[path = "walk_me_through_the_steps.rs"]
mod walk_me_through_the_steps;
#[path = "wanted.rs"]
mod wanted;
#[path = "what_atlas_itself_costs.rs"]
mod what_atlas_itself_costs;
#[path = "what_is_working_right_now.rs"]
mod what_is_working_right_now;
#[path = "what_the_answer_rests_on.rs"]
mod what_the_answer_rests_on;
#[path = "what_the_month_says.rs"]
mod what_the_month_says;
#[path = "what_works_offline.rs"]
mod what_works_offline;
#[path = "what_you_asked_for_and_couldnt_have.rs"]
mod what_you_asked_for_and_couldnt_have;
#[path = "what_you_will_do_on_your_own.rs"]
mod what_you_will_do_on_your_own;
#[path = "which_ear_it_listens_with.rs"]
mod which_ear_it_listens_with;
#[path = "which_model_fits_here.rs"]
mod which_model_fits_here;
#[path = "wired_behaviour.rs"]
mod wired_behaviour;
#[path = "wiring.rs"]
mod wiring;
#[path = "words.rs"]
mod words;
#[path = "working_a_decision.rs"]
mod working_a_decision;
#[path = "workspace_view.rs"]
mod workspace_view;
#[path = "writing_only_what_changed.rs"]
mod writing_only_what_changed;
#[path = "your_own_calendar.rs"]
mod your_own_calendar;
#[path = "your_settings_reach_the_code.rs"]
mod your_settings_reach_the_code;

#[path = "settings_apply_live.rs"]
mod settings_apply_live;
#[path = "the_line_moves.rs"]
mod the_line_moves;
#[path = "seeing_real_pictures.rs"]
mod seeing_real_pictures;
#[path = "reading_pictures.rs"]
mod reading_pictures;
#[path = "asking_before_recording.rs"]
mod asking_before_recording;
#[path = "pressing_the_security_switch.rs"]
mod pressing_the_security_switch;
#[path = "call_notes.rs"]
mod call_notes;
#[path = "working_a_window_for_you.rs"]
mod working_a_window_for_you;
#[path = "the_envelope_when_asked.rs"]
mod the_envelope_when_asked;
#[path = "the_gaps_the_audit_found.rs"]
mod the_gaps_the_audit_found;
#[path = "work_goes_through_the_crew.rs"]
mod work_goes_through_the_crew;
#[path = "what_atlas_took_from_wshobson_agents.rs"]
mod what_atlas_took_from_wshobson_agents;
#[path = "a_no_comes_with_a_way_to_yes.rs"]
mod a_no_comes_with_a_way_to_yes;
#[path = "your_answers_of_the_25th.rs"]
mod your_answers_of_the_25th;
#[path = "after_a_restart.rs"]
mod after_a_restart;
#[path = "runbooks_and_what_comes_next.rs"]
mod runbooks_and_what_comes_next;
#[path = "two_factor_and_signing_in.rs"]
mod two_factor_and_signing_in;
#[path = "correcting_as_you_type.rs"]
mod correcting_as_you_type;
#[path = "acting_on_its_own.rs"]
mod acting_on_its_own;
#[path = "what_atlas_says_unasked.rs"]
mod what_atlas_says_unasked;
#[path = "acting_on_your_things.rs"]
mod acting_on_your_things;
#[path = "reading_documents.rs"]
mod reading_documents;
#[path = "keys_to_reach_atlas.rs"]
mod keys_to_reach_atlas;
#[path = "what_atlas_keeps_and_weighs.rs"]
mod what_atlas_keeps_and_weighs;
#[path = "advice_and_help.rs"]
mod advice_and_help;
#[path = "answers_like_a_person.rs"]
mod answers_like_a_person;
#[path = "asking_your_other_atlas.rs"]
mod asking_your_other_atlas;
#[path = "holding_it_until_youre_out.rs"]
mod holding_it_until_youre_out;
#[path = "tor_ships_with_atlas.rs"]
mod tor_ships_with_atlas;
#[path = "a_network_that_blocks_tor.rs"]
mod a_network_that_blocks_tor;
#[path = "updates_and_feedback_by_voice.rs"]
mod updates_and_feedback_by_voice;
#[path = "a_model_without_curl.rs"]
mod a_model_without_curl;
#[path = "adding_a_phone.rs"]
mod adding_a_phone;
#[path = "panels_are_drawn.rs"]
mod panels_are_drawn;
#[path = "the_mark_is_one_mark.rs"]
mod the_mark_is_one_mark;
#[path = "updates_and_feedback_in_the_hub.rs"]
mod updates_and_feedback_in_the_hub;
#[path = "every_button_says_what_happened.rs"]
mod every_button_says_what_happened;
#[path = "the_other_device_reads_whole_files.rs"]
mod the_other_device_reads_whole_files;
#[path = "speed_measured.rs"]
mod speed_measured;
#[path = "the_phone_build_reaches_no_desktop_module.rs"]
mod the_phone_build_reaches_no_desktop_module;
#[path = "photo_editing.rs"]
mod photo_editing;
#[path = "opportunity_hunting.rs"]
mod opportunity_hunting;
#[path = "wit_setting.rs"]
mod wit_setting;
#[path = "the_background_atlas_hears_you.rs"]
mod the_background_atlas_hears_you;
#[path = "the_overlay_covers_only_its_words.rs"]
mod the_overlay_covers_only_its_words;
#[path = "the_system_recovers_by_itself.rs"]
mod the_system_recovers_by_itself;
#[path = "one_evening_on_the_laptop.rs"]
mod one_evening_on_the_laptop;
#[path = "the_name_and_the_request_in_one_breath.rs"]
mod the_name_and_the_request_in_one_breath;
#[path = "every_screen_is_seen.rs"]
mod every_screen_is_seen;
#[path = "the_right_tools_for_the_sentence.rs"]
mod the_right_tools_for_the_sentence;
#[path = "finishing_what_it_starts.rs"]
mod finishing_what_it_starts;
#[path = "several_approvals.rs"]
mod several_approvals;
#[path = "two_brains.rs"]
mod two_brains;
#[path = "a_real_model_answers_him.rs"]
mod a_real_model_answers_him;
#[path = "a_normal_voice_is_heard.rs"]
mod a_normal_voice_is_heard;
#[path = "atlas_looks_when_you_ask.rs"]
mod atlas_looks_when_you_ask;
#[path = "meaning_picks_the_tool.rs"]
mod meaning_picks_the_tool;
#[path = "meaning_checks_the_reply.rs"]
mod meaning_checks_the_reply;
#[path = "what_gets_used_is_counted.rs"]
mod what_gets_used_is_counted;
#[path = "talking_does_not_hold_the_loop.rs"]
mod talking_does_not_hold_the_loop;
#[path = "a_quiet_tick_is_quick.rs"]
mod a_quiet_tick_is_quick;
#[path = "every_ability_answers.rs"]
mod every_ability_answers;
#[path = "pictures_are_made_here.rs"]
mod pictures_are_made_here;
#[path = "an_old_question_of_its_own_is_dropped.rs"]
mod an_old_question_of_its_own_is_dropped;
#[path = "atlas_tests_itself.rs"]
mod atlas_tests_itself;
#[path = "atlas_works_an_app.rs"]
mod atlas_works_an_app;
#[path = "keeping_track.rs"]
mod keeping_track;
#[path = "weather_answered.rs"]
mod weather_answered;
#[path = "conversation_sweep.rs"]
mod conversation_sweep;
#[path = "getting_things_done.rs"]
mod getting_things_done;
mod voices_told_apart;
#[path = "atlas_checks_itself.rs"]
mod atlas_checks_itself;
#[path = "wake_by_sound.rs"]
mod wake_by_sound;
#[path = "reports_as_files.rs"]
mod reports_as_files;
#[path = "mcp_server.rs"]
mod mcp_server;
#[path = "what_the_model_ranking_found.rs"]
mod what_the_model_ranking_found;
#[path = "answers_not_timings.rs"]
mod answers_not_timings;
#[path = "the_silence_after_you_speak.rs"]
mod the_silence_after_you_speak;
#[path = "there_when_you_sit_down.rs"]
mod there_when_you_sit_down;
#[path = "greet_once.rs"]
mod greet_once;
#[path = "the_npu_engine.rs"]
mod the_npu_engine;
#[path = "apple_first_then_atlas.rs"]
mod apple_first_then_atlas;
#[path = "reminders_ring_with_the_app_closed.rs"]
mod reminders_ring_with_the_app_closed;
#[path = "reaching_the_iphone_with_atlas_closed.rs"]
mod reaching_the_iphone_with_atlas_closed;
#[path = "apple_weather_first.rs"]
mod apple_weather_first;
