# Session, 25 September 2026 (night): your rulings, built

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

This document grows as each group lands. The status of each group is at the top.

| Group | Status |
|---|---|
| Security (B1–B7, C) | **Built and tested**, including in a real browser |
| Correcting as you type (H4) | **Built and tested** (mock). A live run on the laptop is still to do |
| E, F, G | **Built and tested** (mock). Mail and posts still to run against your real accounts |
| H1, H3, H5, H6, H8–H12, H13 a–e, g–j | **Built and tested** (mock, and real PDFs and zips). Keys, camera and a real Defender scan still to try on the laptop |
| D (minimal), I, J | **Built and tested** |
| A, H2, H7, H13f, K | Waiting for the merge or the hub, as you ruled |

## 1. Two-factor (B1)

**Typing a code: "type my code 482917", or "it's in my email", or "it's in my texts".**

- **Codes you read out.** Atlas hears codes however you say them: "four eight two nine one seven", "482 917", "double five", and a leading zero said as "oh".
  - "Type my code for Google" is not read as a 4.
  - A code needs 4 to 8 digits.
- **From your email.** Atlas looks through today's mail on a background errand (it doesn't hold anything else up) and picks the code:
  - The code is the number in the **same sentence** as the code words. So Google's footer ("© 2026 … 1600 Amphitheatre") is ignored.
  - Prices, percentages, order numbers and years are never taken for a code.
  - Only codes from the last 10 minutes count.
  - A code from the site you're signing in to beats a newer code from anywhere else.
- **From your texts.** Atlas reads the Phone Link window, where Windows shows your phone's messages. If Phone Link isn't open, it says so instead of guessing.
- **Where the code goes:**
  - If Atlas is in the middle of signing you in, into the code box on that page. That includes the six-little-boxes kind, and never a password box.
  - Otherwise, into the window in front of you. Atlas reads the box back before pressing Enter, so a code that didn't land as typed is never sent.

**Turning two-factor on or off: "turn off two-factor on github", "turn on 2FA for my google account".**

- Atlas reads it back first ("So: turn two-factor off for github. This can also drop your SSH keys and tokens. Yes or no?").
- It only acts with you at the machine. That means your own keyboard or mouse in the last five minutes, or a voice it knows is yours. A turn from the phone doesn't count.
- It acts only on a clear yes. A mumble is a no.
- Then it goes to the site's security page in its own browser and presses the one switch labelled for that change.
  - If the site wants you signed in first, Atlas signs in with the login it holds.
  - If the site then wants a code, Atlas asks you for it, and makes the change once the code is in.
- Every change goes into the security trail with how to undo it.
- Missing site: it asks which one rather than guessing.
- The walk-through (`atlas walkthrough turn-on …`) can now turn two-factor on as well as off.
- Settings: **Security changes** and **Security switches** are on, per your ruling.

## 2. Signing in and making accounts, for real (B4, B6)

**Found while building:** both were only sentences.

- "Signing you into github.com as eric" was said once the grant checks passed, and nothing then opened a page.
- "Making you an account on x.com" was said the same way.
- The page reader behind account-making had never seen a real page. Run on its first real sign-up page, it stopped on almost everything: its "wants ID" check matched "tin" inside "marketing", "ein" inside "being", and its "wants a bank transfer" check matched "ach" inside "each". Short words now have to stand on their own.

**Now (`webrun`):**

- **Signing in** happens in Atlas's own browser, never your Chrome window.
  - It handles the one-page kind and the two-page kind (email first, then password).
  - It checks the domain on every page that loads, and fills nothing on a look-alike.
  - It recognises "wrong password" and says the password has probably changed elsewhere, rather than "it worked".
  - It hands a robot check to you.
  - It hands a code request to the two-factor route above.
  - If you've asked to be checked with first, the question now actually waits for your yes. Before, it was asked and then nothing listened for the answer.
- **Making an account.** Atlas makes the password and puts it in the vault *before* it starts, so a half-made account never loses its password. Then it goes through the form page by page:
  - Every page is read before anything on it is filled.
  - Payment or identity documents end the run for good.
  - A robot check or a code goes to you.
  - It ticks the terms box (your ruling: it may agree to terms), and leaves marketing boxes unticked.
  - Sign-in is granted on the new account, so "sign me in" works next time.
  - Setting: **Make accounts** (new), on.
- **Tested in a real headless Chromium** against local test pages:
  - sign-in, the code, and a wrong code;
  - a changed password;
  - a look-alike site;
  - a sign-up with the terms ticked and marketing left alone;
  - a sign-up that stops at a card.
- **Honest limit:** Atlas's browser runs hidden. When a robot check hands over to you, you can't click it in that browser yet; Atlas gives you the page to finish yourself. That needs a visible browser window, which comes with the hub design.

**Unused saved logins (B4).**

- A login you haven't used in three months is named in your brief, at most three at a time and at most once a month.
- Nothing is ever removed.

## 3. The rest of the security group

- **B2: wrong tokens at the hub are slowed.**
  - The first wrong token waits 0.1 s, a run of them waits up to 2 s.
  - A run of five or more is said once ("something on this machine is trying").
  - Tested against a real listener.
- **B3: grants don't lapse.**
  - What you've said Atlas may do on its own stands until you take it back.
  - The 90-day expiry is gone. It never actually ran out (the counter behind it had no writer), and now that's the rule rather than an accident.
- **B5: each way back into the vault says its weakness.**
  - Recording the ways is new: `atlas vault way-back add envelope "desk drawer"` (also `split 3 2 …`, `person …` and `keyfile …`), and `atlas vault way-back checked 1`.
  - Each way is said with its weakness and whether you'd know it had been used. The hub's security list shows the same.
- **B7: trusted devices can send notices through the hub's address**, the one a phone or tablet reaches over Tailscale. A peer token still can't open a hub page. The device list is filled in at setup, as you said.
- **C: the agreed call wording is used.**
  - When someone says no while their side is being recorded, Atlas gives you the agreed line for the chat ("Understood — recording off, and I've deleted what there was.").
  - "What do I tell them it does?" gives the agreed answer.
  - The announcement can be set by name (plain, brief, formal, casual) or as your own sentence.

## 4. Correcting as you type (H4, and your round-5 ruling on learning)

- **What happens.** Atlas reads the text box you're typing in; never a password box, never a code editor or terminal. When you pause just after finishing a word that has only one possible fix ("dont", "teh", "recieve"), it deletes the word and types the fix. It then reads the box back, and if it didn't come out as expected, it says so and leaves your typing alone for a minute.
- **You change it back:** it's left alone for the rest of that text or email. The next email is a fresh start.
- **Learning, adaptive but still smart:**
  - **One miss never teaches it.** A correction stops being made on its own only after you've changed it back in three different texts *and* more often than you've kept it.
  - **Where matters.** Change-backs in one app (a game chat's slang) stop it in that app first; everywhere else keeps it.
  - **Stopped isn't deleted.** It goes back to being offered.
  - **Everything fades.** Evidence halves every 45 days, so a correction you stopped comes back as the change-backs age, and a habit you've dropped stops counting against it.
  - **It picks up your own fixes.** If you fix the same word yourself in three different texts (type "thansk", change it to "thanks"), Atlas starts making that fix. Only close spellings of words of five letters or more count, so "form" → "from" is never learned.
- **Where to see it:** `atlas typing` says what it has learned from you and what it has stopped doing (and where).
- **Setting:** **Typing correction**, on.
- **Honest limits:**
  - It acts when you pause after a word. If you type straight on without pausing, a typo in the middle of a burst is left alone rather than reached back for.
  - Not run on the laptop yet.

## Tests

- **`tests/two_factor_and_signing_in.rs` (14):**
  - codes heard and found;
  - the right code picked;
  - Phone Link;
  - typing into the front window;
  - asking when there's no code;
  - the on/off read-back;
  - the walk-through;
  - the slowed hub;
  - the unused-login line;
  - the vault's ways back in;
  - the call wording;
  - sign-in and sign-up in a real browser.
- **`tests/correcting_as_you_type.rs` (6):**
  - fixed after a pause, not mid-burst;
  - changed back, left alone in that text but not the next;
  - stopping only on repeated change-backs, and only where;
  - fading;
  - your own fix learned on the third text, never the first;
  - never in code editors, or windows Atlas is typing in.
- **`tests/selfgrant.rs`:** a grant never lapses.

---

## 3. Atlas acting on its own (E1–E4)

- **E1, fixing its own things:** once an hour Atlas checks a named list of its own small breakages and fixes them without asking. The first is a missing scratch folder, which it remakes. Each fix goes in the log.
- **E2, starting on its own findings:** when the self-audit finds something, the diagnosis (the cause, where, and the test that proves it) goes into the work with it. Atlas takes another pass when a review raises something fixable. The change still lands only on your OK.
- **E3, long jobs:** "keep at it" carries on with a build it gave up on for up to **50 tries or 8 hours**. It stops sooner if it's going in circles, when you say stop, or when it passes. The limits are in settings under **Long jobs**.
- **E4, routines:** after Atlas has seen you do the same things three times, it asks you once, using the name you'd use: "Your usual morning setup?".
  - Concrete, ordinary routines (checking mail, showing the agenda, a backup) run on their own after that, once a day.
  - Anything that sends, researches or decides asks first each time.
  - A no forgets the routine.

## 4. What Atlas says without being asked (F)

- **F1:** It names an idea you saved and never came back to, once, after about a month ("the podcast about night markets, from 5 weeks ago").
- **F2:** "You're usually done by …" keeps re-learning, because old days fade. It's said again only when your hours have really moved (by two hours or more), and never twice in a week.
- **F3:** "It looks like Sam wants to know about the invoice. Would you like me to look for the answer and respond?"
- **F4:** "My goal is …" is kept. A goal that goes quiet for a few days gets a nudge. "I worked on …" counts as movement, and "drop the goal …" ends it.
- **F5:** There's no end-date reminder, as you ruled.
- **F6:** "This usually takes about 11 minutes" is said only for work that is usually long, and not again within two hours.
- **F7:** Your wording: "I seem to be stuck on X for Y."
- **F8:** A change to itself is raised once, "when you've got a minute", and reminded once. "Add that to the later list" keeps anything Atlas just said, and the later list comes up weekly.
- **F9:** An old correction comes up only for work related to it, and only once.
- **F10:** What Atlas can't do on this machine is said at start-up.

## 5. Atlas acting on your things (G)

- **G1, mail:** "sort my mailbox" rehearses first (how many of each, and where they'd go) and moves nothing until you say "go". It files into folders or Gmail labels and archives. It **deletes only when told**: "delete the noise" deletes that category only. "Sort my inbox" never deletes.
- **G2, posts:** a post is approved on its exact words. Atlas then asks "When should it go?" (right now, or "at 6pm", "tomorrow at 9") and schedules it. "Cancel the post" works right up to the time. It's sent in Atlas's own browser at its time. If sending fails for a reason that might clear, it tries again in 5 minutes; if it can't send, it says why.
- **G3:** "What does Discord say" looks at Discord. It never does this while you're in the middle of typing or clicking (under 3 seconds idle).
- **G4:** "Click the Export button" presses it by name. A button that can't be undone (Send, Delete, Pay…) is read back first.
- **G5:** "Move my big files" measures Atlas's own big folders (models, video work, captures) and shows the plan first. It copies, checks every byte arrived, removes the old folder, leaves a `.MOVED.txt` note where it was, and points its settings at the new place. "Where did you move …" answers.
- **G6:** "Undo" asks "Undo X?" and on a yes actually does it.
- **G7:** A multi-step job says up front which steps can't be taken back. If it stops partway, it says which of those already happened.
- **G8:** "Edit this video …" works on a copy and never writes over anything. When it's done: "Keep it?" Then: "Do you want me to get rid of the original?" The original goes to Atlas's trash, so undo brings it back.

## 6. The H rulings

- **H1, three ways in:** the wake word, **push-to-talk** and a **typing box** all work at once.
  - Hold **Tab** anywhere in Windows and speak; let go and Atlas hears it. A quick tap still types a tab.
  - **Ctrl+Shift+Space** opens a one-line box over whatever you're doing. Enter sends it and Escape closes it. The default was Ctrl+Alt+A; it no longer needs Alt, because your keyboard has none.
  - **Both keys are yours to set.** In Settings, under the new **Keys** group, press **Set by pressing** and then the key(s) you want, or type the name.
    - Or say it: "set my typing key to F9", or "set my push to talk key to caps lock".
    - "What are my keys" tells you both.
  - **The typing box takes** Ctrl, Shift or Win with any key, or on its own a key nobody types with (an F-key, Insert, Pause, Scroll Lock). **Push-to-talk takes** any single key.
  - **A key that can't work is refused before it's kept.** For example, a bare letter as the typing box key, or two keys for push-to-talk.
  - A new key takes hold when Atlas restarts, and the settings page offers the restart. If another program owns the typing-box key, Atlas says so at start-up.
- **H3, reading documents:** "read this PDF" reads it in Atlas's own code. It was tested on PDFs made by Word (via LibreOffice), Chrome, reportlab and LaTeX, and matched `pdftotext` word-for-word on real documents.
  - A scanned PDF is recognised and its page photos go through the word reader.
  - Word files (.docx) are read too.
  - A locked PDF is said to be locked.
  - "Unzip …" unpacks beside the zip into a new folder, never over anything. Names that climb out of the folder are refused, as is anything that would balloon past the limit.
  - **Every file is scanned with Windows Defender before it's opened, and everything a zip unpacks to is scanned after.** Defender only reports; nothing of yours is quarantined without you. If the scan can't run, Atlas asks "Open it anyway?"
  - A photo is named for what it looks like ("Looks like a receipt. I'd get you the total, the date and the merchant…").
- **H4, correcting as you type:** see section 2.
- **H5, other languages:** "what languages can you hear" answers from the speech model you have. The English-only model is named as the reason, with the one that fixes it. If Atlas keeps mishearing you, it says so once and names the bigger model that would help.
- **H6, a new gesture:** "teach you a gesture called rock on that opens spotify". Hold the shape up for a couple of seconds and Atlas learns it. It refuses a shape that clashes with one you already have, and says why.
- **H8, bring back what you dropped:** "drop the task dentist" takes it off the list but keeps it. "Bring back what I dropped" finds it and asks "Put it back on your list?"
- **H9, decisions:** "should I …" is worked with you, one question at a turn. When the working supports it, Atlas gives an opinion with its reason ("I'd lean toward … — …. Want the whole working?"), as you ruled: like a friend who helps. With a model it drafts the whole pass at once. "Set aside X because Y", "not now", and "back to the decision" all work.
- **H10:** When Atlas lets something go to make room, it keeps a line saying so. Asked about it later, it says "I knew something about that and let it go. It came from … — want me to look again?"
- **H11:** Your own words (names, projects) are handed to the speech model as hints once you've said them twice.
- **H12:** Old conversation is summarised rather than cut. The local model does it in the background. Anything you asked to be remembered, decided, dated or counted is added back if the summary dropped it.
- **H13:**
  - **a:** Locked, or the screen off, isn't asleep. Work carries on, and nothing that needs you at the machine happens while it's locked.
  - **b:** "Stop suggesting the morning backup" and "start suggesting it again" work. "What do you suggest on your own" lists them, on or off.
  - **c:** What you told Atlas beats what it noticed, and newer beats older.
  - **d:** "Which notes point nowhere" names them. Those are worth writing next.
  - **e:** The microphones are measured all at once (about a second in all), and each heard turn counts for or against the mic it came through.
  - **g:** "Change it to …" on a post's approval asks about the new words and learns from the edit.
  - **h:** The project each request was about is kept.
  - **i:** Sync uses the folder you set, or else a cloud folder this machine already syncs (OneDrive, Dropbox, Google Drive, iCloud). The route is logged.
  - **j:** The overnight account goes into your notes, the brief mentions it, and "what did you do overnight" reads it back.

## 7. D, I and J

- **D, asking a bigger model (minimally):** when a build is still stuck after "keep at it", Atlas writes the problem up properly: the goal, the exact error, what it tried, and the code. It sends that once to the stronger model, and checks the answer here before calling it done. The stronger model is `llm_secondary`: your personal server, when it exists. Without one, the write-up is saved for when there is.
- **I, video and creator advice** (on in settings). Ask it:
  - the colour grading order, and the project setup;
  - export settings and length for TikTok, Reels, Shorts, YouTube, X or LinkedIn;
  - what a talking head, tutorial or review needs;
  - whether a brand deal is really an affiliate deal;
  - the next step for your creator profile.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## Still waiting, as you ruled

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## Still to try on the laptop

- Push-to-talk and the typing box.
- "Read this PDF" on your own files, including a real Defender scan.
- Typing correction, live.
- Teaching a gesture on the camera.
- Mail sorting against your real mailbox.
- A post sent at its time.

## 8. The keys, installed and tested on the laptop (26 Sep 2026, early morning)

25j was installed in the live-test folder (`AtlasLiveTest-0925`). The 25h program and settings were kept in `previous-25h\`, and your other chats' folder wasn't touched. Then the keys were pressed for real, in blank Notepad windows.

**What the first run found, all fixed:**
- **The typing box took about four seconds to appear.** It showed as an empty grey window first, and the first words you typed went to the app underneath. The box now starts hidden with Atlas and is shown the instant the key is pressed.
- **After sending, the keyboard didn't come back** to the app you were in. Now it does.
- **Push-to-talk ignored keys sent by other programs.** A key remapper, which is how a keyboard without a key gets one, would have been ignored. Now only Atlas's own "give the tap back" key is let through.
- **The Atlas log was writing the same connection line four times a second** (776 KB in minutes). Each line is now written once, when it changes.
- **"What time is it" was answered "Which one?"** Atlas now tells the time and date from the laptop's clock.
- **A held key with no speech tools set up did nothing, silently.** It now says it can't hear on this machine yet and points at `atlas doctor`.
- **Keys were printed as "TAB".** They now read "Tab", "Caps Lock", "Right Ctrl" and so on.

**What was then tested and passed:**
- **Ctrl+Shift+Space** opened the box at once. "What time is it", typed 0.3 seconds after the key, all went into the box and none into Notepad. The answer came back ("It's 1:49 AM on Saturday 26 September"), and the next letter typed landed in Notepad.
- **Holding Tab** was seen as held, then let go, and nothing was typed into Notepad. **A quick tap of Tab** typed one tab, as it should.
- **`atlas keys`** is a new way to try your keys. It shows each press for a minute ("Tab held — Atlas would start listening now") without listening or opening anything. It also says when another program already owns the typing-box key.

**Honest limits:**
- Speech itself can't run in the live-test folder: it has no speech tools, so "audio unavailable". Push-to-talk is proven up to the point of listening.
- Setting a key by pressing it in Settings wasn't clicked through on the laptop.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
