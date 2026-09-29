# Session, 25 September 2026 (last): a no comes with a way to yes, and the four calls, thought through

Eric: *"I agree with all the thing built into Atlas. If one of them says no it also needs to suggest what would make it ok, then it gets retested and re-presented for should this be built and is this safe. The ones that need my call — yes, but make them better and more thought through."*

"The ones that need my call" are the four named in doc 27, §3:

1. grading recorded model calls
2. a tamper-evident activity log
3. search that measures itself when its model changes
4. notes about code that flag themselves when the code moves on

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## 1. A no comes with a way to yes

**What changed.** In the two rooms from doc 27, "Should I build this?" and "Is this safe?", a seat that says no is now asked to end with one line: *"It would be OK if …"*. That line has to name a change someone could actually make.

Then Atlas:

1. says the first look ("First look — 2 for, 3 against …");
2. names any seat that said no without saying what would change it ("reach said no without saying what would change it"). A no like that isn't counted as a good answer when the call is graded (§3);
3. lists the conditions ("It would be OK if — money: three people pre-paid; trust: nothing leaves the laptop");
4. rewrites the proposal with those changes in it, noting who asked for each ("The proposal has since been changed so that: … Judge it as changed");
5. puts the changed proposal to **both** rooms, blind: *Should it be built?* and *Is it safe?* Both, because fixing one worry can open another. Pre-payment fixes the money seat, but it may mean taking card details, which the safety room needs to see;
6. says each room's answer ("Changed that way — Should it be built? 5 for … Is it safe? 4 for, 1 it depends …").

If the retest raises new conditions, it goes round again, **at most twice** (`council::MAX_RETESTS`). Whatever is still a no after that is said as yours to decide: "After 2 rounds of changes some still say no unless … — that one's yours to decide." A stop or pause reaches it between passes.

**Thought through.**

- **The cap is two.** Each pass is ten model calls. The rooms also don't get more reliable by being asked the same thing repeatedly: they get more agreeable.
- **Retests are blind.** The open round (seats reading each other) only runs on the first look, when the room is split. A retest where seats saw each other would converge on whatever the first seat said.
- **Conditions collect across passes.** A seat can't ask for something that was already granted and have it counted as new.
- **The general room is unchanged.** "Ask the room" questions that aren't about building or safety don't retest. There's no proposal there to change.
- **Limit:** the change is made *in words*, to the question. Atlas judges the proposal as if the change were made, not a proposal where it actually was. Making the change is still yours.

## 2. The activity log is tamper-evident

**What changed.** Every entry in the activity journal (what Atlas did as you, published, blocked and so on) now carries:

- its place in the whole history (`seq`);
- the seal of the entry before it (`prev`);
- its own seal, a SHA-256 over `prev`, `seq`, the time, the kind, ok, and the words.

Change a word in any entry and its seal stops matching. Remove or reorder one and the next entry's `prev` no longer points at it.

Each save also appends the current head (`seq`, seal, time) to **`activity-anchors.jsonl`** beside it, a file that is only ever added to. A log that was rewritten from scratch, with every seal recomputed, still chains perfectly. But it no longer contains the head that was written down, so it shows. So does a log with entries cut off the end.

**Where you see it:**

- **`atlas journal check`** says "intact, N sealed entries" or the first place it isn't, with when that entry was written.
- **`atlas doctor`** has an "activity log" line.
- **At start-up,** a broken log is said once.

**Thought through.**

- **The log is still capped at 400 entries.** When old ones drop off the front, the seal of the last one dropped is kept (`base`), so the first remaining entry still has something to chain to.
- **Entries from before today** have no seal. They're skipped and counted, not failed.
- **Doc 27 suggested signing with a key (ed25519). I didn't, on purpose.** The key would live on the same laptop as the log, so anything that can rewrite the log can read the key and re-sign it. A signature would look stronger than it is. A hash chain plus a head kept somewhere else gives the same protection honestly.
- **Honest limit:** someone with access to your files could rewrite both the log and the anchor file. What catches that is a copy they can't reach. Your backups already copy the state folder, anchors included, so an old backup's anchor file checked against today's log (`atlas journal check` reads whatever anchors are beside the log) is the off-machine check. Sending the head to your phone or the VPS each day would close it properly; that's a later step and needs your say on where it goes.

## 3. Model calls are graded

**What changed.** Every recorded model call now has a number. When something that already happens shows whether a call was good, that's written down as a grade, with the reason when it's bad:

| Call | Good when | Bad when (the reason kept) |
|---|---|---|
| Writing a window reply | it passed the read-back as written | the read-back found a fault (chatbot voice, a blank, filler…) |
| Its rewrite | the rewrite was kept | "rewrite was no better" |
| A council seat | it took a side and gave a reason | "wouldn't commit"; in the two rooms, "said no without saying what would change it" |
| Research | every figure is in its sources | "stated a figure that isn't in its sources" |
| Anything you correct | | "you corrected it" |

Grades are written to **`model-grades.jsonl`** beside the call log, one line each. They are never written into the log itself, which is only ever appended to. They're laid over the calls on loading.

**`atlas trace grades`** gives a scorecard per kind of call: how many were graded, how many good, a 95% range for the true rate (Wilson), and the commonest reasons for bad ones. Under 30 grades, it says "N of M good so far — too few to call a rate" rather than a percentage you'd over-trust.

**Thought through.** The fine-tuning plugin's rule was "no eval harness, no fine-tune". This is the harness's first half: labelled outcomes, per kind, with reasons. The reasons are the failure categories the plugin says to find by hand, collected as they happen.

**Honest limit:** the trace still stores **no prompt or reply text** (`trace::STORES_NO_CONTENT`), so these grades can't be turned into training rows yet. They can tell you *which kind of call is failing, how often and how*. They can't give a model examples to learn from. Keeping the text would be a change to what Atlas stores about you, and it's yours to rule on. If you do, it should be opt-in per kind (window replies, say) with personal details scrubbed first.

## 4. Search measures itself when its model changes

**What changed.**

- **The meaning model has a fingerprint:** the encoder program, its arguments, and the size and date of any model file. Swap the model and the fingerprint changes.
- **The stored meaning vectors remember which model made them.** A different model clears them all, and the background backlog re-reads every note with the new one. Before, vectors from two models could have been mixed in one index, which gives numbers that look like similarity and mean nothing.
- **When re-embedding finishes, a search check runs** as a crew errand ("the search check": it can be stopped, paused between passes, and named in "what's queued"). It also runs on a fresh install once the notes are embedded.
- **The check asks two sets of known questions.** One is yours, in `search-questions.txt` in the state folder, one per line (`what you'd ask => the note's title`). The other is made from the notes themselves: each note's most informative sentence, with its title words taken out and every third word dropped, roughly how a half-remembered detail comes back.
- **Each question is searched by words alone, and by words plus meaning.** The check scores whether the right note is in the top five, the first-place score (MRR), and nDCG@5.
- **It says the result plainly:** "…by words, the right note is in the top five 82% of the time … With meaning as well: 91%, meaning finds 9 points more." If meaning makes it worse: "…worth a look." Against the previous model, it says up or down, by how much.
- **The last 50 checks are kept.** `atlas search check` runs one on demand, using only vectors already made with the current model.

**Thought through.** It measures the thing that matters: does meaning search actually find more than words alone, on *your* notes? It isn't measured on a benchmark.

**Honest limits:**

- **Questions made from the notes favour word search,** because they're built from the notes' own words. That's why your written questions come first and are marked as yours. Ten written questions of the sort you really ask are worth more than the forty made ones.
- **With no encoder installed there is nothing to compare,** so the automatic check doesn't run. `atlas search check` still measures words alone and says why meaning wasn't measured.

## 5. A queued code change knows when the code has moved on

**What changed.**

- **When Atlas starts working on a change for one of your projects** ("improve the date parser in diary"), it takes the fingerprint (SHA-256) of each file the change could write, *at the start*, as the model reads them. It doesn't wait until the change is finished. So an edit you make while it's being written still counts as the code moving on.
- **The change carries those fingerprints in the queue.** A file that didn't exist yet is fingerprinted as "absent", so one that appears since also counts.
- **Hourly,** waiting changes are checked against the files as they are now. A change whose file has changed is marked out of date and said once: "The 'date parser' change for diary is out of date — dates.rs changed since I wrote it. Ask me to redo it against the current code before implementing it." If the file is put back as it was, the mark clears.
- **"What's queued"** names out-of-date changes.
- **"Implement …" checks again at that moment**, not from the hourly mark. An out-of-date change is refused and **nothing is written**: "I didn't apply 'date parser': dates.rs changed since I wrote it, so it was written against code that isn't there any more and would overwrite the newer version. Ask me to redo it against the current code."

**Thought through.**

- **Whole-file fingerprints, not git,** so it works on a folder that isn't a repo, and needs no model and no git.
- **A change anywhere in the file counts.** A line-level check would miss a function the change relies on being moved.
- **Changes queued before today have no fingerprints** and are never called out of date.
- **An unreachable folder leaves the mark as it was** rather than guessing.

**Honest limit:** it watches the files the change *writes*. If the change depends on another file (a type it uses, say) and only that file changes, this won't see it. The project's own tests, which Atlas runs when it verifies a change in place, are what catch that.

## Also fixed

**"What's queued" never mentioned a window being worked.** The line about windows was added to the answer after the answer had already been put together, so it was dropped. Found while wiring the out-of-date changes into the same answer. Both are now said.

## Tests

`tests/a_no_comes_with_a_way_to_yes.rs` has 22 tests:

- **The rooms:**
  - both rooms ask a no for its condition, and the general room doesn't;
  - a condition is read, and a flat no is named;
  - a no with a condition is changed and put to both rooms (ten seats judge the changed proposal), and every seat's call is recorded and graded;
  - a room that's never satisfied stops after two passes and says it's yours: 5 + 2×10 calls, no more.
- **The log:**
  - an untouched log checks out;
  - an edited, removed or reordered entry shows, with when it was written;
  - a log rewritten with fresh seals disagrees with its recorded head;
  - entries cut off the end show;
  - 450 entries rolling down to 400 still check out;
  - old unsealed entries are skipped;
  - saving writes the head once per change, and the check reads it back.
- **Grades:**
  - kept beside the log and back after a reload, with no prompt or reply text in either file;
  - the scorecard says under-30 as "too few", gives a Wilson range, and ranks the reasons.
  - The window-reply and research tests from doc 27 now also check their grades.
- **Search:**
  - vectors from another model are forgotten;
  - a replaced model file is a new fingerprint;
  - questions are made and read correctly, and scored;
  - the spoken result compares meaning with words and with the previous model;
  - the check runs as a named crew errand and is kept.
- **Stale changes:**
  - a change applies while its file is as it was;
  - it's refused once the file changed, marked once not hourly, and clears when the file is put back;
  - a file that appeared since counts;
  - an old change with no fingerprints is never out of date;
  - end to end, Atlas refuses, writes nothing, says why, and "what's queued" names it.

## Still open, and yours

- **Keeping prompt and reply text** so grades can become training rows (§3): opt-in, per kind, scrubbed.
- **Where the log's daily head should go** off the laptop (§2): phone, VPS, or just the backups.
- **The picture reader's 3 GB download**, live typing and call tests on an unlocked laptop, the envelope question, and the hub (paused) are unchanged.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
