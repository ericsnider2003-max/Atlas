# PLATFORM — Atlas

Date: 2026-10-20 · Facts checked: 2026-10-21 (skill script date, UTC) · Status: awaiting sign-off

## Decision

Windows desktop ships first, because it already exists. The cheapest way to reach Atlas from a phone is the web hub over the private network, so that is also a must. Linux, macOS, Android and iOS come later, each behind a trigger. Starting needs no paid account: Windows and Android sideloading need no store fee (estimate; store fees apply only when iOS or Play start [fact:apple-dev-fee][fact:play-dev-fee]). The thing that could change the answer is the Android proving spike: if a mid-range phone cannot run the offline model usefully, the phone stays a remote to the laptop.

## Gates

| Gate | Result | Source |
|---|---|---|
| G1 Store eligibility | Blocks public App Store and Play listing for the finance features. Atlas has market reading, a ledger and money features (no order placement). v1 mobile is private distribution only: a sideloaded or Play internal-testing APK, and TestFlight for iOS later. | [fact:apple-321-viii] [fact:play-financial-declaration] |
| G2 What is sold | N/A. Nothing is sold. | brief |
| G3 Device APIs | Pass, native needed. Phone: microphone, background service, calendar, widgets, notifications. Desktop: window control and screen reading. A thin web wrapper would also be rejected by Apple. | brief; [fact:apple-42] |
| G4 Offline | Pass. Core must work offline. | brief (charter) |
| G5 Alert reliability | Pass. Reminders must reach Eric when away. Primary is the OS notification. Second channel is Telegram. A heartbeat watches for silence. | [fact:fcm-priority] [fact:apns-time-sensitive] [fact:android-post-notifications] |
| G6 Team and build host | Pass. Eric plus AI agents write Rust. Kotlin and Swift shells exist. There is no Mac, so a cloud macOS builder is needed. Xcode runs only on macOS and the current iOS SDK is iOS 26. | [fact:gh-macos-runner] [fact:apple-sdk-min] |
| G7 Store accounts and deadlines | Pass. Apple fee applies only when iOS starts. Play fee applies only if Play is used. The 12 testers x 14 days rule applies to production listing, not private sideloading. | [fact:apple-dev-fee] [fact:play-dev-fee] [fact:play-12x14] |

## Platform set

| Platform | v1 | Why | Revisit trigger |
|---|---|---|---|
| Windows desktop | must | Beachhead. It exists and is where Eric works. | — |
| Web hub from the phone over the private network | must | Cheapest any-device path. The hub already exists. | — |
| Linux desktop | next | posix layer already covers launch, files, commands and sleep. CI already runs the full suite on Linux. | Eric gets a Linux machine or asks for Linux, and Windows is owner-accepted. |
| macOS desktop | next | Needs window, screen and input work that posix.rs refuses today. | A Mac is available, or a cloud macOS build passes ios.yml-style CI, AND Windows is accepted. |
| Android companion | next | Kotlin shell exists (AtlasService, CalendarSync, GlanceWidget, JNI). | The Windows foundation is owner-accepted and the proving spike passes. |
| iOS | next | Swift shell exists (AtlasCore, AppleBrain, widgets, share extension). No Mac, so it is last. | the Android companion shows 14 days of use in its usage log (to be added), and a macOS build host is chosen. |

Beachhead: Windows desktop

## Stack

Recommendation: keep the Rust core with thin native shells. Desktop uses the existing wry/WebView hub UI. Mobile uses the existing Kotlin and Swift shells over the Rust core via JNI and FFI. This fits G3 (native device APIs), G4 (offline core) and G6 (the team already writes Rust and the shells exist).
Runner-up: Tauri 2 for desktop and mobile shells — lost because Atlas already uses wry (the webview library Tauri builds on) and already has working native mobile shells. It would flip if the mobile shells stall.
Rejected: Flutter — rewrites the UI in Dart; Electron — memory cost breaks the efficiency goal; a pure PWA — cannot run the local model or reach the microphone in the background on iOS [fact:ios-web-push].
One-way doors: Android package name and signing key, iOS bundle ID, the Apple account. Eric signs off on each.

## Commitments

### Shared code
API contract: the hub's HTTP JSON becomes a versioned OpenAPI contract with a generated client.
Domain package: the Rust core in the Atlas repository (src/), shared by all shells.
Money: integer minor units.
Time: UTC instants plus an IANA timezone (tz.rs exists).
Min-version switch: the hub reports a minimum client version; older clients show an update-required screen and stop sending requests.

### Alerts
Priority class: FCM high priority on Android [fact:fcm-priority]; iOS time-sensitive [fact:apns-time-sensitive]; Android 13+ needs the notification permission [fact:android-post-notifications]. Desktop uses toast.rs.
Second channel: Telegram (telegram.rs exists). Heartbeat and Telegram messages go to Eric only, through the existing consent gate.
Heartbeat: the phone app checks the laptop's Atlas every 15 minutes and alerts Eric when it has been silent for 30 minutes (proposed).
Payload: no sensitive figures, balances, keys or note text are ever sent in a notification payload.
Latency target: under 60 seconds from trigger to a locked phone, measured on a real device (proposed).

### Offline and live data
Readable offline: chat, notes, calendar cache and reminders.
Stale state: every offline screen shows an "as of" stamp; anything older than 24 hours is marked stale (proposed).
Never queued: deletions, sends to other people and anything financial.

### Auth and links
Sign-in: device pairing keys (peerkey.rs). No account passwords.
Token storage: Windows Credential Manager, iOS Keychain, Android Keystore.
Step-up: opening the vault and changing settings.
Links: none in v1. No public store listing, so no association files. Eric owns any later ones.

### Release
Build host: local Windows for desktop and Android; a GitHub macOS runner for iOS [fact:gh-macos-runner].
Signing: Windows signed installer. Android signed APK with Eric holding the key, backed up in the vault. iOS signed on the macOS runner and sent to TestFlight.
Tracks: sideloaded APK or Play internal testing; TestFlight for iOS later.
OTA scope: hub web assets may update without store review. Model-file updates ask Eric first. Native code ships only as a new build.
Store forms: none for v1 (private distribution). If Play internal testing is used, the Play data safety form is completed then, with the financial declaration [fact:play-financial-declaration].

### Testing
Inner loop: Android emulator and the iOS simulator on the macOS runner (ios.yml on macos-15; android.yml exists).
Device farm: Firebase Test Lab free tier [fact:firebase-test-lab].
Real-device-only checks: notification while the phone is locked, background microphone, battery drain.

### Divergence
| Row | Windows | Linux / macOS | Android / iOS | Default |
|---|---|---|---|---|
| Notifications | Shared (toast.rs) | Different (OS notifier) | Different (FCM / APNs) | Different |
| Voice in and out | Shared | Shared | Different (phone speech services or light set) | Different |
| Window control | Shared | Not built yet (refuses with a reason, as posix.rs does today); Eric decides whether to build | Not built yet (refuses with a reason); Eric decides whether to build | Windows only |
| Screen reading | Shared | Not built yet (refuses with a reason, as posix.rs does today); Eric decides whether to build | Not built yet (refuses with a reason); Eric decides whether to build | Windows only |
| Local model | Shared | Shared | Different sizes (decision: 1.7B Q4 on 8 GB+ phones, else 0.6B; phonemodel.rs currently ships Q8) | Different |
| File access | Shared | Shared | Different (sandboxed) | Different |
| Autostart | Shared | Different (systemd / launchd) | Different (background service) | Different |

Navigation, layout and accessibility: Different, owned by the design and audit skills.

## Cost

| Item | Range | Basis / source |
|---|---|---|
| Apple developer program | $99 per year, only when iOS starts | [fact:apple-dev-fee] |
| Google Play developer account | $25 once, only if Play is used | [fact:play-dev-fee] |
| macOS CI | $0.062 per minute; about 25 min per build is about $1.55 per build | [fact:gh-macos-runner]; build length is an estimate |
| Firebase Test Lab | $0 on the free tier | [fact:firebase-test-lab] |
| Telegram | $0 | estimate; basis: Telegram bots have no per-message fee in current use, not checked against a source |

## Time

| Workstream | Hours (estimate) | Basis |
|---|---|---|
| Linux desktop parity | 20–40 h | posix layer exists; CI already runs on Linux |
| macOS platform layer (windows, screen, input) | 60–120 h | posix.rs refuses these today |
| Android companion with offline model and hub | 60–100 h | Kotlin shell exists; model and hub wiring remain |
| iOS | 80–140 h | Swift shell exists; no Mac, so CI-only builds |
| Hub API contract | 15–30 h | one developer with AI help |

Critical-path calendar: Windows owner acceptance first. Apple review applies only to TestFlight external testers. The 12 x 14 day Play test [fact:play-12x14] does not apply to private sideloading.

## Proving spike

Assumption: a mid-range Android phone with 8 GB can run Atlas's offline model usefully.
Test: Qwen3 1.7B Q4 (the smaller build; phonemodel.rs currently ships Q8) through the Atlas JNI core, 10 runs of a 150-token reply.
Pass threshold: at least 8 tokens per second and app memory at most 2.5 GB in 10 of 10 runs.

## Assumptions

- Eric is the only user.
- Private distribution is acceptable for v1 mobile; no public store listing.
- Eric has no Mac, so iOS builds use a cloud macOS builder.
- Windows stays the main platform and must be owner-accepted before the others start.
- Eric holds the Android signing key and backs it up in the vault.
- The phone reaches the laptop's Atlas over the private network (mesh, sync, remote, wireguard, hublive modules).
- Atlas never places orders, so the finance gate is about listing, not about trading.
- The 15 and 30 minute heartbeat values and the 60 second latency target are proposals, not measured.
- Linux and macOS desktop have no current users besides possible future ones.

Efficiency and tier budgets: docs/ATLAS_EFFICIENCY_AND_ADAPTATION_DESIGN_2026-10-20.md.
