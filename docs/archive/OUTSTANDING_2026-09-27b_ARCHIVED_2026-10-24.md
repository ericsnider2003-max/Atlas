# Outstanding: everything not done, and what each waits on

**27 September 2026, second session ("27b").** This replaces `OUTSTANDING_2026-09-27.md`; what changed is in doc 39. It started as a copy of that file, which replaced `OUTSTANDING_2026-09-26.md`. It brings three lists together:
- that file (the Atlas Project chat's list, 26b);
- `OPEN_GAPS.md` (the register, with a line per gap and who acts);
- the phone and Windows work of 26–27 Sep.

The detail for each item stays in `OPEN_GAPS.md`. Where an item appears there, its number is given, for example (P.1).

## 0. Everything, in priority order (updated 27 Sep, evening: doc 41)

**Built today, before the Sandbox test** (doc 41):
- **The mark: chosen and applied (doc 42).** The Folded A with the dot, on a rounded tile, is now on Windows (the program, the taskbar and the windows), iPhone and iPad (light, dark and tinted), Android (the launcher, themed icons and notifications), the web app, and the hub. It is also the mark inside Atlas: the waking morning brief, the panels and the idle overlay. Its colour follows each person's appearance (light, dark, tinted on iPhone; light and dark on Android and in Atlas's windows). Only the icon on `atlas.exe` in File Explorer stays light: Windows allows one per program.
- **Widgets:** home screen and lock screen on iPhone, home screen on Android.
- **Your phone** (D5): pick iPhone/iPad or Android, then scan a code. The iPhone's UDID is collected for you.
- **The release key from a button** (8.1).
- **Trying voices before choosing** (H13f).
- **The phone's calendar kept with Atlas's** (H7).
- **The panels drawn** (H2, part): "I'm here" and "show me my tasks" draw their panels, each with **Open in the hub**.
- **One file for Windows**, `Atlas Setup.exe`, which installs as `atlas.exe` whatever the download is called.
- **The Sandbox kit** (now `4. Awaiting Merge\Sandbox test`).
- **Sending an update from the hub** (doc 43): Updates → **Send an update to friends** finds the build you downloaded, and your passphrase signs and queues it. No terminal.

**Waiting on Eric, in order:**
1. **The Sandbox test** (`4. Awaiting Merge\Sandbox test\READ_THIS_FIRST.md`). Windows Home can't run Sandbox; the Azure steps are there instead.
2. **The key sitting (8.1).** Azure: upgrade the free account to pay-as-you-go, then the ID check. The release key: the Updates page on the laptop. Send the key card and the iPhones list to Claude.
3. **Friends' iPhones:** each one uses Your phone once, and the list comes to Eric's Updates page. Then one build (D7).
4. **First installs:** your iPhone and iPad (D1), and an Android phone (D2), both from the Your phone page now.
5. **The first iPhone build with the phone's own model** (D6), and timing it.
6. **Two real networks for friends over Tor** (8.4, 8.7).
7. **Rulings:** 3.1–3.10 (including selfwork's sandbox, 3.4), and how the Atlas program reaches a friend's computer (a link, or by hand).
8. **Later:** the store listings (D9), and Google's developer verification (D8) before 2027.

**Still to build (Claude):**
- **The rest of H2:** the collapsed pill, and the Ready card's actions (Implement / Read / Later) on the desktop.

## 1. Windows, iOS and Android: to finish getting Atlas onto devices

| # | Item | Waits on | Who |
|---|---|---|---|
| D1 | **The first install on your iPhone and iPad** (P.1). The build is signed for both (run 5, valid until 27 Sep 2027). | Tailscale running on the laptop and signed in, with HTTPS Certificates on (admin console, DNS page). The .ipa downloaded to the laptop. Then `atlas install-page Atlas.ipa`. | ERIC, then me |
| D2 | **The first install on an Android phone** (P.1). The APK is signed with your key: `4. Awaiting Merge\Atlas-0.1.0-android-27sep.apk`. | An Android phone: yours, or a friend's with you there. | ERIC |
| D3 | **Windows code signing** (8.1). The workflow is ready and signs once six secrets exist. | Azure subscription, Artifact Signing account, identity validation (government ID and face check), certificate profile, an app registration, then the six GitHub secrets named at the top of `windows.yml`. | ERIC |
| D4 | **Atlas's own release key** (8.1). It signs every update the courier carries. | `atlas release keygen` on the laptop, at the keyboard. Write the recovery key on paper; it's shown once. | ERIC |
| D5 | **Friends' first install** (P.8): ruled 27 Sep. The computer first, then the phone from the hub's Your phone page. Built (doc 41). | Running it with a real phone, and how the Atlas program itself reaches a friend's computer. | ERIC |
| D6 | **The phones' own language model** (P.7). Built 27 Sep (doc 39): llama.cpp inside the core, "get your own model", the right size for the phone's memory. Android core cross-built and linked here; iOS not buildable here. | The first `ios.yml` run with `phone-llm-metal` (its first compile), then on a real phone: "get your own model" on wifi, ask something, time it. | ERIC |
| D7 | **Friends' UDIDs** for the iPhone build. Collected by Atlas now (Your phone → iPhone), and listed on Eric's Your phone page with a Copy button. | Send the list to Claude; the next build carries them. | ERIC |
| D8 | **Android developer verification** (build plan B5). It reaches the US in 2027. | Register `com.ericsnider.atlas` with the same signing key before then. | ERIC |
| D9 | **Unlisted App Store (iPhone) and a Play testing track (Android)** (build plan B5). | A demo mode, a privacy policy, screenshots (the icon is done), and upload steps in both workflows. | later |
| D10 | **The laptop can't compile Atlas** (1.1). Windows builds now come from GitHub (`windows.yml`), so this only matters for building on the laptop. | Visual Studio Build Tools plus `rustup default stable-x86_64-pc-windows-msvc`, or MinGW. | ERIC (optional) |

## 2. Rulings held for you

| item | what's needed |
|---|---|
| H2, the panels and the desktop overlay | Part built 27 Sep (doc 41): the panels Atlas decides on are now drawn, and each opens its hub page. Left: the collapsed pill, and the Ready card's actions. |
| H7, the phone calendar | Built 27 Sep (doc 41): the phone's calendar goes to Atlas and Atlas's comes back into an "Atlas" calendar on the phone. Not yet run on a phone. |
| H13f, trying voices before choosing | Built 27 Sep (doc 41): Hear and Get on the Sound page. |
| K, the hub pages | Resumed 27 Sep (doc 40): Updates, Feedback, Partners online/offline, Documents' Send and share log, the phone model on Connections, `autocomplete`. Left: invoices (no invoice module). A look on the laptop, phone and iPad. |
| *[row removed 28 Sep 2026: trading-system material]* |
| 3.1–3.10 in `OPEN_GAPS.md` | delegate's overnight reach, consult typing into third-party chats, voiceid adaptation, selfwork sandboxing, the enrolment gate, "do this online", Cloudflare Workers, spoken unlock while handed over, one waking line or two. |
| Two appearance menus | One menu would be clearer once the hub design settles. |

## 3. Built, tested with stand-ins, never run for real (on the laptop)

- **A friend's machine** (friend-ready §7). Atlas starting its own llama-server was tested with a stand-in. To check it on the laptop: stop the hand-started server, then ask a question.
- **Push-to-talk, end to end.** It stops at "I can't hear on this machine yet" until whisper and piper are in the install folder. `atlas doctor` names them.
- **Setting a key by pressing it** (Settings → Keys).
- **Typing correction in other apps** (H4).
- **The camera:** "look at my screen", faces, things and hands, and teaching a gesture (H6).
- **A real call** for call notes.
- **Mail sorting** (G1) and **a scheduled post** (G2).
- **Two-factor sign-in** in Atlas's own browser.
- **Toasts, and the Windows sign-in vault,** on the real desktop (1.4).
- **The Windows local-time fix, and identity backup with separate profiles** (8.12).
- **Tor on the real network, between two Atlases** (8.4) -- now with Tor fetched by setup (8.3), bridges on a blocking network (8.16) and kept-open connections (8.7).
- **The firewall rule** (8.9): setup's "Letting your own devices reach Atlas" step, on Windows.
- **Updates and feedback by voice**, and **Report a problem with Atlas** in the window, on the laptop.
- **The phone's own model** on a real phone (D6).
- **Your voice and rooms**, measured on your recordings (2.2, 5.2).

## 4. The main chat's in-flight pieces (§8 of `OPEN_GAPS.md`)

Closed on 27 Sep (doc 39): **8.3** Tor ships with Atlas, **8.6** networks that block Tor, **8.11** any device passes a verified release on.

- **8.2** A real restart into a new build, on the laptop. (The Updates page was built 27 Sep, doc 40.)
- **8.5** Arti.
- **8.7** Friends' Tor connections kept open: built; timing on the real Tor network is yours (with 8.4).
- **8.9** The firewall rule: built into setup; its first run on Windows is yours.
- **8.10** The phone opens friend links.
- **8.13** Data rollback, with the first format change.
- **8.15** (new) A held build is still passed on by friends who have it: a signed hold notice, after the key sitting (8.1).
- **8.16** (new) Bridges through a real blocking network: try once where Tor is blocked.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## 6. The unused-code backlog (measured by the guards; the names are in `catalogs/`)

| list | meaning |
|---|---|
| `TEST_ONLY_MAX` (dead_capabilities) | public functions with a test and no caller |
| `ORPHANS` | no caller and no test, each named with what it waits on |
| `HELPER_UNTESTED_MAX` | module-internal helpers with no direct test |
| `UNWIRED_*.md` | whole modules and public surfaces nothing reaches |
| `DEAD_CONFIG_*.md` | settings that change nothing |

Every list fails the build in both directions: it may shrink, and it may grow only by name.

## 7. Known limits (from the session docs)

- Atlas's browser runs hidden, so a robot check can't be clicked in it; it hands you the page instead.
- Push-to-talk and the typing box need Windows.
- A new key takes hold only after Atlas restarts.
- With no voiceprint enrolled, anyone at the unlocked laptop is taken to be you.
- iOS suspends Atlas in the background (P.3).
- iPhone Duo sizes are worked out, not measured (P.5).
- There has been no hands-on screen-reader pass (P.4).
- ~~`autocomplete` tokens missing on personal-data fields (P.6)~~: closed 27 Sep (doc 40).
- The 3-D, work-day and round-11 tools have limits listed in `OPEN_GAPS.md` §6.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
