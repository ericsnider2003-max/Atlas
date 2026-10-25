# One clean Atlas: all outstanding branches integrated (2026-10-25, Claude, usage-max routed)

Branch `fix/plan-handoffs-2026-10-24` on candidate `02c25362`.

Merged:
- `feat/plans-aeg-2026-10-24` (47e7479f, plans A-G): clean.
- `feat/phone-standalone-2026-10-23` (de58dd24, includes the Android key script past c6221fca): conflicts in daemon.rs and lib.rs, both module declarations; kept both sides. The tick.rs duplicate-event fix (`!self.synclog.holds(e)`) and the Plan A check-in both kept. Merge fallout fixed (module count 508, two name collisions, regenerated CAPABILITIES.md).
- `fix/items-2026-10-21` (C: drive cleanup filing record): clean.
- Not merged: `fix/crash1-watchdog` (13 Oct, predates consolidation; owner decision pending) and `fix/kokoro-first-clause-2026-10-22` (rejected: worse speech).

Hand-offs:
1. One length rule (`persona::LENGTH_RULE`) used by every prompt, including ANSWER_AFRESH. A source scan fails on any prompt stating its own sentence count (named allowlist). Plan G budget holds (2,260 of 2,461 chars). Raised: finishing_what_it_starts 1,500→1,620 chars; average turn 900→915 tokens. Not measured: answer length on a local model.
2. Plan A check-ins: in the tick, behind may_interrupt, modes, pause, mute and quiet hours; at most every 30 min; each item once; surfaced memory persisted (7 days / 200 entries); a restart doesn't repeat.
3. Focus guard: `backends::Guarded` under the platform. Scheduled jobs, Background-lane tasks, every step of scheduled add-on flows, and (when the owner is active) phone requests, task-loop steps and restart replays are refused focus/click/type/press/pointer; the owner's own turns pass. Not guarded: launch/close/place (would break scheduled workspace boot/shutdown); a new operate job's first step (existing "just asked" exemption).
4. `open_work` stays pub(crate): needs_you reads the one work record; a test checks it agrees with Outstanding.
5. Phrase table: the training session's 5 wrong actions are all already stopped by the 22 Oct fix (re-run on both locked sets: 0 wrong actions). Cases added to the corpus. A general long-message guard was rejected: it would cost a correct command on Eric's 678 messages and gain nothing measured.

Review: usage-max reviewer. The merge and check-ins pass. Focus-guard escapes (later flow steps, phone requests, task loop) and one stray length rule were found and fixed in e751284d.

Tests: the final tree's results are stitched from two full runs plus re-runs: 7,268 passed in `all`; 3 env (camera/ffmpeg); 3 timing (passed on re-run). `errand_pause::stop_the_research...` hung once in a full run; the errand group then passed 5/5 (about 4 s each). No single clean one-pass full run on the final tree.

Installed `0821F16A...`, rollback `atlas-0.1.0-dev.previous` = `11135EC4...`. Verifier 14/14 on the second run, hash matched (first run: the known daemon_start_liveness lock-timing flake).

Not verified: anything on a real screen or phone; check-ins firing from the live tick; task-loop/replay guard end to end; local-model answer length.
