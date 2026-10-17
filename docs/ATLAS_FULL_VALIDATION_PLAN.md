# Atlas full validation plan

**Purpose:** validate the Atlas that Eric asked Claude and ChatGPT to build, across code, capabilities, settings, UI, devices, integrations, failure, recovery, and variations.

## Governing rule

The full validation campaign does not begin until the candidate is singular, buildable, installable, observable, and testable by an independent runner.

The campaign must test the complete capability set. Difficult capabilities are not removed from scope. They receive a real result, an external-blocker result, or an explicit missing-implementation result.

No source-only test, mock-only test, rehearsal, handler reachability check, or plausible response may be reported as a working feature.

## Evidence states

Every attempt is retained in the append-only verification ledger:

- passed — the real expected behavior and observable effect occurred;
- failed — the real behavior did not meet the contract;
- blocked_external — the test could not run because a named device, provider, credential, model, or dependency was unavailable;
- inconclusive — the run produced insufficient evidence or conflicting observations;
- not_run — the contract exists but was not scheduled or could not be reached;
- rehearsed_only — safe rehearsal evidence exists, but no runtime claim is made.

A later pass never deletes or overwrites an earlier failure.

## Candidate preflight gates

These must be fixed or explicitly recorded before the acceptance campaign:

1. Reconcile the local changes into one candidate source revision.
2. Remove candidate ambiguity between worktrees, branches, bundles, GitHub, and the installed executable.
3. Build the candidate with a recorded manifest, configuration, version, revision, and SHA-256.
4. Install exactly that candidate in a disposable test profile and on the target device when a device run is required.
5. Reconcile generated capability counts, source states, tests, and documentation.
6. Provide an independent runner outside Atlas.
7. Provide stable disposable fixtures for files, projects, accounts, providers, models, network, and device state.
8. Provide process control, UI control, state snapshots, filesystem/database diffs, logs, screenshots, timing, CPU, memory, and cleanup verification.
9. Provide failure injection for process kill, restart, timeout, dependency loss, network loss, permission failure, corrupted state, disk pressure, and provider failure.
10. Ensure every test produces a ledger result even when Atlas crashes or the harness itself fails.

## Test dimensions

Every applicable contract runs through the following dimensions.

### Code and build

- formatting, compiler, clippy, dependency locks, generated files, feature flags, packaging, signing, and installer output;
- unit, integration, property, fuzz, mutation, and regression checks;
- default, lean, desktop, phone, and full-feature build boundaries;
- clean checkout and upgrade-from-existing-state builds;
- source-to-catalogue and catalogue-to-test coverage.

### Capability behavior

Every capability catalogue row is mapped to a contract with:

- user intent and alternate wording;
- every supported public entry point;
- expected visible answer or action;
- expected durable state;
- expected external effect;
- forbidden side effects;
- approval requirements;
- timeout and progress behavior;
- failure and recovery behavior;
- cleanup requirements.

### Settings

Every setting is tested:

- default value;
- explicitly on;
- explicitly off;
- invalid value;
- missing value;
- changed before startup;
- changed while Atlas is running;
- changed through each supported UI or command path;
- persistence across restart;
- upgrade migration;
- effect on the capability it controls;
- no-effect detection for settings that are accidentally unwired.

Settings must be tested in combinations where they interact, not only one at a time.

### UI and interaction

Every screen, page, panel, dialog, button, link, menu, form, keyboard path, voice path, loading state, empty state, success state, error state, blocked state, confirmation state, and recovery path is exercised.

The UI campaign verifies:

- the intended control is reachable;
- the control changes the correct state;
- the state is visible and truthful;
- navigation and back paths work;
- refresh and restart preserve the correct state;
- accessibility names, focus order, keyboard operation, contrast, scaling, and screen-reader exposure;
- no stale, duplicate, invisible, or misleading controls remain;
- the UI remains responsive while background work runs.

### State and lifecycle

Each contract runs from:

- first launch;
- fresh profile;
- configured profile;
- warm running state;
- idle state;
- active background work;
- paused work;
- failed work;
- interrupted work;
- process crash;
- machine restart;
- update and rollback;
- restored backup;
- partially corrupted state.

### Environment and dependencies

Where applicable, test:

- offline;
- online;
- network interruption;
- slow network;
- provider unavailable;
- provider returns malformed or partial data;
- model available, missing, incompatible, and out of memory;
- speech engine available and missing;
- ffmpeg/tools available and missing;
- permission granted and denied;
- real folder, empty folder, large folder, locked file, changed file, and deleted file;
- one display and multiple displays;
- normal and constrained memory/CPU/disk conditions.

### Safety and security

Every action that changes state, writes files, sends data, opens an application, uses an account, or leaves the device is checked for:

- correct approval boundary;
- correct identity and scope;
- correct redaction;
- no secret leakage in logs, prompts, screenshots, reports, or models;
- safe refusal;
- duplicate-request handling;
- undo or recovery;
- honest result when the action fails halfway through.

### Variations and regression

Every passing contract is rerun with relevant variations:

- alternate phrasing;
- alternate ordering;
- repeated request;
- conflicting request;
- ambiguous request;
- no-op request;
- long input;
- malformed input;
- different model/provider;
- different setting combinations;
- different starting state;
- concurrency;
- restart at each durable boundary;
- failure injected at each external boundary.

## Completion standard

Atlas is not accepted because the campaign has a high pass percentage. Acceptance requires:

- every capability has a current evidence state;
- every failure has a durable record and owner-visible explanation;
- every blocked item names the exact missing external prerequisite;
- every pass is linked to a candidate hash and variation;
- regressions are detected against earlier candidates;
- settings and UI coverage are reported separately from code coverage;
- no test result depends on an unrecorded manual action;
- the installed candidate matches the tested candidate.

## What ChatGPT will produce

Each campaign produces:

- candidate manifest;
- capability contract inventory;
- settings matrix;
- UI surface inventory;
- variation plan;
- append-only result ledger;
- failure and regression index;
- evidence artifact directory;
- resource/performance report;
- external-blocker report;
- final matrix of what works, fails, is blocked, is rehearsed only, or is not implemented.
