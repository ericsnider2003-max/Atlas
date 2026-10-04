# SOURCE OF TRUTH — read this before trusting anything in this repo

**Last updated:** 2026-10-03  ·  **Status:** live pointer

## The current code is `main`

As of **2026-10-03**, `main` is the combined, current tree:

- `main` = **`2ee8adf`**, tagged **`atlas-2026-10-03-integration`**.
- It is the merge of the most-built-out line (`push-1001f`) with `chat-c-3`, so
  nothing from either parallel line is stranded.
- **Not pushed.** `origin/main` is still an unrelated orphan; publishing needs a
  `--force`, on the owner's say-so.

Before this date, `main` was behind and the newest work was on chat branches.
That is no longer true. The plan and the full trace are in the control system
(below), file `MERGE_PLAN.md`.

## Is the tree green?

`./verify.sh` reports RED — but it reported RED on the pre-merge `push-1001f`
too, and **more** of it. The remaining failures are machine-specific (tests that
assert POSIX paths fail on Windows; a few suites are flaky under `--quick`;
19 pre-existing warnings). The merge added no regressions. A fully green run
needs Linux/CI, or those tests made platform-aware. See `MERGE_PLAN.md` §7.

## This repository is the Atlas code

It is the only place with real history and branches. Folder copies elsewhere on
this machine are snapshots and cannot be reconciled with it.

- Remote: `https://github.com/ericsnider2003-max/Atlas.git`
- A second remote points at a **separate** Atlas, kept apart from this one
  (the Atlas handed to friends carries none of it: `personal_atlas_is_its_own`).

## The control system

The project's control system (intent, current-status, session log, gaps, archive
rules, automation) lives at:

```
C:\Users\erics\Atlas\START_HERE.md
```

Read that first. The one file allowed to name the current code is
`C:\Users\erics\Atlas\00_SOURCE_OF_TRUTH\CURRENT_CODE_STATUS.md`.

## For anyone changing code

1. Read `C:\Users\erics\Atlas\01_DESIGN_INTENT\` — what Atlas is for. It is
   written in the owner's words and is the brief.
2. Check `CURRENT_CODE_STATUS.md` for which branch is current.
3. If you change which branch is current, update that file **in the same
   session** and add a session log entry.
4. Do not leave two things claiming to be current. Archive the loser.
