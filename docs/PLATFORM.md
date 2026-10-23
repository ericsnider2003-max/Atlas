# PLATFORM — Atlas

Date: 2026-10-22 · Facts checked: 2026-10-23 (skill script date, UTC) · Status: owner decision recorded for the phone; one-way doors still await sign-off
Replaces: `docs/archive/PLATFORM_ARCHIVED_2026-10-22.md` (20 Oct). That record said the cheapest way to reach Atlas from a phone was the laptop's web hub. Eric overruled that on 22 Oct 2026.

## Owner decision (22 Oct 2026, Eric's words)

> "Remember that phone Atlas is supposed to be able to function as a standalone system then connect when it can."
> "I am trying to create the best product I can I don't want something cheap … My vision is a Jarvis (iron man)/ eve (spider man)/donna (suits)"

## Decision

The phone runs Atlas standalone first. With no signal and no laptop, it runs:
- its own small model;
- the no-model command floor;
- its own memory and its own task list.

When it can reach the laptop, it syncs and hands heavier work over. The laptop's hub over the private network is the connected mode. It is not the main way to use Atlas on the phone, and it is not the cheapest path.

Windows desktop still ships first, because it exists and is where Eric works. The Android standalone phone is a v1 must, gated on the proving spikes below. iOS follows when a macOS build host exists.

Starting needs no paid account. Sideloading on Windows and Android has no store fee (estimate; store fees apply only when iOS or Play start [fact:apple-dev-fee][fact:play-dev-fee]).

## The quality bar

Jarvis, EDITH and Donna. Every surface on every device must meet it:
- Atlas already knows what is going on and says it before Eric asks.
- It remembers everything and never makes him explain something twice.
- It is fast on a weak phone and quiet when nothing needs him.
- It is always truthful about the state of the work, including when the phone is offline.

## Gates

| Gate | Result | Source |
|---|---|---|
| G1 Store eligibility | Blocks public App Store and Play listing for the finance features. Atlas reads markets and keeps a ledger; it places no orders. v1 mobile is private distribution only: a sideloaded or Play internal-testing APK, and TestFlight for iOS later. | [fact:apple-321-viii] [fact:play-financial-declaration] |
| G2 What is sold | N/A. Nothing is sold. | brief |
| G3 Device APIs | Pass, native needed. On the phone: the on-device model, microphone, background service, local storage, calendar, widgets and notifications. A web page cannot run the model or listen in the background, and Apple would reject a thin web wrapper. | brief; [fact:apple-42] [fact:ios-web-push] |
| G4 Offline | Pass, and it now decides the design. The phone must work with no signal and no laptop for days: chat with the small model, the command floor, memory, reminders and its own tasks. Work that needs the laptop waits in an outbox and says so. | owner decision 22 Oct 2026 |
| G5 Alert reliability | Pass. Reminders must reach Eric when he is away. The primary channel is the OS notification, raised by the phone itself for its own reminders, so it needs no server. The second channel is Telegram. A heartbeat watches for silence. | [fact:fcm-priority] [fact:apns-time-sensitive] [fact:android-post-notifications] |
| G6 Team and build host | Pass. Eric and AI agents write Rust. The Kotlin and Swift shells exist. There is no Mac, so iOS needs a cloud macOS builder. | [fact:gh-macos-runner] [fact:apple-sdk-min] |
| G7 Store accounts and deadlines | Pass. The Apple fee applies only when iOS starts, and the Play fee only if Play is used. The 12 testers × 14 days rule applies to a production listing, not to private sideloading. | [fact:apple-dev-fee] [fact:play-dev-fee] [fact:play-12x14] |

## Platform set

| Platform | v1 | Why | Revisit trigger |
|---|---|---|---|
| Windows desktop | must | Beachhead. It exists and is where Eric works. It is also the heavy-work end of every handoff. | — |
| Android phone, standalone | must | Owner decision. The Kotlin shell already embeds the Rust core over JNI. | Ships once the Windows foundation is owner-accepted and both proving spikes pass. |
| Hub from the phone over the private network | must (connected mode) | The same pages the phone's own core serves, with the laptop's work added when the laptop is reachable. | — |
| iOS phone, standalone | next | The Swift shell already embeds the core and registers Apple's on-device model first. | The Android standalone phone shows 14 days of real use, and a macOS build host is chosen. |
| Linux desktop | next | The posix layer already covers launch, files, commands and sleep. CI already runs the full suite on Linux. | Eric gets a Linux machine or asks for Linux, and Windows is owner-accepted. |
| macOS desktop | next | Needs window, screen and input work that `posix.rs` refuses today. | A Mac is available, or a cloud macOS build passes `ios.yml`-style CI, and Windows is accepted. |

Beachhead: Windows desktop. First phone: Android, standalone.

## Stack

Recommendation: the Rust core runs on every device, with thin native shells. On the phone it is the same core built with `--no-default-features` (`atlas/src/mobile.rs`), serving its hub on loopback inside the app.
- Desktop uses the existing wry/WebView hub.
- Android uses Kotlin over JNI (`atlas/mobile/android`).
- iOS uses Swift over FFI (`atlas/mobile/ios`).

The same pages render on both devices. The status model (below) comes from one record, so a page reads the same whether the phone is alone or connected.

This fits G3 (native APIs), G4 (an offline core on the device itself) and G6 (the team writes Rust, and the shells exist).

Runner-up: Tauri 2 shells for desktop and mobile. It lost because Atlas already uses wry, which Tauri builds on, and already has working native shells. It would win if the mobile shells stall.

Rejected:
- Flutter, because it rewrites the UI in Dart.
- Electron, because its memory cost breaks the efficiency goal.
- A pure PWA or the "phone as a remote" model, because it cannot run the model, cannot work without the laptop, and cannot listen in the background on iOS [fact:ios-web-push]. That last point is the 20 Oct record's reason to keep the PWA out, and Eric's decision now adds the first two.

One-way doors, each for Eric to sign off:
- the Android package name and signing key;
- the iOS bundle ID;
- the Apple account;
- the sync log format. Once phones in the field write it, it must stay readable.

## What the code does today, measured against the decision

Read on 22 Oct 2026 from `candidate/2026-10-17-foundation` at a0428325. File references are into `atlas/`.

| Need | Today | Gap |
|---|---|---|
| Core runs on the phone | Yes. Android `AtlasCore.kt:15,44` loads `atlas_jni` and starts the core. iOS `AtlasCore.swift:28-48` calls `atlas_mobile_start`. The hub binds loopback only (`mobile.rs:209`). | Neither the `.so` nor the iOS library is in the repo, and no phone build has been run for real (`docs/CAPABILITIES.md` marks it "built, never run for real"). |
| Small model on the phone | `phonemodel.rs:44-58` picks Qwen3 1.7B on 8 GB+ phones and 0.6B on smaller ones. iOS puts Apple's model first (`AppleBrain.swift:22`). | No model is installed with the app. Until Eric downloads 0.6–1.8 GB, the phone answers only non-model requests (`phonemode.rs:92`). The 20 Oct decision chose Q4 builds, but the code still ships Q8. |
| Works with no model | The command floor (`floor.rs`) covers apps, files, backup, timers, time and memory use in plain code. | Its phone-side run is unverified. |
| Stays offline by default | The model chain is Apple, then the phone's own model, then the configured connection (`mobile.rs:271-301`). | After Eric says yes once, free online services become a default fallback (`phonemode.rs:58-64`). Standalone-first requires that the phone ask before each move off the device, and that it never silently replace the local model. |
| Own memory and tasks | The core's store lives under the app's files (`AtlasCore.kt:39`), so memory and task state are on the device. | The phone's pages hide Sync, Offline and four others (`hub.rs:208-219`). Eric cannot see or start a sync from the phone. |
| Sync when it can connect | `sync.rs` keeps an append-only event log per device and merges by replay. Only edits to the same thing on both sides ask Eric. The log moves by any carrier: file, cloud folder or cable. `transport.rs` also exchanges it directly on the same network. | There is no path over the private network or the internet; the archived gaps list notes that sync across two networks was never tested. Nothing tries again automatically when the laptop comes back. |
| Hand work to the laptop | `sync.rs:1279/1311` (`leave_handoff`/`take_handoff`) write and read handoff bundles in a folder. | There is no "send this task to the laptop" action, and no outbox that holds the task and says "queued until the laptop is reachable". |

H10 note: Dispatch called sync "H10", but in the backlog rulings H10 is "Memory that remembers what it forgot" (`30c_BACKLOG_RULINGS_GROUPED_2026-09-25.md:34`). It was built in September and has nothing to do with sync. No backlog item number for phone-to-laptop sync was found.

## Commitments

### Status everywhere (one record)
- Every piece of work is in exactly one state: **needs you**, **working**, **queued** (with the reason in Eric's words), **failed** (why, and what happens next) or **done**.
- On the phone, "queued" includes "until the laptop is reachable", with the time the phone last saw the laptop.
- Every screen on both devices draws from the same record. A test fails the build if two surfaces disagree.

### Shared code
- API contract: the hub's HTTP JSON becomes a versioned contract, which the phone and the laptop both serve.
- Domain package: the Rust core in `atlas/src/`, shared by every shell.
- Money: integer minor units.
- Time: UTC instants plus an IANA timezone (`tz.rs`).
- Minimum version: each device reports the sync log version it writes. An older device shows "update needed" and stops sending.

### Alerts
- Priority class: Android FCM high priority [fact:fcm-priority]; iOS time-sensitive [fact:apns-time-sensitive]. Android 13 and later need the notification permission [fact:android-post-notifications]. The phone raises its own reminders locally, so they work offline. Desktop uses `toast.rs`.
- Second channel: Telegram (`telegram.rs`), to Eric only, through the existing consent gate.
- Heartbeat: when connected, the phone checks the laptop every 15 minutes and tells Eric if it has been silent for 30 minutes (proposed).
- Payload: never sensitive figures, balances, keys or note text.
- Latency target: under 60 seconds from trigger to a locked phone, measured on a real device (proposed).

### Offline and live data
- Fully usable offline on the phone:
  - chat with the local model;
  - the command floor;
  - memory;
  - notes, reminders and the calendar cache;
  - the phone's own tasks.
- Queued writes: each carries a client id and its device's log sequence, so replay is idempotent.
- Handoff outbox: work that needs the laptop waits on the phone and is shown as "queued until the laptop is reachable". It is sent when the laptop answers, and appears on both devices with the same id.
- Stale state: every view of laptop data on the phone shows when it was last seen ("as of"). Anything older than 24 hours is marked stale (proposed).
- Never queued: deletions, sends to other people and anything financial. These need the laptop or Eric's live confirmation.

### Auth and links
- Sign-in: device pairing keys (`peerkey.rs`). There are no account passwords.
- Token storage: Windows Credential Manager, iOS Keychain and Android Keystore.
- Step-up: opening the vault and changing settings.
- Links: none in v1. There is no public listing, so there are no association files.

### Release
- Build host: local Windows for desktop and Android; a GitHub macOS runner for iOS [fact:gh-macos-runner].
- Signing: a signed Windows installer. A signed Android APK, with Eric holding the key and a backup in the vault. iOS is signed on the macOS runner and sent to TestFlight.
- Tracks: a sideloaded APK or Play internal testing; TestFlight for iOS later.
- OTA scope: hub web assets may update without store review. Model files ask Eric first. Native code ships only as a new build.
- Store forms: none for v1. If Play internal testing is used, the data safety form is completed then, with the financial declaration [fact:play-financial-declaration].

### Testing
- Inner loop: the Android emulator, and the iOS simulator on the macOS runner (`ios.yml`, `android.yml`).
- Device farm: the Firebase Test Lab free tier [fact:firebase-test-lab].
- Checks only a real device proves:
  - the model's speed and memory on the phone;
  - a notification while the phone is locked;
  - the background microphone;
  - battery drain;
  - a full offline day followed by reconnect.

### Divergence
| Row | Windows | Linux / macOS | Android / iOS | Default |
|---|---|---|---|---|
| Hub pages and status record | Shared | Shared | Shared (served by the phone's own core) | Shared |
| Notifications | Shared (`toast.rs`) | Different (OS notifier) | Different (local plus FCM/APNs) | Different |
| Voice in and out | Shared | Shared | Different (phone speech services or the light set) | Different |
| Local model | Shared | Shared | Different sizes (1.7B on 8 GB+ phones, else 0.6B; Apple's model first on iOS) | Different |
| Heavy work (self-repair, large models, laptop files) | Shared | Shared | Handed to the laptop via the outbox | Laptop |
| Window control and screen reading | Shared | Not built; refuses with a reason | Not built; refuses with a reason | Windows only |
| File access | Shared | Shared | Different (sandboxed) | Different |
| Autostart | Shared | Different (systemd / launchd) | Different (background service) | Different |

Navigation, layout and accessibility are owned by the design and audit skills.

## Cost

| Item | Range | Basis / source |
|---|---|---|
| Apple developer program | $99 per year, only when iOS starts | [fact:apple-dev-fee] |
| Google Play developer account | $25 once, only if Play is used | [fact:play-dev-fee] |
| macOS CI | $0.062 per minute; about $1.55 for a 25-minute build | [fact:gh-macos-runner]; the build length is an estimate |
| Firebase Test Lab | $0 on the free tier | [fact:firebase-test-lab] |
| Private network between phone and laptop | $0 on the existing personal setup | estimate; basis: Eric's existing private network already links the phone and laptop at no charge, not checked against a source |
| Telegram | $0 | estimate; basis: Telegram bots have no per-message fee in current use, not checked against a source |

## Time

| Workstream | Hours (estimate) | Basis |
|---|---|---|
| One status record, shared by every surface | 10–20 h | the sources exist; the work is joining them (Hub redesign, 22 Oct) |
| Android standalone: build the `.so`, bundle or first-run the model, run the floor on the device | 40–70 h | the shell and core exist and have never run on a real phone |
| Handoff outbox and sync over the private network, with automatic retry | 40–80 h | `sync.rs` merge and handoff bundles exist; transport and retry do not |
| Phone pages: sync status visible, standalone states | 10–20 h | `Page::here` hides them today |
| iOS standalone | 80–140 h | the Swift shell exists; there is no Mac, so builds are CI-only |
| Linux desktop parity | 20–40 h | the posix layer exists |
| macOS platform layer | 60–120 h | `posix.rs` refuses these today |

The critical path runs through Windows owner acceptance and the two Android spikes. The 12 × 14 day Play test [fact:play-12x14] does not apply to sideloading.

## Proving spike

Assumption: a mid-range Android phone with 8 GB can run Atlas standalone for a day, then catch up with the laptop without losing anything.
Test: (1) Qwen3 1.7B Q4 through the Atlas JNI core, 10 runs of a 150-token reply. (2) In airplane mode, create 20 tasks, notes and reminders and ask for one piece of laptop-only work, then reconnect over the private network.
Pass threshold: (1) at least 8 tokens per second and at most 2.5 GB of app memory in 10 of 10 runs. (2) All 21 items reach the laptop within 60 seconds, with 0 lost and 0 duplicated. The laptop-only item shows "queued until the laptop is reachable" on the phone the whole time, then "working" on both devices.

## Assumptions

- Eric is the only user.
- Private distribution is acceptable for v1 mobile.
- Eric has no Mac, so iOS builds use a cloud macOS builder.
- Windows stays the main desktop and must be owner-accepted before the phone ships.
- Eric holds the Android signing key and backs it up in the vault.
- When connected, the phone reaches the laptop over Eric's private network.
- Atlas never places orders, so the finance gate is about listing, not trading.
- The 15 and 30 minute heartbeat values, the 60-second targets and the 24-hour stale mark are proposals, not measurements.

Efficiency and device-tier budgets: `docs/ATLAS_EFFICIENCY_AND_ADAPTATION_DESIGN_2026-10-20.md`. Hub redesign status model: `docs/HUB_REDESIGN_2026-10-22.md` (to be written with the build).
