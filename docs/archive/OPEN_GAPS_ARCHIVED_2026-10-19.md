# Open gaps — the register

**Kept current as of 27 September 2026, second session (8.3, 8.6, 8.11, 8.14 and P.6 closed; 8.1, 8.2, 8.7, 8.9, P.7 and P.8 narrowed -- docs 39, 40 and 41).** This
lists every known gap that is still open, in one place. For each one: what
it is, why it's open, what closes it, and who has to act. When a gap closes,
delete its line in the same commit and say so in that session's handoff.
Anything that can be built and tested in the cloud workspace isn't listed:
it gets built.

Status legend: **ERIC** = needs you (a decision, a credential, hardware, or an
install on your machine) · **MAIN** = belongs to the main Atlas chat's
in-flight work · **MEASURED** = tried and measured, not solved; the numbers are
in the linked doc · **LIMIT** = a deliberate boundary of what's built, named so
nobody mistakes it for done.

---

## 1. On your laptop (le3o)

| # | Gap | Why it's open | What closes it | Who |
|---|---|---|---|---|
| 1.1 | **Atlas can't be built on the laptop** (ATLAS.bat menu 7 fails) | The active Rust toolchain is `stable-x86_64-pc-windows-gnu`, which needs MinGW's `dlltool.exe`, and that isn't installed. | Either install **MinGW-w64** (put its `bin` on PATH), or install **Visual Studio Build Tools (C++ workload)** and run `rustup default stable-x86_64-pc-windows-msvc`. The second is the usual Windows route. Nothing was installed for you, by standing rule. Meanwhile, Windows builds are cross-compiled in the cloud and copied over. | ERIC |
| 1.2 | **Blender isn't installed**, so `scene3d` makes only its own render there | It's optional. The in-house renderer stands alone. Blender is a second opinion and the "best quality" path. | Install Blender 4.2 or later from blender.org. `find_blender` looks in `Program Files\Blender Foundation\*` and on PATH, or set `vars.blender` in tools.yaml. The script has only been run against Blender 4.2's Python module (in the cloud), never desktop Blender. | ERIC |
| *[row removed 28 Sep 2026: trading-system material]* |
| 1.4 | **Toasts and the Windows sign-in vault on the real desktop** | They were never exercised on the laptop, because putting things on your screen unasked is against the standing rule. | One supervised run: `atlas doctor`, then a test toast from the hub. | ERIC |


## Phones, tablets and accessibility (26 Sep 2026)

| # | Gap | Why it's open | What closes it | Who |
|---|---|---|---|---|
| P.1 | **The phone apps build, but have never run on a phone.** iOS: built and signed on GitHub's Mac (run 5, 27 Sep), for Eric's iPhone and iPad, until 27 Sep 2027. Android: built on GitHub (run 1) and by hand, signed on the laptop. | Nothing has been installed yet. The iPhone needs the install page (`atlas install-page`) over Tailscale; the Android APK can be copied over. | Tailscale running on the laptop with HTTPS certificates on; the .ipa downloaded to the laptop; one install on each device. | ERIC |
| P.2 | ~~Android needs OpenSSL built for Android~~ **Closed 26 Sep:** `native-tls` uses `vendored` on Android only. | — | — | — |
| P.7 | **The phones' own model: built, not measured on a phone.** 27 Sep: llama.cpp is linked into the phone core (`phonemodel`, feature `phone-llm`; Metal on iPhone/iPad), loaded in-process, and "get your own model" downloads Qwen3 1.7B (8 GB+ phones) or 0.6B (the rest), pinned by SHA-256, resuming. Root causes fixed on the way: the phone core was started with **no model connection at all** (it lived in the desktop `main`), and every model call was a `curl` child process a phone can't start; both fixed (`models::connection`, `tools::curl_in_process`). Measured on 2 laptop CPU cores: 0.6B about 16 tokens/s, an answer through Atlas in 12-16 s; 1.7B about 6.5 tokens/s, 38-59 s. | Tokens a second, memory and battery on a real iPhone and Android phone; the iOS build compiling (only the cloud Mac can: `ios.yml`). Atlas's own prompts are ~1,100 tokens and a phone reads them first each time; two kept slots reuse most of it (1,040 of 1,096 tokens measured). | Run the iPhone build, install (D1), say "get your own model" on wifi, then ask something and time it. | ERIC (the phones) |
| P.8 | **Friends' first install** (ruled 27 Sep) | Eric: "Friends receive a copy of Atlas on their computer first, then do their setup, and they get their phone access by selecting their device type and it sends them the link." Built (doc 41): the hub's **Your phone** page. Pick iPhone/iPad or Android, then scan a code. For iPhone the code first collects the device's UDID (Apple's profile service) and sends it to you for the next build. The app is served over the computer's own Tailscale HTTPS for 15 minutes. How the Atlas program itself reaches a friend's computer (a download link, or by hand) is still to settle with the signing. | Run it on the laptop with a real phone. | ERIC |
| P.3 | **iOS suspends Atlas in the background** | That is how iOS works. Reminders due while it is suspended fire when you next open it. | Schedule local notifications ahead of time from the iOS shell. | LIMIT, then build |
| P.4 | **No hands-on screen-reader test** (NVDA, JAWS, Narrator, VoiceOver, TalkBack) | The automated checks (axe-core, at ten screen sizes, 0 violations) can't replace a person using a screen reader. | One pass per screen reader on the real devices. `docs/ACCESSIBILITY.md` lists what to try. | ERIC |
| P.5 | **iPhone Duo sizes are worked out, not measured** | It ships 23 Oct 2026. Its 466 by 678 and 626 by 890 point sizes come from Apple's pixel counts. | Open the hub on one. | ERIC (when available) |

## 2. Credentials, data and hardware only you have

| # | Gap | Why | What closes it | Who |
|---|---|---|---|---|
| 2.1 | **Inbox reading** (`mail`) | Needs an app password for your mailbox, entered into the vault. It's never written to a file. | Make an app password with your mail provider and enter it once with `atlas vault`. | ERIC |
| 2.2 | **Your real voice and rooms** | Wake phrase, voice-lock and hearing thresholds are all measured on synthesized speakers (espeak-ng). Real people vary more, so those numbers are a ceiling. | Three takes of the wake phrase (`atlas voices enrol`) and one room calibration (`atlas hearing calibrate`) at your desk. | ERIC |
| *[row removed 28 Sep 2026: trading-system material]* |
| 2.4 | **A model for `atlas fix`** (the hand-off loop that works a failing test until it passes) | The loop is built and tested with a scripted model. Real use needs a local coding model (`llm_model`) or a delegate. | Pick and install a local model (the 12_LOCAL_LLM doc lists fits for your hardware), or allow online delegation for coding. | ERIC |
| 2.6 | **A real day in the work log** | Round 9's focus blocks, categories and bounded deferral, and round 10's per-app pauses, are measured on synthetic days. What a real one looks like, and whether the built-in categories fit your apps, needs Atlas running on your machine for a day. | Run Atlas for a working day, then `atlas time` (or ask "where did my time go today"). Add your own categories under `worklog.categories` if the built-ins miss. | ERIC |
| 2.5 | **The phone as a real Atlas** (main chat, b70be92) | The core builds without the desktop GUI and has a phone platform layer, but the Android NDK cross-compile, a real two-network sync (phone away from home over a tailnet), and on-device model speed all need hardware. | An Android phone (and the NDK on a build machine), or an iPhone for the PWA path. | ERIC / MAIN |

## 3. Rulings held for you (design calls, not code)

These are built as far as they can be without the decision. Each is named in
the handoffs that held it.

| # | Ruling | State |
|---|---|---|
| 3.1 | `delegate`'s overnight default is `Reach::Converse`, which *sends* in a third-party app. The recommendation is `Reach::Draft` unless sending is chosen. | Unwired until ruled. |
| 3.2 | `consult`: may Atlas type into a third-party chat window? | Held. |
| 3.3 | `voiceid` threshold adaptation: may the voice-lock line move with your own scores? | The evidence is in `atlas doctor`'s voice-lock line. Held. |
| 3.4 | `selfwork` running the full suite in a sandbox. | Held. |
| 3.5 | The enrolment confirmation gate ("I'll stop and check before anything is agreed to"). | `permitted` is wired. The run isn't. |
| 3.6 | A general "do this online" delegation intent, which would wire `dispatch_task`. | Machinery built and tested. Waits on the intent. |
| 3.7 | Cloudflare Workers for whole compute jobs. | The seam (`online.rs`) is ready. |
| 3.8 | Spoken unlock while handed over (currently refused, so a passphrase isn't said aloud in front of whoever holds the laptop). | Needs your agreement. |
| 3.10 | `mind::speak_brief` vs `brief.rs`: one waking line or two? | A design call. |

## 4. The main chat's code

| # | Gap | Why | Who |
|---|---|---|---|
| 4.1 | `transport::bind_local_ephemeral` is reached only by tests | b70be92 deleted the other two (`hlc::resuming_from`, `sync::clock_at`) and named this one as test-only on purpose: it binds a loopback port for tests. | MAIN (settled) |
| 4.2 | **247 functions are built and tested but reached only by tests** (`TEST_ONLY_MAX`) | The long backlog, named one by one in `tests/new_capabilities_are_wired.rs`. The list may shrink, and may grow only out loud. | MAIN (and any session) |
| 4.3 | **Hub conformance to the locked design's 30 artboards** (build plan B1) | Built 26 Sep: the three colourways' exact tokens (Warm Paper as the page's base), the labelled sidebar with the Personal group, a business group once a business exists, and Settings at the foot, the breadcrumb, Home (Brief carrying what's waiting on you, Today, Right now or Business at a glance, first run), Outstanding (four lanes, Tried/Stopped/Needs), Now (the stream, its rail, Plain/Detailed, Pause), and Atlas's own windows in the same colourway. The design is in the tree (`atlas/design/hub/`, `SPEC.md`) and `tests/the_hub_design_is_the_locked_one.rs` holds the hub to it. **The rest was built 26 Sep (second pass):** Messages, business Overview/Clients/Partners, Documents, Sound & voice, Trusted, shared tasks as Table/Board/Calendar, the week grid, Give, Talk, Offline, Help & accessibility, the phone layout (five tabs, one-row top bar), the rail for tablets and foldables, and native windows following Windows. Left: invoices on Overview (no invoice module). Online/offline on Partners and the share log on Documents were built 27 Sep (doc 40). Not yet looked at on the laptop. | ERIC (a look on the laptop, phone and iPad) |
| 4.4 | `contents::Contents` is never constructed, so `nudge::drifted` can't fire | A feature to build (something has to produce the contents snapshot), not a wire. | MAIN |

## 5. Measured, not solved

| # | Gap | What was measured | Next idea |
|---|---|---|---|
| 5.1 | **Diarization: one person split in two** (over-split) | Round 6's pooled ΔBIC second look fixed it on the tuning calls but mixed two people once in 20 held-out calls, so it stays opt-in (`--merge-voices`). Round 8 tried scoring a one-turn label against the other speakers and moving it to the best fit. It changed nothing on the held-out calls, so it was removed. Round 8 also found the **bigger failure was the opposite one**: someone who speaks only once gets swallowed into another speaker (17 of 40 calls). `split_strangers` fixes much of that and is on by default: on held-out calls, exactly-right calls went 12 → 15 and mixed calls 7 → 4. The cost is more over-splits on calls where everyone speaks at least twice (3 → 7 of 60), so over-splitting is now the main error left. | Speaker embeddings trained on real voices would separate speakers better than MFCC Gaussians do, but that needs a model, and building in house rules out downloading one. Or: one fact from you ("three people on this call") would settle it. |
| 5.2 | **Diarization on real recordings** | All numbers come from synthesized voices plus a fan. See 2.2. | Your recordings. |
| 5.3 | **Drafts stay noisy: neither the denoiser nor adaptive sampling does much** | The denoiser takes out about a tenth of a draft's error (3.31 → 2.91). Round 10 built adaptive sampling (more rays where a pixel's first rays disagree) and measured it: about 7% better than uniform at equal cost on the lamp scene (2.64 vs ≈2.85), and **worse** on a held-out scene with a checker floor (4.75 vs ≈3.34), because a pixel of a distant checker is wrong while its few rays agree. It's off unless a scene asks (`"adaptive": true`). | Flag pixels by how much they differ from their neighbours (catches aliasing), or spend the rays on "good". |

| 5.4 | **Breakpoints are still coarse** | Round 10 learns what a natural pause is **per app** from your own pauses (longer than 9 in 10 of that app's, after 80 of them). On a synthetic day it cut false breaks from 197 to 11 with no break missed. Real pauses need real days (2.6). Iqbal & Bailey's within-app breakpoints from trained models (the end of a sentence, a finished edit) are still not detected. | Your days in the log; then, if it's worth it, events inside an app (a file saved, a message sent) as breakpoints. |

## 6. Limits of what's built (named so they aren't mistaken for done)

**3-D (`scene3d`, `meshio`):**

- No image textures. OBJ `vt`/`map_Kd` and glTF textures are read past; a model gets its material colours, or the object's colour, or a procedural pattern.
- glTF: triangles (mode 4) only; no skins, morph targets or the file's own animations; no Draco or meshopt compression.
- STL has no units or colours. Use `fit`.
- Glass passes light but doesn't focus it (no caustics). A shadow through glass is dimmed and tinted, not brightened.
- Light bounces once, roughly (near-field colour bleed), not a full path trace.
- Colours differ from Blender's: Atlas uses an ACES fit and Blender its own view transform. Shape, placement and pattern match (Suzanne's silhouette overlaps Blender's by 90% or more in OBJ, STL and GLB; the pattern class agrees on over 90% of pixels); tone doesn't.
- The noise pattern in Blender is Blender's own noise, so it's the same kind of pattern but not the same one.
- Whether a scene *looks good* is still yours to judge.

**Work day (round 9):**

- Categories are word matches on app and title, not understanding: a meeting in a browser tab reads as browsing unless you add a rule.
- A bare "at 7" is 7 am and "at 6" is 6 pm (chrono's rule, said back to you).
- Numeric dates are read in US order, and marked unsure when both readings are real dates.
- Linux and the phone can't report keyboard timing, so away time isn't separated there, and the report says so.
- A call or a video explains up to 2 hours of input silence (round 10); past that it counts as away, which is right for falling asleep in front of a film and wrong for a very long meeting.
- Pauses are read at the tick, so very short ones (under a tick) aren't counted; the learned threshold leans a little long.
- A time that has already passed is asked about, not booked; so is "tomorrow" said between midnight and 4 am.

**This chat's first package (10 Sep, "work off the tick"): CLOSED 26 Sep.** Only its first pass had reached the tree; the rest was rebuilt from its design (`docs/improvements-project/handover-crew-10sep.md`) against today's `crew`, and landed with its tests.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **Third pass (idle cost), rebuilt:** `persist_after` (a quiet tick saves on the minute, a tick that spoke saves at once); folder scans double their wait when nothing changes, up to 30 minutes (5 while you're at the machine); `tools_cfg` resolved once and shared (80 call sites now clone one section, not the whole config); the model kept warm as `fit` measured (`brain::with_keep_alive`, 30 min or 60 s, local server only, your own `keep_alive` wins); `wait_or_kill` polls 1 ms doubling to 200 ms.
- **Found on the way:** nothing read the battery. `health::read_machine` never filled `on_battery` or `battery_percent`, so the overnight run's battery check could never fire. It's now read on Windows (`GetSystemPowerStatus`), Linux (`/sys/class/power_supply`) and macOS (`pmset`).
- **Not carried over, on purpose:** the 10 Sep progress-line rate limit. Today's `edit` render collects the tool's output whole; there's no per-frame line to limit.
- **Tests:** `tests/crew.rs`, `crew_efficiency.rs`, `idle_cost.rs`, `wants.rs`. And `tests/promised.rs`, which fails by name if anything a package promised leaves the tree. Every future package adds its rows there before it's called done.

**Round 11's tools:**

- **Clipboard history and key chords ship off (ERIC).** Turning either on is your call: "turn on clipboard history", and `workday.chords.enabled: true`.
- **Meeting prep reads people from the title and notes (LIMIT).** Calendar events here carry no attendee list, so a meeting called "Standup" with nobody named gets no prep. Imported `.ics` attendees would close it, once the importer keeps them.
- **Waiting-for reads only the mail you've sent in the last month (LIMIT).** It reads it on each mail check. A promise made in chat or on a call isn't seen. Commitments are found by phrasing ("could you…", "I'll…"), which you can teach ("not a promise 3"), not by a model.
- **CPI dates run out after December 2026 (LIMIT, said when reached).** BLS's 2027 schedule was only "projected" when this was built. NYSE holidays are checked through 2028 and worked out by rule after.
- **PDFs (LIMIT):**
  - An encrypted PDF is refused, not unlocked.
  - A broken file whose cross-reference table is wrong is read by walking its objects. One whose objects are themselves damaged is refused.
  - Forms and annotations are carried over as they are, but links to pages that aren't chosen become empty.
  - There's no rotation or compression step yet.
- **Receipts (LIMIT):** the total, merchant and date only, no line items. A receipt in a photo on the phone reaches the laptop through the tray, and is read there from the screen or from pasted text.
- **Feeds (LIMIT):** no login-only feeds, and no full-text fetch unless you say "read 1". Read-later is the tray's link list.
- **Translation (LIMIT):** only as good as your local model. The checks catch dropped numbers, links, echoes and non-translations, not a subtly wrong word. Back-translation only runs for texts up to 600 characters.
- **Launcher (LIMIT):** the Start menu (all users and yours) plus `apps.yaml`. Store apps without a Start-menu shortcut aren't found.

**Animation:** refining by word (round 10) covers speed, one colour, and size, within an hour of drawing it. Anything else ("make the ball bounce higher") is a new drawing from the model.

**Environment:** the cloud workspace has no microphone, no Windows desktop session, and only two cores. Windows behaviour is checked on your laptop when it's online.

## 8. The main chat's update courier, add-ons, groups and friends (25–26 Sep)

The full build log, with every gap A–AP and how each closed, is §20 of
`atlas/docs/UPDATE_COURIER_SPEC.md`. What's still open:

| # | Gap | Why it's open | What closes it | Who |
|---|---|---|---|---|
| 8.1 | **The signing sitting** | The release key, the Apple and Windows signing are made together, before any friend gets a copy (decided 25 Sep: key first, then copies). Apple Developer account: done (27 Sep). The release key is now a button: hub → Updates → **Make my release key**, no terminal (doc 41); its public card goes in `release-keys.txt`. | Azure: upgrade the free account to pay-as-you-go (Artifact Signing refuses free/trial subscriptions), then the identity check. The release key on the laptop, from the Updates page. | ERIC |
| 8.2 | **Updates: the hub page, and a real restart** | Voice done 27 Sep: "any updates", "install the update", "go back to the last version" (asked first; the yes is the local approval). The hub's **Updates** page built 27 Sep (doc 40), with the releaser's failure list and Hold. Still open: a real restart into a new build on the laptop. | One run on the laptop. | MAIN + ERIC |
| 8.14 | **Feedback from friends** (closed 27 Sep) | Voice and a desktop button done 27 Sep: "report a bug …" reads back exactly what goes and sends only on yes ("… without the failure" leaves the attachment out); "any feedback", "answer feedback 2 fixing"; **Report a problem with Atlas** in Atlas's own window. The hub's **Feedback** page built 27 Sep (doc 40): preview then send, and the releaser's inbox with answers. | -- | done |
| 8.4 | **Tor proven on the real network** | Proven end to end on Tor's private test network and against the real `tor` program; the build machine can't reach the public Tor network. | Two Atlases on two different networks (your laptop and a phone hotspot) become friends. | ERIC |
| 8.5 | **Arti** (AL) | Tor in Rust, compiled in with no separate program. Its onion-service hosting was still being hardened. | Test it inside Atlas; switch when it holds up. | MAIN |
| 8.7 | **Speed through Tor, measured** | Built 27 Sep: each friend's Tor connection is kept open between messages (proven with a stand-in for `tor`: four release pieces, one connection; a dead one replaced). Not measured on the real Tor network. | Time a message and a release on two real networks (with 8.4). | ERIC |
| 8.8 | **Antivirus and `tor.exe`** (AO) | Your signing covers Atlas, not Tor. | Watch for it on first installs. | ERIC |
| 8.9 | **Windows firewall rule: proven on Windows** | Built 27 Sep: the setup asks once and adds the rule (`doorrule`: only atlas.exe, incoming TCP, private and work networks, your own addresses); a no is remembered. `netsh` and the Windows prompt themselves haven't run here. | Run setup on the laptop; check Windows Defender Firewall shows "Atlas - your own devices". | ERIC |
| 8.10 | **A friend's QR code reads as text** (AC) | The phone app doesn't open a friend link directly. | The phone app registers the link. | MAIN |
| 8.13 | **Rollback covers the program, not the data** (AQ) | O1 (26 Sep) makes a failed new version go back by itself, but only the program goes back. Nothing needs more yet: the data format has never changed. | The first release that changes the data format takes a copy of the data before its trial, and rollback restores it. | MAIN |
| 8.12 | **Not yet proven on your laptop** | The Windows local-time fix; identity backup when separate profiles are in use. | One run on the laptop. | ERIC |
| 8.15 | **A held build keeps being passed on** | Since 27 Sep any friend holding a verified release hands it on (8.11 closed). `atlas release hold` stops *your* Atlas handing a build out, but isn't announced, so friends who already have it keep passing it on until a newer release is heard (which stops them). | A signed "hold" notice in the release channel, which every Atlas obeys like a release notice (it needs the release key, so it waits on 8.1). | MAIN, after 8.1 |
| 8.16 | **Bridges through a real blocking network** | Built 27 Sep: a stuck Tor switches to the bundle's obfs4, then Snowflake, then meek, and remembers what got through. Proven: real `tor` 0.4.9.12 accepts all three sets of lines and starts `lyrebird` (conn_done_pt); the switching, with a stand-in `tor`. This workspace's network lets nothing through, bridges included, so none got all the way. | Try it once on a network that blocks Tor (some workplaces, some countries), or with Tor blocked at the router. | ERIC |

## 9. After the three-chat merge (26 Sep 2026)

See `33_MERGE_2026-09-26_three_chats.md` §4–7 for the detail.

- **The third chat's work after its 25h checkpoint (NOT HERE).** At 25h, groups E, F, G, H and then D, I and J of the rulings were in progress or next. Anything it built after 25h wasn't in the folder.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **H7, phone calendar sync (unblocked).** Ruled "worked when the merge takes place".
- **H2, H13f, K (unblocked 26 Sep).** They waited on the hub design, which is now in the tree (`atlas/design/hub/`) and in the hub. H2's panels are the Panels artboard; they now paint in the hub's colourway.
- **Two seals on the activity log (DECIDED 26 Sep: keep both).** The third chat's hash chain, whose heads are checked against every backup, and our Merkle log with daily checkpoints both run on every entry.
- **The hub design (DONE 26 Sep, in part; see 4.3).** It was the pinned canvas locked 20–21 Sep, which had never been brought into the tree, so the merges took the 20 Sep mock-up (`design/command-deck.html`) for it. Eric caught it: "one of the chats when it was doing a merge still didn't pick up the hub design and was still defaulting to the old one." The canvas is now in `atlas/design/hub/`, the mock-up is in `atlas/design/superseded/`, and the hub is rebuilt to the design with Warm Paper as its default colourway. The Aa menu (Paper, Light, Dark, Auto) and Settings → How it looks (colourway, accent, colour-blind mode, density) change it.
- **Two appearance menus (DESIGN).** The hub's "Aa" menu and Settings → Appearance both apply, and "Aa" wins where they overlap. One menu would be clearer, once the hub design settles.
- **Laptop runs not yet done (ERIC):**
  - typing correction (H4), live;
  - a real call for call notes;
  - the typing re-run from doc 29.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **The GUI-free core (checked).** It still builds. The third chat's settings page and overlay draw only in the desktop build.
- **The test suite on Windows (LIMIT, now measured).** Run natively on your laptop for the first time, the `all` suite has tests that fail there on master as well as on the merge:
  - tests that stub a tool with `sh`, `cp` or `echo` (call write-ups, research fallbacks, tool timeouts, animation);
  - screen capture through `cmd`;
  - the friend tests' loopback connections;
  - animation GIFs, which need the browser.
  None is caused by the merge. They pass in the Linux gate, and they need Windows versions of their stubs.

## 7. Proposed next (ideas, not gaps)

Round 10's 17 ideas are built (round 11, `26_SESSION_2026-09-25_round11_the_seventeen_built.md`). Natural next steps from them:

- attendees kept from `.ics` imports, for meeting prep;
- receipts feeding the monthly money summary;
- a PDF page-rotate step;
- a small overlay for the launcher and clipboard history, so they can be used without speaking.

---

*MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE*
