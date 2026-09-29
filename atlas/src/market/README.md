# `atlas::market` — handover

**173 tests, all passing. Zero warnings. Zero external crates.**

Ported from the Python prototype into Atlas's own tree at `src/market/`, wired
into `src/lib.rs`, and type-checked as part of the whole crate through the
offline stub harness (`offline-check/check.sh`): **0 errors**.

```
src/market/
  mod.rs          module root
  bars.rs         the OHLC series and the AsOf view      14 tests
  structure.rs    swings, labels, reversal               17
  levels.rs       support and resistance                 13
  regime.rs       trending / ranging / choppy            18
  claims.rs       the vocabulary and the referee         14
  session.rs      sessions and DST                       12
  events.rs       the news calendar                      25
  timeframe.rs    what a bar is worth                    16
  feed.rs         the door real bars come through        13
  time.rs         civil dates, no calendar crate          9
  params.rs       the parameter freeze                   11
  fixtures.rs     deterministic synthetic series          7
  verify.rs       the offline self-check                  4
```

Run everything:

```bash
rustc --edition 2021 --test src/market/mod.rs -o /tmp/market_t && /tmp/market_t
```

Or, inside the crate: `cargo test market::`

---

## The one thing that got better in the port

In Python, lookahead was fought with discipline and a leak detector: a
truncating helper, a rule that readers must be handed a slice, and a sweep over
every reader looking for one whose answer changed when the future arrived. That
worked and it found real problems. But it is a guard rail beside a cliff —
nothing *stopped* a function taking the whole series and reading the end of it.
The detector existed precisely because the mistake stayed makeable.

**Here the mistake is not expressible.** No reader takes `&Bars`. They take
`AsOf`, which owns a borrow and an upper bound and hands out `&[f64]` slices
already cut at that bound. There is no method on `AsOf` that returns a later
price. A reader cannot peek at bar *n+1* for the same reason it cannot read past
the end of a slice: the value is not reachable from what it was given.

Two more consequences fell out:

- **The swing cache is keyed by `(upto, k)`.** In Python the cache lived on the
  frame's `attrs`, pandas propagated it to slices, and a naive `.iloc[:n+1]`
  inherited *the same cache object* computed over the whole series — the exact
  contamination path the detector was built to hunt. Here a result computed over
  500 bars cannot be returned to a view bounded at 200, because the key does not
  match.
- **`as_of` refuses rather than clamping.** Asking for a bar that has not
  happened is an error, not a request for everything — clamping answers a
  different question quietly, which is how a replay driver's off-by-one becomes
  a profitable-looking backtest.

The one door the type system cannot guard is a bar arriving **stamped in the
future** — clock skew, a broker's server timezone, an off-by-one upstream.
`AsOf` stops a reader looking past what it was given; it cannot make what it was
given honest. `feed::no_future_bars` refuses that at the entry.

## The vocabulary is an enum

`Kind` is an enum and `verify` matches on it exhaustively, so **a claim the
referee cannot check cannot be constructed**. Adding a variant without adding
its reader is a compile error rather than a runtime surprise in front of a live
market. The spec's "adding a claim kind means adding a checker function, one
file, one function" is now enforced by the compiler.

Fourteen kinds, each carrying its `grounds()`:

| grounds | kinds |
|---|---|
| `Measured` — published evidence behind it | `AT_LEVEL`, `LEVEL_BREAK`, `SWEEP` |
| `Untested` — asserted by practitioners, never quantified | `POLARITY` |
| `Arithmetic` — follows from the bars by definition | the other ten |

## No dependencies, and why that is load-bearing

Pure `std`; the whole tree imports only `std::cell` and `std::fmt`. Two places
would normally be borrowed and both are exactly where borrowing is unreliable on
the machine that matters:

- **A timezone crate.** The Python original could not use `zoneinfo` because
  Windows ships no system tz database. `session.rs` hand-rolls the DST rules for
  eight zones. They were validated against IANA over 2024–2027 — 140,256
  offsets, zero mismatches — and the transition instants that run established
  are baked in as literals and checked to the minute.
- **A calendar crate.** `time.rs` is Hinnant's `days_from_civil` /
  `civil_from_days`: branch-free integer arithmetic, round-tripped over every
  day from 1970 to 2070 in the tests.

## What is measured, not assumed

`verify::report()` runs the whole module offline — no clock, no files, no
network — and prints what it found. Current output includes:

- arbitrary levels bounce **54%** on the fixture series (Osler measured 56.2%
  for artificial levels on real FX data — the null is 56%, not 50%, and every
  bounce rate is read against it)
- pure noise reads as a trend **23%** of the time on a 20-bar window at the
  current gate (measured over 60 seeded walks; the Python run measured 31% over
  120 — same order, and the point is that the number is measured at all)
- the efficiency ratio's random-walk baseline is **1/√n** — 0.316 at n=10, so
  the universally quoted "ER > 0.30 means trending" is a threshold *below chance*
- a Hurst test would need **~3,100 bars** to see ρ=0.05, which is why there
  isn't one
- the H4 bar closing 16:00Z **contains** the 12:30 payrolls release; a
  close-only check calls it clean
- 84/84 readings identical when the same bar is reached from a frame that has
  never seen a later bar and from a view of one that has

## The parameter freeze

`params::fingerprint()` hashes every tunable; `params::drift()` compares each
against the live constant in the module that owns it, and the suite fails on any
mismatch. Current: `fa5dc58d1c8f` — **12 values, 8 searchable, 0 configurations tried.** Record the
fingerprint beside any backtest — a result quoted without one cannot be
reproduced.

## What has NOT changed

Still true, and nothing in this port touches it:

> **Nothing yet shows that reading market structure predicts anything on these
> pairs.**

Real bars are still to come. What this buys is that when they
arrive, the code reading them is in Atlas's own language, cannot see the future
by construction, has parameters fixed before anyone saw the data, and checks its
feed at the door.

## Not ported, deliberately

- **The deliberation harness** (two advocates, the referee, the scoreboard,
  timing, the answer cap). Atlas already has `crew.rs` for off-tick work and its
  own notion of earned confidence in `earned.rs`; wiring the debate through
  those rather than transliterating the Python is the right next step and is a
  design decision, not a transcription job.
- **ADX.** Documented in `regime.rs` and not implemented: 26 bars of lag, it
  ignores closes entirely, and it ranked last of thirteen regime filters in the
  one systematic test found. That is a decision, not an omission.
- **Prior-day / prior-week levels.** They need a decision about where the FX day
  ends — 17:00 New York by convention, not midnight UTC — and picking wrong
  would disagree with every broker chart while looking right. Named rather than
  guessed.
