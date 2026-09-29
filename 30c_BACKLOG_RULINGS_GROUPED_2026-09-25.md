# The 145 that need your ruling, grouped into decisions

145 functions, but far fewer real decisions: most are pieces of one feature. They're grouped here into those decisions, each with my recommendation. The per-function evidence is in `30b_BACKLOG_TRIAGE_2026-09-25.md`.

**Already dealt with since the sort (3):**
- `delegate::refused`: wired, since window jobs that ask are now kept.
- `credentials::needs_you_awake`: its memory leak is fixed. Showing it is hub work, which is paused.
- `tune::mechanism_for`: blocked before because the survey was empty. The survey now has real data, so it can be wired.

| # | Decision | Items | My call |
|---|---|---|---|
| *[row removed 28 Sep 2026: trading-system material]* |
| B1 | **Refuse "turn off two-factor", and say what to do instead** | 2 | **Yes.** It only refuses; it never acts. |
| B2 | **Slow down repeated failed logins on the phone link** | 1 | **Yes.** It's a security fix on a door that already exists. |
| B3 | **Grants Atlas gave itself expire when you haven't been around** | 1 | **Yes.** It shrinks Atlas's authority; it never grows it. |
| B4 | **Password autofill gate, and reminders about unused saved logins** | 2 | **Later**, with the sign-in work. |
| B5 | **Vault-recovery wording, and read-back on security pages** | 3 | **Yes**, wording only. |
| B6 | **Atlas signing you up for accounts by itself** (enrol ×4) | 4 | **No.** It creates accounts and agrees to terms as you. |
| B7 | **Open the server to trusted peer devices** | 1 | **Later**, with the multi-device/household work. |
| C | **Call-recording consent wording**: which announcement plays, and what Atlas posts in the call chat to the others | 3 | **Your wording.** It speaks to other people. I'd use it only once you approve the exact sentences. |
| D | **Atlas asking another AI for help** (consult, handoff, the brief, sending to an online worker, overnight delegation) | 10 | **Later.** This belongs with your plan to use bigger models on your own server. Build it once, pointed there. |
| E | **Atlas acting on its own**: fixing itself unasked, feeding its own recommendations into changing its own code, long unattended jobs, running routines automatically | 8 | **No** for self-changing code. **Maybe** for a small, named set of self-fixes (e.g. recreate a missing scratch folder); you'd approve the list. |
| F | **New things Atlas would say without being asked**: "you never looked at these", "you're usually done by 6", "that looks like work mail", nudges, restriction-date countdowns, "this'll take about ten minutes", "I'm stuck, switching approach", self-change reminders, stale corrections, "what I can't do on this machine" | 14 | **Yes to 3:** the up-front time estimate, "I'm stuck, switching", and "what I can't do here" (only when asked). **No** to the rest; unprompted speech is what makes an assistant noise. |
| G | **Atlas acting on your things**: sorting your mailbox, scheduling public posts, moving your windows to answer a question, clicking buttons in other apps, moving models to another drive, actually carrying out an undo, running multi-step chains, media overwrite policy | 10 | **Yes, carrying out an undo.** Today it only asks "Undo X?" and never does it. **Yes, clicking buttons in other apps**, but only inside window jobs you started. **Later** for the mailbox and posting. **No** for moving your windows. |
| H1 | **Push-to-talk key and the quick-typing box** (global hotkey) | 5 | **Yes.** It's the fastest way to talk to Atlas without a wake word. Windows work. |
| H2 | **Floating panels and overlay** (actually drawing the panels) | 3 | **Yes**, but it's tied to the hub look, which is paused. |
| H3 | **Scanned documents, PDF text, safe unzipping** | 4 | **Yes.** "Read this PDF" is a basic ask, and the word reader is now installed. |
| H4 | **Correcting as you type in other apps** | 4 | **No for now.** It types into your apps while you work: high risk, low need. |
| H5 | **Other languages, live** (translation lines, multilingual call notes, suggesting a bigger speech model) | 4 | **Later**, unless you need it. |
| H6 | **Teaching Atlas a new hand gesture** | 2 | **Later.** The camera features need testing first. |
| H7 | **Phone calendar sync** | 2 | **Later**, with the phone app. |
| H8 | **"Bring back what I dropped"** (tasks you let slide) | 2 | **Yes.** Small, and it asks rather than nags. |
| H9 | **Working a decision over several turns** (it would lean one way, or set the decision aside) | 3 | **Your call.** It means Atlas gives a recommendation. |
| H10 | **Memory that remembers what it forgot** (keep a stub of what it deleted) | 3 | **Yes.** It stops Atlas silently losing facts. |
| H11 | **Transcription hints**: feed your own words (names, tickers, project names) to speech recognition | 1 | **Yes.** It's the cheapest big accuracy gain there is. |
| H12 | **Summarising old conversation rather than cutting it** (today old turns become "N earlier exchanges") | 1 | **Yes**, using the local model, off the tick. |
| H13 | **Laptop-lock and screen-off awareness, ear selection, sync route, voice audition, kept references, learning from your edits, which project a turn was about, fact-conflict rule, switching anticipations on and off one by one, where the overnight long account shows** | 14 | **Mixed.** **Yes:** the fact-conflict rule (newest statement from you wins), switching anticipations one by one, and screen-lock awareness. **Later:** the rest. |
| I | **Video/content-creator advice** (edit cuts, effects, colour grading, posting formats, creator profile) | 7 | **Drop**, unless you make video content. |
| *[row removed 28 Sep 2026: trading-system material]* |
| K | **Hub pages** (index page, "why this took so long", correction repeat-rate) | 3 | **Paused** with the hub. |

**If you go with my calls:**
- **Build now:** B1, B2, B3, B5, the undo and the button-clicking in G, H1, H3, H8, H10, H11, H12, the three chosen lines in F, and the three chosen items in H13.
- **Drop:** B6, I, J, and the no's in E, F and G. Dropping removes the code and its tests, so the backlog shrinks for real.
- **Park,** each tied to the work it belongs to: A, B4, B7, D, H5, H6, H7, and K.
- **Wait for your wording or your call:** C and H9.

---

## Eric's rulings, round 1 (25 Sep 2026)

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **B1:** "I want to be able to use Atlas for two factor." Asked back what that means (see round 2).
- **B2, B3, B5, B6, B7:** not explained well enough the first time. Explained again, with a question for each (round 2).
- **B3, concern:** expiry must not make Atlas useless.
- **B4:** Autofill: yes. Reminders about saved passwords for things not used: yes, but not annoying.
- **C:** Important. The wording was already agreed earlier, so it's wired with that wording and not asked again.
- **D:** Can be done, but minimally.
- **E:** Should have been asked as questions, not stated. Re-asked in round 2.
- **F onward:** not ruled yet.

## Eric's rulings, round 2 (25 Sep 2026)

- **B1:** Two-factor, options 1 and 2. Atlas types in a code you read out, and Atlas reads the code from your email or texts and types it. Also: Atlas must be able to turn two-factor on *or off* for you. That replaces the old refusal; it happens through the read-back (B5), with you at the machine.
- **B2:** Yes, slow down wrong guesses on the phone link.
- **B3:** Eric steps away for long periods, and Atlas must not break in that time. Standing self-permissions don't lapse on their own.
- **B5:** Yes, both wordings (the recovery-route weakness, and the read-back before security changes).
- **B6:** Yes, Atlas may create accounts (payment or ID stops it; a robot check hands over to Eric).
- **B7:** Yes, trusted devices may send notices. The list of devices gets filled in later, at setup.
- **E1:** Yes, Atlas fixes its own things without asking.
- **E2:** Yes, Atlas starts improving its own code from its own findings (the change still lands on Eric's OK).
- **E3:** Yes, long unattended jobs, with a limit that isn't ridiculously small.
- **E4:** Routines doing a concrete, normal thing (adding to the calendar, a report) run automatically. Abstract ones ask first ("Your usual morning setup?").

## Eric's rulings, round 3 (25 Sep 2026)

- **F1:** Yes, name ideas you saved and never revisited.
- **F2:** Yes, but it must stay adaptive. Eric works a lot of random hours, so the turn-over time keeps re-learning rather than fixing once.
- **F3:** Not "X looks like work". Instead: "It looks like X wants to know about Y. Would you like me to look for the answer and respond?"
- **F4:** Yes, nudges toward goals you set.
- **F5:** No end-date reminder.
- **F6:** Yes, time estimates, but not annoyingly.
- **F7:** Wording: "I seem to be stuck on X for Y."
- **F8:** Yes. Eric can also tell Atlas to add a recommendation to a list for later, so it isn't forgotten.
- **F9:** Yes, but only corrections related to what's being worked on.
- **F10:** Say it at start-up. There's no reason to talk to Atlas if it can't hear.
- **G1:** Yes, sort the mailbox. Delete only when told to.
- **G2:** Yes, schedule posts.
- **G3:** Yes, but never while Eric is in the middle of something.
- **G4:** Yes, click buttons in other apps by name.
- **G5:** Yes, move big files, as long as things stay findable and organised.
- **G6:** Yes, actually carry out an undo.
- **G7:** Yes, say which steps can't be undone.
- **G8:** Make a copy, then do the work. Once the work is done and approved, ask whether to get rid of the original, and do what Eric answers.

## Eric's rulings, round 4 (25 Sep 2026)

- **H1:** Wake word, push-to-talk *and* a typing box, all three. The keys are configurable per user; no key combination is locked in.
- **H2:** Wait for the hub. The panels get designed around the hub's theme and Eric's transparency preferences.
- **H3:** Yes, PDFs and scans. Atlas unzips when needed, and scans for viruses before opening anything (Windows Defender on the file, and on everything a zip unpacks to).
- **H4:** Yes, correcting as you type in other apps.
- **H5:** Yes, other languages live.
- **H6:** Yes, teaching Atlas a new hand gesture.
- **H7:** Phone calendar sync is worked when the merge takes place.
- **H8:** Yes, "bring back what I dropped".
- **H9:** Yes. Atlas gives reasoning or different paths to the same outcome. Where that doesn't make sense, opinions are allowed: "Atlas is supposed to be like a friend that does work for me and helps."
- **H10:** Yes, memory keeps a stub of what it forgot.
- **H11:** Yes, transcription hints from Eric's own words.
- **H12:** Yes, summarise old conversation, as long as it doesn't forget the important things.
- **H13a–e, g, h:** Yes. H13e must be quick.
- **H13f:** Held until the merge, because it goes with a hub design Eric is working on.
- **H13i:** The sync route is the best route available.
- **H13j:** The overnight long account is a note, briefed with Eric's briefs, or given on request.
- **I:** Keep it. Eric makes video content.
- **J:** Keep it.
- **K:** Yes, and it waits for the merge.

### Where every group stands after four rounds

- **Build now:** A (the split is shown first), B1–B7, C, D (minimal), E1–E4, F1–F4 and F6–F10, G1–G8, H1, H3–H6, H8–H12, H13 except f, I, and J.
- **Dropped:** F5 (no end-date reminder).
- **Waiting for the merge or the hub:** H2, H7, H13f, and K.

## Eric's ruling on H4 (25 Sep 2026)

- **H4:** Option 1. Atlas fixes mistakes in place as Eric types in other apps, with undo. If Eric goes back and changes a fix, Atlas does not fix that spot again in that text or email. The ruling limits this to that one text or email; it says nothing about other texts.

## Eric's rulings, round 5 (25 Sep 2026)

- **H4, learning:** "I would still like Atlas to learn so it gets better at these kinds of tasks. But if it learns too well then it will stop working in general so it needs to make learning adaptive but still smart." A changed fix stays alone in that text or email, and each change also counts as evidence about that correction in general. Atlas stops a correction only on repeated evidence, and one miss never teaches it. It keeps its general rules and never overfits to one case. Its lessons fade if the evidence stops, and it can come back to a correction after it has stopped.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## H13 lettering, confirmed from Eric's screenshot (25 Sep 2026)

The letters, as they were asked:

| Letter | What it is | Ruling |
|---|---|---|
| a | Atlas knows when the laptop is locked or the screen is off, and keeps working rather than treating that as sleep | Yes |
| b | Turn Atlas's individual suggestions on or off by name ("stop suggesting the morning backup") | Yes |
| c | When two things Atlas knows disagree: what you told it beats what it noticed, and newer beats older | Yes |
| d | Say which notes point to things that don't exist yet | Yes |
| e | Pick the best microphone automatically by listening to each | Yes, and it has to be quick |
| f | Try out voices before downloading one | **Held until the merge** (goes with Eric's hub design) |
| g | Learn your style from how you edit Atlas's drafts | Yes |
| h | Track which project each request was about | Yes |
| i | How two devices sync | The best route available |
| j | Where the long overnight report shows | A note, included in your briefs, or on request |

This corrects the guess in the 25h checkpoint: "which project a turn was about" is H13h (build it), and the voice try-out is the one held.
