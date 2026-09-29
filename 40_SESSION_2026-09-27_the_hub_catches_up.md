# Session 27 Sep 2026 (second, part two): the hub catches up

Eric: "The hub should have been included in the 'awaiting merge' folder." It was. Every branch in `Awaiting Merge\_for_merge\atlas-current-all.bundle` (`whole-hub`, `phone-polish`, the tag `snapshot-2026-09-26-hub-design`) is already an ancestor of `session-0927b`. The one exception is `atlas-project-chat`, 4 commits that imported chat notes, and its content is covered by the lost-work check. So the hub was never missing. What was wrong was treating it as paused. Doc 39 had parked the hub pages for everything built that day, so this part builds them. Branch `session-0927b`.

## 1. What was built

| page | what it does | proven by |
|---|---|---|
| **Updates** (Your devices, new) | Shows which Atlas this is, what's been heard, a waiting build, **Install**, and how updates arrive (the usual, by themselves, ask first, off; the choice is kept where the courier reads it). **Go back to the last version** is a link to a question and never a single press: it goes back only on the confirm button. Every action is refused while the laptop is handed over. On the releaser's Atlas it also shows the builds friends reported failing, what each device wrote down, and **Hold this build**. Only you can hold, and only a build someone reported. | `tests/updates_and_feedback_in_the_hub.rs` |
| **Feedback** (Your devices, new) | Write it, then see exactly what will be sent (with what a failed update wrote down, if one failed here), then **Send** or **Change it**. Pressing Send twice sends once. On the releaser's Atlas, the reports that came in, each with an answer box. | the same file: a friend's preview-then-send, and the releaser answering |
| **Partners**: online or offline | **Online** means heard in the last 15 minutes. Otherwise **Offline · last heard …**, or **Paired · not heard from yet**. Atlas now notes every answer a paired device gives (`PeerLink`) and every paired device that reaches this door (`SignalListener`), and keeps both in `peers_reached`. | Partners rows, including the 15-minute edge; a real door noting who came |
| **Documents**: Send and a share log | A Send column (who, then **Send**). It goes through the same sealed hand-over as "send this to Sam": the file if there is one, the text if not. It's refused while handed over. The item records who it went to and when, so it shows **Sent to Sam · …** instead of Private. | A document sent to Sam over real sealed sockets, arriving, and logged |
| **Connections**: the phone's model | In an app built with the model: its state and **Get this phone's own model**. | rendered in the accessibility pass |
| **`autocomplete`** (P.6, closed) | Every text field has one. Your own email, name and phone get their token. Fields that hold someone else's details (a client's) or no personal data get `off`, so the browser doesn't offer your details in a client's form. | A test walks every hub page and fails on any text field without the attribute |

Plus: both new pages are in the palette ("Check for updates", "Report a problem with Atlas"). The Friends page already said which Tor bridges are in use (from doc 39).

## 2. Measured

| what | result |
|---|---|
| personal Atlas, `cargo test --no-fail-fast` | **34 targets, 7,027 passed, 0 failed**, 9 ignored. (The first run had one failure, the wording-only guard in §2 below; fixed, then that target rerun.) |
| Windows, `cargo build --release --target x86_64-pc-windows-gnu` | clean, no warnings; `atlas.exe` sha256 `c96a9d00…a5b03409`, in `Atlas-for-Windows-0927b.zip` with `tor\` |
| the new hub test file | 9 passed (plus 1 ignored test that renders the pages for the accessibility check) |
| axe-core 4.13 (WCAG 2.0/2.1/2.2 A and AA), headless Chromium | **0 violations and no sideways scroll** on Updates, Updates-confirm, Feedback, Feedback-preview, Documents, Partners and Connections, in Warm Paper, Ember and High Contrast, at 320×640, 390×844, 844×390, 768×1024, 1024×768 and 1440×900 (126 runs) |

**Found by the guards:** two of the new tests at first only checked that a page contained some words (`retrospective` flags wording-only tests). They now also check the behaviour: the chosen update mode is stored where the courier reads it, and "online" is cut off at exactly 15 minutes.

## 3. Still open

- **Invoices on Business Overview.** There's no invoice module.
- **A look on the laptop, phone and iPad** (4.3). Nothing here has been seen on a real screen.
- **H13f (trying voices before choosing)** waited on the hub. It can now be scheduled.

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
