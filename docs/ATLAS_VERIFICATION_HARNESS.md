# Atlas verification harness

**Status:** partial implementation; baseline and durable-queue slice proven against the installed candidate  
**Owner requirement:** Atlas must test its real capabilities automatically. Eric should not be required to manually exercise the entire product.

## Purpose

The verification harness is an independent runner that tests one immutable Atlas candidate through its real public interfaces and records what actually happened. It is separate from Atlas's internal `selftest`, unit tests, and synthetic routing tests.

The harness must preserve the full capability set. A capability is not removed because it is hard to test; it is marked with the evidence state that the current environment can prove.

## Evidence rules

A result is only `passed` when the expected observable effect is confirmed. Reaching a handler, returning a plausible string, or completing a mock call is not enough.

Every attempt records:

- candidate version, source revision, executable hash, and test-run ID;
- capability, contract, variation, environment, and configuration;
- exact input and expected observable effects;
- observed output, state changes, side effects, and cleanup result;
- process, resource, and timing measurements where relevant;
- logs, screenshots, traces, and other artifact paths;
- status: `passed`, `failed`, `blocked_external`, `inconclusive`, or `not_run`;
- failure class, reproduction link, and regression link when applicable.

Results are append-only. A later pass does not overwrite an earlier failure. It links to it and records which variation and candidate closed or failed to close the issue.

## What counts as a real test

The runner launches the real Atlas executable or installed candidate. It uses disposable real folders, state stores, processes, and accounts or test accounts where available. It performs the action, observes the result, and verifies forbidden side effects.

Rehearsal, `SafePlatform`, handler-only tests, and source-only tests remain useful evidence, but they must never be reported as runtime feature proof.

## Variation model

Each passing contract is rerun across relevant variations, including:

- alternate user phrasing and entry points;
- cold start, warm start, and restart;
- missing dependency, provider error, network interruption, permission error, and bounded resource pressure;
- concurrent and repeated requests;
- model/provider/configuration differences;
- UI, CLI, IPC, HTTP, and voice paths where supported;
- recovery after crash or interrupted background work.

## Required outputs

Each run produces a machine-readable ledger and a human-readable report containing:

- complete pass/fail/blocked/inconclusive inventory;
- capability and variation coverage;
- newly discovered failures;
- reproduced known failures;
- fixed failures verified on the new candidate;
- regressions against earlier candidates;
- untested boundaries and why they could not run;
- build identity and environment identity.

## Initial implementation slice

Build the harness outside Atlas with these first capabilities:

1. Create an isolated run directory and immutable candidate manifest.
2. Launch and control the real Atlas process.
3. Drive one public request path and assert an observable result.
4. Capture structured logs, process exit, resource samples, and filesystem/state diffs.
5. Kill/restart Atlas during a durable operation and verify the recovery contract.
6. Persist append-only results and generate a coverage report.
7. Rerun the same contract with at least two meaningful variations.

Only after this slice produces trustworthy evidence should the harness expand to the full capability catalogue.

## Current implementation evidence

`verification/Invoke-AtlasVerification.ps1` currently provides the first executable slice. It records a candidate
manifest and append-only JSONL ledger, launches the real executable in a disposable `ATLAS_HOME`, captures stdout,
stderr, exit status, timing, hashes, and newly created state files, and asserts an observable output pattern.

The 2026-10-17 run `verification/runs/20261017T070855009Z-74c2ab4c/` used the installed candidate
`B5D6D239A7C7FB4E6F18EFD0BF2400F7BDA71D45133BC064D68C6C06295182BA` and passed:

- executable identity (`--version`);
- machine diagnosis (`doctor`);
- a harmless public `remote ask` request with a durable queue receipt;
- a separate-process `remote list` replay of that queued request.

This is real-runtime evidence for four contracts only. It does not prove the source worktree, the complete capability
catalogue, UI behavior, settings behavior, failure recovery, or the installed candidate's readiness as an operating
system. The next harness slice must add process interruption/recovery and contract inventory before broad campaign runs.

The first release build of the current worktree was then tested as run
`verification/runs/20261017T074552418Z-0199406f/`. Its candidate SHA-256 is
`187918B56458B34E18E9BAB626B23ACBAB5D85147C171D8E17B0ADD4D0EC95DC`; all four contracts passed. This proves the
current release artifact can execute the initial slice. It has not been installed over the owner's existing Atlas
installation.

The candidate was installed at `C:\Users\erics\AppData\Local\Atlas\atlas.exe` after preserving the previous binary as
`atlas.exe.before-candidate-20261017T074730Z`. The installed hash matched the candidate exactly, and run
`verification/runs/20261017T074744191Z-ec176cb4/` passed all four initial contracts against the installed path.

The backup reliability pass added bounded retries for Windows `ERROR_LOCK_VIOLATION` during staged copy and deferred
flush. The backup-focused integration slice remained green (29 passed). The resulting release candidate has SHA-256
`FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`; it is installed at the same path after saving
the prior installed binary as `atlas.exe.before-candidate-20261017T084611Z`. Installed-path run
`verification/runs/20261017T084811763Z-1682e9bf/` passed all four initial contracts.

The installed profile was exercised through Atlas Talk with `back up`. The first live requests correctly refused while
the call-recorder lease was held, including Windows `ERROR_LOCK_VIOLATION`; after restarting Atlas to clear the stale
in-process recording object, the next request published backup `state-1792227445` at
`C:\\Users\\erics\\AppData\\Local\\Atlas\\data\\backups\\state-1792227445`. Its v2 manifest contains 128 files
across 12 directories and 3 owned roots. This is the first real-profile backup proof for the candidate; live restore
has not been run destructively.

The latest installed-path run is `verification/runs/20261017T224847069Z-d381b129/` against the same candidate hash. It passed
14/14 substantive real-process contracts: identity, resource diagnosis requiring observed memory and disk lines, durable enqueue, authenticated Hub health/page/live surfaces,
cold daemon liveness, forced-stop daemon restart liveness, post-daemon queue replay, restart replay, the approval-gated
`remote start 1 --yes` transition, completion, completed-state replay, and a missing-request failure. The Hub checks assert the
actual browser handoff contract: HTTP 200, an Atlas session cookie, and a clean-path self-navigation target; Atlas intentionally
does not use a 303 here because cross-site camera opens can withhold a Strict cookie across redirects. The queue and Hub contracts
use a fresh disposable `ATLAS_HOME`; they do not touch the owner's profile or claim that the full capability catalogue is
runtime-proven.

## Boundaries

Physical devices, personal accounts, owner-selected folders, and providers that cannot be safely provisioned remain explicit external boundaries. The harness must report them as blocked or unverified, never silently promote synthetic or rehearsed evidence to runtime proof.
