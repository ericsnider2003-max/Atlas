# Item 6 wiring, backup status, items merge: 2026-10-22 (Claude, usage-max routed)

Branch `fix/item6-wiring-2026-10-21` = candidate `2d49d94b` + merged `fix/items-2026-10-21` (`a80602b8`, no conflicts) + the hand-off in `docs/HANDOFF_ONE_COMPLETION_PATH_2026-10-21.md`.

| Change | Commit | Proof |
|---|---|---|
| Context first in chat: a request with no target is read against the conversation, open window and recent actions. One reading acts and says what it understood; several readings get one question offering them; none gets a specific question naming what's missing. Messages, anything outward, and anything after reading outside content (SEC2) are always asked first | 4c39a237, 15346b66 | `fill_from_context` test (one journal person: asked "sending a message to Sam ... Shall I go ahead?", nothing sent); `asks_are_specific` KNOWN = 0 |
| One completion path: chat replies, multi-part voice requests and task loops end done / failed / uncertain / needs-you through `job::finish`; refused requests end Cancelled; parked ones wait on you | 4c39a237, 15346b66 | job, tasks, spoken_steps tests |
| Hub: internal jobs hidden; failures stay until dismissed and show when; a separate waiting-on-you lane; dropping a question cancels it; the record cap never evicts undismissed failures | 4c39a237, 15346b66 | lanes test; drop-does-not-reappear test; cap test |
| Saved approvals: one persisted record per pending approval (chat and scheduled jobs, supervised or not); after a restart it's re-asked, never replayed | 4c39a237 | approval-survives-restart test |
| "is the backup of src finished?" answered from the job records with no model (running / done when and where / failed why / none). "how's it going" stays small talk | 2fb74a2f, ba5f7422 | `is_the_backup_finished_is_answered_from_the_job_with_no_model`; `talking_freely`, `getting_things_done` |
| The typed prompt now prints "done" announcements while waiting (it used to block on input). Masked secret reads are gated so a secret never enters the command channel | 2fb74a2f, 15346b66 | gate unit tests; not exercised in a real console |

Review: usage-max reviewer found 5 defects (guessed recipient messaged without asking; asks recorded as Succeeded; cap evicting failures; dropped question reappearing; stdin thread able to take a masked secret), all fixed in 15346b66.

Full suite (before ba5f7422): 9,290 passed, 8 failed. 3 need system ffmpeg (absent here); `crew::ask_to_stop_on_queued_work` timing under load (3/3 alone); 4 regressions from the status question, fixed in ba5f7422 and re-run green by module.

Installed `A5656D40...`, rollback `atlas-0.1.0-dev.previous` = `4546DC3D...`. Verifier 14/14 (`evidence/installed-verifier-results.jsonl`). An earlier run during the full suite failed `daemon_start_liveness` once ("already running": the force-killed Hub-check daemon still held the lock under load). Two reruns were 14/14.

## Kokoro speed (idle machine, installed build, `atlas kokoro-check`, af_heart, 4 threads, Core Ultra 7 256V)
- Warm: 3.61 s of speech in 2.62-2.67 s (0.72-0.74x real time). First run after load: 5.36 s (1.48x). Load 1.2-1.8 s.
- The 21 Oct figure (1.7-2.05x) was taken while Rust builds ran.
- Atlas already uses the int8 model, keeps it resident for 5 min, and synthesises one sentence ahead of playback, so once warm it keeps up. What you notice is the wait for the first sentence.
