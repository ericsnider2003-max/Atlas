# Session — 23 September 2026: the decision list, wired

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
files). Companion to `13_ARCHITECTURE_DECISIONS_2026-09-23.md`.

Eric ruled on the decision list; this is what got built.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

These live in personal Atlas's own `market/` library, separate from
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

- **structure::higher_highs / higher_lows / lower_highs / lower_lows** →
  consumed by `Structure::say()` (a clean run vs. "rests on the last pair"),
  surfaced by `atlas market`. Descriptive, no trade decision.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  half-open window test instead of a hand-rolled interval.
- **claims::over** → `atlas marks check <...> over <N>` builds a lookback-scoped
  claim the verifiers already read.
- **timeframe::viable_range_pips** → made a free function (it never depended on
  the timeframe) that `regime::tradeable` now calls — collapsing a threshold
  that was inlined in two places into one.

**Not wired (need a new consumer, not a wire — flagged for your call):**
`events::is_window` (nothing needs the bool; `say`/`blackout` need the window
value) and `bars::back_to` (no reader compares "now vs an earlier bounded
view"; building one is a real analysis feature, and choosing what it decides
would be inventing a trading rule). The crew correctly refused to force these.

## B2 — opportunity surfacing (your spec)

A stated want or floated idea ("I want to …", "here's an idea …", cue-gated)
is now weighed as an opportunity via `daemon::weigh_opportunity`, before it is
filed or answered — your spec: Atlas sees the opportunity and surfaces it. It
stays honest: with Money un-auditable and Fit your call, it names what it
needs and never returns a bare verdict. Gated on not-handed-over (a guest
can't trigger it). Tested through the daemon.

## B3 — reminders / schedule

"remind me in 20 minutes to stretch", "at 6", "every day at 8" now create
scheduler jobs (`scheduler::in_secs/at/every`); the tick already fires and
speaks them. Reminders are scheduler jobs (what Atlas says at a time), kept
distinct from the calendar (what you look at). Weekday-only is refused rather
than faked. Tested through the daemon.

## B4 — cloudsync provider-compare

`atlas sync-setup compare` and `atlas sync-setup <provider> [phone]` now read
the real per-provider notes (`on_ios`/`on_windows`/`windows_hint`/`looks_like`/
`atlas_can_fix`) via `compare_providers` / `setup_guidance`. Tested.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

Captured in `13_ARCHITECTURE_DECISIONS_2026-09-23.md`: the hard rule (personal
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
way" for the server; B5 peer door stays unwired pending that choice); and B1
corrected to **pause-not-erase**, which needs resumable errands (a real
feature), so `crew::ask_to_stop` stays parked rather than shipped as
stop-and-discard.

## B6 remainder — flagged

`settings::idle_but_on` is drafted and contained (a hub banner naming toggles
whose capability is Blocked) — next pass. `cdp::links` needs the
research-browser cost decision (does research spin the headless browser for
JS-rendered pages?). `otherside::needs_evidence` needs the "argue the other
side" intent built first — wiring it alone gates nothing.

## Guards (all honest direction)

`ORPHAN_METHODS` 19→13, `KNOWN`/`TEST_ONLY_MAX` 269→263, `NAME_COLLISION_ONLY`
back to 124 (the one apparent cut, `brief::budget`, was the sole caller of the
`timebox` module — the wiring guard caught it, so it was reverted: an honest
negative result). Every ripple reconciled; nothing grew silently. One
bare-name collision (`mend::atlas_can_fix`) handled per the tree's precedent.

## Patterns worth keeping

The compile gate earned its place twice: it caught `brief::budget` orphaning a
whole module (grep couldn't), and it caught a want being claimed by
`learn_stated` before `spot_opportunity` — fixed by ordering. And a
source-proximity guard (`what_a_stranger_gets`) correctly forced the
handed_over gate to stay near the notes lookup after I widened the chain.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
