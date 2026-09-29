# Atlas (personal/business) — full module reference

Generated 28 September 2026, from the source tree itself: every file's own
module-level documentation comment, its public interface, and whether
anything outside the file actually calls into it. Nothing here is written
from memory or guessed at — the description under each heading is the
developer's own words at the top of that file, and the wiring line is a
direct check against the rest of the tree, cross-referenced against
`tests/wiring.rs`'s own maintained list of known-unwired modules.

Of 415 files: **413 wired, 0 not wired,
2 module declarations** (`mod.rs` files, which declare a tree rather
than hold a capability).

The wiring column is taken straight from `tests/wiring.rs`'s maintained
`UNWIRED_BASELINE` rather than recomputed here: recomputing it needs
`strip_config_only`'s rule, and a second copy of that rule would drift from
the one the test enforces.

**Read this alongside the numbered session documents** at the top of the
handoff (`00_START_HERE.md` lists them) for what changed and why, and
`OUTSTANDING_2026-09-26.md` for what's left. This document is the map; those
are the story.

**415 files.** Organized by directory, then alphabetically within it —
`src/lib.rs` and `src/main.rs` first (the two entry points), then
`src/market/*` together, then everything else in one alphabetical run, then
`src/platform/*` together at the end.

## Quick index

| File | Wired |
|---|---|
| `src/lib.rs` | yes |
| `src/main.rs` | yes |
| `src/market/bars.rs` | yes |
| `src/market/claims.rs` | yes |
| `src/market/events.rs` | yes |
| `src/market/feed.rs` | yes |
| `src/market/fixtures.rs` | yes |
| `src/market/levels.rs` | yes |
| `src/market/mod.rs` | — |
| `src/market/multiframe.rs` | yes |
| `src/market/params.rs` | yes |
| `src/market/regime.rs` | yes |
| `src/market/session.rs` | yes |
| `src/market/structure.rs` | yes |
| `src/market/time.rs` | yes |
| `src/market/timeframe.rs` | yes |
| `src/market/verify.rs` | yes |
| `src/accounts.rs` | yes |
| `src/activity.rs` | yes |
| `src/adapt.rs` | yes |
| `src/addressing.rs` | yes |
| `src/afterme.rs` | yes |
| `src/agefile.rs` | yes |
| `src/android.rs` | yes |
| `src/answering.rs` | yes |
| `src/anticipate.rs` | yes |
| `src/appearance.rs` | yes |
| `src/asia.rs` | yes |
| `src/asking.rs` | yes |
| `src/astype.rs` | yes |
| `src/attention.rs` | yes |
| `src/audio.rs` | yes |
| `src/automation.rs` | yes |
| `src/awake.rs` | yes |
| `src/awareness.rs` | yes |
| `src/b64.rs` | yes |
| `src/backends.rs` | yes |
| `src/backlog.rs` | yes |
| `src/bandit.rs` | yes |
| `src/bm25.rs` | yes |
| `src/booking.rs` | yes |
| `src/brain.rs` | yes |
| `src/brief.rs` | yes |
| `src/browser.rs` | yes |
| `src/budget.rs` | yes |
| `src/build_it.rs` | yes |
| `src/calendar.rs` | yes |
| `src/callnotes.rs` | yes |
| `src/callrec.rs` | yes |
| `src/callwatch.rs` | yes |
| `src/capability.rs` | yes |
| `src/capture.rs` | yes |
| `src/categories.rs` | yes |
| `src/cdp.rs` | yes |
| `src/certainty.rs` | yes |
| `src/chain.rs` | yes |
| `src/channel.rs` | yes |
| `src/chat.rs` | yes |
| `src/checks.rs` | yes |
| `src/checkup.rs` | yes |
| `src/chords.rs` | yes |
| `src/chunker.rs` | yes |
| `src/civil.rs` | yes |
| `src/cli.rs` | yes |
| `src/clients.rs` | yes |
| `src/clipboard.rs` | yes |
| `src/cliphist.rs` | yes |
| `src/cloudsync.rs` | yes |
| `src/codes.rs` | yes |
| `src/companion.rs` | yes |
| `src/config.rs` | yes |
| `src/confirmed.rs` | yes |
| `src/connectivity.rs` | yes |
| `src/consent.rs` | yes |
| `src/consolidate.rs` | yes |
| `src/consult.rs` | yes |
| `src/content.rs` | yes |
| `src/contents.rs` | yes |
| `src/council.rs` | yes |
| `src/courier.rs` | yes |
| `src/craft.rs` | yes |
| `src/crash.rs` | yes |
| `src/credentials.rs` | yes |
| `src/crew.rs` | yes |
| `src/cronspec.rs` | yes |
| `src/cutcheck.rs` | yes |
| `src/daemon.rs` | yes |
| `src/daily.rs` | yes |
| `src/dash.rs` | yes |
| `src/decide.rs` | yes |
| `src/delegate.rs` | yes |
| `src/delivery.rs` | yes |
| `src/diagnose.rs` | yes |
| `src/diarize.rs` | yes |
| `src/dictate.rs` | yes |
| `src/diff.rs` | yes |
| `src/digest.rs` | yes |
| `src/doctor.rs` | yes |
| `src/doorrule.rs` | yes |
| `src/draft.rs` | yes |
| `src/drain.rs` | yes |
| `src/earned.rs` | yes |
| `src/edit.rs` | yes |
| `src/editcraft.rs` | yes |
| `src/editors.rs` | yes |
| `src/elsewhere.rs` | yes |
| `src/endpoint.rs` | yes |
| `src/enrol.rs` | yes |
| `src/error.rs` | yes |
| `src/explain.rs` | yes |
| `src/facts.rs` | yes |
| `src/faithful.rs` | yes |
| `src/feedback.rs` | yes |
| `src/feeds.rs` | yes |
| `src/files.rs` | yes |
| `src/filing.rs` | yes |
| `src/filmstrip.rs` | yes |
| `src/finance.rs` | yes |
| `src/findfile.rs` | yes |
| `src/firewall.rs` | yes |
| `src/firstlaunch.rs` | yes |
| `src/firstrun.rs` | yes |
| `src/fit.rs` | yes |
| `src/fixloop.rs` | yes |
| `src/flow.rs` | yes |
| `src/frames.rs` | yes |
| `src/freshness.rs` | yes |
| `src/friends.rs` | yes |
| `src/fxday.rs` | yes |
| `src/gaze.rs` | yes |
| `src/getpieces.rs` | yes |
| `src/gguf.rs` | yes |
| `src/gifenc.rs` | yes |
| `src/glance.rs` | yes |
| `src/gmm.rs` | yes |
| `src/goal.rs` | yes |
| `src/goingaway.rs` | yes |
| `src/goodbye.rs` | yes |
| `src/grade.rs` | yes |
| `src/grading.rs` | yes |
| `src/grants.rs` | yes |
| `src/groups.rs` | yes |
| `src/guessable.rs` | yes |
| `src/habits.rs` | yes |
| `src/handloop.rs` | yes |
| `src/handoff.rs` | yes |
| `src/handover.rs` | yes |
| `src/handshape.rs` | yes |
| `src/handtrack.rs` | yes |
| `src/health.rs` | yes |
| `src/hearing.rs` | yes |
| `src/himalaya.rs` | yes |
| `src/hlc.rs` | yes |
| `src/hollow.rs` | yes |
| `src/hollowcode.rs` | yes |
| `src/hotkey.rs` | yes |
| `src/hotkeys.rs` | yes |
| `src/household.rs` | yes |
| `src/http.rs` | yes |
| `src/hub.rs` | yes |
| `src/hubjobs.rs` | yes |
| `src/hublive.rs` | yes |
| `src/hubpages.rs` | yes |
| `src/hubvault.rs` | yes |
| `src/hubwin.rs` | yes |
| `src/identity.rs` | yes |
| `src/imap.rs` | yes |
| `src/improve.rs` | yes |
| `src/index.rs` | yes |
| `src/infer.rs` | yes |
| `src/inhibit.rs` | yes |
| `src/input.rs` | yes |
| `src/install.rs` | yes |
| `src/integrations.rs` | yes |
| `src/intent.rs` | yes |
| `src/interrupt.rs` | yes |
| `src/ios.rs` | yes |
| `src/judgment.rs` | yes |
| `src/kin.rs` | yes |
| `src/knowhow.rs` | yes |
| `src/kokoro.rs` | yes |
| `src/lanes.rs` | yes |
| `src/language.rs` | yes |
| `src/later.rs` | yes |
| `src/launcher.rs` | yes |
| `src/layout.rs` | yes |
| `src/layout_prefs.rs` | yes |
| `src/learned.rs` | yes |
| `src/ledger.rs` | yes |
| `src/levels.rs` | yes |
| `src/lifecycle.rs` | yes |
| `src/linkage.rs` | yes |
| `src/live.rs` | yes |
| `src/localclock.rs` | yes |
| `src/log.rs` | yes |
| `src/loginseal.rs` | yes |
| `src/look.rs` | yes |
| `src/look_paint.rs` | yes |
| `src/lookalike.rs` | yes |
| `src/mail.rs` | yes |
| `src/mailbook.rs` | yes |
| `src/mailthread.rs` | yes |
| `src/mark.rs` | yes |
| `src/marketdays.rs` | yes |
| `src/mcp.rs` | yes |
| `src/meaning.rs` | yes |
| `src/measure.rs` | yes |
| `src/meetprep.rs` | yes |
| `src/memory.rs` | yes |
| `src/mend.rs` | yes |
| `src/mesh.rs` | yes |
| `src/meshio.rs` | yes |
| `src/messaging.rs` | yes |
| `src/metrics.rs` | yes |
| `src/mfcc.rs` | yes |
| `src/micthread.rs` | yes |
| `src/mind.rs` | yes |
| `src/mobile.rs` | yes |
| `src/models.rs` | yes |
| `src/modes.rs` | yes |
| `src/money.rs` | yes |
| `src/motion.rs` | yes |
| `src/msoauth.rs` | yes |
| `src/nearby.rs` | yes |
| `src/next_up.rs` | yes |
| `src/notify.rs` | yes |
| `src/notifyicon.rs` | yes |
| `src/nudge.rs` | yes |
| `src/ocr.rs` | yes |
| `src/onion.rs` | yes |
| `src/online.rs` | yes |
| `src/onlyone.rs` | yes |
| `src/opportunity.rs` | yes |
| `src/opsec.rs` | yes |
| `src/orders.rs` | yes |
| `src/oslook.rs` | yes |
| `src/ota.rs` | yes |
| `src/otherside.rs` | yes |
| `src/outbox.rs` | yes |
| `src/outreach.rs` | yes |
| `src/overlay.rs` | yes |
| `src/overlaywin.rs` | yes |
| `src/overnight.rs` | yes |
| `src/palette.rs` | yes |
| `src/panel.rs` | yes |
| `src/pdfkit.rs` | yes |
| `src/pdftext.rs` | yes |
| `src/peerkey.rs` | yes |
| `src/people.rs` | yes |
| `src/perf.rs` | yes |
| `src/person.rs` | yes |
| `src/persona.rs` | yes |
| `src/phases.rs` | yes |
| `src/phone.rs` | yes |
| `src/phoneadd.rs` | yes |
| `src/phonelink.rs` | yes |
| `src/phonemodel.rs` | yes |
| `src/picture_talk.rs` | yes |
| `src/pipeline.rs` | yes |
| `src/plainchange.rs` | yes |
| `src/plainly.rs` | yes |
| `src/plugins.rs` | yes |
| `src/pngcodec.rs` | yes |
| `src/policy.rs` | yes |
| `src/portable.rs` | yes |
| `src/preferences.rs` | yes |
| `src/presence.rs` | yes |
| `src/proactive.rs` | yes |
| `src/probe.rs` | yes |
| `src/profiles.rs` | yes |
| `src/pronounce.rs` | yes |
| `src/prose.rs` | yes |
| `src/publish.rs` | yes |
| `src/publishing.rs` | yes |
| `src/quickinput.rs` | yes |
| `src/ratelimit.rs` | yes |
| `src/reach.rs` | yes |
| `src/readable.rs` | yes |
| `src/recall.rs` | yes |
| `src/receipts.rs` | yes |
| `src/reclaim.rs` | yes |
| `src/recovery.rs` | yes |
| `src/recur.rs` | yes |
| `src/redact.rs` | yes |
| `src/reference.rs` | yes |
| `src/references.rs` | yes |
| `src/refusals.rs` | yes |
| `src/register.rs` | yes |
| `src/rehearse.rs` | yes |
| `src/release.rs` | yes |
| `src/remote.rs` | yes |
| `src/research.rs` | yes |
| `src/resume.rs` | yes |
| `src/retention.rs` | yes |
| `src/returning.rs` | yes |
| `src/revise.rs` | yes |
| `src/rollover.rs` | yes |
| `src/roots.rs` | yes |
| `src/roster.rs` | yes |
| `src/route.rs` | yes |
| `src/routine.rs` | yes |
| `src/safety.rs` | yes |
| `src/sandbox.rs` | yes |
| `src/scene3d.rs` | yes |
| `src/scheduler.rs` | yes |
| `src/screentext.rs` | yes |
| `src/sealedlog.rs` | yes |
| `src/selfaudit.rs` | yes |
| `src/selfgrant.rs` | yes |
| `src/selfwork.rs` | yes |
| `src/server.rs` | yes |
| `src/session.rs` | yes |
| `src/settings.rs` | yes |
| `src/settingswin.rs` | yes |
| `src/setupwin.rs` | yes |
| `src/shakedown.rs` | yes |
| `src/shared_task.rs` | yes |
| `src/signals.rs` | yes |
| `src/signin.rs` | yes |
| `src/smtp.rs` | yes |
| `src/snippets.rs` | yes |
| `src/sound.rs` | yes |
| `src/speaker.rs` | yes |
| `src/speaking.rs` | yes |
| `src/speakthread.rs` | yes |
| `src/speech.rs` | yes |
| `src/spoken_form.rs` | yes |
| `src/spoken_numbers.rs` | yes |
| `src/srs.rs` | yes |
| `src/stale.rs` | yes |
| `src/stance.rs` | yes |
| `src/standdown.rs` | yes |
| `src/startup.rs` | yes |
| `src/stemmer.rs` | yes |
| `src/store.rs` | yes |
| `src/strategy.rs` | yes |
| `src/subject.rs` | yes |
| `src/sync.rs` | yes |
| `src/system.rs` | yes |
| `src/taste.rs` | yes |
| `src/telegram.rs` | yes |
| `src/thread.rs` | yes |
| `src/tier.rs` | yes |
| `src/timebox.rs` | yes |
| `src/timing.rs` | yes |
| `src/toast.rs` | yes |
| `src/together.rs` | yes |
| `src/tools.rs` | yes |
| `src/trace.rs` | yes |
| `src/tradeday.rs` | yes |
| `src/translation.rs` | yes |
| `src/transport.rs` | yes |
| `src/tray.rs` | yes |
| `src/triage.rs` | yes |
| `src/tts.rs` | yes |
| `src/tune.rs` | yes |
| `src/twofactor.rs` | yes |
| `src/typebox.rs` | yes |
| `src/typed.rs` | yes |
| `src/typos.rs` | yes |
| `src/tz.rs` | yes |
| `src/uia.rs` | yes |
| `src/understood.rs` | yes |
| `src/undo.rs` | yes |
| `src/unpack.rs` | yes |
| `src/unsub.rs` | yes |
| `src/untrusted.rs` | yes |
| `src/unwaited.rs` | yes |
| `src/update_apply.rs` | yes |
| `src/update_courier.rs` | yes |
| `src/upgrade.rs` | yes |
| `src/urgency.rs` | yes |
| `src/vad.rs` | yes |
| `src/vadcal.rs` | yes |
| `src/vault.rs` | yes |
| `src/vformat.rs` | yes |
| `src/viewing.rs` | yes |
| `src/vision.rs` | yes |
| `src/voice.rs` | yes |
| `src/voiceid.rs` | yes |
| `src/voiceover.rs` | yes |
| `src/voicepick.rs` | yes |
| `src/waitingfor.rs` | yes |
| `src/wakeword.rs` | yes |
| `src/walkthrough.rs` | yes |
| `src/wanted.rs` | yes |
| `src/wants.rs` | yes |
| `src/watch.rs` | yes |
| `src/watching.rs` | yes |
| `src/webrun.rs` | yes |
| `src/webview2_loader.rs` | yes |
| `src/when.rs` | yes |
| `src/which_errand.rs` | yes |
| `src/whichone.rs` | yes |
| `src/why.rs` | yes |
| `src/window.rs` | yes |
| `src/wire.rs` | yes |
| `src/wireguard.rs` | yes |
| `src/words.rs` | yes |
| `src/workday.rs` | yes |
| `src/workingset.rs` | yes |
| `src/worklog.rs` | yes |
| `src/workshop.rs` | yes |
| `src/workspace.rs` | yes |
| `src/workspace_view.rs` | yes |
| `src/ws.rs` | yes |
| `src/yata.rs` | yes |
| `src/yourchanges.rs` | yes |
| `src/zipread.rs` | yes |
| `src/platform/idle.rs` | yes |
| `src/platform/mobile.rs` | yes |
| `src/platform/mock.rs` | yes |
| `src/platform/mod.rs` | — |
| `src/platform/posix.rs` | yes |
| `src/platform/win.rs` | yes |

### `src/lib.rs`

**Wired** — something outside this file calls into it.

### `src/main.rs`

**Wired** — something outside this file calls into it.

### `src/market/bars.rs`

**Wired** — something outside this file calls into it.

Price bars, and a view that cannot see the future.

## The thing this file does that the Python could not

The Python version of this work fought lookahead with discipline and a leak
detector: a truncating helper, a rule that readers must be handed a slice,
and a test that swept every reader looking for one whose answer changed when
the future arrived. That works, and it found real problems. But it is a
guard rail beside a cliff — nothing *stops* a function taking the whole
series and reading the end of it. The detector exists precisely because the
mistake remains makeable.

Rust lets the mistake be unmakeable. **No reader in this module takes
`&Bars`.** They take [`AsOf`], which owns a borrow of the bars and an upper
bound, and hands out `&[f64]` slices that have already been cut at that
bound. There is no method on `AsOf` that returns a later price, because
there is no method on `AsOf` that returns anything but the truncated slice.
A reader cannot peek at bar `n+1` for the same reason it cannot read past
the end of a slice: the value is not reachable from what it was given.

That is the whole argument for doing this port. Lookahead is the one failure
in this system that is silent and flattering — a backtest that peeks does
not crash, it produces exactly the result the person running it hoped for.
Moving it from "tested for" to "not expressible" is worth more than every
other difference between the two languages combined.

## The cache, and the bug that shaped it

Swing detection is an interpreted loop over the window and three readers
want it, several times, per deliberation. In Python that cache lived on the
frame's `attrs`, which pandas **deep-copies on ordinary column access** —
measured at a 6x tax on every read, growing with the number of pivots, so
the "optimisation" reversed sign as the series got longer.

Here the cache is a `RefCell` on `Bars`, keyed by `(upto, k)`. Keying on
`upto` is not an optimisation detail: it is what makes the cache safe under
truncation. A cache computed over 500 bars can never be returned to a view
bounded at 200, because the key does not match — which was the exact
contamination path the Python detector was built to catch.

**Public interface:**

- `fn refuse`
- `fn pip_size`
- `struct Bars`
- `struct Refusal`
- `struct AsOf`

### `src/market/claims.rs`

**Wired** — something outside this file calls into it.

The referee. Only the bars decide truth.

## The rule the whole design rests on

> An analyst may choose WHICH claims to stake and what it thinks they mean.
> It may never decide whether a claim is TRUE. That is decided here, by
> arithmetic, against the bars.

Without that split, two Atlas instances arguing produce the most persuasive
case rather than the most correct one — and a persuasive wrong case is worse
than no case, because it survives review. With it, a side that asserts a
failed break of structure that did not happen simply loses that claim. It
cannot argue its way past the bars.

## What the port made stronger

In Python the vocabulary was a dictionary of reader functions, and a claim
naming a kind nobody had registered was caught at verification time with a
refusal listing the known kinds. That works, and it is a runtime check on a
string.

Here [`Kind`] is an enum and [`verify`] matches on it exhaustively. **A
claim the referee cannot check cannot be constructed** — there is no string
to get wrong, and adding a variant without adding its reader is a compile
error rather than a runtime surprise in front of a live market. The spec's
"adding a claim kind means adding a checker function, one file, one
function" is now enforced by the compiler.

## Three-valued, and the middle value is load-bearing

`Unknown` is not `False`. A claim that cannot be checked earns nothing and
costs nothing; treating it as refuted would let missing data argue for the
other side. Several readers use it deliberately — an unconfirmed reversal is
`Unknown` precisely so a side cannot score off a pullback that came to
nothing, which is most of them.

**Public interface:**

- `fn verify`
- `struct Claim`
- `struct Verdict`
- `enum Side`
- `enum Truth`
- `enum Kind`
- `enum Grounds`
- `const ALL_KINDS`

### `src/market/events.rs`

**Wired** — something outside this file calls into it.

Scheduled news: when it lands, what it moves, how long the damage lasts.

## What the research changed, in order of how wrong this would otherwise be

**1. Non-farm payrolls is not the first Friday of the month.** Everyone says
it is. The BLS rule is the *third Friday after the Saturday ending the week
containing the 12th* of the reference month. They coincide often enough for
the folk rule to feel true and they came apart in **four of the twelve
months of 2026** — by a full week in January and May, and in February the
release was on a WEDNESDAY, which no weekday rule can ever produce. A
blackout built on "first Friday" would have been open for business through
the single largest scheduled event in FX, four times a year, without ever
saying it was wrong.

**2. The ±2 minute blackout everyone uses is about ten times too short**,
and it is convention with nothing behind it — prop firms range from 0 to 5
minutes and none publishes a derivation. Andersen, Bollerslev, Diebold &
Vega (2003) found the conditional MEAN adjusts in 5–10 minutes but
volatility takes about **60 minutes** to return to baseline; Chaboud et al.
measured NFP volume elevated for **120**. So a window is reported per
PURPOSE: "avoid the price jump" and "avoid the volatility" are different
questions with answers an order of magnitude apart.

**3. You cannot dodge the window by predicting a dull print.** Chaboud et
al. decomposed the NFP volume spike: the intercept dominates the
surprise-sensitivity term by roughly **450 to 1**. Most of the extra trading
happens even when the number lands exactly on consensus. The event fires the
window, not the surprise.

**4. Most of a standard economic calendar is noise.** ABDV found NO
significant effect on FX for PPI, housing starts, leading indicators, money
supply, personal income, new home sales, factory orders, business
inventories, or the GDP second and third estimates. PPI independently had
the smallest measured volume impact of anything tested — and sits on nearly
every prop firm's restricted list. None of them are in this module.

**5. Some "structural" cross-currency links have broken.** Oil-to-CAD was
significant 1997–2014 and is **not significant from 2016 on**, with the
correlation now slightly positive. Hard-coding it would encode a
relationship that stopped working a decade ago, so it is absent. AUD/JPY as
a risk proxy, by contrast, is properly evidenced — it is mechanically the
two ends of the carry cross-section — so it is wired.

## How the dates are held

Three tiers, stored on every event, because the honest thing to do with a
date nobody can derive is to say so:

- `Rule` — derivable from calendar arithmetic. Computed.
- `Semi` — a rule that holds most months and breaks on holidays.
- `Fixed` — no rule exists. Published dates, hand-entered, **with an
expiry**. When the table runs out this module refuses rather than
returning a stale answer.
- `Provisional` — published, and labelled provisional by the issuing bank
itself. Carried at the confidence the issuer gave it rather than flattened
to the same footing as everything else.

The SNB looks like it should be "the third Thursday of the quarter-end
month" and in 2026 it was the third, third, **fourth** and **second**. That
is why these are lists and not arithmetic. And two reads of the SNB's own
2027 schedule disagreed about September and December, so those two are
**absent** and [`gaps`] reports why — a missing event makes the calendar
silent, a wrong one makes it confidently wrong.

**Public interface:**

- `fn nfp_date`
- `fn gaps`
- `fn bank_events`
- `fn month`
- `fn overlapping`
- `fn inside_blackout`
- `fn collisions`
- `struct Event`
- `enum Purpose`
- `enum Tier`
- `const NFP_VOLATILITY_WINDOW`
- `const RISK_PROXY`
- `const TABLE_GOOD_UNTIL`
- `const TABLE_GOOD_FROM`

### `src/market/feed.rs`

**Wired** — something outside this file calls into it.

What real bars have to satisfy before any reader is allowed to see them.

## Why this is built with no real bars to run it on

Atlas has no bar feed of its own: bars arrive from whatever you point it at.
But the day real bars arrive is exactly the wrong day to start thinking
about what a bad bar looks like: everything will run, numbers will come out,
and nobody will know which of them were computed across a weekend hole or a
duplicated timestamp.

**Every defect below is silent.** A frame with a hole in it does not raise —
the readers happily compute an efficiency ratio across a 65-hour weekend as
though it were one bar of movement and report a trend. A duplicated bar does
not raise either; it just makes a swing look like a double top. None of
these produce an error. They produce a number. That is the whole argument
for checking at the door rather than hoping to notice later.

## The one that is not like the others

A bar stamped in the future is not a data-quality problem, it is **lookahead
arriving through the front door**. If a feed hands over a bar dated after
the decision instant — clock skew, a broker's server timezone, an off-by-one
in a replay driver — then every reader downstream is legitimately reading a
bar that has not happened.

No amount of care inside the readers can catch that. `AsOf` makes it
impossible for a reader to look past the end of what it was given; it cannot
make the thing it was given honest. This is the door the type system cannot
guard, and it is why [`no_future_bars`] is checked against the decision
instant rather than against the data.

## Refused versus noted

Refusing a whole session because one candle has a high below its low would
be its own kind of failure. The split is whether a reader could produce a
**wrong** answer or merely a **weak** one.

**Public interface:**

- `fn no_future_bars`
- `fn spikes_in`
- `fn accept`
- `struct Note`
- `struct Report`
- `enum NoteKind`
- `const GAP_BARS`
- `const WEEKEND_HOURS`

### `src/market/fixtures.rs`

**Wired** — something outside this file calls into it.

Deterministic synthetic series.

Not test scaffolding, and not `#[cfg(test)]`. Atlas holds no real bars of
its own, so every measurement this module makes about itself — the
random-walk baseline the efficiency ratio is read against, the rate at which
arbitrary levels bounce, how often noise is mislabelled a trend — is made
against a synthetic series. Those are calibration facts the system relies on
at runtime, so the generator that produces them belongs in the build.

Everything here is seeded and integer-based, so it reproduces bit for bit on
every machine. A calibration number that drifts between the developer's box
and the machine it runs on is not a calibration number.

## The bug this file remembers

The first generator shifted `>> 33`, which yields a 31-bit value; divided by
`u32::MAX` it never exceeds 0.5, so subtracting 0.5 made **every step
negative**. It generated a monotone downtrend and called it a random walk.

Nothing failed. A fixture that is not what it claims does not fail — it
passes for the wrong reason, and every test built on it quietly becomes
decoration. [`walk`] is checked by its own tests for the two properties it
is supposed to have: steps that go both ways, and pivots that actually form.

**Public interface:**

- `fn walk`
- `fn walk_with`
- `fn ramp`
- `fn zigzag`
- `fn box_range`
- `fn path`
- `fn from_path`
- `const FIXTURE_SPREAD`

### `src/market/levels.rs`

**Wired** — something outside this file calls into it.

Support and resistance, built on what the evidence actually supports.

## Why this file is shaped the way it is

Most of the retail support-and-resistance canon is unevidenced convention,
and some of it has been directly measured and found worthless. The research
is written up with citations in `TRADING_KNOWLEDGE.md`; the short version,
because it determines every design choice here:

**Best evidenced — round numbers.** Osler (2003, *J. Finance*) examined
9,667 real conditional orders from a major FX dealing bank. 8.7% sat at
rates ending in 00 against 1% expected by chance. Take-profit orders cluster
AT the figure (9.3% vs 4.4% for stops); stop-loss orders cluster **1–10 pips
BEYOND** it. That asymmetry has a measured consequence: price reverses at
round numbers 3.4pp more often than at arbitrary ones, and ACCELERATES after
crossing them. Confirmed independently on the EBS order book a decade later.

So round numbers are the spine of this module, not a garnish — and they are
**bidirectional**: "at the figure" and "ten pips past the figure" are
opposite states, not one level with one behaviour.

**Measured and found worthless — level "strength" ratings.** The six bank
desks in Osler (2000) published strength estimates alongside their levels.
Those estimates had no meaningful correlation with actual bounce frequency,
and the desks agreed with each other on level PLACEMENT only 30% of the
time. So [`Level`] has no strength field and this module refuses to compute
one. It scores on the three things with measured effects — touch count,
recency, round-number status — and nothing else.

**No evidence at all — polarity.** Broken resistance becoming support. Every
practitioner source asserts it; not one quantifies it, and no academic test
exists. Implemented, and it says UNTESTED in its own verdict text so the
scoreboard settles it rather than this file assuming it.

**The size of all of it.** The measured edges are 3–5 percentage points on a
~56% base rate, before spread. Real, and economically marginal. Levels
belong here as context that earns its weight from its own track record, not
as a signal anything may trade on alone.

## The tolerance, and the null

The hardest number in the subject is "how close is *at*". Practitioner
sources insist levels are zones and then give no width at all. The one
principled answer in the literature is the mean absolute bar-to-bar change,
chosen because a random walk then bounces off its own levels at close to
chance — which gives every measurement a built-in null. Measured here at
**56–58%** on synthetic walks, against Osler's **56.2%** for artificial
levels on real FX data. Two roads, same number. See [`null_bounce_rate`].

**Public interface:**

- `fn window_start`
- `fn figures_near`
- `fn levels`
- `fn nearest`
- `fn null_bounce_rate`
- `fn bounce_rate`
- `struct Level`
- `enum Kind`
- `const TOUCH_SEPARATION`
- `const STRUCTURE_LOOKBACK_BARS`
- `const WINDOW_STEP_BARS`
- `const STOP_BAND_PIPS`

### `src/market/mod.rs`

**Module declaration** — declares the files below it; no capability of its own.

Reading a market: structure, levels, regime, sessions, news -- and not
seeing the future.

Pure `std`. No external crates, deliberately: this is the part of Atlas that
has to run offline on a bare Windows machine, and every dependency here is
a thing that can fail to build on the machine that matters. The arithmetic is small
enough to own, and the two places it would normally be borrowed -- a
timezone database and a calendar crate -- are exactly the two places Windows
makes borrowing unreliable.

### `src/market/multiframe.rs`

**Wired** — something outside this file calls into it.

Whether the higher timeframe agrees — reading more than one timeframe
for the same instrument at once, instead of reading one in isolation
and staying blind to what the others say.

General capability, not specific to any one business: any market read
benefits from knowing whether a shorter and a longer view of the same
instrument point the same way, the same reason a chart reader glances
at the daily before trusting the hourly.

## What this is not

**Not a claim that agreement predicts anything.** The one measurement
run against this question so far (12 Sep 2026, on one set of FX
data) found H4 agreement and H4 conflict both landed inside the noise
— a real test, not a proven edge. This module exists because Atlas
could not previously even *ask* the question — "does the higher
timeframe agree" was a capability gap, not a validated signal. Building
the ability to ask is not the same as answering it, and nothing here
should be read as the latter.

**Not a resampler.** This does not build an H4 bar out of four H1 bars.
It takes views the caller already has at each timeframe — exactly how
the numbers were checked on 12 Sep 2026, where separate H1/H4/D books
already exist independently — and reads each one's own structure using
[`crate::market::timeframe`]'s own horizon-to-bar-count conversion, so a
"day of structure" means the right number of bars on every timeframe
rather than one raw count applied everywhere.

**Public interface:**

- `fn read_frame`
- `fn read_all`
- `fn agreement`
- `fn spoken`
- `struct FrameRead`
- `enum Agreement`

### `src/market/params.rs`

**Wired** — something outside this file calls into it.

Every tunable number in this module, frozen, with the reason it has it.

## Why this had to be written before the first real backtest

Sullivan, Timmermann & White took 7,846 technical trading rules — including
1,220 support-and-resistance variants — and tested them under White's
Reality Check for data snooping. The rules that looked best in sample did
not survive out of sample. That is not a curiosity; it is the **default
outcome** of searching a large parameter space against one dataset.

This module has that space: pivot strength, level tolerance, touch
separation, three lookbacks, the trend entry and exit gates, dwell time, the
cost multiple, the minimum window. Turning them against the first real
dataset until the equity curve looks right would produce a system that
backtests beautifully and does nothing — and there would be no way
afterwards to tell that from a system that works, because the evidence would
have been spent.

So they are written down **now**, while nobody knows what the real data
looks like and therefore nobody can be tempted. Each carries its value, its
reason, and how it may legitimately change. That third field is the point:
"measured on the bars" and "it looked better" are different grounds, and the
difference is invisible six months later unless it was recorded at the time.

## How the freeze is enforced

Every entry holds an `expected` literal **and** reads the live constant from
the module that owns it. [`drift`] compares them, and the test suite fails
on any difference — so a value edited in its own module and not here is
caught, and the change has to be deliberate in both places rather than in
neither. A planted mismatch proves the check can fire.

[`fingerprint`] hashes the whole set. Record it beside a backtest result: a
result quoted without one cannot be reproduced, whatever the commit message
says.

[`attempts`] is the multiple-comparisons count. A system that has tried
forty configurations and reports the best has not found an edge — it has
found the largest of forty noise draws, and there is no way to recover that
fact afterwards unless it was counted at the time. It is zero, and that is
the whole value of it.

## What this is not

Not a config file. Nothing reads it at runtime; the modules own their own
constants and this registry checks them. Making it the source of truth would
let one import change the behaviour of the whole system, which is precisely
the door it exists to close.

**Public interface:**

- `fn frozen`
- `fn attempts`
- `fn freedoms`
- `fn fingerprint`
- `fn drift`
- `fn report`
- `struct Param`
- `enum Basis`
- `const CHANGES`

### `src/market/regime.rs`

**Wired** — something outside this file calls into it.

Trending, ranging, or choppy — and the honest reasons those are hard.

## Four things the research changed about how this is built

**1. Every published threshold in this subject is a convention, and some are
decorative.** The Choppiness Index thresholds everyone uses, 61.8 and 38.2,
are Fibonacci ratios — TradingView says so in its own documentation.
Wilder's ADX 20/25 comes from a 1978 book on daily commodity bars and has no
derivation at all. Nothing here takes a number like that on trust: each
measure is read against its own random-walk baseline, and several of those
baselines are derivable in closed form.

**2. The efficiency ratio's baseline is 1/√n, not zero.** For a random walk
E[net move]/E[path] = √n/n. So at n=10 the baseline is 0.316 — and the
universally quoted "ER above 0.30 means trending" is a threshold **below
chance**. At n=30 the baseline is 0.183 and 0.30 is genuinely strong.

This matters most for FX. Measured 22-day ER: S&P 0.30, Bitcoin 0.28,
**EURUSD 0.18** — against a 1/√22 = 0.213 baseline. EURUSD sits *below* its
own random-walk baseline. FX at these horizons is mildly mean-reverting, and
a threshold ported from equity writing essentially never fires.

**3. ADX is the worst tool here, not the default one.** Its lag is 2(n−1)
bars, not (n−1), because Wilder smoothing is applied twice; at n=14 that is
26 bars — six and a half hours on M15. It also ignores closes entirely, so a
run of bars with long upper wicks and falling closes generates positive +DM.
In the one systematic multi-parameter test found it ranked **last of
thirteen** regime filters. It is not implemented, and that is a decision
rather than an omission.

**4. Hurst and variance-ratio tests do not work at this window length.**
Detecting first-order autocorrelation needs |ρ| > 1.96/√T: at 500 bars that
is 0.088, and real FX intraday autocorrelation is 0.01–0.05. Detecting
ρ=0.05 at 80% power needs about 3,100 bars — 32 days of M15, by which time
the regime has changed many times. [`hurst_needs`] returns that number so
nobody adds one to the live classifier.

## And the one that decides whether any of it pays

"Ranging" versus "choppy" has no accepted quantitative definition — it is a
practitioner distinction, and every source that draws it does so in prose.
The honest operationalisation is that it is not purely a property of the
price series at all. It is **cost-relative**:

> A 10-pip box on EURUSD M15, at 0.8 pip spread and 0.3 pip slippage, has
> 22% of its width eaten before you are right. That is not a range. The same
> statistical structure on H4 with a 60-pip box is a range.

So [`Regime::tradeable`] gates on room against cost, and a structure that
passes every statistical test but fails that gate is reported CHOPPY. It is
the most load-bearing line in the file.

**Public interface:**

- `fn efficiency_ratio`
- `fn er_baseline`
- `fn trend_strength`
- `fn r_squared`
- `fn r2_critical`
- `fn squeeze`
- `fn choppiness`
- `fn chop_baseline`
- `fn hurst_needs`
- `fn read`
- `fn read_plain`
- `struct Regime`
- `enum Direction`
- `enum Energy`
- `const DEFAULT_SPREAD_PIPS`
- `const DEFAULT_SLIPPAGE_PIPS`
- `const COST_MULTIPLE`
- `const ENTER_TREND`
- `const EXIT_TREND`
- `const MIN_DWELL`

### `src/market/session.rs`

**Wired** — something outside this file calls into it.

Which market is actually awake.

## Why this exists

A verdict formed at 03:00 UTC and one formed at 14:00 UTC are not the same
kind of fact. The same structure — same swings, same break, same
confidence — means something different when London and New York are both on
the book than it does in the hour after Sydney opens and before Tokyo does.
Ranges hold in thin hours and break in deep ones. A system that scores its
own accuracy without splitting on that is averaging two different games and
calling it one number.

## Why the rules are hand-rolled

Two constraints, pointing the same way. No dependencies is the house rule,
and it is load-bearing here: the Python original could not use `zoneinfo`
because **Windows ships no system timezone database** and every lookup
raises without an extra package. The same argument applies to a Rust tz
crate on a bare Windows machine. And the sessions have to be defined where they actually
live — in local hours at each financial centre.

**London does not open at 07:00 UTC.** It opens at 08:00 London time, which
is 08:00 UTC in winter and 07:00 in summer, and New York's clocks change on
a different date. Measured, for 2026:

```text
4 h   1 Jan –  6 Mar    London–New York overlap
5 h   9 Mar – 27 Mar    US on DST, Europe not yet
4 h  30 Mar – 23 Oct
5 h  26 Oct – 30 Oct    Europe off DST, US not yet
4 h   2 Nov – 31 Dec
```

Note the direction, because it is the opposite of what it sounds like: the
deepest window of the trading day **grows** by an hour for four weeks a
year. Anything with hardcoded UTC session boundaries is quietly wrong for a
month a year, in the most liquid part of the day.

## What a transition actually is

A clock change is a local-time event, so the UTC instant it happens at is
the local instant minus the offset in force just before it. That sounds
obvious and is the one thing that is easy to get wrong: Sydney springs
forward at 02:00 AEST on the first Sunday of October, which is **16:00 UTC
on the Saturday**. Computing the Sunday and stopping there is a day out,
twice a year — which is exactly the bug the Python version shipped with, and
the reason the transition instants are pinned in the tests below.

Those rules were validated against the IANA database over 2024–2027: eight
zones, hourly, **140,256 offsets, zero mismatches**. Rust has no tz database
to validate against here, so the transitions verified in that run are baked
in as literals and checked to the minute.

**Public interface:**

- `fn offset_hours`
- `fn is_open`
- `fn session_at`
- `fn overlap_hours`
- `struct Session`
- `enum Centre`
- `enum Zone`
- `const CENTRES`

### `src/market/structure.rs`

**Wired** — something outside this file calls into it.

Where structure is, and whether it has turned.

## The gap this closes

The first version of this reading answered two questions: are the highs
rising, are the lows rising. Both returned "unknown" the moment a sequence
stopped being monotonic — **which is precisely the moment a trend turns**.
So a reversal and a patch of meaningless noise produced the identical
reading, and the one thing most worth being told was the one thing the
structure could not say.

It was worse than a reporting gap. There was no way to ARGUE it either: the
trend reading asks "is structure trending my way", a question about the last
pair of swings, so a market that has fallen for a week and just put in one
higher low gets FALSE — correctly, because it is not trending up yet. A side
could watch a reversal happen and have nothing to stake.

## The rule

> **A lower high warns. A lower low confirms.**

Price making a lower high means buyers failed at a level. That is a warning
and nothing more — the trend is intact until the last higher low gives way.
When it does, the sequence of higher highs and higher lows is broken in both
halves at once and the turn is real. Mirrored for a downtrend turning up.

**A lower high on its own is not a reversal.** Every range on every chart is
a sequence of lower highs that came to nothing, and a detector that calls the
first one a turn is wrong early, repeatedly, against a trend that is still
running — which is the most expensive way to be wrong there is.

**Public interface:**

- `fn recent`
- `fn confirmed_swings`
- `fn unknowable`
- `fn before_the_turn`
- `struct Turn`
- `struct Pivot`
- `struct Structure`
- `enum Label`
- `enum Trend`

### `src/market/time.rs`

**Wired** — something outside this file calls into it.

Civil dates from a Unix timestamp, with no calendar crate.

`chrono` and `time` are excellent and neither is here, for the same reason
nothing else in this module tree is: Atlas has to build and run on a bare
Windows machine offline, and every dependency is a thing that can fail to build on the
machine that matters. The conversion is two dozen lines of integer
arithmetic and it is exactly right, so it is owned.

The algorithm is Howard Hinnant's `days_from_civil` / `civil_from_days`,
which is branch-free, exact for the whole proleptic Gregorian range, and has
been in the C++ standard library's lineage for a decade. It is not clever
and it is not mine; it is simply correct, which is what a date conversion
underneath a trading calendar needs to be.

Everything here is UTC. Local time is a separate problem and lives in
`session.rs`, where the daylight-saving rules are — deliberately not mixed
in, because a conversion that quietly applied an offset would be the single
easiest way to make every session boundary an hour wrong for half the year.

**Public interface:**

- `fn days_from_civil`
- `fn civil_from_days`
- `fn nth_weekday`
- `fn last_weekday`
- `fn nth_business_day`
- `struct Utc`
- `const MS_PER_MIN`
- `const MS_PER_HOUR`
- `const MS_PER_DAY`

### `src/market/timeframe.rs`

**Wired** — something outside this file calls into it.

What a bar is worth.

## The problem, stated plainly

Every lookback in this module started life as a BAR COUNT — twenty for the
efficiency ratio, two for the swing detector, twenty for a break. Each was
chosen once, against one mental picture of a chart, and then applied to
every timeframe as if a bar were a unit of anything.

It is not. Twenty bars is:

```text
M1     20 minutes      -- inside a single news window
M5     1 hour 40 min   -- part of one session
M15    5 hours         -- most of a session
H1     20 hours        -- nearly a full day, across three sessions
H4     3 days 8 hours  -- a week of trading
```

Five different questions. And it failed SILENTLY: the number came back, it
looked plausible, and nobody was told the question had changed.

So lookbacks are wall-clock horizons and the bar count is derived. Ask for
"the last day of price action" and get 24 bars on H1 and 96 on M15 — the
same question, asked correctly on each.

## Three things that follow

**A bar is a span, not a moment.** The H4 bar closing at 16:00 covers
12:00–16:00, so it CONTAINS a 12:30 payrolls print, its window and the whole
recovery — while its close sits three and a half hours clear and a
close-only check calls it clean. It is the least clean bar of the week.

**A sample size is a length of time.** Thirty observations is half an hour
on M1 and a full trading week on H4. That turns "we need more data" into a
date.

**A window can be too short to ask.** On H4 a day is six bars, and six bars
cannot support the measures: the efficiency ratio's random-walk baseline at
n=6 is 0.41, so the gap between a trend and noise has closed, and the 95%
critical R² at n=5 is 0.77, which almost nothing clears. Converting the
horizon correctly and then handing over a sample too small to answer it is a
different way of being wrong, not a fix — hence [`MIN_BARS`], and
[`stretched`] to say when it bit.

**Public interface:**

- `fn viable_range_pips`
- `fn say_span`
- `fn stretched`
- `fn infer`
- `fn window`
- `enum Tf`
- `enum Horizon`
- `const ALL`
- `const TRADING_HOURS_PER_WEEK`
- `const MIN_BARS`

### `src/market/verify.rs`

**Wired** — something outside this file calls into it.

A self-check that runs the whole module offline and prints what it found.

This exists because the thing most worth demonstrating about this module
cannot be shown by a test suite passing. A suite says "no assertion failed".
It does not say what the market looked like, what the calibration numbers
came out at, or which of the readings had evidence behind them — and those
are the facts somebody reviewing a merge actually needs.

It also runs with **no real bars, no network, no files and no clock**, which
is the deployment condition that matters: if this prints on a bare Windows
machine with nothing installed, the module works there.

Call [`report`] from a binary, a test, or Atlas itself.

**Public interface:**

- `fn report`

### `src/accounts.rs`

**Wired** — something outside this file calls into it.

Knowing where your accounts stand.

## What this does, and what it deliberately doesn't

You asked whether Atlas could turn two-factor authentication off across
your accounts, with a confirmation and a list. The answer is no, and it's
worth being clear that this isn't about legality — they're your accounts.

It's that a system which *can* disable two-factor across your bank, your
email and your socials is a system where one confused moment, one bad
instruction, or one compromise costs you everything at once. The
confirmation doesn't help: the danger is the capability existing, not the
click. It's the same reason Atlas doesn't touch the firewall or Windows
Defender, and I'd rather be consistent about it.

What it does instead is the useful half, and probably the half you actually
wanted: **find every account, say what protection each has, and rank what's
worth fixing.** Most people have no idea SMS codes are their weakest link
or which account has no second factor at all. Atlas can tell you that,
open the right settings page, and walk you through it.

**Public interface:**

- `fn stakes_for`
- `fn audit`
- `fn spoken`
- `fn asked_to_weaken`
- `fn instead`
- `struct Account`
- `struct Advice`
- `struct AccountsConfig`
- `struct Book`
- `enum SecondFactor`
- `enum Stakes`
- `enum Recorded`
- `enum Change`
- `const WONT_WEAKEN`
- `const FILE`

### `src/activity.rs`

**Wired** — something outside this file calls into it.

What Atlas did.

An assistant that acts while you are away needs to be able to account for
it. Without this, coming back to a changed workspace means guessing what
happened — which is the fastest way to stop trusting it with anything.

Deliberately not a debug log. Bounded, plain language, and the things that
actually changed the world are marked so a summary can lead with them.

**Public interface:**

- `fn anchor_path`
- `fn anchors`
- `fn check_with_backups`
- `fn said_with_backups`
- `struct Event`
- `struct Journal`
- `enum Kind`
- `enum Sealed`

### `src/adapt.rs`

**Wired** — something outside this file calls into it.

Making Atlas fit whatever machine it lands on.

Everything Atlas needs to know about a computer — where the apps live, what
the monitors are, which microphone hears you — is different on every
machine, and none of it belongs in a config file that ships to someone
else. A config with `C:/Users/erics/` in it is broken for everybody except
Eric.

So configuration comes in two layers:

* **`config/*.yaml`** — generic. Ships to anyone. Uses `%LOCALAPPDATA%`
style placeholders and never a literal username.
* **`config/machine.yaml`** — written by Atlas on first run, never shipped.
App paths it found, monitor geometry, the microphone that hears you.

The generic layer is the recipe. The machine layer is what this kitchen
actually has.

**Public interface:**

- `fn detect`
- `fn portable`
- `fn leaks_a_username`
- `fn apply`
- `fn monitor_fixture`
- `fn first_run_message`
- `struct Machine`
- `struct AppFound`
- `struct MonitorFact`

### `src/addressing.rs`

**Wired** — something outside this file calls into it.

Was that meant for Atlas?

"Stop when the user speaks" is too blunt. If Atlas is halfway through a
task and you take a phone call, or someone walks in, it should not abandon
the work because it heard a voice. Equally, if you genuinely say "stop",
it must stop.

So speech is assessed, not just detected: is this directed at Atlas, and
how sure are we? Low confidence on a consequential decision goes back
through the confidence system and becomes a question.

**Public interface:**

- `fn assess`
- `fn respond`
- `struct Assessment`
- `struct Situation`
- `enum Directed`
- `enum Kind`
- `enum Response`
- `const STILL_TALKING_SECS`

### `src/afterme.rs`

**Wired** — something outside this file calls into it.

If something happens to you.

## What the envelope actually is

Physical. A piece of paper with the passphrase on it, in a sealed envelope,
in a place you control — your safe, a lockbox, a fire tin. Not given to
anyone. Not a file, not a cloud folder, not a photo on your phone.

It's paper for a specific reason: it can't be phished, can't be
brute-forced, can't be copied remotely, and doesn't quietly sync to five
devices the way anything digital does. It also can't be accessed by
coincidence — nobody stumbles into a sealed envelope in your safe the way
they stumble into an unlocked laptop.

## Nobody gets your information

What another person is given is **where it is and when to open it**. Not
the passphrase, not a copy, not access. A sentence: *"if I'm out of contact
for ninety days, there's an envelope in the safe."*

That's the whole design. In your hands first, reachable by you at any time,
and reachable by someone else only after a condition you set has actually
happened.

**Public interface:**

- `fn suggest_for_you`
- `fn where_from`
- `fn timer_from`
- `struct Instruction`
- `struct AfterMeConfig`
- `struct Gap`
- `struct Arrangement`
- `enum Where`
- `enum When`
- `enum Timer`
- `const THE_SHAPE`
- `const WHY_PAPER`
- `const NOT_ATLAS`
- `const USE_THE_PLATFORMS`
- `const RECORD`

### `src/agefile.rs`

**Wired** — something outside this file calls into it.

Sending a file only one person can open: the `age` format, in house.

**Sources:** the age v1 specification (C2SP, `age-encryption.org/v1`) —
header of recipient stanzas, an HMAC over it, and a STREAM of 64 KiB
ChaCha20-Poly1305 chunks; RFC 7748 (X25519: the Montgomery ladder over
GF(2^255−19), with the RFC's test vectors); RFC 5869 (HKDF) and RFC 2104
(HMAC) over the tree's own SHA-256 (`digest`); BIP 173 (bech32) for the
`age1…` and `AGE-SECRET-KEY-1…` strings. `FiloSottile/age` (BSD-3) and
`str4d/rage` (MIT/Apache-2.0) read as references. ChaCha20-Poly1305 is the
`chacha20poly1305` crate the vault already depends on. Clean-room.

**Why Atlas wants it.** Sending a contract or a statement to a business
partner meant either email in the clear or a third-party service. An
`age1…` key is a line anyone can paste into an email; a file sealed to it
opens with their `age` (or their Atlas) and nothing else, offline, and
the real `age` tool reads what this writes (checked in the tests against
`age` 1.1.1).

**Public interface:**

- `fn new_identity`
- `fn recipient_of`
- `fn seal`
- `fn open`

### `src/android.rs`

**Wired** — something outside this file calls into it.

What Atlas can do on Android.

Worth stating up front because it surprises people: **Android lets Atlas do
most of what the iPhone can't.** It can listen for a wake word in the
background, be the assistant you get when you hold the home button, read
the screen, and act in other apps.

That's a genuinely different product on the same idea, and if you're handing
this to friends it's worth knowing that the Android ones get the better
version.

**Public interface:**

- `fn abilities`
- `fn serious_permissions`
- `fn wake_word_battery_percent_per_hour`
- `struct Ability`
- `struct AndroidConfig`
- `const VERSUS_IOS`
- `const THE_TRADE`

### `src/answering.rs`

**Wired** — something outside this file calls into it.

Getting an answer to a question, however you want to give it.

Atlas asks things — "go ahead?", "which one?", "record this call?" — and
you shouldn't have to be able to speak to answer. You might be on a call,
in a room with someone, or it might simply have misheard you twice.

So a question stays open across three channels at once: your voice, a
gesture on camera, and the typing box. Whichever arrives first wins.

One rule holds throughout: **a nod is not a signature.** A thumbs-up from a
webcam is two fingers of confidence from a camera that has been wrong
before. It can decline anything, and it can approve anything ordinary, but
it cannot approve something irreversible — that still takes a word.

**Public interface:**

- `fn describe`
- `struct AnsweringConfig`
- `struct Question`
- `enum Channel`
- `enum Answer`
- `enum Step`

### `src/anticipate.rs`

**Wired** — something outside this file calls into it.

Doing the work before you ask for it.

The second thing that makes Jarvis feel like Jarvis: the answer is usually
already there. *"The simulation completed while you were out."*

Every part needed for this already existed — the scheduler, the background
lane, research, the journal. What was missing was the layer that decides
*when* something is worth doing unasked. That is this.

One rule throughout: anticipated work only ever runs in the background lane
and only ever prepares. It never takes your screen and never sends
anything. Preparation is safe; acting unasked is not.

**Public interface:**

- `fn matches`
- `fn suggested`
- `fn ready_line`
- `struct Rule`
- `struct Moment`
- `struct Anticipator`
- `enum Trigger`

### `src/appearance.rs`

**Wired** — something outside this file calls into it.

How the hub looks, stored on the machine.

One choice, applied everywhere. The shell reads it and writes it onto the
`<html>` element as `data-*` attributes; the stylesheet's colour tokens do
the rest, so the whole surface turns over at once and nothing carries a
colour of its own. Kept on this machine like every other preference — it
never leaves, and it applies to every page, desktop and phone.

The design (locked with Eric, 20-21 Sep): Warm Paper the lead colourway,
Ember Dark the dark one, Access the colour-blind-safe one; the accent is
user-pickable; and colour-blind mode swaps any theme to the safe palette,
dropping the accent pick so safety always wins over taste.

Since the three-chat merge (26 Sep): the hub is laid out as the command
deck (Eric's design of 23 Sep, built on the third chat's line), and Warm
Paper is its default colourway (Eric's ruling, 26 Sep) as `data-theme=paper`.
The deck's own dark and light, and following the system, are a choice away.
The hub's "Aa" menu (`hub::Appearance`: paper, light, dark or auto, text
size, contrast, motion) is written first and wins where the two say the
same thing; this one adds the accent, colour-blind mode and density.

**Public interface:**

- `fn choose`
- `struct Appearance`
- `enum Theme`
- `enum Accent`
- `enum Cvd`
- `enum Text`
- `enum Density`
- `const FILE`

### `src/asia.rs`

**Wired** — something outside this file calls into it.

The overnight range, and why anybody watches it.

While Tokyo and Sydney are the only desks on the book, price usually does
very little. The high and low it makes in those hours become the two lines
London opens into — and they are watched for the same reason yesterday's
high is watched: **everybody can see them, and everybody knows everybody
can see them.** That is the whole mechanism. It is not a property of the
bars.

## Why this is not a breakout strategy

It would be very easy to write "buy the break of the Asian high" here, and
that is not what this is. This module says where the two lines are and how
wide the range they bound is. What to do about it is a decision, and a
decision belongs somewhere it can be held to an outcome.

## The part that is measured rather than assumed

An Asian range is only interesting when it is **narrow**. A night that
already moved a hundred pips has spent the move; the lines are still there
and the coiled-spring reading behind them is gone.

Every retail version of this hard-codes a pip threshold — twenty pips,
thirty pips — which is one number doing two jobs badly: it is far too tight
for GBP/JPY and far too loose for EUR/CHF, and it means something different
in a quiet month than a violent one. So nothing is hard-coded. The range is
reported against the **median of this instrument's own recent complete
days**, from the bars in front of it, and the ratio is handed over with the
lines. A caller that wants a threshold picks one knowing what it is a
fraction of.

## Where the session boundary comes from

Not from a clock constant here. `market::session` already knows which
centres are awake at an instant, daylight saving and all, so "overnight"
means exactly **Tokyo or Sydney open and London shut** — which moves with
the clocks in March and October without this file knowing they exist.

## What it refuses

A range still forming is not a level. If price is still inside the
overnight session, there is no completed high and low yet and this says so
rather than handing back a high that can still move — the same rule
`fxday::prior_day` follows, for the same reason.

**Public interface:**

- `fn overnight`
- `fn last_night`
- `fn against_a_normal_day`
- `fn levels`
- `fn spoken`
- `struct Overnight`

### `src/asking.rs`

**Wired** — something outside this file calls into it.

Getting the question ready before searching for it.

`recall` takes whatever it is handed and matches it against the index. What
it gets handed is a transcript of something you said out loud, and spoken
questions are a bad shape for search in three specific ways.

**They carry words that match everything.** "Can you find me the thing
about the budget" is eight words of scaffolding and one word of content.
Every one of those scaffolding words appears in half the index, so they
dilute the score of the word that mattered.

**They point at things instead of naming them.** "That file I was looking
at yesterday" contains no term the index has ever seen. Searching it
finds nothing, and nothing reads as a definite answer.

**They arrive as one question that is really two.** "What did I decide
about the trip and who was I going with" retrieves the average of two
topics and the best match for neither.

None of that needs a model. It is the cheapest possible improvement to
retrieval quality and it happens before anything expensive runs — which
matters, because a search that goes off badly wastes the expensive part
too.

What this deliberately does **not** do is guess at what you meant. It
strips, splits and flags. When a question cannot be searched as it stands,
it says which part is the problem rather than inventing a referent — an
invented one retrieves confidently and wrongly, which is worse than
retrieving nothing.

**Public interface:**

- `fn prepare`
- `struct Prepared`
- `enum Unsearchable`

### `src/astype.rs`

**Wired** — something outside this file calls into it.

Correcting as you type, in your other apps, and learning from it.

Eric, 25 Sep 2026 (H4): option 1. Atlas fixes a mistake in place as you
type it, the way phone autocorrect does. "If I go back and correct it then
it doesn't fix it for that text or email again." And then: "I would still
like Atlas to learn so it gets better at these kinds of tasks. But if it
learns too well then it will stop working in general so it needs to make
learning adaptive but still smart."

## What happens as you type

Atlas reads the text box you're typing in (only its own text, never a
password box, never an app on the `prose.never_in` list). When you pause
just after finishing a word, and that word is one of the fixes with only
one possible correction (`prose`'s certain kind: "dont", "teh",
"recieve"), Atlas deletes the word and types the correction. It reads the
box back afterwards; if the text isn't what it expected, it says so and
leaves it.

## You changing it back

If a word Atlas fixed turns back into what you typed, you changed it back.
Two things follow:

- **In that text or email:** that word is left alone for the rest of it.
That's the ruling, and it holds on the first change-back.
- **In general:** one change-back is one piece of evidence, not a lesson.

## Learning, adaptive and still smart

Each correction ("dont" → "don't") keeps two tallies: times you kept it,
times you changed it back, each counted once per text. They decide:

- **One miss never teaches it.** A correction stops being made on its own
only after you've changed it back in at least `STOP_AFTER` different
texts *and* more often than you've kept it. Until then it keeps fixing.
- **Where you changed it back matters.** Change-backs in one app (a game
chat's slang) stop it in that app first; it keeps working everywhere
else unless the change-backs are spread across apps.
- **Stopped isn't deleted.** A correction you keep changing back is
offered rather than made ("did you mean don't?" is `prose`'s flag).
- **Evidence fades.** Every tally halves every `HALF_LIFE_DAYS`, so a
correction stopped months ago comes back once the change-backs are old,
and a habit you've changed stops counting against it. Nothing it
learned is permanent, which is what keeps it from over-learning.
- **It picks up your own fixes.** When you correct a word yourself (type
"recieve", then change it to "receive") in `LEARN_AFTER` different texts,
that becomes a correction Atlas makes, provided the two are close
spellings of each other. One of your fixes is never enough, and learned
corrections fade like everything else.

**Public interface:**

- `fn close_spellings`
- `fn look_at_the_box`
- `fn start`
- `struct Tally`
- `struct Seen`
- `struct Lessons`
- `struct Made`
- `struct Text`
- `struct Watch`
- `enum Now`
- `enum Polled`
- `const STOP_AFTER`
- `const LEARN_AFTER`
- `const HALF_LIFE_DAYS`
- `const MOST_LEARNED`
- `const POLL_MS`
- `const STILL_FOR_MS`

### `src/attention.rs`

**Wired** — something outside this file calls into it.

"Pause." / "I'm ready."

Pausing must be instant and total: Atlas stops talking mid-sentence, stops
the task it is running, and stops offering things.

Since 28 Sep 2026 it also stops *listening*: the microphone's thread
(`micthread`) records nothing while paused — no wake word, no watching
while Atlas speaks. It used to keep listening so that "resume" could be
said; there are other ways back now that don't need an open microphone —
the icon by the clock, the hub's Carry on, typing it, or holding the talk
key (a deliberate press, which still works while paused).

Resuming picks up where it stopped rather than starting over.

**Public interface:**

- `fn hear`
- `struct Attention`
- `enum Heard`
- `enum Mode`

### `src/audio.rs`

**Wired** — something outside this file calls into it.

Choosing which microphone and which speakers.

This looks trivial and isn't, because of one hardware fact:

> **A Bluetooth headset cannot play high quality audio and record at the
> same time.** The moment anything opens its microphone, the connection
> drops from A2DP to a headset profile — mono, roughly 8–16kHz, and the
> music you were listening to becomes muddy.

So an assistant that naively grabs "the AirPods mic" quietly wrecks
everything else you're listening to, for as long as it's listening. With a
wake word running, that's all day.

Atlas's default is therefore to **listen on the laptop's own microphone and
speak through your headphones.** You get private replies without the codec
collapse. It only uses the headset mic when there's nothing else, or when
you tell it to — walking around, say, where the laptop mic can't hear you.

**Public interface:**

- `fn parse_devices`
- `fn parse_alsa`
- `fn parse_avfoundation`
- `fn listing_command`
- `fn parse_listing`
- `fn choose`
- `fn changed`
- `fn announce`
- `fn probe_devices`
- `fn probe`
- `fn level_db`
- `fn samples_from_le`
- `fn window_samples`
- `fn wav_bytes`
- `fn stream_args`
- `struct Device`
- `struct AudioConfig`
- `struct Selection`
- `enum Kind`
- `const SILENT_DB`

### `src/automation.rs`

**Wired** — something outside this file calls into it.

Standing watches: "when THIS has been true for THAT long, and these
conditions hold, do (or propose) this".

**Source:** Home Assistant's automation model (`home-assistant/core`,
Apache-2.0): trigger → condition → action, with a state trigger's `for:`
meaning the new state "must remain unchanged" for that long before firing,
and numeric triggers that fire on *crossing* a threshold rather than on
every reading above it. Clean-room, and with two deliberate differences:

1. Home Assistant's `for:` timer resets on restart. Here the pending timer
is plain data the caller persists, so "disk above 90% for 10 minutes"
survives Atlas restarting at minute 8.
2. An action is never executed here. A firing is a *proposal* carrying the
reason it fired; whether it runs, asks, or only tells is the caller's
(the tree's approval gates own that — nothing here can act).

**Why Atlas wants it.** Idea #1 on the 22 Sep list, ranked first: "tell me
when the homelab server disk is >90%", "alert me if a file in this folder
changes", "notice when I get mail from X". `watch.rs` checks one thing
(is a port up) with flap detection; `watching.rs` follows jobs Atlas
started. Neither is a user-defined rule. This is the rule engine; the
readings come from whatever already measures them.

**Public interface:**

- `struct Automation`
- `struct Fired`
- `struct Memory`
- `struct Engine`
- `struct RuleSpec`
- `enum Trigger`
- `enum Condition`

### `src/awake.rs`

**Wired** — something outside this file calls into it.

Staying up long enough to finish, and no longer.

The problem is real and has a bad default answer. You go to bed, the laptop
sleeps, and whatever Atlas was doing stops halfway — so the obvious fix is
to stop the machine sleeping. That's wrong: a laptop that never sleeps is
a laptop with a flat battery and a hot lid in a bag.

Windows has the right mechanism for this and almost nothing uses it
properly. You can tell the system "don't sleep while I'm doing this", scoped
to a piece of work rather than to the program, and release it the moment
you're done. Screens still turn off. The machine still sleeps the instant
the work ends.

So: **Atlas keeps the machine awake only while something is actually
running, and only for work worth it.**

**Public interface:**

- `fn running_state`
- `fn decide`
- `fn on_waking_checked`
- `fn woke_checked`
- `struct AwakeConfig`
- `struct Power`
- `struct Checked`
- `enum Because`
- `enum Hold`
- `enum LidAction`
- `enum Running`
- `enum OnWaking`
- `const BUDGET_MS`
- `const WHAT_THIS_DOES`

### `src/awareness.rs`

**Wired** — something outside this file calls into it.

Layer 1 awareness: what you are doing right now, cheaply.

Deliberately excludes screenshots and OCR. Those are Layer 3 — expensive,
privacy-heavy, and on request only. Active window title plus file-change
events covers most of what proactive assistance actually needs, at a cost
low enough to poll all day on a laptop.

**Public interface:**

- `fn describe`
- `struct Signals`
- `struct Awareness`
- `const BREAK_WINDOW`
- `const PAUSE`
- `const SCAN_BACKOFF_MAX`
- `const SCAN_BACKOFF_PRESENT`
- `const AT_THE_MACHINE`

### `src/b64.rs`

**Wired** — something outside this file calls into it.

Minimal base64 encoder. Screenshots go to vision models as base64; pulling
a crate in for 30 lines of table lookup isn't worth the dependency.

**Public interface:**

- `fn encode`
- `fn decode`

### `src/backends.rs`

**Wired** — something outside this file calls into it.

Choosing how to touch the workspace.

There is no single best mechanism. Each one is good at something and bad at
something else, so Atlas keeps all of them and picks per request:

| backend        | works on            | steals focus | needs its own copy |
|----------------|---------------------|--------------|--------------------|
| Cdp            | Chrome pages        | no           | optional           |
| Uia            | cooperating apps    | rarely       | no                 |
| HiddenDesktop  | literally anything  | never        | yes                |
| SendInput      | anything visible    | always       | no                 |

The routing rule is: cheapest backend that can do the job, on this app,
given whether we are allowed to interrupt. And it **learns** — a backend
that keeps failing on Discord stops being first choice for Discord,
without being written off everywhere else.

**Public interface:**

- `struct Spec`
- `struct Outcome`
- `struct Learned`
- `struct Request`
- `struct Router`
- `struct Choice`
- `enum Backend`
- `enum Capability`

### `src/backlog.rs`

**Wired** — something outside this file calls into it.

Nothing Atlas couldn't do gets forgotten.

When Atlas says "I can't do that right now", the request goes here with the
reason. Every tick it checks whether the blocker has cleared. When it has,
Atlas offers to pick the task back up rather than either silently doing it
or silently dropping it.

The two failure modes this exists to prevent:
* "I'll do it when we're back online" and then never doing it.
* Asking about the same stuck task every ninety seconds until you
disable the whole feature.

**Public interface:**

- `fn asked_to_wait`
- `fn now_secs`
- `struct Conditions`
- `struct Item`
- `struct BacklogConfig`
- `struct Backlog`
- `enum Blocker`

### `src/bandit.rs`

**Wired** — something outside this file calls into it.

Choosing between options while still learning which is best: Thompson
sampling over Beta posteriors.

**Sources:** Thompson (1933); Chapelle & Li (2011), *An Empirical
Evaluation of Thompson Sampling*; Russo et al. (2018), *A Tutorial on
Thompson Sampling* §3 (Beta–Bernoulli). Each option keeps a Beta(1 + yes,
1 + no) belief about how often it is welcome; to choose, draw one sample
from each belief and take the highest. Beta draws come from two Gamma
draws (Marsaglia & Tsang, 2000, *A Simple Method for Generating Gamma
Variables*). Clean-room.

**Why Atlas wants it.** When two proactive offers clear every bar at once,
`proactive::consider` took the more confident one — every time. An offer
kind you would welcome but that is never the most confident never got
raised, so its record never grew, so it never got raised: learning stops
exactly where it is needed. Sampling from the belief still favours what
you have welcomed, and still tries the others often enough to find out.

**Public interface:**

- `fn pick`
- `struct Rng`

### `src/bm25.rs`

**Wired** — something outside this file calls into it.

Okapi BM25 over an inverted index, and Reciprocal Rank Fusion to merge it
with meaning search.

**Sources:** `quickwit-oss/tantivy` (MIT) for the parameters it ships
(k1 = 1.2, b = 0.75, and the never-negative IDF
`ln(1 + (N − n + 0.5)/(n + 0.5))`); `dorianbrown/rank_bm25` (Apache-2.0)
as a second reference — it uses k1 = 1.5 and floors negative IDF with an
epsilon, which the tantivy IDF makes unnecessary. RRF from Cormack, Clarke
& Büttcher, SIGIR 2009: `score = Σ 1/(k + rank)`, k = 60. Clean-room.

**Why Atlas wants it.** `recall::word_score` already has two of BM25's three
ideas — a saturating term count (`hits/(hits+1.6)`) and a rarity weight —
but not the third: **length normalisation**. Without it a long note that
mentions a word once beats a short note that is *about* that word, and
Idea #6 (grounded answers from your own files) is exactly where notes get
long. It also scans every piece per query; this is an inverted index, so a
query touches only the documents that hold its words.

RRF is the second half. `recall` merges words and meaning as
`words·(1−w) + meaning·w·3.0` — two scores on different scales added with a
hand-set weight. RRF merges *ranks*, so it needs no scale and no weight,
and it is what the published hybrid-search systems converged on.

**Public interface:**

- `fn term_weight`
- `fn rrf`
- `struct Index`
- `const K1`
- `const B`
- `const RRF_K`

### `src/booking.rs`

**Wired** — something outside this file calls into it.

Times with other people.

Atlas finding a slot and putting it in the calendar is the version that
looks impressive and is wrong: a time with someone else is a small promise
made in your name, and the only person who can make it is you.

So Atlas does the tedious half — reading what was proposed, checking it
against what you already have, working out what else would fit — and then
stops. You say yes, no, or a different time, and only then does anything
get written down or sent.

**Public interface:**

- `fn assess`
- `fn could_offer`
- `fn to_decide`
- `fn answered`
- `fn going_stale`
- `fn stale_nudge`
- `struct Proposal`
- `struct Slot`
- `struct Assessed`
- `struct BookingConfig`
- `enum State`
- `enum Fit`

### `src/brain.rs`

**Wired** — something outside this file calls into it.

The reasoning layer.

Turns arbitrary speech into an action, using whatever model you point it
at — a local Ollama, or an API endpoint. The model is reached by shelling
out to curl through the same ExternalTool mechanism as everything else, so
there is no HTTP client compiled in and switching providers is a YAML edit.

Design rule that matters: **the model is an enhancement over the
deterministic parser, never a replacement.** If the model is down, slow, or
returns nonsense, `config/commands.yaml` phrases still work. A workspace
assistant that stops booting your workspace because a language model is
unreachable is worse than no assistant.

**Public interface:**

- `fn inline_tool_calls`
- `fn flatten_messages`
- `fn set_keep_warm`
- `fn with_keep_alive`
- `fn dig`
- `fn context`
- `fn focus_line`
- `fn recent_files_line`
- `fn orders_in_view`
- `fn head_and_tail`
- `fn announces_an_action`
- `fn decision_from_chat`
- `fn spoken_text`
- `fn parse_decision`
- `fn model_must_ask`
- `fn worth_looking_up`
- `fn is_an_action`
- `fn default_say`
- `struct LlmConfig`
- `struct Msg`
- `struct ToolCall`
- `struct ChatReply`
- `struct ChatRequest`
- `struct FallbackLlm`
- `struct ShellLlm`
- `struct MockLlm`
- `struct Decision`
- `struct Brain`
- `struct Turn`
- `struct SentenceCap`
- `struct SpeechGate`
- `struct Sentences`
- `enum Role`
- `enum Endpoint`
- `enum Reached`
- `const WARM_FOR`
- `const COLD_FOR`
- `const ACTION_SCHEMA`
- `const TALK`

### `src/brief.rs`

**Wired** — something outside this file calls into it.

The morning run.

One pass, before you sit down, that reads what arrived overnight, puts it
against the day, drafts what can be drafted, and hands you a short list of
the things only you can decide.

The shape is borrowed from how a good assistant actually reports: not
"you have 47 emails", which is an accusation, but "here is what I handled,
here are the two things I need you from". A count makes a long list sound
like a failure. A named next action makes it a morning.

Three rules hold the whole thing up:

* **Nothing sends.** Every reply this produces is a draft in the pending
state. `Outcome::Sent` cannot be reached from here at all — sending is
`mail`'s job, after you approve, and the separation is the point.
* **The brief is bounded.** A brief that grows with your inbox is one you
stop reading in a bad week, which is exactly the week you needed it.
`MAX_LINES` caps it and the overflow is counted, not printed.
* **Mail is one source, not the source.** The first version of this module
took `items` that were only ever email, which meant a machine with no
inbox had no brief at all — and this one has no inbox, so for its whole
existence the brief was `run(&[], &[], &default())` and always said
"Nothing needs you." An assistant that is useful offline has to be able
to say what is waiting from what it already holds: a friend's note at the
door, a job that failed overnight, a request it could not finish, a time
someone proposed a week ago, a post waiting on your yes. `Source` names
where an item came from and `gather` collects them. Mail keeps its place
in the list for the day there is a reader; nothing else waits on it.

**Public interface:**

- `fn run`
- `fn spoken`
- `fn from_mail`
- `fn vet_draft`
- `fn budget`
- `fn as_chain`
- `fn due`
- `fn ask`
- `fn from_handoffs`
- `fn from_backlog`
- `fn from_jobs`
- `fn handled_since`
- `fn from_bookings`
- `fn from_posts`
- `fn from_upkeep`
- `fn gather`
- `fn from_here`
- `struct Item`
- `struct Commitment`
- `struct BriefConfig`
- `struct Brief`
- `struct Sources`
- `enum Outcome`
- `enum Weight`
- `enum Source`
- `const MAX_LINES`
- `const NEVER_SENDS`
- `const HANDOFF_WAITING_URGENT_SECS`

### `src/browser.rs`

**Wired** — something outside this file calls into it.

The browser Atlas drives.

A headless Chrome, started on demand, attached to over DevTools, and shut
down when idle. Never the window you are using.

Site profiles are the piece that makes "post this to X" possible without an
API key: a small table of selectors per site, so Atlas knows which box is
the compose field and which button is Post.

**Public interface:**

- `fn default_sites`
- `fn post_plan`
- `struct BrowserConfig`
- `struct SiteProfile`
- `struct Browser`
- `enum PostStep`

### `src/budget.rs`

**Wired** — something outside this file calls into it.

Spending as little as possible on a hosted model.

Atlas works locally and free. A hosted model is only for the one thing a
3B can't do: writing real code against a large codebase. This module makes
that as cheap as it can be, and makes it impossible to be surprised by a
bill.

Four mechanisms, and they stack:

1. **Try local first.** Most tasks never reach a hosted model at all.
2. **Cheapest model that can do it.** Reading a diff is not the same job as
designing a change.
3. **Cache the codebase.** The source barely changes between requests, and
a cache read costs a tenth of a fresh one.
4. **Batch overnight work.** Half price for anything that can wait, which
is exactly what "while I sleep" means.

On top of that, a hard spending cap that stops rather than warns.

Rates are per million tokens and are checked against
<https://platform.claude.com/docs/en/about-claude/pricing>; they change, so
they live in config rather than in the code.

**Public interface:**

- `fn estimate`
- `fn first_run_estimate`
- `fn route`
- `fn allow`
- `fn overnight_estimate`
- `fn report`
- `fn untracked`
- `struct Rate`
- `struct BudgetConfig`
- `struct Job`
- `struct Spend`
- `struct Ledger`
- `enum Tier`
- `enum Difficulty`
- `enum Approval`

### `src/build_it.rs`

**Wired** — something outside this file calls into it.

Building code from a description, and proving it before handing it over.

You describe what you want; Atlas writes it, then **checks its own work
against the real toolchain** — the compiler, the linter, the tests — and
only stands behind what passes. A model that writes plausible-looking code
is common and not worth much on its own; the value is the loop that reads
the compiler's actual complaint and fixes it, and the honesty to say "this
is the best I got and here is where it still fails" when the loop runs out.

## Why the compiler is the authority, not the model

A language model's confidence that code is correct is worth nothing; the
compiler's verdict is worth everything. So generation is the cheap,
fallible step and `craft`'s ladder — `cargo check`, `clippy`, `cargo test`
— is the ground truth. This module owns the fallible half: turning a
description into a first draft, and turning a specific failure into a
specific fix. The daemon owns the loop that runs the ladder in a
`Sandbox` between rounds, because that touches the filesystem and spawns
processes; everything here is pure and takes the model as a `&dyn Llm`, so
the whole contract is tested against a mock with no toolchain at all.

## In-house or out-of-house, same check

The generating model can be the local one, an in-house crew worker, or a
Cloudflare worker when online — that is the caller's choice, and it does
not change anything here. Wherever the draft comes from, it is checked on
this machine against this machine's toolchain before it is trusted. A
draft from a bigger model online is still a draft until the local compiler
agrees.

**Public interface:**

- `fn build_loop`
- `fn generate`
- `fn fix_draft`
- `fn extract_code`
- `fn lang_from_words`
- `fn keep_building`
- `fn write_up`
- `fn ask_for_help`
- `struct BuildConfig`
- `struct Struggle`
- `enum Outcome`
- `enum Check`
- `const GENERATE_SYSTEM`
- `const FIX_SYSTEM`
- `const HELP_PROMPT`

### `src/calendar.rs`

**Wired** — something outside this file calls into it.

Your own calendar, kept here, and the bridge to the one on your phone.

The point of building this in rather than leaning on someone's app is that
it works with nothing connected: events live in Atlas's own store, offline,
and are yours whether or not a phone is paired. The native phone calendar
— the one already on your iPhone or Android, whatever app you use — is a
*sync target*, not a dependency. Nobody has to install a particular app to
use this.

Two halves, kept apart on purpose:

- **The store** (`Calendar`): add, query, remove. Pure, offline, tested.
- **The bridge** (`merge_from_phone` / `for_phone`): reconcile a batch the
phone read from its native calendar, and hand back the Atlas-made events
the phone should add to it. The actual EventKit (iOS) / CalendarProvider
(Android) calls live in the phone app — the same boundary the Android
client sits behind — so this tree holds the seam, not the platform code.

Times are Unix seconds, like the rest of the daemon. Civil-date maths goes
through `market::time`, the one date library already in the tree.

**Public interface:**

- `fn repeat_from`
- `fn resolve_when`
- `fn resolve_when_in`
- `fn reminder_from`
- `fn resolve_recurring_when`
- `fn event_title`
- `fn space_for_request`
- `fn kind_for_request`
- `fn start_of_day`
- `struct Event`
- `struct When`
- `struct CalendarConfig`
- `struct Calendar`
- `struct PhoneBatch`
- `struct PhoneEvent`
- `enum Source`
- `enum EventKind`
- `enum Repeat`
- `const PHONE_BATCH_MAX`

### `src/callnotes.rs`

**Wired** — something outside this file calls into it.

Call notes, end to end.

Eric, 24 Sep 2026: "call: yes notes, voices ask then record". This is
the part that makes those rulings do something:

1. **Notice the call.** `callwatch` sees a call app holding the
microphone. Or you say "take notes on this call".
2. **Note your side.** Your microphone only, which captures nobody else
and needs nobody's permission (`consent::Scope::YouOnly`).
3. **The others, only after a yes.** "Record everyone" gives you the
question to ask them; nothing of theirs is recorded until you say
"they said yes". "They said no", or no answer, means your side only.
4. **When the call ends,** both sides are transcribed on this machine,
put together as who-said-what in time order, summed up by the model
if there is one, and written into your notes folder.
5. **The audio goes** after `keep_audio_days`. The notes stay.

Every decision about recording goes through `consent::Recorder`; this
module only carries out its steps. That keeps the rules in one place.

**Public interface:**

- `fn who_said_what`
- `fn notes_name`
- `fn notes_text`
- `fn audio_to_delete`
- `fn write_up`
- `fn transcribe_timeout_secs`
- `fn free_name`
- `struct Call`
- `struct Said`
- `struct Finished`
- `struct Notes`
- `struct WrittenUp`

### `src/callrec.rs`

**Wired** — something outside this file calls into it.

Recording a call: your microphone, and — once they've said yes — what
the others say.

Your side comes from the microphone. The other side is what the laptop
plays, captured the way Windows lets any program capture its own sound
output ("loopback", through WASAPI). No virtual cable, no Stereo Mix, no
second program. The two are kept as separate files, which is what lets
the notes say who said what without guessing from voices.

Both are written as 16 kHz mono, the form the speech-to-text model reads,
straight to disk as they arrive: an hour is about 115 MB per side, and
nothing is held in memory beyond a second or so.

**Public interface:**

- `fn wav_header`
- `fn silence_due`
- `fn start`
- `fn silent`
- `struct To16k`
- `struct WavOut`
- `struct Recording`
- `enum Side`
- `const RATE`

### `src/callwatch.rs`

**Wired** — something outside this file calls into it.

Noticing that you're on a call.

Call notes (Eric, 24 Sep 2026: "call: yes notes") need to know when a call
starts and ends, and there's no call app Atlas could ask. Windows already
knows: the privacy page that lists "apps using your microphone" is kept
in the registry, one entry per program, and a program holding the
microphone right now has a start time and a stop time of zero. That's the
same fact the little microphone icon in the taskbar shows.

A call is a call app, or a browser (Google Meet, Teams on the web),
holding the microphone. Atlas's own listening is left out, or it would
notice itself.

**Public interface:**

- `fn mic_users_from`
- `fn call_from`
- `fn call_now`
- `struct MicUser`
- `struct Watch`
- `enum Change`

### `src/capability.rs`

**Wired** — something outside this file calls into it.

What Atlas can do, in one place you can ask.

There are now over a hundred modules, and "what can you do?" was becoming a
question only the source could answer. That's a bad sign — a system you
can't get an honest inventory of is one you stop trusting the edges of.

So every capability is registered with what it needs, what state it's in,
and what it costs. The state is the useful part: **built but never run** is
a different thing from **working**, and pretending otherwise is how you
find out at the worst moment.

**Public interface:**

- `fn all`
- `fn in_area`
- `fn working`
- `fn blocked`
- `fn what_would_unblock_most`
- `fn to_finish`
- `fn to_finish_report`
- `fn how_verified`
- `fn commissioning`
- `fn commissioning_report`
- `fn about_atlas`
- `fn is_about_atlas`
- `fn summary`
- `fn full`
- `fn since`
- `fn can`
- `fn offline_count`
- `fn runs_on`
- `fn blocked_by`
- `fn says`
- `fn on`
- `fn walled_on`
- `fn on_platform_summary`
- `fn on_platform_full`
- `fn as_markdown`
- `fn heads_up`
- `fn claimed_modules`
- `fn module_coverage`
- `fn what_uses`
- `struct Capability`
- `struct Unfinished`
- `enum State`
- `enum Area`
- `enum Finisher`
- `enum Verify`
- `const EVERY_AREA`
- `const MODULES_IN_TREE`
- `const PLUMBING`

### `src/capture.rs`

**Wired** — something outside this file calls into it.

Catching a thought before it's gone, and filing it without you deciding
where.

## The idea

Everyone's notes app is a graveyard. Not because people are disorganised,
but because capture and filing are the same action in every tool: to write
something down you must first decide where it goes, and that decision — at
the moment you have the thought — is exactly what stops you writing it
down. So the thought is lost, or it lands in one enormous untitled note
nobody reads.

Splitting the two fixes it. **Capture is instant and costs nothing**: you
say a thing, it's saved, no questions. **Filing happens afterwards**, by
Atlas, from what the thought is actually about.

The test of whether the filing worked is not whether the folders look tidy.
It's whether "where's that thing about the broker fees" finds it — which is
why what's stored is not a folder but a set of handles you might reach for
it by.

**Public interface:**

- `fn kind_of`
- `fn read_spoken`
- `fn made`
- `fn handles`
- `fn review_said`
- `fn found`
- `fn ideas_to_name`
- `fn ideas_line`
- `struct Note`
- `struct Spoken`
- `struct CaptureConfig`
- `struct Notebook`
- `enum Kind`

### `src/categories.rs`

**Wired** — something outside this file calls into it.

The four action categories from the spec, enforced rather than documented.

These existed only in prose across the six baselines. The distinction that
actually matters is the last one: work done locally is yours, and work sent
to a third-party AI service leaves your machine. That difference should be
visible in the type system, not remembered by whoever writes the next
feature.

**Public interface:**

- `fn category_of`
- `fn media_category`
- `fn media_decision`
- `fn consent_line`
- `enum Category`
- `enum MediaOp`

### `src/cdp.rs`

**Wired** — something outside this file calls into it.

Chrome DevTools Protocol backend.

This is the answer to "navigate behind the scenes": a real browser Atlas
drives over a local websocket. It clicks by CSS selector rather than by
pixel, reads the DOM directly instead of screenshotting and guessing, and
runs against a headless instance so your own Chrome window is untouched.

Deliberate choice: almost everything goes through `Runtime.evaluate` rather
than the Input and DOM domains. Telling the page `document.querySelector(x)
.click()` is one round trip and hits the right element; synthesising a
mouse event at computed coordinates is three round trips and misses when
the page scrolls between them.

**Public interface:**

- `fn js_str`
- `fn exists_js`
- `fn click_js`
- `fn fill_js`
- `fn ws_url_from_targets`
- `struct Cdp`

### `src/certainty.rs`

**Wired** — something outside this file calls into it.

Saying "I don't know".

A small local model will answer anything, confidently, including things it
has no idea about. That's worse than a slower model, because a wrong answer
delivered in the same tone as a right one has to be checked — and if you
have to check everything, the assistant has saved you nothing.

So answers are inspected before they're spoken. Not for truth, which can't
be measured here, but for the shapes an answer takes when a model is
filling a gap: invented specifics, hedging stacked on hedging, and claims
about your machine that Atlas has no way to know.

**Public interface:**

- `fn assess`
- `fn phrase`
- `fn aged`
- `struct CertaintyConfig`
- `struct Grounding`
- `enum Confidence`

### `src/chain.rs`

**Wired** — something outside this file calls into it.

Doing something that crosses several apps.

Phase 3. "Take the numbers out of the spreadsheet, put them in the report,
and send it to Marta" is three apps and four failure points, and the
interesting part is not the happy path — it's what happens when step three
fails after step two already changed something.

Two rules make this safe enough to be worth having. Nothing irreversible
happens until every reversible step has succeeded, so a chain that's going
to fail fails before it sends anything. And a step Atlas isn't sure about
stops the chain rather than guessing, because a wrong guess halfway through
a chain is much worse than a wrong guess on its own.

**Public interface:**

- `fn what_stands`
- `struct Step`
- `struct Chain`
- `enum StepState`
- `enum Next`

### `src/channel.rs`

**Wired** — something outside this file calls into it.

Saying how it's going, and saying how it went.

Atlas has one voice. A long job either narrates into silence or says
nothing until it finishes, and both are bad in the same way: you cannot
tell a system that is working from one that has stopped.

Two channels fix it, and the split has one rule that matters. **Progress is
disposable; the result is not.** You may miss every update — asleep, out of
the room, the speaker off — so the final word has to stand on its own for
someone who heard none of it. A result that says "as I mentioned" is a
result that fails for the person who most needed it.

That rule is enforced here rather than trusted, because it is the one that
decays first: it is always tempting to lean on something already said.

Ordering is `faithful`'s job — anything that went wrong leads. This decides
what may be said where.

**Public interface:**

- `struct Run`
- `enum Channel`
- `enum Leak`

### `src/chat.rs`

**Wired** — something outside this file calls into it.

Talking to the people you work with, inside Atlas.

One-to-one and groups, the shape everybody already knows from WhatsApp or
Telegram — except it lives in your hub, beside the tasks and the documents
the conversation is *about*, and no company is in the middle of it.

# What is in this file and what is deliberately not

This is the part that does not depend on how the bytes travel: rooms, who
is in them, what was said, when it was said, and whether it has actually
arrived. Transport (a direct link, a relay, mail as a fallback) and the
encryption over it are a separate concern and a separate file, and keeping
them apart is what lets the hard parts here be tested without a network.

# The three things that are easy to get wrong

**1. Sending must not wait for anybody.** You write a message when you
think of it, at your desk, at midnight, with the other person's laptop
shut. `post` therefore always succeeds and always returns immediately; the
message is real, timestamped and yours the moment it exists. Delivery is a
separate fact recorded separately — see `Delivery`.

**2. The timestamp is the sender's, and it is never rewritten.** A message
carries the wall clock of the person who wrote it, plus the offset their
machine was on. If arrival rewrote it, a message written on Tuesday and
collected on Thursday would read as Thursday, and the conversation would
quietly become a lie about when things were said. So `sent_at` is set once,
by the sender, and everything downstream treats it as evidence rather than
as something to correct.

**3. Which means wall clocks cannot be what orders the conversation.**
This is the trap under the previous point. Two machines' clocks disagree —
by seconds usually, by hours if somebody's timezone is wrong, and a laptop
that has been shut for a week can come back genuinely behind. Sort by
wall clock and you get a reply displayed above the question it answers,
which is not cosmetic: it changes what the conversation *means*.

So every message also carries `after`: a counter that says what the sender
had already seen when they wrote it. It is a Lamport clock, and the only
property it has is the one that matters here — if A was seen by whoever
wrote B, then A sorts before B, whatever the two clocks say. Wall clock
breaks ties between messages that genuinely did not know about each other,
because two people typing at once is not an ordering question, it is a
coincidence.

# Delivery is per person, never a single tick

In a group of four, "delivered" is four separate facts. A single flag that
means "at least one of them has it" is the kind of summary that reads as
reassurance and is not one — you would see a tick and assume the person you
were actually talking to had read it. So delivery is recorded per
recipient, and `fully_delivered` exists for the cases that genuinely want
the summary, spelled out rather than implied.

# Nothing here deletes anything on anybody else's machine

The same rule `mail.rs` states for your mailbox and for the same reason,
one step further out: once a message is on somebody else's computer it is
theirs. Atlas can stop showing it to you. It cannot reach over and remove
it, and an "unsend" that only clears your own copy while claiming more
would be worse than not having one.

**Public interface:**

- `struct Message`
- `struct Room`
- `struct Chats`
- `enum Delivery`
- `enum Refused`
- `const ME`
- `const FILE`

### `src/checks.rs`

**Wired** — something outside this file calls into it.

What Atlas can actually check and change on this machine.

`tune` measures — how much memory something is holding, how long a startup
item costs. This is the other half: the specific, named Windows mechanisms
behind those measurements, so a finding can say *run this* rather than
*something is wrong*.

Every entry came out of watching what people actually do to Windows and
then checking it against the mechanism underneath. Most of it is Microsoft's
own tooling that ships turned off or buried.

The organising idea is the same one `system` already uses: **reversibility
decides the gate, not how impressive the action sounds.** A keyboard repeat
rate is nothing. Deleting the component store is one-way. Turning off a
security feature to gain frames is a trade you make, not one Atlas makes.

Three things this deliberately will not do, listed in `NEVER` below with
reasons. They are not configurable, and `tests/guards.rs` fails the build
if the list stops existing.

**Public interface:**

- `fn is_refused`
- `fn by_id`
- `fn read_only`
- `fn reversible`
- `fn needs_approval`
- `fn first_pass`
- `struct Check`
- `enum Undo`
- `enum Kind`
- `const NEVER`
- `const CHECKS`

### `src/checkup.rs`

**Wired** — something outside this file calls into it.

A fast, on-device self-check.

The test suite — thousands of tests across hundreds of files — is a
*development* gate. It runs on a build machine before a release, proving the
code is sound; it is not something a person runs on their computer to use
Atlas, and nobody should have to sit through it. What a fresh install
actually needs is a different, much smaller question: does Atlas *work on
this machine*, right now?

This is that check — a handful of fast probes of the parts everything else
rests on (its store, its memory, the screen, the model), plus an honest
count of what's ready here. Seconds, offline, and safe to run any time: it
writes only to its own probe key and opens nothing, sends nothing, changes
nothing of yours.

**Public interface:**

- `fn all_clear`
- `fn report`
- `struct Check`
- `enum Outcome`

### `src/chords.rs`

**Wired** — something outside this file calls into it.

Key chords Atlas answers to anywhere -- Ctrl+Alt+Space captures what's
selected as a note, Ctrl+Alt+E expands the snippet word you just typed,
Ctrl+Alt+T copies the text off the window in front -- without ever
watching your typing.

**How, and why this way.** Windows' `RegisterHotKey` asks the OS to tell
Atlas when one exact chord is pressed, and nothing else. Unlike the
push-to-talk key (`hotkey`, a low-level hook that sees every key so it can
hold one back), a registered chord can't see what you type. That's the
property snippet expansion needs: Espanso-style expansion reads every
keystroke; this reads none. When the expand chord comes, Atlas selects
the word before the cursor (Ctrl+Shift+Left), copies it through the
clipboard, puts your clipboard back as it was, and replaces the word only
if it is exactly one of your triggers (`snippets::Snippets::exact`).

A chord another program already owns fails to register, and that's said
(`doctor`), not swallowed.

**Sources:** Microsoft's `RegisterHotKey` documentation (MOD_NOREPEAT so
a held chord fires once); PowerToys' Keyboard Manager read for which
chords Windows itself reserves (Win+letter), which is why the defaults
use Ctrl+Alt.

**Public interface:**

- `fn read_chord`
- `fn plausible_trigger`
- `fn expand`
- `fn start_chords`
- `struct Chord`
- `struct ChordsConfig`
- `enum Does`
- `const MOD_ALT`
- `const MOD_CONTROL`
- `const MOD_SHIFT`
- `const MOD_WIN`
- `const MOD_NOREPEAT`

### `src/chunker.rs`

**Wired** — something outside this file calls into it.

Cut a document into pieces small enough to search and quote, and keep the
line numbers so every answer can say where it came from.

**Source:** `benbrandt/text-splitter` (MIT) — its idea is a ladder of
semantic levels (characters → words → sentences → line breaks), splitting
at the *largest* level that fits and packing neighbours up to a maximum,
with an optional overlap that must be smaller than the chunk. Clean-room;
this version adds what Atlas specifically needs and text-splitter does not
carry: **1-based line spans** and the **heading path** of each chunk.

**Why Atlas wants it.** Idea #6 is "grounded, cited answers from your own
material — citing the source file+line". `recall` stores whole notes. A
40-page document as one piece either matches everything or quotes the wrong
paragraph; as chunks with spans, the answer can say `plan.md:112-131`.
Chunks never cross a markdown heading, because a chunk that is half one
section and half the next cites a place that says neither thing.

**Public interface:**

- `fn chunk`
- `struct Chunk`
- `struct ChunkConfig`

### `src/civil.rs`

**Wired** — something outside this file calls into it.

Calendar arithmetic shared by `recur` and `cronspec`.

Howard Hinnant's days-from-civil / civil-from-days (public domain, see
<https://howardhinnant.github.io/date_algorithms.html>). Written out here
rather than taken from `triage.rs` or `digest.rs` so this crate builds
alone; the merge should collapse the three copies into one.

Every time in this crate is **local seconds**: unix seconds plus the
caller's UTC offset. That is honest about one limit — a fixed offset does
not know about daylight-saving changes. See `HANDOFF.md`, gap G1.

**Public interface:**

- `fn days_from_civil`
- `fn weekday`
- `fn is_leap`
- `fn days_in_month`
- `struct Civil`

### `src/cli.rs`

**Wired** — something outside this file calls into it.

Reading the words off the command line.

`main` builds its `words` list by dropping every `--flag` before anything
looks at it. That is the right default for a command that takes no flags
and silently wrong for one that does: `atlas remote done 1 "it rendered"
--secs 200` arrived as `done 1 it rendered`, the duration fell back to
"no time at all", and the reply said the job had finished too quickly to
be worth mentioning. Nothing errored. The sentence was just false.

That was found by running the command rather than by reading it, which is
why these two functions live here — in the library, where a test can reach
them — instead of beside their callers in `main.rs`, where nothing can.

**Public interface:**

- `fn tail_after`
- `fn plain_words`
- `fn flag_value`

### `src/clients.rs`

**Wired** — something outside this file calls into it.

Who your clients are, so a reply to one can be treated differently
from a reply to anyone else.

Deliberately not inferred. Atlas could probably guess "this looks like
a client" from reply patterns and engagement, the same shape of thing
`unsub.rs` already does for the opposite question — but guessing wrong
here means either drafting an unwanted reply to a stranger, or,
eventually, sending one. `unsub.rs` guessing wrong costs an unopened
newsletter kept a while longer; this guessing wrong costs a client
relationship or a stranger's inbox. Explicit addition only.

**Public interface:**

- `struct Client`
- `struct ClientList`

### `src/clipboard.rs`

**Wired** — something outside this file calls into it.

The clipboard as a way of telling Atlas what you mean.

Idea #1 on the list, and the cheapest context Atlas can get. You copy
something — an error, a paragraph, a table, a link — and say "explain
this". No screenshot, no vision model, no guessing which window you meant.
You already selected exactly the thing.

The reply goes back to the clipboard, so you paste it where you were.

**Public interface:**

- `fn classify`
- `fn take`
- `fn prompt`
- `fn refers_to_clipboard`
- `struct ClipboardConfig`
- `struct Grab`
- `enum Kind`
- `const ANSWER_SYSTEM`

### `src/cliphist.rs`

**Wired** — something outside this file calls into it.

What you copied, kept for a while so it can be found and pasted again.

**Off unless you turn it on** (`clipboard_history.enabled`). `clipboard.rs`
was written on the rule that Atlas never watches the clipboard, because a
clipboard monitor sees every password you copy. Round 11 builds the
history you asked for, and keeps that rule's reason rather than its
letter:

- **Nothing is read on a timer.** The tick asks the OS one number -- the
clipboard's sequence number -- and only when it has moved is the copy
read, once (`Platform::clipboard_change`, `clipboard_copy`).
- **A password manager's "don't keep this" is obeyed before any text is
read.** Windows' `ExcludeClipboardContentFromMonitorProcessing`,
`CanIncludeInClipboardHistory = 0` and `Clipboard Viewer Ignore` (the
formats KeePass, 1Password and Bitwarden set) make the copy `Private`,
and its text never enters this process.
- **Anything that looks secret is not kept either**, whoever copied it:
keys, tokens, card and account numbers (`redact::secrets_in`).
- **Nothing leaves the machine.** The history isn't synced, isn't sent to
a model, and isn't in backups' `upgrade::YOURS` list of things to carry.
- **It forgets.** Entries older than `keep_hours` go, and there are never
more than `max_items`; one item is capped at `max_chars`.

**Sources:** Ditto (GPL-3.0; read for its ideas only -- search, paste
again, the app a copy came from); Microsoft's clipboard documentation for
`GetClipboardSequenceNumber` and the do-not-keep formats; the CrossPaste
and Bitwarden issues that established which formats password managers
actually set.

**Public interface:**

- `fn line`
- `struct HistoryConfig`
- `struct Clip`
- `struct History`
- `enum Skipped`

### `src/cloudsync.rs`

**Wired** — something outside this file calls into it.

Setting up the folder two devices meet in.

The cloud path is the workhorse: it doesn't need both devices on the same
network, or even on at the same time. One writes a bundle, the other picks
it up whenever it next runs.

Atlas can do nearly all of this itself. What it can't do is sign you into
anything — so the split is: you install the app and sign in once, and Atlas
finds it, makes its folder, checks the sync is actually working, and never
bothers you about it again.

**Public interface:**

- `fn space_needed_mb`
- `fn free_tier_is_enough`
- `fn laptop_steps`
- `fn phone_steps`
- `fn where_to_look`
- `fn compare_providers`
- `fn setup_guidance`
- `fn still_syncing`
- `fn setting_up`
- `fn result`
- `struct Step`
- `struct CloudConfig`
- `enum Provider`
- `enum Trouble`
- `enum Setup`
- `const WHY_ONEDRIVE`
- `const ABOUT_BUNDLES`
- `const WHAT_YOU_DO`

### `src/codes.rs`

**Wired** — something outside this file calls into it.

Recovery codes — the thing built for exactly your situation.

## Why this and not turning it off

Your problem is precise: you need to sign in from a government computer,
where you can't receive a text and may not have your phone. That is the
*specific* problem recovery codes were invented for. They are ten one-time
strings, printed on paper, that work as the second factor on any machine,
with no phone, no signal, no app and no key. You type one, you're in, and
that code is spent.

They beat turning two-factor off on every axis that matters to you:

* They work on a locked-down machine where an authenticator app can't be
installed.
* They don't require you to already be signed in to arrange — which
disabling does, and which is the hole in that plan.
* The account stays protected for everyone who isn't holding your paper.
* You can regenerate a fresh ten before each deployment.

Ten logins per account is usually a deployment's worth. Where it isn't,
most services let you print a new set, and Atlas tracks how many you have
left so you find out before you're down to your last one.

**Public interface:**

- `fn where_to_get`
- `fn gaps`
- `fn used_one`
- `fn before_you_go`
- `fn logins_available`
- `fn worth_raising_now`
- `struct Set`
- `struct CodesConfig`
- `struct Gap`
- `const THE_LIMIT`
- `const WHY_THIS_WORKS`

### `src/companion.rs`

**Wired** — something outside this file calls into it.

Atlas on your phone.

An assistant that needs you to be at a particular laptop is a filing
cabinet with opinions. The point is that what you were doing is where you
are.

## Superseded — see `sync.rs`

This module was built on the idea that the phone should be a window rather
than a real Atlas, to avoid two copies of your state drifting apart. That
was the wrong call: it made Atlas useless for the months a laptop is off,
which is most of the point of having it.

`sync.rs` has the answer — sync *what happened* rather than *state*, and
two full copies merge cleanly however long they've been apart. What's left
here is still true and still used: the rules about what must never travel
to a device you might lose in a taxi.

**Public interface:**

- `fn never_travels`
- `fn unbuilt`
- `fn merge`
- `fn on_return`
- `fn how_they_talk`
- `struct Pending`
- `struct CompanionConfig`
- `struct Phone`
- `enum Piece`
- `enum Merge`
- `const WHAT_IT_CANNOT_DO`
- `const WHY_NOT_TWO_ATLASES`

### `src/config.rs`

**Wired** — something outside this file calls into it.

Every knob Atlas has, in YAML rather than in code.

The reason for this being a hard rule rather than a preference: a setting
compiled into the program is one nobody can change without rebuilding, and
this has to be usable by people who will never run a compiler. If it's a
choice, it lives in a file with a comment saying why the default is what it
is.

Loading is deliberately strict — a mistyped key is an error rather than a
silently ignored line, because a setting you think you changed and didn't
is worse than one that refuses to load.

**Public interface:**

- `fn settings_that_do_nothing`
- `struct AppSpec`
- `struct AppsConfig`
- `struct FracRect`
- `struct RoleSpec`
- `struct LayoutsConfig`
- `struct CommandSpec`
- `struct CommandsConfig`
- `struct Config`
- `enum RoleMatch`
- `const NO_FIELD_TO_LAND_IN`
- `const PARSED_AND_NEVER_READ`

### `src/confirmed.rs`

**Wired** — something outside this file calls into it.

Doing something on a security page, with you there.

You've scoped this tightly and the scope is what makes it workable: you're
at the machine, you say what you want, Atlas repeats it back in your own
terms, and nothing happens until you say yes.

That's a genuinely different thing from a system that *can* change security
settings. The capability here is bounded by your presence, your instruction
and your confirmation, in that order, every time — and none of the three
can be assumed from the others.

The read-back is the part that does the work. Not because you'd forget what
you asked, but because it's where a misheard instruction surfaces: "turn it
off on Instagram" and "turn it off on Instagram and Facebook" sound similar
and Instagram's settings page changes both.

**Public interface:**

- `fn consequence`
- `fn labels_for`
- `fn press_js`
- `fn pressed_from`
- `fn needs_reading_back`
- `fn saying_it_back`
- `fn read_back`
- `fn answer`
- `fn before_a_run`
- `fn record`
- `fn how_to_undo`
- `fn site_of`
- `fn host_of`
- `fn before_pressing`
- `struct Asked`
- `struct ConfirmConfig`
- `struct Run`
- `struct Record`
- `enum Change`
- `enum Pressed`
- `enum Step`

### `src/connectivity.rs`

**Wired** — something outside this file calls into it.

Internet is an enhancement, never a dependency.

Atlas runs on your laptop. Everything that makes it useful day to day —
hearing you, speaking, controlling the workspace, searching your files,
remembering, scheduling, reasoning — happens locally with no network at
all. The internet only adds *reach*: web research, and optionally a hosted
model if you choose one over the local default.

Two rules enforced here:
1. A local capability must never be blocked by a network check.
2. Something that genuinely needs the internet is deferred and retried,
not failed silently and forgotten.

**Public interface:**

- `fn need_of`
- `fn deferral_message`
- `struct ConnectivityConfig`
- `struct Connectivity`
- `enum Reach`
- `enum Need`

### `src/consent.rs`

**Wired** — something outside this file calls into it.

Recording a call, and telling people you are.

You asked how consent would actually work. It's the part that decides
whether this feature is usable or a liability, so it's the part that gets
built first rather than bolted on.

The legal shape, roughly: some places require only one party to consent —
you, since you're in the call. Others require **everyone**. Getting it
wrong isn't a technicality; in two-party jurisdictions it's a criminal
matter. Atlas has no way to know where the other people are sitting.

So the design refuses to guess:

1. **Your side is always safe.** Recording your own microphone captures
only you. That alone gives you your own notes, and needs nobody's
permission.
2. **Everything else is announced.** To record the whole call, Atlas says
so at the start — out loud or in the chat — before anything is captured.
3. **Silence is not consent.** If the announcement can't be delivered,
nothing is recorded. Ever.
4. **Anyone can stop it.** One objection ends recording and discards what
was captured.

**Public interface:**

- `fn script`
- `fn script_line`
- `fn the_announcement`
- `fn announcement_named`
- `fn who_gets_told`
- `fn explain`
- `struct ConsentConfig`
- `struct Recorder`
- `enum Scope`
- `enum Rule`
- `enum State`
- `enum Step`
- `const QUESTION`
- `const ANNOUNCEMENTS`

### `src/consolidate.rs`

**Wired** — something outside this file calls into it.

One thing learned once, however many times you ask about it.

Two problems, and they're the same problem seen from either end.

**Asking differently shouldn't cost you what you knew.** You ask about the
wash sale rule in January and again in March, phrased differently. Without
this, that's two notes: two timestamps, two decay curves, and confidence in
a settled fact drifting down because you happened to ask twice. Nothing
changed except your wording.

**And a store that only grows is a store you eventually turn off.** But
size isn't the right thing to cap — it's *value density*. A settled fact
costs a few hundred bytes and never needs looking at again. Twelve stale
quotes cost the same and are worth nothing.

So: the same claim arriving again **merges and strengthens** rather than
duplicating, and what gets dropped when space is short is chosen by what
it's worth rather than by when it arrived.

**Public interface:**

- `fn same_claim`
- `fn learn`
- `fn worth_keeping`
- `fn make_room`
- `fn trim_with_stones`
- `fn trim`
- `fn over_budget_on_purpose`
- `fn compact`
- `fn worth_compacting`
- `fn tombstone`
- `fn once_knew`
- `fn knew_once`
- `fn dropped_note`
- `fn size_note`
- `struct Claim`
- `struct Confirmation`
- `struct Tombstone`
- `struct ConsolidateConfig`
- `enum Density`
- `const ASKING_AGAIN_IS_FREE`
- `const NEVER_DROPPED`

### `src/consult.rs`

**Wired** — something outside this file calls into it.

Talking a problem through in a window.

Atlas has already tried everything on its ladder. Now it takes the write-up
into a conversation that's already open and works through it — sending,
waiting, reading, and following up until something testable comes back.

Two rules make this behave like a person rather than a script:

1. **Wait for the whole answer.** A reply that's still arriving looks like
a short reply. Acting on half of one is how you end up applying the
first paragraph of a two-paragraph fix.
2. **A turn is spent on a solution, not on a sentence.** A question back, a
request for more detail, an explanation with no code — none of those cost
an attempt. Atlas reads them, works out what's being asked, answers, and
carries on. The budget is only touched when there's something to test.

**Public interface:**

- `fn settled`
- `fn classify`
- `struct ConsultConfig`
- `struct Exchange`
- `struct Consultation`
- `enum Reply`
- `enum Move`

### `src/content.rs`

**Wired** — something outside this file calls into it.

Running your content.

Not writing it for you — that produces the flat, interchangeable stuff
everyone can spot. What a manager actually does is know **why** something
worked, notice when you're about to repeat a mistake, and handle the
tedious half so you can make more.

The knowledge here is about short-form video specifically, because that's
where the rules are unusually firm: attention is decided in the first
second, retention is the only metric that compounds, and almost every
failure is one of about six things.

**Public interface:**

- `fn hook_of`
- `fn faults`
- `fn learn`
- `fn before_posting`
- `fn how_its_going`
- `fn edits_it_can_do`
- `struct Piece`
- `struct Performance`
- `struct Learned`
- `struct ContentConfig`
- `enum Hook`
- `enum Fault`

### `src/contents.rs`

**Wired** — something outside this file calls into it.

Knowing the map without carrying the territory.

A memory folder that has grown for a year cannot be read at boot. Atlas
either loads all of it — slow, and most of it irrelevant to whatever you
just asked — or loads none of it and does not know what it has.

The way out is an index note in every folder, a bullet index at the top of
every daily note, and one master index at the root outside every folder.
**At boot Atlas reads the master index and nothing else.** It learns what
exists and where, and loads a note only when a task needs it.

The difference is between reading forty notes to find one fact and reading
one line to know which note holds it.

Two rules stop this decaying into a second thing to maintain:

* **One line per item, and the line says what is inside, not what it is
called.** "spain-trip — flights booked, hotel undecided" is worth reading.
"spain-trip.md" is the filename again.
* **An index that disagrees with its folder is worse than none**, because
Atlas trusts it and stops looking. `drift` finds that, and a stale index
is a fault rather than a detail.

**Public interface:**

- `fn what_to_open`
- `fn drift`
- `fn boot`
- `fn master_path`
- `fn names_on_disk`
- `fn says_for`
- `fn from_folder`
- `fn parse`
- `fn save`
- `fn load`
- `fn rebuild`
- `struct Line`
- `struct Contents`
- `struct Drift`
- `struct Boot`
- `const MAX_LINES`
- `const MASTER_FILE`
- `const HEADER`
- `const MAX_SAYS_CHARS`

### `src/council.rs`

**Wired** — something outside this file calls into it.

A room of people who do not agree with each other.

Asking one model a question four times gets you the same answer four times
in different words. Asking four *seats* — each with its own brief, its own
disposition, and no sight of the others — gets you the thing a board
meeting is actually for: the disagreement.

Three rules make this work, and dropping any one of them collapses it back
into an expensive way to ask one question:

1. **Seats answer blind.** No seat sees another's answer before giving its
own. Show them and they converge, which is the entire failure this is
built to avoid. `Round::Blind` enforces it and `Round::Open` — the
follow-up round where they *do* see each other — can only happen after.
2. **The verdict names the split.** A verdict that averages four opinions
into a moderate one has destroyed the only thing the council produced.
Where seats disagree, the disagreement is the output.
3. **Unanimity is suspicious, not reassuring.** Four seats agreeing on the
first blind round usually means the brief leaked the answer. It gets
said out loud rather than reported as high confidence.

`otherside` argues one case against a decision you have already made. This
is the plural version, for decisions you have not made yet.

**Public interface:**

- `fn is_hardware_question`
- `fn hardware_room`
- `fn conditions`
- `fn flat_noes`
- `fn amended`
- `fn retest_rooms`
- `fn is_build_question`
- `fn build_room`
- `fn is_security_question`
- `fn security_room`
- `fn room_for`
- `fn parse_opinion`
- `fn empty_chairs`
- `struct Seat`
- `struct Opinion`
- `struct Verdict`
- `struct Council`
- `enum Disposition`
- `enum Lean`
- `enum Round`
- `const MIN_SEATS`
- `const MAX_SEATS`
- `const IF_AGAINST`
- `const MAX_RETESTS`

### `src/courier.rs`

**Wired** — something outside this file calls into it.

Holding a message until the other person's Atlas can take it.

# The shape, in one paragraph

You say something. Your Atlas writes it down with your clock on it
(`chat.rs`) and hands it here. This tries to give it to *their* Atlas —
on whichever of their devices is up, phone or computer, it does not care
which. If none is up it keeps hold of it and tries again. When one
appears, it goes, and the timestamp is still the moment you wrote it,
exactly like a text sent while their phone was off.

# What is deliberately not decided here

**How two Atlases reach each other.** Whether that is a direct connection
over a private network, a link across a local wifi, or something else, it
arrives here as a `Transport` and this file never learns which. That is
not tidiness: the reachability problem is a platform problem — carrier
NAT, and what a phone operating system will let an app do in the
background — and the answer differs per platform and will change again.
Everything in this file is the same whichever way it resolves, which is
why it is built first and tested against a transport that exists only in a
test.

# Delivery is to a person, not to a device

Somebody has a phone and a computer. A message is theirs once *either* has
taken it, and the second device must not receive a duplicate — a
conversation that repeats itself on the laptop because it also arrived on
the phone is worse than one that arrives late. So devices are tried in
turn, one acknowledgement ends the attempt for that person, and their
other devices get it from their own Atlas rather than from yours.

# An attempt is not a delivery

Handing bytes to a socket is not arrival, and `chat::Delivery` has no
state for it on purpose. Only an acknowledgement from the far side marks
a message `Arrived`. Anything else leaves it `Waiting`, which is the
honest answer and the one that keeps it in the outbox where you can see
it.

# Trying forever is its own failure

A message that has been failing for days is not "waiting", it is stuck,
and the difference matters because a person can act on one and can only
sit through the other. `Attempts` backs off so a shut laptop is not
hammered, and after long enough says so in words rather than going quiet.

**Public interface:**

- `fn run`
- `fn send_receipts`
- `fn nothing_can_move_yet`
- `struct Device`
- `struct Attempts`
- `struct Round`
- `struct Tries`
- `struct NoLink`
- `enum Kind`
- `enum Handoff`

### `src/craft.rs`

**Wired** — something outside this file calls into it.

Building your things, offline.

The instinct for "make it better at coding" is a bigger model, and on this
machine that instinct has nowhere to go. A model that fits alongside
everything else is not going to write good Rust from a rough description,
and pretending otherwise produces confident code that does not compile.

But generation is not where a local setup loses. It loses on the loop
around the generation, and that is entirely fixable, because the strongest
signal available is free, local, and instant: **the toolchain already knows
whether the code is right.**

Two ideas do the work here.

**Cheapest signal first.** A type error found by `cargo check` in two
seconds is the same error a test suite finds in forty, except it arrives
with a line number and a suggestion. A weak model iterating against precise
compiler errors converges. The same model iterating against test output
flails, because test output describes a symptom and a compiler describes
the cause.

**A failed gate makes the later ones meaningless.** Running tests on code
that does not compile produces noise, and noise is worse than nothing —
it gives the next attempt something confident and wrong to work from.

Everything here runs with no network. That is not a compromise: for this
job the local tools are the authority, and a model somewhere else is the
thing guessing.

**Public interface:**

- `fn ladder`
- `fn read_ladder`
- `fn still_worth_running`
- `fn lang_of_dir`
- `struct Gate`
- `struct Ran`
- `struct Spec`
- `enum Lang`
- `enum Tells`
- `enum Next`
- `enum Ask`

### `src/crash.rs`

**Wired** — something outside this file calls into it.

Atlas knowing it has crashed.

Before this, it could not. There was no `panic::set_hook`, no
`catch_unwind`, no watchdog and no restart anywhere in the tree — the only
mention of `catch_unwind` in `src` was in `mend.rs`, listing it as a
*cheat to detect*. So a single `unwrap` on a malformed file ended the
process, the console window closed, and nothing brought it back. The next
time you started Atlas it would greet you as though nothing had happened,
because as far as it knew, nothing had.

That matters more here than in most programs, for two reasons this project
has already written down elsewhere: Atlas is meant to be always-on and
handed to friends, so the person in front of it is often not the person
who could read a stack trace; and "Atlas works on itself" cannot mean
anything while Atlas cannot report its own failures.

Three pieces, deliberately small:

1. **A note, written at the moment of the panic.** Not a log line — a
dated file in the store, so it survives the process dying and is still
there next start.
2. **A caught tick.** One bad intent must not end the session. The daemon
wraps the tick body; a panic inside it becomes a sentence and the loop
goes round again.
3. **One sentence, next start.** Said out loud, once, and then cleared —
a crash you are never told about is the same as no crash report at all.

What this deliberately does **not** do: restart Atlas, register a service,
or retry the thing that panicked. A panic means an assumption in the code
was wrong; repeating it immediately is how a crash becomes a loop. The
note says what happened and the next tick carries on with the rest.

**Public interface:**

- `fn note_path`
- `fn watch`
- `fn last`
- `fn take`
- `fn caught`
- `fn may_start_again`
- `struct Note`
- `const AGAIN_AT_MOST`
- `const AGAIN_WINDOW_SECS`

### `src/credentials.rs`

**Wired** — something outside this file calls into it.

Every credential Atlas holds, in one place you can check.

You asked a fair question and the answer had drifted, so here it is
pinned down: **Atlas has never had your passwords, and there is no code in
it that logs into anything.** What it does instead is ride sessions you
established yourself — you're signed into LinkedIn in your browser, so
Atlas can post there; you're not, so it can't and says so.

That's a real distinction rather than a technicality. A session is scoped
to one browser profile on one machine, expires, and can be revoked from the
site. A password is the account itself.

The one exception is mail, which needs a credential because IMAP has no
concept of "the session you already have". That one is an **app password**:
issued separately, revocable from the account without changing anything
else, and scoped to mail only. It is not your password, and if it leaks you
revoke it and nothing else moves.

This module exists so that stays true. Every credential is registered here
with what it opens and where it lives, and there's a test that nothing
sensitive is sitting in a config file.

**Public interface:**

- `fn all`
- `fn never_held`
- `fn misplaced`
- `fn needs_you_awake`
- `fn spoken`
- `fn written`
- `struct Credential`
- `enum Opens`
- `enum Kept`
- `const CAN_IT_LOG_IN`
- `const SESSION_VS_PASSWORD`

### `src/crew.rs`

**Wired** — something outside this file calls into it.

Background work that does not block the tick.

`scheduler` decides *when* a job is due; `lanes` decides *whether* it may
touch the screen; both then ran the job **on the tick thread**, so a job
taking four minutes was four minutes in which Atlas did not listen,
notice, report, or answer. `crew` is the place slow work goes instead.

`Crew::hand(name, work)` takes an errand; `Crew::settle(t)` is called once
a tick and never blocks. An errand runs on another thread, so it must own
everything it needs — it cannot borrow the daemon. That is not a
limitation to design around, it is the line `lanes` already draws:
background work is self-contained and goes to the crew; foreground work
needs your windows, and driving an app stays on the tick.

Rules, each with a test below:
- **The tick never waits.** Not even stopping: `ask_to_stop` sets a flag
and returns. The ending arrives through `settle` like every other
ending. The one place waiting is correct is shutdown.
- **Work that will not stop is said out loud.** A called-off errand still
running after `WONT_STOP_AFTER_SECS` is reported once as `WontStop`.
- **An errand that dies without a word is `Vanished`, not finished.** A
thread that panics drops its channel and says nothing; read naively
that is indistinguishable from work still in progress, forever.
- **Stopped is not failed.** Getting this wrong means being told off for
changing your mind.
- **More work than hands waits.** It does not fail and does not all
start — bounded, so a peer or a bug filling the queue fills a list
rather than memory.
- **Pausing holds, it does not erase.** `pause` sets a second flag; an
errand that reaches [`Control::checkpoint`] parks there with everything
it has done still on its own stack, and `resume` lets it carry on from
that exact point. Eric's ruling (23 Sep 2026): a single-errand control
*pauses but doesn't erase what it is doing*. A parked errand does not
hold a hand, so queued work can use it meanwhile.
- **Work says what it costs, and is admitted by that** ([`Needs`]). One
whole-machine job at a time with nothing thinking beside it; one-core
work capped at `cores - 1` so a core is left for you; waiting work
(downloads, copies, mail) on its own allowance, never behind a render.
Nothing that thinks starts under the memory margin, and whole-machine
work Atlas chose itself waits for mains under the battery floor.
- **Urgency ages** ([`Urgency`], `AGE_UP_SECS`), so nothing waits forever
at the bottom, and a whole-machine job at the front holds the thinking
hands rather than being kept out by a stream of small ones.
- **The same work is done once** ([`Job::keyed`]).
- **What each errand waited and ran is kept** (`recently_finished`,
`longest_wait`), and `why_waiting` says which rule is holding a job.
- **Shutdown waits, but not forever.** `Drop` asks and waits with a
deadline; anything still running past it is left to run detached
rather than hanging the process on the way out.

**Public interface:**

- `fn spoken_ms`
- `struct Control`
- `struct Errand`
- `struct News`
- `struct Job`
- `struct Room`
- `struct Limits`
- `struct CrewConfig`
- `struct Timing`
- `struct Crew`
- `enum State`
- `enum Ending`
- `enum Needs`
- `enum Urgency`
- `enum Taken`
- `const WONT_STOP_AFTER_SECS`
- `const SHUTDOWN_DEADLINE_SECS`
- `const MAX_WAITING`
- `const AGE_UP_SECS`
- `const WAITING_HANDS`
- `const RECENT_KEPT`

### `src/cronspec.rs`

**Wired** — something outside this file calls into it.

Standing jobs on a clock: five-field cron expressions.

**Source:** Vixie cron semantics, with `Hexagon/croner-rust` (MIT) read as
the reference for the extensions (`L`, `#`, nicknames) and for the
day-of-month/day-of-week rule. Clean-room.

**Why Atlas wants it.** Idea #3 on the 22 Sep list — "every morning
summarise overnight trades", "hourly server health check" — needs a
schedule the user sets and Atlas keeps across restarts. `recur` is for
calendar events a person attends; this is for jobs Atlas runs, where
"every 15 minutes during market hours on weekdays" is one line.

The one trap worth naming: when BOTH day-of-month and day-of-week are
restricted, classic cron fires when EITHER matches (`0 9 1 * MON` = the 1st
AND every Monday). Most people read it as AND. This keeps Vixie's OR (so a
crontab pasted from anywhere means what it meant there), accepts croner's
`+` prefix on the weekday field to ask for AND, and `describe()` says which
one it is doing.

**Public interface:**

- `struct Cron`

### `src/cutcheck.rs`

**Wired** — something outside this file calls into it.

Reading a video's cuts: where they are, and where the eye goes across each.

`editcraft` knows the rule that matters most in an edit — a cut that
moves the viewer's eye across the frame reads as a jump — and had nothing
that looked at a video. This finds the cuts with ffmpeg's scene score, takes
the frame either side of each, and finds where the eye would be on each:
the centre of the image's strongest detail (edge energy), which is where a
viewer looks first on an ordinary shot. Then `editcraft` says which cuts
jump. A proxy for attention, not a measurement of it; it reads "a face on
the left, then a face on the right" correctly and can be fooled by a busy
background.

**Public interface:**

- `fn cuts`

### `src/daemon.rs`

**Wired** — something outside this file calls into it.

The always-on core.

This is what turns Atlas from a command you type into something that runs
all day: wake word, spoken turn, spoken reply, follow-up without needing
the wake word again, plus a background tick that runs scheduled work and
decides whether to speak first.

Voice in, voice out. Typing is a debugging affordance, not the interface.

**Public interface:**

- `fn parse_offset`
- `fn undo_intent`
- `fn gesture_asked`
- `fn key_spoken`
- `struct Daemon`
- `struct WorkingForYou`
- `enum Autonomy`
- `enum Arrival`
- `const PERSIST_SWEEP_SECS`
- `const WINDOW_JOB_IDS`
- `const QUESTION_LIFETIME_SECS`
- `const SAFETY_SENTENCES`
- `const CHATTING_FOLLOWUP_SECS`
- `const HISTORY_EXCHANGES`
- `const HISTORY_TOKENS`
- `const FACTS_IN_PROMPT`
- `const NOTE_HINT_FLOOR`
- `const RETRIEVED_TOOLS`

### `src/daily.rs`

**Wired** — something outside this file calls into it.

The day as a unit.

A task list that never ends is one you stop trusting, because "outstanding"
quietly comes to mean "everything I have ever thought of". Closing it at
midnight and opening a fresh one forces the useful question every morning:
is this still worth doing today?

**The thinking does not reset.** That's the important half. A task carried
forward six times carries its whole history with it — what was tried, what
it's stuck on, why it keeps slipping — because that history is the reason
it's still there and the thing that tells you to drop it.

**Public interface:**

- `fn day_of`
- `fn day_of_with`
- `fn has_rolled`
- `fn whereabouts`
- `fn arriving`
- `fn close`
- `fn opening`
- `fn thread`
- `fn find_dropped`
- `fn picking_back_up`
- `fn still_keep`
- `fn worth_saying_again`
- `struct Closed`
- `struct Carried`
- `struct DailyConfig`
- `struct Rhythm`
- `struct Dropped`
- `enum Arrival`
- `enum Whereabouts`

### `src/dash.rs`

**Wired** — something outside this file calls into it.

The dashboard you arrange yourself.

The hub was twelve pages, each one a function that printed a fixed shape.
Nothing about what you see was yours — not the order, not which parts show,
not how much room each one gets. That is fine for a settings screen and
wrong for the thing you open on purpose when you want to know where you
stand.

## Why the layout is data

The lesson worth stealing from Notion is not the dragging. It is that a
view is not a copy of the data — it is one arrangement of the same
underlying source, and you can keep several. Once the arrangement is data,
rearranging is an edit to a value rather than a change to a template, it
survives a restart, it can be reset, and the same card can be shown
somewhere else without being written twice.

## Why moving is a click, and dragging is the extra

Dragging alone is not enough: WCAG 2.2's dragging-movements criterion asks
for a single-pointer alternative to every drag, and a sortable dashboard is
explicitly not one of the cases where dragging counts as essential. So the
move buttons are the real mechanism — a plain form post that works with no
script, on a phone, on a bad connection — and dragging, when it is added,
posts exactly the same thing. One path, not two that can disagree.

## What it does not do

No free-floating pixel positions. A card sits in an order and takes either
half the width or all of it. A twenty-four-column collage is a lot of
machinery to let someone leave a gap, and gaps are what makes a homemade
dashboard look homemade.

**Public interface:**

- `struct Placed`
- `struct Layout`
- `enum Card`
- `enum Span`
- `enum Move`
- `const FILE`

### `src/decide.rs`

**Wired** — something outside this file calls into it.

Working a decision instead of answering it.

`council` gets you disagreement. `otherside` argues against something you
have already chosen. `certainty` says how sure it is. Each is one move.
Nothing runs the whole thing, so a decision gets whichever move you happen
to ask for.

The order below is not arbitrary and it is the part that does the work.
Framing comes before options, because most bad decisions are right answers
to the wrong question. Options come before evidence, because the moment
anything is recommended you stop comparing the rest. Stakes come last,
because how much thought a decision deserves depends on what it costs to be
wrong, and you cannot know that until you know what you are choosing
between.

**It recommends only when it can show its working.** An unexplained pick is
a coin flip in a confident voice, and that is what the no-recommendation
rule was really guarding against. The fix is not silence, it is a
recommendation that arrives with what it rests on attached, so you can
disagree with the reasoning rather than only with the answer.

Three conditions before it will lean. Every option has to have been costed,
or the cheap-looking one wins by not having been examined. Something has to
have been argued against it, because a lean nobody attacked is a preference
wearing evidence. And where the choice turns on what you want rather than
on what is true, it asks instead of assuming — an assistant guessing your
preference and then reasoning from the guess is confidently wrong in a way
you cannot see.

One exception, and it is the useful one: when a decision is cheap and
reversible, working it is a waste of your evening. It says so and stops.

**Public interface:**

- `fn how_much_it_matters`
- `fn from_draft`
- `fn said`
- `fn wants_working`
- `struct Option_`
- `struct Lean`
- `struct Decision`
- `enum Move`
- `enum Weight`
- `enum CannotLean`
- `const DRAFTER_PROMPT`

### `src/delegate.rs`

**Wired** — something outside this file calls into it.

Working an app on your behalf.

"Finish the conversation with Claude until I'm back."
"Read this email and draft a response."

Atlas reads what is on screen, works out what to say or do, and puts it
there. The loop is: observe → compose → place → observe again.

Limits keep this from running away, because an assistant typing into
your apps unattended is the highest-consequence thing in this whole system:

* a turn budget, so it cannot loop forever
* a stop condition it watches for
* **it is an errand like any other** (`daemon::WorkingForYou`): "stop",
or "stop the Slack one" with several going, pauses it and loses
nothing; pausing Atlas holds it. Asking Atlas for something else does
*not* stop it — that was this module's first rule ("you speaking ends
it"), written before the crew could run work side by side, and it made
every new request quietly end the old one (corrected 25 Sep 2026).
* **it never fights you for the keyboard**: it reads the window in the
background and only types in a gap in your own typing (`lanes`)
* nothing is sent into an app that needs confirmation without one

**Public interface:**

- `fn interpret`
- `fn for_the_window`
- `fn is_screen_noise`
- `fn after_reply`
- `fn something_new`
- `fn type_into_window`
- `struct DelegateConfig`
- `struct Delegation`
- `enum Reach`
- `enum State`
- `enum Step`
- `const SYSTEM`

### `src/delivery.rs`

**Wired** — something outside this file calls into it.

Getting an approved post out of the queue and onto the site.

The publisher decides *whether* something may go. This decides *how*, and
carries it out through the browser. The two are kept apart on purpose: the
approval logic should not know or care what a CSS selector is, and the
browser code should never be able to decide that something is approved.

Approval is re-checked here, immediately before the click. That is
deliberate duplication — the last thing between a draft and the public is
worth checking twice.

**Public interface:**

- `fn profile_for`
- `fn plan`
- `fn send`
- `fn classify`
- `fn spoken`
- `enum Outcome`

### `src/diagnose.rs`

**Wired** — something outside this file calls into it.

Atlas checking on itself.

An assistant that quietly half-works is worse than one that plainly
doesn't — you keep asking, it keeps not quite delivering, and you never
find out why. So Atlas runs its own checks, and reports in three
categories: what it fixed, what it wants your permission to fix, and what
it can't fix at all.

The line it will not cross: **it only repairs things it created.** Scratch
folders, its own caches, its own state files. Anything belonging to you is
a recommendation, never an action.

**Public interface:**

- `fn diagnose`
- `fn self_fixable`
- `fn needs_permission`
- `fn yours`
- `fn report`
- `fn detail`
- `struct Symptom`
- `struct Vitals`
- `enum Impact`
- `enum Remedy`

### `src/diarize.rs`

**Wired** — something outside this file calls into it.

Who said what: a recording turned into lines with a speaker on each.

**Sources:** agglomerative hierarchical clustering with average linkage
over cosine similarity (Ward's family of methods; the recipe used by
`pyannote.audio` (MIT) after its embedding step and by the NIST RT
diarization baselines): every speech segment starts as its own speaker,
the two most similar groups merge, and merging stops when no two groups
are similar enough to be one voice. Segments come from `vad::segments`;
the embeddings from whichever speaker encoder `speaker` runs. Clean-room.
The second looks: ΔBIC between full-covariance Gaussians on pooled MFCCs
(Chen & Gopalakrishnan 1998) for `merge_same_voices` and `to_count`;
leave-one-out Gaussian log-likelihood per frame for `split_strangers`.

**Why Atlas wants it.** `call_notes` has been in `tools.yaml` — scope
"you only" or "everyone" — with nothing behind it that could tell one
voice from another. With the encoder Atlas already uses for voice-lock,
a recording becomes "You: … / Speaker 2: …", and "you only" can mean it.

**Public interface:**

- `fn who_said_what`
- `fn who_said_what_grouped`
- `fn read_wav`
- `fn split_strangers`
- `fn to_count`
- `fn merge_same_voices`
- `struct Line`
- `const STRANGER_MARGIN`
- `const SAME_VOICE_LAMBDA`

### `src/dictate.rs`

**Wired** — something outside this file calls into it.

Typing what you say.

Not a command — the words go into whatever window you're looking at. For
anyone who thinks faster than they type, this is the highest-frequency use
of a voice assistant there is, and every piece of it already existed.

The thing that makes dictation usable rather than infuriating is knowing
when you meant a word and when you meant an instruction. "New paragraph" is
almost never a phrase you wanted typed. "Full stop" usually is a full stop.
But "period drama" is not punctuation, so the rule has to be about position
and isolation, not just the word.

**Public interface:**

- `fn read_ambiguous`
- `fn ask_which`
- `fn parse`
- `fn render`
- `fn may_type_into`
- `fn refusal`
- `struct DictateConfig`
- `struct Dictation`
- `enum Piece`
- `enum Reading`
- `enum State`
- `const AMBIGUOUS`

### `src/diff.rs`

**Wired** — something outside this file calls into it.

What actually changed between two versions: the shortest edit script.

**Source:** Myers (1986), *An O(ND) Difference Algorithm and Its
Variations* — the greedy forward search over diagonals, keeping each
round's furthest-reaching point, then walking the saved rounds back to
recover the script. The same algorithm `git diff` uses by default; the
unified-format output follows GNU diffutils (`@@ -a,b +c,d @@`, three
lines of context). Clean-room.

**Why Atlas wants it.** `selfwork::lines_touched` — the figure that
decides whether Atlas calls its own fix "a big change" before you land it
— counted lines "in one and not the other" as sets. A file full of `}` and
blank lines reads almost unchanged under that count however much moved,
and a line duplicated reads as nothing. The shared-document CRDT (`yata`)
also needs to turn "here is the new text" into the fewest inserts and
deletes, which is this, over characters.

**Public interface:**

- `fn edits`
- `fn lines_changed`
- `fn unified`
- `enum Edit`

### `src/digest.rs`

**Wired** — something outside this file calls into it.

SHA-256, written out in full, and the one date format Atlas prints.

In-house rather than a dependency: the hash is small, fixed by the
standard, and checked here against the published test vectors, including
the awkward ones — a message that lands exactly on the padding boundary, a
leap day, a century that is not a leap year. Everything in Atlas that
fingerprints something (the tamper-evident log, releases, downloaded
pieces, plugins) uses this one copy.

**Public interface:**

- `fn sha256_hex`
- `fn sha256_file_hex`
- `fn iso_utc`

### `src/doctor.rs`

**Wired** — something outside this file calls into it.

`atlas doctor` — find out what is actually on this machine.

This exists because every path in the original spec set was a guess that
nobody could check. Instead of guessing harder, ask the machine: enumerate
the real monitors, hunt for the real executables, probe for the real tools,
then print config you can paste.

**Public interface:**

- `fn run`
- `fn machine_findings`
- `fn monitor_fixture`
- `fn find_exes`
- `fn expand_env`
- `fn lookup_env`
- `struct Finding`

### `src/doorrule.rs`

**Wired** — something outside this file calls into it.

The Windows Firewall rule for the door your own devices use (gap AJ, 8.9).

Friends reach Atlas through Tor, which only ever connects *out*, so no
rule is needed for them. But the ordinary door (`server::SignalListener`)
also listens beyond this machine, for your own phone over Tailscale and a
friend on the same wifi. The first time something connects to it, Windows
stops and asks -- and a "Cancel" there quietly makes a *block* rule that
stays. So the setup adds the rule itself, once, with Windows' own tool
(`netsh advfirewall`), and asks for your permission to do it the ordinary
way (the Windows "allow changes" prompt), because a firewall rule is an
administrator's change.

The rule is as narrow as the door's job:
- **only Atlas** (`program=` this exe) -- not a port anyone could reuse;
- **only incoming TCP**;
- **only on private and work networks** (`profile=private,domain`): on a
café's wifi, which Windows calls public, nothing gets in;
- **only from your own networks**: this subnet, Tailscale's addresses
(100.64.0.0/10 and fd7a:115c:a1e0::/48), which is what the door itself
also checks (`onion::is_local_origin`).

Nothing here runs anywhere but Windows; elsewhere it says there's nothing
to do.

**Public interface:**

- `fn add_args`
- `fn describes_rule_for`
- `fn elevated_command`
- `fn ensure`
- `fn said_no`
- `fn run_program`
- `enum Standing`
- `const RULE_NAME`
- `const OWN_NETWORKS`

### `src/draft.rs`

**Wired** — something outside this file calls into it.

Writing something, then being honest about it.

Phase 3. A model asked to write a post writes one, and it is usually
mediocre in predictable ways: it opens with throat-clearing, it hedges, it
says the same thing twice, and it ends with a question nobody asked.

The fix is not a better prompt, it's a second pass. Atlas writes, then
reads what it wrote against a list of things that are actually wrong with
most first drafts, then rewrites. The critique is the valuable half, and
it's worth showing you even when you don't want the rewrite.

**Public interface:**

- `fn blanks`
- `fn critique`
- `fn revision_brief`
- `fn spoken`
- `fn improved`
- `fn revise`
- `fn for_replies`
- `struct Note`
- `struct DraftConfig`
- `enum Fault`
- `enum Outcome`
- `const REVISE_SYSTEM`

### `src/drain.rs`

**Wired** — something outside this file calls into it.

A day of log lines, read as the handful of things that actually happened.

**Source:** He, Zhu, Zheng & Lyu (2017), *Drain: An Online Log Parsing
Approach with Fixed Depth Tree* (ICWS), as `logpai/Drain3` (MIT)
implements it: lines are grouped first by how many words they have, then
by their first few words, and within that by similarity to each group's
template (similarity threshold 0.4, tree depth 4, numbers masked); a line
that joins a group turns the words that differ into `<*>`. Clean-room.

**Why Atlas wants it.** `atlas.log` rotates at a few megabytes and nobody
reads it, because ten thousand lines of "selected: Send button" and
"checked mail in 812 ms" hide the one line that says something broke. As
templates with counts, the same file is twenty lines, and a warning that
happened four hundred times is one line saying so.

**Public interface:**

- `fn read_log`
- `struct Template`
- `struct Drain`

### `src/earned.rs`

**Wired** — something outside this file calls into it.

Confidence Atlas has earned, per kind of work.

The complaint this answers, in Eric's words: when Atlas isn't confident
enough, the fix must not be "raise the bar until it acts less". That is not
smarter, it is more restrictive, and it ends with him doing everything
himself again.

## The distinction the old system missed

`certainty.rs` asks *how sure am I about this answer* — from grounding,
hedging words, whether a source was read. That is about one answer, and it
is the same question every time regardless of whether Atlas has done this
kind of thing five hundred times correctly or never once.

This asks a different question: **how often has Atlas been right about this
kind of thing before?** A ceiling that is the same for tidying a folder and
for sending a message to a business partner is not calibration, it is a
single global setting wearing calibration's clothes.

## Why per category, and why it can go down

Globally, Atlas's record is dominated by whatever it does most. Being
excellent at reading files would raise its licence to act on money, which
is exactly backwards. So the record is kept per `Kind`, and a `Kind` earns
its own ceiling.

It falls faster than it rises, and one correction on something already
trusted costs more than one on something new. A track record that only ever
improves is not a record, it is a counter.

## What it never does

It does not decide. It answers "may Atlas do this alone", and something
else decides what to do with the answer. Nothing here can grant reach that
the permission settings have not already given — earning trust widens what
Atlas may do *within* what you allowed, never past it.

**Public interface:**

- `fn kind_of`
- `struct Outcome`
- `struct Record`
- `enum Space`
- `enum Kind`
- `enum Rope`
- `const WINDOW`
- `const ENOUGH`
- `const FILE`

### `src/edit.rs`

**Wired** — something outside this file calls into it.

Video editing from a described vision.

"Take this video, here's what I want, edit it." The intelligence is in
*planning* — turning a description into an edit decision list. The
rendering is ffmpeg, which is CPU work with hardware decode, so this runs
fine on an integrated GPU where generation would not.

One rule everywhere: **the source is never touched.** Every render writes a
new file, and a plan whose output collides with an input is rejected before
ffmpeg is ever called.

**Public interface:**

- `fn ffmpeg_args`
- `fn atempo_chain`
- `fn escape_drawtext`
- `fn probe_args`
- `fn duration_from_probe`
- `fn plan_from_model`
- `fn describe`
- `fn render`
- `fn path_and_wish`
- `fn copy_and_result_paths`
- `struct Segment`
- `struct Overlay`
- `struct EditPlan`
- `const PLANNER_PROMPT`
- `const RENDER_TIMEOUT_SECS`

### `src/editcraft.rs`

**Wired** — something outside this file calls into it.

Editing knowledge, taken from people who actually do it.

Transcribed from the videos rather than guessed at, which matters: most
"editing tips" available to a machine are about software, and the ones that
change how a piece feels are about where the viewer's eye is and where the
camera was.

Two ideas here are worth more than everything else combined.

**Public interface:**

- `fn check_cuts_within`
- `fn check_cuts`
- `fn too_many_effects`
- `fn what_they_asked`
- `fn reply_to`
- `fn what_it_is`
- `fn judge_deal`
- `fn ladder`
- `fn next_rung`
- `fn profile_note`
- `fn is_scheduling_rather_than_capturing`
- `fn terms_from`
- `fn rungs_from`
- `struct Cut`
- `struct DealTerms`
- `struct EditCraftConfig`
- `enum Transition`
- `enum BrandAsks`
- `enum WhatItIs`
- `enum Rung`
- `const THE_PRINCIPLE`
- `const RIGHTS_ARE_THE_PRICE`
- `const NOT_THE_SAME`
- `const AFFILIATE_IS_NOT_EXCLUSIVE`

### `src/editors.rs`

**Wired** — something outside this file calls into it.

Using editing software you already own.

Everything Atlas does to video works with ffmpeg alone, and that's
deliberate: your friends won't have Premiere, and a system whose basic
functions need a £50-a-month subscription isn't one you can hand to
anyone.

But if *you* have it, not using it is silly. So professional tools are an
optional better path, never a requirement, and Atlas says which one it
used so you always know whether the result is reproducible on a plain
machine.

## What's actually possible

Honesty matters here, because "can Atlas use Adobe" has a more interesting
answer than yes or no:

* **Premiere** — scriptable through ExtendScript. Atlas can build a whole
sequence: cuts, captions, colour, and hand it to you open and editable.
This is the good case.
* **After Effects** — same, plus `aerender` for headless output. Templates
with editable text are the useful part.
* **Photoshop** — scriptable, and genuinely useful for thumbnails at
volume.
* **DaVinci Resolve** — has a proper Python API, and the free version is
fully capable. For colour work this is the best option for most people,
because it costs nothing.
* **Final Cut** — no useful scripting. Atlas can prepare and hand off, not
drive.

**Public interface:**

- `fn best_for`
- `fn where_to_look`
- `fn used`
- `fn without_anything`
- `struct EditorConfig`
- `enum Editor`
- `enum Job`

### `src/elsewhere.rs`

**Wired** — something outside this file calls into it.

An Atlas somewhere else, that you can ask about.

`kin.rs` is the door another Atlas knocks on, and its first rule is the one
that makes it safe: **a signal can become exactly one thing, a `Nudge`.**
Never a command, never an action. That rule is right and nothing here
weakens it.

But it only runs one way. A server-side Atlas can tell you something is
urgent; you cannot ask it how it is getting on. On a server with nobody
logged in, that is the wrong way round — the machine that most needs
looking in on is the one nobody looks at.

## What this is

Your Atlas asking another Atlas a question you already had the right to
ask, over the hub API that Atlas already serves: `GET /status`,
`GET /outstanding`, `GET /queued`. Read-only, token on every request, and
**you** started it.

## The rule this keeps

A brief is **words you read**. It never becomes an `Intent`, an `Action`,
or an approval, and there is no function in this file that turns one into
any of those. That is the same rule `kin.rs` holds itself to, for the same
reason: another Atlas saying "sell everything" is a sentence, not an
instruction, whichever direction it travelled in.

The difference between this and `kin` is only who started it. An unbidden
message from another machine is a nudge; an answer to a question you asked
is a report. Neither is a command.

## What it deliberately is not

Not a way to run something over there. Handing work to another Atlas is a
different problem with a different answer — it arrives as something a
person approves, and the hub already has `POST /approve` for that. Mixing
"tell me how you are" with "do this" in one channel is how a read-only
door stops being one.

**Public interface:**

- `fn ask`
- `fn spoken`
- `struct Elsewhere`
- `struct ElsewhereConfig`
- `struct Brief`

### `src/endpoint.rs`

**Wired** — something outside this file calls into it.

Knowing when you've stopped talking.

Recording for a fixed eight seconds is the most-copied mistake in voice
software. It cuts you off mid-word when you have more to say, and makes you
wait seven seconds after "yes". On this machine it is also the single
biggest avoidable cost: every turn transcribes eight seconds of audio
whether you spoke for one or for seven.

So Atlas listens for the silence instead. The whole problem is choosing how
long a silence has to be — too short and it cuts you off while you think,
too long and it feels slow. The answer is that it depends on what you were
saying, so that's what this measures.

**Public interface:**

- `fn shape_of`
- `struct EndpointConfig`
- `struct Endpointer`
- `enum Listening`
- `enum Why`
- `enum Shape`

### `src/enrol.rs`

**Wired** — something outside this file calls into it.

Signing you up.

`signin.rs` argued that autofill is ordinary software, and it was right.
This is the other half, and it is not the same argument. Signing in hands
back a credential you already own. Signing up creates a relationship and
agrees to terms, and that is an act performed *as you* rather than *for*
you.

So the shape here is different. `signin` decides whether to fill. This
decides, at every step, whether Atlas is still allowed to be the one
acting — and there are two places where the honest answer is no.

**Anything asking for payment ends the run.** Not a prompt, not an
approval — the run stops and does not resume. A signup that wants a card
is a signup Atlas has no business completing, and making that refusable by
a tired yes at 1am defeats the point of having the rule.

**Anything asking whether you're a robot hands over to you.** Atlas does
not answer it, does not attempt it, and does not look for a way around it.
It stops, says so, and waits. That check exists to find out whether a
person is present; the only honest response is to make a person present.
When you have cleared it, Atlas picks up where it left off.

Everything between those two lines, Atlas does: filling fields, generating
the password, submitting, and the long tail of settings afterwards, which
is where the time actually goes.

**Public interface:**

- `fn never_enrols_on`
- `fn domain_from`
- `fn read`
- `struct PageSignals`
- `struct PasswordPolicy`
- `struct EnrolConfig`
- `struct Enrolment`
- `enum Stopped`
- `enum Verdict`
- `enum PasswordStyle`
- `enum Phase`

### `src/error.rs`

**Wired** — something outside this file calls into it.

What can go wrong, and how it gets said.

One error type for the whole program, with the variants named after what
actually happened rather than after where it happened. `Platform` rather
than `WinApiError`, because the person reading it doesn't care which layer
failed — they care that something outside Atlas said no.

Every variant carries enough to act on. An error that says "failed" and
nothing else forces whoever hits it to reproduce the problem to find out
what it was, which is the whole cost of the error all over again.

**Public interface:**

- `enum AtlasError`

### `src/explain.rs`

**Wired** — something outside this file calls into it.

Explaining code to someone who doesn't code.

The same honest split as `craft`, `taste` and `motion`. The model writes the
explanation — and whether an explanation is *correct* needs actually
understanding the code, which is the model's job and, in the end, a person's
read. What a machine can check offline is whether it *reads* like a
non-coder explanation: that it's words and not leaked code, that it's the
length a plain summary should be, and that it isn't quietly leaning on jargon
a non-coder won't know. This is that check — the reliable half — and it never
claims the explanation is right, only that it's shaped like an explanation
rather than a wall of code or a sentence of unexplained terms.

**Public interface:**

- `fn check`
- `fn check_at`
- `fn blocking`
- `fn spoken`
- `fn explain_loop`
- `fn in_plain_english`
- `struct Finding`
- `enum Severity`
- `enum Depth`
- `enum Outcome`
- `const EXPLAIN_SYSTEM`
- `const EXPLAIN_SIMPLE_SYSTEM`
- `const EXPLAIN_TECHNICAL_SYSTEM`
- `const EXPLAIN_FIX_SYSTEM`

### `src/facts.rs`

**Wired** — something outside this file calls into it.

What kind of thing a remembered fact is, and what it connects to.

`freshness` knows how long a fact stays true. `consolidate` knows when two
facts are the same fact. Neither knows *what kind* of fact it is, so a
standing instruction about how Atlas should behave is stored beside a note
about a project deadline and recalled the same way.

That matters in three places:

- **Decay.** A preference does not go stale on a clock. A project note
does. Typing lets `freshness` pick the right shelf instead of guessing
from wording.
- **Precedence.** When a recalled fact contradicts what Atlas is about to
do, which wins depends on the kind. Something you told Atlas to do beats
something Atlas noticed.
- **Reach.** One fact leads to another. Without links, recall returns a
sentence; with them it returns the sentence and what it depends on.

Links are written by name and may point at a fact that does not exist yet.
A dangling link is not an error, it marks something worth writing down —
and refusing to store one would mean facts could only ever be added in
dependency order, which is not how anything is learned.

**Public interface:**

- `fn slug`
- `fn alias_decl`
- `fn triple`
- `fn into_facts`
- `fn reference_fact`
- `fn answers`
- `struct Fact`
- `struct Book`
- `enum Kind`
- `const MEMORY_BUDGET_BYTES`

### `src/faithful.rs`

**Wired** — something outside this file calls into it.

Saying what happened, not what was meant to happen.

`certainty` asks whether Atlas knows a thing. `why` explains a decision
after the fact. Neither asks the question underneath both: **does this
report match what actually occurred?**

That gap is the one this whole codebase keeps falling into from the other
side. The vault declared itself encrypted while its cipher cancelled
itself. The capability list advertised modules nothing could reach. The
no-list refused a command string nobody types. Every time, the account of
the system was more careful than the system.

A spoken report is the same artefact. "Backed up and cleaned up" is a claim
about the world, and if the backup failed and the cleanup ran anyway, the
sentence is false in the way that costs most: it is fluent, it is
plausible, and it stops you looking.

The rule this implements: a claim that something is done, saved, sent, or
checked must rest on an outcome observed in the same run. Not on the step
having been attempted, and not on it usually working.

Pure text and enums. No I/O, nothing to schedule, nothing to slow down.

**Public interface:**

- `fn check`
- `fn lead_with_the_problem`
- `struct Step`
- `enum Outcome`
- `enum Fault`

### `src/feedback.rs`

**Wired** — something outside this file calls into it.

Feedback: your friends tell you something's wrong with Atlas, when *they*
decide it is, and you tell them what you did about it.

Eric, 26 Sep: "I don't want my friends' Atlas to tell me. I want a way for
my friends to be able to submit feedback to me to tell me that there is a
bug when the friend makes that determination, then I can get a report.
Kind of like a feedback loop."

So nothing here happens by itself.

1. **The friend decides.** `atlas feedback send` asks what's wrong in their
own words.
- If an update failed on their Atlas, it offers to attach what was
written down about it (`update_apply::FailureReport`). The attachment
holds the version, the step it failed at, its error lines and where it
crashed, with their name and home folder already taken out.
- It shows them exactly that, and attaches it only on their yes.
- It sends only on a second yes.
2. **It travels over the pairing** to whoever sends them Atlas updates: the
owner of their release channel. It goes through its own door,
`/feedback`, which is token-checked, size-capped and rate-limited. The
only thing that door does is file it.
3. **You get a report.** Each piece of feedback lands in `atlas feedback`
with its own number, who sent it, from which version and device, their
words, and the attachment. An attached failure also goes into
`atlas update failures`, so the fix brief has it.
4. **You answer, and they hear it.** `atlas feedback reply <n> ...`
marks it seen, being fixed, fixed in a version, or not something you'll
change, with a note if you like. The answer travels back through
`/feedback-reply`.
- Their Atlas tells them in one sentence.
- It takes an answer only from the person the feedback was sent to,
and only about feedback it really sent.

What never happens: a friend's Atlas reporting on them, halting your
releases on a friend's say-so, or sending anything the friend didn't see.

**Public interface:**

- `fn release_sender`
- `fn send_decided`
- `fn spoken_list`
- `fn read_reply`
- `fn compose_feedback`
- `fn feedback_preview`
- `fn queue_feedback`
- `fn feedback_outbox`
- `fn feedback_delivered`
- `fn feedback_sent`
- `fn heard_feedback`
- `fn feedback_inbox`
- `fn answer_feedback`
- `fn answers_out`
- `fn answer_delivered`
- `fn heard_answer`
- `struct Feedback`
- `struct Answer`
- `enum FeedbackStatus`
- `enum Sending`
- `const MAX_FEEDBACK_BYTES`
- `const MAX_WORDS_CHARS`

### `src/feeds.rs`

**Wired** — something outside this file calls into it.

Following sites without visiting them: RSS and Atom feeds, read here, new
items listed, and the ones you want kept to read later.

"Follow theverge.com." "What's new?" "Read 2." "Save 3 for later."

**Sources:** Miniflux (Apache-2.0) read for what a feed reader must get
right -- RSS 2.0, RSS 1.0 (RDF) and Atom all in the wild; ids that are
sometimes missing (fall back to the link); feeds found from a page's
`<link rel="alternate">`; backing off from a feed that keeps failing; and
stripping tracking parameters from links (its rewrite rules, and the
ClearURLs rule list, read for which parameters). The XML reader here is
new and deliberately small.

**Soundproofing.**
- A feed's first read marks everything seen and lists only its newest
three, so following a site never floods you with its archive.
- A feed that fails backs off -- the interval doubles per failure, to a
day -- and says so in "what's new", rather than retrying every tick.
- Bounded: 200 feeds, 500 remembered ids each, 300 unread, 4 MB a feed
(the http reader's own cap), XML depth 64.
- Only http(s) links are kept; `javascript:` and `data:` never are.
- Redirects are followed three hops at most, and never from https down
to http.

**Public interface:**

- `fn text_of`
- `fn parse`
- `fn parse_date`
- `fn clean_link`
- `fn split_feed_url`
- `fn absolute_link`
- `fn discover`
- `fn fetch`
- `struct Item`
- `struct Parsed`
- `struct Feed`
- `struct Unread`
- `struct Feeds`
- `struct FeedsConfig`
- `const MAX_FEEDS`
- `const MAX_SEEN`
- `const MAX_UNREAD`

### `src/files.rs`

**Wired** — something outside this file calls into it.

Reading, converting and joining whatever you point at.

"Atlas needs to handle every file type" is really three jobs: get the text
and structure out of anything, turn one thing into another, and join or
split. All three are ffmpeg, a PDF library, an unzipper and OCR — nothing
exotic, all local, all free.

The one that matters most in practice is the scan: a photo of a page is the
most common way a document arrives now, and it's the format that's least
useful until something reads it.

**Public interface:**

- `fn pdf_is_really_a_scan`
- `fn convert`
- `fn join`
- `fn scan_steps`
- `fn what_was_scanned`
- `fn path_in`
- `fn after_scan`
- `fn safe_to_unpack`
- `struct FilesConfig`
- `enum Sort`
- `enum Convert`
- `enum Join`
- `enum Scanned`

### `src/filing.rs`

**Wired** — something outside this file calls into it.

Deciding where a file should live.

## What this is built on, and what it is not

There is no filing research recorded anywhere in this tree — `docs/` has
nothing on folder structure. So this is built on published method rather
than on earlier work of yours, and that distinction is worth keeping: if
you settled on a scheme in another session, this should be replaced with
it rather than argued with.

The method is **PARA** (Tiago Forte), for one reason: it sorts by *how soon
you need it* rather than by what a thing is. Sorting by type — a Documents
folder, a Spreadsheets folder — puts the invoice you need today in the same
place as the invoice from 2019, which is how filing systems stop being
used.

- **Projects** — has an end. A thing you are finishing.
- **Areas** — ongoing, no end. Health, finances, the house.
- **Resources** — reference. Useful, not yours, no deadline.
- **Archive** — anything from the first three that has gone quiet.

## The rule that overrides all of it

**Never move something you cannot find again.** Every move is reported and
reversible, and anything Atlas is unsure about is left exactly where it is
with a note saying why. A file put somewhere clever that you cannot find is
worse than a messy Downloads folder — the mess is at least where you left
it.

That is also why this makes no attempt at cleverness. It reads the name,
the extension and the dates. It does not read your documents to guess what
they are about.

**Public interface:**

- `fn suggest`
- `fn as_change`
- `fn spoken`
- `enum Bucket`
- `enum Suggestion`
- `const ARCHIVE_AFTER_DAYS`

### `src/filmstrip.rs`

**Wired** — something outside this file calls into it.

Turning an SVG animation into frames, and the frames into a GIF or an MP4.

`motion` draws and checks an SVG animation; a GIF or a video was the
deferred half, because it needs the animation *played* — sampled at each
moment and drawn to pixels. Atlas doesn't carry a renderer, but every
Windows machine carries one: Edge (and usually Chrome). This drives it:

1. For each frame time, a small page holds the SVG with every animation
paused at that moment — SMIL through `SVGSVGElement.pauseAnimations()` /
`setCurrentTime()`, CSS through the Web Animations API
(`document.getAnimations()`, each paused with `currentTime` set). So a
frame is exactly the animation at t, not "whenever the screenshot
happened".
2. The browser runs headless with its own throwaway profile (so it never
touches an open browser window), and Atlas drives it over the DevTools
protocol it already speaks (`cdp`): load the page once, then per frame
seek, wait two animation frames, and `Page.captureScreenshot` exactly
the viewport. (Chrome's one-shot `--screenshot` flag was tried first:
in Chromium 141 it captured before the page was fully drawn — a circle
missing, a square cut to its top 13 rows — so every frame would have
been a guess.)
3. `pngcodec` reads each PNG; `gifenc` writes the GIF. An MP4 needs a video
encoder, which Atlas does not reimplement: ffmpeg does that part, from
the same frames, when it's there.
4. What comes out is checked: `motion::verify_render` on the file, and the
frames themselves — if every frame is the same picture, nothing moved
when it was played, whatever the source claimed.

One browser for the whole strip, closed when it's done.

**Public interface:**

- `fn page_at`
- `fn find_browser`
- `fn play_frames`
- `fn motion_findings`
- `fn film`
- `struct Plan`
- `struct Made`

### `src/finance.rs`

**Wired** — something outside this file calls into it.

Money, read-only — and read-only by construction rather than by promise.

You want Atlas auditing your finances between statements, which means
reaching your accounts. That's reasonable, and it's also the highest-stakes
thing in this system, so the guarantee can't be "Atlas is configured not
to". It has to be that Atlas *cannot*.

So: on any domain marked financial, Atlas may navigate, read, and click
things that only fetch. It may not submit a form, click a button whose
label suggests moving money, or type into a field that isn't a login or a
date filter. That's enforced here, ahead of the browser layer, and it fails
closed — an action it doesn't recognise is refused, not allowed.

Three ways to get the data, cheapest and safest first. See FINANCE.md.

**Public interface:**

- `fn sources`
- `fn is_safe_field`
- `fn moves_money`
- `fn allowed`
- `fn review`
- `fn parse_csv`
- `fn split_row`
- `fn summary`
- `struct FinanceConfig`
- `struct Transaction`
- `enum Source`
- `enum PageAction`
- `enum Verdict`
- `enum Flag`

### `src/findfile.rs`

**Wired** — something outside this file calls into it.

Finding a file by what you remember of it: part of its name, what kind of
thing it is, roughly when you last touched it -- and then opening it.

Built on the index (`index::Index`), which already knows every file under
your roots by name, type and date. What this adds:

- **Filters from the words.** "the pdf from last week", "spreadsheets
from yesterday", "photos this month": a type and a date window are read
out of the question and the rest is the name.
- **Paths, numbered.** "open 2" opens the second one (`Found`), with the
app Windows uses for that type.
- **A near miss is offered, never taken.** The index's search is exact on
purpose ("a wrong file confidently returned is worse than no result");
when it finds nothing, names within a typo are listed as "closest",
so you can say which -- not opened.

**Sources:** Everything (voidtools) and Recoll were read for what makes a
finder feel instant -- a filename index held in memory and filters that
narrow before ranking -- which is the shape `index` already has.

**Public interface:**

- `fn read`
- `fn passes`
- `fn by_filter`
- `fn near`
- `fn short_path`
- `fn numbered`
- `fn which`
- `struct Asked`

### `src/firewall.rs`

**Wired** — something outside this file calls into it.

The line between your own work and a business you share.

## The decision this implements

Eric said twice that this must not be specified without him, and then
specified it: **block, notify, and pause the item** — and the boundary runs
**one way**. Personal never reaches a business space. Business freely
reaches his own Atlas.

One-way because that is where the harm actually is. His files reaching a
customer is the thing that cannot be undone; a business's task data
reaching his own Atlas is not a problem at all. A boundary built to hold in
both directions is twice as much machinery guarding one real risk, and the
half that guards nothing is the half that will break something.

## Block, notify, pause — three things, not three names for one

- **Block.** It does not cross. Not later, not partially, not as a
summary. That is settled before anything else happens.
- **Notify.** He is told, through `notify`, which already knows how to
reach him whether he is at the desk, away, or out. A silently blocking
firewall is indistinguishable from a broken feature: he would never learn
that a business task kept failing because it needed something it could
never have.
- **Pause.** The item is held, not dropped. Blocking without holding means
the work is simply lost and has to be noticed and redone; holding means
he can look at what it was, and release that one item if it was fine.

## What is never written down here

The held list records **what the thing was called and where it was going,
never what was in it**. A firewall that logs the contents of what it
blocked has copied that content across the boundary into its own log, and
the log is the one file nobody thinks of as sensitive.

## Default deny

Anything whose origin is not positively known to be that business's own is
personal. There is no third state and no benefit of the doubt: a boundary
that is unsure and lets things through is not a boundary, and "I couldn't
tell" is exactly the case it exists for.

**Public interface:**

- `fn note`
- `struct Held`
- `struct Firewall`
- `enum Crossing`

### `src/firstlaunch.rs`

**Wired** — something outside this file calls into it.

Double-click and it's set up.

Eric, 23 Sep 2026: *"I have to ensure Atlas is in a very specific folder …
I have to have a file directly next to another, hard to explain to people
who don't have Claude or myself present for set up."* Both were true:
`atlas.exe` refused to start without a `config/` folder beside it, and the
only way to fetch its voice pieces was a batch file in the same folder.

This module removes both:

- **The settings travel inside the program.** Every `config/*.yaml` is built
into `atlas.exe`. A copy that finds no settings beside it writes the
defaults out and carries on — it never overwrites a file that is there.
- **Atlas picks its own home.** Double-clicked from anywhere that isn't
already an install (Downloads, the desktop, a USB stick), it moves itself
to the one standard per-user place — `%LOCALAPPDATA%\Atlas` on Windows —
puts itself in the Start menu and on the desktop, and opens its own
setup window from there. No folder to choose, nothing to keep next to
anything, no administrator rights.
- **An install that already works is left where it is.** A folder that
already holds Atlas's settings (a developer's tree, an unzipped copy, an
`ATLAS_HOME`) is used in place, exactly as before — `roots` decides that,
and nothing here second-guesses it.

**Public interface:**

- `fn write_default_config`
- `fn settings_missing`
- `fn standard_home`
- `fn where_to_live`
- `fn move_in_over`
- `fn tidy_set_aside`
- `fn replacing`
- `fn replacing_in`
- `fn show_problem`
- `fn ask_yes_no`
- `fn download_mark_of`
- `fn forget_download_mark`
- `fn app_control_from`
- `fn app_control`
- `fn app_control_words`
- `fn is_set_up`
- `fn mark_set_up`
- `fn what_opening_does`
- `fn atlas_running`
- `fn hub_port_at`
- `fn ask_atlas_to_stop`
- `fn open_atlas_window`
- `fn spawn_quietly`
- `fn run_quietly`
- `fn started_without_a_terminal`
- `fn let_go_of_the_console`
- `fn make_shortcuts`
- `fn downloads_and_desktop`
- `fn desktop_entry`
- `struct Opening`
- `enum Where`
- `enum Replacing`
- `enum AppControl`
- `enum First`
- `const DEFAULT_CONFIG`
- `const INSTALLED_NAME`

### `src/firstrun.rs`

**Wired** — something outside this file calls into it.

The first time you run it.

The alternative to this is a list of `FAIL` lines, which is what setup has
been so far. That's fine for someone who reads build output and useless for
anyone else — including you, on a day when you don't feel like it.

So Atlas walks through it out loud instead. It finds what it can by itself,
asks only about what it genuinely can't work out, and lets you skip
anything and come back. Nothing here blocks: if you say "not now" to every
question it still ends up working, just with less.

**Public interface:**

- `fn is_skip`
- `fn which_monitor`
- `struct Answer`
- `struct FirstRun`
- `struct Found`
- `enum Step`
- `enum Move`

### `src/fit.rs`

**Wired** — something outside this file calls into it.

Fitting Atlas to whatever machine it lands on.

Your laptop has 15.7GB shared with the graphics and roughly 3.5GB genuinely
spare. A friend's desktop might have 64GB and a real GPU; another's might
have 8GB and nothing. Shipping one configuration means it's wrong on two of
those three.

So nothing is hard-coded to a machine. Atlas measures what it has and picks
a plan, and every choice degrades rather than fails: too little memory for
a language model means the rule-based paths do the work, not that Atlas
stops.

The measurements are deliberately crude. Precise capability detection is a
research problem; "how much memory is actually free" gets you 90% of the
way and never lies.

**Public interface:**

- `fn plan_for`
- `fn limits`
- `fn what_to_drop`
- `fn plan_as_set`
- `fn worth_replanning`
- `fn describe`
- `fn measure`
- `fn spare_weight`
- `struct Machine`
- `struct Plan`
- `struct Trim`
- `struct FitConfig`
- `enum Tier`

### `src/fixloop.rs`

**Wired** — something outside this file calls into it.

The hand-off loop: working a failing test with a counsel until it passes.

`strategy` knows twelve genuinely different angles on a stuck problem,
`handoff` writes the brief a reader needs and pulls code back out of an
answer, and `consult` keeps a conversation honest — only a testable answer
costs an attempt, a question gets answered, a lecture gets asked for the
change. All three were built, tested, and driven by nothing. This drives
them, in the shape the SWE-agent work (MIT) found makes a model useful on
code: small bounded steps, the real test output fed back every time, a
fixed budget, and a person's yes before anything leaves the sandbox.

The counsel is whatever answers: the local model (offline, the default),
the stronger online one if you've set it, or a script in a test. The loop
works in a copy of the folder (`sandbox`); your files are never touched.
What comes out is either a tested change with its diff, or — when every
angle is spent — the brief, written up for you or for someone else.

**Public interface:**

- `fn run`
- `fn land`
- `struct ModelCounsel`
- `struct Job`
- `struct Outcome`

### `src/flow.rs`

**Wired** — something outside this file calls into it.

The workflow engine: multi-step work Atlas carries out on its own.

A single command is one action. Real work is a chain — research a topic,
write it up, save it, open it. This runs those chains, passes each step's
output to the next, survives a failure partway through, and can pause for
approval in the middle without losing its place.

Chains are recorded from what you actually did, so a sequence you repeat
becomes something you can name.

**Public interface:**

- `fn expand`
- `struct Step`
- `struct Workflow`
- `struct Run`
- `struct Library`
- `enum OnFail`
- `enum RunState`
- `enum Next`

### `src/frames.rs`

**Wired** — something outside this file calls into it.

Pictures out of the camera, continuously.

## Why not just take a photo each time

Atlas already has `capture_webcam`: run ffmpeg, get one PNG. That is right
for "look at the room once every twenty seconds" and wrong for tracking a
hand, because it pays the whole cost of opening the camera, negotiating a
format and starting a process — a few hundred milliseconds — for every
single frame. Fifteen times a second, that is not a slow tracker, it is a
machine doing nothing but starting and stopping ffmpeg.

So one process is started and left running, and frames are read off its
output as they arrive. The camera opens once.

## Raw, not encoded

The pipe carries uncompressed pixels. Encoding to PNG and decoding again
is work done twice for no gain — the model wants numbers, and a PNG is
numbers that have been squeezed and unsqueezed on the way. Raw at a small
capture size is both faster and simpler than compressed at a large one.

## Closing it properly

A camera left open is a light left on. `Drop` kills the process and waits
for it, because a webcam that stays lit after Eric said stop is the most
visible way this could misbehave.

**Public interface:**

- `fn from_capture_args`
- `struct Feed`
- `struct Rolling`

### `src/freshness.rs`

**Wired** — something outside this file calls into it.

How long a thing stays true.

`certainty` asks whether an answer looks invented. This asks a different
question that nothing was asking: *is what we stored still the case?*

A note saying "TCP retransmits on timeout" and a note saying "the latest
release is 1.4.2" were stored identically — same struct, same timestamp,
same confidence when read back. One is true forever and one was true for a
fortnight. Recall ranked them the same and Atlas said them in the same
tone, which is the failure that makes a knowledge store worse than
nothing: a stale fact delivered confidently costs more than an absent one,
because you act on it.

The whole thing is arithmetic on a timestamp. No model, no index, no
background pass — it costs nothing to run, which matters on a machine that
is also doing your actual work.

What it does **not** do is decide truth. It decides how loudly to say
something, and when to offer to go and look again.

**Public interface:**

- `fn ago`
- `fn shelf_for`
- `fn ranking_multiplier`
- `struct Known`
- `enum Shelf`
- `enum Checkable`
- `enum State`

### `src/friends.rs`

**Wired** — something outside this file calls into it.

Adding a friend in one step each: you send a link, they tap it, done.

Pairing used to be three hand-offs: type their name, send them a code,
they paste it and send a *second* code back, you paste that. Every app
people actually use is one step each side, and this is that:

1. **You** say "add a friend" (or press it on the hub). Atlas makes a
one-time **friend link** -- your name, how to reach your Atlas, your
Atlas's public key, and a one-time secret -- and you send it any way you
like: a text, a QR code, in person.
2. **They** open it in their Atlas and press **Add**. Their Atlas pairs
with yours and tells yours in the same moment, carrying the one-time
secret. Your Atlas recognises its own invitation and completes the pair.
You chose them when you sent the link; nobody is asked twice, and
nobody waits for a confirmation.

And from inside a group: tap someone you're in a group with and **send a
friend request**. It travels through the group's owner to them alone, and
they accept with one press -- underneath, it is the same friend link.

**Why this is safe with nothing in the middle.** The secret is 128 random
bits, good for one use and seven days. Only whoever holds the link can use
it, so sending it is the decision. Both Atlases pin each other's public key
from the exchange, so later messages are checked against the key that was
in the link you sent -- a link copied by someone else is used up the moment
your friend uses it, and a used one is refused.

**How their Atlas reaches yours -- with nothing in the middle.** Tailscale
may join *your own* devices together; it never joins yours to a friend's,
and there's no server and no friend holding anyone's messages (Eric,
25 Sep). The link carries your Atlas's onion address, reachable through Tor
from anywhere it's online (`onion`), and its home address for a friend on
the same wifi. Everything between you is sealed by Atlas itself (`wire`).
When your friend's Atlas is off, yours keeps what it was sending and sends
it when theirs is back.

**Public interface:**

- `fn make_link`
- `fn redeem`
- `fn record`
- `fn my_name`
- `fn knock`
- `fn read_spoken`
- `fn request_body`
- `fn read_request`
- `struct Link`
- `struct Invite`
- `struct Invites`
- `struct Me`
- `struct Hello`
- `struct Pending`
- `struct SentRequest`
- `struct Outbox`
- `struct Request`
- `struct Requests`
- `enum Knock`
- `enum Spoken`
- `const PREFIX`
- `const REQUEST_PREFIX`
- `const LINK_DAYS`

### `src/fxday.rs`

**Wired** — something outside this file calls into it.

Where the trading day ends: **17:00 New York.**

Eric's ruling, and the convention every retail FX broker runs on. The
second development chat named this as a decision rather than guessing at it,
which was right — picking the other answer disagrees with every broker chart
while looking perfectly reasonable.

## It is a local time, not a UTC hour

```text
winter (EST, UTC-5)   17:00 New York = 22:00 UTC
summer (EDT, UTC-4)   17:00 New York = 21:00 UTC
```

Hard-coding either one puts every prior-day high and low an hour out for
roughly half the year — and does it silently. The levels are still levels,
still plausible, still near price. Nothing looks broken; the numbers are
just about a different day.

## Why this and not midnight UTC

Because it gives **five twenty-four-hour days a week instead of six.** The
week opens 17:00 Sunday New York and closes 17:00 Friday, so the Sunday
evening session belongs to Monday rather than forming a stub of its own. A
midnight-UTC reader splits that opening session into a three-hour Sunday bar
and a short Monday, and then disagrees with the chart Eric is looking at on
every single day.

It is also why brokers run their servers on GMT+2 in winter and GMT+3 in
summer: both put 17:00 New York at midnight server time, so the daily candle
closes on their own midnight all year.

## The thing I got wrong, and the measurement that said so

I assumed a trading day would be twenty-three hours once a year, when the
clocks go forward. **It never is.** The change happens at 02:00 on a Sunday,
and the market is shut from 17:00 Friday to 17:00 Sunday — so it always
lands inside the weekend. Swept across 2024–2027 in
`docs/fxday_reference.py`: every transition falls in the Saturday-to-Sunday
span, and every trading day is exactly twenty-four hours.

What *does* move is the weekend: **47 hours in March and 49 in November**,
against 48 the rest of the year. That matters for anything deciding whether
a gap in the bars is a weekend or a hole.

## Which days exist is decided by the bars, not the calendar

The calendar says where the boundaries are. It does not know about
Christmas, or a broker's maintenance window, or a pair that simply did not
print. So "the prior day" here means the previous span that **has bars in
it**, which skips weekends and holidays without needing to know what either
one is.

**Public interface:**

- `fn close_on`
- `fn day_start`
- `fn day_of`
- `fn week_start`
- `fn week_of`
- `fn days`
- `fn prior_day`
- `fn prior_week`
- `fn levels`
- `fn spoken`
- `struct DayShape`
- `const CLOSE_HOUR`

### `src/gaze.rs`

**Wired** — something outside this file calls into it.

Reading a face and a pair of hands off the camera.

The loop this closes: `presence::Look`, `presence::Gesture`,
`presence::Sensor::observe` and `answering::saw` were all written, all
tested, and **nothing anywhere ever produced one**. Atlas could decide what
a thumbs-up means and had no way to see a thumb. `presence` has sat in the
unwired list since it was written for exactly that reason.

## What this is and is not

It is not a vision model. It is the honest join between one — whatever you
install — and the decisions Atlas already knows how to make. The detector is
an external tool like `ocr` and `stt`: Atlas runs it on a frame and reads
lines back. That keeps the model a thing you choose and can replace, and
keeps Atlas's side testable without a camera.

## The three rules that matter more than the detection

**An unread camera is not an empty room.** The failure this codebase keeps
producing. If the tool is missing, fails, or is switched off, that is
`blind()` — not "nobody there". One of those means "speak freely, he's
gone"; the other means "you have no idea". `presence::Presence::Unknown`
already treats worth_speaking as true, which is the right call for a
machine with no camera.

**Unsure is absent, not false.** A face seen at forty percent confidence is
not a face and it is not the absence of one. Below the floor the field is
left out, so nothing downstream can read a shrug as a finding.

**A face is not a password.** Recognising you well enough to say "he's back
at the desk" is a much lower bar than "unlock the vault", and the same
number should not serve both. Identity from a camera never satisfies
anything the confidence rules call Sensitive — that is what
`identity.rs` and a typed passphrase are for. A photograph held up to a
webcam is a real attack and there is no honest way to rule it out here.

**Public interface:**

- `fn read`
- `fn gesture_named`
- `fn spoken`
- `fn why_look`
- `fn how_often`
- `fn to_screen`
- `fn what_happened`
- `fn read_hand`
- `fn snags`
- `fn verdict`
- `fn in_use`
- `struct GazeConfig`
- `struct Sighting`
- `struct Situation`
- `struct Hand`
- `struct Steering`
- `struct Sign`
- `enum Reason`
- `enum Move`
- `enum Shape`
- `enum Snag`
- `const GONE_AFTER`
- `const OPEN_FRAMES`
- `const TAP_SLOP`
- `const STEERING_STOPS_AFTER`
- `const DELIBERATE_MS`

### `src/getpieces.rs`

**Wired** — something outside this file calls into it.

Fetching Atlas's voice pieces — from inside Atlas, not a batch file.

This used to be `ATLAS.bat`'s `:setup`, and on 23 Sep 2026 checking it
against the real internet found it could not have worked:

- **The listening engine's address was dead.** It asked for "the latest
whisper.cpp release", and the latest release (v1.9.4) ships no Windows
program at all — that address answered 404. The version is now pinned
(v1.9.2, the newest that still ships `whisper-bin-x64.zip`).
- **The zip downloads were quoted wrong.** `'$env:TEMP\…'` in single quotes
is never expanded by PowerShell, so whisper and piper would have been
written to a folder literally named `$env:TEMP`, and failed.
- **Nothing was checked.** A half-finished or substituted download was
accepted as long as a file existed.

Every piece here is pinned to an exact file and its SHA-256, measured by
downloading it (23 Sep 2026). A download that doesn't match is thrown away
and said, never used. A piece already present is left alone, so running it
again picks up where it stopped.

The fetching itself is Windows' own `curl.exe` and the unpacking Windows'
own `tar.exe` — both ship with Windows 10 and 11 — so Atlas carries no
HTTP or zip code of its own for this.

**Public interface:**

- `fn catalogue`
- `fn seeing`
- `fn pictures`
- `fn draft_model`
- `fn tor`
- `fn set`
- `fn have`
- `fn marker_path`
- `fn space_needed`
- `fn room_for`
- `fn free_bytes`
- `fn clear_unfinished`
- `fn fetch`
- `fn curl_args`
- `fn deadline_for`
- `fn plain_download_error`
- `fn use_own_tools`
- `fn setup_pieces`
- `struct Piece`
- `struct Tools`
- `enum Lands`
- `enum Unzip`
- `const MARKER`
- `const SPARE_BYTES`

### `src/gguf.rs`

**Wired** — something outside this file calls into it.

GGUF reader — the model file format, parsed in pure Rust.

This is the first piece of replacing Ollama. GGUF holds the weights plus a
metadata block describing the architecture, context length, tokenizer and
quantization. Ollama reads that block to decide how to load a model and how
many layers fit on the GPU. So can we.

Format (little-endian throughout):
magic "GGUF" | version u32 | tensor_count u64 | kv_count u64
kv pairs: key string, type u32, value
tensor info: name, n_dims u32, dims[u64], ggml_type u32, offset u64
padding to `general.alignment`, then the tensor data

Only the header is read. The weights are never loaded here — inspecting a
40GB model must cost a few kilobytes of I/O, not 40GB of RAM.

**Public interface:**

- `struct QuantType`
- `struct TensorInfo`
- `struct Gguf`
- `enum Value`
- `const MAGIC`

### `src/gifenc.rs`

**Wired** — something outside this file calls into it.

Writing animated GIFs, in house.

**Sources:** CompuServe's GIF89a specification (W3C copy): the header, the
logical screen descriptor, the global colour table, the Graphic Control
Extension (frame delay in hundredths of a second), the image descriptor,
and the variable-width LZW with its clear and end codes packed
least-significant bit first into 255-byte sub-blocks (Appendix F). The code
width grows the moment the next code would not fit, as giflib's
`EGifCompressOutput` does. Looping is Netscape's application extension
("NETSCAPE2.0", loop count 0 = forever). The palette is Heckbert's median
cut (1982): split the colour box with the most pixels × widest range at its
weighted median until there are 256. Clean-room; no gif crate.

**Why Atlas wants it.** An animation was an SVG and, at best, one PNG still
(`motion::render`). A GIF is the thing you can drop into a message or a
document and have it move everywhere. Each frame after the first stores
only the rectangle that changed, so a ball crossing a still background costs
the ball, not the background.

**Public interface:**

- `fn palette_for`
- `fn encode_gif`
- `fn encode_gif_dithered`
- `struct Frame`

### `src/glance.rs`

**Wired** — something outside this file calls into it.

What a phone widget shows: a glance, never the workspace.

The design (UPDATE_COURIER_SPEC §10): read-only glances, no secrets.
- **Now / next:** what Atlas is doing, or the next thing today.
- **Waiting:** how many things wait on you, the same count as the hub's
Outstanding badge.
- **Status:** on, paused or not running, and "as of" when this was
written, so a widget the phone hasn't refreshed says how old it is
instead of passing old news off as current.
- **Capture:** a tap into Give (`atlas://hub/give`); a widget can't run
Atlas, so it opens the app rather than taking text itself.

Widgets can't reach Atlas's data. The iPhone widget runs in its own
process with its own sandbox, and Android's is drawn by the launcher. So
Atlas hands over this small projection instead. The phone app fetches it
from `/hub/glance.json` and hands it to the widget (iOS through the app
group, Android through `AppWidgetManager`). The widget never holds a
token, the vault, or anything this file leaves out.

**Two views, because of where they're seen.** A home-screen widget is only
seen on an unlocked phone. A lock-screen one is seen by anyone who picks
the phone up. So `home` carries titles (scrubbed the way anything leaving
the laptop is, and cut short) while `lock` carries only times and counts,
unless you turn on `phone.widget_titles_on_lock_screen`. That is the same
rule as the phone's notifications (`phone.include_detail`): off by default,
because Atlas can't know who's looking.

**Public interface:**

- `fn for_widgets`
- `struct Next`
- `struct View`
- `struct Glance`
- `struct Facts`
- `const TITLE_MAX`
- `const CAPTURE_LINK`

### `src/gmm.rs`

**Wired** — something outside this file calls into it.

A diagonal Gaussian mixture over cepstral frames, and the "supervector"
of how one clip moves it (Reynolds, Quatieri & Dunn 2000; Campbell et al.
2006 for the supervector).

Why this and not just averaging a clip's cepstra (tried first, round 5):
a two-second clip's average depends as much on *what* was said as on
*who* said it. A mixture fitted to all the speech at hand gives each
component a rough class of sound; adapting it to one clip moves each
component only as far as that clip has evidence for, so two clips are
compared sound-class by sound-class — like with like.

**Public interface:**

- `struct Gmm`

### `src/goal.rs`

**Wired** — something outside this file calls into it.

Knowing when a long job is finished.

`overnight` decides what is *safe* to run unattended. Nothing decides what
*finished* means, so a job that runs for six hours has no way to tell
whether it got there — and neither does the report in the morning.

A long-horizon task is three things: a trigger, the work, and a way to
check. The third is the one that gets skipped, and skipping it is what
turns an overnight run into a pile of activity you have to read.

The rule that makes it work: **the check has to be something a machine can
run.** "Make the report better" cannot be evaluated at 3am by the thing
that wrote it. "Every section has at least three sources and the build
passes" can. The more of the criterion that rests on judgement, the closer
the loop gets to a thing that runs forever and grades its own homework.

So this refuses subjective criteria rather than accepting them and hoping.
A job that cannot say what done looks like is a job that should wait for
you, and saying so at the start costs a question. Saying nothing costs a
night.

**Public interface:**

- `fn keep_at_it`
- `fn ended_spoken`
- `struct Goal`
- `struct Attempt`
- `struct LongJobConfig`
- `enum Check`
- `enum NotRunnable`
- `enum Standing`
- `enum Ended`

### `src/goingaway.rs`

**Wired** — something outside this file calls into it.

Getting your accounts ready before you can't reach them.

## The problem, stated properly

You go away. You can't receive a text. Two-factor locks you out of your own
accounts — so turning it off looks like the fix.

It isn't, and the reason matters: **the thing that breaks when you travel
is SMS, not two-factor.** A text needs your number, your carrier and a
signal. An authenticator code needs none of those — it's a clock and a
secret, and it works on a plane, in a facility with no signal, on a device
that has never been online. A printed recovery code works with no phone at
all. A hardware key works with no phone, no signal and no battery.

So the answer is not less security. It's **the same security on something
that survives where you're going** — which happens to be both more
available and harder to steal than what you have now.

Turning it off is also the worst possible option for someone who is away:
the account is least protected exactly when you are least able to notice
something wrong with it and least able to fix it.

**Public interface:**

- `fn plan`
- `fn would_lock_you_out`
- `fn spoken`
- `fn if_you_do_one_thing`
- `fn periodic_nudge`
- `fn the_trip_is_close`
- `fn leaving_on`
- `struct Prepare`
- `struct AwayConfig`
- `struct Away`
- `enum Survives`
- `const WHY_NOT_OFF`
- `const WHY_NOT_ATLAS_HOLDS_IT`
- `const AWAY_RECORD`

### `src/goodbye.rs`

**Wired** — something outside this file calls into it.

Atlas being asked to stop, and stopping properly.

## What there was before this

Nothing. `Daemon::run` was `loop { ... }` with no `break` and no signal
handler, so the only way Atlas ever ended was being killed — Ctrl-C, the
console window closed, a logoff, Task Manager. That meant three things
that exist for the way out never ran:

* **`OnlyOne::release()`** — the instance lock. It has a caller in the
tests and none in `src`, because there was no way out to call it from.
So the lock always outlived the process, and restarting inside
`GONE_AFTER_SECS` (150s) was refused with *"Atlas is already running…
Close the other one first"* when there was nothing to close. Change a
config line and restart, and you waited two and a half minutes.
* **`Helpers::stop_all()`** — documented *"Everything down — on suspend,
on battery, on the way out."* Also no caller. So whisper, piper and the
model server **survived Atlas**, holding their memory, and the next
start spawned more.
* **A final `persist()`** — anything changed since the last one was lost.

## Signal safety

A signal handler may do almost nothing: it interrupts the process at an
arbitrary instruction, so allocating, locking, or writing a file from
inside one is how a shutdown handler becomes the crash it was meant to
prevent. This one stores `true` in an `AtomicBool` and returns. **All** the
real work happens on the main thread, on the next pass of the run loop,
where it is ordinary code.

## No new dependency

`platform/win.rs` already declares the Win32 functions it needs with
`unsafe extern "system"`, so the console handler is declared the same way
rather than adding a crate for one function. On unix, `signal` comes from
the libc that `std` already links.

The `ctrlc` crate would do this in three lines. It is not here because
this Cargo.toml is curated — every dependency in it has a comment
explaining why it earns its place — and one atomic flag plus two
declarations is less to own than a dependency and its tree.

**Public interface:**

- `fn asked_to_stop`
- `fn please_stop`
- `fn stop_file`
- `fn asked_by_file`
- `fn asked_twice`
- `fn listen`
- `fn nap`
- `fn stop_and_wait`
- `fn reset_for_test`

### `src/grade.rs`

**Wired** — something outside this file calls into it.

Making it look and sound like someone made it on purpose.

The gap between amateur and professional in short-form is almost never the
camera. It's four things, all fixable after the fact and all measurable:
loudness, dialogue clarity, colour that doesn't clip, and text that isn't
under the interface.

Everything here is a number, not a taste. "Make it look better" is not
actionable; "your dialogue is at -22 LUFS and the platform will normalise
everything else up to -14, so you'll be the quiet one in the feed" is.

**Public interface:**

- `fn preset_named`
- `fn check_audio`
- `fn check_picture`
- `fn presets`
- `fn preset_filter`
- `fn audio_chain`
- `fn spoken`
- `fn recording_advice`
- `struct GradeConfig`
- `struct Audio`
- `struct Picture`
- `struct Note`
- `struct SafeArea`
- `struct Preset`
- `const TARGET_LUFS`
- `const MAX_TRUE_PEAK_DB`

### `src/grading.rs`

**Wired** — something outside this file calls into it.

Colour, done in a fixed order.

Transcribed from someone who grades for a living, and the useful part isn't
any single adjustment — it's that **the order is fixed**. The reason people
flounder learning colour is that every tool is available at once and
nothing tells you which to reach for. A node tree you always build the same
way removes that entirely.

Five nodes, always the same five, always in this order. Anything you can't
fix inside them wasn't a colour problem.

**Public interface:**

- `fn tree`
- `fn recommended_setup`
- `fn check`
- `fn spoken`
- `struct ProjectSetup`
- `struct Measured`
- `struct GradingConfig`
- `enum Node`
- `const WHY_A_FIXED_TREE`

### `src/grants.rs`

**Wired** — something outside this file calls into it.

Permission to touch an app.

Three rules, from how you actually described it:

1. If Atlas doesn't know an app, it asks before using it.
2. If you *tell* it to use something — "use Excel to build that sheet" —
naming the app is the permission. It doesn't ask again for that.
3. Some apps are confirm-every-time regardless. Discord can be interacted
with, but each send is confirmed, because a wrong message there is
public and permanent.

**Public interface:**

- `fn grant_in_instruction`
- `fn span_from_answer`
- `struct Granted`
- `struct AppFacts`
- `struct Permissions`
- `enum Span`
- `enum Verdict`

### `src/groups.rs`

**Wired** — something outside this file calls into it.

Groups with an owner: who is in a group chat, and who may say what in it.

The first group chats had no owner. Every member's Atlas kept its own list
and learned members from the messages it heard, so any member could bring
anyone in just by naming them on a message -- and nobody could take
anybody out. That is fine for three friends and wrong for everything else,
including the one group this project needs most: the release channel,
where Eric posts updates and nobody else can.

So a group can now have an **owner** -- whoever made it -- and the owner's
Atlas alone decides:

- who is in it (add, remove),
- what each person may do (the `Role`: owner, member, or reader -- a
reader reads and cannot post),
- what it is called, and whether it is a release channel.

**How every member can trust that.** The owner's Atlas writes the decision
down as a `GroupState` -- the whole membership, numbered -- and signs it
with its own key (`peerkey`). Every member's Atlas checks the signature and
takes only a newer number, so a stale list can't be replayed and no member
can forge one. The group's id is made from the owner's key
(`og-<fingerprint>-<random>`), so nobody else can publish a list for it: a
list signed by any other key for that id doesn't match the id and is
refused. People are named by their keys, which every Atlas learns from its
paired devices over the pairing channel (`/hello`), so "Sam" means the same
person on every member's Atlas even though each of them may call Sam
something different.

What a member's Atlas enforces with that list: a message from someone not
on it, or on it as a reader, is not filed; you can't post where you are a
reader; and being taken off the list closes the group on your end.

**Public interface:**

- `fn is_owned_id`
- `fn open`
- `fn views`
- `fn read_spoken`
- `fn changes_to_carry`
- `fn take_synced`
- `fn act`
- `struct Seat`
- `struct GroupState`
- `struct Signed`
- `struct Held`
- `struct Waiting`
- `struct Groups`
- `struct Relay`
- `struct View`
- `enum Role`
- `enum Taken`
- `const FORMAT`
- `const DOMAIN`
- `const MAX_STATE_BYTES`
- `const MAX_SEATS`
- `const SYNC_GROUP`
- `const SYNC_DEVICE`

### `src/guessable.rs`

**Wired** — something outside this file calls into it.

How many guesses would it take? — a passphrase estimate that counts
patterns, not character classes.

**Source:** Wheeler (2016), *zxcvbn: Low-Budget Password Strength
Estimation* (USENIX Security), and `dropbox/zxcvbn` / the Rust port
`shssoichiro/zxcvbn` (both MIT). The frequency lists in
`config/guessable/` are the top of theirs (MIT, see LICENSE-zxcvbn.txt).
The matchers, guess counts and the minimum-guess search are re-written
here, smaller: dictionary (with reversal, capitals and l33t), sequences,
repeats, keyboard walks, dates and years, and brute force for the rest;
then the cheapest way to cover the whole string, `k! · Π guesses` for k
pieces, as the paper does. Score 0–4 at 10³ / 10⁶ / 10⁸ / 10¹⁰ guesses.

**Why Atlas wants it.** The vault refused anything under twelve
characters and accepted anything over. "password1234" and "aaaaaaaaaaaa"
passed; "correct horse battery" is fine and should stay fine. Length was
standing in for what actually matters: how early in a guessing list the
phrase comes.

**Public interface:**

- `fn estimate`
- `fn fit_for_the_vault`
- `struct Piece`
- `struct Estimate`
- `enum Kind`

### `src/habits.rs`

**Wired** — something outside this file calls into it.

Habits, counted kindly: "did my reading", "how are my habits?".

**Sources:** Loop Habit Tracker (GPL-3.0; read for its ideas only). The
thing it gets right, and streak apps get wrong, is the **strength**
score: an exponential average that one missed day dents rather than
zeroes, so a habit that's mostly kept reads as mostly kept. The update is
Loop's -- `score = score·m + checked·(1−m)`, `m = 0.5^(√f / 13)` for a
habit of frequency `f` a day -- and for "3 times a week" a day counts as
checked by how much of the week's target the last 7 days met. The code is
new.

**Soundproofing.**
- **Pauses** -- ill, travelling, a holiday -- leave the score where it
was instead of counting as misses.
- **Never about the body's numbers.** A habit whose name touches
`nudge::NEVER_NUDGES_ABOUT` (weight, calories, a dose…) can be tracked
if you ask, but is *never* reminded about and never appears in the
brief -- the same line every nudge holds.
- Reminders come at most once a day per habit, only when it's due and
not yet done, and only from the brief -- never a pop-up.
- Bounded: 50 habits, two years of days each.

**Public interface:**

- `fn read`
- `struct Habit`
- `struct Habits`
- `enum Refused`
- `enum Asked`
- `const MAX_HABITS`
- `const KEEP_DAYS`

### `src/handloop.rs`

**Wired** — something outside this file calls into it.

Hand tracking on its own thread.

## Why this has to exist

Every other capability in Atlas is checked once per daemon tick, and that
tick sleeps for up to two seconds. That discipline is right for almost
everything: a kin signal, a page request, a link handed over from a phone
can all wait two seconds and nobody notices.

A pointer cannot. Two seconds of latency is not a slow pointer, it is a
broken one, and no amount of smoothing or prediction rescues it —
prediction covers a hundred milliseconds, not two thousand. Everything
built in `handtrack` and `handshape` is correct and would still have felt
unusable, because it was queued behind the same loop as everything else.

So this is the first thing in Atlas with its own thread, and the shape of
it is deliberately the shape the worker model will need later: a loop that
owns its work, paces itself, and reports back over a channel rather than
sharing state.

## Nothing is shared

The thread does not borrow the daemon's platform. It builds its own — the
real platforms hold no state for pointer work, so there is nothing to
share and therefore nothing to lock. A mutex on the pointer path would put
a contended lock in the hottest loop in the program.

What comes back is only what the daemon genuinely needs to know: a gesture
that fired, or a complaint that the machine cannot keep up. The pointer
itself never crosses the channel, because it is already where it needs to
be by the time the daemon would have read it.

**Public interface:**

- `fn where_the_hand_is`
- `fn start`
- `fn learn_a_shape`
- `fn shape_shown`
- `struct Tracking`
- `struct Seeing`
- `struct Setup`
- `enum Said`

### `src/handoff.rs`

**Wired** — something outside this file calls into it.

Asking for help properly.

Atlas tries. It reads its own error, forms a theory, writes a change into
the sandbox, runs the tests, and looks at what happened. Sometimes that
works. When it doesn't — after a few honest attempts — the useful thing is
not to keep guessing, it's to hand the problem to someone who can solve it.

The difference between a good handoff and a bad one is enormous. "It
doesn't work" gets a question back. A brief with the symptom, the exact
error, what was already tried and why each attempt failed, and the twenty
lines that matter, usually gets an answer first time.

So this assembles that brief, sized to be read rather than skimmed. You
paste it into a chat, paste the answer back, and Atlas applies it to the
sandbox and runs its own tests before anything touches your machine.

**Public interface:**

- `fn should_ask`
- `fn write_brief`
- `fn spoken`
- `fn extract_blocks`
- `fn read_answer`
- `fn trim_middle`
- `struct Try`
- `struct Snippet`
- `struct Problem`
- `struct HandoffConfig`
- `struct Block`
- `enum Usable`

### `src/handover.rs`

**Wired** — something outside this file calls into it.

Handing your laptop to someone else.

The question this answers is not "who is at the machine" — Atlas cannot
know that, and the honest reasons are written down elsewhere in this tree:
a camera reading never stands in for you saying who you are, because a
photograph held to a webcam defeats face recognition, and `voiceid`'s own
doc says a recording of you sounds like you.

So this answers a different question: **have you said you are handing it
over?** That is knowable, because you said it.

# The asymmetry, which is the whole design

Entering handover only ever *narrows* what Atlas will do. Leaving it
*grants*. Those are not the same act and must not be guarded the same way:

* **Anyone may enter.** Your friend can say "guest mode" themselves. The
worst a false entry costs is that Atlas is briefly less useful to you,
and you can undo it in one sentence.
* **Only the passphrase leaves.** `vault.rs` has real crypto on every
platform — argon2 over a passphrase, XChaCha20-Poly1305 over the
contents. That is the one thing in this codebase that genuinely proves
it is you, because it is something you know rather than something you
sound or look like.

The previous arrangement had this exactly backwards. Nothing could put
Atlas into a guest state at all, and `atlas profiles switch eric` — which
passes through no gate of any kind — took anyone straight out of one.

# Why no sensor may do either

A voice or a face reading is a **hint**, and hints are allowed to *ask*.
They are not allowed to decide, in either direction:

* A sensor that could *enter* handover can lock you out of your own
assistant. `voiceid::handle` returning `Ignore` on `NotYou` was already
this: a cold, a new headset, or sitting further from the microphone and
Atlas silently stops answering, with no error and nothing said. That is
fixed at the source; nothing here reintroduces it.
* A sensor that could *leave* handover is worth spoofing. A photograph is
cheap. A passphrase is not.

So `Hint` exists, and the only thing it can produce is an offer.

**Public interface:**

- `fn refuses`
- `fn would_hand_out_the_way_back`
- `fn not_yours_to_set`
- `fn refusal`
- `fn take_back_with`
- `struct Handover`
- `enum Stance`
- `enum Hint`
- `const FILE`
- `const NO_PASSPHRASE_YET`

### `src/handshape.rs`

**Wired** — something outside this file calls into it.

Gestures you define, from the geometry of your hand.

## Why this exists

There are two shapes a hand model can come in, and the difference decides
whether the gesture vocabulary is fixed forever or open.

A **classifier** outputs a label from a list somebody else chose:
`thumb_up`, `fist`, `victory`. Whatever is on that list is what you get. Ask
for a gesture the trainer did not think of and the answer is "retrain the
model", which in practice means never.

A **landmark model** outputs the position of every joint — twenty-one
points per hand. It has no opinion about what any of them mean. Every
gesture is then arithmetic on those points, written here, in Atlas.

Eric's constraint settles it: he does not want his vocabulary limited by
what somebody else's model already knows. So Atlas reads landmarks and
defines the gestures itself, and adding one is a few lines of geometry
rather than a training run. Two hands, twenty-one points each, is enough to
express anything a hand can physically do.

## Only computing what is actually used

His other constraint: nothing processed that nothing acts on. So a
`Vocabulary` holds only the gestures currently bound to something, and
`Reading` computes a feature the first time a gesture asks for it and not
at all if none do. Curl for five fingers, spread, pinch distance and
orientation are cheap individually and pointless in aggregate when three
gestures are enabled.

**Public interface:**

- `fn in_the_frame`
- `fn from_model`
- `fn needs_holding`
- `fn as_demonstrated`
- `fn needs_deciding`
- `fn sketch`
- `fn how_to`
- `struct Point`
- `struct Landmarks`
- `struct Reading`
- `struct Motion`
- `struct Trail`
- `struct Band`
- `struct Gesture`
- `struct Vocabulary`
- `struct Holding`
- `struct Learning`
- `enum Test`
- `enum Progress`
- `const POINTS`
- `const SAMPLES`
- `const AGREEMENT`

### `src/handtrack.rs`

**Wired** — something outside this file calls into it.

Making a slow detector feel instant.

Everything here is arithmetic. No model, no dependency, nothing to
download — the parts of hand tracking that decide whether it feels good
are not the parts that recognise a hand.

## The problem this solves

A hand detector that runs at ten frames a second gives you a pointer that
updates every hundred milliseconds. That is not a slow pointer, it is a
*broken-feeling* one: it lags behind your hand, then jumps to catch up, and
the jump is what makes people give up on gesture control within a minute.

Two things fix it, and neither needs a faster model:

**Predict where the hand is going.** Between detections, keep moving the
pointer along the direction it was already travelling. By the time the next
real reading arrives you are usually within a few pixels of it, and the
correction is invisible instead of a jump.

**Smooth harder when still, barely at all when moving.** A fixed smoothing
filter forces a choice between a jittery pointer at rest and a laggy one in
motion. Making the smoothing depend on speed gets both — this is the
one-euro filter, and it is about thirty lines.

## What it deliberately does not do

It does not invent a hand. Prediction runs for a fixed short window and
then stops; a pointer that keeps gliding after you drop your arm is worse
than one that stops. `Track::confident` says which you are getting.

**Public interface:**

- `struct SmoothConfig`
- `struct Track`
- `struct PaceConfig`
- `struct Pace`
- `const PREDICT_FOR_MS`

### `src/health.rs`

**Wired** — something outside this file calls into it.

Watching your machine.

Jarvis constantly reports suit status — power, damage, what's failing. The
useful version is Atlas watching *this laptop*, because the things that
actually interrupt your work are boring and predictable: a full disk, RAM
pressure, a backup that stopped running, a battery that stopped holding
charge.

The hard part isn't reading the numbers. It's saying something **once**,
at a moment worth interrupting for, and then shutting up about it.

**Public interface:**

- `fn assess`
- `fn summary`
- `fn read_machine`
- `fn power_from_status`
- `fn power_from_sysfs`
- `struct Readings`
- `struct Finding`
- `struct HealthConfig`
- `struct Reporter`
- `enum Severity`

### `src/hearing.rs`

**Wired** — something outside this file calls into it.

Which ear Atlas listens with, decided automatically.

The situation this exists for: a laptop closed on a stand behind the
monitors, so its own microphone array is muffled and half-blocked. A webcam
sitting on the monitor with a clear line to your face. AirPods that hear
you anywhere but cost audio quality while they listen. And a phone that can
hear you in another room entirely.

No single one of those is right. So Atlas keeps all of them and picks, and
the rules are:

1. **Never make you choose.** It measures which microphone actually hears
you rather than trusting a name.
2. **Only pay the Bluetooth cost when it buys something.** At the desk with
a webcam that hears you, the AirPods stay on full-quality playback.
3. **Follow you.** Away from the desk it switches to the headset; out of
the room, to the phone; back at the desk, back again.
4. **Don't flap.** Switching ears mid-sentence is worse than a slightly
worse microphone, so a change has to be clearly better and has to last.

**Public interface:**

- `fn short`
- `fn mean_volume`
- `fn calibration_args`
- `struct Candidate`
- `struct Where`
- `struct HearingConfig`
- `struct Choice`
- `struct Hearing`
- `enum Ear`

### `src/himalaya.rs`

**Wired** — something outside this file calls into it.

Reading mail through Himalaya, when you choose it (`mail.backend:
himalaya`).

**Source:** `pimalaya/himalaya` (MIT or Apache-2.0), a command-line mail
client, driven as a program with its `--json` output -- written against
its 2.1.0 source (28 Sep 2026): `envelope search [QUERY]` with its query
language (`not flag seen`, `after <yyyy-mm-dd>`), `-m/--mailbox` (an
alias such as `inbox` or `sent` from its own config), and `message read
<ID>`, which since 2.0 leaves a message unread unless told `--seen`. Its
`--json` message is `mail-parser`'s parsed message: every part decoded.

**Why it's offered.** Atlas's own IMAP (`imap`) is solid at the protocol
-- literals, timeouts, XOAUTH2 -- but it hands the body back as the raw
`BODY[TEXT]` and headers as sent: a `=?UTF-8?B?…?=` subject stays
encoded, a quoted-printable or base64 body stays encoded, a multipart
message arrives with its boundaries. Himalaya's parser decodes all of
that, and speaks JMAP, Gmail's and Microsoft's own APIs and local
Maildir as well as IMAP. It keeps its own accounts and passwords
(`himalaya configure`), so nothing from Atlas's vault is handed to it.

Default stays `imap`: Himalaya is a separate install, with its own setup.

**Public interface:**

- `fn search_args`
- `fn read_args`
- `fn iso_date`
- `fn envelopes`
- `fn fill_from_message`
- `fn fetch_inbox`
- `fn fetch_since`
- `fn as_host`
- `fn route`

### `src/hlc.rs`

**Wired** — something outside this file calls into it.

The clock two devices order their histories by, when their wall clocks
disagree.

Atlas merges by replaying both devices' event logs in order (see `sync`).
"In order" needs a timestamp both sides compute the same way. Wall-clock
time alone can't be that timestamp: your phone and your laptop are never
perfectly in step, and a phone that has been offline in a drawer can come
back a few seconds behind. Order by raw wall time and an edit you made on
the phone *after* one on the laptop can sort *before* it, so the merge
settles on the wrong answer — silently, and only when the clocks happen to
be skewed, which is the worst kind of bug to find later.

A **Hybrid Logical Clock** fixes exactly that. It's the well-worn design
from Kulkarni et al. (2014) — the same one CockroachDB and most local-first
systems use — kept in-house here because it's a page of logic, not a
dependency. The robustness details (the drift guard below, the carry on
counter overflow) are adapted from Eclipse Zenoh's `uhlc-rs`, the most
battle-tested HLC in Rust, translated to Atlas's seconds-and-"nothing is
lost" world rather than pulled in as a crate.

A stamp carries two numbers:

- `wall`: physical time (Atlas uses seconds, `store::now()`), never allowed
to go backwards even if the OS clock does.
- `count`: a tie-break that steps up when several events land in the same
`wall` second, or when a message arrives stamped in the same second.

Three promises: a device's own stamps only move forward; a stamp read off an
incoming event pulls this device's clock up to meet it, so anything it does
next sorts *after* everything it just learned; and `wall` stays within a
bound of true time — see the drift guard — so the ordering still reads like
real time to a person, and one device with a wildly wrong clock can't drag
everyone else's into the next decade.

Total order across devices is `(wall, count, device-id)`: this module gives
the first two, and `sync` breaks the last tie by device id so both sides
reach the identical order without talking to each other.

**Public interface:**

- `struct Stamp`
- `struct Skew`
- `struct Recv`
- `struct Clock`
- `const MAX_AHEAD_SECS`

### `src/hollow.rs`

**Wired** — something outside this file calls into it.

Answers that are not answers.

Two bugs found on the first day Atlas ran on real hardware shared a shape,
and neither was caught by 2,500 tests:

* `Daemon::readings` returned `Readings::default()`. Every field zero.
`assess` only reports on values above zero, so a machine that read as all
zeros looked like a machine with nothing wrong. **Absence of a finding
was indistinguishable from absence of a problem.**
* The typed prompt answered `parsed Outstanding — not wired to an action
yet` for every intent outside six. The sentence was true of the prompt
and false of Atlas — the action existed. **The code announced its own
incompleteness and nothing was listening.**

Call these hollow answers. They are worse than errors, because an error
stops and a hollow answer proceeds, looking fine, forever.

This module is the listener. It is used three ways: by `doctor` on demand,
by the nightly self-audit, and by `tests/no_quiet_nothings.rs` as a ratchet
over the source so the count can only go down.

**Public interface:**

- `fn judge`
- `fn judge_readings`
- `fn unread_instruments`
- `fn audit`
- `fn spoken`
- `struct Hollow`
- `enum Why`
- `const ADMITS_INCOMPLETE`
- `const ECHOES_THE_QUESTION`
- `const CLAIMS_A_GOOD_STATE`
- `const NULL_WORDS`
- `const SELF_QUESTIONS`

### `src/hollowcode.rs`

**Wired** — something outside this file calls into it.

Hollow code, in whatever language it arrives in.

`hollow.rs` judges an answer: did Atlas say something, or did it produce a
well-formed sentence about nothing. This is the same question one level
down, asked of code: **does this compile, run, pass, and do nothing?**

It is the failure this whole codebase keeps hitting. A timing window
nothing filled. A board nothing added to. A vault function nothing called.
Every one of those compiled, every one had passing tests, and every one was
found by reading rather than by running. This is an attempt to find that
shape by reading, automatically, in anyone's code.

## Why it reads rather than parses

No parser, no syntax tree, no language server. Those are per-language and
Eric will hand over whatever he happens to be looking at — a Go file from a
repo, a shell script, some JavaScript from a page. A real parser for each
is years of work and a wrong parse is worse than no parse.

What is being looked for does not need one. A function whose whole body is
`pass`, an empty `catch`, a result assigned and never used — these are
visible in the shape of the text in every language that has them. Reading
for shape is shallow, and shallow is honest here: it finds the thing it
claims to find and says nothing about what it hasn't looked at.

## What it will not do

It does not say the code is correct. It cannot: it has not run it, and it
does not know what it was supposed to do. Everything below is "this looks
like the shape of something unfinished" — a place to look, not a verdict.

**Public interface:**

- `fn read`
- `fn manifest_near`
- `fn made_up_dependencies`
- `fn spoken`
- `fn porting_notes`
- `struct Finding`
- `enum Tongue`
- `enum Shape`

### `src/hotkey.rs`

**Wired** — something outside this file calls into it.

The push-to-talk key, heard wherever you are.

`input::HoldToTalk` decided what a hold meant and nothing fed it: the
push-to-talk tier waited for Enter in Atlas's own console, which is no
use when you are in another window. This is the missing key source.

- **Windows:** a low-level keyboard hook (`WH_KEYBOARD_LL`) on its own
thread. The chosen key is held back from the app you're in until Atlas
knows whether it's a hold: a hold starts listening and the app never sees
the key; a quick tap is given back to the app (`SendInput`), so Tab still
tabs. Events Atlas injects are marked and ignored on the way back in.
- **Linux:** the kernel's keyboard device (`/dev/input/event*`), read, not
grabbed — so the key still reaches the app as well. Reading it needs the
`input` group; `doctor` says so when it can't.

The decisions — what to hold back, when a hold becomes talking, when to
give a tap back — live in `Gate`, which is plain logic and tested; the
platform code only reports key-down and key-up.

**Public interface:**

- `fn windows_vk`
- `fn linux_code`
- `fn parse_linux_event`
- `fn spawn`
- `struct Verdict`
- `struct Gate`
- `struct Keys`

### `src/hotkeys.rs`

**Wired** — something outside this file calls into it.

Keys that reach Atlas from anywhere in Windows (Eric's ruling H1, 25 Sep
2026): the wake word, push-to-talk *and* a typing box, all three, with the
keys set per person and none locked in.

Two keys, both from settings:

* **Push-to-talk** (`push_to_talk.key`, held for `push_to_talk.hold_ms`).
Hold it and speak; let go and Atlas hears it. A quick tap is given back
to the app you're in, so a push-to-talk key of Tab still types a tab.
That's what `input::HoldToTalk` was written for: the key is held back
until it's clear which you meant, and only a hold is kept.
* **The typing box** (`quick_input.hotkey`, e.g. `ctrl+shift+space`). Press it
and a one-line box opens over whatever you're doing (`typebox`).

On Windows the push-to-talk key is watched by a low-level keyboard hook,
which is the only way to hold a key back from the app you're in, and the
typing-box combination is registered with `RegisterHotKey`, which Windows
refuses if another program already owns it — said, rather than silently
not working. Elsewhere there are no global keys, and `start` says so.

**Public interface:**

- `fn key_code`
- `fn safe_alone`
- `fn spec_of`
- `fn check_setting`
- `fn key_word`
- `fn try_them`
- `fn heard_as`
- `fn start`
- `struct Keys`
- `struct Gate`
- `struct Hotkeys`
- `enum Pressed`
- `enum Hook`

### `src/household.rs`

**Wired** — something outside this file calls into it.

Keeping your Atlas yours, and your friends' theirs.

You're handing this to people. That means the default has to be that two
installs know nothing about each other — not "they're separate unless you
link them", but **separate in a way that can't be undone by accident**.

The danger isn't malice, it's convenience. Discovery on a shared wifi, a
private network someone joins to help you, a cloud folder in a family
account: any of those could quietly put two people's Atlas in the same
room. So belonging is decided by a key that only your devices have, and
nothing else — not the network, not the folder, not the account.

**Public interface:**

- `fn meets`
- `fn new_pairing`
- `fn encode_pairing`
- `fn decode_pairing`
- `fn this_device_name`
- `fn init`
- `fn accept_bundle`
- `fn share_with_friend`
- `fn saw_another`
- `fn new_invite_code`
- `fn leave_invitation`
- `fn take_invitation`
- `fn sweep_invitations`
- `struct Household`
- `struct Pairing`
- `struct HouseholdConfig`
- `struct Handoff`
- `struct Received`
- `struct ReceivedFile`
- `struct Inbox`
- `struct Invitation`
- `struct Inside`
- `enum Meeting`
- `const HANDOFF_FOLDER`
- `const MAX_WAITING`
- `const SEPARATE_BY_DEFAULT`
- `const THEIRS_IS_THEIRS`
- `const CODE_LEN`
- `const INVITE_VERSION`

### `src/http.rs`

**Wired** — something outside this file calls into it.

Minimal HTTP/1.1 client for localhost.

Needed for exactly one thing: asking Chrome's debugger endpoint which tabs
exist, so we can find a websocket URL to attach to. Plaintext, loopback,
no redirects, no TLS. A general HTTP client would be a much larger
dependency for a job this small.

**Public interface:**

- `fn get`
- `fn post_json`
- `fn post_json_with_token`
- `fn request`
- `fn get_with_token`
- `fn post_over`
- `fn post_kept`
- `fn https_get`
- `fn https_post_json`
- `fn whole_reply`
- `fn build_request`
- `fn parse_response`
- `fn dechunk_for_test`
- `struct Response`
- `enum KeptReply`
- `const MAX_RESPONSE`

### `src/hub.rs`

**Wired** — something outside this file calls into it.

The hub — a page you open when you want to change something.

Deliberately not an app. It's a page served on loopback by the API server
that already exists, which means: nothing new to install, it works from
your phone and iPad over the same connection, and **Atlas still runs with
no window open.** You open the hub the way you open a router's admin page —
occasionally, on purpose, then close it.

Plain HTML with no JavaScript framework and no external requests. It has to
work offline, because Atlas does.

**Public interface:**

- `fn crumbs`
- `fn index_rows`
- `fn nothing`
- `fn lines`
- `fn rows`
- `fn deck_figures`
- `fn gauge`
- `fn more`
- `fn works_without_voice`
- `fn access_page`
- `fn access_page_full`
- `fn route`
- `fn esc`
- `fn shell`
- `fn shell_at`
- `fn with_appearance`
- `fn with_waiting`
- `fn with_palette`
- `fn public_file`
- `fn manifest`
- `fn with_app_head`
- `fn with_business`
- `fn with_owner`
- `fn workspace_page`
- `fn looking_back_page`
- `fn recommendations_page`
- `fn idle_banner`
- `fn settings_page`
- `fn workshop_page`
- `fn calendar_page`
- `fn addons_page_with`
- `fn edits_page`
- `fn friends_page`
- `fn groups_page`
- `fn groups_page_with`
- `fn after_button`
- `fn back_with`
- `fn with_said`
- `fn with_refresh`
- `fn permissions_page`
- `fn dashboard_page`
- `fn dashboard_deck`
- `fn accounts_page`
- `fn palette_overlay`
- `fn find_page`
- `fn gestures_page`
- `fn status_page`
- `fn outstanding_page`
- `fn live_version`
- `fn now_page`
- `fn list_page_at`
- `fn form_fields`
- `fn form_field`
- `fn urldecode`
- `fn phone_block`
- `fn vault_section`
- `fn with_handed_over_banner`
- `fn sync_page_with`
- `fn space_section`
- `struct Appearance`
- `struct PublicFile`
- `struct FriendsView`
- `struct Deck`
- `struct Glance`
- `struct Stopped`
- `struct Open`
- `struct NowView`
- `struct VaultView`
- `struct SyncView`
- `struct SpaceView`
- `enum Page`
- `enum Dot`
- `enum Mark`
- `enum Step`
- `enum HouseView`
- `const NAV`
- `const THINKING`
- `const IDLE`
- `const APPEARANCE_KEY`
- `const MANIFEST_PATH`
- `const SERVICE_WORKER_PATH`
- `const ICON_192`
- `const ICON_512`
- `const ICON_MASKABLE_512`
- `const APPLE_TOUCH_ICON`
- `const SERVICE_WORKER`
- `const MARK`
- `const IDLE_TOGGLES`
- `const LIVE_SCRIPT`

### `src/hubjobs.rs`

**Wired** — something outside this file calls into it.

Hub buttons whose work goes over the network, run off the daemon's thread.

Every hub request is answered on the daemon's own thread, so a button
whose work waits on the network (sending a document over Tor, knocking on
a friend's Atlas, reaching a phone through Tailscale) used to freeze all
of Atlas until it finished: no voice, no other page, no tick. Now the
handler does its quick local checks, hands the slow part to the crew, and
comes straight back to the page with a job number in the address. The page
says what is happening and refreshes itself until the job has an answer,
then shows that answer once.

The crew thread writes the answer here itself, so a page shows it even
while the daemon's tick is paused; whatever the daemon must do afterwards
with its own state (log the send on the document, keep a friend to try
again) happens when the crew's news is taken (`take_crew_news`).

Also here: [`Flash`], for text that must never be in an address (a friend
link, an invitation code, a recovery key). It is held in memory, shown on
the next visit to its page, and gone after that or after five minutes.

**Public interface:**

- `fn notice_for`
- `fn keep_flash`
- `fn take_flash`
- `struct Job`
- `struct Jobs`
- `enum State`
- `enum Flash`
- `const FLASH_SECS`

### `src/hublive.rs`

**Wired** — something outside this file calls into it.

The hub, served by the Atlas that is actually running.

Every page except Settings and Access used to answer *"needs the full Atlas
running. This is settings-only mode."* — and there was no other mode. One
serve loop existed, in `run_hub`, with no daemon behind it. So `Now`,
`Outstanding`, `What I did`, `Connections` and the rest were written,
tested, routed, rendered and permanently empty. That is the `hollow`
pattern at the size of a whole feature: each part worked, and the thing
made of them did nothing.

This module is the join. It takes a live `Daemon` and answers a hub
request from what that daemon actually knows right now — no cache, no
second copy of the state, no placeholder text.

## Nothing here may print like code

Debug formatting (`{:?}`) on an internal enum reaches the screen as
`LookingBack` or `Kind::Upkeep`. That is a variable name leaking into a
product. Every string that reaches a page comes from a `plain()`-style
method written for a person, and `tests/hub_is_not_code.rs` fails the
build if debug formatting shows up in a page again.

**Public interface:**

- `fn reply`
- `fn ipa_facts`
- `fn ipa_facts_ready`

### `src/hubpages.rs`

**Wired** — something outside this file calls into it.

The hub pages the locked design drew and the first port didn't build.

The design (`design/hub/`, locked with Eric 20–21 Sep 2026, with its phone
screens redrawn on 24 Sep) has thirty desktop artboards and a phone for
each. The first port (26 Sep) built the frame, Home, Outstanding and Now.
This module is the rest: Messages, Documents, the business section
(Overview, Shared tasks as a table, a board or a calendar, Clients with the
firewall on the record, Partners), Sound & voice, Trusted recipients, Give
Atlas something, Offline, Talk, Start a project, and Help & accessibility.

Each page is a pure function of a view the running Atlas fills in
(`hublive`), so every one renders — and is tested — without a daemon. The
phone is not a second set of pages: the same pages reflow to one column
and the frame swaps the sidebar for the design's bottom tab bar
(`hub::STYLE`'s phone rules), because on a phone Atlas serves this hub to
its own WebView from the core running on the phone (`mobile`).

Accessibility is in the markup, not bolted on (EN 301 549 / WCAG 2.2 AA,
see `Help`): every control is a real `<a>`, `<button>` or labelled input,
every status is an icon *and* a word, tables have header cells, and live
parts announce themselves politely.

**Public interface:**

- `fn messages_page`
- `fn documents_page`
- `fn business_page`
- `fn shared_tasks_page`
- `fn ymd`
- `fn days_of`
- `fn clients_page`
- `fn partners_page`
- `fn sound_page`
- `fn trusted_page`
- `fn give_page`
- `fn offline_page`
- `fn talk_page`
- `fn new_project_form`
- `fn help_page`
- `fn updates_page`
- `fn feedback_page`
- `fn phone_page`
- `struct RoomRow`
- `struct Said`
- `struct MessagesView`
- `struct DocRow`
- `struct BusinessView`
- `struct TaskRow`
- `struct ClientRow`
- `struct VoiceRow`
- `struct EngineView`
- `struct SoundView`
- `struct OfflineView`
- `struct UpdatesView`
- `struct FeedbackView`
- `struct PhoneCode`
- `struct PhoneView`
- `enum TaskView`
- `enum Reach`
- `enum SendBuild`
- `enum ReleaseKey`
- `const MONTHS`
- `const TALK_WAIT_SCRIPT`

### `src/hubvault.rs`

**Wired** — something outside this file calls into it.

The hub pages that replaced terminal commands (27 Sep 2026).

Four things could only be done by typing `atlas …`: setting the vault
passphrase, taking a handover back, starting a household and giving this
device the household key, and freeing disk space. Somebody who never opens
a terminal could therefore never take their own machine back after handing
it over, and a fresh install's Sync page answered with a command. This
module is the hub's half of each; the deciding is in the library, shared
with the command line: `vault::set_passphrase`, `vault::make_recovery_key`,
`handover::take_back_with`, `sync::set_key`, `household::init`,
`reclaim::roots_from_env` and `reclaim::reclaim`.

The drawing is in `hub.rs` (`vault_section`, `sync_page_with`,
`space_section`, `with_handed_over_banner`). Each handler here is reached
by one arm in `hublive`'s dispatch.

## What is kept, and where

A recovery key is shown **once**, on the page, and nowhere else: never in
an address (a redirect's `?said=` lands in history and logs), never in the
store. It waits in `ShownOnce`, in memory, until the next draw of the
Accounts page takes it. The passphrase forms each carry a one-time mark,
so a refresh that re-sends the form is answered "already sent" rather than
acted on twice. `ShownOnce` is this module's own one-shot store; the hub's
general one-shot notice (`flash_once`) arrived in parallel and the two
belong together.

**Public interface:**

- `struct ShownOnce`
- `struct Survey`
- `const SURVEY`

### `src/hubwin.rs`

**Wired** — something outside this file calls into it.

The hub, inside Atlas's own window.

Eric, 23 Sep 2026: *"What was the hub designed for … if it's not going to
get used or accessible on the laptop? … that's how everything can be
tracked, things get accessed, and settings changed."* He was right. The 17
Sep ruling ("the hub should not be a browser — you say that if Atlas has a
dependency on the internet") had been read as "the laptop doesn't get the
hub", which left every hub page — status, activity, approvals, devices,
outstanding, connections — reachable only from the phone.

His ruling on 23 Sep: the hub shows **inside Atlas's window**, drawn by the
web view that ships with Windows (WebView2), not by a browser. What that
means in practice, and what keeps the 17 Sep concern answered:

- **No browser.** No address bar, no tabs, no other program launched. It is
a region of the Atlas window.
- **No internet.** It loads only Atlas's own hub on this machine
(`127.0.0.1` on Atlas's port). Any link that would leave it is refused —
`stays_on_the_hub` is the one rule, and it is tested.
- **The same pages as the phone.** One set of pages, so the laptop and the
phone can never disagree about what a setting is or what's outstanding.

On anything but Windows there's no system web view to borrow, so the Hub
page says so and gives the address instead.

**Public interface:**

- `fn page_url`
- `fn stays_on_the_hub`
- `fn without_token`
- `fn for_the_browser`
- `struct Area`
- `struct Hub`

### `src/identity.rs`

**Wired** — something outside this file calls into it.

Proving it's you, without getting in your way.

Face ID's real lesson isn't the camera — it's that you prove yourself
*rarely* and it feels like nothing. An assistant that demands a PIN before
every action is one you stop using, and one that demands it while you're on
your phone is one you can't use at all.

So identity here works on three ideas:

1. **Proof lasts.** Like `sudo`. Prove once and consequential actions go
unchallenged for a good while. The window resets on activity, not on a
fixed clock.
2. **Almost nothing needs it.** Only genuinely irreversible things. Opening
apps, research, notes, drafts — never.
3. **The device you're on already proved it.** Your phone unlocked with
your face before Atlas ever saw the request. Asking again is asking the
same question twice, so a trusted device carries its own proof.

Where Windows Hello isn't available at all, unavailable means "fall back to
a spoken yes" — never "assume it's him".

**Public interface:**

- `fn grace_remaining`
- `fn explain`
- `struct IdentityConfig`
- `struct Identity`
- `enum Hello`
- `enum Proof`
- `enum From`
- `enum Gate`

### `src/imap.rs`

**Wired** — something outside this file calls into it.

IMAP, from scratch, over whatever transport you hand it.

Not curl. curl's IMAP support is real but incomplete — its own
maintainers describe `UID FETCH` as unimplemented and custom commands
as "overloading the behaviour of a LIST command." Good enough for a
single URL-shaped fetch (which is all `research.rs` needs), not good
enough for a real mail client that has to `SEARCH`, then `UID FETCH`
exactly the messages that search found.

`Session<S>` is generic over `S: Read + Write` rather than hardcoding a
TLS socket, on purpose: the protocol logic — command formatting,
response parsing, literal handling — is the part worth getting right
and the part a test can actually exercise, by handing it an in-memory
stream instead of a real server. The real transport (`native-tls` over
`TcpStream`) is one thin call site, not tangled into the parsing.

**Public interface:**

- `fn connect`
- `fn list_entry`
- `struct Session`
- `struct Response`
- `struct Message`
- `enum Status`

### `src/improve.rs`

**Wired** — something outside this file calls into it.

Getting better without new hardware.

The question worth answering: given the machine you already have, what
makes Atlas better in six months than it is today?

Not a bigger model. The honest answers are all about **spending what you
have more cleverly** and **accumulating things that don't cost memory** —
and most of them run while you sleep.

**Public interface:**

- `fn mechanisms`
- `fn automatic`
- `fn automatic_cost_mb`
- `fn hint_args`
- `fn progress`
- `struct Mechanism`
- `struct Vocabulary`
- `enum Gain`
- `const HINTS_GIVEN`

### `src/index.rs`

**Wired** — something outside this file calls into it.

Indexing engine: "know what exists in the workspace".

Metadata-first by design — path, name, extension, size, timestamp, asset
class. Reading file *contents* is on-demand enrichment, never part of the
background scan, because a laptop that reindexes Documents by content will
be unusable while it does.

**Public interface:**

- `fn default_roots`
- `struct Entry`
- `struct IndexConfig`
- `struct Index`
- `struct Missed`
- `struct ContentHit`
- `struct Changes`
- `struct Loading`
- `enum AssetClass`
- `enum Settled`
- `const STILL_READING`

### `src/infer.rs`

**Wired** — something outside this file calls into it.

Running a model inside Atlas.

`tract` is a pure-Rust inference engine. It compiles into `atlas.exe` —
no second process, no Python, no runtime to install. That makes it more
self-contained than what Atlas already does for speech, which launches
`whisper-cli.exe` and `piper.exe` as separate programs.

What it does not remove is the weights file. No inference engine invents
one, and training a hand tracker from nothing is months of work and a
dataset Eric does not have. So the arrangement is the same one Atlas
already uses for `ggml-base.en.bin` and the Piper voice: **our code,
somebody's weights, downloaded once.** The difference is that the code
running them is now ours rather than another program.

## What this module is careful about

Loading a model is slow and happens once; running it happens twenty times
a second. So the load is separate from the run, failures at load are
reported plainly rather than retried per frame, and a model that is absent
is a stated fact rather than a silent no-op — which is the failure this
whole codebase keeps producing.

## Models do not agree on anything

The first version of this module assumed one shape of model, because it
only had one job. Every model it fed got a picture laid out as
`[1, height, width, 3]`, scaled to nought-to-one, red first, and every
caller read output number zero.

None of that is universal, and each assumption fails silently rather than
loudly:

- **Layout.** A picture can arrive with each pixel's three channels
together (`[1, h, w, 3]`) or with all the reds, then all the greens,
then all the blues (`[1, 3, h, w]`). The hand models want the first;
the face, object and picture models want the second. Handing a model
the wrong one is not an error — the numbers all fit — it is a model
that runs perfectly and sees nothing recognisable.
- **Scale.** Some models want nought-to-one, some want the raw
nought-to-255, some want the average subtracted first.
- **Which output.** A model with one answer has one output. A face
detector has twelve, and the boxes are not the first of them. Reading
output zero and calling it the answer is how you get a face detector
that appears to work and can never find a face.

So a `Kind` now carries a full `Recipe`, and running a model hands back
*every* output, named by position, with a stated error when the one asked
for is not there.

**Public interface:**

- `fn whats_missing`
- `fn spoken`
- `fn prepare`
- `fn prepare_crop`
- `fn fit`
- `fn arrange`
- `struct Recipe`
- `struct Outputs`
- `struct Model`
- `enum Layout`
- `enum Kind`

### `src/inhibit.rs`

**Wired** — something outside this file calls into it.

Actually keeping the machine awake while work runs.

`awake` decides whether a piece of work is worth holding the machine up
for, and the daemon asked it every night — then could do nothing with the
answer, because no platform hook existed ("Atlas cannot physically inhibit
sleep"). This is the hook, one per system, each scoped to the work and
released the moment it ends:

- **Windows:** `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`
on a thread that lives exactly as long as the hold. The screen may still
turn off; only system sleep is held. (The approach keepawake-rs, MIT,
takes; written here against the API directly.)
- **Linux:** a `systemd-inhibit --what=sleep` child that holds the lock
until it is killed.
- **macOS:** `caffeinate -i`, the same way.

Dropping the `Held` lets go. So does the process ending: every one of
these is released by the operating system when its owner goes, so a crash
can't leave a laptop hot in a bag.

**Public interface:**

- `fn apply`
- `struct Held`

### `src/input.rs`

**Wired** — something outside this file calls into it.

Three ways in, in priority order, with automatic fallback.

1. **Voice** — the primary interface. Wake word, speak, done.
2. **Push-to-talk** — when the wake word stops firing (noisy room, model
struggling), a key press replaces it. Same speech path, manual trigger.
3. **Typing** — when audio itself is broken (no mic, device unplugged,
binaries missing). Always available, never the thing you have to use.

Degradation is automatic and announced. You should never be standing there
repeating yourself at a machine that quietly stopped listening.

**Public interface:**

- `fn audio_available`
- `struct Utterance`
- `struct Tiers`
- `struct HoldToTalk`
- `struct Keyboard`
- `enum Source`
- `enum Tier`
- `enum KeyEvent`

### `src/install.rs`

**Wired** — something outside this file calls into it.

Getting everything Atlas needs, in one go.

Setting this up meant visiting four sites, picking the right build out of a
release page, unzipping into the right folder, and downloading two models
whose filenames you had to already know. That's a lot of chances to get one
step wrong and not find out until something silently doesn't work.

So: one list, one command, and it can be run again safely. Anything already
present is left alone, anything half-downloaded is replaced, and at the end
it says what's there and what isn't rather than assuming success.

**Public interface:**

- `fn pieces`
- `fn required`
- `fn download_mb`
- `fn state_of`
- `fn what_to_fetch`
- `fn before`
- `fn after`
- `fn where_it_lands`
- `fn wanted`
- `struct Piece`
- `struct InstallConfig`
- `enum Source`
- `enum State`
- `enum Step`
- `const COSTS_NOTHING`

### `src/integrations.rs`

**Wired** — something outside this file calls into it.

Whether each connection is still working.

`connectivity` asks whether the internet is reachable. `doctor` checks
everything, once, when you ask. Neither answers the question that actually
bites: **is this one integration still working right now?**

The failure it exists for is quiet. A token expires, a service changes an
endpoint, a machine goes off the network — and Atlas carries on, because
nothing that needed that integration happened to run. You find out days
later when something you cared about silently did not happen.

Three ideas do the work here.

**Last success, not last attempt.** An integration that has failed forty
times in a row is not "recently active". The clock that matters runs from
the last time it actually worked.

**Silence is not health.** An integration nobody has called in a week is
unknown, not fine. Reporting it as working is the mistake this module was
written to stop making, and it is the same mistake `hollow` names: absence
of a finding read as absence of a problem.

**A stale check is worse than no check.** A monitor whose own last look was
yesterday will tell you yesterday's answer in the present tense.

**Public interface:**

- `fn dependencies`
- `fn sources_for`
- `fn mark`
- `struct Integration`
- `struct Board`
- `enum Health`
- `const QUIET_AFTER_SECS`
- `const MONITOR_STALE_AFTER_SECS`
- `const INTERNET`
- `const MODEL`

### `src/intent.rs`

**Wired** — something outside this file calls into it.

Phrase -> Intent. Deterministic, config-driven, no model in the loop.
Longest phrase wins, so "open workspace" never gets eaten by "open".

**Public interface:**

- `fn without_fillers`
- `fn normalize`
- `fn from_tool`
- `struct Parser`
- `struct KnownNames`
- `struct ToolEntry`
- `struct ToolBook`
- `enum Intent`
- `enum Exposure`
- `const NEVER_FOR_THE_MODEL`

### `src/interrupt.rs`

**Wired** — something outside this file calls into it.

Deciding whether to say anything at all.

Everything Atlas might tell you goes through here. Without one place that
decides, every feature politely announces itself and the sum is a system
that talks constantly about nothing — which is how people end up ignoring
the one message that mattered.

The test is not "is this true" or "is this interesting". It is **would you
do something differently if you knew**. Almost nothing passes that, which
is the point.

**Public interface:**

- `fn unmute_from`
- `fn mute_from`
- `struct Thing`
- `struct InterruptConfig`
- `struct Gate`
- `struct Muted`
- `enum Weight`
- `enum Doing`
- `enum Decision`

### `src/ios.rs`

**Wired** — something outside this file calls into it.

What Atlas can actually do on an iPhone or iPad.

iOS is a much smaller box than Windows and pretending otherwise leads to a
phone app that promises things it can't do. Some of what Atlas does on the
laptop is simply unavailable — not hard, unavailable — and some of what
iOS gives you for free is better than anything I'd have built.

The honest summary: **it can hear you, talk back, think, remember, read
documents and cameras, and reach the laptop. It cannot watch your screen,
touch other apps, or run all the time.**

**Public interface:**

- `fn abilities`
- `fn cannot`
- `fn works_offline`
- `fn ways_to_start`
- `fn phone_is_better_at`
- `fn laptop_is_better_at`
- `fn first_run`
- `struct Ability`
- `struct IosConfig`
- `enum Can`
- `const THE_BIG_ONE`
- `const WHAT_IOS_DOES_BETTER`

### `src/judgment.rs`

**Wired** — something outside this file calls into it.

Graded judgments, and how sure Atlas is of one.

# What this is

Atlas makes small judgments in a dozen places and each one grew its own
way of doing it. `certainty` grades an answer on three levels with two
thresholds. `grade` checks audio and pictures against numbers. `tier`
and `levels` band things. `hollow` finds answers
that are not answers and returns them **in the order the questions were
asked**, then calls the first three "worst".

They are all the same two shapes underneath:

* **Level** — where something sits on an ordered scale whose bands are
*described*, not just numbered. "Over budget" and "beyond help" are
different bands and the difference is a sentence, not a number.
* **Holds** — how strongly the evidence says a condition is true, with
the evidence against it counted rather than ignored.

[`crate::whichone`] is the third — picking one of several named readings —
and it is where it is because it was built first, for the daemon's intent
arms. This module shares its [`crate::whichone::Clarity`] rather than
declaring a second word for the same idea: **two ways to say "I am not
sure" is how one of them goes unused and then wrong.**

# The contract, which is the point

Every judgment here returns *how sure* alongside *what*, and the caller
is expected to do something different when the answer is `Close`. That is
the rule `certainty` and `understood` already keep, made available to
everything else rather than reimplemented per module.

A judgment that cannot be made comes back as [`Clarity::Nothing`] rather
than as a middle band. **Not knowing and being in the middle are different
facts**, and collapsing them is how `readings` returned all zeros and
looked like a healthy machine — the bug `hollow.rs` was written about.

# No model

Everything here is arithmetic over evidence the caller already has. It
runs on a machine with nothing downloaded, it is testable, and it is the
same answer every time. Where a judgment genuinely needs to read prose,
`brain.rs` is the model and `certainty`/`understood` are what grade what
it says; this is for the far larger number of judgments that never needed
one.

# And it is wired

A substrate is an excellent place for things to be built and never
reached — this tree has 372 functions that only tests call, and a general
mechanism with no caller would be the 373rd.
`tests/a_judgment_says_how_sure_it_is.rs` fails the build if anything
here has no production caller.

**Public interface:**

- `fn add_up`
- `fn which_band`
- `fn how_strongly`
- `fn how_unusual`
- `fn ordinary_for`
- `fn ordinary_for_counts`
- `fn weighed_together`
- `fn only_partly_measured`
- `fn severity_of`
- `fn worth_raising`
- `fn worst_first`
- `struct Signal`
- `struct Band`
- `struct Graded`
- `struct JudgmentConfig`
- `struct Held`
- `struct Measured`
- `const MIN_SAMPLE`
- `const HOW_BAD`

### `src/kin.rs`

**Wired** — something outside this file calls into it.

A door another Atlas can knock on, that only ever rings a bell.

Built for one thing: a future Atlas running on its own server,
not linked to this one, that needs to say "this is urgent" without either
instance reaching into the other. "Not linked but able to communicate" is
a real tension — a shared network address is not the same thing as shared
trust, and the whole design here is making sure it never becomes that.

Three rules, and the first is the one that makes the other two matter:

1. **A signal can become exactly one thing: a `Nudge`.** Not a command,
not an approval, not a memory write, not a setting change. There is no
function in this file that turns an `Incoming` into an `Intent` or an
`Action`, and there must never be one. `tests/guards.rs` fails the
build if that stops being true.
2. **Trust is named, not inherited.** Being reachable on the same Tailscale
network is not the same thing as being trusted. A peer has to be added
once, deliberately, the way a person adds a contact — never
trust-on-first-use, never "anything that knows the address."
3. **A channel that is always right stops being checked.** The same
discipline `nudge` already applies to your own stalled goals applies
here: rate-limited per peer, and a peer that floods this loses standing
the same way a nudge you keep ignoring does — see `standing()`.

What this deliberately does not do: authenticate the *content* of a
message as true. Another Atlas saying "sell everything" is not a
command Atlas can act on through this door — it becomes a nudge you read
and decide about, exactly like everything else Atlas has ever proposed.

**Public interface:**

- `fn as_nudge`
- `fn as_waiting`
- `fn json_string_for_test`
- `fn encode_invite`
- `fn decode_invite`
- `fn where_pairings_live`
- `fn invite`
- `fn accept`
- `fn sealed_post`
- `fn chat_wire_body`
- `struct PeerConfig`
- `struct KinConfig`
- `struct Peer`
- `struct Incoming`
- `struct Delivered`
- `struct DeliveredFile`
- `struct FeedbackIn`
- `struct Befriended`
- `struct Hello`
- `struct GroupList`
- `struct LeftGroup`
- `struct ReadReceipt`
- `struct Chatted`
- `struct Door`
- `struct Contact`
- `struct Invite`
- `struct Pairings`
- `struct Accepted`
- `struct RoomMeta`
- `struct PeerLink`
- `struct Routes`
- `struct Reached`
- `struct TorConnections`
- `enum Urgency`
- `enum Refused`
- `enum Arrived`
- `enum InviteError`
- `enum Sent`
- `enum Via`
- `const DEFAULT_PORT`
- `const MAX_PER_WINDOW`
- `const WINDOW_SECS`
- `const MAX_HANDOFFS_PER_WINDOW`
- `const MAX_CHATS_PER_WINDOW`
- `const MAX_READS_PER_WINDOW`
- `const MAX_LEAVES_PER_WINDOW`
- `const MAX_NOTICES_PER_WINDOW`
- `const MAX_HANDOFF_FILE_BYTES`
- `const MAX_FRIEND_KNOCKS_PER_WINDOW`
- `const MAX_RELEASE_PIECES_PER_WINDOW`
- `const INVITE_TAG`
- `const REACHED`
- `const ONLINE_SECS`
- `const MAX_KEPT`
- `const KEPT_IDLE_SECS`

### `src/knowhow.rs`

**Wired** — something outside this file calls into it.

Knowing how to do things without asking anyone.

A model is not knowledge. It is a way of producing plausible text about
knowledge, and when it isn't there — or is a 1.5B running on a laptop —
Atlas needs something else to fall back on that isn't guessing.

So Atlas ships with procedures. Not facts about the world, which go stale
and are what the internet is for, but **how to get things done**: the steps,
what has to be true first, what usually goes wrong, and what to do when it
does. That's the part that doesn't change, and it's why Atlas offline is
narrower than Atlas online rather than useless.

Everything here is written once and improved by use: when a procedure fails
in a new way, the failure is added, so the same surprise happens once.

**Public interface:**

- `fn content_words`
- `fn shipped`
- `fn announce`
- `fn as_plan`
- `fn checklist`
- `fn look_back`
- `struct Procedure`
- `struct Step`
- `struct Snag`
- `struct Knowhow`
- `const LEARNED`

### `src/kokoro.rs`

**Wired** — something outside this file calls into it.

Kokoro, the better voice, spoken inside Atlas.

Until 28 Sep 2026 "Kokoro" in this tree meant a wrapper script you were
told to write yourself (`tools/kokoro/speak.cmd`), which nothing
installed, so nobody had it. This is Kokoro spoken by Atlas itself,
through sherpa-onnx (k2-fsa, Apache-2.0) — its C library, loaded when the
first sentence is spoken, not linked into atlas.exe.

## Why loaded at run time rather than linked

The official Rust crate (`sherpa-onnx` 1.13.8) links sherpa-onnx and ONNX
Runtime statically, from prebuilt archives its build script downloads.
Measured here on 28 Sep 2026:

- **Linux**: builds and runs (a 40 MB test program).
- **Windows, cross-built with MinGW** (`x86_64-pc-windows-gnu`, how every
atlas.exe so far was made): fails. The only Windows archives are built
with Microsoft's compiler (`win-x64-static-MT-Release-lib`, `.lib`
files, 123 MB). MinGW's linker can't find them under their names at
all, and renamed, it can't read the Microsoft linker directives inside
(`/FAILIFMISMATCH`, `/DEFAULTLIB:libcpmt`, Microsoft's own C++ runtime)
and ran out of memory (ld killed, signal 9) before finishing.
- **Windows on GitHub Actions** (`windows.yml`) builds with Microsoft's
own toolchain (`x86_64-pc-windows-msvc`), where the static archives
would link — but it would put ~20 MB of ONNX Runtime into every
atlas.exe, download 123 MB on every uncached build, and make the MinGW
build impossible.

The C library's *shared* build has none of those problems: its functions
are plain C, so an exe from either compiler can call them. The Windows one
(`win-x64-shared-MT-Release-lib`, 8 MB) carries its own C runtime and needs
nothing installed (its imports are KERNEL32, ADVAPI32, and ONNX Runtime
beside it). So Atlas downloads it with the voice, into `tools/kokoro/`,
and opens it with `libloading` the first time it speaks in Kokoro.

The structures handed across are copied from `sherpa-onnx-sys` 1.13.8
(Apache-2.0) and are only right for that version, so the library's own
version is checked before anything is passed to it: a different one is
refused in words, never called with the wrong layout.

## Licences

Kokoro's weights and sherpa-onnx are Apache-2.0; ONNX Runtime is MIT.
**The sherpa-onnx library also contains espeak-ng (GPL-3.0)**, which it
uses to turn words it has no pronunciation for into sounds. That is the
same licence `piper1-gpl` was avoided for. Atlas doesn't link it — it is a
separate download, opened at run time — which keeps it out of atlas.exe
itself, but it is GPL code running in Atlas's process, and that is said
here rather than discovered later. (piper, the voice Atlas already ships,
is in the same position: its Windows zip carries `espeak-ng.dll`.)

## Falling back

No runtime, no model, a load that fails: Atlas speaks in piper as before,
says why once (`note_once`), and doesn't try again on every sentence
(`engine` keeps the failure until `forget` — after a download finishes).

**Public interface:**

- `fn english_voices`
- `fn speaker_id`
- `fn voice_or_default`
- `fn runtime_files`
- `fn runtime_piece`
- `fn model_piece`
- `fn pieces`
- `fn download_mb`
- `fn check`
- `fn to_wav`
- `fn thread_count`
- `fn warm_up`
- `fn engine`
- `fn forget`
- `fn note_once`
- `fn last_note`
- `fn fetch_all`
- `fn display_name`
- `fn accent`
- `struct Ready`
- `struct Kokoro`
- `struct Ahead`
- `enum Missing`
- `const SHERPA_VERSION`
- `const RUNTIME_DIR`
- `const MODEL_DIR`
- `const DEFAULT_VOICE`
- `const VOICES`
- `const DOWNLOAD_ID`

### `src/lanes.rs`

**Wired** — something outside this file calls into it.

Two lanes of work, so Atlas can be busy without making you wait.

The constraint that shapes this: on Windows, synthetic clicks and
keystrokes go to whatever window has focus. Anything Atlas does that needs
*your* windows will take focus from you. There is no way around that at the
OS level.

So work is split. Background work never touches your screen — web research
in a separate headless browser, searching indexed files, reading documents,
writing notes. It runs whenever, while you keep working. Foreground work —
clicking, scrolling, typing into a window, rearranging your layout — is
queued and waits until you are actually idle, or until you tell it to go
now.

The result is that "do some research on X" runs immediately and invisibly,
and "fill in that form" waits for a gap.

**Public interface:**

- `fn lane_for`
- `struct Task`
- `struct LaneConfig`
- `struct Queue`
- `enum Lane`
- `enum TaskState`

### `src/language.rs`

**Wired** — something outside this file calls into it.

Accents, other languages, and understanding a room.

Two different problems that share a solution.

**Accents.** Whisper was trained on a very wide range of speech, so it
handles accents better than almost anything you could build, and better
than most people expect. Where it struggles is heavy accents on the
smallest models — and the fix is a bigger model, which costs memory you
don't have much of. So this measures how well it's actually doing rather
than guessing, and suggests a step up only when the evidence says so.

**Other languages.** The English-only model cannot hear them at all — not
badly, at all. Swapping to the multilingual model of the same size costs
nothing in memory and a little accuracy on English, and gains ninety-odd
languages plus translation. Whisper can transcribe *or* translate to
English in the same pass, which is the whole feature for one flag.

**Public interface:**

- `fn model_facts`
- `fn next_size_up`
- `fn template_vars`
- `fn insert_whisper_vars`
- `fn args`
- `fn plan`
- `fn notes`
- `fn live_line`
- `struct LanguageConfig`
- `struct ModelFacts`
- `struct Heard`
- `struct Listening`
- `struct Turn`
- `enum Task`
- `enum Plan`

### `src/later.rs`

**Wired** — something outside this file calls into it.

The list for later.

Eric, 25 Sep 2026 (F8): "I can instruct Atlas to add it to a list for
later as well so it's not forgotten." Anything Atlas just said (a
recommendation, an offer, an answer) can be put on it, read back on
request, and it's mentioned in the brief once a week so it isn't
forgotten. Nothing on it is acted on; it's a list, not a queue.

**Public interface:**

- `fn gist`
- `struct Item`
- `struct Later`
- `const RECORD`

### `src/launcher.rs`

**Wired** — something outside this file calls into it.

One place to say what you want to open: an app, a file, a Start-menu
shortcut or one of Atlas's own commands -- ranked by how well the words
match and how often, and how lately, you've picked it.

**Sources:** PowerToys Run (MIT) and Flow Launcher (MIT), read for the
shape: one query box, many plugins, results ranked and the top one
taken when it's clearly the one. The ranking is frecency in the sense
Mozilla's Places used for the Firefox address bar -- a use counts for less
as it ages -- here as an exponential half-life (`HALF_LIFE`), which needs
no buckets and stays bounded (`MAX_USES` per item). Typo tolerance is
`typos` (OSA distance, the palette's allowance).

**Soundproofing.** It opens only what the query clearly means: when the
best two are close, it lists them rather than guessing (`Pick::Choose`).
It learns only from what you actually picked. The list of things it can
open is built from what's on this machine; nothing is fetched.

**Public interface:**

- `fn match_score`
- `fn launch_ranking`
- `fn pick`
- `fn shortcuts`
- `fn start_menu_dirs`
- `fn say_choices`
- `struct Candidate`
- `struct Uses`
- `enum Kind`
- `enum Pick`
- `const HALF_LIFE`
- `const MAX_USES`
- `const MAX_ITEMS`

### `src/layout.rs`

**Wired** — something outside this file calls into it.

Resolve logical monitor roles to physical monitors at runtime.

This is the fix for the hard-coded `id: 1 / id: 2` in the old
monitor_layout.yaml: roles claim monitors by geometry, so unplugging a
display or changing the Windows arrangement does not break placement.

**Public interface:**

- `fn resolve_roles`
- `fn monitor_for_role`
- `fn to_pixels`

### `src/layout_prefs.rs`

**Wired** — something outside this file calls into it.

Arranging the hub the way you want it.

A dashboard someone else laid out is one you read once. The reason
drag-and-drop matters isn't the dragging — it's that after ten minutes of
rearranging, the thing you look at first is the thing you actually care
about, and that's what makes you open it again tomorrow.

What's kept is an order and a size, nothing more. No pixel positions: a
layout pinned to coordinates breaks the moment you use a different screen,
and you have three.

**Public interface:**

- `struct Placed`
- `struct Layout`
- `struct LayoutConfig`
- `enum Block`
- `enum Size`
- `const DRAG_SCRIPT`

### `src/learned.rs`

**Wired** — something outside this file calls into it.

Remembering what didn't work.

Phase 3, and the thing that stops Atlas being annoying over months rather
than over one session. Without it, every failure is new: the same approach
gets tried again in a fortnight, the same dead end gets walked into, and
you have to say "we tried that" yourself.

Two rules keep this from becoming a system that refuses to do anything.
What failed is recorded with **why**, so a failure caused by something
since fixed doesn't count forever. And a lesson expires — the world moves,
and a site that blocked automation in March may not in July.

**Public interface:**

- `fn spoken`
- `struct Lesson`
- `struct Learned`
- `enum Cause`
- `enum Advice`

### `src/ledger.rs`

**Wired** — something outside this file calls into it.

Reading your statements, and knowing enough about money to be useful.

Phase 2.5, and the oldest item in your spec. Entirely local: files you
already have, no credentials, nothing leaves the machine.

## On the tax knowledge

Atlas knowing the rules is the difference between "you spent $4,200 on
software" and "that's likely deductible, and the receipts need keeping for
three years". But it is knowledge, not advice — it can tell you what a
category usually is and what a rule generally says. It cannot tell you what
your situation is, and for a trader the rules are genuinely unusual, so it
says so rather than pretending.

Everything here is US federal, general, and current to the model's
knowledge. Anything that matters is worth checking with someone whose job
it is.

**Public interface:**

- `fn trading_rules`
- `fn keep_for`
- `fn categorise`
- `fn summarise`
- `fn spoken`
- `fn relevant_rule`
- `struct Rule`
- `struct Summary`
- `enum Category`
- `const NOT_ADVICE`

### `src/levels.rs`

**Wired** — something outside this file calls into it.

Where a stop and a target go, and why.

## Best, and what that word is allowed to mean

It names a best target **by this analysis** — the nearest level in the way
that clears the minimum reward, because the nearest level is the one price
is most likely to actually reach — and it lists the runners-up with what
each would be worth. That is a defensible claim about a reading of the
chart.

What it does not do is claim the analysis is the truth. **An unscored
analysis is worth nothing**: a call made here is only as good as the
record of checking calls like it against what actually happened.

It also will not tell you which way to trade. That belongs to whatever is
arguing the direction out.

## What it does instead

Given a direction someone else has decided, it answers three questions of
arithmetic:

1. **Where is this idea wrong?** A stop goes beyond the structure whose
breaking would mean the reason for the trade is gone — not at a round
number, and not at a fixed pip distance.
2. **How far is the next thing in the way?** That is the target, because
it is where price has something to do other than continue.
3. **Does the arithmetic survive the cost?** The spread is charged twice
and compared to the reward being aimed at. This is the check that makes
a small target at a small timeframe *arithmetically impossible* rather
than merely unwise: a spread charged on both legs is a small share of a
one-R move on an hourly chart and a large one on a one-minute chart, and
a target a small fraction of the stop away can need a win rate above
100% just to pay for itself.

Every answer carries the reason for it and the price at which the idea is
wrong. A level with no stated invalidation is not a level, it is a hope.

## Refusing is a result

`propose` returns a refusal far more often than it returns an idea, and
each refusal names its own cause and the number behind it. That is the
point. A system that always has an answer is a system whose answer means
nothing, and this codebase has been burned specifically by things that
produced confident output from nothing at all.

**Public interface:**

- `fn gaps`
- `fn how_many_ranges_out`
- `fn stop_is_in_noise`
- `fn how_far_it_travels`
- `fn cost_share`
- `fn propose`
- `struct Purse`
- `struct Rules`
- `struct Idea`
- `enum Side`
- `enum NoTrade`

### `src/lifecycle.rs`

**Wired** — something outside this file calls into it.

Keeping heavyweight helpers from becoming a permanent tax.

A headless Chrome is ~180MB and it does not give that back while it runs.
Windows will page it out under pressure, which is worse — now you pay disk
I/O as well. So nothing heavyweight is started until it is needed, and
everything is reaped once it has been idle.

There is also a hard ceiling. When Atlas's helpers together exceed the
budget, the least recently used one is killed even if it has not timed out.
An assistant is not entitled to unbounded memory on your machine.

**Public interface:**

- `fn typical_mb`
- `struct LifecycleConfig`
- `struct Helper`
- `struct Supervisor`
- `struct Helpers`
- `enum Order`

### `src/linkage.rs`

**Wired** — something outside this file calls into it.

Is "Jon Smith <jon.smith@acme.com>" the same client as "Smith, John
(555) 010-2000"? Record linkage for the client list and contacts.

**Sources:** Jaro–Winkler as in `rapidfuzz/strsim-rs` (MIT): prefix scale
0.1, at most 4 prefix characters, and the prefix bonus applied only when
the plain Jaro score is above 0.7. The scoring model is Fellegi–Sunter as
used by `moj-analytical-services/splink` (MIT): each field compares into a
level, each level carries m (P(level | same person)) and u (P(level |
different people)), and the evidence adds up as log2(m/u). Clean-room.

**Why Atlas wants it.** The business hub wants a client list; mail, vCards
(`vformat`), the calendar and the phone all mint contacts, and the same
person arrives four ways. `facts.rs` resolves entities by declared alias
("X is also known as Y"); nothing notices an *undeclared* duplicate. This
does, and says why — "same email; names 0.96 similar" — so merging stays
a yes/no question to the user rather than something Atlas does quietly.

The m numbers are **chosen, not measured**: splink estimates them by EM,
and with a few hundred contacts that estimate is noise. The u numbers are
**measured** when the list is big enough (`Model::measured_on`, 30+
contacts): u is how often a level happens between two *different*
people, and nearly every pair in a list is two different people, so the
level's frequency over all pairs estimates it well — splink's
`estimate_u_using_random_sampling`, done exhaustively because the list is
small.

**Public interface:**

- `fn jaro`
- `fn jaro_winkler`
- `fn norm_name`
- `fn norm_email`
- `fn norm_phone`
- `fn compare`
- `fn candidate_pairs`
- `fn duplicates`
- `struct Contact`
- `struct Level`
- `struct Model`
- `struct Match`
- `enum Verdict`

### `src/live.rs`

**Wired** — something outside this file calls into it.

The bar that has not closed yet.

## What is left of this module after the merge

Most of it went. It used to hold its own replay machinery — prime a window,
push bars through one at a time, and check that reading them that way gave
the same answer as reading them all at once. `market::bars::AsOf` does that
by construction: a view bounded at bar *n* cannot reach bar *n+1*, so there
is nothing left to check and nothing left to get wrong. A test that cannot
fail is worse than no test, so it went with the rest.

What survives is the one thing `Bars` genuinely does not model: **a candle
that is still forming.**

## Why that matters more than it sounds

Every closed-bar reader in existence is, at the moment it matters, looking
at a bar that has not finished. Price is through the level *right now*. The
break is on the screen. And somewhere between a third and half the time it
is not there when the bar closes.

A system that reads the forming bar as though it were settled will take
those. A system that ignores the forming bar entirely will be told about
every move one bar late, which on H4 is four hours. Neither is right, and
the difference between them is not a threshold — it is **saying which one
you are looking at**.

So `Now` carries both readings and names the gap between them. "The
structure has turned" and "the structure will have turned if this candle
closes here" are different sentences, and only one of them is a fact.

**Public interface:**

- `struct Forming`
- `struct Now`
- `struct Live`
- `enum Firmness`

### `src/localclock.rs`

**Wired** — something outside this file calls into it.

The time on this machine's own clock, for anything shown to you.

Atlas stores every moment as UTC seconds, which is right: a UTC second
means the same thing on the laptop, the phone and the server. But a time
*shown* to you has to be the one on your wall. The hub's command deck
(Eric's design, 23 Sep 2026) puts times on the day's timeline and against
everything Atlas did, and "14:02" read in UTC is four hours out for
someone in New York.

The offset is asked of the operating system rather than configured, the
same reasoning as `daemon::local_offset_mins`: a config field would be a
second declaration of a fact Windows already knows, and it would be wrong
twice a year. That function asks `date +%z`, which doesn't exist on
Windows; this one asks Windows directly.

**Public interface:**

- `fn offset_secs`
- `fn set_home_zone`
- `fn zone`
- `fn machine_offset_secs`
- `fn told_offset`
- `fn pin_offset`
- `fn hhmm`
- `fn hour`
- `fn day`
- `fn hour_here`
- `fn day_here`
- `fn midnight`
- `fn weekday`
- `fn spoken_now`

### `src/log.rs`

**Wired** — something outside this file calls into it.

Bounded local logging — a Milestone 1 acceptance criterion.

Rotates by size and keeps exactly one previous file. An assistant running
all day will otherwise quietly fill a disk.

**Public interface:**

- `struct Log`

### `src/loginseal.rs`

**Wired** — something outside this file calls into it.

Sealing a key to your Windows sign-in.

The vault opens with your passphrase or your recovery key. Neither works
for something scheduled at 6 a.m.: nobody is there to type. The usual
answers are to store the passphrase somewhere (a secret you then have to
keep safe, which is the design you've ruled out) or to leave the vault
open (worse).

Windows has a third: the Data Protection API seals bytes so that only the
same Windows account, signed in on the same machine, can unseal them. The
key to that is your Windows sign-in itself, which you already use every
day and which Windows lets you reset. So a copy of the vault's data key is
sealed that way (a `How::ThisLogin` wrap), and scheduled work can open
the vault while you're signed in — for the kinds of secret that
`vault::Kind::usable_unattended` allows, and never the recovery codes or
authenticator seeds.

Only on Windows. On Linux the equivalent is the desktop keyring (Secret
Service over D-Bus), which isn't wired, and this says so.

**Public interface:**

- `fn available`
- `fn seal`
- `fn unseal`
- `const NOT_HERE`

### `src/look.rs`

**Wired** — something outside this file calls into it.

How the panels look.

## The brief

These are read at a glance from a few feet away, over the top of whatever
you're doing, for a few seconds. That is a different problem from a settings
page, and it drives everything below: type large enough to take in without
leaning forward, almost no chrome, and one thing on screen that moves.

## Palette

Cool slate glass, warm off-white text, a single mint signal.

Deliberately *not* the two obvious routes. Iron Man cyan is the cliché the
moment you say Jarvis, and near-black with one acid accent is what every
dark interface does. Slate is bluer and softer than black, so it reads as
glass laid over the desktop rather than a hole punched in it, and mint sits
far enough from both cyan and orange to be its own thing.

## Type

One family, two cuts. Windows 11 ships Segoe UI Variable, whose Display cut
is drawn for large sizes and Text cut for small — which is exactly the
distinction these panels need. Nothing is fetched: a panel has to render
with the network unplugged, so a web font was never an option, and the
constraint turns out to pick a better face than a downloaded one would.

## The mark

The Folded A (27 Sep 2026), specified in `mark`: one strip of paper folded
once, with the dot that matters today. Its earlier forms, an arc bearing a
point and then a hanging hairline trace, are gone.

**Public interface:**

- `const TOKENS`
- `const TOKENS_DARK`
- `const TOKENS_ACCESS`

### `src/look_paint.rs`

**Wired** — something outside this file calls into it.

`look`'s design, painted natively.

`look` renders the panels as HTML/CSS/SVG. That is the wrong surface for a
window Atlas owns: an HTML panel needs either an external browser (a
dependency on something outside Atlas) or a bundled web engine, and the
rule for this system is in-house and self-contained first. So the design
`look` defines — the slate-glass palette, the catenary mark, the row
kinds — is reproduced here as values an egui painter can draw, with no new
dependency (eframe/egui is already compiled in) and nothing fetched.

**This module is the design made testable.** The colours and the mark
geometry are pure functions with no window attached, so a test can prove
the catenary math and the palette match `look` without a display — which
is the only way to verify faithfulness in a headless build. The actual
painting (which consumes these) lives with the window, since it needs a
live `egui::Ui`.

Every constant here is lifted from `look`'s own CSS, named to the line it
came from, so the two cannot drift silently: if `look::TOKENS` changes a
colour, the test that pins them together fails.

**Public interface:**

- `fn colourway`
- `fn visuals`
- `fn dress`
- `fn palette_and_os`
- `enum MarkState`

### `src/lookalike.rs`

**Wired** — something outside this file calls into it.

"This is from your installer — isn't it?" Senders that look like someone
you deal with and aren't, and mail whose own server says the sender
checks failed.

**Sources:** Unicode TR #39 (*Unicode Security Mechanisms*) §4, the
confusable *skeleton*: map every character to its prototype and compare
the results — a subset of `confusables.txt` (Unicode licence) covering the
Cyrillic and Greek letters and the ASCII look-alikes (`rn`→`m`, `vv`→`w`,
`0`→`o`, `1`/`I`→`l`) that phishing actually uses. `elceef/dnstwist`
(Apache-2.0) for the list of domain permutations worth checking: one letter
dropped, added, swapped or changed; a different ending; the real name with
a word bolted on; the real domain as a subdomain of someone else's. RFC
8601 for the `Authentication-Results` header (`spf=`, `dkim=`, `dmarc=`).
Clean-room.

**Why Atlas wants it.** The mail check already knows who your clients are
(`clients`) and drafts replies to them. That is exactly the list a
lookalike is built against: `acme-lnstall.com` replying about an invoice
would get a polite, helpful draft. Now it gets a warning instead, and no
draft.

**Public interface:**

- `fn sender_warning`

### `src/mail.rs`

**Wired** — something outside this file calls into it.

Getting at your email, whoever provides it.

Gmail, Outlook, Yahoo, Fastmail, a work server — all of them speak IMAP,
which means one implementation covers everything rather than four
integrations that each break separately.

Categorising happens **in your mailbox**, not in Atlas. A label applied
here shows up on your phone, and if you stop using Atlas tomorrow the
organisation stays. Anything that only exists inside this program is
organisation you lose.

Nothing here deletes. Moving to a folder is reversible; deleting isn't, and
a wrongly-categorised message you can find again is a nuisance where a
wrongly-deleted one is a problem.

**Public interface:**

- `fn categories`
- `fn action_for`
- `fn credential_source`
- `fn may_touch`
- `fn rehearsal`
- `fn trace`
- `fn what_the_trail_says`
- `fn category_of`
- `fn category_to_delete`
- `struct Account`
- `struct Where`
- `struct MailConfig`
- `struct Trail`
- `struct SortPlan`
- `enum Provider`
- `enum Action`

### `src/mailbook.rs`

**Wired** — something outside this file calls into it.

A small, local record of your recent mail -- sent and received -- so the
things that need "who said what, and when" have something to ask.

Mail used to be fetched, sorted and dropped: nothing remembered that you
asked Sam for the contract on Monday, so nothing could notice on Thursday
that Sam never answered. The waiting-for tracker (`waitingfor`), meeting
prep (`meetprep`) and your people (`people`) all read this.

**Kept small and plain on purpose.** Headers, the thread links (JWZ, see
`mailthread`), and the first `EXCERPT` characters of the body with any
secret-looking string scrubbed out (`redact`). Not attachments, not the
whole body. `keep_days` of it, and never more than `MAX_LETTERS`.
It stays on this machine: it isn't synced to the phone and isn't sent to
a model whole (the waiting-for rules read it locally).

**Public interface:**

- `fn mail_addresses`
- `fn excerpt`
- `struct Letter`
- `struct MailBook`
- `const EXCERPT`
- `const MAX_LETTERS`

### `src/mailthread.rs`

**Wired** — something outside this file calls into it.

Turn a pile of mail into conversations.

**Source:** Jamie Zawinski's message-threading algorithm (the one Netscape
Mail shipped, and the basis of the IMAP `THREAD=REFERENCES` extension,
draft-ietf-imapext-thread). `akuchling/jwzthreading` (BSD-3-Clause) and
`floatpane/jwz-go` (MIT) read as references. Clean-room.

**Why Atlas wants it.** `imap.rs` already fetches `In-Reply-To` and stops
there; nothing groups mail into conversations. The mail brief, "what did
Jordan and I decide about the installer", and the digest all want the
*thread* — and the naive grouping (by subject) merges every "Quick
question" ever sent, while the naive parent link (In-Reply-To only) breaks
the moment one message in the middle is missing. JWZ handles both: it
builds placeholder containers for messages it has only heard of through
`References`, so a thread with a gap stays one thread, and it only falls
back to subjects for roots, carefully.

It needs `References` too; `imap.rs`'s FETCH asks for IN-REPLY-TO but not
REFERENCES — adding it to that header list is part of the wire.

**Public interface:**

- `fn conversations`
- `fn parse_ids`
- `fn base_subject`
- `fn threads`
- `struct Mail`
- `struct Node`

### `src/mark.rs`

**Wired** — something outside this file calls into it.

Atlas's mark: the Folded A (Eric, 27 Sep 2026).

One strip of paper, folded once. It rises in ink, turns over at a flat
crease, and comes down showing its orange back, the way origami paper is
coloured on one side. Where it lies over itself the paper darkens: the
fold. The dot is the one thing that matters today, and the part that moves.

It replaced a dot over an arc (which read as Amazon and Headspace) and,
before that, the hanging hairline trace. Everything that draws the mark
draws it from here:
- the native painter (`window::paint_mark`: the setup window, the panels,
the morning brief and the idle overlay);
- the hub (`hub::MARK`);
- every generated icon: the web app's, the Windows program's, Android's
launcher and notification icons, and the iPhone and iPad app icon
(`design/mark/make_marks.py` writes them from these numbers, and
`tests/the_mark_is_one_mark.rs` holds every file to them).

**The geometry is paper's, not a drawing's.** The two legs are the same
strip, 21 units wide measured across the crease. The apex sits *on* the
crease, so the legs meet there and never cross, and the crease is a flat
edge rather than a point. The fold is exactly where the two legs overlap.
The numbers were computed from that rule, not placed by eye, and a test
checks the rule still holds.

**Sizes:** at 16 px and up the dot stays (Eric: "including the dot"). The
fold tone drops out below 32 px, where it would only muddy the crease.

**Motion** (`pose`), built from the 150/200/300 ms steps and moving only
position, size and opacity:
- Idle: dim, a very slow breath.
- Thinking: the dot rises toward the crease and settles.
- Speaking: the dot carries the voice (the real level when there is one),
and the folded leg answers it.
- Waking (the morning brief): the ink leg rises (300), the strip folds
over (300), the crease darkens (150), and the dot arrives (200).

**Public interface:**

- `fn shapes`
- `fn svg_for_test`
- `fn window_icon`
- `fn window_icon_for`
- `fn pose`
- `struct Colours`
- `struct Pose`
- `enum Motion`
- `const FRONT`
- `const BACK`
- `const FOLD`
- `const DOT`
- `const CREASE_Y`
- `const WARM_PAPER`
- `const EMBER_DARK`
- `const ACCESS`
- `const PAPER`
- `const EMBER`

### `src/marketdays.rs`

**Wired** — something outside this file calls into it.

The market's own calendar: when New York is shut, when it shuts early,
and which scheduled releases land on a day -- said as a schedule, never
as a view on what the market will do.

**Sources:** NYSE's published holidays and early closes for 2026–2028
(nyse.com/trade/hours-calendars, read 25 Sep 2026) are the check the rules
below were written against; the rules are the NYSE's own (Rule 7.2): New
Year's Day (a Saturday New Year is not moved back into the old year),
Martin Luther King Jr. Day, Washington's Birthday, Good Friday, Memorial
Day, Juneteenth, Independence Day, Labor Day, Thanksgiving and Christmas,
each moved to the Friday before when it falls on a Saturday and the Monday
after on a Sunday; 1:00 pm closes on the day after Thanksgiving, on
Christmas Eve and on July 3 when those are ordinary weekdays. Good Friday
is Easter less two days, and Easter is the Gregorian computus (the
anonymous "Meeus/Jones/Butcher" algorithm, public domain). BLS's CPI
release dates for 2026 (bls.gov/schedule/news_release/cpi.htm, as carried
by two independent listings that agree date for date) are a hand-entered
table with an expiry -- the same rule `market::events` keeps: a missing
date makes the calendar silent, a wrong one makes it confidently wrong.
Central banks and payrolls come from `market::events`, which already holds
them to that rule.

**What this is not.** It says when; it never says what to do about it. No
line here reads a market, and none is allowed to (`says_nothing_about_direction`).

**Public interface:**

- `fn holidays`
- `fn day`
- `fn market_marks`
- `fn marks_spoken`
- `fn today_and_tomorrow`
- `fn next_closure`
- `struct Mark`
- `enum Day`
- `const CHECKED_THROUGH`
- `const CPI_GOOD_UNTIL`

### `src/mcp.rs`

**Wired** — something outside this file calls into it.

Other programs' tools, offered to the model: Atlas as a Model Context
Protocol **client**.

**Source:** the Model Context Protocol (spec revision 2025-06-18,
`modelcontextprotocol/modelcontextprotocol`) and its official Rust SDK,
`modelcontextprotocol/rust-sdk` ("rmcp", Apache-2.0), read as the reference
for the message shapes: `initialize` → `notifications/initialized`,
`tools/list` with its `nextCursor` pages, `tools/call` with `content`
blocks and `isError`. rmcp itself is not linked: it is built on tokio, and
nothing else in Atlas is async — the whole runtime for three JSON-RPC
methods over a child's stdin and stdout. This is those three methods,
newline-delimited JSON over stdio as the spec's stdio transport says, with
a reader thread and a timeout on every request.

**What a server is for.** Somebody else's program that offers tools — the
reference Filesystem, Time and Fetch servers (`modelcontextprotocol/servers`),
Microsoft's Playwright browser (`microsoft/playwright-mcp`), Windows UI
automation (`mediar-ai/terminator`). Each is listed in `tools.yaml` under
`mcp.servers`; none ships turned on, because each needs Node or a download.

**What keeps it safe.**
* Started lazily, on its own thread, the first time a conversation needs
tools (`McpHub::wake`). The daemon's loop never waits on a server: a
server still starting simply offers nothing yet.
* Every call is asked about first (`ServerConfig::may_run_unasked`), unless
that server's entry says `ask_first: false` **and** the tool is on its
`allow` list.
* Nothing is offered while Atlas is handed over (`profiles::NEVER_AS_A_GUEST`
has `mcp_tool`, and the daemon offers no tools at all then).
* What a tool returns is somebody else's text: it is quoted
(`untrusted::Read::quoted`) for the one model call that phrases the answer,
and that call is given **no tools**, so nothing in a result can become an
action. A tool whose own *description* is written as orders to the model
is not offered at all (`clean_description`).
* Tools are found for a sentence by BM25 over name and description, at
most `MOST_PER_TURN` a turn, inside the same ceiling the commands use.

**Public interface:**

- `fn result_of`
- `fn tool_name`
- `fn clean_description`
- `fn merge`
- `fn payload`
- `fn read_payload`
- `fn plain`
- `fn answer`
- `fn connections_block`
- `struct McpConfig`
- `struct ServerConfig`
- `struct RemoteTool`
- `struct CallResult`
- `struct Client`
- `struct ServerView`
- `struct McpHub`
- `enum Standing`
- `const PROTOCOL_VERSION`
- `const PREFIX`
- `const MOST_PER_TURN`
- `const TOOLS_CEILING`
- `const RESULT_CHARS`
- `const RETRY_AFTER`
- `const SWITCHED_OFF`

### `src/meaning.rs`

**Wired** — something outside this file calls into it.

Turning text into a meaning vector, so `recall` can search by what a note
is *about* rather than only the words it happens to use.

`recall.rs` was built two-handed from the start — word search that needs no
model, and a meaning path behind `RecallConfig::semantic` that merges a
cosine score into the ranking. The meaning half had everything except a
source of vectors: `Piece.embedding` was only ever `None`,
`set_embedding`/`unembedded` sat on the dead-methods list, and
`semantic: false` shipped with a comment saying "turn on once an embedding
model is installed". This module is the missing source, and it is
deliberately the smaller half — the same shape as `speaker.rs`, for the
same reason: the encoder itself is an external program, like speech-to-text
and text-to-speech, so a model is swapped by editing the config, never by
recompiling. `fit.rs` already names the model this machine should run
(`all-MiniLM-L6-v2`, ~90MB); what was missing was the seam.

Two jobs live here:

* **The seam.** Run the configured encoder over a piece of text and read
the numbers back. Text goes in on stdin (and as a `{text}` var, for an
encoder that takes it as an argument instead); parsing reuses
`speaker::parse_embedding`, because "prints numbers, somehow" is the same
loose contract voice encoders have and requiring one exact format would
mean the first encoder you try fails for a reason that has nothing to do
with meaning search.

* **The memory.** `Daemon::reload_library` rebuilds the library from the
notes folder on every load, with every `embedding: None`. Without a
remembered copy, each restart would re-run the encoder over every note —
slow, and pointless for notes that have not changed. `Remembered` keeps
vectors keyed by a stable hash of the note's content, so an unchanged
note is rehydrated for free and an edited one (new content, new key)
is re-embedded exactly as it should be. Keys use FNV-1a written out
here rather than `DefaultHasher`, because `DefaultHasher` is seeded per
process and a key that changes every run is a cache that never hits.

**Public interface:**

- `fn embed`
- `fn available`
- `fn key`
- `fn fingerprint`
- `struct MeaningConfig`
- `struct Remembered`
- `const NO_ENCODER`

### `src/measure.rs`

**Wired** — something outside this file calls into it.

Measuring a real file, so that grading it has something true to grade.

`grade.rs` has known how to judge a clip since it was written: give it an
`Audio` and a `Picture` and it tells you what a viewer will notice, in the
order they will notice it. Nothing ever filled those two structs in, which
is why `grade` sat unreachable — the judgement existed and the
measurements did not.

Everything here is measured by ffmpeg and parsed, never estimated. Where a
field cannot honestly be measured with what is on this machine it is left
at its neutral value and `unmeasured()` names it, so that a clean report
means "nothing found" rather than "nothing looked at":

- **`skin_kelvin`** needs to find a face before it can read its colour
temperature. Atlas has no face detection on this path, so this is `None`
and `check_picture` skips white balance entirely.
- **`saturation`** is defined as a multiple of untouched, and a file
carries no record of what it looked like untouched. Measuring mean
saturation and dividing by a number somebody picked would produce a
confident figure with nothing behind it.

## The two booleans

`rumble` and `harsh_s` are not numbers in `grade::Audio`, they are yes/no.
They are answered here by comparing the energy in one band against the
energy in the whole signal — both measured, the threshold between them
chosen and named as a constant rather than buried in an `if`.

**Public interface:**

- `fn loudness_from_loudnorm`
- `fn noise_floor_from_astats`
- `fn mean_volume_db`
- `fn frame_shape_from_probe`
- `fn parse_rational`
- `fn grey_stats`
- `fn has_rumble`
- `fn has_harsh_s`
- `fn unmeasured`
- `fn audio_of`
- `fn picture_of`
- `const RUMBLE_WITHIN_DB`
- `const SIBILANCE_WITHIN_DB`
- `const LOW_BAND`
- `const SIBILANCE_BAND`

### `src/meetprep.rs`

**Wired** — something outside this file calls into it.

A short brief before a meeting: who's in it, what you last wrote to each
other, what you noted about them, and what's still open between you.

Built from what's already on this machine -- the mail cache (`mailbook`),
your captured notes (`capture`) and the waiting-for list (`waitingfor`) --
so it costs nothing to keep up and nothing leaves the machine. Said once,
a quarter of an hour before, at a natural break (the same deferral every
offer goes through), and on request: "prep me for my next meeting".

**Who's in it.** Calendar events here don't carry an attendee list (the
`.ics` import keeps summary, place and notes), so the people are read
from the title and notes: "call with Sam", "Sam / Priya sync", an email
address in the notes. A name the mail cache has never seen is still
listed, with "no mail with them in the last N days" -- an honest blank
rather than a guess.

**Public interface:**

- `fn people_in`
- `fn prepare`
- `fn said`

### `src/memory.rs`

**Wired** — something outside this file calls into it.

Memory: five separate stores, per the spec. Not one generic bucket.

The separation earns its keep because the stores have different lifetimes
and different consequences. A wrong preference is annoying; a wrong
approval-history entry makes Atlas act without asking. Keeping them apart
means one can be cleared without touching the others.

**Public interface:**

- `struct WorkflowMemo`
- `struct Project`
- `struct StyleMemo`
- `struct ApprovalRecord`
- `struct Memory`

### `src/mend.rs`

**Wired** — something outside this file calls into it.

Fixing what's broken without papering over it.

`handoff` asks a person for help when Atlas is stuck. `plainly` says what
is wrong in your words. This is the layer underneath both: **deciding
whether a proposed fix is a fix at all.**

Every failing check has a cheap way out. A borrow error goes away if you
clone. A type error goes away if you widen the type to something that
accepts anything. A failing test goes away if you delete the test. Each of
those turns the build green, and each leaves the thing that was actually
wrong exactly where it was, now with nothing pointing at it.

That is worse than the original bug. The bug was visible; the paper-over is
a bug with a green tick on it. And an assistant that is graded on the build
passing will find the cheap way out every time, because it is faster and it
works.

So this refuses them by name, and it does that rather than trusting good
intentions, because the pressure to take the cheap route is strongest at
3am on the eighth attempt.

## The second half: asking someone who doesn't read code

Some failures cannot be fixed without a decision, and those decisions are
almost never technical. "Should a missing file be an error or an empty
list" is a question about what the thing should do, and you can answer it
without reading a line.

The rule this enforces: **a question put to you must be answerable without
reading code.** No identifiers, no file paths, no error codes, no jargon.
If Atlas cannot phrase it that way, it has not understood the problem well
enough to be asking yet.

**Public interface:**

- `fn paper_overs`
- `fn refusal`
- `fn should_ask`
- `fn about_approval`
- `struct Proposed`
- `struct Question`
- `enum Kind`
- `enum Cheat`

### `src/mesh.rs`

**Wired** — something outside this file calls into it.

Reaching the laptop when you're on cell service.

The cloud folder works but it's a relay: something is written, then read
later. For a conversation you want the two devices talking directly, and
for that they need to be able to find each other across networks.

A private network does exactly this. Tailscale is free for personal use and
covers a phone, a laptop and a tablet several times over; Headscale is the
same thing with the coordination part run by you, which is only worth it if
you already have a server, and you don't.

It's an addition, not a replacement — everything still works without it.

**Public interface:**

- `fn what_it_adds`
- `fn works_without`
- `fn on_this_network`
- `fn choose`
- `fn setup_steps`
- `fn what_a_private_network_would_give_you`
- `struct MeshConfig`
- `enum Mesh`
- `enum Path`
- `const YOU_APPROVE_THE_DEVICE`
- `const NOT_BUILT_HERE`
- `const WHAT_ID_DO`

### `src/meshio.rs`

**Wired** — something outside this file calls into it.

3-D models from files: OBJ (with its MTL colours), STL, and glTF 2.0
(`.gltf` with its buffers, or a single `.glb`) — read in house, and made
fast to hit with a bounding volume hierarchy.

**Sources:**
- Wavefront OBJ and MTL (Library of Congress format descriptions; Paul
Bourke's copy of the Wavefront spec): `v`, `vn`, `f` with `v/vt/vn` and
negative indices, polygons fanned into triangles, `mtllib`/`usemtl`/`Kd`.
- STL: 3D Systems' format — binary (80-byte header, count, 50 bytes a
facet) or ASCII (`facet … vertex`).
- glTF 2.0 (Khronos): scenes → nodes (matrix or translation/rotation/scale,
children) → meshes → primitives (mode 4, triangles) → accessors →
buffer views → buffers (a file, a base64 `data:` URI, or the GLB's BIN
chunk); `pbrMetallicRoughness.baseColorFactor` for colour.
- Möller & Trumbore (1997) for ray–triangle intersection; a BVH split by
the surface area heuristic over 12 bins (Wald 2007; *PBRT* 4ed §7.3).

Nothing here draws: `scene3d` places a [`Mesh`] like any other shape.

**Public interface:**

- `fn read_obj`
- `fn read_stl`
- `fn read_gltf`
- `fn read_model`
- `fn cached`
- `struct Tri`
- `struct Mesh`
- `struct MeshHit`

### `src/messaging.rs`

**Wired** — something outside this file calls into it.

The places people actually message you.

Email is where formal things arrive and chat is where everything else
does — which means an assistant that only reads email misses the message
from the brand and catches the newsletter.

The three platforms differ enormously in what they permit, and it's worth
being blunt about that rather than promising the same for all three.

**Public interface:**

- `fn what_you_asked_for`
- `fn sort`
- `fn interrupts`
- `fn folder_for`
- `fn note_on`
- `fn filed`
- `fn spoken`
- `struct Message`
- `struct Person`
- `struct MessagingConfig`
- `enum Platform`
- `enum Reach`
- `enum Sort`
- `enum Folder`
- `const PEOPLE`
- `const WHY_NOT_WHATSAPP`

### `src/metrics.rs`

**Wired** — something outside this file calls into it.

Counting Atlas, so no document has to.

Ten documents in `docs/` each state a test count. They were all correct on
the day they were written and none of them are correct now. The fix is not
discipline, it is removing the opportunity: numbers live here, prose links
to them, and `atlas metrics` regenerates the block.

Deliberately dependency-free and deliberately approximate about lines of
code — the point is drift detection, not accountancy.

**Public interface:**

- `fn gather`
- `struct Metrics`

### `src/mfcc.rs`

**Wired** — something outside this file calls into it.

Mel-frequency cepstral coefficients: the numbers speech tools compare.

The standard front end (Davis & Mermelstein 1980; the HTK recipe that
every speaker and wake-word system since has used): pre-emphasis, 25 ms
Hamming frames every 10 ms, a power spectrum, 40 triangular filters spaced
on the mel scale, their log, and a DCT-II that keeps the first 20
coefficients. c0 (overall loudness) is dropped from what gets compared,
because how loud you are says nothing about who you are.

Written here rather than taken from a crate so the speaker check
(`speaker`), the wake word (`wakeword`) and room calibration all share one
front end that nothing outside the tree can change underneath them.

**Public interface:**

- `fn frames`
- `fn voiced`
- `fn normalised`
- `struct Frame`
- `const CEPS`

### `src/micthread.rs`

**Wired** — something outside this file calls into it.

The microphone on its own thread.

**Why.** With the wake word on, the run loop used to record a three-second
clip (and transcribe it, or match it against your taught phrase) on every
pass. The loop is also what answers the hub, the typing box and the icon
by the clock, so for those three seconds everything else waited: Atlas
running in the background with no window open was, most of the time, a
hub that didn't answer (Eric, 28 Sep 2026).

Now the recording happens here. The loop polls a channel that never blocks
(`MicThread::poll`); when the wake word is heard this thread also records
what you say next and sends both at once, so the loop only stops for the
turn itself.

**Cutting in by voice** (`barge_in` in settings, off by default). While
Atlas is speaking a reply, this thread can watch the microphone for your
voice: sustained speech (300 ms by default) stops the playback and what
you say becomes the next turn. Speech is told from noise by Silero VAD
(MIT, Silero Team) when its model file is in the models folder, run by
`tract` — the ONNX engine already built into Atlas, pure Rust, so nothing
native is needed on Windows — and otherwise by Atlas's own detector
(`vad`), which learns the room. A missing or unreadable model is never an
error: it is said once in the log and the in-house detector is used.

**Echo.** Without echo cancellation the microphone also hears Atlas
itself through the speakers, and Atlas's voice *is* speech. What is done
about it: the level a window must reach is measured against a floor that
follows everything that isn't you — Atlas's own voice through the
speakers included — and is raised further while the reply is loud
(`speaking`'s envelope says how loud each moment of the reply is). That
helps; it does not replace a headset. With speakers, expect Atlas to cut
itself off now and then, which is why this is off until you turn it on.

**Pause.** Pausing Atlas stops this thread listening: no wake word, no
watching for your voice, no recording. It picks up again on resume.
Holding the talk key still works while paused — that is you deliberately
pressing a key, and it is one way to say "carry on".

**Public interface:**

- `fn cut_playback`
- `fn playback_cut`
- `fn clear_cut`
- `fn detector_for`
- `struct BargeInConfig`
- `struct RoomVad`
- `struct SileroVad`
- `struct BargeGate`
- `struct Learned`
- `struct MicThread`
- `struct MicLink`
- `enum Heard`
- `enum CutIn`
- `const SILERO_FILE`
- `const WINDOW`
- `const MAX_LAG`

### `src/mind.rs`

**Wired** — something outside this file calls into it.

What Atlas is doing right now, and why.

Two things this makes possible. You can ask what it's working on and get a
real answer rather than "working on it". And you can watch the reasoning as
it happens, which is the difference between a system you trust and one you
hope about.

It's also the thing that lets you talk to Atlas mid-task without stopping
the task. Background work has its own thread of thought; a question from
you is a separate one, and neither disturbs the other unless you say so.

**Public interface:**

- `fn speak_brief`
- `struct Thought`
- `struct Work`
- `struct Step`
- `struct Mind`
- `struct Item`
- `enum Stage`
- `enum Started`
- `enum Weight`

### `src/mobile.rs`

**Wired** — something outside this file calls into it.

Atlas on a phone, standing alone.

Eric's ruling: Atlas is a standalone on phones as well — not a window onto
the laptop. So the phone runs the same core (`--no-default-features`: no
desktop GUI stack, `platform::mobile`), and the phone app's screens are the
hub this core serves to the app's own WebView over loopback. The design's
phone artboards are those same pages at phone width (`hub::STYLE`'s phone
rules: the tab bar, one column, safe areas).

This module is the one door the native shells (`mobile/ios`,
`mobile/android`) call through, as a C ABI they link against:

- `atlas_mobile_start(home, port)` (`start_once`) starts Atlas in its own thread with its
data under `home` (the app's private folder), serving the hub on
127.0.0.1 only. It never blocks for long: 0 if the hub is answering,
2 if it's still starting (poll `atlas_mobile_url`), 1 if it was
already running. Only one Atlas is ever started: a second call while
the first is starting says "starting" rather than starting another.
- `atlas_mobile_url(buf, len)` writes the hub's address, token included,
for the WebView to open; -1 until it's answering.
- `atlas_mobile_state()` says where it is: 0 not started, 1 starting,
2 running, -1 it couldn't start.
- `atlas_mobile_stop()` asks it to stop; the thread finishes its turn.

Nothing here opens a port anyone else can reach: loopback, and the same
token the laptop's hub uses.

**Public interface:**

- `fn phase`
- `fn starts`
- `fn start_once`
- `fn stop_now`
- `fn serve`
- `enum Phase`
- `enum Started`
- `const START_WAIT_MS`

### `src/models.rs`

**Wired** — something outside this file calls into it.

Model registry and engine — the rest of what Ollama does, in Rust.

Ollama's job is: find the models on disk, read their metadata, work out
what fits in memory, format the prompt the way each model expects, and run
a llama.cpp process to do the actual inference. None of that requires Go or
Ollama. This module does it directly, so Atlas talks to `llama-server`
itself and there is one fewer piece of someone else's software in the path.

**Public interface:**

- `fn estimate_memory`
- `fn server_args`
- `fn draft_path`
- `fn speculation_args`
- `fn gpu_layers_for`
- `fn listen_host`
- `fn health_url`
- `fn completion_url`
- `fn completion_body`
- `fn launch`
- `fn server_tool`
- `fn budget_bytes`
- `fn pick`
- `fn layers_here`
- `fn is_graphics_build`
- `fn footprint_mb`
- `fn server_post`
- `fn self_built_endpoint`
- `fn server_get`
- `fn is_running`
- `fn llm_config_for`
- `fn connection`
- `fn chat_url_beside`
- `fn on_tailscale`
- `fn unreachable_words`
- `fn chat_available`
- `fn tools_late_template`
- `fn judge_chat_failure`
- `fn chat_call`
- `struct Model`
- `struct ModelsConfig`
- `struct Registry`
- `struct WaitsForServer`
- `struct ChatStream`
- `enum Template`
- `enum ChatFail`
- `const NGRAM_KINDS`
- `const DRAFT_MAX`
- `const DRAFT_MIN`
- `const CACHE_IDLE_SLOTS_ENV`
- `const LOADING_SECS`
- `const ALL_LAYERS`
- `const CHAT_RETRY_SECS`
- `const STABLE_TOOLS_KWARG`

### `src/modes.rs`

**Wired** — something outside this file calls into it.

Named workspace modes.

"Trading mode." "Writing mode." "Call mode." Each is a set of apps, a
layout, a notification policy, and a lighting state. The layout engine
already supported all of this; nothing had names.

The part that earns its keep is the notification policy. Half the value of
saying "call mode" is that Atlas then shuts up.

**Public interface:**

- `fn suggested`
- `fn sentences_for`
- `struct Mode`
- `struct Modes`
- `struct Transition`
- `enum Interruptions`

### `src/money.rs`

**Wired** — something outside this file calls into it.

Everyday money, not just trading.

Trading has its own module because its rules are strange — wash sales,
Section 1256, mark-to-market. This is the other 95% of your financial life,
which has no strange rules and is still where most of the money goes.

The point isn't budgeting. Budgets are a plan you fail at in month two.
This is about **knowing where it actually went**, which is a different and
much more answerable question.

**Public interface:**

- `fn remember_month`
- `fn unusual_buckets`
- `fn sort_one`
- `fn summarise`
- `fn new_or_grown`
- `fn buckets_that_jumped`
- `fn spoken`
- `fn work_spend`
- `struct Entry`
- `struct KeptMonth`
- `struct Month`
- `struct MoneyConfig`
- `enum Bucket`
- `const THIS_MONTH`
- `const LAST_MONTH`
- `const MONTHS`
- `const MONTHS_KEPT`
- `const MONTHS_FLOOR`
- `const NOT_ADVICE`

### `src/motion.rs`

**Wired** — something outside this file calls into it.

In-house animation, the part of it that can be checked.

Same honesty as `craft` and `taste`, one domain over. There is no offline
tool that says "this animation looks good" — so this does not pretend to
judge that. What it checks is the part a machine can: that what came back is
a real, self-contained SVG animation (it will render, and something in it
actually moves), and that it matches the numbers you asked for — the size,
and roughly the duration. Everything past that — is the motion nice, does it
feel right — stays with you, through the preview, or with a stronger model.

Why SVG first, and why it needs no extra toolchain: an SVG animation is
plain text that every browser renders natively, with the motion declared in
the file (SMIL `<animate>` elements, or CSS `@keyframes` in an inline
`<style>`). So "does it render and move" is answerable by reading the file,
offline, with no headless browser and nothing installed — exactly the
property that made the `craft` ladder worth having. Heavier media (a Manim
or Blender render to frames) are the next medium; they need their renderer
present and run, and belong behind the same "does it render + match the
spec" gate this establishes.

**Public interface:**

- `fn check`
- `fn blocking`
- `fn spoken`
- `fn draw_loop`
- `fn declared_size`
- `fn refine`
- `fn verify_render`
- `fn render`
- `struct MotionSpec`
- `struct Finding`
- `struct Refined`
- `struct Expect`
- `enum Severity`
- `enum Outcome`
- `enum RenderKind`
- `const MOTION_SYSTEM`
- `const MOTION_FIX_SYSTEM`

### `src/msoauth.rs`

**Wired** — something outside this file calls into it.

Microsoft's OAuth2 device code flow — the one real way into Outlook
and Microsoft 365 mail, now that password-based IMAP and SMTP are
gone entirely.

One thing this module cannot do for you: get a client ID. That means
registering an app in Azure AD yourself — Microsoft's own portal,
your own Microsoft account, a few clicks. No code here can do that
part; it's the one manual step everything else is built to need only
once. See `SETUP` below for exactly what to do.

Everything past that is real: request a device code, show you the
short code and the URL, poll until you've approved it somewhere else
(your phone, another tab — never a browser Atlas drives itself, which
is the whole point of this flow existing), then hold the refresh
token in the vault and mint a fresh access token before every
connection rather than caching one that might have gone stale.

Uses `curl` for the HTTP, the same as the unsubscribe one-click POST —
this is exactly curl's strong protocol, unlike the IMAP support that
ruled it out for the mail client itself.

**Public interface:**

- `fn request_device_code`
- `fn poll_once`
- `fn refresh`
- `fn xoauth2_string`
- `struct DeviceCode`
- `struct Tokens`
- `enum PollOutcome`
- `const SETUP`

### `src/nearby.rs`

**Wired** — something outside this file calls into it.

Finding your other Atlas on the same network, without typing an address.

# What was missing

Two Atlases can already talk. `kin.rs` is the door one knocks on,
`elsewhere.rs` is the asking, `server.rs` answers, and every request
carries a token. All of it works and none of it needs a third party.

What it needs is this, in `config/tools.yaml`:

```yaml
elsewhere:
known:
- name: homelab
host: "10.0.0.9"
```

A hand-typed address, on a home network that hands out a different one
after a reboot. So the one transport Atlas has that reaches another
machine directly was gated behind a setting that goes stale on its own,
and `mesh.rs` carried four routes -- `SameNetwork`, `Mesh`, `Cable`,
`Cloud` -- of which only `Cloud` was ever reachable, because `Cloud` is a
folder and needs no address at all.

This is the missing half of `Path::SameNetwork`: a shout on the local
network and whatever answers.

# What this is not

**Discovery is not trust, and nothing here grants access.** Finding a
machine tells you its name and where to reach it, and that is all it may
ever do. The token still comes from the other Atlas -- `atlas hub` prints
it there -- and is still required on every request. An announcement
carries no secret, so anything listening on the network learns only that a
machine calling itself "homelab" has a door open, which it could learn
by connecting to the port anyway.

Answering is **off by default**, and that is deliberate rather than
cautious-by-habit. The network you are on is not always your own: a café,
an office, a hotel. Announcing your machine's name to it is a small thing
that you should choose rather than inherit. Looking is harmless and is on.

# Why UDP and nothing else

A broadcast to the local network is the only way to ask "is anyone there"
without already knowing where there is, which is the entire problem. mDNS
would be the conventional answer and is a large specification, a
dependency, and a second name-resolution system to be wrong in; this is
forty lines of `std::net` with no third party at all, and it answers the
one question being asked.

Broadcast does not cross a router, which is the property that makes it
safe: "the same network" is exactly the set of machines it can reach.

**Public interface:**

- `fn answer_for`
- `fn answer_from`
- `fn look`
- `fn answer_probes`
- `fn spoken`
- `struct Found`
- `struct NearbyConfig`
- `const PORT`
- `const ASKING`

### `src/next_up.rs`

**Wired** — something outside this file calls into it.

What comes first, when several things are waiting to be said.

The shape is wshobson's recommender pipeline, sized for Atlas: take the
candidates, drop what doesn't belong, score the rest, pick the top few,
and count what was passed over so it's said as "and N more" rather than
silently dropped. One place, so the order of what you hear is decided by a
rule you can read rather than by whichever thing happened to be written
down first.

Its first job: coming back to your desk (`returning::welcome`). That
named the *oldest* two things needing you, in the order they happened —
so a failed backup from lunchtime was said before the question that's
been waiting on you for five minutes and still is.

**Public interface:**

- `fn top`
- `struct Picked`

### `src/notify.rs`

**Wired** — something outside this file calls into it.

Getting something to you when you are not at the desk.

Three modules already sit next to this one and none of them do this job.
`interrupt` decides **whether** anything is worth saying. `presence` knows
whether you are there. `channel` splits disposable progress from a result
that has to stand alone. What was missing is the step between "this is
worth saying" and "you actually heard it": the **route**.

Until now everything Atlas wanted to tell you either went to the speakers
or went to the log. Speakers reach an empty room; the log reaches nobody.
A disk filling up while you are away is exactly the case where the log is
the wrong answer and the speaker is too.

## The rules this enforces

**Nothing is reported as delivered unless it was.** The failure this
codebase keeps producing is a component that returns success because
nothing objected. A notifier that is not installed must say so, not
quietly succeed. `Sent::Failed` exists so a caller cannot mistake one for
the other, and `Delivery::sent()` is deliberately not a bool.

**Something that could not reach you is held, not dropped.** Held items
are delivered when you come back. An alert that evaporates because you
were out is worse than no alerting, because you will believe you were told.

**Private things wait for a private moment.** `presence` already knows when
somebody else is at your desk. A notification is visible on a screen other
people can see, so discretion has to be checked here rather than assumed
upstream.

## Why an external command rather than a library

Atlas runs on Windows, macOS and Linux, and each has its own notification
mechanism with no common Rust crate worth the dependency. The established
pattern in this codebase for "the OS can already do this" is an
`ExternalTool` configured per platform in `tools.yaml` — the same way
research reaches curl and the browser. Nothing new to install, and the
command is visible and editable rather than compiled in.

**Public interface:**

- `fn route`
- `fn spoken`
- `fn show`
- `fn can_notify`
- `fn how_to_say`
- `fn on_a_call`
- `struct Note`
- `struct NotifyConfig`
- `struct Outbox`
- `enum Urgency`
- `enum Route`
- `enum Sent`
- `enum Say`
- `enum Quiet`
- `const NO_NOTIFIER`
- `const CALL_APPS`

### `src/notifyicon.rs`

**Wired** — something outside this file calls into it.

Atlas's icon by the clock (the notification area), owned by the
background Atlas on Windows.

Eric, 28 Sep 2026: *"I don't want a command terminal to be open. When on
windows I don't even want to have the hub or the application open for
Atlas to run."* With no window open, the icon is how you know Atlas is
there, and the one place to reach it from: open it, open the hub in your
browser, pause it, or quit it.

The menu and what each entry does are plain functions (`tray_menu`,
`tray_choice`, `tray_tooltip`) so they are tested anywhere. The Win32 part
— `Shell_NotifyIconW`, a hidden window on its own thread to receive the
icon's clicks, `TrackPopupMenu` — is Windows-only and has only been
cross-compiled here, never clicked.

The icon's thread never touches the `Daemon`. Pausing and resuming are
*asked for* through a small queue the run loop empties every pass
(`tray_asks`), and go the same way as the hub's Pause button (`turn("pause")`);
the run loop tells the icon whether it is paused (`tray_paused_now`).
Quitting is `goodbye::please_stop`, the same in-process door Ctrl-C uses,
so Atlas saves and lets go of its lock on the way out.

**Public interface:**

- `fn tray_menu`
- `fn tray_choice`
- `fn tray_tooltip`
- `fn tray_tip`
- `fn tray_hub_address`
- `fn tray_hub_now`
- `fn tray_hub_note`
- `fn take_icon_away`
- `fn bring_icon_back`
- `fn icon_taken_away_count_for_test`
- `fn tray_ask`
- `fn tray_asks`
- `fn tray_paused_now`
- `fn show_icon`
- `struct DesktopConfig`
- `struct TrayIcon`
- `enum TrayAction`
- `const ID_OPEN`
- `const ID_BROWSER`
- `const ID_PAUSE`
- `const ID_QUIT`
- `const RUNNING_ENTRY`
- `const PAUSED_ENTRY`

### `src/nudge.rs`

**Wired** — something outside this file calls into it.

Pushing you toward progress.

`proactive` answers "may I speak?". This answers "is there anything worth
saying?" — and unlike every other detector in the system, these are not
triggered by something you did. Nothing here is a reaction. That is the
whole point, and it is also the whole danger.

The failure mode is not that Atlas says something useless once. It is that
it says something useless four times, you mute it, and then it is worth
less than nothing because you have stopped listening to the one channel
that would have told you something real. So every nudge here is built
around three rules:

1. **A nudge that cannot offer to take work is a nag.** Every nudge carries
a `relief` — the specific thing Atlas will do instead of you. "You've
slowed down" is a nag. "You've slowed down, I can take the invoice
chase off you" is help.
2. **Silence backs off, it does not repeat.** Ignoring a nudge widens the
interval. It never shortens it.
3. **Asking why is a one-shot.** After enough silence Atlas may ask, once,
what it got wrong — because the answer is worth more than the nudge was.
It may never ask twice about the same subject. Asking repeatedly why you
are ignoring something is the purest form of nagging there is.

**Public interface:**

- `fn is_medical`
- `fn goal_words`
- `fn which_goal`
- `fn daypart_with_brief`
- `fn convene`
- `fn convene_with`
- `fn offer_to_mend`
- `fn what_i_know_of`
- `fn drifted`
- `fn trace_line`
- `fn link_broke`
- `struct Goal`
- `struct Nudge`
- `struct NudgeConfig`
- `struct Nudger`
- `enum Trigger`
- `enum Part`
- `enum Response`
- `const NEVER_NUDGES_ABOUT`
- `const GOALS`

### `src/ocr.rs`

**Wired** — something outside this file calls into it.

Reading text off the screen and off paper — the outside program.

Where this sits among the other options, cheapest first: the accessibility
tree (exact, instant, needs a cooperating app), then reading the pixels
(exact enough, a second or two, works on anything visible), then a vision
model (understands layout and pictures, and on this hardware genuinely
slow).

## This is no longer the way Atlas reads a screen

`words` is. It runs two model files inside `atlas.exe` — no second
program, no install, nothing to go missing. This module drives
`tesseract.exe`, which has **never run**, because nothing in `ATLAS.bat`
ever downloaded it: the capability page said *"waiting on tesseract"* from
the day it was written until the day `words` replaced it.

It is kept rather than deleted for one reason: Tesseract reads capitals and
punctuation, and the English recogniser in `words` reads thirty-six
lower-case characters and nothing else. If Eric ever installs Tesseract by
hand, this is a better reader for a scanned page than `words` is. It is not
the default and nothing reaches for it on its own.

See `words::NO_CAPITALS` for the limit that makes this worth keeping.

**Public interface:**

- `fn parse_tsv`
- `fn args`
- `fn read_image`
- `fn tidy`
- `struct OcrConfig`
- `struct Word`
- `struct Reading`
- `const NOT_THE_DEFAULT`

### `src/onion.rs`

**Wired** — something outside this file calls into it.

How one person's Atlas reaches another's: through Tor, with nothing in the
middle that anybody runs or pays for.

Eric's rules (25 Sep): Tailscale may join *your own* devices; it never
joins two people's. No server in the middle, nothing to pay for, and no
friend's Atlas holding anyone's messages. So each desktop Atlas is a Tor
**onion service**: it has a permanent address made from its own key, and it
reaches -- and is reached -- only by connecting *out* into the Tor network,
which joins the two ends inside itself. That works from behind any router
and any provider, needs no door opened anywhere, and nobody along the way
learns who is talking to whom. The same way Briar and Ricochet work.

What Atlas does here:
* makes the onion address from this Atlas's key (`Identity::derive`, so the
onion key is its own secret, not the signing key reused), and writes it
where `tor` expects it -- nothing to set up;
* starts `tor` itself, shipped beside Atlas, and knows when it's ready;
* connects to a friend's onion address through it (SOCKS5, written here).

What travels through it is still sealed by Atlas (`wire`), so the door
checks who sent what exactly as before.

Also here: the few facts about *your own* networks the door needs -- which
addresses count as this machine, home, or your own private network.

**Public interface:**

- `fn sha3_256`
- `fn is_onion`
- `fn my_address`
- `fn find_tor`
- `fn torrc`
- `fn pid_file`
- `fn stop_orphan`
- `fn connect`
- `fn bridge_lines`
- `fn next_bridge_kind`
- `fn is_stalled`
- `fn is_local_origin`
- `fn read_addr`
- `fn lan_v4`
- `struct Tor`
- `const ONION_PORT`
- `const CONNECT_SECS`
- `const BRIDGE_KINDS`
- `const STALL_SECS`

### `src/online.rs`

**Wired** — something outside this file calls into it.

Delegating work to online sub-agents, when the machine is online.

Atlas is offline-first: the crew runs errands on this machine, on this
machine's model, and that never stops being true. This is the *other*
half — when there is a connection, heavy background work can be handed out
to a faster worker instead of grinding on the local 3B, and the result
pulled back, checked for accuracy here, and either handed to you or used
to finish the task without interrupting whatever Atlas is already doing.

The first provider is Cloudflare's free tier: Workers AI for the model
call, and (later) a Worker for whole compute jobs and Browser Rendering
for fetching pages server-side. Nothing here is Cloudflare-specific in the
code, though. A provider is reached through the same `LlmConfig`/`curl`
path the local model uses (`brain.rs`), so the endpoint, the model name
and the bearer token all live in `config/tools.yaml` and the vault — this
module is the provider-agnostic delegation logic that sits on top: is a
provider ready, send the task, get the result, and **check it before
trusting it**.

## Offline is not a downgrade, and online is not a default

When there is no connection, or no provider is configured, the local crew
does the work exactly as before — this module reports `Blocked` and the
caller falls through. When a provider *is* ready, delegation is still the
caller's choice per task, not a global switch: the point is to make the
machine more capable when it can be, never to move your work off it
without asking.

## The check is the point

A result from somewhere else is a claim until Atlas has looked at it. Every
delegated result is graded here — for how well-grounded it is
(`certainty`), and optionally by a second, local pass that reads the answer
back and says whether it holds. A result that fails the check is handed
over *with the doubt attached*, never as a clean fact. That is what makes
"have it analysed in the background for accuracy" true rather than a slogan.

**Public interface:**

- `fn readiness`
- `fn dispatch_task`
- `fn verify_result`
- `struct CloudflareConfig`
- `struct Delegated`
- `enum Readiness`
- `const DELEGATE_SYSTEM`
- `const VERIFY_SYSTEM`

### `src/onlyone.rs`

**Wired** — something outside this file calls into it.

Only one Atlas at a time.

Nothing stopped two from running. Start it from the shortcut, forget, start
it again from the batch file, and there are two — both reading and writing
`data/state`, both rewriting the trash ledger, both holding the microphone.

The damage is quiet, which is what makes it worth guarding rather than
documenting. Each instance reads a file, changes it in memory, and writes
the whole thing back. Neither is corrupt. The second write simply erases
whatever the first learned, and nothing anywhere reports a problem. You
notice weeks later that something you told it didn't stick.

The server port is the only accidental protection today — the second
instance fails to bind — and it is partial: the second daemon carries on
doing everything else, minus a web interface it never needed to work.

## Why a heartbeat rather than a process id

The obvious lock holds the pid and checks whether that process is alive.
Doing that properly needs different system calls on Windows and Linux, and
pids get reused, so a stale lock can point at something unrelated that has
since started.

A file whose modification time is refreshed while running avoids all of it.
Fresh means someone is there. Old means whoever held it is gone — crashed,
killed, or power-cut — and nothing needs to ask the operating system
anything.

The cost is that recovering from a crash takes as long as the staleness
window. That is the right trade: refusing to start for a couple of minutes
after a crash is a small annoyance, and starting a second instance
alongside a live one costs you state.

**Public interface:**

- `struct OnlyOne`
- `struct Watching`
- `enum Found`
- `const BEAT_EVERY_SECS`
- `const GONE_AFTER_SECS`
- `const CLAIM_STALE_SECS`
- `const WOKE_GRACE_SECS`

### `src/opportunity.rs`

**Wired** — something outside this file calls into it.

Whether an opportunity is worth your time.

`decide` works a choice you are already facing. This is for the other
shape: something crossed your feed and looks like it might be worth doing.

Five axes, and the reason there are five is that any one of them alone is
the way people talk themselves into things. Money alone gets you a plan
that pays well and that you will abandon in March. Fit alone gets you the
thing you would enjoy that nobody will pay for. The point of the frame is
that a weak axis is visible instead of getting averaged away.

**Nothing is scored on a hunch.** Every axis carries what it rests on, and
an axis with nothing behind it counts as unknown rather than as neutral.
Neutral is the more dangerous default: it lets an opportunity nobody has
examined score the same as one that was examined and came out middling.

Money is the axis that cannot be answered honestly yet, and it says so
rather than guessing. Until Atlas can read the actual accounts, a figure
here would be a number with the authority of a measurement and the content
of a wish.

**Public interface:**

- `fn money_is_not_auditable_yet`
- `struct Look`
- `struct Opportunity`
- `enum Axis`
- `enum Finding`
- `enum Verdict`
- `const FATAL_BELOW`

### `src/opsec.rs`

**Wired** — something outside this file calls into it.

Checking content before it goes out, while the rules apply to you.

Two things this has to get right. The checks have to be about **what's
visible in the frame**, not about what you're allowed to think — Atlas is
looking for a patch and a tail number, not vetting your opinions. And it
has to know there is a date after which none of this is its business.

That second part matters more than it sounds. A system that keeps applying
rules you're no longer under is a system you start ignoring, and then it's
useless on the day it's right.

**Public interface:**

- `fn check`
- `fn spoken`
- `fn frame_unchecked`
- `struct Spotted`
- `struct OpsecConfig`
- `enum Risk`
- `const NOT_ITS_BUSINESS`

### `src/orders.rs`

**Wired** — something outside this file calls into it.

What you ordered, and where it is — built from confirmation, shipping,
and delivery emails, so "what's going on with my Amazon order" has a
real answer instead of needing you to go dig through your inbox.

Status is read off a closed set of phrases a retailer's own email
already uses ("has shipped", "out for delivery", "was delivered"),
deliberately not left to a model to interpret — the same reasoning
`unsub.rs` and `triage.rs` already apply: a bounded set of real
signals is worth more than a plausible-sounding guess, especially for
something as easy to get definitively right as "which of five known
phrases appears in this subject line."

**Public interface:**

- `fn status_from_subject`
- `fn merchant_from_address`
- `struct Order`
- `struct Orders`
- `enum Status`

### `src/oslook.rs`

**Wired** — something outside this file calls into it.

What this computer's own settings ask for: light or dark, high contrast,
bigger text, less motion.

The hub's pages read these through CSS (`prefers-color-scheme`,
`forced-colors`, `prefers-reduced-motion`, the browser's text size). This
module is the same thing for Atlas's own native windows, so "Follow this
computer" means the same in both places and the windows honour the user's
platform settings (EN 301 549 11.7, WCAG 1.4.4 / 1.4.11 / 2.3.3).

On Windows it asks Windows directly, in-house, through the calls Windows
itself documents — no crate:

- light or dark: `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
`AppsUseLightTheme` (0 means dark);
- high contrast: `SystemParametersInfoW(SPI_GETHIGHCONTRAST)`, and then the
user's own contrast colours from `GetSysColor`;
- text size: `HKCU\Software\Microsoft\Accessibility` `TextScaleFactor`
(Settings → Accessibility → Text size, 100–225);
- animation: `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`.

Elsewhere it reports nothing and the defaults stand.

**Public interface:**

- `fn text_scale_from_percent`
- `fn colorref_to_rgb`
- `fn read`
- `struct Contrast`
- `struct OsLook`

### `src/ota.rs`

**Wired** — something outside this file calls into it.

Putting the iPhone app on an iPhone with no Mac: the install page.

Apple lets an ad hoc build install "over the air": Safari opens an
`itms-services://?action=download-manifest&url=…` link, the phone fetches a
small XML manifest, and the manifest points at the `.ipa`. Both URLs must be
HTTPS with a certificate the phone already trusts, and the phone must be
one of the devices in the build's provisioning profile.

The update-courier spec (§6) says the person's own Atlas serves this, over
their own tailnet, with Tailscale's HTTPS certificate (a real, Safari-trusted
one). So this module:

* reads the `.ipa` itself: the app's `Info.plist` (binary plist) and the
embedded provisioning profile, for the bundle ID, the version, which
devices it installs on and when it expires. Nothing is typed by hand,
so the manifest can't disagree with the app;
* writes the manifest and the page;
* serves them, with the `.ipa`, on 127.0.0.1 only, under a random path,
for a limited time. Tailscale's `serve` (your own devices) or `funnel`
(a friend's phone, for those minutes only) carries it to the phone with
HTTPS; Atlas never listens beyond this machine.

**Sources:** Apple's *Distribute proprietary in-house apps* guide (the
manifest's `items` → `assets` (`software-package` url) and `metadata`
(`bundle-identifier`, `bundle-version`, `kind` = `software`, `title`)
keys and the `itms-services` link); Apple's binary property list format
(`CFBinaryPList.c`: the `bplist00` header, the 32-byte trailer, the
offset table and the object markers); Tailscale's `serve` / `funnel` docs.
Clean-room; no plist crate.

**Public interface:**

- `fn bplist`
- `fn xml_plist`
- `fn profile_facts`
- `fn android_page`
- `fn manifest_plist`
- `fn install_link`
- `fn page`
- `fn route_install`
- `fn fresh_token`
- `fn now_iso`
- `fn serve_install`
- `fn tailscale_args`
- `fn page_url`
- `struct Ipa`
- `struct Apk`
- `enum Value`
- `enum Package`
- `enum Served`
- `const HTTPS_PORT`

### `src/otherside.rs`

**Wired** — something outside this file calls into it.

Arguing the other side.

You've decided something. Asking Atlas whether it's a good idea gets you
agreement, because you've framed it as a good idea — that's how questions
work, and it's why "what do you think?" is nearly useless once you've
already made your mind up.

This is the deliberate version: **make the strongest case against**, on
request. Not to change your mind, and not as a general habit — asked for,
and then done properly.

The failure mode to avoid is a system that lists generic risks. "There may
be unforeseen costs" is true of everything and helps with nothing. What's
useful is the argument someone who disagreed with you would actually make.

**Public interface:**

- `fn against`
- `fn spoken`
- `fn written`
- `fn is_asked_for`
- `fn decision_from`
- `fn not_raised`
- `fn argued`
- `struct Objection`
- `struct Decision`
- `enum Angle`

### `src/outbox.rs`

**Wired** — something outside this file calls into it.

Client and brand replies Atlas has drafted, waiting either for you to
look at them or for standing approval to let them go on their own.

The drafting itself needs no approval — Eric's own rule. What this
module holds is the gap between "drafted" and "sent": without
`may_email_clients` or `may_email_brands` on, a reply sits here until
you ask to see it; with it on, the same reply is sent and reported,
and this is where the record of that lives too.

**Public interface:**

- `struct PendingReply`
- `struct Outbox`
- `enum Kind`
- `enum Status`

### `src/outreach.rs`

**Wired** — something outside this file calls into it.

Who Atlas may cold-email, one name at a time.

Deliberately not the same list as `clients.rs`. A client is an
ongoing relationship — replying to one is expected. Cold outreach is
Atlas emailing someone who has never heard from it, on your behalf,
unprompted from their side. Eric's own rule: `may_email_brands`
turns the capability on at all, but that alone is not enough — each
recipient still needs its own yes. A master switch says "this
category of sending is allowed to exist"; this list says "this
specific person may actually receive one."

Atlas may still *draft* outreach to anyone, unapproved included —
drafting needs no approval, the same rule as everywhere else in this
feature. What this list gates is only the send.

**Public interface:**

- `struct Approved`
- `struct OutreachTargets`

### `src/overlay.rs`

**Wired** — something outside this file calls into it.

Drawing on the desktop itself.

Panels in a browser window can't do what you asked for. A browser always
paints a background, so "transparent" isn't available at any setting — the
best it manages is a dark rectangle. For text that appears *over* your
desktop with nothing behind it, the window has to be a layered window with
a real alpha channel, drawn by Atlas.

So this is a native overlay: no browser, no URL, no chrome, no background.
Just letters and a mark appearing over whatever you were looking at, and
then gone.

## Reading over anything

The problem with transparent text is that your desktop might be a white
document, and white text on white is nothing. Two things fix it, and both
are cheap: every glyph gets a soft dark halo, and behind the text sits a
wide, very faint gradient that darkens the area without reading as a box.
Together they hold contrast over a photograph, a spreadsheet or a terminal.

## Typing

Text arrives a character at a time. It reads as something being said rather
than something being displayed, and — more usefully — the movement draws
your eye to it, so a line that appears while you're looking elsewhere still
gets noticed.

**Public interface:**

- `fn around`
- `fn window_style`
- `struct OverlayConfig`
- `struct Typing`
- `struct Overlay`
- `enum Element`
- `enum MarkState`
- `enum Align`
- `enum Phase`
- `const WS_EX_LAYERED`
- `const WS_EX_TRANSPARENT`
- `const WS_EX_TOOLWINDOW`
- `const WS_EX_TOPMOST`
- `const WS_EX_NOACTIVATE`

### `src/overlaywin.rs`

**Wired** — something outside this file calls into it.

Atlas's words on the desktop itself, while it speaks.

`overlay` has held the design since the start — the mark arriving, the
words typing themselves in over whatever you're looking at, a faint shade
so they read over a white page, then gone — and nothing drew it (doc 19,
still open in doc 21). Eric, 24 Sep 2026, item 4: the desktop overlay.

This draws it: a window with no frame, no background and no taskbar
button, above everything, that every click passes straight through. It is
its own small process (`atlas overlay`), started by the background Atlas
on Windows, so a stall in either can't freeze the other. It follows
`speaking`: when Atlas starts saying something, the mark arrives, the
words type in and the line moves with the voice; when it has finished and
the words have been read, it fades and the desktop is untouched again.

It goes away by itself when the background Atlas does, and only one runs
at a time.

**Public interface:**

- `fn atlas_is_up`
- `fn run`
- `struct Stage`
- `struct Folders`

### `src/overnight.rs`

**Wired** — something outside this file calls into it.

Working while you're asleep.

You go to bed, Atlas works through the things it couldn't finish, and in
the morning there's a brief and a set of changes waiting for you to accept
or throw away.

The important design choice: **this doesn't care where the answers come
from.** A session works through problems using whatever brain it's been
given — the local model, a hosted one, or a queue of briefs for a person to
answer over coffee. Swapping that later changes one line of config and
nothing else.

Two rules make unattended work safe rather than alarming:

1. **Nothing is applied while you're asleep.** Everything lands in the
sandbox with its tests run. You accept in the morning.
2. **It stops when it stops being useful** — a budget, a time limit, and a
rule that repeated failure on the same problem ends that problem rather
than burning the night on it.

**Public interface:**

- `fn morning_brief`
- `fn morning_detail`
- `fn note_for`
- `fn worth_doing_overnight`
- `struct Result_`
- `struct OvernightConfig`
- `struct Session`
- `enum Brain`
- `enum Outcome`
- `enum Step`

### `src/palette.rs`

**Wired** — something outside this file calls into it.

Everything you can reach, by typing.

Menus stop scaling somewhere around six items. Atlas has thirteen pages and
a growing number of things you can actually *do* — arrange the dashboard,
track an account, lock the vault — and none of those actions were reachable
at all except by first navigating to the page that happens to hold the
button.

## What makes this different from a search box

Three rules, all of which are the difference between a palette people use
and one they try twice:

**Results are things you can do, not places to go.** "Track a new account"
is actionable; "Accounts" is navigation wearing an action's clothes. Both
are here, but the doing ones rank above the going ones.

**It is useful before you type.** An empty palette shows what you reached
recently and the handful of things most people want, because the moment you
open it is exactly the moment you have not yet decided what to call the
thing you want.

**It does not replace the menu.** The sidebar stays. A palette is how
someone who knows the product moves; the menu is how anyone learns what is
in it, and a product that only has the former cannot be learned.

## Why the matching is here rather than in the browser

So it can be tested, and so the same ranking answers the typed palette and
the plain `/hub/find` page. A second implementation in script would be a
second set of rules that quietly disagrees with the first.

**Public interface:**

- `fn catalogue`
- `fn find`
- `fn did_you_mean`
- `struct Entry`
- `struct Recent`
- `enum Does`
- `const SHOW`
- `const FILE`
- `const KEEP`

### `src/panel.rs`

**Wired** — something outside this file calls into it.

Free-standing panels.

Not a hub. These appear when asked for, say their piece, and leave. They
are furniture on your desk rather than an application you visit, and that
shapes every decision here: big type because they are glanced at from a few
feet away, almost no controls because they are driven by voice, and a
lifetime measured in seconds.

Placement follows from the same idea. Anything you need to read while
working goes on a second screen if you have one, because covering the thing
you are working on to tell you about it is self-defeating. With one screen
Atlas asks first, and takes no for an answer by simply speaking instead.

**Public interface:**

- `fn faded`
- `fn place`
- `fn place_anyway`
- `fn place_on_second`
- `fn window_args`
- `fn narration`
- `struct Placement`
- `struct PanelConfig`
- `enum Panel`
- `enum Decision`

### `src/pdfkit.rs`

**Wired** — something outside this file calls into it.

PDF pages, moved about in house: merge files, split one into pages, take
a range out, stamp a signature image onto a page -- without a PDF
library.

**Sources:** ISO 32000-1:2008 (PDF 1.7) -- §7.3 objects, §7.5 file
structure (including §7.5.7 object streams), §7.4.4 FlateDecode and its
PNG predictors (§7.4.4.4, from RFC 2083), §7.7.3 the page tree and its
inheritable attributes, §8.9.5 image XObjects and soft masks. `lopdf`
(MIT) was read as a reference for the shape of a writer that renumbers
objects; nothing is copied. Inflate is the tree's own (`zipread`).

**How it reads.** Not by trusting the cross-reference table: many PDFs
in the wild have one that's slightly wrong, and a reader that trusts it
fails on them. It walks the file once, object by object (`N G obj …
endobj`), skipping stream bytes by their length, so a later definition of
an object replaces an earlier one exactly as an incremental update means
it to; objects packed inside object streams are unpacked the same way.
Password-protected files are refused, never half-read.

**How it writes.** Only the objects the chosen pages reach, renumbered
from 1, streams copied byte for byte (no re-encoding, so nothing is lost
and nothing is slow), a fresh page tree with the attributes each page
inherited written onto the page itself, and a plain cross-reference
table. Every file it writes is read back and its pages counted before
it's reported done (`check_written`). It never writes over the file it
read.

**Public interface:**

- `fn dict_get`
- `fn write`
- `fn check_written`
- `fn stamp`
- `fn page_size`
- `fn ranges`
- `struct Doc`
- `struct PageRef`
- `struct Overlay`
- `enum Obj`
- `const MAX_BYTES`
- `const MAX_OBJECTS`

### `src/pdftext.rs`

**Wired** — something outside this file calls into it.

Reading the words out of a PDF, in Atlas's own code (Eric's ruling H3,
25 Sep 2026: "read this PDF" is a basic ask).

Windows has no PDF-to-text program, and asking you to install one to read
a letter is the wrong way round. A PDF is objects and streams: the pages
say which fonts and which content streams they use, the content streams
draw strings with `Tj`/`TJ`, and a font's `ToUnicode` map says which letter
each code is. That is enough for what Word, Google Docs, browsers and
scanners' "searchable PDF" produce, which is what arrives in mail.

What it does not do: encrypted PDFs (said, not guessed), and fonts with no
`ToUnicode` map and a custom encoding (the text comes out as whatever
Latin-1 makes of the codes, and `looks_like_words` catches that).

A scanned PDF is photos of pages with little or no text. Those photos are
nearly always JPEGs, stored as they are, so `images` hands them back
untouched for the word reader to read (`files::pdf_is_really_a_scan`
decides when).

**Public interface:**

- `fn read`
- `fn looks_like_words`
- `struct Pdf`

### `src/peerkey.rs`

**Wired** — something outside this file calls into it.

This Atlas's own identity: a signing key that says "this came from me"
in a way anyone can check and nobody else can forge.

Pairing gives each pair of Atlases a shared token, and a token proves who
is knocking *on your own door*. It proves nothing about a message that
reached you through somebody else. Group chats need exactly that: a group's
membership is decided by whoever created it, and every member has to be
able to trust a membership list they heard from another member, not only
one the creator handed them directly. So each Atlas gets an ed25519 key of
its own (the same algorithm the release signature uses), introduces its
public half to each paired Atlas over the authenticated pairing channel
(`/hello`), and signs what it alone may decide.

The private half lives with the pairings, in this install's state folder --
the same protection class as the pairing tokens, which already let anyone
holding them speak as you to your peers. It never leaves this machine.

**Public interface:**

- `fn exchange_point`
- `fn verify_delegation`
- `fn fingerprint`
- `fn is_public_key`
- `fn verify`
- `struct Identity`
- `struct Delegation`
- `const DELEGATION_DOMAIN`

### `src/people.rs`

**Wired** — something outside this file calls into it.

The people you deal with, and what you'd want to remember about them:
a small personal CRM, kept on this machine.

"Remember Sam's daughter is called Leo." "Keep in touch with Priya every
month." "Who haven't I talked to in a while?" "What do I know about Sam?"

**Sources:** Monica (AGPL; read for its ideas only) for what a personal
CRM holds -- notes, how you met, a keep-in-touch cadence, birthdays -- and
for the lesson that the entry form is where these tools die. So nothing
here asks you to fill anything in: a person comes into being the first
time you name them, and "last talked" comes from the mail cache
(`mailbook`) rather than from you logging calls.

**Soundproofing.**
- A note that holds a secret-looking string (a key, a card number) is
refused, not kept -- the vault is for that.
- Health words (`nudge::NEVER_NUDGES_ABOUT`) can be kept as a note if you
say them, but never come back unasked: "who's due" and the brief read
only names and dates.
- Bounded: 2,000 people, 100 notes each, 500 characters a note.
- A name that could be two people ("Sam" when you know Sam Lee and Sam
Ortiz) is asked about, never guessed.

**Public interface:**

- `fn read`
- `fn due_said`
- `struct Contact`
- `struct People`
- `enum Found`
- `enum Refused`
- `enum Asked`
- `const MAX_PEOPLE`
- `const MAX_NOTES`
- `const MAX_NOTE_CHARS`

### `src/perf.rs`

**Wired** — something outside this file calls into it.

Staying out of the way.

An always-on assistant that costs you 5% CPU forever is a tax you pay every
second of every day. The design rule here is that Atlas does nothing on a
schedule that it could do on an event, and when nothing is happening it
backs off geometrically until it is checking in about once a minute.

**Public interface:**

- `struct PerfConfig`
- `struct Power`
- `struct Throttle`

### `src/person.rs`

**Wired** — something outside this file calls into it.

Learning you.

Not a profile you fill in — those go stale and nobody updates them. This is
built from what actually happens: when you work, what you come back to,
how you phrase things, what you always change about Atlas's output, what
you keep and what you throw away.

## What this deliberately isn't

It doesn't diagnose you and it doesn't infer things about your inner life.
It notices patterns in what you *do* — which is both more useful and more
honest than a system guessing at how you feel from your word choice. The
difference matters: "you've been at this since six and it's now eleven" is
an observation you can check. "You seem stressed" is a guess dressed as
insight, and a wrong one is worse than silence.

**Public interface:**

- `fn project_named`
- `fn learn_from_edit`
- `fn learn_from_refusal`
- `fn beyond_me`
- `fn having_a_hard_time`
- `fn hard_day`
- `fn noticing`
- `struct Trait_`
- `struct Person`
- `struct PersonConfig`
- `struct Said`
- `enum Kind`
- `enum Noticed`
- `const NOT_A_THERAPIST`
- `const SAID_RECORD`

### `src/persona.rs`

**Wired** — something outside this file calls into it.

Atlas's voice.

Not the text-to-speech voice — the character. This is the thing that makes
an assistant feel like *something* rather than a menu, and it is almost
entirely policy rather than technology.

The rules come from what actually makes Jarvis work on screen: it is brief,
it is dry, it never flatters, it states limits plainly, and it disagrees
when it has reason to. An assistant that opens every reply with "Great
question!" is doing the opposite of all five.

**Public interface:**

- `fn strip_list_marker`
- `fn strip_filler`
- `fn trim_to_sentences`
- `fn social_reply`
- `struct Persona`
- `enum Tone`
- `const FILLER_OPENERS`
- `const FILLER_PHRASES`

### `src/phases.rs`

**Wired** — something outside this file calls into it.

Long work in phases, each written down as it finishes, so a restart picks
up after the last finished phase instead of starting over.

From wshobson/agents' `comprehensive-review/full-review`: each phase
writes its result to a file and a state file says how far the work got.
Rewritten small for Atlas: a folder per piece of work under the state
folder, one JSON file per finished phase, keyed by what the work *is*
(your words and where it applies) so asking again — or `resume` redoing it
after a restart — finds the phases already done.

What it doesn't do: a phase that was halfway through when Atlas stopped
is run again from its start. Only a finished phase is kept.

**Public interface:**

- `struct Phases`

### `src/phone.rs`

**Wired** — something outside this file calls into it.

Reaching you when the laptop isn't with you.

The last gap in the delivery chain. Atlas speaks when you are there, draws
its own window when you are not, and holds anything it cannot get to you —
but if you have walked out of the house, holding means you hear nothing
until you come back. For "your disk is full" that is fine. For "the trade
you were watching just moved" it is not.

## How it reaches the phone

A single HTTP POST to a push endpoint you control, which the phone is
already subscribed to. No app to build, no account, nothing running on the
phone that Atlas has to maintain.

**ntfy's publish format.** The message goes to the server's root as JSON
with the topic inside it (`{"topic": "atlas", "title": …}` — docs.ntfy.sh,
"Publish as JSON"). The first version posted that JSON to `/atlas`, which
ntfy treats as a plain-text message: the phone would have shown the raw
JSON. A token, when set, goes in `Authorization: Bearer`, where ntfy reads
it; before round 5 the field was read by nothing.

**Plain HTTP or TLS.** `http://box:80` or `box:80` — a server on your own
network over Tailscale, which encrypts the hop itself — goes as plain
HTTP. `https://…` goes over TLS with the system's certificate checks,
which is how a public push service works. Either way this is the online,
secondary path; nothing depends on it.

## What goes in the message

Titles, never contents. A phone notification lands on a lock screen that
anybody can see, and Atlas has no way to know who is looking — the same
problem the desk window has, with less control over it. So the phone gets
the knock and the detail waits until you open Atlas.

**Public interface:**

- `fn configured`
- `fn body_for`
- `fn send`
- `struct PhoneConfig`
- `enum NotSet`
- `const NO_PHONE`

### `src/phoneadd.rs`

**Wired** — something outside this file calls into it.

Adding a phone or iPad from the computer (D5, Eric's ruling, 27 Sep 2026).

"Friends receive a copy of Atlas on their computer first, then do their
setup, and they can get their phone access by selecting their device type
and it sends them the link." And: nobody goes into files or a command
prompt. So the hub's **Your phone** page does all of it from buttons:

- **Android:** the app is on the computer (`apps/Atlas.apk`, taken in
from Downloads by `app_file`). One press puts its install page on the
phone's reach and shows a code to scan. The phone asks once whether to
allow installs from there; that's Android's own switch.
- **iPhone and iPad:** Apple installs an app outside the App Store only on
devices listed in the build, by their UDID, and a UDID isn't something
anyone should have to look up. Apple's own way to read it is a "profile
service": the phone opens a small profile, Settings asks to install it,
and the phone then sends its UDID back to the address inside it. This
module makes that profile and reads the reply. The UDID then goes to
whoever sends Atlas out (Eric), whose Updates page lists the phones
waiting for the next iPhone build. Once a build that lists the phone has
arrived, the same page shows its install code.

Both go out over the computer's own Tailscale name with HTTPS
(`tailscale serve`, the install page's port 8443, `ota`), because iOS
installs only over HTTPS with a certificate it trusts, and nothing here
listens beyond 127.0.0.1. Everything lives under a random path, for
fifteen minutes.

**Sources:** Apple's *Over-the-Air Profile Delivery and Configuration*
(the Profile Service payload: `URL`, `DeviceAttributes`, `Challenge`; the
device POSTs a CMS-signed plist with `UDID`, `PRODUCT`, `VERSION`,
`DEVICE_NAME`; the server answers with a 301 to where Safari goes next),
and the Configuration Profile Reference (the top-level payload keys).

**Public interface:**

- `fn keep`
- `fn looks_like_udid`
- `fn device_from_reply`
- `fn route_enrol`
- `fn serve_enrol`
- `fn send_to_releaser`
- `fn heard`
- `fn reach_out`
- `fn stop_reaching_out`
- `fn app_file`
- `fn devices_card`
- `struct Device`
- `struct Showing`
- `enum Kind`
- `enum Enrol`
- `const MINE`
- `const WAITING`
- `const WIRE_PREFIX`
- `const MINUTES`
- `const APPS_DIR`

### `src/phonelink.rs`

**Wired** — something outside this file calls into it.

Putting Atlas on your phone, with nothing to type.

The ruling (23 Sep): the phone reaches the laptop over Tailscale. The hub
does not move — it stays on `127.0.0.1`, where [`crate::server`] has always
kept it. What changes is that Tailscale's own `tailscale serve` publishes
that loopback address to the tailnet, over HTTPS, under the laptop's
tailnet name:

```text
tailscale serve --bg --https=443 http://127.0.0.1:8787
-> https://laptop.tail1234.ts.net/
```

Nothing outside the tailnet can reach it, Tailscale supplies the
certificate, and Atlas never opens a port of its own. The phone then gets
one link — `https://<name>/hub?t=<token>` — shown as a QR code on the hub,
and the first visit trades the token for a cookie exactly as the laptop's
own browser does.

## How it is split

Everything that decides something is a pure function over text:
[`read_status`], [`serve_outcome`], [`serving`], [`phone_url`], [`say`],
[`qr_modules`], [`qr_svg`]. The two functions that run a process —
[`publish`] and [`unpublish`] — only glue those together, so every
judgement here is testable on a machine without Tailscale.

## What the tailnet needs, once

MagicDNS on, and "HTTPS Certificates" enabled, both on the DNS page of the
Tailscale admin console. Without HTTPS, `tailscale serve` refuses (older
versions) or prints a link and waits for you to enable it (newer ones).
Both come back as [`Serve::NeedsHttps`], and [`say`] tells Eric exactly
where the switch is.

**Public interface:**

- `fn tailscale_tool`
- `fn read_status`
- `fn serving`
- `fn phone_url`
- `fn serve_outcome`
- `fn publish`
- `fn unpublish`
- `fn say`
- `fn qr_modules`
- `fn qr_svg`
- `struct Tailnet`
- `enum Serve`
- `const WINDOWS_CLI`
- `const MAC_CLI`
- `const TIMEOUT_SECS`
- `const QUIET`
- `const LINK_KEY`

### `src/phonemodel.rs`

**Wired** — something outside this file calls into it.

The phone's own language model (OPEN_GAPS P.7, D6).

On the laptop Atlas runs `llama-server` as a separate program. A phone
can't: iOS lets an app start no other program at all, and Android offers
none to start. So on the phone the model runs *inside* Atlas -- llama.cpp
linked in as a library (`llama-cpp-2`), Metal on iPhone and iPad, the CPU
on Android -- behind the same `brain::Llm` every other model is behind
(`PhoneLlm`). The prompt is wrapped from the file's own chat template, as
on the laptop (`models::Template`). A connection you've set yourself
(`tools.llm`, say your laptop's model over Tailscale) stays as the
fallback, answered in-process (`tools::curl_in_process`).

Which model: the laptop's 4B is too big for a phone's memory. Two pinned
files, chosen by how much memory the phone has (`choose`):
- Qwen3 1.7B (Q8_0, 1.8 GB) on phones with 8 GB or more;
- Qwen3 0.6B (Q8_0, 0.64 GB) on the rest.

Both are Qwen's own GGUF files, pinned by SHA-256 (the Hugging Face LFS
object id, checked by downloading both on 27 Sep 2026). The phone fetches
its model itself (`fetch`), only when asked -- 0.6 to 1.8 GB is not
something to start on a phone's data plan unasked.

The engine is compiled only with the `phone-llm` feature, which the phone
builds turn on; everything else here (the choice, the download, the pins)
is ordinary code and tested on any machine.

**Public interface:**

- `fn present`
- `fn https_range`
- `fn cut_at_stop`
- `fn no_thinking`
- `fn without_thinking`
- `fn add_bos_for`
- `fn download_state`
- `fn fetch_by_itself`
- `fn download_said`
- `fn start_download`
- `struct PhoneModel`
- `struct Asked`
- `struct Download`
- `const MODELS`

### `src/picture_talk.rs`

**Wired** — something outside this file calls into it.

Asking about a picture: a chart, a screenshot, a photo.

`vision` finds faces and names things from a fixed list, and `words`
reads the text on a screen. Neither can say what a chart *shows* — that
sales rose from 120 to 190 over six months and dipped in March — and
`vision`'s own header said why: "a model that talks about pictures is
several gigabytes and a different decision." Eric, 24 Sep 2026, item 5:
real local vision, charts included, with the laptop's memory checked
first.

The laptop has 15.7 GB (Core Ultra 7 256V, Arc 140V sharing it). The model
chosen is Qwen3-VL 4B Instruct at 4-bit, 2.5 GB, plus its 0.45 GB picture
encoder — about 3 GB while it runs and nothing when it doesn't, because it
is run once per question by llama.cpp's own `llama-mtmd-cli` rather than
kept loaded. Both files are Qwen's own, pinned by SHA-256 in `atlas get`.
Nothing leaves the machine: the picture is a file in Atlas's scratch
folder and the program is on this laptop.

Before this, "look at my screen" reached the running Atlas and was
answered "Capture runs through the voice layer." — a sentence about the
code, spoken to the person, and nothing looked at the screen at all.

**Public interface:**

- `fn where_they_are`
- `fn ready`
- `fn question_for`
- `fn command_line`
- `fn the_answer_in`
- `fn ask_until`
- `fn smaller`
- `struct PictureTalkConfig`
- `const MEMORY_MB`
- `const ASK_TIMEOUT_SECS`
- `const MAX_WIDTH`

### `src/pipeline.rs`

**Wired** — something outside this file calls into it.

Thought, build, review, refine, implement.

The old loop went straight from a goal to an attempt, which is why it would
have rotted. Two things go wrong when you skip the thinking, and both are
invisible while the tests are green:

1. You fix the symptom. The test passes, the cause is still there, and it
comes back somewhere else in three weeks.
2. You write a test that would have passed anyway. It proves nothing and
it will never fail again, so it's worse than no test — it's a false
reassurance you'll trust later.

The rule that catches both: **before writing any code, say what would prove
it fixed, and check that the proof fails right now.** A proving test that
passes before the change tests something else.

Every stage produces an artefact. You cannot enter a stage without the
previous one's artefact, and nothing lands without all five.

**Public interface:**

- `fn review`
- `fn refinement_is_warranted`
- `struct Thought`
- `struct Diagnosing`
- `struct Build`
- `struct Review`
- `struct Note`
- `struct Refinement`
- `struct Work`
- `struct PipelineConfig`
- `enum Stage`
- `enum Concern`
- `enum Next`
- `const WHY_THE_STAGES`

### `src/plainchange.rs`

**Wired** — something outside this file calls into it.

Explaining a change without showing you code.

Atlas working on itself is worth nothing if the only way to check it is to
read a diff. You need to know **what will be different**, not what lines
moved.

There is a good hook for this in how the tests are written. Every test in
this project is named as a sentence about behaviour —
`a_dangling_word_means_you_are_not_finished` — so the tests that were added
or removed describe the change better than the code does. That's what this
reads.

**Public interface:**

- `fn sentence_from_test`
- `fn area_of`
- `fn diff_of`
- `fn explain`
- `fn spoken`
- `fn written`
- `fn ask`
- `struct Effect`
- `struct Diff`

### `src/plainly.rs`

**Wired** — something outside this file calls into it.

Saying what's wrong in your own words.

The last version needed you to speak in LUFS and Kelvin, which is
backwards: you can hear that something's wrong, and knowing the word for it
is Atlas's job, not yours.

So "I'm not loud enough" becomes a measurement, a check, and a fix. And
when what you said could mean two different things — "the music's too
loud" might be the music or might be your voice being too quiet — Atlas
measures rather than guessing, because those need opposite fixes.

**Public interface:**

- `fn understand`
- `fn confirm`
- `fn result`
- `fn didnt_understand`
- `struct Reading`

### `src/plugins.rs`

**Wired** — something outside this file calls into it.

Add-ons: capabilities you or a friend add to Atlas, kept through every
update, and unable to do more than you allowed.

This is Tier 1 of the plugin boundary in `docs/UPDATE_COURIER_SPEC.md` §5:
**declarative, no code.** An add-on is one file, `data/plugins/<id>/plugin.yaml`,
that gives Atlas new things to do *by composing what it already does* -- a
named sequence of ordinary commands with the phrases that start it. The
engine interprets it; nothing in it ever executes as code. So the largest
class of hole a plugin system has (running someone else's program) does
not exist here.

What stops an add-on doing more than it should, in the order it is met:

1. **It declares what it needs** (`permissions`), in plain categories you
can read. Every step is checked against that list when the file is
read; a step that needs something undeclared makes the add-on refuse to
load, rather than load and fail later.
2. **You approve it** -- on the hub or with `atlas plugins approve` -- and
the approval records the exact file (its SHA-256) and the permissions
you granted. Nothing that arrives from anywhere is ever approved for
you; there is no command an add-on (or a message) can send that
approves an add-on.
3. **Every step is checked again as it runs**: the file must still be the
one you approved, the add-on must not be switched off, and the command
the step turned out to be -- decided by the same parser as anything you
say -- must fall in a permission you still grant. This is what makes
taking a permission away immediate, and what catches a step whose text
was changed by an earlier step's output (`{name}`).
4. **It then goes through the ordinary approval gate** (`policy`), like
anything you say. An add-on cannot make a consequential step skip the
question by being part of a chain.

Some commands are never available to an add-on at all (`NEVER`): the vault,
pairing, accounts, handing Atlas over, Atlas changing its own code or its
own standing instructions. And add-ons cannot call each other, or your own
saved sequences: a step is parsed as a command, never matched as a
trigger.

**Public interface:**

- `fn plugins_dir`
- `fn permission`
- `fn read_schedule`
- `fn why_always_asks`
- `fn scan`
- `fn may_run`
- `fn approve`
- `fn revoke`
- `fn set_off`
- `fn add_from`
- `fn may_skip_question`
- `fn trust_step`
- `fn untrust_step`
- `fn remove`
- `fn offered`
- `fn take_offer`
- `fn decline_offer`
- `fn to_share`
- `fn is_due`
- `fn changes_to_carry`
- `fn take_synced`
- `fn hub_action`
- `struct Permission`
- `struct Manifest`
- `struct PluginFlow`
- `struct PluginStep`
- `struct Approval`
- `struct Approvals`
- `struct Plugin`
- `struct StepQuestion`
- `struct Kept`
- `struct Registry`
- `struct Offered`
- `struct Offers`
- `struct ScheduleRuns`
- `enum Schedule`
- `enum Status`
- `enum Offer`
- `const PLUGIN_API`
- `const OLDEST_PLUGIN_API`
- `const PLUGINS_DIR`
- `const MANIFEST_FILE`
- `const MAX_MANIFEST_BYTES`
- `const PERMISSIONS`
- `const NEVER`
- `const MIN_EVERY_MINUTES`
- `const ALWAYS_ASKS`
- `const SENT_SUFFIX`
- `const SHARED_IN`
- `const SYNC_PREFIX`

### `src/pngcodec.rs`

**Wired** — something outside this file calls into it.

Reading and writing PNG, in house.

**Sources:** the W3C PNG specification (2nd edition): the signature, the
chunk layout (length, type, data, CRC-32), IHDR, PLTE, tRNS, IDAT, IEND,
the five scanline filters and the Paeth predictor (§9), and zlib's framing
(RFC 1950: a two-byte header, DEFLATE, then an Adler-32 of the raw bytes).
DEFLATE is the in-house inflater `zipread` already has; CRC-32 is its too.
Clean-room; no image crate.

**Why Atlas wants it.** Animations were SVG only: a raster or a video needed
frames, and frames come back from a browser as PNG screenshots. Reading
them here is what lets Atlas build a GIF itself (`gifenc`) and draw a 3-D
scene straight to a picture (`scene3d`) with nothing installed.

Reads 8-bit greyscale, grey+alpha, RGB, RGBA and palette images (bit depths
1, 2, 4 and 8 for palette and greyscale), non-interlaced — what every
browser and renderer writes. Writes RGBA with stored (uncompressed) DEFLATE
blocks: larger files, but always correct and read by everything.

**Public interface:**

- `fn read_png`
- `fn write_png`
- `struct Rgba`

### `src/policy.rs`

**Wired** — something outside this file calls into it.

Approval gate. This is enforced in code, not described in a document.

Design rule: the gate is the ONLY path to a side effect. A new intent that
is not classified defaults to RequireApproval — unclassified is not "safe".

**Public interface:**

- `fn classify`
- `fn classify_with_policy`
- `fn gate`
- `struct PolicyConfig`
- `struct DenyAll`
- `struct AllowAll`
- `enum Decision`

### `src/portable.rs`

**Wired** — something outside this file calls into it.

What runs where, and what doesn't.

Written because "it's Rust so it's portable" is the answer that gets people
into trouble. The logic is portable — every module that decides something
compiles anywhere. What isn't portable is everything that touches a machine:
moving a window, reading the screen, hearing you, protecting a secret.

The useful question isn't "does it run" but "what does it do once it's
running", and the answers differ enough to be worth stating per platform
rather than as a single yes.

**Public interface:**

- `fn how`
- `fn because`
- `fn coverage`
- `fn honest_summary`
- `fn for_a_friend`
- `struct PortableConfig`
- `enum Platform`
- `enum Needs`
- `enum How`
- `const WHAT_TRAVELS`

### `src/preferences.rs`

**Wired** — something outside this file calls into it.

What you changed in the settings page, kept.

Every toggle on the hub reported success and wrote nothing.

`Settings::set` mutates one in-memory `Setting`, and in the running daemon
the whole `Settings` is rebuilt per request from `settings::registry(&self
.tools_cfg())` and then dropped — so the handler logged *"Voice is now
on"*, redirected to a page that re-rendered from the unchanged config, and
the toggle snapped back. There was no writer for `tools.yaml` anywhere in
the tree; the only YAML writers were `adapt.rs` (machine.yaml) and
`kin.rs` (kin_peers.yaml). That covered every switch on the page,
including the ones the module marks `Permission` and `Sensitive`.

**Why a separate file rather than editing `tools.yaml`.**
`config/tools.yaml` is about nineteen hundred lines, and most of them are
the reasoning: why a number is what it is, what breaks if you change it,
which decision it came from. Round-tripping it through a YAML serializer
would delete all of that on the first toggle. It is also `upgrade::SHIPPED`
— the installer replaces it wholesale — so anything written into it is
lost on the next update anyway.

So this is a third layer, and the project already had the shape for it:
the generic `config/*.yaml` ships to anyone, `config/machine.yaml` is what
`atlas adapt` found on this computer, and now `config/settings.yaml` is
what you chose. Each is applied over the last, in `Config::load`, in one
place rather than at every call site.

`config/settings.yaml` is in `upgrade::YOURS`, so an update keeps it.
Without that line it would be the trash-folder bug again: a file Atlas
promises to keep and the updater has never heard of.

**Public interface:**

- `fn path_for`
- `struct Preferences`

### `src/presence.rs`

**Wired** — something outside this file calls into it.

Are you at the desk?

This is worth more than it sounds, and not for the reason it first appears.
It is not security — a camera cannot tell you from a photograph either.
Its value is **timing**:

* Foreground work waits for a gap. Presence turns "you've been quiet for
20 seconds" into "you actually left", which is the difference between
guessing and knowing.
* Atlas should not talk to an empty room.
* Coming back is the moment to deliver the away-brief.
* If someone else is at your desk, sensitive output should stay quiet.

The camera is off by default, samples at a low rate, and the frames never
leave the machine.

**Public interface:**

- `fn keep_it_to_yourself`
- `fn interpret`
- `fn may_answer`
- `struct Look`
- `struct PresenceConfig`
- `struct Sensor`
- `enum Presence`
- `enum Change`
- `enum Gesture`
- `enum Signal`

### `src/proactive.rs`

**Wired** — something outside this file calls into it.

Proactive assistance.

The whole difficulty here is not detecting opportunities — it is *not*
taking most of them. An assistant that speaks whenever it notices something
is Clippy, and you will turn it off within a day. So every offer must clear
four independent bars, and the engine learns to stop offering things you
decline.

Hard rule: proactive assistance **offers**, it never acts. Autonomous
execution comes from the scheduler, where you put it deliberately.

**Public interface:**

- `fn detect`
- `fn from_nudge`
- `struct Offer`
- `struct ProactiveConfig`
- `struct Proactive`

### `src/probe.rs`

**Wired** — something outside this file calls into it.

Context acquisition by moving the workspace.

When you ask something Atlas can't answer from the focused window, it can
go and look: bring another window forward, capture it, and put your focus
back where it was. You get the answer without losing your place.

Two rules make this safe to do without asking every time. It only ever
*reads* — focus and capture, never a click or a keystroke. And it always
restores focus, even when a capture fails partway through.

**Public interface:**

- `fn app_named`
- `fn target_for`
- `struct Capture`
- `struct Probe`
- `enum Target`

### `src/profiles.rs`

**Wired** — something outside this file calls into it.

More than one person using Atlas.

The risk here is not technical, it is that one person's assistant quietly
knows another person's business. Memory, conversation history, approval
records, research notes, drafts, the outstanding list — all of it is
personal, and none of it should ever cross.

So a profile is not a setting. It is a **separate state directory**, and
the isolation is enforced by construction: nothing shares a path, and
switching wipes what's in memory rather than trusting the next read.

For your friends the right answer is usually simpler still — each runs
their own copy on their own machine. Profiles exist for the case where two
people share one computer, and for keeping a guest off your own data.

**Public interface:**

- `fn slug`
- `fn active_dir`
- `fn isolated_for_test`
- `struct Profile`
- `struct Profiles`
- `struct Switch`
- `enum Role`
- `const NEVER_AS_A_GUEST`
- `const ONLY_YOU_MAY_ASK`
- `const THE_OWNERS_OWN`

### `src/pronounce.rs`

**Wired** — something outside this file calls into it.

Text as it should sound: names, tickers, acronyms and symbols rewritten
before they reach the speech engine.

**Source:** the idea is the SSML `<sub alias>` / lexicon step every
text-to-speech front end has (W3C *Pronunciation Lexicon Specification*
1.0; eSpeak's and piper's own normalisers do the same for numbers). The
word lists here are Atlas's own. In house.

**Why Atlas wants it.** `docs/GAPS.md` §B, written down and never done:
"Local TTS will mangle product names, tickers, and acronyms. A small
pronunciation dictionary fixes it and is trivial to build." Piper reads
"EURUSD" as one word and "VPS" as "vips"; a reply with "→" or "±" in it is
read as silence or as the symbol's Unicode name.

**Public interface:**

- `fn for_speech`

### `src/prose.rs`

**Wired** — something outside this file calls into it.

Fixing what you type, as you type it.

Grammarly is a cloud service: every keystroke you make goes to a server.
For a system whose whole premise is that nothing leaves your machine,
that's the wrong shape — so this is a local proofreader instead.

It cannot do what a large model does, and it doesn't try. What it does is
catch the errors people *actually* make at speed, which turn out to be a
small and very repetitive set: dropped apostrophes, doubled words, the
wrong one of a homophone pair, a lower-case start after a full stop.
Between them those account for most of what you'd want caught.

The important design rule: **fix silently only what has one possible
correction.** "dont" is unambiguously "don't". "its" might be right. Fixing
the first automatically and merely flagging the second is the difference
between helpful and infuriating.

**Public interface:**

- `fn may_correct_in`
- `fn check`
- `fn check_phrases`
- `fn apply_certain`
- `fn spoken`
- `struct Fix`
- `struct ProseConfig`
- `struct Overreach`
- `struct Voice`
- `enum Kind`

### `src/publish.rs`

**Wired** — something outside this file calls into it.

Drafting, scheduling, and sending things that go out into the world.

Social posts and emails are the one category where a mistake is public and
cannot be taken back. So the rules here are stricter than anywhere else in
Atlas:

* Nothing is ever sent without approval of **that specific post**.
Approving one does not approve the next.
* **Editing after approval voids the approval.** Otherwise "yes, send that"
could be attached to text you never read.
* A scheduled post is checked again at send time — approval, length, and a
connection. Time passing is not consent.
* Cancelling is always available right up to the moment it goes.

**Public interface:**

- `struct Post`
- `struct Publisher`
- `enum Channel`
- `enum PostState`
- `enum SendCheck`

### `src/publishing.rs`

**Wired** — something outside this file calls into it.

Knowing where it's going.

A piece isn't finished until it's finished *for somewhere*. The same cut
wants different export settings, a different description, and a different
length depending on where it lands — and the differences are specific
enough to be worth knowing rather than guessing.

**Public interface:**

- `fn export_for`
- `fn export_args`
- `fn format_of`
- `fn where_to_get_music`
- `fn description_from`
- `fn tags`
- `fn ready_to_post`
- `fn platform_in`
- `fn format_named`
- `struct Export`
- `enum Platform`
- `enum Format`
- `enum MusicSource`
- `const IN_APP_TRADE`

### `src/quickinput.rs`

**Wired** — something outside this file calls into it.

Somewhere to type when speaking isn't working.

The gap this fills: voice fails — the room is loud, you have a cold, the
mic dropped — and there is nowhere to type. Opening Notepad to talk to your
assistant is absurd, and a terminal buried behind three windows is barely
better.

The answer is a key you press anywhere. A single-line box appears over
whatever you're doing, you type, press Enter, and it disappears. Nothing to
find, nothing to alt-tab to, no window management.

Two ways of showing that box, because one is available today and the other
is nicer:

* **Console** — bring Atlas's own window forward and focus it. Works
immediately, no new UI code.
* **Overlay** — a small borderless window drawn over everything, dismissed
on Escape. Better, and needs real Win32 work.

**Public interface:**

- `fn parse_hotkey`
- `struct QuickInputConfig`
- `struct QuickInput`
- `enum Surface`
- `enum State`
- `enum Action`

### `src/ratelimit.rs`

**Wired** — something outside this file calls into it.

Two guards for anything that talks to something outside Atlas: a rate
limit, and a breaker that stops calling a thing that is down.

**Sources:** GCRA (the Generic Cell Rate Algorithm) as `boinkor-net/governor`
(MIT) implements it — one number of state per key, the theoretical arrival
time: with `t` the interval per cell and `τ = t·(burst−1)`, a request at
`now` is refused if `now < TAT − τ`, else `TAT = max(TAT, now) + t`. The
breaker is the Closed → Open → HalfOpen machine of `dmexe/failsafe-rs`
(MIT). Clean-room.

**Why Atlas wants it.**
* The **online-secondary model** (`FallbackLlm`'s secondary slot, and the
coming WireGuard server): when it is down, every request today waits out
a timeout before falling back. A breaker learns it is down after a few
failures and falls straight back to local until a probe says otherwise.
* **Outbound** — outreach, SMTP, the Telegram channel — needs a ceiling so
a bug or a loop cannot send three hundred messages in a minute. GCRA gives
that with one integer per key and no background timer.
* **Anything that calls another Atlas** has the same shape in its degrade
path. A breaker makes "unreachable" a state with a reason instead of a
per-call timeout.

All times are passed in (ms). Nothing here reads a clock, so it is exact
under test and replay.

**Public interface:**

- `struct Gcra`
- `struct Breaker`
- `enum State`

### `src/reach.rs`

**Wired** — something outside this file calls into it.

Telling a signal from a fluke.

One post doing well means nothing. It's the single most expensive mistake
in content: something lands, you conclude you've found the formula, and you
spend a month making variations of a fluke.

So nothing here reports a single post as a finding. What it looks for is
**a pattern that holds across several**, and it says how sure it is.

**Public interface:**

- `fn quality_against`
- `fn findings`
- `fn direction`
- `fn outlier`
- `fn spoken`
- `struct Post`
- `struct Finding`
- `struct ReachConfig`
- `enum Sure`
- `enum Direction`
- `const ENOUGH_POSTS`
- `const WHAT_MAKES_A_POST_GOOD`

### `src/readable.rs`

**Wired** — something outside this file calls into it.

Pull the article out of a web page and leave the menus, cookie banners,
"related stories" and footers behind.

**Source:** Mozilla Readability (`mozilla/readability`, Apache-2.0) — the
Firefox Reader View algorithm; `kumabook/readability` (MIT, a Rust port)
read as a reference. Clean-room. The heuristics kept are the ones that
carry the result:

* drop "unlikely candidates" by class/id (comment, sidebar, footer, ad,
banner, social…) unless they also look like content (article, main, body);
* score each paragraph-ish block of 25+ characters as
`1 + (commas + 1) + min(len/100, 3)`, credit its parent fully, its
grandparent by half, and further ancestors by `1/(level·3)`, up to 5;
* seed a candidate by tag (div +5, pre/td/blockquote +3, lists −3,
headings −5) and by class/id (±25);
* multiply by `1 − link density`, take the best, and pull in siblings that
score at least `max(10, 0.2·best)` or are long, link-poor paragraphs.

**Why Atlas wants it.** `research::strip_html` removes script/style/svg and
the tags, then keeps *all* the text — so a research answer is built from
the navigation menu, the cookie notice and twelve "you may also like"
headlines as much as from the article. This returns the article.

**Public interface:**

- `fn extract`
- `struct Article`

### `src/recall.rs`

**Wired** — something outside this file calls into it.

Finding something you wrote, without remembering where you put it.

Phase 2. This is the payoff for everything Atlas has been accumulating —
notes, drafts, research, transcripts, the conversation thread. Months of it
is worth nothing if the only way in is remembering a filename.

Two ways of searching, and they answer different questions:

* **Words.** Fast, exact, needs no model, and finds the thing when you
remember a word from it. Ranked properly rather than by count — a word
that appears in every note tells you nothing, and one that appears in
three notes tells you a great deal.
* **Meaning.** Finds the thing when you remember what it was *about*.
Needs a small embedding model — 50–100MB, which is nothing beside a
language model, and on this laptop it runs on the NPU.

Both together beat either alone, so results are merged rather than chosen
between.

**Public interface:**

- `fn words_of`
- `fn spoken`
- `fn needs_a_model`
- `fn context_terms_from`
- `fn questions_from`
- `fn questions_written`
- `fn measure`
- `fn library_from_dir`
- `struct Piece`
- `struct RecallConfig`
- `struct Library`
- `struct Hit`
- `struct KnownQuestion`
- `struct Scores`
- `struct SearchCheck`
- `enum Clarity`

### `src/receipts.rs`

**Wired** — something outside this file calls into it.

Receipts, read: a photo or a PDF of one becomes a line you can find and
a spend `money` can count -- merchant, date, total.

"Keep this receipt" (a photo handed to the tray, or a screenshot);
"what did I spend at Costco this month?"; "receipts for work".

**Sources:** the receipt layouts in the SROIE dataset (ICDAR 2019) were
read for where the three fields sit -- merchant in the first lines, the
total on a line labelled TOTAL / AMOUNT DUE / BALANCE, usually the last
such line and the largest amount near the bottom -- and for the usual
traps: SUBTOTAL, TOTAL SAVINGS, TOTAL ITEMS, CHANGE and TENDERED all
carry the word or a bigger number.

**Soundproofing.**
- A total is only taken when it's labelled; the largest-number fallback
is offered as "probably", with a question, never filed silently.
- When subtotal + tax is on the receipt and doesn't make the total, it
says so rather than picking one.
- A card number on the receipt (the full PAN some printers still print)
is scrubbed from the kept text; only the last four stay.
- The same receipt kept twice (merchant, date, total) is one receipt.

**Public interface:**

- `fn amounts`
- `fn read`
- `fn money`
- `struct Receipt`
- `struct Reading`
- `struct Receipts`
- `enum Total`
- `const MAX_RECEIPTS`

### `src/reclaim.rs`

**Wired** — something outside this file calls into it.

Finding space on your disk, without ever being the reason you lost something.

Everything else Atlas deletes lives under `data/` — its own folder, its own
mess, capped at 500MB, with `retention::out_of_bounds` refusing anything
outside and logging the attempt as a bug upstream. That boundary is why
Atlas is safe to leave running.

This crosses it, deliberately and narrowly, because running out of disk
stops Atlas working at all and "I could see the problem and wasn't allowed
to mention it" is a poor answer.

## Four rules, and the reasoning for each

**1. An allowlist of places, never a blocklist.** A blocklist means Atlas
deletes anything nobody thought to exclude, and the first thing nobody
thought of is the thing you cannot replace. Every location here is named,
and anything unnamed is invisible to this module.

**2. Nothing is deleted. Things are moved to Atlas's trash**, which keeps
them 30 days. Every reclaim is reversible for a month. If Atlas is ever
wrong about a file, you get it back.

**3. Nothing recent.** A cache written this morning is a cache in use. Age
thresholds are per-category and deliberately generous.

**4. Atlas proposes; you decide.** `survey` returns candidates and nothing
else. There is no code path in this module that removes a file on Atlas's
own initiative — `reclaim` takes an explicit list, which has to come from a
person saying yes.

## What is deliberately not here

No `Documents`, `Desktop`, `Pictures`, source folders, or anything you
made. No "large files you haven't opened lately" — that heuristic finds
your archives and your backups. No emptying the system Recycle Bin: that is
the undo you already have, and a tool that empties it has removed your
safety net to save you space.

**Public interface:**

- `fn roots_from_env`
- `fn forbidden`
- `fn classify`
- `fn survey`
- `fn reclaim`
- `fn spoken`
- `fn whole_disk`
- `fn installed_apps`
- `fn report`
- `struct Candidate`
- `enum Kind`
- `const KNOWN`
- `const NEVER`

### `src/recovery.rs`

**Wired** — something outside this file calls into it.

Getting into the vault when you can't.

The passphrase being only in your head is what makes the vault worth
having, and it's also a single point of failure attached to a person who
goes places with no phone signal. Both are true at once.

The answer is not a backdoor. It's **a second way in that you set up
deliberately, that takes effort and time to use, and that you can see has
been used.** Three shapes, and you can have more than one.

**Public interface:**

- `fn gaps`
- `fn suggest`
- `fn described`
- `fn route_from`
- `fn spoken`
- `struct Setup`
- `struct RecoveryConfig`
- `struct Gap`
- `enum Route`
- `const TEST_IT`
- `const NOT_ATLAS`

### `src/recur.rs`

**Wired** — something outside this file calls into it.

Repeating events the way every calendar on earth writes them: RFC 5545 RRULE.

**Source:** the iCalendar spec (RFC 5545 §3.3.10), with `fmeringdal/rust-rrule`
(MIT OR Apache-2.0) and python-dateutil's `rrule` read as references for
behaviour. Clean-room: no code copied, the expansion order below is the
spec's table.

**Why Atlas wants it.** `calendar::Repeat` is deliberately four patterns
(once / daily / weekdays / weekly). That covers a standup and the gym. It
does not cover "the second Tuesday", "the last Friday of the month",
"every two weeks on Monday and Thursday" — and it cannot read the RRULE line
inside any `.ics` a business partner sends (see `vformat`). This is the
general rule, with a plain-English reader for the common spoken forms and a
describer so Atlas can say back what it understood.

Supported: FREQ (DAILY/WEEKLY/MONTHLY/YEARLY), INTERVAL, COUNT, UNTIL,
BYDAY (with ordinals in MONTHLY/YEARLY), BYMONTHDAY (negative = from the
end), BYMONTH, BYSETPOS, WKST, plus EXDATE on the series. Not supported,
refused rather than ignored: HOURLY/MINUTELY/SECONDLY, BYWEEKNO, BYYEARDAY,
BYHOUR/BYMINUTE/BYSECOND. A rule part this module does not understand is an
error, never silently dropped — a dropped BYSETPOS would put a meeting on
the wrong day and nothing would look broken.

**Public interface:**

- `fn parse_ical_time`
- `fn format_ical_time`
- `fn from_plain`
- `struct Rule`
- `struct Series`
- `enum Freq`
- `const MAX_PERIODS`

### `src/redact.rs`

**Wired** — something outside this file calls into it.

What leaves the machine, scrubbed: secrets and personal numbers replaced
with placeholders before a prompt goes to an online model, and put back in
the reply when it returns.

**Sources:** the token shapes are the published formats — AWS access key
ids (`AKIA`/`ASIA` + 16 base-32 characters), GitHub tokens (`ghp_` … + 36,
`github_pat_` + 82), Slack (`xoxb-`/`xoxp-`/`xoxe-`/`xoxa-`), PEM private
key blocks, `sk-` API keys, JWTs — as collected by Yelp's
`detect-secrets` (Apache-2.0) and `gitleaks` (MIT). The high-entropy rule
is detect-secrets' (Shannon entropy over the base-64 alphabet above 4.5,
over hex above 3.0). The personal-number checks are the standards': Luhn
for card numbers (ISO/IEC 7812), mod-97 for IBANs (ISO 13616), the SSA's
never-issued ranges for US SSNs. No regex crate — each shape is a small
scanner. Clean-room.

**Why Atlas wants it.** The secondary model is the one place a prompt
leaves the machine, and a prompt is built from whatever was on screen, in
the clipboard, in a file. The project rule is offline first; this is what
makes the online second safe to have switched on: the key in a config
file, the card number in an email, never reach a server, and the reply
still makes sense because the placeholders are swapped back locally.

**Public interface:**

- `fn secrets_in`
- `struct Scrubber`
- `enum Kind`

### `src/reference.rs`

**Wired** — something outside this file calls into it.

Knowing things offline, without a bigger model.

The instinct for "Atlas should know more" is a larger model, and it's the
wrong instinct. A model that fits on your laptop is about 4GB and it does
not reliably recall a futures contract's tick size or the wash sale rule's
exact window — it recalls something *shaped* like them, confidently, which
is worse than not knowing.

A text file recalls it exactly, costs 50MB, and can cite where it came
from.

So the split is: **the model reasons, the shelf remembers.** Anything with
a number, a date, a threshold or a legal definition lives on the shelf.
Anything requiring judgement goes to the model, with the relevant page from
the shelf attached.

**Public interface:**

- `fn worth_having`
- `fn chosen`
- `fn for_trading`
- `fn trading_mb`
- `fn is_stale`
- `fn stale_warning`
- `fn quoted`
- `fn nothing_found`
- `fn needs_the_shelf`
- `fn worth_keeping`
- `fn shelf_life`
- `fn gone_off`
- `fn correct_it`
- `fn never_used`
- `struct Shelf`
- `struct Found`
- `struct ReferenceConfig`
- `struct Kept`
- `enum Sort`
- `enum Worth`
- `const WHY_NOT_A_BIGGER_MODEL`
- `const WHAT_IT_COSTS`

### `src/references.rs`

**Wired** — something outside this file calls into it.

"Move it to the other screen."

Speech is full of pronouns, and an assistant that makes you name the target
every single time is a command line with extra steps. Atlas resolves "it",
"that", "this one" against what just happened.

The rule that keeps this safe: **resolve only when there is something to
resolve to.** A dangling pronoun becomes a question, never a guess. Guessing
wrong here means acting on the wrong window.

**Public interface:**

- `fn has_pronoun`
- `fn resolve`
- `struct Referents`
- `enum Kind`
- `enum Resolution`

### `src/refusals.rs`

**Wired** — something outside this file calls into it.

Why Atlas isn't trading.

Every refusal in [`crate::levels`] is a decision, and until now every one of
them was printed once and thrown away. That is half a record. A system that
keeps only what it did, and none of what it declined to do, cannot answer
the question that matters most when nothing is happening: **is it being
careful, or is it broken?**

Those two look identical from outside. A reader that refuses everything
because the market genuinely offers nothing, and a reader that refuses
everything because its cost limit is set for a timeframe it is not on, both
produce silence. One is the system working and one is a bug that will never
announce itself — nothing errors, nothing looks wrong, and the account just
never trades.

## What the shape of the refusals tells you

Counted by cause, the answer is usually obvious at a glance:

- Nearly all **costs too much** → the timeframe is too small for this
spread. Not a market condition; an arithmetic one, and no amount of
waiting fixes it.
- Nearly all **inside the noise** → the stop rule is tighter than the
market's ordinary movement. Same thing from the other side.
- Nearly all **standing down** → either the calendar is stuck on, or Atlas
is only ever asked during release windows.
- Nearly all **not enough room** → the minimum reward is set above what
this market actually offers.
- A **spread** of causes → this is a reader being careful, which is what it
was built for.

None of that is derivable from the ideas Atlas *did* produce. It is only
visible in what it turned down.

## Deliberately not a judgement

This counts and reports. It does not decide that a limit is wrong and
change it — nothing here writes to `Rules`. A module that quietly loosened
its own limits because it had been refusing a lot would be a system that
talks itself into trades, which is the exact failure the refusals exist to
prevent.

**Public interface:**

- `struct Turned`
- `struct Refusals`
- `const KEEP_EXAMPLES`
- `const ENOUGH_TO_LOOK`
- `const LOPSIDED`

### `src/register.rs`

**Wired** — something outside this file calls into it.

Reading the room.

Atlas shouldn't answer "what's the plan for the quarter" and "what did you
think of that film" in the same voice. Nor should it drop a formal
transparency notice into a call with two friends.

So before it decides *what* to say, it works out what kind of moment this
is. Everything downstream — length, tone, whether a joke is welcome —
keys off that.

**Public interface:**

- `fn read`
- `fn formality`
- `fn announcement_for`
- `struct Moment`
- `struct CallContext`
- `enum Register`
- `enum Formality`

### `src/rehearse.rs`

**Wired** — something outside this file calls into it.

Showing you what it would do, without doing it.

Idea #4. Before letting Atlas near a real workflow you watch the whole
thing play out harmlessly: every window it would move, every file it would
touch, every message it would send.

The machinery already existed — the fake operating system that the test
suite runs against. Nothing exposed it to you. This does.

The guarantee is structural: a rehearsal runs against the mock platform, so
there is no code path from a rehearsal to your actual windows.

**Public interface:**

- `fn from_actions`
- `fn preamble`
- `struct Beat`
- `struct Rehearsal`

### `src/release.rs`

**Wired** — something outside this file calls into it.

Proving an update is really from you — the release signature.

Atlas already has symmetric crypto: the household key seals a bundle so only
your own devices can open it. That proves *from this household*. It cannot
prove *authored by you, specifically* — everyone in the household holds the
same key and could forge with it. An update channel that runs over a mesh
needs the stronger statement, because a forged update is code execution on
every device that takes it. So a release is signed with an **ed25519** key
whose **private half never leaves the build machine**, and whose **public
half is baked into every Atlas as the one trust anchor**. A device stages an
update only if its signature verifies against that anchor.

This module is that primitive — sign, verify, and the trust anchor. The
signed *manifest* that lists the per-platform artifacts and their hashes is
built on top of it (next step of the update courier), and the daemon wires
the check into the sync pass after that. Until then this is the crypto root,
proven by its own tests and not yet reached from production — deliberately,
because the root must be right before anything is allowed to depend on it.

## The safe default

The baked-in anchor starts as an all-zero **placeholder**, because the real
keypair is generated once, by you, as part of build-account setup. A
placeholder is treated as *no anchor configured*: verification returns
`NoAnchor` and the updater trusts **nothing**. A forgotten or unset key can
therefore never masquerade as trust — the failure mode is "refuse every
update", never "accept an unsigned one".

**Public interface:**

- `fn make_release_key`
- `fn anchor_configured`
- `fn verify_against`
- `fn signing_key_from_seed`
- `fn anchor_of`
- `fn this_platform`
- `fn seal_manifest`
- `fn accept`
- `fn accept_against`
- `fn new_seed`
- `fn seed_hex`
- `fn seed_from_hex`
- `fn as_rust_array`
- `fn manifest_for`
- `fn verified_notice`
- `fn check_artifact`
- `fn freshness`
- `fn seal_rotation`
- `fn apply_rotation`
- `fn apply_rotation_with`
- `fn find_build`
- `fn sign_build`
- `struct MadeKey`
- `struct Artifact`
- `struct Manifest`
- `struct SignedManifest`
- `struct Accepted`
- `struct LocalApproval`
- `struct Fingerprint`
- `struct Installed`
- `struct TrustState`
- `struct Rotation`
- `struct SignedRotation`
- `struct FoundBuild`
- `struct SignedBuild`
- `enum Verdict`
- `enum Refusal`
- `enum Direction`
- `enum Freshness`
- `const RELEASE_PUBLIC_KEY`
- `const RELEASE_KEY_NAME`
- `const KEY_CARD`
- `const LAST_SEQUENCE`
- `const SENT_SHA`
- `const MANIFEST_FORMAT`
- `const MAX_MANIFEST_BYTES`
- `const KNOWN_PLATFORMS`
- `const INSTALLED_FILE`
- `const RECOVERY_PUBLIC_KEY`

### `src/remote.rs`

**Wired** — something outside this file calls into it.

Asking the laptop to do something while you're not at it.

You're out with your phone, the laptop is at home and on. Anything that
needs the bigger model, your files, or the browser should happen there —
and you should hear about it when it's done, not have to remember to check.

The rule that keeps this sane: **the phone asks, it doesn't command.** The
laptop decides whether it can, does the work, and reports back. If the
laptop is off, the request waits rather than failing, because a request
that quietly disappears is worse than one that's slow.

**Public interface:**

- `fn handing_off`
- `fn finished`
- `fn never_ran`
- `fn worth_waking_for`
- `fn reading_or_change`
- `fn needs_your_yes`
- `fn asking_before_it_runs`
- `struct Request`
- `struct RemoteConfig`
- `struct Queue`
- `enum Needs`
- `enum State`
- `enum How`
- `enum Looks`

### `src/research.rs`

**Wired** — something outside this file calls into it.

Research engine — the flagship background-lane job.

Search, fetch, strip, summarize, write a note. None of it touches your
screen: the fetching happens in a separate headless browser process, not in
the Chrome window you are using. So "look into X for me" starts immediately
and you carry on working.

**Public interface:**

- `fn how_well_read`
- `fn rests_on`
- `fn extract_urls`
- `fn search_page_url`
- `fn urls_from_links`
- `fn searxng_url`
- `fn urls_from_searxng`
- `fn page_markdown`
- `fn page_text`
- `fn strip_html`
- `fn first_sentences`
- `fn urlencode`
- `fn figures_not_in`
- `struct ResearchConfig`
- `struct Source`
- `struct Note`
- `struct Research`
- `const WELL_READ`
- `const SUBSTANTIAL`
- `const UNCONFIRMED`

### `src/resume.rs`

**Wired** — something outside this file calls into it.

What Atlas was doing when it stopped, and what it does about it when it
starts again.

Before this, a restart lost everything in hand: a window being worked for
you, research half done, a council mid-debate. Nothing said so; you'd find
out by the answer never coming. Now each piece of work you asked for is
written down as it's handed to the crew and crossed off when it ends, and
a window being worked is written down whenever it changes. What's still
written down at start-up is what was cut off.

What happens to each depends on what redoing it would do:

- **Again** — work whose only effect is an answer or a proposal (research,
the council, building or improving code that lands in a queue for your
go-ahead). Redone once, from your own words, and said so. A piece of
work that was already redone once after a restart isn't redone a second
time: if it was what brought Atlas down, doing it again would bring it
down again.
- **Ask** — work that reaches outside the machine or acts as you (mail,
unsubscribing, outreach, signing in to Outlook), or that depended on a
moment that has passed (a look at your screen, a call's write-up). Named,
never redone on its own.
- Chores Atlas starts itself (backups, housekeeping, the search check)
aren't recorded at all: they come round again on their own.

**Public interface:**

- `fn what_to_do_with`
- `fn sort`
- `fn said`
- `struct Unfinished`
- `struct SavedWindow`
- `enum After`
- `const RECORD`
- `const WINDOWS`

### `src/retention.rs`

**Wired** — something outside this file calls into it.

Keeping what matters, discarding what doesn't, under a hard ceiling.

The governing principle: **store pointers, not payloads.**

Atlas never copies your documents into its own store. The index holds a
path, a size, and a timestamp — about 200 bytes per file, so a 200,000 file
index is roughly 40MB. When you ask what's in a document, Atlas re-reads
the original. Recall costs nothing to keep because the data is already on
your disk; duplicating it would be the expensive mistake.

What actually grows is the stuff Atlas *generates*: screenshots, wav
scratch files, logs, and its own history. Those are what this module bounds.

**Public interface:**

- `fn classify_within`
- `fn classify`
- `fn survey`
- `fn usage`
- `fn plan`
- `fn apply`
- `fn out_of_bounds`
- `fn irreducible`
- `fn discard_audio`
- `struct RetentionConfig`
- `struct Item`
- `struct Usage`
- `struct Recording`
- `enum Class`
- `enum Plan`

### `src/returning.rs`

**Wired** — something outside this file calls into it.

Coming back.

You step away and things happen: a job finishes, something fails, a post
misses its window. When you sit back down, the wrong move is to say all of
it, and the other wrong move is to say none of it and let you find out.

What you get depends on how long you were gone. Five minutes is not an
absence and deserves silence. Four hours is, and deserves the short
version. Overnight deserves to know what needs deciding before you start.

**Public interface:**

- `fn how_long`
- `fn welcome`
- `fn address_change`
- `fn confirm_address`
- `fn full_brief`
- `struct Happened`
- `struct ReturnConfig`
- `enum Address`
- `enum Gone`
- `enum Welcome`

### `src/revise.rs`

**Wired** — something outside this file calls into it.

Turning corrections into edits.

You correct Atlas. The conversation ends. The correction dies with it, and
next week you make the same correction again. That is the single most
expensive thing a memory system can get wrong, because it wastes the one
input that is unambiguously worth keeping: you, telling it directly that it
was wrong.

Four rules, and the third is the one everything else hangs on:

1. **Where it is written decides whether it works.** A correction about
*how a task is done* belongs in the instructions for that task, not in a
diary. The instructions are read every time the task runs. The diary may
never be read again. Same fact, two places, one of them useless.
2. **Store why it was wrong and what right looks like** — never "you didn't
like it". A note that records displeasure teaches nothing.
3. **Wait for the repeat.** One correction in a session is a note. It earns
an edit only when you have said it twice. Otherwise Atlas rebuilds
itself around a bad day, and a rule made from one irritable evening is
worse than no rule.
4. **Show the exact lines before changing them.** Surgical edits, named,
reviewable. Never a rewrite.

The scoreboard is one question: does it make the same mistake twice.
`repeat_rate` answers it and nothing else here matters if that number is
not falling.

**Public interface:**

- `fn slug`
- `fn related_stale`
- `fn home_for`
- `fn proposal`
- `fn standing`
- `fn wanted_in`
- `fn subject_of`
- `struct Correction`
- `struct Edit`
- `struct Mending`
- `enum Home`
- `const REPEATS_NEEDED`
- `const MAX_STANDING`

### `src/rollover.rs`

**Wired** — something outside this file calls into it.

What it costs to hold, and the worst ninety seconds of the day to decide in.

Two things happen at 17:00 New York and they are the same event seen from
two sides.

## 1. The book goes thin

Every desk rolls its positions at once, the outgoing session has gone home
and the incoming one has not arrived. Quoted spreads at the turn are
routinely several times their normal width, and they are quoted on a book
nobody is really making.

A stop sitting in that window gets filled at the wide price. So does a
market order. The trade is not worse — the *execution* is worse, and the
record afterwards shows a losing trade with a perfectly good reason behind
it, which is exactly the way a system learns the wrong lesson.

This module will not say *how* wide, because that is a broker fact and
Atlas has no feed. It says **when**, which is the half that can be known
offline and is the half that matters for deciding whether to act now or in
twenty minutes.

## 2. Swap is charged — and on Wednesday it is charged three times

Spot FX settles two business days out. A position held through Wednesday's
17:00 New York rolls its value date from Friday to **Monday**, so it is
charged or paid **three days** of interest in one go.

This is not a subtlety and it is not rare: it happens every single week.
On a carry-negative pair it can be the difference between a small winner
and a small loser, and it lands on the one night in five that nobody
remembers. A swing trade opened Wednesday morning is a different trade from
the same setup opened Tuesday morning, and nothing in a chart says so.

## What is modelled, plainly

- Rollover at **17:00 New York**, from [`crate::fxday`], so it follows New
York's clock rather than a fixed UTC hour.
- Rollovers on **Monday to Thursday**. Friday's 17:00 is the week's close
— nothing is held through it into a trading session — and the weekend
nights are the ones Wednesday already paid for.
- **Triple on Wednesday.**

Some brokers differ, most often on pairs whose value dates fall on a local
holiday, and a few roll a day early around New Year. None of that is
derivable without that broker's calendar, so none of it is invented here.
What is here is the rule that holds for the overwhelming majority of nights
at the overwhelming majority of brokers, and [`Nights::caveat`] says so out
loud rather than letting the number look more certain than it is.

**Public interface:**

- `fn rollover_ending`
- `fn held_through`
- `fn thin`
- `fn spoken`
- `struct Rollover`
- `struct Nights`
- `enum Thin`
- `const THIN_MINUTES`

### `src/roots.rs`

**Wired** — something outside this file calls into it.

Where this install lives.

Everything Atlas remembers — `data/state`, `data/backups`, `data/logs`,
`config/` — used to hang off the *current working directory*, because the
call sites said `Store::new("data/state")` and `Config::load("config")`.
`ATLAS.bat` hid that, because its first real line is `cd /d "%~dp0"`. The
day the `.bat` is not the launcher — a Start Menu shortcut with the wrong
"Start in", Task Scheduler (whose working directory is `system32`), a
terminal that happens to be somewhere else, `atlas update` run from
anywhere — Atlas came up with an *empty* `data/state`, silently: no
memory, no pairings, no backups, and a second `data/` tree written
wherever it was launched from. Nothing errored.

`Store::install_root()` did not save us either. It climbs two levels when
the root's last two components are literally `data`/`state`, so for the
relative `"data/state"` the two parents are `"data"` and `""` — it
returned the **empty path**, and every `install_root().join("data/logs")`
downstream was cwd-relative again. The rule was stated at the leaf and
broken at the trunk.

So: one place decides, once, and everything derives from it. The rule is
*the install is where the executable is*, not where you happened to be
standing. `ATLAS_HOME` overrides it for anyone who wants the program and
its data apart.

`tests/one_install_root.rs` asserts that no literal `"data/…"` or
`"config/…"` path survives anywhere else in `src`, so a future call site
cannot reintroduce this one file at a time — which is how it happened
four times already (`BackupConfig.dir`, `data/index.md`, the trace log,
and then the trunk itself).

**Public interface:**

- `fn looks_like_an_install`
- `fn install_root`
- `fn how`
- `fn where_and_why`
- `fn first_run_here`
- `fn data_dir`
- `fn state_dir`
- `fn install_state`
- `fn store`
- `fn config_dir`
- `fn config_file`
- `fn data_sub`
- `fn logs_dir`
- `fn notes_dir`
- `fn backups_dir`
- `fn trash_dir`
- `fn tmp_dir`
- `fn models_dir`
- `fn under_install`
- `fn sweep_run_scratch`
- `struct RunScratch`
- `enum Chosen`
- `const RUN_SCRATCH_STALE_SECS`

### `src/roster.rs`

**Wired** — something outside this file calls into it.

Who, from another Atlas entirely, may see a shared business space.

## The gap this closes

`kin.rs` answers "which Atlas instance is this, really" -- a peer's
token proves they are who their pairing says. It answers nothing about
*what* they may see. Without this file, the only fact available at a
business's door would be "a paired Atlas is asking", and every paired
Atlas would be equally able to ask -- a friend paired for ordinary
nudges would sit exactly as close to a business's shared space as an
actual partner in it. That is not a hypothetical: it is the shape of
the liability Eric named directly -- a business partner's own Atlas, or
anyone else's, reaching a business they were never added to.

So this is deliberately a second gate, not a wider first one. Being
paired is necessary and proves identity; being on a business's roster
is separate and proves permission. Neither implies the other, and
`may_see` checks both every time rather than caching the answer,
because the interesting failure is not "a stranger got in" -- `kin.rs`
already refuses those -- it is "someone real, paired for a different
reason entirely, quietly inherited access to something they were never
added to."

## Default deny, the same shape as `firewall.rs`

An unlisted name sees nothing. There is no third state, no benefit of
the doubt, and no way to add someone to a business without first
knowing them as a kin peer -- a roster entry for a name `kin.rs` has
never heard of would be a promise with no channel behind it, silently
unenforceable the moment somebody typed it.

## Revocation is automatic, not a second step

`may_see` re-checks `kin::Pairings` every time rather than trusting its
own membership list alone. Forgetting a pairing (`Pairings::forget`)
therefore removes every business that person could see through it, in
the same motion, with nothing left to separately clean up. The
dependency runs one way on purpose: being on a roster requires an
active pairing; an active pairing implies nothing about any roster.

## What this cannot do

It cannot recall what has already crossed. Removing someone from a
roster stops the *next* thing from reaching them; anything they already
received is already theirs, the same limit `firewall.rs` names for
personal material and does not pretend to solve. And it says nothing
about what a roster member may *do* once something has reached them --
read versus write is a separate, real question this file does not
answer.

**Public interface:**

- `struct Roster`
- `enum RosterError`

### `src/route.rs`

**Wired** — something outside this file calls into it.

Finding another way in.

"I'd rather not, we've tried that twice" is honest and useless. If Atlas
knows an approach fails, the useful next move is a different approach —
and most problems have several ways in that fail independently.

The other half of what you asked for is speed. Quality and speed only
conflict when everything gets the same treatment; the way out is to spend
effort where it changes the answer and nowhere else. So a route is chosen
by what's cheapest among the things that could actually work, rather than
by trying them in the order they were written down.

**Public interface:**

- `fn known_routes`
- `fn needs_internet`
- `fn coverage`
- `fn plan`
- `fn plan_with`
- `fn all_routes`
- `fn stuck_on`
- `fn switching`
- `fn stuck_spoken`
- `struct Route`
- `struct RouteConfig`
- `struct Record`
- `enum Kind`
- `enum Plan`

### `src/routine.rs`

**Wired** — something outside this file calls into it.

Learning something you do the same way every time.

Phase 3.2. You open the same three tabs every Monday, or check the same
four numbers before the market opens, or file the same report the same way.
Doing it for you is worth having; the interesting question is how Atlas
knows what "it" is without you writing a script.

The answer is that it watches, notices repetition, and **asks**. A system
that silently decides your Monday morning is a routine and starts doing it
is unnerving even when it's right.

**Public interface:**

- `fn is_concrete`
- `fn sends_something`
- `fn ask_about`
- `fn starting`
- `struct Did`
- `struct Routine`
- `struct RoutineConfig`
- `struct Watcher`

### `src/safety.rs`

**Wired** — something outside this file calls into it.

Backup and undo.

Two gaps from the audit, and they are the same gap seen from either side:
everything Atlas has learned lives in one folder with no copy, and nothing
it does can be taken back.

Neither is exotic. A backup is a dated copy with old ones pruned. Undo is
a trash folder plus a record of what came from where. Both are cheap, and
both are the difference between an assistant you can trust with your files
and one you can't.

**Public interface:**

- `fn back_up`
- `fn backups`
- `fn list_backups`
- `fn prune_backups`
- `fn due_for_backup`
- `fn restore`
- `struct BackupConfig`
- `struct Backup`
- `struct Discarded`
- `struct TrashConfig`
- `struct Trash`
- `enum LedgerState`

### `src/sandbox.rs`

**Wired** — something outside this file calls into it.

Where Atlas does its own work.

For Atlas to write and test code, it needs somewhere to be wrong. A
sandbox is a scratch directory it owns completely: it can create, edit and
run things there, and **nothing reaches your machine until you say so.**

Three properties carry the safety, and all three are enforced here rather
than left to whoever writes the next feature:

1. **Nothing is written outside the sandbox root.** A path that climbs out
with `..` is rejected, not normalised and allowed.
2. **Promotion requires an explicit yes**, per batch, showing what changes.
3. **Anything replaced goes to trash first**, so accepting a bad change is
undoable.

**Public interface:**

- `fn trim_output`
- `struct Attempt`
- `struct Change`
- `struct Fingerprint`
- `struct Sandbox`

### `src/scene3d.rs`

**Wired** — something outside this file calls into it.

3-D scenes that move, drawn in house — and handed to Blender when it's there.

A scene is JSON: shapes with colours, a sun, a sky, a camera — and, when it
has a `duration`, a timeline. Anything can move: an object's position,
turn, size and colour, the camera's position, target and lens, each by
keyframes with an easing, or by `spin` (degrees a second) and a camera
`orbit`. Still scenes come out as a picture and a turntable; moving ones as
a GIF, an MP4 (with ffmpeg) and the frames.

**How it's drawn.** A ray tracer, written here, with the parts that make a
picture read as solid rather than as flat shapes:

- **Shapes** — sphere, box, cylinder, cone, capsule, torus and a ground —
each turned (`rotate`, degrees about x, y, z) and sized (`scale`) by its
own transform, so a box can tumble. The torus is found by sphere tracing
its distance function (Hart 1996); the rest are solved exactly.
- **Light** — a sun with a real size, so shadows go soft at the edges;
light from the whole sky, darkened where it can't reach (ambient
occlusion); a highlight, and reflections weighted by angle (Schlick's
Fresnel) on shiny things; haze toward the horizon.
- **Film** — several jittered rays a pixel on a fixed pattern (so still
parts of a moving scene don't shimmer), each lighting sample spread across
them; the camera's shutter open for half a frame, so fast things blur the
way they do on film; then the ACES filmic curve (Narkowicz's fit) and the
sRGB gamma, so bright light rolls off instead of clipping to white.
- Rows are drawn on every core.

**Keyframes** follow glTF 2.0's animation model: a channel (`prop`) has
times and values, and between two keys the value is interpolated — here
with an easing from Penner's set (linear, step, ease-in/out, bounce, back,
elastic). Values are positions, angles, a size or a colour.

**Blender** gets the same scene, motion baked frame by frame from this
module's own timeline — so what Blender renders moves exactly as the
in-house render does — as a Python script it runs headless. What comes back
is checked like every render: it exists, it is the size and the frame
count asked.

**What's checked** before and after drawing: the scene reads, the timeline
is sane, something is in view, every object stays in frame (or it's said
when one leaves), and a scene that claims to move does move. Whether it
looks good stays with you.

Sources: Shirley, *Ray Tracing in One Weekend* series (CC0); Blinn 1977;
Schlick 1994; Hart 1996 (sphere tracing); Khronos glTF 2.0 §3.11
(animations); Penner's easing equations; Narkowicz 2016 (ACES fit);
Blender's Python API (`keyframe_insert`, `render.render(animation=True)`);
round 8: Snell's law with Schlick's Fresnel for glass; next-event
estimation by uniform cone sampling for lamps (PBRT 4ed §12, §13.2);
Roberts' R2 sequence; Dammertz, Sewtz, Hanika & Lensch 2010 (edge-avoiding
à-trous) and Schied et al. 2017 (SVGF variance steering) for the denoiser;
model files through `meshio`.

**Public interface:**

- `fn render_frame`
- `fn model_files`
- `fn load_scene`
- `fn parse_scene`
- `fn check_scene`
- `fn blender_script`
- `fn find_blender`
- `fn draft_scene`
- `fn make`
- `struct Camera`
- `struct Object`
- `struct Scene`
- `struct Track`
- `struct SceneMade`
- `enum Shape`
- `enum Material`
- `enum Pattern`
- `enum Sun`
- `enum Quality`
- `enum Ease`
- `const SCENE_SYSTEM`

### `src/scheduler.rs`

**Wired** — something outside this file calls into it.

Scheduler: one-time, delayed, and recurring jobs, plus jobs parked waiting
for approval.

Time is passed in rather than read from the clock, so schedule behaviour is
testable without sleeping.

**Public interface:**

- `struct Job`
- `struct Scheduler`
- `enum JobState`

### `src/screentext.rs`

**Wired** — something outside this file calls into it.

"Copy the text off the screen": the window in front, read, and put on
the clipboard -- an error dialog, a chart's labels, a paused video's
caption, a PDF that won't let you select.

**Sources:** PowerToys Text Extractor (MIT; read as the reference for the
idea and for its choice of engine) uses Windows' own `Windows.Media.Ocr`,
which runs entirely on the device, needs no install beyond the display
language's OCR pack, and returns lines. That is the first engine here
(`Platform::recognise_text`). Atlas's own reader (`words`, two ONNX models)
is the second, where it's installed; `words` was already reading handed
photos. The capture is the window's rectangle off the screen (GDI
`BitBlt`), so what's read is what you see.

**Kept honest.** A reading that's mostly noise isn't put on the clipboard
(`plausible`): half-recognised words look like a quotation and aren't
one. Anything secret-looking on the screen is read, because you asked for
the screen -- but it's said, so it isn't pasted somewhere by surprise.

**Public interface:**

- `fn tidy_lines`
- `fn plausible`
- `fn pick`
- `fn said`
- `fn question_prompt`
- `fn said_without_a_model`
- `enum Engine`
- `const WORDS_ONLY`

### `src/sealedlog.rs`

**Wired** — something outside this file calls into it.

A record that can prove it was only ever added to.

**Source:** RFC 6962 (Certificate Transparency) Merkle tree hashing —
leaf `SHA-256(0x00 ‖ data)`, node `SHA-256(0x01 ‖ left ‖ right)`, split at
the largest power of two below n — and the consistency proof between two
tree sizes, verified by RFC 9162 §2.1.4.2. `google/trillian` (Apache-2.0)
read as the reference implementation. Clean-room. SHA-256 is the tree's
own (`digest`), not a second copy.

**What it is for here.** `activity::Journal` is "what Atlas did" — the
record you read when you come back to a changed workspace, and the one an
audit of what went out into the world reads. It is a JSON file anyone (or
any bug) can edit, and a quietly edited record of what an assistant did is
worse than none. So every entry also goes into a Merkle log of hashes, and
a checkpoint (size + root) is kept each day. `atlas doctor` then checks
that every entry still hashes to its leaf and that today's log *extends*
each earlier checkpoint — proven with a handful of hashes per checkpoint,
not by trusting the file.

What it does not claim: someone who rewrites the journal, the leaves and
every checkpoint consistently is not caught by a file on the same disk.
Copying a checkpoint somewhere else (the phone, the hub) is what closes
that, and is the next step, not this one.

**Public interface:**

- `fn hex`
- `fn unhex`
- `fn leaf_hash`
- `fn verify_consistency`
- `struct Checkpoint`
- `struct Log`

### `src/selfaudit.rs`

**Wired** — something outside this file calls into it.

Atlas looking at itself and deciding what to fix.

The pipeline needs a diagnosis before it will do anything, and until now
that diagnosis had to come from you. That's the wrong way round: you are
the person least able to see which of Atlas's own routes keep failing, or
which answers you keep correcting.

So this is the stage before Thought — noticing. It reads what Atlas already
records about itself, turns the strongest signals into diagnoses, and hands
them to the pipeline in the shape it demands: a symptom, a cause, where it
is, and something that would prove it fixed.

**It never proposes work on the strength of one observation.** One failed
route is a bad day; the same route failing eleven times is a fault.

**Public interface:**

- `fn recommend`
- `fn spoken`
- `fn as_thought`
- `fn time_to_look`
- `fn unprompted`
- `struct Signal`
- `struct Recommendation`
- `struct LastLook`
- `struct Unprompted`
- `struct SelfAuditConfig`
- `enum Kind`
- `const LOOK_RECORD`
- `const WHY_IT_ASKS`

### `src/selfgrant.rs`

**Wired** — something outside this file calls into it.

What Atlas may fix on its own.

Asking before every change sounds safe and isn't — a system that needs
permission to fix a typo in a phrase list will never fix one, and you end
up with the recommendations piling up unread. That's the failure mode of
"always ask": it doesn't make anything safer, it makes everything stop.

So permission is granted by **what a change touches**, once, rather than by
confidence in each change. Confidence is self-assessed, which makes it
useless as a safety property — Atlas being sure about something is not
evidence.

**Public interface:**

- `fn reach_of`
- `fn reach_of_change`
- `fn may_land`
- `fn raise_it`
- `fn raised_again`
- `fn asking_for`
- `struct Granted`
- `struct SelfGrantConfig`
- `enum Reach`
- `enum Verdict`
- `const LIMIT_MODULES`
- `const WHY_STANDING`
- `const NOT_BY_CONFIDENCE`

### `src/selfwork.rs`

**Wired** — something outside this file calls into it.

Atlas working on Atlas.

**This module is the machinery; `pipeline` is the discipline.** What was
here went straight from a goal to an attempt, which is how a self-improving
system rots: green tests say nothing about whether the right thing was
fixed, and a patched symptom passes exactly as well as a real fix.

`Session::after` still drives the attempts. What changed is that a session
can no longer start without a diagnosis, or land without a review.

The pieces have all existed for a while — a sandbox it can be wrong in, a
ladder of distinct approaches, a way to ask for help, and 1158 tests. This
is the loop that joins them.

The test suite is what makes this reasonable rather than reckless. A change
that breaks something gets caught before you ever see it, and nothing
reaches your files until you've seen the diff.

**Public interface:**

- `fn may_edit`
- `fn files_named`
- `fn draft_fix`
- `fn lines_touched`
- `fn run_the_proof`
- `fn read_a_proof_run`
- `fn run_tests`
- `fn count_passing`
- `fn what_holds_it_back`
- `fn land`
- `fn prove_in_project`
- `struct SelfWorkConfig`
- `struct Edit`
- `struct Tried`
- `struct Session`
- `struct ProjectProof`
- `enum Verdict`
- `enum Step`
- `enum ProofToday`
- `enum Held`
- `const MEND_SYSTEM`
- `const PROOF_BUDGET_SECS`
- `const PROJECT_PROOF_BUDGET_SECS`

### `src/server.rs`

**Wired** — something outside this file calls into it.

The local API.

This is what makes "carry on from my phone" possible. Atlas listens on
loopback only; anything from another device reaches it through a VPN that
terminates on this machine, so the listener itself is never exposed.

Three rules, none of them optional:

1. **Loopback only.** Binding to 0.0.0.0 would put a command endpoint for
your workspace on whatever café network you're on.
2. **Every request carries a token.** Generated on first run, stored with
the rest of Atlas's state, compared in constant time.
3. **Read and queue, never execute directly.** A phone can ask what's
happening and add to the queue. It cannot make Atlas type into a window
you can't see.

**Public interface:**

- `fn bind_address`
- `fn parse_request`
- `fn query_field`
- `fn content_length`
- `fn token_matches`
- `fn private_line`
- `fn route_signal`
- `fn route_handoff`
- `fn route_chat`
- `fn route_read`
- `fn route_hello`
- `fn route_group`
- `fn route_feedback`
- `fn route_left`
- `fn route`
- `fn body_cap`
- `fn render`
- `fn new_token`
- `fn token_for`
- `fn hub_url`
- `fn open_hub`
- `fn record_door`
- `fn ping`
- `fn atlas_hub_port`
- `fn hub_port`
- `struct ServerConfig`
- `struct Request`
- `struct Reply`
- `struct Secret`
- `struct Failures`
- `struct Server`
- `struct Waiting`
- `struct HubCost`
- `struct HubDoor`
- `struct Retry`
- `struct Door`
- `struct SignalListener`
- `enum Body`
- `enum Action`
- `const COOKIE`
- `const CALENDAR_BODY`
- `const FALLBACK_PORTS`
- `const PING_PATH`
- `const DOOR_FILE`

### `src/session.rs`

**Wired** — something outside this file calls into it.

Conversation / session manager.

Holds enough context for follow-ups ("move it to the other screen") to
resolve, and parks a pending question or approval so the next thing you
say is read as an answer rather than a new command.

**Public interface:**

- `fn is_yes`
- `fn is_always`
- `fn is_no`
- `fn kind_of`
- `fn app_of`
- `struct Turn`
- `struct Session`
- `enum Pending`

### `src/settings.rs`

**Wired** — something outside this file calls into it.

Every switch in one place.

Atlas has accumulated a lot of behaviour that can be turned on and off, and
until now the only way to change any of it was to edit YAML. That's a gap:
a setting nobody can find is a setting that doesn't exist.

This is the registry the hub renders. Each entry knows what it is, what it
does, what happens if you change it, and — importantly — **what it costs**.
A toggle that turns on a camera should not look identical to one that
changes how many sentences Atlas speaks.

**Public interface:**

- `fn needs_a_restart`
- `fn registry`
- `struct Setting`
- `struct Settings`
- `enum Weight`
- `enum Value`
- `const GROUP_ORDER`
- `const NEEDS_A_RESTART`

### `src/settingswin.rs`

**Wired** — something outside this file calls into it.

Atlas's settings, in Atlas's own window.

Eric, 23 Sep 2026: *"I still don't know how to access the hub to be able to
get to my settings for Atlas."* On the laptop there was no way a person
would find: the hub is a web page reached by pasting a printed address with
a token in it, and by Eric's own ruling (17 Sep) the desktop surface is not
a browser — while the native settings panel was a placeholder that said
"Settings are up." and listed nothing.

This is the real one. Every setting in `settings::registry` — the same list
the hub renders — grouped the same way, each with what it does and what it
costs, with a control that fits it: a switch, a number in its range, a
choice, a name. A change is checked by `Settings::set` (which knows a
toggle from a number) and kept in `config/settings.yaml` through
`Preferences` — the very path the hub's own form takes, so the two can
never disagree about what a setting is. Changes that widen what Atlas may
touch or see ask once before they're kept. Anything changed can be put
back. A running Atlas picks most changes up within seconds; the few that
set something up at the start (`settings::NEEDS_A_RESTART`) wait for a
restart, and only then does the page offer one.

Reached from the Atlas window's Settings button, from the Start-menu
shortcut (`atlas home settings`), and by saying "show me settings".

**Public interface:**

- `fn keep_setting`
- `fn when_it_applies`
- `fn put_back`
- `fn current_settings`
- `fn raw_of`
- `fn needs_a_yes`
- `fn why_ask`
- `fn pretty_key`
- `fn key_pressed`
- `struct Asking`
- `struct Page`
- `enum Ask`
- `const KEY_SETTINGS`

### `src/setupwin.rs`

**Wired** — something outside this file calls into it.

Atlas's own window for setting up and starting — the thing a double-click
opens.

One window, no terminal, no browser (Eric's ruling, 17 Sep 2026: the
desktop surface is Atlas's own window). It walks the steps itself and says
each one in words as it goes:

1. a home for Atlas (and its Start-menu and desktop shortcuts),
2. the voice pieces and Tor, fetched and checked (`getpieces`),
3. the Windows Firewall rule for your own devices, asked for once
(`doorrule`; Tor, fetched with the pieces, is how friends reach you),
4. getting to know this computer (`atlas adapt`),
5. checking everything (`doctor`), keeping only what you can act on,
6. your phone — a code to scan, over Tailscale (`phonelink`).

Then: whether Atlas is running, a button to start it, and a switch to start
it with Windows. Everything is safe to run again; opening the window later
walks the same steps, and the ones already done say so at once.

The mark — Atlas's line — is at the top: drawing in when the window opens,
the slow thinking wave while work is going on, still when it's done.

**Public interface:**

- `fn for_you_to_look_at`
- `fn walk_the_steps`
- `fn run`
- `fn here`
- `struct Step`
- `struct Progress`
- `struct FeedbackForm`
- `struct Place`
- `enum StepState`
- `enum Phone`
- `const RUNNING_WORDS`

### `src/shakedown.rs`

**Wired** — something outside this file calls into it.

The commissioning shakedown: walk everything that has never run on this
machine and verify as much of it as can be verified without you.

`capability::how_verified` says, for each never-run capability, *how* it
would be checked. This turns that into an actual pass: the read-only checks
run now (is there a screen, can it read the active window), the ones with a
visible side effect are queued for a one-tap "go ahead" (Atlas opens or
moves something, reads the result back, and undoes it), the data-and-logic
ones are confirmed as you use them against your real files, and the handful
that need your eyes are named. The point is that your part is a short,
guided pass — not days of watching every feature by hand.

**Public interface:**

- `fn report`
- `fn all_clear`
- `struct Step`
- `enum Outcome`

### `src/shared_task.rs`

**Wired** — something outside this file calls into it.

A task or deadline, personal or shared with a business.

## Why one type, not two

Building a "business task" and a "personal task" as two separate types
would be exactly the near-identical-module failure this codebase keeps
naming as its own recurring mistake -- two things that drift apart in
some small way nobody notices until it matters. A task is a task; what
changes is whose it is, and `earned::Space` already exists to say that.
The personal to-do list this gives Eric for free is not a second
feature bolted on afterward -- it is the same list, under
`Space::Personal`, that a business's shelf is under `Space::Business`.

## Two ways a task ends up in a business's space, and only one of them
is a crossing

A task created directly for a business (`add(Space::Business(name),
...)`) never touched anything personal -- it is business-native from
the start, the same as a client asking for something by name, and the
firewall was never going to stop a business's own material.

Sharing an *existing personal task* into a business is the real
crossing -- `share_into_business`. And per `firewall.rs`'s own rule,
**a personal-sourced crossing is never immediately allowed.** Every
first attempt is held, notified, and paused -- `check` has no path that
lets `Space::Personal` through on the spot. So this file does not treat
`Crossing::Allowed` as the normal outcome of a share; it treats
`Crossing::Stopped` as the normal outcome, records which task was
waiting on which hold, and only actually copies the task across once
Eric releases that specific hold -- `complete_release`, called after
`Firewall::release`. The firewall itself never stores what it stopped,
only what it was called and where it was going, so the payload has to
be kept on this side until the release comes back.

Sharing copies rather than moves. The personal original stays personal
and stays yours -- sharing it does not make it stop being tracked on
your own side, any more than telling someone a thing makes you forget
it.

**Public interface:**

- `struct Task`
- `struct Tasks`

### `src/signals.rs`

**Wired** — something outside this file calls into it.

Where the self-audit signals come from.

`selfaudit` knew how to read signals and recommend from them. Nothing built
any. `Daemon.signals` was declared, passed to `recommend()`, and never
pushed to — so asking Atlas what it should fix about itself returned
nothing, every time, not because nothing was wrong but because the vector
was empty.

That is a different failure from an unwired module. `selfaudit` *is*
reachable; `tests/wiring.rs` is satisfied and always would be. A wired
consumer with no producer looks identical to a working one from every
angle except the answer it gives.

So this derives signals from records the daemon already keeps. Nothing new
is measured here — that is deliberate. A producer that needs new
instrumentation is a producer that ships later, and an empty audit shipping
now is what caused the problem.

**Public interface:**

- `fn from_undo`
- `fn from_misunderstandings`
- `fn from_unused`
- `fn gather`
- `const REGRET_WINDOW_SECS`

### `src/signin.rs`

**Wired** — something outside this file calls into it.

Signing you in.

This is a password manager with autofill, which is ordinary software —
1Password and Bitwarden and your browser all do it. Framed that way, most
of my earlier hesitation was misplaced, and the parts that weren't are
design constraints rather than reasons not to build it.

Three of those constraints do real work:

**It fills on the domain and nowhere else.** This is the actual security
benefit over you typing it: a person types their password into
`paypa1-secure.com` because it looks right. Nothing here will, because it
matches the registered domain and not the look of the page. Autofill is
better phishing protection than a careful human.

**Signing in is not permission to change security settings.** Two separate
grants, and holding the first never implies the second. This is what stops
"log me into my bank" from becoming "and now you can move money".

**Access is per-site and revocable from one page**, without touching
anything else and without your having to remember what you granted.

**Public interface:**

- `fn registered_domain`
- `fn hub_rows`
- `fn spoken`
- `fn probably_changed`
- `struct Grant`
- `struct Use`
- `struct SignInConfig`
- `struct Access`
- `enum Allowed`
- `enum Refused`
- `enum Which`
- `const QUIET_AFTER_DAYS`
- `const QUIET_EVERY_DAYS`
- `const BANKS_ARE_SITES_TOO`
- `const WHY_SAFER`
- `const SIGNIN_IS_NOT_SETTINGS`
- `const IF_YOU_HAVE_ONE`

### `src/smtp.rs`

**Wired** — something outside this file calls into it.

SMTP, from scratch, over whatever transport you hand it.

The only two things Atlas is allowed to use this for: unsubscribing
(the one send that never needs asking — Eric's rule, not a default
this module invented) and, once given standing approval, mail to
clients or brands. Neither of those decisions lives here — this
module only knows how to *send*, not when it's allowed to.

Same shape as `imap.rs` on purpose: `Session<S>` is generic over
`Read + Write` so the protocol — command formatting, multi-line reply
parsing — is testable against an in-memory stream, and the one real
socket (`connect`, over TLS) is a single, separately-untestable call
site rather than tangled into the parsing.

**Public interface:**

- `fn connect`
- `fn may_send`
- `struct Session`
- `struct Reply`

### `src/snippets.rs`

**Wired** — something outside this file calls into it.

Text you type again and again -- an address, a signature, a standard
reply -- kept once and typed for you: "type my address", or `;addr`
followed by the expand key.

**Sources:** Espanso (GPL-3.0; read for its ideas only) for triggers and
variables. Espanso expands by reading every keystroke through a system
hook, which is a keylogger by construction; **this doesn't.** Atlas never
watches your typing. Expansion happens only when asked:

- by voice or the command line ("type my signature"), or
- with the expand hotkey (a chord the OS delivers only when pressed,
`RegisterHotKey`): Atlas selects the word just before the cursor, reads
it through the clipboard, puts your clipboard back, and types the
snippet over the word if -- and only if -- it's one of your triggers.

**Soundproofing.** It won't type into an app on your no-input list or the
dictation never-list (`dictate::may_type_into`). An app running as
administrator refuses typed input from Atlas (Windows' UIPI) and that is
said, not swallowed. A snippet that holds a secret-looking string isn't
saved at all -- that's what the vault is for.

**Public interface:**

- `fn fill`
- `fn read_save`
- `struct Snippets`
- `enum Refused`
- `const MAX_CHARS`

### `src/sound.rs`

**Wired** — something outside this file calls into it.

Sound & voice: when Atlas speaks, how loud, and when it may pop up.

The design's Sound & voice page (`design/hub/locked-2026-09-21/Sound.dc.html`)
and the interrupt rule locked with Eric on 20–21 Sep ("pop-ups appear only
when Eric asks, or when it's urgent … tunable: Only when I ask / When it's
urgent (default) / Anything ready"). Each setting here is read where it
acts: `Daemon::say` asks `may_speak_now`, `Daemon::reach_you` asks
`may_pop_up`, and `Voice::speak` scales the synthesised audio by `volume`
before it plays — in-house, on the WAV itself, so no player needs a volume
flag.

**Public interface:**

- `fn minutes`
- `fn scale_wav`
- `struct SoundConfig`
- `const SPEAK_REPLIES`
- `const POPUPS`

### `src/speaker.rs`

**Wired** — something outside this file calls into it.

Turning a recorded turn into something `voiceid` can compare.

`voiceid.rs` was written complete — the comparison, the grey band, the
policy that decides whether Atlas listens rather than whether Atlas is
allowed — and it has never once run. Its own doc comment says embeddings
come from "an external speaker-encoder"; no such encoder existed anywhere
in the tree, no tool was configured for one, and `VoiceId::check` had no
caller. The capability was reachable, tested, and did nothing.

It is worth being precise about why the wiring guard missed it, because it
is the sharpest example of that blind spot in this codebase: `voiceid` left
`UNWIRED_BASELINE` when `recall.rs` started calling `voiceid::cosine` to
compare *notes*. One vector-maths helper, borrowed for an unrelated job,
made the whole module count as wired while the speaker identification it
exists for still had no path to it. Reachability and "actually runs" are
different questions.

This module is the missing half, and it is deliberately the smaller half:
the encoder itself is an external program, the same way speech-to-text and
text-to-speech are. What lives here is the seam — running it over the wav
the turn was already recorded into, and reading the numbers back.

**Public interface:**

- `fn parse_embedding`
- `fn embed`
- `fn which`
- `fn background`
- `fn learn_background`
- `fn still_learning`
- `fn available`
- `fn clip_frames`
- `fn recording_embeddings`
- `struct SpeakerConfig`
- `struct Background`
- `enum Encoder`
- `const NO_ENCODER`
- `const COMPONENTS`
- `const RELEVANCE`
- `const BUILTIN_DIMS`
- `const MIN_BACKGROUND`
- `const GROUP_CENTRED_AT`

### `src/speaking.rs`

**Wired** — something outside this file calls into it.

What Atlas is saying right now, and how loud each moment of it is.

The mark was drawn to move with the voice, but nothing produced a level,
so it never moved when Atlas spoke (doc 19, still open in doc 21). Eric,
24 Sep 2026, item 4: the mark moves when Atlas speaks. Since 27 Sep the
mark is the Folded A, and the level drives its dot (`mark::pose`).

The level comes from the speech itself, not a microphone and not a guess.
Piper writes the whole reply to a WAV file before it's played, so the
loudness of every 30 ms of it is known before the first sound. That
envelope is written beside Atlas's data together with the moment playback
starts, and anything drawing the mark — the Atlas window, the desktop
overlay — reads it and looks up "how loud is the voice now". Nothing is
streamed and no audio device is opened twice; the drawing side only reads
a small file.

**Public interface:**

- `fn caption`
- `fn begin`
- `fn end`
- `fn now_saying`
- `fn levels_of_wav`
- `fn now_ms`
- `struct Speaking`
- `struct Watch`
- `const FRAME_MS`
- `const CAPTION_CHARS`
- `const PLAYBACK_LAG_MS`

### `src/speakthread.rs`

**Wired** — something outside this file calls into it.

Speaking on its own thread.

**Why.** The run loop answers the hub, the typing box and the icon by the
clock. Until 28 Sep 2026 it also played every sentence of a reply itself
(`Mouth::speak` returns when the sentence has been heard), so a long
sentence held all of that for as long as it took to say -- the hub was
answered between sentences, never during one.

Now a reply is handed, sentence by sentence, to a player thread
(`SpeakWork`, the voice's owned twin), and the loop waits on it in slices
of a few milliseconds: answering the hub, watching the talk key and your
voice (`micthread`), and printing each sentence as it starts. Everything
the reply did before still happens, in the same place:

* **cutting in** -- the talk key or your voice stops the player at once
(`micthread::cut_playback`), not at the end of the sentence;
* **"carry on"** -- what wasn't said is kept, *starting with the sentence
that was cut*: a sentence you heard half of was not said (until 28 Sep
2026 it was counted as said, and "carry on" skipped it);
* **one watch per reply** -- the microphone is watched from the reply's
first sentence to its last, including the gaps while the model is still
writing, rather than opened again for each sentence;
* **the next sentence made while this one plays** (Kokoro, `Mouth::prepare`
and `prepare_more`) -- and now it can be, even for a reply the model is
still writing, because the loop is free to hand the next one over;
* **typed and quiet** -- printed, not spoken, a sentence at a time, as
before.

A `Mouth` that can't give an owned voice for a thread (`speak_work` is
`None`: most test stand-ins) is spoken on the loop as it always was, a
sentence at a time with the hub answered between.

**Public interface:**

- `fn wait_quiet`
- `struct Saying`
- `struct Said`
- `const CUT_IN_WAIT`

### `src/speech.rs`

**Wired** — something outside this file calls into it.

Interruptible speech.

Atlas speaks in chunks and listens between them. The industry default is to
stop the moment *any* voice is detected, which gives a false-interrupt rate
you notice — someone coughs, a colleague talks, and the assistant cuts
itself off. Backchannel noises ("mm", "yeah") are the worst case: pure
energy detection reads them as an interruption when they mean "keep going".

So this is deliberately conservative: **only an explicit stop or pause
interrupts.** Everything else is heard, kept, and handled after Atlas
finishes the sentence it is on.

The bug this design has to avoid: recording as *said* something you never
heard. Whatever was cut off is tracked separately so "carry on" resumes
exactly there -- at the start of the sentence that was cut, which you
heard only part of -- and the transcript reflects what actually reached
you. The speaking itself is `speakthread`'s.

**Public interface:**

- `fn is_interruption`
- `fn split`
- `fn acknowledge`
- `struct Delivery`

### `src/spoken_form.rs`

**Wired** — something outside this file calls into it.

Turning written text into something a voice can say.

Piper reads what it is given. Handed "$2.35" it says "dollar two point
three five" or worse; handed "mph" it spells the letters; handed a
markdown table it reads the pipes and dashes aloud. None of that is the
model being bad — it is being asked to speak text that was written to be
*seen*. This is the layer that was missing: everything bound for the
speaker passes through here first, so numbers, money, units and symbols
come out as words and the things that only make sense on a screen are
dropped rather than spelled.

It is deliberately small and rule-based. A general text-to-speech
normaliser is a research project; this handles the cases that actually
turn up in what Atlas says — a price, a percentage, a time, a count, a
unit — and leaves ordinary prose untouched. What it cannot say cleanly it
leaves alone rather than guessing, because a word read oddly is a smaller
failure than a sentence mangled.

Written text is for the eye; spoken text is for the ear, once, with no
chance to re-read. That is the whole reason this exists, and the rule
behind every choice in it.

**Public interface:**

- `fn for_speech`

### `src/spoken_numbers.rs`

**Wired** — something outside this file calls into it.

Numbers, money, times and dates as words, before the speech engine sees
them.

`pronounce` fixed names and tickers; numbers were left to the engine, and
the engine reads "1,250" as "one, two hundred fifty", "$3.50" as "dollar
three point five zero", "14:30" as "fourteen colon thirty" and
"2026-09-24" digit by digit. The classes and their order follow NVIDIA's
NeMo text normalisation (Apache-2.0: money, time, date, ordinal, decimal,
cardinal, measure); the grammar is written here, for English, in house.

**Public interface:**

- `fn cardinal`
- `fn year`
- `fn words`

### `src/srs.rs`

**Wired** — something outside this file calls into it.

Remembering what you want to remember: flashcards on a schedule that
brings each one back just before you'd forget it.

"Make a card: what's the CPI release time | 8:30 Eastern." "Quiz me."
Then "again", "hard", "good" or "easy" (or 1–4) after each.

**Source:** FSRS-5, the Free Spaced Repetition Scheduler
(open-spaced-repetition; the algorithm as its wiki states it, and the
default parameters fsrs-rs ships). Written here from those formulas:

- retrievability `R(t,S) = (1 + 19/81 · t/S)^−0.5` (so `R(S,S) = 0.9`);
- first stability `S₀ = w[G−1]`; first difficulty
`D₀ = w4 − e^(w5·(G−1)) + 1`;
- difficulty `D' = D − w6·(G−3)` damped by `(10−D)/9`, then pulled back
toward `D₀(4)` by `w7`;
- after a recall `S' = S·(e^w8·(11−D)·S^−w9·(e^(w10·(1−R))−1)·hard·easy + 1)`;
- after a lapse `S' = min(S, w11·D^−w12·((S+1)^w13−1)·e^(w14·(1−R)))`;
- a second look the same day `S' = S·e^(w17·(G−3+w18))`.

The interval for a wanted retention `r` is `S/F·(r^(1/−0.5) − 1)`, which
is `S` days at 90%.

**Soundproofing.** A card holding a secret-looking string is refused.
"Quiz me" takes at most 20 due cards (config) so a missed week doesn't
become a wall. Intervals are capped at a year and a half; days are whole
local days, so a card is never due at 3 a.m.

**Public interface:**

- `fn retrievability`
- `fn interval`
- `fn read_card`
- `struct Card`
- `struct Deck`
- `struct SrsConfig`
- `enum Grade`
- `enum Refused`
- `const W`
- `const MAX_INTERVAL`
- `const MAX_CARDS`

### `src/stale.rs`

**Wired** — something outside this file calls into it.

The trade that isn't working. It just hasn't lost yet.

A stop answers "was I wrong". A target answers "was I right". Neither
answers the third thing that happens to most trades, which is **nothing**:
the setup fired, price went nowhere, and the position sits there for two
days paying spread and swap while the reason it was opened quietly expires.

That trade is not a winner waiting to happen. The reason for it was a
reading of the market *at a moment*, and a reading has a shelf life — a
break of structure that was going to run has, by and large, run. What is
left after it doesn't is an open position with no thesis, held because
closing it would make the loss real.

## Why this is not just "close after N bars"

Because N is different on every timeframe and in every market, and a fixed
N is the same mistake as a fixed pip stop. What this does instead is
measure two things off the bars and compare them:

- **How long the market's own movement says it should take.** The target is
some distance away and this market covers some distance per bar. Divide.
That is a pace, and it comes from the series rather than from a guess.
- **How far it has actually got.** The best the trade has been, as a share
of the distance to target. Best, not current — a trade that reached 80% of
its target and came back is a different animal from one that never moved,
and only one of them is stale.

## What is chosen rather than measured, said plainly

Two numbers in [`StaleConfig`] are judgements: how many times the implied
pace to allow, and how little progress counts as none. They are named as
choices, and the honest way to settle them is the same as everything else
here — score closed trades and read the answer off the record. Until that
has thirty of them, these are starting points and this module says so.

## What it will not do

It will not move the stop. A time exit is a decision to leave at market,
and dressing it up as a stop move would confuse two different things: one
is "the trade is wrong", the other is "the trade is nothing".

It also will not fire on a trade that has never had the chance. Under the
implied pace, `Going::TooEarly` — because a rule that can fire on the
second bar is a rule that will.

**Public interface:**

- `fn how_its_going`
- `fn implied_bars`
- `fn spoken`
- `struct Open`
- `struct StaleConfig`
- `enum Going`

### `src/stance.rs`

**Wired** — something outside this file calls into it.

Writing that says something.

Catching faults makes a draft less bad. It doesn't make it good, and the
difference matters: you can remove every hedge and every filler word from a
paragraph and still be left with something that takes no position, supports
nothing, and could have been written about anything.

So this asks a different question. Not "what's wrong with it" but "does it
make a claim, is the claim held up, and is it built in an order that makes
sense for where it's going".

**Public interface:**

- `fn assess`
- `fn brief`
- `fn spoken`
- `fn kind_of`
- `struct Gap`
- `struct Support`
- `enum Kind`
- `enum Missing`
- `enum SupportKind`

### `src/standdown.rs`

**Wired** — something outside this file calls into it.

When Atlas should have no view, whatever the chart says.

## This is a rule, not a reader

`market::events` already knows when every central bank speaks, out to 2027,
with windows that have measurements behind them rather than a vendor's
traffic lights. `inside_blackout` already answers the question.

**Nothing called it.** That is the whole of what this file fixes, and it is
worth being plain about the size of it: the calendar was built, correct, and
inert. A reader that knows when the Fed speaks and trades through it anyway
is a reader that has the information and does not use it — which is
indistinguishable, from the outside and from the results, from not having
the calendar at all.

## Why this earns its place

Most of the losses in a retail FX record that look like bad analysis are not
bad analysis. They are ordinary setups taken into a release: the spread goes
from one pip to eight, the stop is filled four pips past where it sat, and
the trade that would have worked is closed at a loss before the move it
predicted happens.

None of that shows up as a flaw in the reading. The reading was fine. The
record just shows a loss, and a system learning from its record learns the
wrong lesson — it marks down whichever reading happened to fire that day.

## The bar that gets this wrong

> The H4 bar closing at 16:00 covers 12:00–16:00, so it **contains** the
> 12:30 payrolls print, its window and the whole recovery. Its close sits
> three and a half hours clear, so a close-only news check calls that bar
> clean. It is the least clean bar of the week.

That is the second chat's finding and it is the reason this checks a bar's
**span** and not its close. Asking an interval question about an instant
gets the worst bar of the week exactly wrong, and gets it wrong in the
reassuring direction.

**Public interface:**

- `fn standing_down`
- `fn spanning`
- `fn spoken`
- `struct StanddownConfig`
- `enum Blackout`

### `src/startup.rs`

**Wired** — something outside this file calls into it.

Atlas starting itself when you log in.

**Eric's ruling, 17 Sep 2026: yes, in the background.** So the answer to
"how do I start it" should be that you do not — it is already running, and
the launcher is for setup and repair rather than for starting things.

## Why this did not exist, and why that showed

Nothing in this tree created a startup entry, a shortcut, or a scheduled
task. Two modules nevertheless *assumed* one: `onlyone.rs` opens with
"Start it from the shortcut, forget, start it again from the batch file,
and there are two", and `doctor.rs` explains that the install root "used to
hang off the current working directory, so a shortcut with the wrong
`Start in` broke it". Both were written about a shortcut nobody could have
had, because nothing made one.

That is the quiet kind of gap: the code reads as though the feature exists,
nothing fails, and the person just keeps opening the folder by hand.

## The one that would have bitten

**A scheduled task does not start in your install folder.** Windows starts
it in `system32`, and a logon task has no inherited working directory worth
anything. Before 17 Sep that would have been fatal in a way nobody would
have diagnosed: `Store::new("data/state")` resolved relative to the
process's working directory, so Atlas would have come up with an empty
memory, written a `data/` tree into `system32`, and reported nothing wrong.

`roots::decide()` fixes it by resolving from `current_exe()` rather than
from where the process was started, and `tests/one_install_root.rs` keeps
it that way. This module is only safe *because* that landed first, which is
worth writing down: the ordering was luck, not planning.

## What it does NOT do

It does not run Atlas elevated. `LeastPrivilege` in the task's XML (it was
`/RL LIMITED` before 28 Sep 2026) is deliberate — an assistant
that holds your microphone and reads your screen has no business running as
administrator, and a task created with `/RL HIGHEST` would also need an
elevated shell to create, which would make "set this up for me" a UAC
prompt. Nothing here needs admin.

It does not install a service. A service runs without a desktop session,
and Atlas needs one: the panels, the microphone and the screen reading are
all session-bound. A logon task is the honest shape.

**Public interface:**

- `fn task_command`
- `fn register`
- `fn task_file_path`
- `fn task_xml`
- `fn task_file_bytes`
- `fn write_task_file`
- `fn run_entry_add`
- `fn run_entry_remove`
- `fn turn_on`
- `fn turn_off`
- `fn decision_file`
- `fn decided`
- `fn remember`
- `fn after_setup`
- `fn remove`
- `fn whether_registered`
- `fn unit_file`
- `fn unit_path`
- `fn run`
- `struct Plan`
- `struct AfterSetup`
- `enum Mode`
- `const TASK_NAME`

### `src/stemmer.rs`

**Wired** — something outside this file calls into it.

"trading", "trades" and "traded" are one word to a person. Porter2 makes
them one word to recall.

**Source:** the Snowball English (Porter2) stemmer, Martin Porter —
`snowballstem/snowball`, BSD-3-Clause; `CurrySoftware/rust-stemmers` (MIT)
read as a reference. Clean-room from the published algorithm description
(snowballstem.org/algorithms/english/stemmer.html).

**Why Atlas wants it.** `recall::word_score` matches `*w == q` — exact
words. A note that says "the trades closed early" is invisible to "what
did I trade". The meaning encoder papers over it when installed; this makes
the words path right on its own, which matters because the words path is
the one that always runs (no model, no download).

Stemming is for *matching*, never for display: a stem like "happili" is
not a word and is never shown to anyone.

**Public interface:**

- `fn stem`
- `fn stems_of`

### `src/store.rs`

**Wired** — something outside this file calls into it.

Durable local state. Atomic writes: temp file then rename, so a crash
mid-save leaves the previous good copy rather than a truncated one.

**Public interface:**

- `fn now`
- `fn set_aside_since`
- `fn set_aside_sentence`
- `fn tell_set_aside`
- `struct Store`
- `struct SetAside`
- `const SCHEMA`

### `src/strategy.rs`

**Wired** — something outside this file calls into it.

Trying different things, not the same thing repeatedly.

"Three attempts then give up" was a bad rule, and you were right to push on
it. Three attempts is only meaningful if they're three *different*
attempts — and left to itself a model will happily retry the same idea
with the wording changed and call it a second try.

So Atlas works through a ladder of genuinely distinct approaches. Each one
looks at the problem from a different angle, each is only used once, and it
stops when the ladder runs out rather than after an arbitrary count. That
means a hard problem gets ten real attempts instead of three lazy ones, and
an impossible one still terminates.

**Public interface:**

- `struct StrategyConfig`
- `struct Effort`
- `struct Campaign`
- `enum Angle`
- `enum Next`

### `src/subject.rs`

**Wired** — something outside this file calls into it.

Working out what "this" is.

You say "explain this". The clipboard is empty. A system that answers
"there's nothing on the clipboard" is technically correct and useless —
you were obviously looking at something.

So "this" is resolved against everything Atlas can see, cheapest first:
what you copied, what you have selected, the window in front of you, the
file you just opened, the thing you were last talking about. Each source
carries a confidence, and when two are equally plausible Atlas asks which
rather than picking.

**Public interface:**

- `fn wants`
- `fn resolve`
- `fn confirm`
- `struct Candidates`
- `enum Subject`
- `enum Resolution`
- `enum Wants`

### `src/sync.rs`

**Wired** — something outside this file calls into it.

Atlas on more than one device, without the copies drifting apart.

I argued against this and I was wrong about the shape of the problem. What
I was worried about — two copies of your state disagreeing — is a real
problem if you sync *state*. It mostly disappears if you sync **what
happened**.

Every device keeps an append-only log of events: you said this, a note was
captured, a task was finished. Merging is replaying both logs in order.
Appending can't conflict with appending, so two devices that have never
seen each other for six months merge cleanly. Only an edit to the same
thing on both sides needs a decision, and for one person that is rare.

That means the phone can be a real Atlas rather than a window: it listens,
it talks, it thinks with a smaller model, it remembers. What it can't do is
touch your laptop's files and windows, which nobody expects of a phone.

## Getting the log across

The log is one file. Anything that can move a file can sync Atlas: the same
wifi, a cloud folder, a cable, or AirDrop. There is no server anywhere in
this.

**Public interface:**

- `fn merge`
- `fn make_bundle`
- `fn from_the_same_atlas`
- `fn can_open`
- `fn already_seen`
- `fn how_to_carry`
- `fn spoken`
- `fn drifting`
- `fn new_key_phrase`
- `fn key_from_phrase`
- `fn seal`
- `fn peek`
- `fn read_bundle`
- `fn read_bundle_for`
- `fn card_path`
- `fn recovery_card`
- `fn write_card`
- `fn phrase_in_card`
- `fn set_key`
- `fn new_key`
- `fn ensure_key`
- `fn made_one`
- `fn write_whole`
- `fn leave_handoff`
- `fn take_handoff`
- `fn sweep_handoffs`
- `fn best_folder`
- `fn route_of`
- `struct Device`
- `struct Event`
- `struct Log`
- `struct Merged`
- `struct Clash`
- `struct Bundle`
- `struct SyncConfig`
- `struct KeptKey`
- `struct SealedBundle`
- `struct KeySetup`
- `struct KeyHandoff`
- `enum Kind`
- `enum What`
- `enum Carry`
- `enum Reader`
- `const BUNDLE_VERSION`
- `const NOT_A_FILE_YOU_MANAGE`
- `const WHY_THIS_WORKS`
- `const WHAT_THE_PHONE_MISSES`
- `const KEY_CONTEXT`
- `const KEY_FILE`
- `const SEALED_VERSION`
- `const NO_KEY_YET`
- `const HANDOFF_CONTEXT`
- `const HANDOFF_VERSION`

### `src/system.rs`

**Wired** — something outside this file calls into it.

Changing your machine.

Arranging the desktop, moving files where they belong, setting a wallpaper,
changing a setting when you ask or when Atlas needs it to work.

The thing that makes this safe isn't a list of allowed actions — it's
sorting every change by **how hard it is to undo**. A wallpaper is nothing:
Atlas records the old one and can put it back. A moved file is undoable,
because it goes through the trash. A network or account setting is not, and
Atlas doesn't touch those at all.

Reversibility decides the gate, not how impressive the action sounds.

**Public interface:**

- `fn reversibility`
- `fn judge`
- `fn describe`
- `struct SystemConfig`
- `enum Change`
- `enum Undo`
- `enum Verdict`
- `const NEVER`
- `const SAFE_SETTINGS`

### `src/taste.rs`

**Wired** — something outside this file calls into it.

Design taste, the part of it a machine can actually hold to.

Taste has no compiler. There is no tool that reads a page and says "this is
well designed", the way `cargo check` says "this compiles" — so the honest
move is not to pretend otherwise, but to split taste into the part that IS
checkable and the part that isn't, and to be rigorous about the first.

What *is* checkable is consistency and correctness against a stated house
style: is the spacing on the scale, are colours coming from tokens rather
than typed-in hex, does every image have alt text, does every control have a
name. None of that is "beautiful" — but a page that breaks these reads as
careless no matter how good the underlying idea is, and a page that keeps
them reads as considered. This module is that checkable part, and nothing
more: it is the design equivalent of `craft`'s ladder, a gate a draft can be
iterated against, not a claim to judgement it does not have.

The part that isn't checkable — is this the right layout, does it feel right
— stays where it belongs: with a person, or a stronger model. `review` never
speaks to that, and a page that passes every rule here is "consistent and
accessible", never "good". Saying more than that would be the same overclaim
`build_it` and `selfwork` exist to prevent, one domain over.

**Public interface:**

- `fn review`
- `fn blocking`
- `fn spoken`
- `fn build_web`
- `fn wants_web_page`
- `struct Finding`
- `struct Rules`
- `enum Severity`
- `enum Outcome`
- `const WEB_SYSTEM`
- `const WEB_FIX_SYSTEM`

### `src/telegram.rs`

**Wired** — something outside this file calls into it.

Reading your Telegram, which is the one chat service that lets you.

# Why this one

`messaging.rs` has had a truthful table of six platforms since the day it
was written, and `Platform::what_it_permits` says of each what you can
actually do. Two of the six can never work for a personal account —
WhatsApp's interface is for businesses and costs per message, Signal is
deliberately closed — and the tools claiming otherwise drive the desktop
app while pretending to be you, which gets accounts banned. Atlas does not
do that.

Telegram and GroupMe have real interfaces. This is Telegram's, because it
is the one you are most likely to already use and the setup is two minutes
with BotFather.

# This is an online, secondary capability, and it says so

Everything primary in Atlas works with the network unplugged. This cannot:
the messages are on somebody else's server. So it is off by default, it is
marked `offline: false` in the catalogue, and nothing else depends on it —
a machine with no network loses this and keeps everything.

# What a bot can and cannot see, said plainly

A Telegram bot is not you. It sees:

* messages sent directly to it;
* messages in a group it has been added to — and by default **only those
that name it**, unless you turn privacy mode off in BotFather.

It does **not** see your existing one-to-one conversations with other
people. Nothing can, short of logging in as you, which is the thing this
module exists not to do. So this is useful for a channel you point at
Atlas, and it is honest about not being your whole inbox.

# The token

It goes in the vault, never in `tools.yaml`. A token in a config file is a
token in your backups, your sync folder and any screenshot of your
settings — and this one can read and send as the bot.

**Public interface:**

- `fn into_messages`
- `fn read_up_to`
- `fn updates_path`
- `fn fetch`
- `fn looks_like_a_token`
- `struct TelegramConfig`
- `const TOKEN`
- `const KEPT`
- `const READ_UP_TO`
- `const HOST`
- `const HOW_TO_SET_UP`

### `src/thread.rs`

**Wired** — something outside this file calls into it.

One conversation, forever.

This is the largest single difference between what Atlas was and what you
pictured. Jarvis never starts a session — Tony walks in mid-thought and it
continues. No greeting, no re-explaining, no "how can I help you today".

So there are no sessions here. There is one thread that spans days. It
grows, gets folded into a summary as it grows, and picks up where it left
off. The only thing that resets is what fits in the model's context, and
that is a compression problem, not a conversation boundary.

**Public interface:**

- `fn now_secs`
- `fn must_keep`
- `fn with_the_important_kept`
- `fn plain_fold`
- `struct Exchange`
- `struct ThreadConfig`
- `struct Thread`
- `const FOLD_PROMPT`

### `src/tier.rs`

**Wired** — something outside this file calls into it.

How much work an answer needs.

`route` picks *how* to do a task once Atlas has decided to do it. This
decides something earlier and cheaper: whether the task needs doing at all.

Three tiers, and the middle one is the point.

* **Run a named thing.** You said the name of something Atlas already
knows how to do. There is nothing to work out — execute it and say when
it is done.
* **Read from what's already there.** You asked about something Atlas
worked out this morning. The answer is sitting in a report; going away to
think about it again would be slower and no better.
* **Actually think.** Everything else. Slow because it should be.

Most questions are tier two and get treated as tier three, which is why a
local assistant can feel sluggish on questions it already knows the answer
to. Waking a model to re-derive something computed an hour ago is the
single most common way a voice assistant wastes your time.

**The fall-through only goes one way.** Guessing too low means answering
from something stale and being confidently out of date. Guessing too high
means being slower than necessary. So anything uncertain goes up, and a
report that cannot be shown to be current is not used at all.

**Public interface:**

- `fn tier_for`
- `struct Report`
- `struct Mix`
- `enum Tier`

### `src/timebox.rs`

**Wired** — something outside this file calls into it.

Stopping before you have to ask what's taking so long.

Phase 3. An assistant that works on something indefinitely is worse than
one that gives up, because you can't tell the difference between thinking
and stuck.

So every piece of work gets a budget, and the budget is spent out loud: it
says how long it expects to take, tells you when it's over, and stops with
what it has rather than with nothing.

**Public interface:**

- `fn size_of`
- `fn usual_secs`
- `fn estimate_worth_saying`
- `struct Box_`
- `enum Size`
- `enum State`
- `enum Stopped`

### `src/timing.rs`

**Wired** — something outside this file calls into it.

How long a turn actually took, and where it went.

Nothing measured this. `perf` decides how hard to work when the machine is
busy; `budget` counts what a model call costs. Neither answers the question
you ask when it feels slow: **which part was slow?**

Without that, every latency improvement is a guess. You can shorten the
wake word, warm the model, stream the speech — and have no way to tell
which of the three did anything, or whether the thing you shortened was
ever the problem.

It is also the missing producer for `selfaudit::Kind::GotSlower`, which has
sat in the taxonomy since it was written with nothing able to raise it. A
signal nothing can produce is a signal that will never fire.

## What it deliberately doesn't do

No averages. A mean turn time hides the turn that took nine seconds, and
the nine-second turn is the entire complaint — nobody has ever been annoyed
by an average. It keeps the worst recent turns and the typical one, which
are the two numbers that answer different questions.

No storage. This lives in memory and is lost on restart. Turn latency is
only interesting while it is happening or shortly after, and writing it to
disk would make the thing that measures overhead into a source of it.

**Public interface:**

- `struct Turn`
- `struct Recent`
- `enum Stage`
- `const FEELS_SLOW_MS`
- `const KEEP`

### `src/toast.rs`

**Wired** — something outside this file calls into it.

Windows notifications that stay in the Action Center.

Atlas's own panel shows an alert while it's on screen and then it's gone;
if you were away from the desk, nothing is left to find. A Windows toast
goes to the Action Center and waits there. Raised through WinRT directly
(the calls tauri's winrt-notification wraps, MIT/Apache-2.0), not through
PowerShell, so no console flashes.

An unpackaged program needs a registered app ID or Windows drops its
toasts silently; Atlas registers its own ("Atlas") under the current
user the first time, which needs no administrator rights.

**Public interface:**

- `fn xml`
- `fn show`
- `const APP_ID`

### `src/together.rs`

**Wired** — something outside this file calls into it.

Three trades that are one bet.

## One opinion counted three times

Two opinions that share an input are one opinion counted twice. Positions
are the same: **long EURUSD, long GBPUSD and long AUDUSD is not three
positions. It is short dollar, three times.**

It has every property that makes a failure expensive:

- The risk rules pass. Each trade is one per cent, and one per cent is the
rule.
- The trade log looks diversified. Three instruments, three setups.
- It only shows up on the days it matters, when one dollar print takes all
three out together and the account loses three per cent from a rule that
said one.

And it gets **worse** as Atlas gets better, which is the part worth sitting
with. A reader that is genuinely good at spotting dollar strength will
spot it on every dollar pair at once, and will be right, and will put on
three correlated trades because it was right. Skill concentrates this risk
rather than diluting it.

## How it is measured

Not with a correlation matrix. A correlation is a number about the past
that needs a long history and is unstable exactly when it matters — every
correlation goes to one in a crisis, which is the day the number was
supposed to warn you.

Instead: a pair is two currencies, and a position is an opinion about both
of them. Add up the opinions. That is not a model of anything; it is
arithmetic on what is actually held, and it cannot be wrong about the past
because it is not about the past.

**Public interface:**

- `fn legs`
- `fn net`
- `fn if_i_add`
- `fn spoken`
- `struct Position`
- `struct Netted`
- `struct TogetherConfig`
- `const PAIRS`

### `src/tools.rs`

**Wired** — something outside this file calls into it.

External tool runner.

Every heavy capability Atlas needs — audio capture, speech-to-text,
speech synthesis, playback, screen capture — is a free single-binary
program that already exists and is better than anything we'd write.
So Atlas shells out to them through a config-declared template instead
of linking bindings.

Consequences that matter:
* swapping Whisper for Vosk, or Piper for Windows SAPI, is a YAML edit
* nothing here is Windows-specific, so it is testable on any machine
* a missing binary is a clear error, not a link failure

**Public interface:**

- `fn poll_gap`
- `fn expand`
- `fn curl_in_process`
- `fn which`
- `fn command`
- `struct ExternalTool`

### `src/trace.rs`

**Wired** — something outside this file calls into it.

A flight recorder for every model call.

The hosted tools that do this — Helicone, Phoenix, Langfuse — are proxies
built to watch cloud API traffic across a team. Atlas is one process, on
one machine, calling a model on the same disk. There is no network hop to
proxy and no team to share a dashboard with, so the whole thing is an
append-only file and a reader.

It earns its place for four reasons, in the order the pain arrives:

1. **You cannot debug what you cannot see.** When an answer is wrong next
month, the question is what Atlas actually sent and with what loaded.
Without a record that is unanswerable.
2. **It closes the correction loop.** `mending`'s scoreboard is "does it
make the same mistake twice", and counting that needs a record of the
first time.
3. **It is how a local model earns trust.** Whether the 7B is good enough
or a question needs the 14B is a measurement, not an opinion.
4. **Without it every prompt change is a vibe.**

One line per call, newest last, never rewritten. A log that gets rewritten
is a log you cannot trust, and the entire value here is trust.

**Public interface:**

- `fn to_line`
- `fn from_lines`
- `fn log_path`
- `fn append`
- `fn load`
- `fn compact`
- `fn open`
- `fn keep_bounded`
- `fn model_name`
- `fn wilson`
- `fn grades_path`
- `fn grade_and_keep`
- `fn examples_path`
- `fn keep_example`
- `fn examples`
- `fn scrub`
- `fn example_of`
- `struct Call`
- `struct Trace`
- `struct Score`
- `struct TraceConfig`
- `struct Words`
- `struct Example`
- `const STORES_NO_CONTENT`
- `const KEEP`
- `const ENOUGH_TO_MEASURE`

### `src/tradeday.rs`

**Wired** — something outside this file calls into it.

A trading day's process, kept: a short check before the session and a
short journal after it -- about **you and your process**, never about a
trade.

**Sources:** the pre-market checklist and post-session journal prompts
common to trading-journal practice (FX Replay's "5 journal prompts",
TradesViz on psychology tracking) were read for the shape: a handful of
yes/no questions on readiness and rule-following, a 1–5 state rating,
one free line. Kept to that. The day's scheduled releases come from
`marketdays`, as a schedule.

**What it will never do.** No question asks about a position, a level or
a direction, and no summary reads one. The summary counts what you
answered -- days checked in, rules kept, the state you gave -- and ends,
with the line that says so:
`MEASUREMENTS. NO VERDICT. YOU DECIDE. · NOT FINANCIAL ADVICE`.

**Public interface:**

- `fn ask`
- `fn read`
- `struct Question`
- `struct TradeDayConfig`
- `struct Entry`
- `struct Journal`
- `enum When`
- `enum Answer`
- `enum Given`
- `const THE_LINE`
- `const NEVER_SAYS`

### `src/translation.rs`

**Wired** — something outside this file calls into it.

Translation on this machine: "translate this into Spanish", "what does
this say in English?".

**Sources:** Mozilla's Firefox Translations (the Bergamot project, local
models, MPL-2.0) was read for the shape of an offline translator -- whole
sentences in, the text cut at sentence boundaries into bounded pieces --
and for its honesty about quality. Atlas already runs a local model, so
that model translates; no new download and nothing leaves the machine
unless you've set a stronger model as the fallback yourself.

**Soundproofing -- the checks a model can't talk its way past.** A
language model translating fails quietly: it drops a sentence, changes a
number, "helpfully" answers the text instead of translating it. So every
result is checked, cheaply, before it's handed over:
- **Numbers, times, amounts, links and email addresses** in the original
must all be in the translation, exactly. A missing one is named.
- **Length**: a translation under a third or over three times the
original's length is flagged -- the dropped-paragraph case.
- **Echo**: a "translation" identical to the original is flagged, not
passed off.
- Optionally (`back_check`, on for short texts by default) the result is
translated back and compared word-for-word; low overlap is said as
"the meaning may have drifted", with the back-translation shown.

Text is cut at paragraph and sentence ends into pieces of at most
`MAX_PIECE` characters, so a long document never overruns the model's
context and one bad piece doesn't take the rest down.

**Public interface:**

- `fn language_named`
- `fn read`
- `fn translation_pieces`
- `fn fixed_tokens`
- `fn check_translation`
- `fn word_overlap`
- `fn translation_prompt`
- `fn translate`
- `struct TranslateConfig`
- `struct Translated`
- `const MAX_PIECE`
- `const MAX_TEXT`
- `const SYSTEM`

### `src/transport.rs`

**Wired** — something outside this file calls into it.

Sending a bundle straight to your other Atlas on the same network.

`sync` says any carrier that moves a file can sync Atlas, and the folder is
the workhorse. But when your phone and your laptop are on the same wifi, the
folder is a slow way to move something across the room: it waits on a cloud
provider to notice, upload, and hand back down. `nearby::look` already finds
the other machine by shouting on the network and hearing it answer — what
was missing was the last step, actually handing the bundle across. The
daemon even said so out loud: "sending straight across isn't built yet."

This is that. A tiny framed protocol over a plain TCP connection: connect to
the peer, send it the bundle, get its bundle back. One round trip is a full
two-way sync — you learn what it did, it learns what you did — so a phone
coming back onto the home wifi is caught up in the time it takes to open a
socket, with no folder, no cloud, no server, and nothing on the internet.

The payload is opaque here on purpose. `sync::seal` / `sync::read_bundle`
decide sealed-or-plain exactly as they do for the folder, and this module
just moves the bytes — so the wire is protected by the same household key,
and the transport has no idea what a bundle is. Offline-first to the core:
the only thing it needs is a local network, and it degrades to the folder
when there isn't one.

Style note: the receiver is a `poll`, not a thread that blocks on `accept`.
Atlas advances a step at a time on one clock — the same reason `run_the_night`
is written as "advance by one" — so the daemon asks this "is anyone waiting?"
once per tick and never blocks the turn that also answers you.

**Public interface:**

- `fn exchange`
- `struct Server`
- `const SYNC_PORT`
- `const MAX_FRAME`

### `src/tray.rs`

**Wired** — something outside this file calls into it.

Things you handed Atlas, from wherever you were standing.

You find a link on your phone. Today that means mailing it to yourself, or
remembering it, or losing it. What you want is to hand it over and have it
be waiting, read, when you sit down.

The pieces for the reading already existed: `browser.rs` drives a real
headless Chrome, `research.rs` fetches a page and strips it down. What was
missing was somewhere to put a thing before Atlas gets to it, and a rule
about what handing something over does and does not mean.

## The rule that matters

**Handing Atlas a link says "look at this". It never says "do what this
says."**

That distinction is the whole safety property here, and it is easy to lose
by accident: the natural next step after fetching a page is to feed the text
to the part of Atlas that works out what to do, and at that moment any page
on the internet can issue Atlas instructions in your name. So fetched text
is stored as something to be *shown*, never parsed into an intent, and
`tests/tray.rs` fails the build if a route from `found` to the parser ever
appears.

## Why it waits

An item is read, then offered. Whether Atlas goes further — replies to it,
files it, acts on what it found — is a question for `earned`, per kind of
work, exactly like anything else it does on its own. Dropping something in
is not consent to act on it, and a tray that acted immediately would be a
way to get around every bar Atlas has.

## Where it syncs

Nowhere new. Your phone already reaches your desktop through the hub's own
door — same person, same machine, a token you already hold. `kin.rs` exists
because another Atlas is a different trust boundary; your own phone is not,
and inventing a second sync path for it would mean two doors to keep honest
instead of one.

**Public interface:**

- `fn from_base64`
- `struct Item`
- `struct Tray`
- `enum Sort`
- `enum State`
- `const FILE`
- `const KEEP_DONE`
- `const MAX_LEN`
- `const FOLDER`
- `const MAX_FILE_BYTES`

### `src/triage.rs`

**Wired** — something outside this file calls into it.

Sorting a full inbox by what it actually asks of you.

Phase 3. Every mail client sorts by sender, date or folder, which are all
facts about the message rather than about you. The only question that
matters is **what does this need from me, and by when** — and that cuts
across all three.

Nothing here sends anything. Triage reads, sorts, and drafts; sending stays
where it was, behind approval that's checked again at the moment of send.

**Public interface:**

- `fn imap_date`
- `fn triage`
- `fn sort_all`
- `fn spoken`
- `fn can_wait`
- `struct Message`
- `struct Triaged`
- `struct Corrections`
- `enum Needs`

### `src/tts.rs`

**Wired** — something outside this file calls into it.

Atlas's voice, and changing it by asking.

Every voice here is free. Piper is MIT-licensed and its voices are free
downloads — no account, no key, nothing to sign up for. "Model" in this
project has never meant "paid"; it means a file on your disk.

You should be able to say "a bit slower", "try a deeper one", "use a
British voice" and have it change, then and there, without going to look
for a setting. That is what this is.

**Public interface:**

- `fn catalogue`
- `fn find`
- `fn interpret`
- `fn adjust`
- `fn audition_line`
- `struct Voice`
- `struct VoiceSettings`
- `struct EngineConfig`
- `enum Quality`
- `enum Change`
- `enum Engine`
- `const SHORTLIST`
- `const KOKORO_HAS_NO_RANGE`

### `src/tune.rs`

**Wired** — something outside this file calls into it.

Looking after the laptop it lives on.

Your machine isn't optimised, and on 15.7GB of shared memory that stops
being cosmetic. But "optimising Windows" is also the single most common
excuse for software to do something reckless, so this is deliberately
narrow:

* It **finds** things — startup programs you never use, junk that can be
deleted, memory being held by something you forgot was open.
* It **explains** each one with the actual number, so you can disagree.
* It only **acts** on things that are reversible or genuinely disposable.

The named Windows mechanisms behind these findings live in `checks`, which
also holds the permanent no-list. Registry cleaners are still how people
break their machines, and they are on it.

**Public interface:**

- `fn examine`
- `fn actionable`
- `fn worth_it`
- `fn summary`
- `fn storage_plan`
- `fn mechanism_for`
- `fn other_drives`
- `fn move_folder`
- `struct Finding`
- `struct Survey`
- `struct TuneConfig`
- `struct StoragePlan`
- `struct Moved`
- `enum Fix`
- `const TEMP_FILES`
- `const MOVED_RECORD`

### `src/twofactor.rs`

**Wired** — something outside this file calls into it.

Two-factor codes: Atlas typing them in for you.

Eric, 25 Sep 2026 (B1): "I want to be able to use Atlas for two factor",
options 1 and 2. Either you read the code out and Atlas types it, or
Atlas finds it in your email or your texts and types it. Turning
two-factor on or off is the other half, and it lives in `confirmed`
(read-back, then your yes, with you at the machine).

What this file decides is the part that can go quietly wrong:

- **Which number is the code.** A security email has a lot of numbers
in it: the year, a phone number, an order number, a price. A code sits
next to words like "code" or "verification", it is 4 to 8 digits, and it
is not a year, a price or a percentage. When more than one fits, the one
nearest those words wins.
- **Which message is the right one.** Only codes from the last few minutes
count (they expire anyway), and one from the site you're signing in to
beats a newer one from somewhere else.
- **Where it goes.** Into the code box on the page, including the kind
that is six separate one-digit boxes. Never into a password box.

Nothing here sends a code anywhere but the box it was asked for.

**Public interface:**

- `fn source_of`
- `fn code_in_words`
- `fn code_in_text`
- `fn newest`
- `fn from_phone_link`
- `fn asks_for_code`
- `fn code_box_js`
- `fn code_box_result`
- `fn read_out`
- `fn ask_for_it`
- `fn none_found`
- `struct Found`
- `enum Target`
- `enum Source`
- `enum Filled`
- `const FRESH_FOR_SECS`

### `src/typebox.rs`

**Wired** — something outside this file calls into it.

The typing box (Eric's ruling H1): press the key, a one-line box opens
over whatever you're doing, type, press Enter, and it's gone.

Its own small process (`atlas typebox`), like the overlay, so the box can
never stall the background Atlas and the other way round. What you type is
printed on one line to its output, which the background Atlas reads as a
typed turn — the same door as the console.

The box follows `quickinput::QuickInput`: Enter sends, Enter on nothing or
Escape closes, Backspace takes a letter back, and a box you opened and
wandered away from closes by itself after `quick_input.idle_close_secs`.
Its look is plain on purpose: the panels' look waits on the hub's design
(H2), and this is the working part.

**Public interface:**

- `fn apply_keys`
- `fn typed_line`
- `fn run`
- `struct Standby`
- `const SENT`
- `const TITLE`

### `src/typed.rs`

**Wired** — something outside this file calls into it.

Something you **type**, in a codebase that is otherwise spoken to.

One module, because there is exactly one thing in Atlas that a microphone
must never carry: the vault passphrase. `handover::take_back` rests on it,
`vault::Vault::open` checks it, and both of those would be decoration if
the way it arrived were "say your passphrase out loud in the room where
you have just handed your laptop to somebody else".

So a spoken phrase may *summon* this prompt. It can never answer it.

# Why a trait and not a function

`daemon.rs` is a library module that tests drive thousands of times a run.
A function that reads stdin would make the daemon untestable at exactly
the point worth testing — and worse, would make the tests hang rather than
fail, which is the failure mode people work around by deleting the test.

The daemon therefore holds an `Option<Box<dyn AsksQuietly>>`. `None` is
the honest default and the one the tests get by construction: no terminal
was wired up, so Atlas says so and names the command that has one, rather
than pretending to have asked. The binary installs `Console`.

**Public interface:**

- `fn ask_quietly`
- `fn how_typing_is_hidden`
- `struct Console`

### `src/typos.rs`

**Wired** — something outside this file calls into it.

Forgiving a typo without matching everything.

**Source:** Meilisearch's typo-tolerance rules (`meilisearch/meilisearch`,
Community Edition, MIT): words shorter than 5 characters must match
exactly, 5–8 characters may carry one typo, 9 or more may carry two, and a
typo on the *first* character counts as two. Distance is optimal-string-
alignment Damerau–Levenshtein (a swap of two neighbours is one typo, the
commonest real typing error). Clean-room.

**Why Atlas wants it — and why this does not break `palette`'s rule.**
`palette::score` is "deliberately not fuzzy": a matcher that returns
something for every query means the palette never says "I don't have
that". That rule is right, and it is exactly what Meilisearch's thresholds
protect: "banana" is never within two typos of "settings", but "setings"
and "sttings" are within one. Today those return nothing and the user
learns the wrong lesson — that the thing is not there. The proposal is a
*fallback*: only when the exact matcher finds nothing, and said as "did you
mean Settings?", never silently substituted.

**Public interface:**

- `fn allowance`
- `fn osa`
- `fn suggest`

### `src/tz.rs`

**Wired** — something outside this file calls into it.

Local time, for the first time: a zone's offset at any instant, and a wall
clock time turned back into UTC.

**Sources:** POSIX.1-2017 §8.3 (the `TZ` variable: `std offset dst
[offset],start[/time],end[/time]` with `Jn`, `n` and `Mm.w.d` rules, week 5
meaning the last); musl's `src/time/__tz.c` (MIT) read for the rule
arithmetic; the POSIX strings are the footer lines of the IANA tz database
(public domain) for each zone as of 2026; the Windows names map through
CLDR's `windowsZones.xml` (Unicode licence), territory 001. RFC 5545
§3.3.5 for the two awkward hours: a time that happens twice means the
first, a time that never happens is read with the offset before the gap.
Clean-room.

**Why Atlas wants it.** Nothing in the tree knew local time. The calendar
read an Outlook invite at `TZID=Pacific Standard Time:…T100000` as 10:00
UTC — seven hours early. The greeting refused to say "morning" rather than
risk saying it at night. Standing watches written "at 7" meant 7 UTC.
A rule string is small enough to carry for every zone, needs no tz
database on disk (Windows has none), and is right until the rules change —
which, for a zone, is a news item years in advance.

**Public interface:**

- `fn home`
- `fn machine`
- `fn suggest`
- `fn names`
- `struct Zone`

### `src/uia.rs`

**Wired** — something outside this file calls into it.

UI Automation — reading your open windows without a screenshot.

Windows publishes an accessibility tree for screen readers: every control
with a role, a name, and a value. Reading that is far better than
screenshotting and asking a vision model what it sees — it is exact, it is
fast, it costs no GPU, and it works on a window that isn't in front.

The catch is app cooperation. Native Windows apps expose everything.
Electron apps often expose one undifferentiated blob. Custom-drawn UIs
expose nothing. So a large part of this module is **detecting that the tree
is useless**, so Atlas can fall back to a screenshot instead of confidently
reporting nonsense.

**Public interface:**

- `fn assess`
- `fn outline`
- `fn cannot_be_undone`
- `fn button_request`
- `struct Node`
- `enum Role`
- `enum Quality`

### `src/understood.rs`

**Wired** — something outside this file calls into it.

Acting on a guess.

Atlas arrives at what you meant two different ways. A phrase in its own
list is matched outright — it cannot have misread that. Anything else is
handed to a small local model, which returns an intent in exactly the same
confident shape whether it recognised your sentence or invented a reading
of it.

The policy gate grades *what Atlas is about to do*. Nothing has ever told
it *how sure Atlas is that you asked for it*. So a dictation matched from
the phrase list and a dictation the model guessed out of a mumble are
graded identically, and both simply happen. `brain.rs` asks the model to
"use ask rather than guessing" — but that is an instruction to the very
model whose confident wrongness `certainty.rs` exists to catch, and
nothing checks whether it obeyed.

This is the missing half, and it is deliberately the smallest thing that
closes the gap: an inferred reading of something that *changes the world*
is graded one rung stricter, so Atlas says what it took you to mean and
waits for an answer instead of acting in hope. Being told what Atlas
assumed costs you a sentence. Undoing what it did on a guess costs
whatever it did.

Three properties, on purpose:

- **It only ever escalates.** Like [`crate::voiceid::handle`], this
function can return a stricter grade and never a looser one, so the
worst case is a question you did not need to be asked.
- **Reading is left alone.** An inferred `AutoProceed` stays automatic.
Ordinary conversation reaches the model constantly, and asking "did you
mean that?" before answering a question would make Atlas unusable while
protecting nothing — nothing was going to change.
- **Already-asking stays as it is.** `AskClarification` and
`RequireApproval` are the two rungs that already stop and ask. There is
nothing above them to escalate to.

**Public interface:**

- `fn grade`
- `fn checking`
- `struct UnderstoodConfig`
- `enum Understanding`

### `src/undo.rs`

**Wired** — something outside this file calls into it.

What did you do, and take it back.

Atlas now touches files, settings, mail, posts and security pages. Each of
those keeps its own record, which is no use at all at the moment you need
it — you don't know which area it was in, that's why you're asking.

So there's one list, in order, and one way to reverse things. And you don't
have to remember a phrase: anything that sounds like the question works,
because the moment you need this is the moment you'll be least inclined to
recall the right wording.

**Public interface:**

- `fn understand`
- `fn tell`
- `fn reverse`
- `fn say`
- `struct Did`
- `struct History`
- `enum Undo`
- `enum Asking`
- `enum Reversal`

### `src/unpack.rs`

**Wired** — something outside this file calls into it.

Opening zips, and scanning for viruses before anything is opened (Eric's
ruling H3, 25 Sep 2026: "Atlas unzips when needed, and scans for viruses
before opening anything — Windows Defender on the file, and on everything
a zip unpacks to").

The zip reader is Atlas's own: a zip is a list at the end of the file
saying where each entry is, and each entry is stored as-is or deflated.
No outside program, so nothing to be missing on the day you need it.

What it refuses, before unpacking a byte: names that climb out of the
folder (`..\..\Windows\…`), absolute paths, archives nested deeper than
`files.max_archive_depth`, and anything that claims to grow past
`files.max_unpacked_mb` (`files::safe_to_unpack`). What it never does:
write over a file that is already there.

The scan runs Windows Defender's own command-line scanner with
`-DisableRemediation`, so Defender reports what it finds and Atlas tells
you, rather than something of yours vanishing into quarantine unsaid. If
the scan can't run, nothing is opened until you say so.

**Public interface:**

- `fn entries_of`
- `fn read_entry`
- `fn name_inside`
- `fn nesting`
- `fn folder_beside`
- `fn unzip`
- `fn docx_text`
- `fn scan`
- `struct Entry`
- `struct ScanConfig`
- `enum Verdict`

### `src/unsub.rs`

**Wired** — something outside this file calls into it.

Clearing out a personal inbox.

Different problem from triage. Triage sorts what matters; this gets rid of
what doesn't, which is most of a personal inbox.

## The trap worth knowing about

Clicking "unsubscribe" in a spam email is how you confirm your address is
real and read. It makes things worse, reliably. So Atlas distinguishes
sharply:

* **A legitimate sender** — a shop, a newsletter you signed up to — has a
`List-Unsubscribe` header, which is the machine-readable, one-click,
standardised way out. Use it.
* **Spam** has no such header, or has one pointing somewhere odd. Never
touch it. Block the sender and move on.

Getting that distinction wrong in the wrong direction actively harms you,
which is why it's the thing this module is built around.

**Public interface:**

- `fn senders_from`
- `fn judge`
- `fn plan`
- `fn spoken`
- `fn one_click`
- `struct Sender`
- `struct UnsubConfig`
- `struct Cleanup`
- `enum Verdict`

### `src/untrusted.rs`

**Wired** — something outside this file calls into it.

Everything Atlas reads, and the one thing none of it may do.

## The rule, already established, now made general

Handing Atlas a link says **look at this**. It never says **do what this
says**. That rule was written for the tray, because a fetched web page
that could be parsed into an intent would let any page issue Atlas
instructions in Eric's name.

The same rule has to hold for everything handed over — records, past
verdicts, closed trades, notes, a config someone edited, a file dropped in
a folder. Atlas is a consultant reading a client's material. A
consultant reads the brief; it does not take orders from the stationery.

## The protection is structural, and it is not the detector

`looks_like_orders` exists and finds the obvious attempts, and it is **not
what keeps this safe**. A detector can be got around by anyone who thinks
about it for a minute, and a system that relies on one has a security
boundary made of a word list.

What actually keeps it safe is that a `Read` has no route into the parser.
It carries text, it renders that text as a quotation, and there is no
method on it that produces an intent. A source-reading guard holds that
line, because the failure mode is a line of code that does not exist yet.

The detector's job is different and still worth having: it says **when
somebody tried**, which is a thing Eric would want to know.

**Public interface:**

- `fn looks_like_orders`
- `struct Read`
- `struct Inbox`
- `const ORDER_SHAPED`

### `src/unwaited.rs`

**Wired** — something outside this file calls into it.

Children nobody is waiting for.

## The name

This was called `orphans` for twenty minutes, which is the obvious word
and the wrong one. Two reasons, and both are about this tree rather than
about English:

* **"Orphan" already means something else here.** `dead_methods.rs`,
`dead_capabilities.rs` and `new_capabilities_are_wired.rs` all use it
for a *capability* with no caller — "these orphans now have a caller or
a test". A module named after the tree's word for unwired code, whose
subject is unreaped processes, is two unrelated ideas under one word.
* **The guards read words.** `bug_sweep`'s stale-documentation scan looks
for a module name appearing as a whole word near a phrase like "waiting
on", and `HANDOVER_2026-09-14.md` contains *"the nine orphans are named
individually with what each is waiting on"* — ordinary prose about
capabilities, which the guard immediately and correctly reported as a
doc claiming something unbuilt that is now wired. A module whose name is
a common English word in this project's own vocabulary will keep doing
that, and the fix is the name, not a baseline entry.

`unwaited` is what these actually are: started on purpose, not waited for
on purpose, and collected here so that not waiting does not mean leaking.

## The leak

Three places start a process and deliberately do not wait for it, because
waiting is the wrong thing to do: a notification panel, an app the person
asked for, a browser. Each wrote

```text
cmd.spawn().map(|_| ())
```

which starts the process and drops the [`Child`] on the same line.

On Windows that is fine — dropping the handle is all the cleanup there is.
On Linux and macOS it is not. A child that has exited stays in the process
table as a zombie until its **parent** collects its exit status, and
dropping a `Child` in Rust does not collect it and does not detach it. The
documentation is explicit: *"There is no way to detach a child; the
`Child` structure does no cleanup on drop."*

Atlas is a daemon. It runs for days, and until it exits nothing collects
any of them:

* `window::open` re-execs Atlas once per panel. Every notification the
person reads and closes leaves a `<defunct>` entry behind for the rest
of the session.
* `Platform::launch` leaves one per app opened.
* `browser::launch` leaves one per headless browser started.

None of it is visible until it is: a zombie holds a process-table slot and
a PID, and the per-user limit (`RLIMIT_NPROC`, commonly a few thousand) is
shared with everything else the person is running. Atlas is then the
program that stopped their machine from starting processes, and a `ps`
showing a hundred `atlas <defunct>` lines reads as Atlas leaking real
processes rather than exit statuses.

## Why a list and not `SIGCHLD`

`signal(SIGCHLD, SIG_IGN)` makes the kernel reap automatically, in one
line, everywhere. It is not used here because it also makes `wait` and
`waitpid` fail with `ECHILD` for every child in the process — and three
things in this tree depend on waiting working: `tools::wait_or_kill` polls
`try_wait` to enforce every external tool's timeout, `lifecycle::Helpers`
kills and waits for whisper, piper and the model server, and
`frames::Rolling` does the same for ffmpeg. A one-line fix that silently
breaks the timeout on every voice tool is not a fix.

So instead: hand the child here, and it is collected on the tick with
everything else.

**Public interface:**

- `fn dont_wait`
- `fn reap`
- `fn still_running`

### `src/update_apply.rs`

**Wired** — something outside this file calls into it.

InstallStep 2 of the update courier: getting a signed release installed, and
undone.

What was already there: `update_courier` hears a signed release notice and
fetches the file, checked piece by piece against the signed fingerprint.
`release` holds the checks. `upgrade` swaps a new build in only after it
passes its health check, then keeps it on probation (O1). Nothing joined
them up: a downloaded release sat in the state folder and never went in.

This module is the join:

- **Stage** (`stage_update`). Right before installing, check everything
again rather than trusting the record from download time: the notice's
signature against the key trusted *now*, the release order, the data
format, and the downloaded file's own size and fingerprint. Only then is
the file copied to `updates/`, the one way in, so it still has to pass
O1's health check and trial.
- **Finish** (`finish_after_start`). The release number moves only once the
new build has actually got through its trial. A build that was rolled
back leaves the number where it was, and is never offered again.
- **When** (`mode_for`, `next_step`, `quiet_enough`).
- Automatic on Eric's own devices, ask on friends' copies (Eric, 26 Sep).
Anyone can change it on their own device, and nothing arriving over the
network can.
- An automatic install waits for a quiet moment.
- **Undo** (`undo_update`). Going back one version needs a
`release::LocalApproval`, which only the person at this device can give.
The version you went back from isn't offered again; a newer one is.
- **Key rotation arriving** (`heard_rotation`). A signed rotation posted in
the release channel moves this device's trust, or is refused and said
once.

The daemon tick, the `atlas update install/undo` commands, the hub's
Updates page and the voice phrases call these. They are wired after the
merge of the three Atlas versions, because every one of those files is in
that merge.

**Public interface:**

- `fn mode_for`
- `fn quiet_enough`
- `fn next_step`
- `fn ask_due`
- `fn asked`
- `fn not_offered_again`
- `fn failed_because`
- `fn scrub_personal`
- `fn record_failure`
- `fn last_failure`
- `fn unfiled_own_failure`
- `fn file_own_report`
- `fn file_friend_report`
- `fn hold_release`
- `fn is_halted`
- `fn failure_reports`
- `fn fix_brief`
- `fn stage_update`
- `fn pending`
- `fn finish_after_start`
- `fn relaunch_self`
- `fn take_news`
- `fn chosen_mode`
- `fn choose_mode`
- `fn say_yes`
- `fn update_tick`
- `fn previous_build`
- `fn undo_update`
- `fn rotation_notice`
- `fn heard_rotation`
- `fn heard_rotation_with`
- `struct FailureReport`
- `struct StagedUpdate`
- `struct UpdateMoment`
- `enum AutoUpdate`
- `enum InstallStep`
- `enum FailedBecause`
- `enum UpdateSettled`
- `enum Ticked`
- `const ASK_AGAIN_SECS`
- `const QUIET_AFTER_SECS`
- `const MACHINE_RETRY_SECS`
- `const MAX_REPORT_BYTES`
- `const ROTATION_PREFIX`

### `src/update_courier.rs`

**Wired** — something outside this file calls into it.

Hearing about updates: the release channel, read by Atlas.

Eric posts a signed release notice into a release channel -- a group he
owns where everyone else is a reader (`groups`). Every friend's Atlas is in
that group, so the notice reaches them the way any group message does, over
their own pairings, with nothing hosted anywhere. This file is what their
Atlas does when one arrives:

1. Only a message from the channel's **owner** is read at all (the daemon
checks the signed group list before calling `heard`).
2. The notice must carry a **release signature** that verifies against the
key built into this Atlas (`release`). Who posted it doesn't make it
true; the signature does. A notice that doesn't verify is refused and
said, once.
3. A verified notice counts as **having heard from Eric** even if it's for
the release you already have -- that is what stops a device being
quietly held back (`release::freshness`).
4. A verified notice for a newer release that fits this device and can open
its data is **recorded as available** and said once. Nothing downloads
or installs from here: that is the desktop and phone apply step, and it
will ask you before it changes anything.
5. The file itself then **travels the same way**: in pieces, from the
channel owner's Atlas over the pairing, resuming where it stopped, and
kept only if the whole file's fingerprint is the one the signed notice
gave. Eric's Atlas serves only files `atlas release sign` put aside,
named by that fingerprint, and only to people it's paired with.

**Public interface:**

- `fn announcement`
- `fn heard`
- `fn keep_for_friends`
- `fn chunk`
- `fn fetch_step`
- `fn fetch_from_any`
- `fn status`
- `struct Available`
- `enum Fetched`
- `const OUTBOX`
- `const PREFIX`
- `const FILES`
- `const DOWNLOADS`
- `const CHUNK`
- `const KEEP_OWN_FILES`

### `src/upgrade.rs`

**Wired** — something outside this file calls into it.

Replacing the binary without losing what Atlas knows.

The reason this exists, in one sentence someone actually said: *"There
isn't enough evidence to justify deleting and reinstalling Atlas multiple
times."* That is the correct instinct about the wrong situation —
updating Atlas should never have meant deleting it, and until this file
there was nothing that said so, nothing that checked it, and nothing that
could be pointed at as evidence.

## What an update actually is

Atlas keeps three things in three places, and only one of them comes from
a download:

| | what | comes from |
|---|---|---|
| **the program** | the `atlas` binary | rebuilt or re-downloaded |
| **the recipe** | `config/*.yaml` | ships with the program, generic, no username in it |
| **your machine and your memory** | `config/machine.yaml`, `data/` | written here, never shipped, never replaced |

So an update is: stop Atlas, replace one file, start Atlas. The third row
is untouched, and the second is only touched when the shipped recipe
itself changed.

## Why it needs code rather than a sentence in a README

Because "it should be fine" is what every destructive upgrade has said.
`check` looks at a real install and answers a specific question — if the
binary were replaced right now, what survives, what is regenerated, and
what is at risk — before anything is moved. It is the difference between
believing an update is safe and having looked.

**Public interface:**

- `fn check`
- `fn check_with`
- `fn version`
- `fn sha256_of`
- `fn build_tag`
- `fn tag_version`
- `fn tag_of`
- `fn this_tag`
- `fn older_version`
- `fn keep_old_at`
- `fn prune_kept`
- `fn staging_path`
- `fn waiting`
- `fn version_of`
- `fn failed_at`
- `fn update_history`
- `fn current_trial`
- `fn begin_trial`
- `fn is_known_bad`
- `fn known_bad_reason`
- `fn forgive`
- `fn health_check`
- `fn check_new_build`
- `fn roll_back`
- `fn swap_checked`
- `fn trial_on_start`
- `fn trial_passed`
- `fn trial_passed_by`
- `struct Item`
- `struct Report`
- `struct Trial`
- `enum Fate`
- `enum Swapped`
- `enum TrialStep`
- `const YOURS`
- `const SHIPPED`
- `const DATA_FORMAT`
- `const RENAMED_SETTINGS`
- `const KEEP_BUILDS`
- `const HEALTH_TIMEOUT_SECS`
- `const TRIAL_STARTS`
- `const HEALTHY_AFTER_SECS`

### `src/urgency.rs`

**Wired** — something outside this file calls into it.

"What should I do next?" as a number with its reasons shown.

**Source:** Taskwarrior's urgency model (`GothenburgBitFactory/taskwarrior`,
MIT) — a weighted sum of simple terms, with the published default
coefficients and its due-date ramp. Clean-room from the documented model
(taskwarrior.org/docs/urgency, taskrc(5)).

**Why Atlas wants it.** `shared_task::Task` has a description, a due date and
a done flag; `for_space` lists tasks in insertion order. "What's on my plate"
and the morning brief need an order, and an order Atlas can explain:
"top because it's two days overdue and it blocks three others" is a reason
Eric can argue with; a model's ranking is not. Taskwarrior's model has
fifteen years of people living with it, every term is visible, and every
coefficient is a setting.

Terms that need fields `Task` does not have yet (priority, tags, blocking,
started) default to off, so this works on today's `Task` and gets better
as fields are added.

**Public interface:**

- `fn due_term`
- `fn rank`
- `fn why`
- `struct Item`
- `struct Coefficients`

### `src/vad.rs`

**Wired** — something outside this file calls into it.

Is that a voice, or the room? — speech detection that learns the room.

**Source:** Moattar & Homayounpour (2009), *A Simple but Efficient
Real-Time Voice Activity Detection Algorithm* (EUSIPCO): per 10 ms frame,
three features — energy, the dominant frequency, and spectral flatness —
each compared with the room; a frame is speech when at least two of the
three say so, the room's level is re-learned from quiet frames, and runs
too short to be speech or silence are absorbed. Three changes from the
paper, each found failing on a test: energy is dB above the learned floor
(6 dB) rather than `40·log10(min E)`, which goes negative for a quiet
room; the frequency vote asks for a dominant frequency in the voice band
(80–1100 Hz) rather than "185 Hz above the room's", which broadband noise
passes and fails at random; and the room is re-learned only from frames
within 3 dB of it, because re-learning from every frame judged silent let
one misjudged syllable lift the floor until speech stopped counting at
all. The FFT is an in-place radix-2. Clean-room.

**Why Atlas wants it.** `endpoint` decided "you've stopped talking" with a
fixed −38 dBFS line. A laptop fan or an air conditioner sits above that
line, so in a noisy room the turn never ended until the 20-second hard
stop; a quiet speaker in a quiet room sat below it and was cut off. This
learns the room in the first third of a second and then asks whether
what's above it is shaped like a voice.

Silero VAD (a small neural model, MIT) is better still, and was
considered: it needs an ONNX runtime crate and a model file fetched from
outside the tree — third-party weight the project rule asks to avoid
unless it's earned. This is the in-house baseline it would have to beat.

**Public interface:**

- `fn segments`
- `fn level_for_endpoint`
- `struct VadParams`
- `struct Vad`
- `const DEFAULT_MEASURED`

### `src/vadcal.rs`

**Wired** — something outside this file calls into it.

Tuning the speech detector to your room, from two recordings.

Round 4 measured the detector's three thresholds on five synthesized rooms
and shipped the best. Your room is none of them. This takes two recordings
made on your own microphone — the room with nobody talking (fan, air
conditioning, whatever is usually on), and you talking somewhere quiet —
and builds the test the thresholds are scored on from them:

1. Where you are speaking is known from the quiet recording alone: frames
within 35 dB of its loudest.
2. The room recording is laid under it, at the level your microphone
actually picked it up (both came through the same gain), with room-only
stretches either side.
3. The same grid as `tests/vad_measured.rs` is scored on that mix, and the
best setting is written into your settings — where the three sliders on
the settings page show it and you can move it back.

**Public interface:**

- `fn calibrate`
- `fn from_recordings`
- `struct Calibration`
- `struct Outcome`
- `const ENERGY`
- `const FLATNESS`
- `const LOUD`

### `src/vault.rs`

**Wired** — something outside this file calls into it.

Holding secrets so a stolen laptop is worth nothing.

## The thing that makes this real rather than theatre

Encrypting a file is easy. The hard part is where the key lives, and it's
where most "encrypted" local storage quietly fails: if Atlas can read the
secrets whenever it likes, the key is on the machine, and whoever has the
machine has both. That's a locked box with the key taped to the lid.

So the key is **derived from something not on the machine** — a passphrase
you type — and held only in memory, only while you're using it. A stolen
laptop then contains a file nobody can open, including Atlas.

The cost is honest and worth stating: you type a passphrase once per
session. Nothing unattended can touch the vault, which means overnight work
can't use it either. That's the trade, and it's the right way round.

**Public interface:**

- `fn key_from_words`
- `fn short_code`
- `fn new_recovery_key`
- `fn tidy_recovery_key`
- `fn seal_aead`
- `fn unseal_aead`
- `fn seal_bytes`
- `fn unseal_bytes`
- `fn real_crypto_here`
- `fn set_passphrase`
- `fn make_recovery_key`
- `struct Secret`
- `struct VaultConfig`
- `struct Wrap`
- `struct Vault`
- `enum Kind`
- `enum State`
- `enum How`
- `const REAL_CRYPTO`
- `const KDF_ROUNDS_IS_IGNORED`
- `const NOT_A_PASSWORD_MANAGER`
- `const WHILE_YOU_SLEEP`
- `const WHAT_THIS_DOES_NOT_STOP`

### `src/vformat.rs`

**Wired** — something outside this file calls into it.

Read and write `.vcf` contacts and `.ics` calendars — the two file formats
every phone, mail client and calendar already speaks.

**Sources:** RFC 6350 (vCard 4.0) and RFC 5545 (iCalendar): content lines
`NAME;PARAM=VALUE:VALUE`, folded at 75 octets with CRLF + one space,
text escaping of `\\ \; \, \n`. `Peltoche/ical-rs` (Apache-2.0) read as a
reference for the parser shape (one generic component tree, typed views on
top). Clean-room.

**Why Atlas wants it.** The business hub wants a client list and a shared
calendar that business partners — each on their own Atlas, or on no Atlas
at all — can exchange. Offline, with no service in the middle, the
exchange format is a file, and these are the files. A partner's Outlook
invite is an `.ics`; a client's business card is a `.vcf`. Without this,
either Atlas invents a format nobody else reads, or the data is re-typed.

The typed views read what Atlas uses and keep everything else in the
generic tree, so a file read and written back loses nothing it did not
understand.

**Public interface:**

- `fn unfold`
- `fn parse`
- `fn write`
- `fn events_in`
- `fn calendar`
- `struct Prop`
- `struct Component`
- `struct Card`
- `struct Event`
- `struct Zones`

### `src/viewing.rs`

**Wired** — something outside this file calls into it.

Watching a video, rather than only listening to it.

Transcribing the sound gets you half of a video and often the less useful
half. Someone says "and you can see the problem here" over a screen that
contains the entire answer; a demo shows a menu path with no narration at
all; a recorded call spends ten minutes on a slide nobody reads aloud.

## Why not just take screenshots

Because that is the version that looks like it works and doesn't. Sampling
a frame every few seconds gives you two bad outcomes at once: hundreds of
near-identical pictures of a static slide, filling the disk, and a missed
frame at the one second something appeared. Partial context, at the cost of
space, which is the worst trade available.

So three decisions instead:

**Sample when the picture changes, not when the clock ticks.** A forty
minute screen recording has perhaps thirty moments where anything actually
changed. Those are the frames worth having, and there are thirty of them
rather than twenty-four hundred.

**Keep the reading, throw the frame away.** What a frame is worth is the
words on it. Those are a few hundred bytes; the frame is a few hundred
kilobytes. Atlas reads each one and deletes it, so watching an hour of
video costs about as much disk as a long email.

**Put the picture back next to the words.** This is the part that makes it
worth doing at all. Neither the transcript nor the screens are the video —
the video is what was on screen *at the moment those words were said*, and
that only exists if the two are stitched back together by time.

**Public interface:**

- `fn too_long`
- `fn read_timed`
- `fn scene_times`
- `fn where_to_look`
- `fn frame_times`
- `fn one_per_moment`
- `fn same_screen`
- `fn weave`
- `fn weave_seen`
- `fn retell`
- `struct ViewConfig`
- `struct Moment`
- `struct Spoken`
- `struct Seen`

### `src/vision.rs`

**Wired** — something outside this file calls into it.

Seeing, rather than reading.

## What was missing

Until now Atlas's only eyes were `ocr` — it could read the *words* in a
picture and nothing else. A chart was a picture with no words in it. A
face was a picture with no words in it. A mug on the desk, a thing being
pointed at, a photo with no caption: all of them came back as "the writing
in it is too unclear for me to read honestly", which is a true sentence
about the wrong question.

This module answers the other question: not what does it *say*, but what
*is* it.

## Local, and why that was never really a choice

The alternative was a hosted vision model, and Eric's standing rule rules
it out on its own — as offline and self-built as possible, no
subscriptions, third-party services are concepts to copy rather than
dependencies to adopt. But the numbers say the same thing without needing
the rule: the four model files together are well under a tenth of a
gigabyte, against the spare memory `fit` actually measures on his laptop.
Sending his camera to somebody else's computer would cost more, do less,
stop working on a train, and drag the whole personal/business boundary
question along behind it.

Everything here runs inside `atlas.exe` through `infer`. No second
process, no Python, no account.

## The four honesty rules

Each of these is a way this feature could look like it works and not.

1. **An unread camera is not an empty room.** `Sight::Unread` exists so
"I couldn't look" can never be reported as "there's nothing there".
The same rule `gaze` already turns on.
2. **Below the floor, say nothing rather than guess.** A thing Atlas is
not sure about is left out of the list, not listed with a low number
nobody reads.
3. **A name carries how sure it is.** Above the higher bar Atlas says what
it is; below, it says what it *thinks* it is; the same sentence with
and without a hedge is the difference between useful and misleading.
4. **A face is not a password.** Recognising a face never authorises
anything, for the reason `gaze` already gives: a photograph held up to
a webcam has no honest defence at this layer.

## What it is not

It does not describe a scene in a sentence, reason about what is
happening, or read a chart's meaning. It finds faces, finds and names
things from a fixed list, describes a whole picture, and recognises
anything Eric has shown it and named. A model that talks about pictures is
several gigabytes and a different decision.

**Public interface:**

- `fn letterbox`
- `fn faces`
- `fn things`
- `fn whole_picture`
- `fn thin_out`
- `fn to_unit`
- `fn alike`
- `struct Patch`
- `struct Object`
- `struct Face`
- `struct Scene`
- `struct VisionConfig`
- `struct Framing`
- `struct Shown`
- `struct Album`
- `struct Looking`
- `enum Sight`
- `enum Guess`

### `src/voice.rs`

**Wired** — something outside this file calls into it.

The voice loop: record -> transcribe -> (caller acts) -> synthesize -> play.

Each stage is an ExternalTool from config/tools.yaml, so this module has no
idea whether it is driving Whisper or Vosk, Piper or Windows SAPI.

**Public interface:**

- `fn loose`
- `fn clean_transcript`
- `struct ToolsConfig`
- `struct HubConfig`
- `struct VideoConfig`
- `struct UiaConfig`
- `struct PttConfig`
- `struct WakeConfig`
- `struct Voice`
- `struct VoiceWork`
- `struct VoiceSpeaker`
- `struct TradingConfig`
- `const RECORD_RATE_HZ`

### `src/voiceid.rs`

**Wired** — something outside this file calls into it.

Recognising your voice.

You asked for this at the very start and I talked you out of it. That was
half right: voice is genuinely weak as *authentication*, because a
recording of you passes. It is genuinely useful as a *filter*, because the
television, a podcast, and someone at the next desk should not be able to
drive your workspace.

So the rule this module enforces:

> Voice identity decides whether Atlas **listens**. It never decides
> whether Atlas is **allowed**. Anything consequential still goes through
> the approval gate, where a human answer is required.

Embeddings come from an external speaker-encoder; everything here is the
comparison and the policy, which is where the mistakes actually live.

**Public interface:**

- `fn handle`
- `fn cosine`
- `fn enrollment_prompt`
- `struct Voiceprint`
- `struct VoiceIdConfig`
- `struct VoiceId`
- `enum Verdict`
- `enum Handling`

### `src/voiceover.rs`

**Wired** — something outside this file calls into it.

Reading your script over your footage.

The recording is the easy half — piper does that, locally and for nothing.
The half that makes it sound deliberate rather than pasted on is the
timing: where the lines land, how long the gaps are, and what happens to
the music underneath while you're talking.

A voiceover that ignores the picture reads as a voiceover. One that lands
on the cuts reads as the video.

**Public interface:**

- `fn how_long`
- `fn lay_out`
- `fn fits`
- `fn music_ducking`
- `fn duck_filter`
- `fn spoken`
- `fn snapped_count`
- `fn break_into_lines`
- `struct Line`
- `struct Beat`
- `struct VoiceoverConfig`
- `enum Fit`

### `src/voicepick.rs`

**Wired** — something outside this file calls into it.

Trying a voice before downloading it (H13f, held for the hub until 27 Sep
2026).

The Sound page lists the voices in `tts::catalogue`, but until now only
Amy could be downloaded (setup fetches her), and the rest showed as "not
installed yet" with no way to hear them or get them. A voice is 60 MB and
you'd pick it by its sound, so the page now does both: **Hear** plays the
voice's own sample, and **Get** downloads it.

**Where the sounds come from.** Each piper voice on Hugging Face ships a
short sample (`samples/speaker_0.mp3`, about 100 KB) beside the model.
Atlas fetches it once, checks it against the SHA-256 pinned below, keeps it
beside the voices (`<voices_dir>/samples`), and plays it from the hub, so the browser
never talks to Hugging Face itself and a second listen is offline. The
models are pinned the same way (their LFS hashes, read from the Hub's API
on 27 Sep 2026), so a changed file on the far side is refused rather than
installed. This is the online, secondary path: nothing else depends on it.

**Public interface:**

- `fn source`
- `fn sample_piece`
- `fn voice_pieces`
- `fn size_said`
- `fn fetch_voice`
- `struct Source`
- `struct Downloads`
- `enum Getting`
- `const SOURCES`

### `src/waitingfor.rs`

**Wired** — something outside this file calls into it.

Who owes you a reply, and what you said you'd do.

Two lists, read from your recent mail (`mailbook`):

- **Owed to you.** You asked something ("can you send…", "could you…",
"let me know…", a question to them) and nobody on the other side has
written back in that conversation since. Said once it's been
`owed_after_days` working days.
- **You promised.** You wrote "I'll send it Friday", "I will get back to
you", "let me check and…" -- a commitment, with a date when you gave
one (read by `when`, against the day you wrote it). Said on the day
it's due, and after.

**Sources:** Microsoft Research's work on commitment detection in email
(Lampert et al.; the "Email overload" project) established the two
things this is built around: requests and commitments are carried by a
small set of phrasings, and a detector trained on one organisation's mail
does badly on another's. So these are rules, not a model, and **you teach
it**: "that's not a promise" (or "done") on an item is recorded, and a
phrasing you've dismissed more than you've kept stops counting
(`Taught::trusts`). Research on why people abandon trackers (Epstein et
al., UbiComp 2015: forgetting and upkeep) is why it fills itself in from
mail you already sent rather than asking you to log anything.

**Public interface:**

- `fn read_letter`
- `fn open`
- `fn due_now`
- `fn say`
- `struct WaitingConfig`
- `struct Waiting`
- `struct Taught`
- `enum Side`

### `src/wakeword.rs`

**Wired** — something outside this file calls into it.

Your own wake word, taught from three recordings.

The wake word used to mean one of two things: a dedicated detector program
fetched from outside (openWakeWord, Porcupine — a model file someone else
trained), or running speech-to-text on every two-second clip and looking
for the phrase in the transcript, which keeps a CPU core busy all day.

This is the classic third way, from before neural detectors (the
template-matching keyword spotters of the 1970s–90s; Sakoe & Chiba 1978
for the alignment): you say the phrase three times, each take is kept as a
sequence of cepstral frames (`mfcc`), and a stretch of audio counts as the
phrase when dynamic time warping lines it up closely enough with one of the
takes. No model to download, nothing leaves the machine, and it is your
phrase in your voice — which also makes it weaker for anyone else's voice,
and the measurement says how much.

**Public interface:**

- `fn phrase_take`
- `fn train`
- `fn distance`
- `fn closest`
- `fn heard`
- `fn load`
- `fn add_take`
- `fn forget`
- `struct Take`
- `struct WakeModel`
- `const TAKES`
- `const MARGIN`

### `src/walkthrough.rs`

**Wired** — something outside this file calls into it.

Walking you through a change on your own accounts.

You offered this yourself as the alternative — Atlas pulls up the page and
you make the change — and it's the right shape, so here it is properly.

Atlas finds the setting, opens the exact page, tells you where the switch
is, waits, and moves to the next one. Fifteen accounts becomes fifteen
clicks instead of an afternoon of hunting through settings menus that are
all laid out differently on purpose.

## One thing worth noticing about turning two-factor off

You can only change it from inside the account. Which means in every
scenario where you'd want it off, you can already get in — and in the
scenario you're actually worried about, being locked out, having it off
wouldn't have helped because you'd have needed access to turn it off.

That's not an argument against doing it. It's the reason the preparation
path solves the problem and this one only feels like it does.

**Public interface:**

- `fn where_2fa_lives`
- `fn to_turn_off`
- `fn to_turn_on`
- `fn to_prepare`
- `fn before_turning_off`
- `struct Stop`
- `struct Walk`
- `struct WalkConfig`
- `const FAMILY_ACCESS`

### `src/wanted.rs`

**Wired** — something outside this file calls into it.

Working out what you want back.

Someone tells you a problem. There are three useful things you can do, and
doing the wrong one is most of what makes people bad at this:

* **Solve it** — right when they asked, wrong when they didn't.
* **Hear it** — right when they're working something out, insulting when
they wanted an answer and got sympathy.
* **Both, in order** — hear it first, then offer.

Atlas reads which from how you said it, and **asks when it can't tell**
rather than guessing. Asking costs one sentence; guessing wrong costs the
conversation.

Two things this is not. It doesn't infer how you feel — it reads what
response you're asking for, which is a different and much more legible
thing. And listening is not agreeing: Atlas will still say when it thinks
you're wrong, because a system that only reflects you back is worse than
useless when you need to be told something.

**Public interface:**

- `fn is_a_question`
- `fn read`
- `fn ask_which`
- `fn answer_to_ask`
- `fn heard`
- `fn then_offer`
- `fn is_empty_sympathy`
- `fn decide`
- `struct Reading`
- `struct Preferences`
- `struct WantedConfig`
- `enum Wanted`
- `const STILL_HONEST`
- `const EMPTY_SYMPATHY`

### `src/wants.rs`

**Wired** — something outside this file calls into it.

Atlas noticing what it hasn't got.

Different from diagnosis. Diagnosis is "something is broken". This is
"I could be better at this, and here is what it would take" — grounded in
measurements of its own performance and in what this specific machine can
actually do.

The rule that keeps it useful: **every recommendation carries the evidence
that prompted it and an honest cost.** A wish list without either is just
an assistant asking for things.

**Public interface:**

- `fn machine_from`
- `fn recommend`
- `fn ask`
- `struct Machine`
- `struct Measurement`
- `struct Observations`
- `struct Recommendation`
- `enum Cost`

### `src/watch.rs`

**Wired** — something outside this file calls into it.

Watching something from the outside.

A machine you run elsewhere (a server, a home lab, a cloud box) is
watched cleanly: Atlas can confirm it is alive and reachable **without any path into it**. A TCP
connect and nothing else — no credentials, no control, no data. If the
Atlas laptop were ever compromised, this gives an attacker nothing they
could not learn by port-scanning.

The design problem is not the probe. It is not crying wolf: networks blip,
and an alert on every dropped packet is an alert you learn to ignore.

**Public interface:**

- `fn reachable`
- `struct Target`
- `struct Status`
- `struct Watcher`
- `enum Health`
- `enum Alert`

### `src/watching.rs`

**Wired** — something outside this file calls into it.

Watching something long finish.

You start a render, a build, a large download, and walk away. The useful
thing is not a progress bar you have to look at — it's being told the
outcome when it happens.

The judgement here is about when to speak. Something that finishes in
twenty seconds does not need announcing; you were still sitting there.
Something that took ten minutes does, even if it succeeded.

**Public interface:**

- `fn describe`
- `struct Job`
- `struct WatchConfig`
- `struct Watcher`
- `enum Outcome`

### `src/webrun.rs`

**Wired** — something outside this file calls into it.

Atlas actually doing sign-ins and sign-ups in its own browser.

Until 25 Sep 2026 both of these were only sentences. "Signing you into
github.com as eric" was said after the grant checks passed, and nothing
then opened a page or filled a box. "Making you an account on x.com" was
said the same way, over an `enrol` module whose page reader had never
seen a page. Eric's rulings (B4: autofill yes; B6: Atlas may create
accounts) make both real, and this is where they happen.

Both run in Atlas's own browser (`browser.rs`, its own profile, never your
Chrome window), and both keep the rules that were already written:

- **The domain is checked on the page that loaded,** not the one asked
for. A redirect to a look-alike stops everything before a box is filled
(`signin::registered_domain`).
- **A password only goes in a password box** on that domain.
- **A sign-up stops for good at payment or identity documents**, and
hands over to you at a robot check (`enrol::read`), before any field on
that page is touched.
- **A page asking for a code** is handed to `twofactor`: you read it out,
or Atlas finds it in your email or texts.

The page scripts return plain words so the decisions stay testable
without a browser; `tests/two_factor_and_signing_in.rs` also runs them
against a real headless Chromium serving local test pages when one is
installed.

**Public interface:**

- `fn login_url`
- `fn sign_in`
- `fn sign_in_at`
- `fn enter_code`
- `fn sign_up`
- `enum SignedIn`
- `enum SignedUp`

### `src/webview2_loader.rs`

**Wired** — something outside this file calls into it.

Windows' web view, loaded only when the hub is first shown.

The hub sits inside Atlas's window through WebView2 (`hubwin`). Microsoft
splits WebView2 in two: the runtime, which ships with Windows 11 and Edge,
and a small loader, `WebView2Loader.dll`, which each app carries. On the
toolchain atlas.exe is built with, the bindings linked that loader as an
import. That meant Windows refused to start atlas.exe *at all*, before any
of Atlas ran, unless the DLL sat in the same folder. That breaks "one file,
double-click it" (found 23 Sep 2026 by reading the built exe's import
table, before it ever reached the laptop).

So the bindings are vendored with that import removed
(`vendor/webview2-com-sys/ATLAS_VENDORED.md`), and the five loader
functions are defined here. Each one loads Microsoft's own signed loader,
carried inside atlas.exe and written to `tools/webview2/` beside Atlas the
first time it's needed, then passes the call through. If the loader can't
be written or loaded, only the Hub page is affected: it says so in words,
and the rest of Atlas runs as before.

**Public interface:**

- `fn place`

### `src/when.rs`

**Wired** — something outside this file calls into it.

Words to a time: "tomorrow at 3", "in 20 minutes", "next Tuesday
afternoon", "the 14th at noon", "Oct 3 from 2 to 4pm", "end of day".

**Sources:** the shape is chrono's (`wanasit/chrono`, MIT; its README and
parser list read as the reference, clean-room): small parsers, each for one
kind of expression — a day, a date, a clock time, an offset, a length — and
a refining step that puts what they found together. Its refinement for a
bare hour ("at 3" means the afternoon) is followed, and said.

**Honest about doubt.** Every result says whether it is `sure`. It is not
sure — and says why — when the words allow two readings a person could
mean: "next Monday" said on a Sunday (tomorrow, or a week tomorrow?), a
numeric date like 3/4 (March or April?), "a few days", "next week"
with no day. A calendar asks rather than guesses when it isn't sure; a
time that was filled in ("tonight" with no hour is 8 pm, a bare "at 3" is
3 pm) is marked `guessed` so it can be said back.

Every time here is **local seconds** (Unix seconds plus the local offset),
the convention `calendar` and `civil` use.

**Public interface:**

- `fn parse`
- `struct Parsed`

### `src/which_errand.rs`

**Wired** — something outside this file calls into it.

"Stop" when several things are running: which one did you mean?

Eric's ruling (23 Sep 2026): stopping a single errand **pauses it and
keeps what it has done** — and when several errands are running, Atlas has
to work out which one "stop" was about, say which one it paused and which
ones carry on, and ask when it genuinely can't tell.

The order it decides in, each step only if the one before settled nothing:

1. **Named.** "stop the research", "pause the backup", "hold the Postgres
one" — a word of the errand's kind or of what it was asked about.
2. **Numbered.** "the second one", "the last one" — oldest first, the
order a list of them is read out in.
3. **Only one.** Nothing to choose between.
4. **The conversation.** What you were just talking about names exactly
one of them.
5. **Just started.** One of them began in the last minute and a half and
none of the others did — "stop" right after asking is about that one.
6. **Ask.** Name them, numbered, and wait for the answer.

Steps 4 and 5 are guesses from context, so they are only ever used for a
pause (which loses nothing and is undone by "no, the other one"), and the
answer always says what it picked, why, and what is still going. Calling an
errand off for good is never decided from context: that needs a name, a
number, "all", or an answer to the question.

**Public interface:**

- `fn verb_and_target`
- `fn correction`
- `fn pick`
- `fn describe`
- `fn question`
- `fn answered`
- `struct Candidate`
- `enum Verb`
- `enum Why`
- `enum Pick`
- `const JUST_STARTED_SECS`

### `src/whichone.rs`

**Wired** — something outside this file calls into it.

Which of several things you meant — and whether it was clear enough to act on.

# The gap this fills

Atlas arrives at what you meant in two places. `intent.rs` matches a phrase
from your own list, and `understood.rs` grades how a reading was arrived at
so an inferred one that changes the world gets checked with you first.

Between those two there was nothing. Once an `Intent` carried an argument,
the daemon decided what to do with it by asking whether your words
*contained* a substring, in `match` arms whose order was never written down
anywhere and was doing the deciding:

```ignore
Intent::Mail(what) if what.contains("tax") || what.contains("statement") => { .. }
Intent::Mail(what) if what.contains("money") || what.contains("spend")   => { .. }
```

"How much did I spend on my trading statement last month" is a question
about money. It contains `statement` and `trading`, so it reached the tax
arm, because the tax arm is written first. Nothing was wrong, nothing
failed, and the answer was about the wrong thing — the failure this tree
keeps finding in other shapes.

# What this does instead

The competing readings are named, each says what belongs to it **and what
belongs to one of the others**, and they are weighed together rather than
tried in sequence. The winner comes back with how far it won by, and a win
too narrow to trust is reported as [`Clarity::Close`] rather than taken.

Saying *"did you mean the money side or the tax side?"* costs you a
sentence. Answering the wrong question costs you the answer you wanted and
the time spent believing it.

# Why the contrast half matters

A list of words that mean an option, with no list of words that mean a
different one, cannot tell "trading statement" from "tax statement". Every
reading here is defined by both, and
[`tests/asking_which_one_you_meant.rs`] fails the build for a set where two
readings share a word without either disclaiming it — which is the only
way this degrades back into the thing it replaced.

# No model, on purpose

This is a word-overlap judgment over a handful of named options, and a
model would make it slower, unavailable offline, and untestable, in that
order. `certainty.rs` and `understood.rs` already handle the cases where a
model has been consulted; this one runs everywhere Atlas runs, including
the machine with nothing downloaded yet.

**Public interface:**

- `fn weigh`
- `fn which_did_you_mean`
- `struct Reading`
- `struct Weighed`
- `struct WhichOneConfig`
- `enum Clarity`
- `const ABOUT_MAIL`
- `const ABOUT_A_POST`
- `const WHICH_MACHINE`

### `src/why.rs`

**Wired** — something outside this file calls into it.

Answering "why did you do that?"

Every decision Atlas makes already produces a reason — the layout engine
says which monitor and why, the approval gate says which rule applied, the
audio layer says which microphone and what it measured. None of it was ever
shown to you.

That's a shame, because an assistant that can account for itself is one you
extend trust to, and one that can't is one you second-guess forever. It is
also the thing none of the fictional systems do — they all announce
conclusions.

**Public interface:**

- `fn is_asking_why`
- `fn answer`
- `fn account`
- `fn steps`
- `struct Decision`
- `struct Record`

### `src/window.rs`

**Wired** — something outside this file calls into it.

Atlas's own window.

Four things go here, and nothing else: the brief when you come back, the
outstanding list when you ask for it, Atlas's thought process when you ask
to see it, and an urgent item it could not say out loud.

**Not the hub.** The hub is a web page on localhost and it opens when you
ask for it, never on its own. This is a small panel Atlas owns and can put
in front of you.

## Why this runs as a separate process

Not a stylistic choice. On macOS a window has to be created on the process
main thread — that is a hard rule of the platform, not a convention — and
Atlas's main thread is the daemon loop, which cannot block for as long as a
window is open. Spawning a thread does not fix it, because the restriction
is about *which* thread, not about blocking.

So the daemon writes the panel to a file and launches `atlas window` with
it. That child gets its own main thread, the daemon carries on, and the
same code path works on Windows, macOS and Linux without a per-platform
branch. It also means a crash in the window cannot take Atlas down.

**Public interface:**

- `fn stage`
- `fn read_staged`
- `fn open`
- `fn can_open`
- `fn run`
- `struct Contents`
- `enum Panel`

### `src/wire.rs`

**Wired** — something outside this file calls into it.

Atlas's own encryption between two people's Atlases.

Your own devices can reach each other over your own private network, which
encrypts for them. Between two *people* nothing like that sits in the way:
your Atlas knocks on your friend's over the open internet, so everything it
says has to be sealed by Atlas itself. This is that seal.

**Who can open it.** Every request is sealed *to* the key the sender pinned
for the receiver when they became friends, and *by* the sender's own key. A
fresh one-time key is made for every envelope, so no two envelopes share a
key and an old one is never readable later from anything sent before it.
Only the receiver can open it, and opening it proves who sent it -- nobody
else could have made it -- so a friend passing it on (`mailbox`) carries a
blob it can neither read nor forge.

**What's inside** is the same request the door always handled -- which door
(`/chat`, `/group`, ...), the pairing's token, and the body -- so every
door keeps exactly its own rules. The answer comes back sealed with a key
only the sender can derive.

**Replays.** An envelope carries the time it was made and is refused
outside a ten-minute window, and each one-time key is accepted once.

The pieces: X25519 for agreeing a secret (`peerkey`), HKDF-SHA256 to turn
it into keys, ChaCha20-Poly1305 to seal. Nothing here is new cryptography;
it is the shape of Noise's "K" pattern, one message and its answer.

**Public interface:**

- `fn seal`
- `fn open`
- `fn seal_reply`
- `fn open_reply`
- `struct Envelope`
- `struct Inner`
- `struct ReplyKey`
- `struct Reply`
- `struct Seen`
- `enum Refused`
- `const PATH`
- `const WINDOW_SECS`
- `const MAX_ENVELOPE`

### `src/wireguard.rs`

**Wired** — something outside this file calls into it.

Reaching your own server over WireGuard — and only the part of it Atlas
is allowed to reach.

Eric's rulings (23 Sep 2026): the phone reaches the laptop over Tailscale;
the personal server (his own machine, not a VPS) is reached **another way,
not Tailscale** — WireGuard. And the hard rule: personal Atlas may use
that server **for higher models only**, and never touches anything else
on it except through the server's own Atlas, if it runs one.

So this module is two things at once:

1. **The tunnel.** Keys, and one config each for the server, the laptop
and the phone. Plain WireGuard: no coordination server, nobody else's
machine in the path.
2. **The fence.** WireGuard decides who can *reach* the server, not what
they can reach *on* it. That is the server's firewall, so this module
writes the rules: from the tunnel, personal devices get the model
port and nothing else. Every other port is closed to them at the
network, not merely unconfigured — a mistake in Atlas's own settings
still can't open them. The server's own Atlas door is a separate,
named switch, off until you turn it on.

Two facts shape the layout, both researched rather than assumed:

- **An iPhone runs one VPN at a time.** Tailscale and the WireGuard app
cannot both be connected. Eric's ruling (23 Sep 2026): **the phone keeps
Tailscale.** It reaches the laptop over Tailscale as it always has, and
the server's models through the laptop — the laptop runs both tunnels,
which a laptop can. The phone still gets its own WireGuard config, for
the times you want the server directly; switching to it takes Tailscale
down until you switch back. Routing the phone to the laptop through the
server (`phone_reaches_laptop_through_server`) is kept, off.
- **WireGuard needs one UDP port the outside world can reach.** At home
that is a port forward on the router and a name that follows your home
address. If the provider puts the house behind carrier-grade NAT there
is no port to forward, and the only fix is a relay somewhere public.

**Public interface:**

- `fn is_key`
- `fn wg_tool`
- `fn make_keys`
- `fn server_conf`
- `fn device_conf`
- `fn fence`
- `fn nftables`
- `fn all_but`
- `fn windows_rules`
- `fn model_door_problem`
- `fn handshakes`
- `fn connection`
- `struct WgConfig`
- `struct Plan`
- `struct Keys`
- `struct Rule`
- `enum Device`
- `const SERVER_ATLAS_DOOR`
- `const YOURS`
- `const ONE_TUNNEL_ON_A_PHONE`

### `src/words.rs`

**Wired** — something outside this file calls into it.

Reading the words off a screen, in Atlas's own code.

## What this replaces, and why

`ocr` shells out to `tesseract.exe`: a separate program, a separate
install, a separate thing to go missing. It has never run, because nothing
in `ATLAS.bat` ever downloaded it — the capability page has said *"waiting
on tesseract"* since the day it was written.

This is the same job done in two model files and this module. No second
process, no PATH, no install, nothing to go out of date separately from
Atlas. The same arrangement as everything else Atlas sees with: **our
code, somebody's weights, downloaded once.**

## Two models, because it is two jobs

Finding *where* the words are and reading *what they say* are different
problems and are not solved by one network.

1. **The finder** turns a whole picture into a map of how likely each pixel
is to be part of a letter. It has no idea what the letters are.
2. **The reader** turns one strip of picture into characters. It has no
idea where the strip came from and cannot find a second one.

Everything between those two — turning a probability map into boxes, and
turning a box into a strip the reader will accept — is this file, and it is
where the accuracy is won or lost.

## The part that is deliberately narrower than the model allows

Boxes here are **upright rectangles**. The finder can support text at any
angle, and doing that properly means fitting a minimum-area rotated
rectangle to each blob and then offsetting a polygon outward — a
respectable amount of geometry, most of which has no test that can be
written without a photograph.

This reads screens. Text on a screen is upright, near enough always. So a
rotated line here is read as the upright box around it, which for a slight
tilt is fine and for a real rotation is poor — and that is written down
rather than discovered. If Atlas ever needs to read a photograph of a
street sign, this is the thing to come back to.

## The failure this is built to avoid

OCR does not fail by returning an error. It fails by returning
**confident, plausible, wrong text** — and an assistant that acts on
plausible wrong text is worse than one that says it could not read the
screen. So every strip carries its own number, the whole reading carries
one, and `worth_acting_on` exists so a caller has to pass through a
judgement rather than reach straight for the string.

**Public interface:**

- `fn blobs`
- `fn strength`
- `fn grow`
- `fn reading_order`
- `fn shares_a_line`
- `fn find`
- `fn looks_like_odds`
- `fn ctc`
- `fn capture_args`
- `fn picture_args`
- `fn picture_size`
- `fn pixels_from`
- `fn read_file`
- `fn whole_picture`
- `fn words_in`
- `fn crop_in`
- `struct WordsConfig`
- `struct Strip`
- `struct Located`
- `struct Letters`
- `struct Lettering`
- `struct Screenful`
- `struct Reader`
- `const ALPHABET`
- `const CLASSES`
- `const NO_CAPITALS`

### `src/workday.rs`

**Wired** — something outside this file calls into it.

Round 11's seventeen tools, joined to the daemon: what you say reaches
them, what they keep is saved, the tick feeds the few that run on their
own, and the brief reads your day from them.

The tools themselves live in their own modules and know nothing of the
daemon; this is the one place that does, so the wiring can be read in one
sitting. Three rules it keeps for all of them:

- **Lazy.** Nothing is loaded until it is first used, and nothing runs on
the tick unless it's switched on and has something to do -- a daemon
that never uses a card deck never reads one off the disk.
- **Follow-ups are short-lived.** "Open 2" means the list you were just
shown, for ten minutes; after that "open 2" is an ordinary sentence
again.
- **Your data stays yours.** Every one of these is on the owner's list
(`profiles::THE_OWNERS_OWN`) or harmless to a guest, and none of them is
open to an add-on (`plugins::NEVER`).

**Public interface:**

- `fn numbered_reply`
- `fn read_first`
- `fn reads_whole`
- `fn unused_name`
- `struct WorkdayConfig`
- `struct Known`
- `struct Kit`
- `enum Follow`
- `const FOLLOW_FOR`

### `src/workingset.rs`

**Wired** — something outside this file calls into it.

Taking the work with you.

You start something on the laptop, pick your phone up, walk out, and lose
signal. If the files the task needs are still on the laptop, Atlas on the
phone can talk about the task and do nothing with it — which is the
difference between continuing and merely remembering.

So a task carries its files. Not your whole drive — the handful of things
*this* piece of work touches, worked out from what you've actually opened
and referred to, packed small enough to live on a phone.

**Public interface:**

- `fn pack`
- `fn spoken`
- `fn not_here`
- `fn returning`
- `struct Carried`
- `struct WorkingSetConfig`
- `struct Packed`
- `struct Changed`
- `enum Why`

### `src/worklog.rs`

**Wired** — something outside this file calls into it.

Where the time went — kept by Atlas as you work, on this machine only.

Every tick Atlas already looks at which window has the foreground. This
keeps that as a record: which app, which window, for how long — and, from
the keyboard and mouse, whether you were at the machine at all. From it:
"where did my time go today?", your longest stretch of focus, how often you
switched, and — when you come back from a break — what you were in the
middle of.

**How it's kept.** ActivityWatch's model (`ActivityWatch/activitywatch`,
MPL-2.0; its data model and heartbeat documentation read as the reference,
clean-room): a watcher sends a *heartbeat* of the current state, and a
heartbeat identical to the last span that arrives within the *pulse time*
extends that span instead of starting a new one — so a day is a few
hundred spans, not tens of thousands of samples. Away time is keyboard and
mouse silence of `away_after` seconds (ActivityWatch's AFK watcher uses
three minutes); the span ends when the input stopped, not when Atlas
noticed. Where the platform can't say how long since the last input,
away time can't be told from working time, and the report says so.

**Focus.** A focus block is a run of time in one category that tolerates
short excursions (a glance at mail under a minute) and breaks nothing
shorter than `lull` seconds away from the keyboard; it counts from 25
minutes (a choice, the length of a Pomodoro, not a finding). Switches are
changes of category that last at least ten seconds.

**Privacy.** Nothing here leaves the machine or goes into sync. Window
titles pass through `redact` first, so a key or card number in a title
never lands on disk, and titles can be switched off (`keep_titles`).

**Public interface:**

- `fn category_for`
- `fn effective_idle`
- `fn summarise`
- `fn duration_words`
- `fn say`
- `fn time_on`
- `struct Span`
- `struct Rule`
- `struct WorkLogConfig`
- `struct WorkLog`
- `struct Context`
- `struct Block`
- `struct Summary`
- `enum Beat`
- `const PAUSE_BINS`
- `const PAUSES_TO_LEARN`
- `const WATCHING_MAX`
- `const FOCUS_MIN`

### `src/workshop.rs`

**Wired** — something outside this file calls into it.

The workbench: each project you work on, and its queue of proposed changes.

You tell Atlas to change something on a project; Atlas scopes it, does the
work (itself or by handing it to a sub-agent), checks the result against
the project's own toolchain, and files it here as a **proposed change** —
complete, titled, described, and **not yet applied**. You are told it is
ready. Then, when you want it, you say "implement <title>" (or click
implement in the hub) and only then does it touch the project.

## Why a hold queue and not just "do it"

The gap between "the work is done" and "the work is live" is the whole
point. It is where you look at what Atlas actually built, in your own
time, per project, and decide. A change that edits your real files the
instant a model finishes is a change you did not get to see first. So
everything lands here first, with a title you can refer to and a plain
description of what it does, and nothing is applied until you say so.

## One queue per project

Atlas has its queue, every other project its own.
You sort by project in the hub and see, for each: what is being worked,
what is waiting on your go-ahead, and what is still outstanding. Nothing
from one project's queue can be confused with another's.

**Public interface:**

- `fn bases_in`
- `struct FileEdit`
- `struct Change`
- `struct Task`
- `struct Project`
- `struct Workshop`
- `struct Implementation`
- `enum State`
- `enum ImplementError`
- `const ABSENT`

### `src/workspace.rs`

**Wired** — something outside this file calls into it.

workspace_on / workspace_off.

Two properties the old scaffold did not have:
1. It waits for a window to actually exist before placing it.
2. A single app failing does not abort the whole sequence — it is
collected and reported, because a half-open workspace you can see is
more useful than an error and three unplaced windows.

**Public interface:**

- `fn workspace_on`
- `fn workspace_on_within`
- `fn open_app`
- `fn close_app`
- `fn input_blocked_apps`
- `fn focus_app`
- `fn workspace_off`
- `struct Report`
- `const BRINGUP_BUDGET_SECS`

### `src/workspace_view.rs`

**Wired** — something outside this file calls into it.

What you see when you ask what's outstanding.

A flat list of tasks is what every app gives you and it's why nobody looks
at them twice. What makes a Notion-style workspace worth opening is not the
prettiness — it's three structural things:

1. **Everything is one kind of thing with properties**, so the same items
can be a list today, a board tomorrow and a calendar next week without
being re-entered.
2. **Views are saved questions, not folders.** "What's blocked" isn't a
place things live, it's a filter over everything.
3. **Items link to each other**, so a task carries its project, its notes
and the thing it's waiting on rather than referring to them.

Atlas already has the items — outstanding work, captures, projects, mail
that needs you, content in progress. What it lacked was a way to look at
them together.

**Public interface:**

- `fn shipped`
- `fn apply`
- `fn grouped`
- `fn day_of`
- `fn day_spoken`
- `fn why_this_took_so_long`
- `fn could_hand_over`
- `fn overview`
- `fn spoken`
- `fn pick`
- `fn is_a_view`
- `fn still_worth_showing`
- `fn grouping`
- `struct Item`
- `struct Thought`
- `struct View`
- `struct Day`
- `struct Overview`
- `struct WorkspaceConfig`
- `enum Handoff`
- `enum Thinking`
- `enum Kind`
- `enum Status`
- `enum Origin`
- `enum Shape`
- `enum Group`
- `enum Sort`

### `src/ws.rs`

**Wired** — something outside this file calls into it.

A minimal WebSocket client. About 200 lines, no dependencies.

It exists because Chrome's DevTools Protocol speaks WebSocket and nothing
else, and pulling in a full async runtime plus a WS crate for one localhost
connection would cost more than writing the framing.

Scope is deliberately narrow: client side, plaintext, localhost. No TLS, no
compression, no fragmentation on send. That is all DevTools needs.

**Public interface:**

- `fn handshake_request`
- `fn encode_frame`
- `fn unmask`
- `struct WebSocket`

### `src/yata.rs`

**Wired** — something outside this file calls into it.

A shared document two devices can both edit while apart, that comes back
together without a conflict to settle.

**Sources:** Nicolaescu, Jahns, Derntl & Klamma (2016), *Near Real-Time
Peer-to-Peer Shared Editing on Extensible Data Types* (the YATA paper),
and the integration loop as `yjs/yjs` (MIT, `src/structs/Item.js`,
`integrate`) implements it: every character carries a unique id and the
ids of its left and right neighbours when it was typed; a concurrent
insert at the same place is ordered by those origins and then by device,
so every replica arrives at the same text whatever order the edits came
in. Deletes leave a tombstone. Clean-room, characters only.

**Why Atlas wants it.** Sync (`sync`) settles a clash on a *field* by
asking you — right for "the meeting is at 3" versus "at 4", wrong for a
page of notes both machines added to while apart, where there is nothing
to choose between: both edits should simply be there. The ops travel as
ordinary `What::Captured` events (they can never clash), so the sync
format does not change.

**Public interface:**

- `fn from_log`
- `fn queue`
- `fn take_queued`
- `fn current`
- `fn all_names`
- `struct Id`
- `struct Doc`
- `enum Op`

### `src/yourchanges.rs`

**Wired** — something outside this file calls into it.

Your own edits to the shipped settings files, kept through every update.

The shipped `config/*.yaml` files are `upgrade::SHIPPED`: an update
replaces them, because that is how a release changes a default. But people
edit them by hand -- `policy.yaml` to let Atlas do something without asking,
`commands.yaml` to add a phrase, `indexing.yaml` to point at a folder -- and
before this file an update would have put every one of those edits back
without a word. For `policy.yaml` that is worse than lost work: a
permission you deliberately *narrowed* would quietly widen again.

`config/settings.yaml` (the Settings page) already survives, because it is
a sparse layer of only what you chose, applied over the shipped file. This
does the same for hand edits, for every shipped YAML file:

- **Base** -- `config/local/base/<file>`: the exact text Atlas last shipped
into `config/<file>`. Kept so a hand edit can be told apart from an older
release's default.
- **Yours** -- `config/local/<file>`: the changes you made, one entry per
setting, each with the value you chose and the shipped value it replaced.
- **Theirs** -- the new shipped text, built into this program.

At startup `keep_hand_edits` compares each shipped file on disk with its
base. Anything you changed is moved into `config/local/<file>`, and the
shipped file is put back to exactly what this build ships. `Config::load`
then lays your changes over the shipped file every time it loads. So:

- a default you never touched follows the release;
- a value you set stays yours, even when the release changes that default
(and you are told once, so you can decide);
- a setting the release removed is reported rather than recreated, because
a setting nothing reads anymore would do nothing and hide that it does.

The rule the whole file rests on: **never lose an edit.** When it cannot
tell whether a difference is yours (no base on record) it keeps it and says
so. When your overlay file will not parse, it touches nothing. When a file
it replaces had any change of yours, even only comments, the old copy goes
to `config/local/previous/<file>` first.

**Public interface:**

- `fn shipped_yaml`
- `fn diff`
- `fn apply`
- `fn conflicts`
- `fn overlay_path`
- `fn load_overlay`
- `fn shown`
- `fn all_kept`
- `fn forget`
- `fn kept_count`
- `fn keep_hand_edits`
- `fn is_source_tree`
- `fn keep_hand_edits_with`
- `struct Change`
- `struct KeptEdit`
- `struct Kept`
- `const LOCAL_DIR`

### `src/zipread.rs`

**Wired** — something outside this file calls into it.

Reading inside a .zip: its list of files, and the text in them.

**Sources:** PKWARE's *APPNOTE.TXT* (the ZIP format: end-of-central-
directory record, central directory headers, local headers; methods 0
"stored" and 8 "deflate"); RFC 1951 (DEFLATE: stored, fixed-Huffman and
dynamic-Huffman blocks, the length/distance tables); the canonical-Huffman
decoder follows Mark Adler's `puff.c` (zlib licence), which decodes a code
one bit at a time from the counts per length — slower than a table and
short enough to check by eye. CRC-32 (IEEE 802.3 polynomial, reflected)
checks every file that comes out. Clean-room; no zip crate.

**Why Atlas wants it.** `index::AssetClass` classified a `.zip` and
stopped; `files::safe_to_unpack` was the guard for an unpacker that did
not exist, and `look_inside_archives` had been deleted from the settings
for promising one. "Find the thing about the budget" now finds it in
`Q3-handover.zip › budget.md`, and a zip bomb is still refused by the same
guard before a byte is inflated.

**Public interface:**

- `fn file_inside`
- `fn texts_inside`

### `src/platform/idle.rs`

**Wired** — something outside this file calls into it.

How long since *you* last used the keyboard or mouse, leaving out
Atlas's own typing.

**Public interface:**

- `fn idle_of_yours`
- `fn own_input_starts`
- `struct OwnInput`

### `src/platform/mobile.rs`

**Wired** — something outside this file calls into it.

The platform layer for a phone — Android and iOS.

On a phone Atlas runs as one app among many, inside the OS's sandbox. It
cannot enumerate the desktop's monitors, launch other apps, or move their
windows around — none of that is a missing feature, it is the platform
forbidding it, the same wall `sync::Kind::Standalone::cannot()` names as
hardware rather than permission. So this layer answers those the honest
way: no monitors to arrange, and window management refused with a reason,
rather than pretending or (worse) reaching for X11/Wayland the way the
Posix layer would if `here()` handed a phone to it.

What a phone build *does* run is the whole core above the platform line —
the daemon, capture, the local model, the event log and the sync that makes
the phone a real peer. The UI is the server-rendered hub inside a WebView
(see `20_PHONE_AS_PEER`), not a native desktop window, so nothing here needs
to draw. The `mobile` shell drives this from Kotlin/Swift over the C ABI.

**Public interface:**

- `struct MobilePlatform`

### `src/platform/mock.rs`

**Wired** — something outside this file calls into it.

A fake OS. Lets the workspace sequence be tested and dry-run anywhere.

**Public interface:**

- `struct MockPlatform`
- `enum Action`

### `src/platform/mod.rs`

**Module declaration** — declares the files below it; no capability of its own.

Everything OS-specific lives behind this trait, so the orchestration logic
can be tested on any machine — including one that is not the target laptop.

**Public interface:**

- `fn virtual_key`
- `fn here`
- `fn what_am_i`
- `struct Monitor`
- `struct WindowId`
- `struct Grab`
- `struct PixelRect`
- `struct ActiveWindow`
- `enum ClipCopy`
- `enum Button`
- `enum OsQuiet`

### `src/platform/posix.rs`

**Wired** — something outside this file calls into it.

Mac and Linux, as far as they go without platform-specific work.

Written because "not supported on your platform" for *everything* is a
lack of effort dressed as a limit. Launching an app, reading files, running
a command, sleeping — none of that needs Win32, and refusing to do it off
Windows was sloppiness rather than a boundary.

What genuinely needs writing per platform is window management, screen
reading and input synthesis. Those still refuse here, and the refusal now
names the specific thing rather than the whole platform.

**Public interface:**

- `fn works_here`
- `struct Posix`
- `enum Flavour`

### `src/platform/win.rs`

**Wired** — something outside this file calls into it.

Windows implementation.

Cfg-gated to Windows. Cross-compiled for Windows on every build, and the
parts that can be are run on Eric's laptop; the tests on Linux run against
the mock platform, so a change here is proved on Windows or not at all.

**Public interface:**

- `fn random_bytes`
- `fn protect`
- `fn unprotect`
- `struct WindowsPlatform`
