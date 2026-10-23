# Phrase-table misfires, hard-to-undo confirms, Kokoro first audio: 2026-10-22/23 (Claude, usage-max routed)

Branch `fix/phrases-and-confirms-2026-10-22` (bdd14239..df448a02), fast-forwarded onto candidate `a0428325`. Every change passed a usage-max reviewer. Four review rounds found 10 defects, all fixed.

## Phrase table (misfires from Eric's real messages; training/zero-cost reports)
| Message (truncated) | Was | Now | Root cause |
|---|---|---|---|
| "Remove the older version from drop box and change drop box back to online only…" | ChangeGroup | Unknown | group name took the rest of the paragraph; now ≤ 4 words, no sentence punctuation, not ending in folder/drive/desktop/downloads/bin/trash |
| Long complaint containing "memory … full" | MachineHealth | Unknown | `rescue` matched keywords anywhere; now ≤ 24 words |
| "Fix the issues" | WorkOnYourself | Unknown | bare "fix the issues/problems" listed as self-repair; now needs "yourself"/"you found" |
| "Are you working the Atlas direction and recovery charter?" | SelfCheck | Unknown | prefix match ignored the rest; short tail only |
| "Ok so test everything you can … or create a complete handoff…" | SelfTest | Unknown | prefix match; ≤ 5-word remainder, no or/if/and/then |
Still right: "Ok please continue" → Resume; "Research github repos…" → Research; "add Sam to Reading List" → ChangeGroup. Tests: `tests/free_form_corpus.rs`.

## Hard to undo always asks first (Eric, 22 Oct)
- `RequireApproval` in `classify`, and on `policy::is_hard_to_undo`, which saved "always allow" history can't relax: Message, MoveFile, TypeCode (2FA), Updates install, picture-maker and phone-model downloads, Feedback replies, SchedulePost.
- Already covered: CloseApp, SignIn, CreateAccount, Pair, WorkOnYourself. Own-question handlers: TwoFactor setting, Feedback send, Updates undo, PressButton/Operate, MoveBigFiles, TidyDesktop, PcTune, EditMedia (code-read).
- Bypasses closed: undo of "opened X" closed the app unasked; scheduled/plan routes ran SchedulePost (publishing an unread draft); bare `classify` callers (splitter, add-on step check, voice-id) saw Message/MoveFile as unasked.
- Posting question now names the draft and time. "not now" / "no" / "later" keep it a draft. A "what time?" question can't outlive the next turn (test proven to fail with the fix disabled).
- Nothing to do → no confirm: "install the update" with none waiting, "type the code" with nowhere to type it.
- Tests: `tests/consent_routes.rs` (every hard intent asks on classify / classify_with_policy / gate; history can't learn it away; multi-part plans stop at each), plus `integration_wired`, `acting_on_your_things`, `two_factor_and_signing_in`, `updates_and_feedback_by_voice`.
- "is it done?" right after a job you started is answered from the job again; it regressed on 22 Oct (ba5f7422).

## Checks
- Full `--all-targets` (before df448a02): 9,306 passed, 7 failed. 3 env (camera/ffmpeg absent); 3 real, fixed in df448a02; `the_floor_is_quick_and_small` = load (11/11 alone).
- After df448a02: all 39 standalone targets once (521 passed, 1 = that timing test); `the_floor_without_a_model` 20/20 x3; `what_a_stranger_gets` 11/11; targeted `--test all` 194/194. No full run after df448a02.
- Installed `1A950B7A...`, rollback `atlas-0.1.0-dev.previous` = `A5656D40...`. Verifier 14/14. Real profile start/stop: Hub 12.2 s, min free 4.4 GB, clean stop 2.1 s, nothing left running.
- Not verified: anything on a real desktop (real send/close/install); close-fails branch; TidyDesktop/EditMedia scheduled routes only skimmed.

## Kokoro first audio: NOT shipped (branch `fix/kokoro-first-clause-2026-10-22`, 8a11d6cc, unmerged)
- First-clause split: first audio about 1.5 s (6-word clause), and the remainder created a mid-sentence gap of about 1.1 s.
- Adaptive chunking: first audio 1.4 s at best, gaps 0.7-2.0 s. Worse than today.
- Root cause found: on this laptop only the first one or two synth calls in a process are fast. Back-to-back calls take 2.4-3 s each regardless of length (4 identical 1.02 s chunks: 1.0, 1.3, 2.6, 2.5 s). Smaller chunks can't fix a per-call cost. Measured with another session's training job holding the CPU at about 32%; not separated from power state or thread contention.
- Next: profile consecutive `Kokoro::synth` calls (num_threads 1/2/4, power plan, onnxruntime spinning) on a quiet machine before any more chunking.
