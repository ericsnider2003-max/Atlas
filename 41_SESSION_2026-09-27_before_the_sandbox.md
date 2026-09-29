# Session 27 Sep 2026 (second, part three): everything before the Sandbox test

Eric: "What can you do right now before we test the sandbox? I'd rather test with as much done as possible." Then the rulings that came in during the work:
- Atlas installs like any download: no files, no command prompt, on computers, phones and iPads.
- Friends get Atlas on their computer first, then their phone by picking its type.
- Review H2 to H13f.
- The mark and its animations should be my own design, from how professionals design them.

Branch `session-0927b`.

## 1. What was built

| | what it does now | proven by |
|---|---|---|
| **The mark** (not yet applied) | Four machine-generated directions didn't land, so I drew my own. About 18 candidates were each rendered at 440, 64, 32 and 16 px, in the tray and in three colourways. I cut every one that read as a figure, a smile, a generic globe, or a letter with nothing behind it. **Meridian** survived: the letter A drawn as the top of a globe, with the equator as the part that moves. It sweeps the dome while Atlas thinks, becomes the voice waveform when it speaks, and draws in for the morning brief. It's on the Superdesign canvas with the motion live. | Eric's call |
| **Widgets** | Atlas makes a glance safe itself (`glance`, `/hub/glance.json`): today's next thing, the waiting count, and whether it's on. The home screen gets titles, scrubbed like anything leaving the laptop and cut at a word. The lock screen gets times and counts only, unless `phone.widget_titles_on_lock_screen` is on. **iPhone:** home-screen (small and medium) and lock-screen widgets, beside the Live Activity, reading what the app leaves in the app group. **Android:** a home-screen widget, redrawn only when what it shows changes. | `glance` unit tests; through the daemon (`hublive`); on a real socket with and without the token; **the Android code compiled here against Android 15's API**. Swift can't be compiled here. |
| **Your phone** (D5, P.8) | A hub page under Your devices. Pick iPhone / iPad or Android, then scan a code. **Android:** the app on the computer (`apps/Atlas.apk`), served on the computer's own Tailscale HTTPS for 15 minutes. **iPhone:** the code first collects the device's UDID through Apple's own profile service, so nobody looks it up. It's kept, and sent to whoever sends Atlas out over the paired feedback channel. There it lands in a list on their Your phone page with a **Copy** button. That list goes into `mobile/ios/devices.txt`, and the iPhone build registers new ones with Apple itself (`register_devices.py`). When a build that lists the device arrives, the page shows its install code. | The profile, the phone's reply, the challenge check, and the whole round over a real socket (`phoneadd`); the page, the picking, "no Tailscale" said plainly, a phone kept once, the releaser's list (`adding_a_phone`) |
| **The release key, from a button** (8.1) | Hub → Updates → **Make my release key**: type the vault passphrase (twice for a first one). Atlas shows the recovery key **once**, rendered directly and never put in an address or history, and a key card with **Copy**. The build reads the card from `release-keys.txt` at compile time, where a malformed key fails the build. `atlas release keygen` uses the same code. | Making the key into a vault, the card matching, a second key refused, the recovery key kept nowhere (`release`); the page in all four states (`updates_and_feedback_in_the_hub`) |
| **Trying voices** (H13f) | Sound page: every voice has **Hear**, its own sample, fetched once through Atlas, checked against its pin and kept. **Get** downloads it (the model plus its settings, pinned by the Hub's own hashes), on its own thread, with progress. A second press doesn't start a second download. Before this, only Amy could be downloaded at all. | `voicepick` (every catalogue voice pinned; Amy's pins match setup's); an ignored test fetching the real samples |
| **The phone's calendar** (H7) | The phone reads its own calendars for a week back and five weeks on, and sends them to Atlas on the phone. Atlas merges them, and removes what was deleted on the phone within that window, but only phone events. Atlas's own events, each occurrence, go back into one "Atlas" calendar on the phone, which is never read back. **iPhone:** EventKit, asked once. **Android:** CalendarContract, asked once. | `calendar` phone-sync tests (add, change, delete, a narrower window, nonsense dropped); **the Android side compiled here**; the iPhone side isn't compiled here |
| **The panels drawn** (H2, part) | **Found:** "I'm here", "show me my tasks" and "what are you doing" set `wants_panel`, and nothing ever drew it. The only reader faded it out. So the morning brief's waking mark never appeared on "I'm here". Now `panel_contents` turns that decision into the window, with the same lines the voice said. Every panel except a private knock has **Open in the hub**. | `panels_are_drawn` |
| **One file for Windows** | Handed out as `Atlas Setup.exe`, a single program. Tor now comes from setup, not the zip. It installs itself as `atlas.exe`, whatever the download was called. | `easy_setup` |
| **The Sandbox kit** | `8. Sandbox test`: a `.wsb` that opens Windows Sandbox and starts setup by itself; a checklist; and the same test on Azure for Windows Home. | Run by Eric |

## 2. Found on the way

- **Artifact Signing refuses free and trial Azure subscriptions.** It needs pay-as-you-go ([Microsoft's FAQ](https://learn.microsoft.com/en-us/azure/artifact-signing/faq)). The free account's $200, 30-day credit does cover the Azure test machine.
- **Windows Sandbox doesn't exist on Windows Home.** HP OMEN laptops often ship with Home, so the kit has the Azure route too.
- The Sound page checked for a voice relative to wherever Atlas was started from, not the install folder. Fixed while there.
- `calendar::merge_from_phone` had no caller: the guard listed it. It has one now.

## 3. Measured

| what | result |
|---|---|
| personal Atlas, `cargo test --no-fail-fast` | **34 targets, 7,054 passed, 0 failed**, 10 ignored. The first full run had 9 failures, all from this session's changes, all fixed, and those targets re-run: the guards caught a write whose failure was ignored, a hard-coded data path, a name collision, three lists that shrank (good news: `calendar::merge_from_phone`, `tts::find`, and one test-only function now have real callers), the module count, the palette, and the catalogue document. |
| the Android code (`GlanceWidget.kt`, `CalendarSync.kt`, and the edited `AtlasService`, `AtlasCore`, `MainActivity`) | compiled with Kotlin 2.0.21 against Android 15's `android.jar`: no errors, one warning that was already there (`onBackPressed`) |
| the Android and iOS resource files, `project.yml`, `ios.yml` | all parse |
| the Swift (widgets, calendar sync) | **not compiled here**: no iOS SDK on Linux. The first `ios.yml` run compiles it. |
| my mark candidates | about 18, each rendered at 440, 64, 32 and 16 px, in the tray and in three colourways; the Meridian sheet is on the canvas |

## 4. Still open

- **The mark everywhere:** waits on Eric's choice.
- **The rest of H2:** the collapsed pill, and the Ready card's actions (Implement / Read / Later).
- **Signing a release from the hub:** `atlas release sign` still needs a terminal.
- **How the Atlas program itself reaches a friend's computer** (a link, or by hand).
- **Swift:** the iPhone widgets, the calendar sync and the widget's app group are compiled first by `ios.yml`. `asc_profiles.py` now also wants App Groups on `com.ericsnider.atlas.live`, and says exactly where to tick it if it isn't.

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
