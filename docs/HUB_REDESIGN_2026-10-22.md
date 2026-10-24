# Hub redesign — 22 Oct 2026

Owner: Eric. Approved on 22 Oct 2026: "Have the claude code session use whatever skill plugin or github repo needed for the redesign. I am tired of low quality results."

Branch: `feat/hub-redesign-2026-10-22` (worktree `wt-hub`).

## The standard: Jarvis, EDITH and Donna

Eric's words:
- "I am not looking for a cheaply or poorly created assistant, I am looking for an AI Donna."
- "My vision is a Jarvis (iron man)/ eve (spider man)/donna (suits)."

Every page is held to these points:

1. **It already knows.** The first screen says what needs Eric, what failed and why, what is running, what is queued and why, and what got done. He never has to ask.
2. **It never contradicts itself.** Every surface draws on one record of the work: Home, Now, Outstanding, the header count and the phone's `live.json`. A test fails the build if two of them disagree (`atlas/tests/the_status_is_one_truth.rs`).
3. **It is honest about time.** Queued work is never called working. A view taken while Atlas is busy says "how things stood a moment ago". On the phone, laptop work says "queued until the laptop is reachable" (`docs/PLATFORM.md`).
4. **It is quick on a bad phone.** First view in under 1 s, page weight under 30 KB, no framework, and live updates that ask for a few bytes and only fetch when something changed.
5. **It is discreet.** One loud thing at a time, and empty states shrink to one quiet line.
6. **It never makes Eric explain twice.** His words from past sessions are kept here and in `docs/PLATFORM.md`.

## Why (audit, 22 Oct, measured on a fresh profile)

- **The status pages disagreed.** Home said "Working", Now said "Nothing underway" and Outstanding said "In progress 2", all about one self-repair run that was really waiting for memory. Each page read its own source.
- **Empty lanes filled the phone.** Outstanding's first phone screen was four empty lanes, and the one real item sat below the fold.
- **Pages were heavy and slow.** Every page was about 95 KB, and 60 KB of that was the same stylesheet, pasted in, never cached and never compressed. On slow 3G with a 6× slower CPU, Home took 1.8 s to first paint and 2.4 s to load; Settings took 4.0 s.
- **There were too many links.** Each page carried 150 links across the sidebar, two folded menus and a phone menu.

## Page map: six sections, one job each

Nothing was removed or switched off. Every page keeps its address and is reached through its section's row of tabs.

| Section | Job | Pages in it |
|---|---|---|
| Home | What is happening | Home, Now, Outstanding, Calendar, History, Activity |
| Talk | Ask Atlas anything | Talk |
| Projects | Your work | Projects, Documents, Board, Opportunities, Give, Social |
| Messages | People | Messages, Friends, Groups, Trusted |
| Business | Each business | Overview, Shared tasks, Clients, Partners |
| Settings | Atlas itself | Settings, Sound, Gestures, Permissions, Access, Accounts; Devices and connections; About Atlas |

The phone keeps the five tabs Eric chose on 26 Sep (Home, Projects, Messages, Business, Settings), with Talk as the microphone in the top bar.

## Free references used (licences checked 22 Oct 2026)

| Reference | Licence | What was taken |
|---|---|---|
| GOV.UK Design System, task list and tag patterns | Content OGL v3.0; govuk-frontend code MIT | Each state named in a plain word beside its colour; sentence-case labels |
| IBM Carbon, status indicator pattern | MIT | Never colour alone: each state has a shape (icon), a word and a colour |
| GitHub Primer, StateLabel accessibility | MIT | Contrast targets: 4.5:1 for the label text, 3:1 for the icon |
| MDN, `aria-live` and conditional requests | MDN docs | `aria-live=polite` on the live lanes; a small "changed?" check before any fetch |
| Ink & Switch, "Local-first software" (2019) | Essay (licence not checked; only ideas used) | The phone works on its own first and syncs later |
| Alex Russell, "The Performance Inequality Gap" | Blog (ideas only) | Budget for a low-end phone |

No code was copied from any of them. htmx and idiomorph (0BSD) were considered and not used: the existing 2 KB `live.js` already covers the need.

## Implementation (commits on the branch)

1. **One work record.** `hub::Open` gains a Queued lane. A conversation's work joins it. Home's NOW row only says "Working" when something actually runs. Now falls back to the record, and `live.json` carries it.
2. **Speed.** The stylesheet is one file named by its hash and cached for a year. Pages and the stylesheet are gzip-compressed, using the deflate and CRC code Atlas already has.
3. **Six sections.** The sidebar shows the six. Each page has its section's tabs. The phone's all-pages menu is gone.
4. **Home leads with the work.** The live lanes show needs you, failed or stopped, working, queued and done today. Empty lanes become one quiet line. Home also answers from the busy-time snapshot.
5. **Plain words.** "This is the latest completed status. Pause requests reach active workers…" became "I'm busy, so this is how things stood a moment ago. Buttons act when I'm free."

Results, screenshots and gate outcomes: `verification/runs/20261023T000000Z-claude-hub-redesign/RESULTS.md`.
