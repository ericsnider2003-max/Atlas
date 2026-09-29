# Outstanding: everything not done, and what each waits on

**27 September 2026.** This replaces `OUTSTANDING_2026-09-26.md`. It brings three lists together:
- that file (the Atlas Project chat's list, 26b);
- `OPEN_GAPS.md` (the register, with a line per gap and who acts);
- the phone and Windows work of 26–27 Sep.

The detail for each item stays in `OPEN_GAPS.md`. Where an item appears there, its number is given, for example (P.1).

## 1. Windows, iOS and Android: to finish getting Atlas onto devices

| # | Item | Waits on | Who |
|---|---|---|---|
| D1 | **The first install on your iPhone and iPad** (P.1). The build is signed for both (run 5, valid until 27 Sep 2027). | Tailscale running on the laptop and signed in, with HTTPS Certificates on (admin console, DNS page). The .ipa downloaded to the laptop. Then `atlas install-page Atlas.ipa`. | ERIC, then me |
| D2 | **The first install on an Android phone** (P.1). The APK is signed with your key: `4. Awaiting Merge\Atlas-0.1.0-android-27sep.apk`. | An Android phone: yours, or a friend's with you there. | ERIC |
| D3 | **Windows code signing** (8.1). The workflow is ready and signs once six secrets exist. | Azure subscription, Artifact Signing account, identity validation (government ID and face check), certificate profile, an app registration, then the six GitHub secrets named at the top of `windows.yml`. | ERIC |
| D4 | **Atlas's own release key** (8.1). It signs every update the courier carries. | `atlas release keygen` on the laptop, at the keyboard. Write the recovery key on paper; it's shown once. | ERIC |
| D5 | **Friends' first install** (P.8): Funnel or in person? | A ruling. | ERIC |
| D6 | **The phones' own language model** (P.7). | Build: link llama.cpp into each app, or put an engine in the core. Then measure on a real phone. | build + ERIC |
| D7 | **Friends' UDIDs** for the iPhone build. | Collect them all, register them, then one build. | ERIC |
| D8 | **Android developer verification** (build plan B5). It reaches the US in 2027. | Register `com.ericsnider.atlas` with the same signing key before then. | ERIC |
| D9 | **Unlisted App Store (iPhone) and a Play testing track (Android)** (build plan B5). | A demo mode, a privacy policy, an icon and screenshots, and upload steps in both workflows. | later |
| D10 | **The laptop can't compile Atlas** (1.1). Windows builds now come from GitHub (`windows.yml`), so this only matters for building on the laptop. | Visual Studio Build Tools plus `rustup default stable-x86_64-pc-windows-msvc`, or MinGW. | ERIC (optional) |

## 2. Rulings held for you

| item | what's needed |
|---|---|
| H2, the panels and the desktop overlay | Waited on the merge and the hub. Both are now done: it can be scheduled. |
| H7, the phone calendar | The same. |
| H13f, trying voices before choosing | Waits on the hub. |
| K, the hub pages | The hub is paused, as you asked. |
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
- **Tor on the real network, between two Atlases** (8.4).
- **Your voice and rooms**, measured on your recordings (2.2, 5.2).

## 4. The main chat's in-flight pieces (§8 of `OPEN_GAPS.md`)

- **8.2** The Updates page in the hub, its voice phrases, and a real restart into a new build.
- **8.14** Feedback from friends: its hub page and voice.
- **8.3** Tor in the installer.
- **8.5** Arti.
- **8.6** Detecting a network that blocks Tor.
- **8.7** Keeping friends' Tor connections open.
- **8.9** The installer adds the firewall rule.
- **8.10** The phone opens friend links.
- **8.11** Any device holding a verified release can serve it.
- **8.13** Data rollback, with the first format change.

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
- `autocomplete` tokens are missing on personal-data fields (P.6).
- The 3-D, work-day and round-11 tools have limits listed in `OPEN_GAPS.md` §6.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
