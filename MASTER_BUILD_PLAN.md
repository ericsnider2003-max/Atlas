# Atlas — Master Build Plan & Completeness Map

**24 September 2026.** The single reconciliation of everything worked across
this project (10–24 Sep, 71 project docs) against what actually survives in code
in the durable base (`~/Atlas/atlas-current`, git). Purpose: make sure nothing
we spent hours on is dropped, and lay out exactly what remains to build.

Legend: **[IN]** in the base and building · **[PARTIAL]** partly in, rest listed
· **[REBUILD]** was built but source was lost (only a binary/handoff survives)
· **[NEVER CODED]** designed/scoped, never written into code.

---

## How we got here (why this plan exists)
Every past session built in a throwaway cloud container and handed off a zip;
nothing was ever pinned to a durable repo. Containers recycled, trees diverged
into parallel lineages, and two things fell through: the newest feature source
(only its compiled `atlas.exe` survives) and the agreed hub *look* (never coded).
**Fixed now:** `~/Atlas/atlas-current` is a real git repo (base = the richest
surviving tree, `23e`). Everything below commits into it as it's built.

---

## A. In the durable base — built, in code, verified at handoff

| Workstream | Modules | Status |
|---|---|---|
| **Hub** (all 17 pages, server, live content, dashboard, panels, layout) | hub, hublive, dash, panel, layout_prefs, server | [IN] + Warm Paper palette now ported (commit ba4da22) |
| **Learning** — fact book, entity resolution, provenance, corrections, bulk import, associative + **semantic** recall, contextual recall | facts, recall, memory, subject, infer, **meaning**, consolidate, understood + **`embed/`** (local embedding engine, compiled) | [IN] |
| **Animation / rendering** — SVG animation v1 (generate → check → fix → render → verify) | motion, look, look_paint, frames, viewing | [PARTIAL] SVG done; GIF/MP4 + 3D deferred (see C) |
| **Self-finishing** — remaining-work report, atomic land, commissioning, shakedown, self-check, crew | selfwork, selfaudit, checkup, shakedown, improve, build_it, crew, handloop | [IN] |
| **Voice & sensing** — TTS, voice-id, wake/endpoint, gestures, gaze, OCR | tts, voice, voiceid, speech, hearing, endpoint, gaze, handshape, handtrack, ocr | [IN] |
| **Coding capability** — build/fix loop, explanations, diagnosis, delegation | craft, editcraft, explain, diagnose, consult, strategy | [IN] |
| **Messaging / mesh** — E2E Atlas-to-Atlas, hold-and-forward, group mesh, courier | messaging, chat, mesh, courier, telegram, nearby, channel | [IN] |
| **Calendar** — events, repeats, booking, time-blocking, scheduler | calendar, booking, scheduler, timebox | [IN] |
| **Models / local LLM** — model tiers, fallback, gguf | models, brain, gguf, tier + **`llm/`** (local LLM tooling) | [IN] |
| **Business hub / firewall** — clients, partners, trusted recipients, sharing | earned, clients, roster, firewall, profiles | [IN] |
| **Vault / recovery / pairing / sync** | vault, recovery, sync, cloudsync, household, codes | [IN] |
| *[row removed 28 Sep 2026: trading-system material]* |

The base carries **283 source modules**, ceiling 255, ~5,744 tests. It is a
strict superset of every other surviving tree.

---

## B. The build queue — what must be coded, in order

### B1. Hub design conformance to the 29 screens  ·  [PARTIAL]
- **Done:** Warm Paper (lead) / Ember Dark / Access colourways, orange `#d9730d`
  accent, accent hooks, colour-blind tokens, PWA/offline updated. (ba4da22)
- **To build:** the **Appearance & access controls** that *set* theme / accent /
  colour-blind / text-size / density — stored on the machine and emitted by the
  shell as `data-*` attributes (right now the palette only follows the system).
  Plus the **Now** thought-stream's explicit Plan→Doing→Delegated→**Rerouted**→
  Checked→Now→Next steps, which the current page renders more simply than the
  design.
- **Then:** walk all 29 screens one-by-one against the rendered hub and fix drift.

### B2. The 21 GitHub ports (round 3)  ·  [REBUILD]
Source lost; only `atlas.exe` survives. Re-implement with full plumbing + tests:
`recur, cronspec, stemmer, bm25, chunker, typos, linkage, mailthread, vformat,
hlc, automation, urgency, ratelimit, readable, sealedlog` (personal Atlas) and
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

### B3. Round-4 gap-closures  ·  [REBUILD]
Also source-lost. From the round-4 doc: hub calendar/client **import-export
buttons**, `bootstrap::kpss`, measured **VAD** thresholds, **diarization** fix,
Windows cross-build verification, booking home-zone, nudger fix, `atlas doc`
lock, dead-config cleanup, update-drop-in.

### B4. Rendering — the deferred arms  ·  [NEVER CODED]
- **GIF/MP4 output:** frame-capture of the SVG motion + encode (headless browser
  + ffmpeg). `verify_render` already handles the signatures.
- **3-D generation:** emit Blender-CLI / render-code and verify it *runs* —
  quality stays unverifiable by design (Eric's or a stronger model's call).

### B5. Getting the phone apps to people  ·  [DECIDED 27 Sep — ad hoc now, store later]
Eric, 27 Sep: "I want to do the Adhoc for now. We need to keep the Unlisted
App store on the build plan.... For IOS and Android if possible." He wants his
own and his friends' copies to last longer than 90 days, so **TestFlight is
ruled out** (every TestFlight build expires at 90 days).

**Now:**
- **iPhone/iPad, ad hoc.** A build lasts about a year, capped by the
  membership year. Every device's UDID is registered before the build, up to
  100 iPhones a year. Eric's iPhone and iPad are registered. Each new friend
  means one new build, and there's one re-sign a year.
- **Android, the signed APK sideloaded.** It never expires.
  - **Watch:** Google's developer verification. From 30 Sep 2026 it applies
    in Brazil, Indonesia, Singapore and Thailand; the rest of the world
    follows in 2027. After that, a normal phone install of an APK needs the
    app registered in the Android Developer Console. Unregistered apps still
    install through the "advanced flow" or adb.
  - **Before it reaches the US:** register `com.ericsnider.atlas` under the
    free limited-distribution account (up to 20 devices, no fee, no ID) or
    a full account. The signing key must stay the laptop's
    `atlas-android.p12`, since registration ties the app to that key.

**Later — the lasting route: store listings nobody can find without the link.**
- **iPhone/iPad: Unlisted App Store distribution.**
  - No expiry, no UDIDs, no 100-device cap. Friends install with their own
    Apple ID from a private link.
  - Needs full App Review plus Apple approving the unlisted request.
  - To build first:
    - (a) a demo mode, so a reviewer can use the app without Eric's laptop
      or tailnet;
    - (b) a privacy policy page;
    - (c) a 1024 px app icon and screenshots;
    - (d) an App Store upload step in `ios.yml` (App Store export, the
      `altool`/`notary` upload with the existing API key).
- **Android: Google Play has no "unlisted".** The nearest thing is a
  **closed or internal testing track**, invite-only by email, with no
  expiry and updates through Play.
  - Internal testing: up to 100 testers, no review wait.
  - A new personal Play account (a one-time $25) must run a closed test
    with at least 12 testers for 14 days before it can publish to
    production. The testing tracks themselves have no such wait.
  - To build: a Play upload step in `android.yml` (an AAB instead of an
    APK, uploaded with a service-account key) and the same privacy policy.
  - Open: whether to use Play App Signing (Google holds the upload's
    signing key) or keep the laptop key. That's Eric's call when it comes
    up.
- **Why this is last:** both need a stable app first. A reviewer who can't
  use it rejects it.

Sources:
- [Apple: unlisted app distribution](https://developer.apple.com/support/unlisted-app-distribution)
- [Play Console: testing requirements for new personal accounts](https://support.google.com/googleplay/android-developer/answer/14151465?hl=en)
- [Android developer verification timeline](https://android-developers.googleblog.com/2026/06/android-developer-verification.html)

---

## C. Honest limits carried forward (by design, not gaps to close)
- 3-D renders, blueprints, "does this look good" — Atlas can emit the code and
  prove it *runs*, never that it's *good*. Unverified-quality zone.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  datasets is only partially testable here (Wine / synthesized audio / archive).

---

## D. Working method (so this can't happen again)
1. One durable repo: `~/Atlas/atlas-current` (git). Every change commits there.
2. Build in the cloud container (cargo), commit each increment back to the repo.
3. Each workstream: guards + tests green before it's called done.
4. This doc is the plan of record — updated as items land.

---

## Sequence
B1 (design conformance — what Eric most wants and is half-done) →
B2 (the 21 ports) → B3 (round-4) → B4 (rendering arms).

## Update, 24 Sep (Atlas Improvements chat, round 6)
The round 3-5 source was not lost: it lived in that chat's round zips and cloud workspace. It is now 3-way merged onto this repo (commits a0e645e..68f6b5d on top of 3c87755). The appearance and palette commits are kept; a line-by-line check confirms nothing either side added was dropped. B2 and B3 are IN. B4 is built: `filmstrip` (SVG animation played in Edge or Chrome over DevTools, then an in-house GIF encoder, plus MP4 via ffmpeg) and `scene3d` (an in-house ray tracer with a turntable GIF, plus a Blender script when Blender is installed). Details: 21_SESSION_2026-09-24_round6_merge_and_rendering.md.
Found on the laptop: the active Rust toolchain is windows-gnu with no MinGW `dlltool`, so `cargo build` (ATLAS.bat menu 7) fails here. Install MinGW-w64 or VS Build Tools.

## Update, 25 Sep (Atlas Improvements chat, round 8)
Everything buildable in the cloud workspace was built; everything else is in **`OPEN_GAPS.md`** (the register of open gaps: what, why, what closes it, who). Round 8 added: 3-D models from OBJ/STL/glTF/GLB (`meshio`, in house, BVH), glass/glow/patterns, a denoiser (drafts), lamps lit by next-event estimation, all checked against Blender 4.2; and in diarization, `split_strangers` (on by default, held-out 12 → 15 right) and `atlas notes --people N` (90 → 105 of 120 exactly right). Master's a1b19fb and b70be92 are merged into **`round8m`**, which fast-forwards master (`git merge --ff-only round8m`). Details: 23_SESSION_2026-09-25_round8_models_materials_voices.md.

## Update, 25 Sep (Atlas Improvements chat, round 9: a working day)
Research on workflow (ActivityWatch, chrono, interruption science, Windows' own interrupt API) found one blind spot: Atlas only knew you were there when you *spoke*. Built on branch `round9` (fast-forwards master `5dda9b2`):
- `platform`: seconds since keyboard/mouse input and Windows' "is now a good moment" state (presenting, full screen, away).
- Presence from input: no more "welcome back" or notes pushed to the phone after silent typing; a shut lid counts as a break.
- `worklog` (new): where the time went, in spans, categories and focus blocks; "where did my time go today" and `atlas time`. Local only, titles redacted.
- Offers wait for a natural break (bounded deferral, at most 20 min; no bound while presenting).
- A one-line cue on coming back: what you were in the middle of.
- `when` (new): times as people say them; the calendar reads through it ("at 3" is 3 pm, "in 20 minutes", "the 14th", "next Tuesday afternoon").
Session record: `24_SESSION_2026-09-25_round9_a_working_day.md`. Open items: `OPEN_GAPS.md` 2.6 (a real day in the log) and 5.4 (coarse breakpoints).

## Update, 25 Sep (Atlas Improvements chat, round 10: gaps and ideas)
A second look found 19 gaps in round 9 and around it, all fixed on branch `round10` (fast-forwards master `773454a`). The main ones:
- times read sure but wrong ("11-1pm" at 23:00, "in 3 days at 5" today, "tonight at 1");
- reminders dropped in silence, and a booking reply with no time in it;
- meetings and videos counted as away, and a long answer read as a break;
- the offer bound that never fired;
- everyday sentences routed to project work;
- a crash on "İ".

Closed from the register:
- natural pauses learned per app (5.4);
- the Windows passphrase read in-process (3.9), proven on the laptop;
- animations refined by word.

Built and measured, but left off: adaptive sampling (5.3). It helps about 7% on noisy light and loses on fine patterns.

There are also 17 ranked capability ideas. Suggested first: clipboard history, on-screen OCR, market-day awareness, one-step capture.

Session record: `25_SESSION_2026-09-25_round10_gaps_and_ideas.md`.

## Update, 26 Sep (Atlas Improvements chat, round 11: the seventeen built)

All 17 of round 10's ideas are built on branch `round11` (fast-forwards master `24bf4b4`). One module, `workday`, joins them to the daemon: sentences, saving, the tick and the brief.

- **Everyday:** clipboard history, text off the screen, launcher, snippets, key chords, find any file, PDF merge/split/sign, translation.
- **Your day:** waiting-for (from your sent mail), dated capture with a weekly review, meeting prep, people, habits, flashcards, receipts, feeds.
- **Trading:** the market's calendar (checked against NYSE through 2028), and a before/after check-in about process, never a position, ending with the line.

The things that make them sound:
- the clipboard history is off until you turn it on, and kept in memory only;
- no keylogging;
- near file matches are offered, never opened;
- PDFs are written beside the original and re-checked before saving;
- ambiguous names are asked about;
- body-number habits are never raised;
- translations are checked for dropped numbers and links;
- lists are read whole, not cut to two sentences.

Session record: `26_SESSION_2026-09-25_round11_the_seventeen_built.md`.


## Update, 27 Sep (Atlas Improvements chat): phone distribution decided
Ad hoc now, unlisted store listings later, for both platforms. See B5.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
GitHub (`ericsnider2003-max/Atlas`, branch `main`).

## Update, 27 Sep (later): phones and Windows on GitHub
- `atlas install-page` exists (ota.rs): the iPhone's way on with no Mac, and an Android download page.
- The Android and Windows workflows built on their first runs. Windows is unsigned until Azure Artifact Signing is set up (secrets are listed in windows.yml).
- Open item: **on-device language model for the phones.** Today the phone reaches the laptop's model; `models.rs` needs a llama-server process, which phones can't run. Link llama.cpp as a library (Metal on iOS, CPU/Vulkan on Android) or put an engine in the Rust core. Measure on a real phone.
