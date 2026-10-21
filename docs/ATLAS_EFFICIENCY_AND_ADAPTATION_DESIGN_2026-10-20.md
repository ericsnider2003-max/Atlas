# Atlas efficiency and device adaptation: design

Last updated: 2026-10-20 · Updated by: Claude (research session) · Status: research and design — nothing built, nothing downloaded

Eric, 20 Oct: "Realistically Atlas needs to be made as efficient as possible in every way, processing, memory, capabilities, learning. It has to be able to work, adapt, and respond efficiently, effectively, and preemptively in every way while being usable on any device."

Companion docs. This doc does not repeat their detail.

| Doc | What it holds |
|---|---|
| `docs/OFFLINE_ASSISTANT_DESIGN_2026-10-20.md` | Model tiers, voice loop, the 2 GB floor rule, the component sheet |
| `docs/OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md` | Options for a model Atlas owns |
| `docs/PLATFORM.md` | Which devices and platforms, gates, cost, time |

How numbers are tagged in this doc:

| Tag | Meaning |
|---|---|
| measured | Measured today on this laptop |
| recorded | Taken from an Atlas file or note |
| sourced | Has a URL or a [fact:id] |
| estimate | A guess with a stated basis |
| proposed | A target or rule that is not built yet |

## 1. Summary

The six biggest changes, in plain words.

1. Fix detection. Atlas reads RAM, disk and power but reports video memory as 0 and the NPU as absent. The bigger models, vision and the NPU note therefore never switch on (recorded).
2. Pick a device tier by itself (T0 to T3) and re-plan when something changes, not only once an hour. Each tier has a memory budget.
3. Stop waking every 2 seconds when idle. Wake on events. Run background work at low priority (EcoQoS).
4. Load heavy parts only when needed and unload them in a fixed order under memory pressure. Say so out loud. Never hide a feature.
5. Put a cap on storage. The install folder is about 8.5 GB (measured), of which 2.6 GB is 50 old copies of atlas.exe. Nothing is deleted without Eric's say-so.
6. Act ahead of Eric, safely. Pre-warm voice, the model and context. Pre-work only reads or computes. Anything with an outside effect still goes through the existing gate.

Everything below E0 changes code. The feature freeze applies. Eric decides (section 11).

## 2. What Atlas does today

All of this section is recorded from the code unless tagged measured.

### 2.1 Detection

| What | Where | Recorded fact |
|---|---|---|
| RAM, disk, power | `health.rs:337-343` | Reads RAM with `GlobalMemoryStatusEx` (`health.rs:432-443`), disk, and power with `GetSystemPowerStatus` (`health.rs:352-371`) |
| CPU cores | `fit.rs:476-495` | Adds core count |
| Video memory | `fit.rs:489` | `vram_mb` is always 0 |
| NPU | `fit.rs:490` | `has_npu` is always false |
| NPU helper | `npu.rs:113-120` | `has_intel_npu()` exists but is not used here |
| NVIDIA | `wants.rs:78` | `has_nvidia` is hardcoded false |
| Re-measure | `daemon/tick.rs:1028-1047` | Once an hour |
| Budget | `fit.rs:98-102` | 55% of free memory, capped at total divided by 3 |

Result of the gaps: the larger tiers (`fit.rs:198`), vision (`fit.rs:229`) and the NPU note (`fit.rs:283`) never fire.

### 2.2 Processing

| What | Where | Recorded fact |
|---|---|---|
| Main loop nap | `daemon/running.rs:1026,1039` | Every nap is capped at 2 s |
| Idle backoff | `perf.rs:20-23` | 60 s idle backoff and a battery x3 multiplier, applied before the 2 s cap |
| Net effect | derived | Atlas wakes at least every 2 s even when idle or on battery. Recorded from code; confirm by measuring (E0) |
| Priority classes | whole code base | No EcoQoS and no idle priority anywhere |
| Only priority change | `voicefirst.rs:44-60` | Model server drops to BELOW_NORMAL during speech |
| CPU warning | `cpuuse.rs:17-19` | Warns above 10% of one core, only logs |
| Child processes | `childjob.rs` | Tied to a job object for cleanup, not for limits |

### 2.3 Measured today

Disposable profile, this laptop, 60 s.

| Item | Value | Tag |
|---|---|---|
| Atlas core, no model loaded | about 70 MB (32 MB daemon plus a 37 MB helper) | measured |
| Idle CPU | about 0.05 of one core | measured |
| Startup time | none found in any file or note | recorded: none |

### 2.4 Memory

| Part | Behaviour | Where |
|---|---|---|
| Kokoro voice | Cached forever | `kokoro.rs:625-646` |
| Parakeet speech recognition | Spawns per utterance. `warm()` is a no-op | `parakeet.rs:61` |
| Meaning encoder | Resident child process | `meaningroute.rs:11` |
| Main model | Keep-warm 1,800 s. Resident up to 7 days when it fits | `lifecycle.rs:33-59` |
| Memory pressure | Only produces speech at 90% and 96% RAM. Nothing unloads | `health.rs:71-72,113-127` |

### 2.5 Storage

Install folder `C:\Users\erics\AppData\Local\Atlas`, about 8.5 GB in total (measured today).

| Part | Size | Tag |
|---|---|---|
| 50 old copies of atlas.exe | 2.6 GB | measured |
| data/ | 1.84 GB | measured |
| tools/ | about 3.6 GB | measured |
| models/ | 474 MB | measured |

data/ in detail:

| Folder | Size | Tag |
|---|---|---|
| tmp | 778 MB | measured |
| backups | 732 MB | measured |
| state | 162 MB | measured |
| calls | 100 MB | measured |
| webview | 74 MB | measured |

tools/ in detail:

| Tool | Size | Tag |
|---|---|---|
| rust | 1,388 MB | measured |
| llvm | 724 MB | measured |
| go | 268 MB | measured |
| ffmpeg | 201 MB | measured |
| npu | 198 MB | measured |
| node | 107 MB | measured |
| llama | 98 MB | measured |
| sd | 93 MB | measured |
| uv-cache | 76 MB | measured |
| pyenv | 73 MB | measured |
| sherpa | 61 MB | measured |
| other smaller tool folders | about 0.3 GB | estimate: measured total of about 3.6 GB minus the itemised 3.29 GB |

Retention defaults (`retention.rs:58-95`, recorded):

| Rule | Default |
|---|---|
| Budget for Atlas-generated data | 500 MB |
| Scratch | 10 minutes |
| Captures | 24 hours |
| Notes | 365 days |
| Trash and backups | Excluded from the budget |

Reconcile before E3: the proposed 2 GB data/ cap on T2 and the 500 MB retention default are different numbers. data/ is already 1.84 GB (measured). Eric picks the one rule.
| Backups kept | 7 (`safety.rs:155-169`) |

The 778 MB in data/tmp suggests scratch cleanup is not covering data/tmp. This is an estimate; check it.

### 2.6 Learning

| What | Limit | Where |
|---|---|---|
| Facts budget | 8 MiB | `facts.rs:32` |
| Consolidation | Up to 10,000 claims | `consolidate.rs:487-503` |
| Model-call trace | Compacts at 5,000 | `trace.rs:113,434` |
| Stale-item forgetting | `learned.rs:116` `forget_stale` has no production caller | recorded |
| Total growth | No single cap on data/ | recorded |

### 2.7 Acting ahead

| What | Recorded fact |
|---|---|
| `anticipate.rs`, `proactive.rs` | Offer but never act (`proactive.rs:9-10`) |
| Bandit | Learns from declines (`proactive.rs:270`) |
| Overnight backlog work | Gated by battery (`daemon/tick.rs:3440-3530`) |
| Autonomy | `Autonomy::{Supervised, Unattended}` (`daemon.rs:96-104`) |
| Consent | `policy.rs:76` `needs_consent`; `policy.rs:451` `gate` |
| Earned trust | `earned.rs:392-396` `may_act_alone` |
| Scheduler | `park_for_approval` (`scheduler.rs:216`) |
| Working set | `workingset.rs` pack is CLI-only, not pre-warming |

## 3. Device tiers

Proposed. Atlas detects the device and picks the tier by itself and logs why. It re-plans on events, not just hourly. Events:

- AC unplugged or plugged in.
- Memory pressure.
- Total or available RAM changes by 1 GB or more.

| Tier | Device | Model | Voice | Atlas active | Atlas idle |
|---|---|---|---|---|---|
| T0 (provisional until Q7 is answered) | Phone, or any device with 4 GB RAM or less | 0.6B on the device (Qwen3 0.6B, about 0.64 GB, recorded in `phonemodel.rs`), or the laptop's Atlas over the private network | The phone's own speech services or a light set | 1.2 GB or less for the 0.6B; phones with 8 GB+ may use the 1.7B (about 1.8 GB recorded) under the floor rule | 100 MB or less (proposed; Atlas core measured about 70 MB on Windows) |
| T1 | Low laptop, 8 GB, no usable GPU | 1.7B Q4 on CPU, 4 threads | Light set | 2.5 GB or less | 0.2 GB or less |
| T2 | Eric's laptop, 16 GB shared iGPU | As in the OFFLINE doc (4B or 1.7B by available RAM; 9B swap at 9.5 GB available) | Full set when RAM allows, otherwise Light set (OFFLINE doc) | 4.5 GB or less (see note 1) | 0.3 GB or less |
| T3 | Strong desktop, 32 GB+ or a GPU with 8 GB+ VRAM | 8-9B on request by default; kept warm only when available system RAM stays at 2 GB or more after loading (floor rule) and VRAM holds it; on a discrete GPU the model's weights sit in VRAM, so the 9.5 GB system-RAM gate becomes a VRAM-fit check plus the floor rule. The 4B fast tier and 8-9B can coexist only on T3, with enough VRAM (proposed). Vision on. | Full set | 10 GB or less system RAM (VRAM separate) | 0.5 GB or less |

All figures in the table are proposed.

Note 1. The 9B deep model (about 7.4 GB, only with 9.5 GB available) is the one exception to the T2 active cap. Eric may choose "go ahead anyway", which breaks the floor. The floor-violation metric (row 55) counts only violations Atlas causes without Eric's choice.

Every tier also obeys the 2 GB floor rule in the OFFLINE doc.

Signals to read:

| Signal | Source today | Change needed |
|---|---|---|
| Total and available RAM | `health.rs:432-443` | None |
| VRAM | Always 0 (`fit.rs:489`) | Fix. For example read the DXGI adapter description, or use llama.cpp's device listing |
| GPU kind | `wants.rs:78` hardcoded false | Fix with the same adapter read |
| NPU | `fit.rs:490` always false | Wire `npu::has_intel_npu` |
| Cores | `fit.rs:476-495` | None |
| Battery, AC, power mode | `health.rs:352-371` | Add power mode; raise an event on change |

A tier choice is logged with its reasons, so Eric can read why Atlas chose it.

## 4. Processing

Proposed.

| Change | Detail |
|---|---|
| Wake on events | When idle, wait for an event (input, timer, message) instead of a fixed 2 s cap |
| EcoQoS | Background lanes run with EcoQoS so Windows keeps them on efficient cores |
| Normal priority | Only during a live turn |
| Core limit | Atlas work uses at most half the cores |
| Staged startup | See below |

Staged startup:

| Stage | Target | Tag |
|---|---|---|
| Ready to type | 2 s or less | proposed |
| Hear and answer from tier 0 | 5 s or less | proposed |
| Model load | Later, never blocks the first two | proposed |

Targets:

| Metric | Today | Target |
|---|---|---|
| Idle CPU | about 0.05 of one core (measured) | 0.02 of one core or less (proposed) |
| Idle wakeups on battery | not measured | 1 per 10 s or fewer (proposed) |

## 5. Memory

Proposed.

Load on demand for every heavy part:

| Part | Today | Proposed |
|---|---|---|
| Kokoro | Cached forever | Unloads after idle |
| ASR (Parakeet) | Spawns per utterance | Resident only during a voice session |
| Meaning encoder | Resident child | Started on demand |
| Main model | Warm 1,800 s | Warm per tier budget and floor rule |

Memory-pressure handler. When available RAM nears the floor, unload in this fixed order and say so:

| Step | Action |
|---|---|
| 1 | Unload the deep model |
| 2 | Swap the main model for a smaller one |
| 3 | Move voice to the light set |

Rules:

- Atlas says what it unloaded and why, in plain words.
- No feature is hidden or switched off. The smaller option still answers.
- Budgets per tier are the "Atlas active" and "Atlas idle" columns in section 3.
- Reloading happens when pressure clears, using the same tier plan.

## 6. Storage

Proposed. Deletions, cap enforcement and backup conversion are all report-then-ask. Atlas lists what would go, and Eric approves. Nothing is deleted by this plan.

| Change | Effect | Tag |
|---|---|---|
| Installer keeps the last 2 builds instead of 50 | Would free about 2.5 GB | estimate |
| One enforced cap on all of data/, including tmp (2 GB on T2) | Stops open-ended growth | proposed |
| Backups become incremental or deduplicated | 732 MB today should shrink | proposed |
| Coding toolchains and models: no automatic downloads. Atlas offers the download with its size when a coding feature is first used, and Eric approves | About 2.5 GB not shipped by default | estimate |
| Eric can remove a toolchain | Atlas offers the download again, with its size, on next use, and Eric approves | proposed |
| Target install size | Under 1.5 GB excluding models and toolchains | proposed |

The 2.5 GB toolchain figure is rust 1,388 MB plus llvm 724 MB plus go 268 MB plus node 107 MB, about 2.5 GB (measured sizes, summed here).

Questions for Eric before any removal are in section 12.

## 7. Learning without bloat

Proposed.

Every learned item gets a budget and an eviction rule. All forgetting is report-then-ask: Atlas lists what would go, and Eric approves. The 2 GB data cap and the 500 MB retention default need reconciling (section 2.5); data/ is already 1.84 GB (measured).

| Item | Rule | Tag |
|---|---|---|
| Stale learned items | Wire `learned::forget_stale` to a production caller that lists candidates and asks first | proposed |
| Examples for future adapters | At most 20,000 examples, about 50 MB | proposed; size is an estimate |
| Adapters | About 50 MB each at most, one per section | estimate |
| Facts | Keep the 8 MiB budget (`facts.rs:32`) | recorded |
| Claims | Keep the 10,000 cap (`consolidate.rs:487-503`) | recorded |
| Trace | Keep the 5,000 compaction (`trace.rs:113,434`) | recorded |

Learning is judged by the replay test, not by size. The test: a correction Eric made is applied the next time. Target: 95% or more. Source: S3 of the 18 Oct plan (`00_SOURCE_OF_TRUTH/deferred-atlas-plans/ATLAS_QUALITY_SPEED_AND_LOCAL_MODELS_PLAN_2026-10-18.md`).

## 8. Acting ahead of the user

Proposed.

Pre-actions:

| Trigger | Pre-action |
|---|---|
| Eric returns (presence) or says the wake word | Pre-warm voice |
| Calendar shows a work block and the floor rule holds | Pre-load the main model |
| Idle, under EcoQoS | Prefetch context: today's brief, calendar, recent files, the cached system prompt |
| Likely question, for example the morning brief | Draft the answer before it is asked for |

Guardrails:

| Guardrail | Detail |
|---|---|
| Read or compute only | Pre-work never sends, posts, deletes, buys, trades or changes settings |
| Outside effects | Still go through `policy::gate` and Autonomy |
| Battery | No pre-warming on battery below 30% (proposed) |
| Floor rule | No pre-warming that would break the 2 GB floor |
| Log | Every pre-action is logged with its reason |
| Local only | No network or online-model calls and no cost while preparing ahead; pre-work uses only local models and local data |
| Hit rate | Will be measured. If pre-warms are used less than 30% of the time (proposed), Atlas backs off by itself |
| Learning | The proactive bandit keeps learning from declines |

How this fits what exists:

- `anticipate.rs` and `proactive.rs` today only offer. This design adds pre-work, not unasked actions.
- `workingset.rs` already builds a pack, but only by command. Pre-warming would call it from the idle lane.
- The overnight backlog gate on battery (`daemon/tick.rs:3440-3530`) stays.

## 9. Any device

Summary of `docs/PLATFORM.md`.

- Windows desktop is the beachhead and a must.
- The web hub, reached from a phone over the private network, is a must. It is the cheapest any-device path.
- Linux desktop, macOS desktop, the Android companion and iOS are all "next", each with an observable trigger.
- Public App Store and Play listing is blocked for the finance features. Mobile v1 is private distribution only.
- Stack: keep the Rust core with thin native shells. Tauri 2 is the runner-up. Flutter, Electron and a pure PWA are rejected.
- Phone model: Qwen3 1.7B Q8 (about 1.8 GB) on phones with 8 GB or more, otherwise 0.6B Q8 (recorded, `phonemodel.rs`). These map onto tier T0 here.
- Alerts: OS notification first, Telegram second, and a phone heartbeat that alerts after 30 minutes of silence (proposed).
- Windows-specific code today: 143 `#[cfg(windows)]` lines in 49 files (recorded).
- A proving spike decides whether a mid-range 8 GB Android phone can run the model usefully.

## 10. Scorecard additions

Rows 49-62, numbered to follow the 48-row Scorecard in `00_SOURCE_OF_TRUTH/deferred-atlas-plans/ATLAS_QUALITY_SPEED_AND_LOCAL_MODELS_PLAN_2026-10-18.md`.

| # | Area | Metric | Target | How measured | Current value |
|---|---|---|---|---|---|
| 49 | Processing | Idle CPU | 0.02 of one core or less | Disposable profile, 60 s sample | about 0.05 (measured) |
| 50 | Processing | Idle wakeups on battery | 1 per 10 s or fewer | ETW or counter over 60 s on battery | not measured |
| 51 | Startup | Startup to typing-ready | 2 s or less | Timer from process start to input accepted | not measured |
| 52 | Startup | Startup to tier-0 answer | 5 s or less | Timer from process start to first spoken or written answer | not measured |
| 53 | Memory | Idle RAM per tier | T0 60 MB, T1 0.2 GB, T2 0.3 GB, T3 0.5 GB or less | Working set after 60 s idle | about 70 MB, no model (measured, T2 profile) |
| 54 | Memory | Active RAM per tier | T0 1.2 GB, T1 2.5 GB, T2 4.5 GB (9B deep model is the one exception, note 1 in section 3), T3 10 GB or less | Peak working set over a standard 10-turn script | not measured |
| 55 | Memory | Floor-rule violations | 0 caused by Atlas without Eric's choice ("go ahead anyway" is not counted) | Log scan plus pressure test | not measured |
| 56 | Adaptation | Tier detection correct on a test matrix of 4 machine profiles | 100% | Test with faked hardware profiles (T0 to T3) | not measured; VRAM and NPU always read 0/false today |
| 57 | Storage | Install size | Under 1.5 GB excluding models and toolchains | Folder size | about 8.5 GB total (measured) |
| 58 | Storage | data/ growth per month | 100 MB or less on T2 | Folder size diff, 30 days | not measured; data/ is 1.84 GB (measured) |
| 59 | Acting ahead | Pre-warm hit rate | 30% or more | Used pre-warms divided by all pre-warms, from the log | not measured |
| 60 | Acting ahead | Pre-work outside effects | 0, test-enforced | Test that pre-work cannot call a send, post, delete, buy, trade or settings path | not measured |
| 61 | Learning | Learning replay | 95% or more | A correction is applied next time (18 Oct plan S3) | not measured |
| 62 | Platforms | Platforms building green | Windows and Linux today; macOS, Android, iOS per PLATFORM.md | CI results | Windows and Linux in CI (recorded, `.github/workflows/ci.yml`); Windows is clippy plus lib tests with continue-on-error |

## 11. Phased plan

Folded into the phases in the OFFLINE doc.

| Phase | Work | Changes code | Needs Eric |
|---|---|---|---|
| E0 | Measure startup, wakeups and data growth. No code | No | Say go |
| E1 | Detection fixes (VRAM, NPU, GPU kind) plus event re-planning | Yes | Freeze exception |
| E2 | Wake-on-event, EcoQoS, memory-pressure handler | Yes | Freeze exception |
| E3 | Storage caps | Yes | Approves every deletion |
| E4 | Pre-warming with guardrails | Yes | Freeze exception |
| E5 | Platforms per `docs/PLATFORM.md` | Yes | Sign-off on one-way doors |

E1 onward changes code. The feature freeze applies. Eric decides.

Suggested order and why:

- E0 first. It costs no code and gives the baseline for rows 50, 51, 52 and 58.
- E1 before E2, because the memory handler needs correct VRAM and tier data.
- E3 can run any time after E0, since it only needs Eric's decisions.
- E4 last of the on-device work, because pre-warming needs the handler and the log from E2.
- E5 follows the triggers in PLATFORM.md, not this order.

Exit test for each phase:

| Phase | Done when |
|---|---|
| E0 | Rows 50, 51, 52 and 58 have a measured value |
| E1 | Row 56 passes on all 4 profiles |
| E2 | Rows 49, 50 and 55 meet target |
| E3 | Rows 57 and 58 meet target, with Eric's approval recorded |
| E4 | Rows 59 and 60 meet target |
| E5 | Row 62 shows each platform green as its trigger fires |

## 12. Open questions

For Eric.

1. May the installer keep only the last 2 builds? That would free about 2.5 GB (estimate). Report-ask-then-act: Atlas lists the files first.
2. Is 2 GB the right cap for data/ on T2?
3. Should coding toolchains (about 2.5 GB) be fetched on first use? Which would you rather keep installed?
4. Should the pre-warm battery floor be 30%, or higher?
5. Is the 30% hit-rate threshold for backing off pre-warms right?
6. Is 100 MB per month of data/ growth acceptable on T2?
7. Is the T0 target (phone, 4 GB or less) the right split, or should phones follow the PLATFORM.md rule (1.7B Q8 on 8 GB+)?
8. Does the feature freeze get an exception for E1 (detection fixes), which is the smallest and most valuable code change?
9. Should backups stay at 7, or fewer once they are deduplicated?
10. Is the unload order (deep model, main model to small, voice to light) the order you would choose?
11. Where should the tier decision and its reasons be shown: Settings, Hub, or only in the log?
