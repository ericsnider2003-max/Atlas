# Hand-off: finishing "one completion path" (item 6)

Last updated: 2026-10-21
Status: open — for the session that owns the files below

Eric's rule: "anything can be a request. If I ask Atlas to do something, it is a valid request." Every request ends visibly as **done**, **failed**, or **needs you**, and survives a restart. No request kind gets a lesser path.

## Done on branch `fix/items-2026-10-21` (files outside the other session's work)

- `job.rs`: one terminal set and one ending helper, `job::finish(store, id, stage, state, detail, provenance)`. It refuses non-terminal states and creates-and-closes a record that was never begun. `begin_as(.., internal)` marks self-work.
- `daemon/errands.rs`: every errand Eric's requests start gets a durable record and an ending (`take_crew_news` → `job::finish`). A vanished worker ends **Uncertain** ("check whether it finished before running it again"), not Failed. A stopped queued errand ends Cancelled instead of staying Running forever. Atlas's own self-work only leaves a record when it fails.
- `hubjobs.rs`: the finish time is persisted (wall clock), so a failure stays visible for 24 h after it failed instead of about 10 minutes after the last boot. Running-at-restart becomes "Interrupted: Atlas stopped while this was running; check whether it finished before running it again." It no longer says "review and retry".
- `main/carrying.rs`: remote-queue requests (`atlas remote ...`) get job records, and `atlas remote list` shows interrupted ones with time and reason.

## Still needed, in files the reliability session is editing

1. **`hublive/settings_and_outstanding.rs`**:
   - Skip `job.internal` records in every lane. Today it skips only dismissed ones for Failed and Uncertain; Succeeded ignores `dismissed`.
   - Replace the 24 h display cutoff (`RECENT_SECS`, ~148 and ~337) with "stays until dismissed" for Failed and Uncertain.
   - Give NeedsYou (AwaitingApproval) its own lane instead of Blocked after 15 min (~359-378).
   - Expose a hub-job finish-time getter so `failures()` (~384) shows when.
2. **`daemon/turn/run_command.rs` ~1036**: the chat tail hard-codes `ok = true`. Pass the real outcome through `job::finish`: Failed or Uncertain on a miss, AwaitingApproval on needs-you.
3. **`daemon/turn/spoken_steps.rs` ~322**: multi-part voice requests write no record. Call `job::begin_as` at the start and `job::finish` at the end; an abandoned step ends Uncertain.
4. **`daemon/tasks.rs`**: the `finish_task_job` early returns (~213-223) must still close the record via `job::finish`. `TaskLoop::drop` (~86-91) must end a still-Running record as Uncertain ("dropped before it ended").
5. **`daemon.rs` ~527-530**: `CrewLink.job_id` is now always set; update its comment.

## Approval contract (still open)

Chat approvals live in `Session::pending` (memory only) and survive a restart only as re-asked text in `left_waiting`. Scheduled-job approvals are spoken only when Supervised; otherwise they wait silently. Needed: one persisted NeedsYou record per pending approval, shown in the Hub's needs-you lane and re-asked (never replayed) after a restart.

## Context before asking (Eric, 21 Oct)

Eric's rule, verbatim: "Atlas should ask and think with context. Atlas shouldn't ask and be guessing. Whatever Atlas does should be thought out and use common sense or context."

In practice: resolve every request against the conversation, the active window, recent actions and habits. Clear: act and say what was understood. Ambiguous: ask ONE question that offers the candidates. No candidates: ask a specific question naming what is missing, with an example. Never a generic "Which one?" / "I didn't catch that". Anything hard to undo is still confirmed (RequireApproval, `uia::cannot_be_undone` stay).

### Built (items worktree, not committed)

- `understood.rs`: `Context`, `Resolution { Clear, Choose, Missing }`, `resolve(kind, what_for, ctx)`, `stated(action, r)`, `ask_with_options(what_for, options)`, `targets_in(text)`; `checking()` now states the reading and asks "Shall I go ahead?". Unit tests cover every branch.
- `daemon/execute.rs`: `resolution_context()` (last turns, `plat.active_window()`, last 50 journal lines and turns parsed into folder/file/app/person targets; habits = targets seen twice or more), `context_question()` (pronoun with no referent now offers candidates), `unrecognised_line()` (replaces "I didn't catch that.": says what was heard, the closest commands via the new `Parser::closest_commands`, and the open window).
- `references.rs`: the bare "Which one?" for a pronoun with no kind now names what would settle it. `daemon/turn/unwatched.rs` passes the ambiguity through `context_question`.
- `tests/asks_are_specific.rs` (in `tests/all.rs`): fails on any new string literal that is only a generic question; KNOWN is an exact count (brain.rs: 1).
- The other sites named in the brief (daemon.rs:1323, intent.rs, helping.rs:234, learning.rs:201, answering.rs:3, unwatched.rs comments, run_command.rs:476/671 and errands.rs:103) are comments describing the old failure, not live questions; nothing to replace there.

### Wiring still needed in forbidden files (owned by the reliability session)

1. `daemon/turn/run_command.rs`, before the `AskClarification` branch (~660-680): call `self.resolution_context()` and `understood::resolve(kind, what_for, &ctx)` for the intent's missing target. On `Clear`, act and prefix the reply with `understood::stated(action, &r)`. On `Choose`/`Missing`, use its `question` instead of `decision.say`. On the `ProceedAndReport` path prefix `understood::stated`. The two "I didn't catch that" comments (~476, ~671) describe behaviour the new `unrecognised_line` already replaces for `Intent::Unknown`; confirm nothing in that file still produces the old sentence.
2. `brain.rs:3299`: `Intent::Unknown(_) => "I didn't catch that.".into()` is the one live generic literal left (KNOWN in `asks_are_specific.rs`). Replace with a line built from the raw text and `Parser::closest_commands` (needs the raw text passed in) then lower KNOWN to 0.
3. `daemon/floor.rs` `Pending::BackupDest` (~186-193): before asking "Where should I put the backup", call `resolve("folder", "where the backup should go", &self.resolution_context())`. Clear: start the backup with `stated("Backing up to <dest>", &r)` (last backup destination, or an Explorer folder that is open). Choose: ask `question`. Missing: today's question is already specific.
4. `daemon/errands.rs:103`: the "which one?" answer path; the question itself is built elsewhere (`which_errand`) and already lists options. Nothing live to change; if its text is ever rebuilt, use `ask_with_options`.
