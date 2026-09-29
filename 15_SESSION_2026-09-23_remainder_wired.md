# Session — 23 September 2026 (later): the rest of the decision list

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

Eric's ruling: "Continue with 1/2 — spin up for #2 if that's the best option.
For 1 I genuinely don't know what it should read; it's been so long since
this was designed."

## #1 — the last two trading primitives: what they were designed to read

Neither got a new trading rule. I went back to what the original design said
each one was *for*, and built exactly that reader: something that describes
the market and decides nothing.

**`bars::back_to`.** Its own doc comment names its use: *"a break reader
asking what structure looked like before the move it is judging."* It gives
you a view of the bars that stops at an earlier bar and can't be pushed
forward. So the new `market::structure::before_the_turn` does this: when the
structure reports a break, it reads the same structure through a view
bounded at the bar just before the swing that broke. Only pivots that were
confirmed by that bar count, so it is what could actually have been known at
the time, not a redraw with hindsight. `atlas market` prints it under the
structure line, for example:

> before the break at bar 338, as it could be read then: trend RANGE; …
> MAY be turning DOWN — lower high at 1.02860, but the low at 1.02440 is still
> higher — a pullback until that gives way

That shows the before next to the after: at that bar it was only a warning,
and then the low broke. It makes no call.

**`events::is_window`.** Its doc: *"The BoJ announces somewhere in an
hour-wide band rather than at a timestamp, and the delay is itself
information… Modelling it as a point would be precise and wrong."* The
stand-down explanation (`standdown::Blackout::plain`) now says so whenever a
release in the blackout is banded: *"BoJ … has no fixed time — it lands
somewhere in its band, so there's no 'after the print' until it has actually
spoken, and a late statement is itself information."* A release with a fixed
time is still read as a point. This changes the wording only. The blackout
interval already used the band's end.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
`standdown.rs` and `market/structure.rs`. These were **not** updated, because
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
provenance manifest (`vendor_drift` green), but they now lag personal Atlas
by these two readers. That gap is deliberate and is noted here for when
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## #2 — B6 finished

**Research's headless-browser fallback (`cdp::links`).** Here is the problem
it solves. The research search step fetches DuckDuckGo's results page with
curl, and search engines often send curl a page with no links in it (a
JavaScript-only page or a challenge). When that happened, research used to
fail with "no sources found". Now, when the curl copy has no links, research
starts the **headless** Chrome from the `browser:` block. That is a separate,
invisible Chrome with its own profile, and it never touches your windows. It
opens the same search URL (taken from the search tool's own arguments, so
there is no new config key), waits for links to render, reads them
(`Browser::links` → `Cdp::links`), and runs them through the existing noise
filter. DuckDuckGo's click-through redirects (`/l/?uddg=…`) are unwrapped to
their real targets. If that fails too, you get the original error with "(the
headless browser found nothing either)" added. Page fetching was already done
through headless Chrome (`--dump-dom`); search was the only step that relied
on curl alone.

**"Argue the other side" is now a real intent (`otherside`).** Say "argue the
other side: I'm selling the car for good because everyone says EVs are the
future", "talk me out of the new laptop, it costs £1400", "devil's advocate",
"make the case against…", or just "argue the other side" (which argues the
last thing you said). Atlas finds the decision in your own words. It reads
irreversibility from phrases like "for good", "permanent" or "no going
back", cost from "costs …", and dependencies from "as long as …". It never
makes up history. Then it gives the strongest case against and what would
answer it, and **names the angles it could not argue for lack of evidence**
(`not_raised`, through `Angle::needs_evidence`), for example "I didn't argue
that you've tried this before — I'd need something to point at, and I don't
have it." Weaker phrases like "what's wrong with" only count when a decision
follows them, so "what's wrong with the printer" is still a fault report. It
only runs when you ask, and never for a guest. The check sits above
reference resolution, because "the other side" contains "the other", which
the resolver used to answer with "Which one?". The gate caught that.

**Hub "on but doing nothing" banner (`settings::idle_but_on`).** The settings
page now names switches that are on while the capability behind them is
Blocked (wake, endpoint, dictate, OCR, translate…). Keys were paired with
capabilities only where the code shows the same subsystem on both sides.
`voice.enabled` (which covers two capabilities), `recall.semantic` (which
still works on words without a model) and `watching.enabled` (a different
feature from `watch`) were left out on purpose, with the reasons written in
the source.

## Guards

`TEST_ONLY_MAX` 263 → 256, which is exactly the seven that got real callers
(`bars::back_to`, `cdp::links`, `events::is_window`,
`otherside::against/is_asked_for/needs_evidence`, `settings::idle_but_on`).
`ORPHAN_METHODS` 13 → 8. `NAME_COLLISION_ONLY` 124 → 123
(`capability::blocked` gained a qualified caller in the hub).
`otherside::spoken` is no longer an ambiguous bare name. Every change is in
the honest direction, and nothing grew silently.

## What remains on the ruling-gated floor

`crew::ask_to_stop` stays parked, because B1 is pause-not-erase and needs
resumable errands (doc 13 §3). `server::with_peers` stays parked, because B5
waits on your "another way" choice for reaching the server (doc 13 §2).
`language::good_enough` and `scheduler::cancel/active` are unchanged from
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## One flaky test found and fixed

The gate's first run had one failure:
`a_message_with_a_recipient_and_no_words_asks_rather_than_sending_nothing`.
On repeat runs it failed about once in 12. The cause was old: three messaging
tests in `the_branches_that_do_damage.rs` read the handover flag, which lives
in a single file the whole process shares, but they didn't hold the file's own
`alone` lock. So when a handover test ran at the same moment, it handed the
install over partway through the test. I wrapped all three in `alone`, and
they then passed 15 runs out of 15.

## Crew

Two crew agents drafted in isolated copies with no compiling: the hub banner
and the research fallback. The main thread wrote the otherside intent and
both market readers, then merged, compiled and gated everything serially.
Each file had a single owner, so there were no merge collisions this pass.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
