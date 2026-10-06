//! Price bars, and a view that cannot see the future.
//!
//! ## The thing this file does that the Python could not
//!
//! The Python version of this work fought lookahead with discipline and a leak
//! detector: a truncating helper, a rule that readers must be handed a slice,
//! and a test that swept every reader looking for one whose answer changed when
//! the future arrived. That works, and it found real problems. But it is a
//! guard rail beside a cliff — nothing *stops* a function taking the whole
//! series and reading the end of it. The detector exists precisely because the
//! mistake remains makeable.
//!
//! Rust lets the mistake be unmakeable. **No reader in this module takes
//! `&Bars`.** They take [`AsOf`], which owns a borrow of the bars and an upper
//! bound, and hands out `&[f64]` slices that have already been cut at that
//! bound. There is no method on `AsOf` that returns a later price, because
//! there is no method on `AsOf` that returns anything but the truncated slice.
//! A reader cannot peek at bar `n+1` for the same reason it cannot read past
//! the end of a slice: the value is not reachable from what it was given.
//!
//! That is the whole argument for doing this port. Lookahead is the one failure
//! in this system that is silent and flattering — a backtest that peeks does
//! not crash, it produces exactly the result the person running it hoped for.
//! Moving it from "tested for" to "not expressible" is worth more than every
//! other difference between the two languages combined.
//!
//! ## The cache, and the bug that shaped it
//!
//! Swing detection is an interpreted loop over the window and three readers
//! want it, several times, per deliberation. In Python that cache lived on the
//! frame's `attrs`, which pandas **deep-copies on ordinary column access** —
//! measured at a 6x tax on every read, growing with the number of pivots, so
//! the "optimisation" reversed sign as the series got longer.
//!
//! Here the cache is a `RefCell` on `Bars`, keyed by `(upto, k)`. Keying on
//! `upto` is not an optimisation detail: it is what makes the cache safe under
//! truncation. A cache computed over 500 bars can never be returned to a view
//! bounded at 200, because the key does not match — which was the exact
//! contamination path the Python detector was built to catch.

use std::cell::RefCell;

/// One OHLC series, in column form.
///
/// Columns rather than a vector of structs because every reader walks one
/// series at a time — highs for swings, closes for the efficiency ratio — and
/// a slice of `f64` is the shape the arithmetic actually wants.
///
/// `time` is milliseconds since the Unix epoch, UTC, and may be empty: plenty
/// of fixtures have no timestamps and inventing them would make every
/// timeframe conversion downstream confidently wrong. Absence is reported as
/// absence.
#[derive(Debug)]
pub struct Bars {
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    time: Vec<i64>,
    cache: RefCell<Cache>,
}

#[derive(Default, Debug)]
struct Cache {
    // A per-level touch cache used to live here and has been removed. The
    // premise was right -- a touch count is a forward-only fold, so it
    // caches the way `grown` does -- and one detail of the question defeated
    // it: the key has to include the TOLERANCE, and the tolerance is
    // `AsOf::gamma`, a prefix mean that takes a different `f64` on
    // essentially every bar. Measured: 50 distinct values over 50
    // consecutive bars. So the key changed every bar, every lookup missed,
    // and the only thing the cache did was push ten to forty entries a bar
    // that nothing would ever read, plus an eviction sort and a full index
    // rebuild every few hundred bars, for ever.
    //
    // Quantising the tolerance would make it hit. It is not done, because
    // the tolerance is part of the question -- it is the width at which "at
    // a level" is decided -- and rounding the question to make a cache work
    // is the wrong way round.
    //
    // What actually fixed the cost is `levels::STRUCTURE_LOOKBACK_BARS`:
    // each fold is bounded by the structure window rather than by the length
    // of the file, so the work per bar is constant in the file's size. The
    // measurement in
    // `tests/levels_do_not_get_slower_the_longer_the_file.rs` is unchanged by
    // this removal -- flat at about 13 microseconds a bar from 2,000 bars to
    // 32,000 -- which is the evidence that the cache was contributing
    // nothing.
    /// Per `k`: every pivot decided so far, and how far the scan has reached.
    ///
    /// This used to be `(upto, k) -> (highs, lows)` with room for 32 entries,
    /// cleared wholesale when it overflowed. A walk-forward replay asks once
    /// per bar with a different `upto` every time, so it missed on essentially
    /// every call and rescanned the whole visible series each time: O(n) per
    /// bar, O(n²) over a run. Measured on a real twenty-year H1 file, that
    /// was about 27 minutes, and it is why the cost grew faster than the bar
    /// count.
    ///
    /// The shape is wrong rather than the size. **A pivot is decided once and
    /// never revised**: index `i` is a swing high exactly when its `±k`
    /// neighbours say so, and every one of those bars exists as soon as
    /// `i + k` does. A later bar cannot change it. So the answer for `upto` is
    /// the answer for `upto - 1` plus at most one newly-decidable pivot, and
    /// the scan only ever moves forward.
    grown: Vec<Grown>,
    /// Running total of absolute bar-to-bar close change: `drift[i]` is the
    /// sum over bars `1..=i`, so `drift[0]` is zero.
    ///
    /// The third instance of the same thing, and the one that cost the most,
    /// because `AsOf::gamma` is asked once per `levels()` call and `levels`
    /// is called twice per bar in a walk-forward. A prefix sum is the
    /// friendliest case of all: it is exactly truncatable, so a view bounded
    /// earlier reads its own index rather than needing its own scan.
    drift: Vec<f64>,
}

#[derive(Debug)]
struct Grown {
    k: usize,
    /// Highest index tested. Everything up to here is settled.
    tested_through: usize,
    highs: Vec<usize>,
    lows: Vec<usize>,
}

/// Something the bars cannot answer. A refusal is a result, never a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Refusal {}

/// Shorthand used throughout this module.
pub type Answer<T> = Result<T, Refusal>;

pub fn refuse<T>(why: impl Into<String>) -> Answer<T> {
    Err(Refusal(why.into()))
}

impl Bars {
    /// Build from four equal-length columns. Timestamps optional.
    ///
    /// Ragged columns are refused rather than truncated to the shortest. A
    /// silently shortened series is the kind of defect that produces a number
    /// rather than an error, which is the class of failure this whole module
    /// is arranged against.
    pub fn new(
        open: Vec<f64>,
        high: Vec<f64>,
        low: Vec<f64>,
        close: Vec<f64>,
        time: Vec<i64>,
    ) -> Answer<Self> {
        let n = close.len();
        if n == 0 {
            return refuse("a series with no bars in it");
        }
        if open.len() != n || high.len() != n || low.len() != n {
            return refuse(format!(
                "ragged columns: open {}, high {}, low {}, close {}. Truncating \
                 to the shortest would produce a number rather than an error",
                open.len(),
                high.len(),
                low.len(),
                n
            ));
        }
        if !time.is_empty() && time.len() != n {
            return refuse(format!(
                "{} timestamps for {} bars",
                time.len(),
                n
            ));
        }
        // Finite, or it is not a price.
        //
        // This check lived only in `feed::check_ohlc`, which is reachable
        // through `feed::accept` and nowhere else -- so a `Bars` built any
        // other way carried NaN into every comparison downstream. A NaN in a
        // sort is not a wrong answer, it is a panic: `partial_cmp` returns
        // `None` and the caller unwrapped it. Those call sites now use
        // `total_cmp` and cannot panic, but an ordering that quietly sorts
        // NaN to one end is still a made-up answer about your money. It is
        // refused here instead, at the one gate every series passes through.
        //
        // The constructor rather than `feed`, deliberately: length and
        // raggedness are a caller's mistake, but a non-finite value makes
        // every comparison in this type ill-defined. That is a
        // representational invariant. `feed`'s own rules -- positive prices,
        // a possible OHLC -- stay in `feed`, because they are about what a
        // price feed may contain rather than about whether this struct holds
        // a total order. `check_ohlc` keeps its finiteness branch too, as
        // deliberate redundancy: the layer that reports *which* defect a
        // feed has must not start trusting an invariant another module holds.
        //
        // Both chats wrote this check independently on 17 Sep, which is worth
        // recording: it is the strongest evidence either had that the gap was
        // real. This is the merge of the two -- their single pass and their
        // reason, and the value printed, with NaN and infinity named
        // correctly rather than both called "not a number".
        for (name, col) in [("open", &open), ("high", &high), ("low", &low), ("close", &close)] {
            if let Some(i) = col.iter().position(|v| !v.is_finite()) {
                let v = col[i];
                let what = if v.is_nan() { "is not a number" } else { "is not finite" };
                return refuse(format!(
                    "{name} has a value at bar {i} that {what} ({v}). A price \
                     series with a hole in it produces a figure rather than an error, \
                     which is worse than refusing it"
                ));
            }
        }
        Ok(Bars {
            open,
            high,
            low,
            close,
            time,
            cache: RefCell::new(Cache::default()),
        })
    }

    /// Synthetic bars from closes, with a fixed spread. Fixtures only.
    ///
    /// High and low hang off the bar's OWN close, so a close-peak is a
    /// high-peak.
    ///
    /// **`c + spread`, not `max(c, o) + spread`**, and the difference is the
    /// whole function. With `max(c, o)`, a bar whose close is a local peak and
    /// the bar after it BOTH take that same peak as their high -- they tie, the
    /// strict-fractal uniqueness test fails, and no swing forms anywhere in the
    /// series. The first version of this did that and produced a fixture with
    /// exactly zero pivots in 300 bars.
    ///
    /// That is the failure mode worth remembering: the detector was correct
    /// throughout. A fixture that manufactures ties does not fail, it silently
    /// makes every structural test vacuous. `a_fixture_actually_produces_swings`
    /// pins it.
    ///
    /// `spread` must be large relative to the bar-to-bar step or `o` dominates
    /// and the ties come back; `walk()` passes one that is.
    pub fn from_closes(closes: &[f64], spread: f64) -> Answer<Self> {
        let n = closes.len();
        let mut open = Vec::with_capacity(n);
        let mut high = Vec::with_capacity(n);
        let mut low = Vec::with_capacity(n);
        for (i, &c) in closes.iter().enumerate() {
            let o = if i == 0 { closes[0] } else { closes[i - 1] };
            open.push(o);
            high.push((c + spread).max(o).max(c));
            low.push((c - spread).min(o).min(c));
        }
        Bars::new(open, high, low, closes.to_vec(), Vec::new())
    }

    pub fn len(&self) -> usize {
        self.close.len()
    }

    pub fn is_empty(&self) -> bool {
        self.close.is_empty()
    }

    /// The view as it stood at the close of `bar`.
    ///
    /// **The only way to read prices out of this type.** Asking for a bar that
    /// has not happened refuses rather than clamping to the end — a clamp would
    /// silently answer a different question, and answering a different question
    /// quietly is how a replay driver's off-by-one becomes a profitable-looking
    /// backtest.
    pub fn as_of(&self, bar: usize) -> Answer<AsOf<'_>> {
        if bar >= self.len() {
            return refuse(format!(
                "asked for the series as of bar {} but it has {} bars. \
                 Clamping would answer a different question quietly, which is \
                 how a replay driver's off-by-one becomes a profitable-looking \
                 backtest",
                bar,
                self.len()
            ));
        }
        Ok(AsOf { bars: self, upto: bar })
    }

    /// The view at the last bar. What a live system always has.
    pub fn latest(&self) -> Answer<AsOf<'_>> {
        self.as_of(self.len() - 1)
    }

    /// Full-series column access, for the feed checks and for building views.
    ///
    /// Deliberately not `pub`: everything that reads prices to form an opinion
    /// goes through [`AsOf`]. `feed.rs` is in the same module tree and checks
    /// the series as data rather than reading it as a market, which is the one
    /// legitimate reason to see the whole thing at once.
    pub(crate) fn all_open(&self) -> &[f64] {
        &self.open
    }
    pub(crate) fn all_high(&self) -> &[f64] {
        &self.high
    }
    pub(crate) fn all_low(&self) -> &[f64] {
        &self.low
    }
    pub(crate) fn all_close(&self) -> &[f64] {
        &self.close
    }
    pub(crate) fn all_time(&self) -> &[i64] {
        &self.time
    }
}

/// A borrow of the bars, bounded at one of them.
///
/// Every reader in this module takes one of these and nothing else. There is
/// no accessor here that returns a price after `upto`, so a reader cannot use
/// one — not because it would be caught, but because the value is not
/// reachable from what it holds.
///
/// `Copy`, because it is a borrow and a bound, and making callers clone it
/// would push people toward passing `&Bars` around instead. The easy path has
/// to be the honest one.
#[derive(Clone, Copy, Debug)]
pub struct AsOf<'a> {
    bars: &'a Bars,
    upto: usize,
}

impl<'a> AsOf<'a> {
    /// How many bars are visible. Never the length of the underlying series.
    pub fn len(&self) -> usize {
        self.upto + 1
    }

    pub fn is_empty(&self) -> bool {
        false // as_of refuses to build an empty view
    }

    /// The index of the bar being decided at, within the full series.
    pub fn bar(&self) -> usize {
        self.upto
    }

    pub fn open(&self) -> &'a [f64] {
        &self.bars.open[..=self.upto]
    }
    pub fn high(&self) -> &'a [f64] {
        &self.bars.high[..=self.upto]
    }
    pub fn low(&self) -> &'a [f64] {
        &self.bars.low[..=self.upto]
    }
    pub fn close(&self) -> &'a [f64] {
        &self.bars.close[..=self.upto]
    }

    /// Timestamps up to the bound, or empty when the series carries none.
    pub fn time(&self) -> &'a [i64] {
        if self.bars.time.is_empty() {
            &[]
        } else {
            &self.bars.time[..=self.upto]
        }
    }

    /// The close of the bar being decided at.
    pub fn now(&self) -> f64 {
        self.bars.close[self.upto]
    }

    /// **When the bar being decided at OPENED**, if the series is timestamped.
    ///
    /// # The stamp is the open, and that was not written down anywhere
    ///
    /// This replaced `now_time()`, whose doc comment said *"When this bar
    /// closed"* — and that was wrong, which mattered.
    ///
    /// Broker bars are open-stamped, and that can be checked rather than
    /// assumed: an H4 bar stamped 18:00 has exactly the open, high, low and
    /// close of the H1 bars at 18:00, 19:00, 20:00 and 21:00, so it does not
    /// finish until 22:00. A bar's end is `stamp + bar_ms`, and the question
    /// that matters is whether the BAR overlaps a blackout — not whether its
    /// opening instant does.
    ///
    /// Meanwhile this accessor claimed the stamp was a close, and
    /// `standdown::spanning` believed it — feeding the stamp to
    /// `Tf::bar_window(close_ms)`, which returns `(close - span, close)`. On
    /// open-stamped data that is **the previous bar's window**, so the news
    /// blackout was computed one whole bar early: the bar containing a
    /// payrolls print read clean, and the bar after it — hours clear of the
    /// release — was refused.
    ///
    /// Nothing caught it because `tests/standdown.rs` builds its fixtures
    /// close-stamped too (`h4_ending`: *"n H4 bars ending with one that closes
    /// at `last`"*), so the fixture and the code agreed with each other and
    /// only disagreed with real data. Fixed together: the fixture is
    /// open-stamped now, and the properties it asserts are unchanged.
    ///
    /// **Ruled 17 Sep 2026: a bar should know its open and close time.** So
    /// there is no single ambiguous accessor any more — ask for
    /// [`opened_at`](Self::opened_at) or [`closes_at`](Self::closes_at) and
    /// the answer cannot be misread.
    pub fn opened_at(&self) -> Option<i64> {
        self.bars.time.get(self.upto).copied()
    }

    /// How long one bar of this series lasts, in milliseconds.
    ///
    /// The **median** gap between consecutive stamps, not the mean and not the
    /// last one: a weekend is a 65-hour gap and a missing bar is a double gap,
    /// and either would drag an average badly. The median is the bar length as
    /// long as most bars are present, which is the condition
    /// `market/feed.rs` already checks and reports on.
    ///
    /// Derived here from the series itself rather than taken from
    /// `timeframe::infer`, so a bar can answer "when do I close?" without the
    /// timeframe taxonomy having to recognise its spacing. `infer` snaps to a
    /// known `Tf` and returns `None` for anything unfamiliar; a five-minute
    /// series whose stamps are two seconds off still has a perfectly well
    /// defined bar length.
    pub fn bar_span_ms(&self) -> Option<i64> {
        let t = self.time();
        if t.len() < 2 {
            return None;
        }
        // Capped: the span is a property of the series, and 200 gaps settle it
        // as well as a hundred thousand do.
        let mut gaps: Vec<i64> = t.windows(2).take(200).map(|w| w[1] - w[0]).collect();
        gaps.sort_unstable();
        let mid = gaps[gaps.len() / 2];
        (mid > 0).then_some(mid)
    }

    /// **When the bar being decided at CLOSES.**
    ///
    /// `opened_at` plus one bar. `None` when the series carries no stamps, or
    /// carries too few to know how long a bar is — in which case the honest
    /// answer is that the close is unknown, not that it equals the open.
    pub fn closes_at(&self) -> Option<i64> {
        Some(self.opened_at()? + self.bar_span_ms()?)
    }

    /// An earlier view of the same series.
    ///
    /// Refuses to move forward. A view that could be widened would be a view
    /// in name only, and `back_to` is the shape every honest use has anyway —
    /// a break reader asking what structure looked like before the move it is
    /// judging.
    pub fn back_to(&self, bar: usize) -> Answer<AsOf<'a>> {
        if bar > self.upto {
            return refuse(format!(
                "a view bounded at bar {} cannot be moved forward to {}; \
                 widening a view is the one thing it exists to prevent",
                self.upto, bar
            ));
        }
        Ok(AsOf { bars: self.bars, upto: bar })
    }

    /// The same view, `n` bars earlier. Saturates at the start of the series.
    pub fn back(&self, n: usize) -> AsOf<'a> {
        AsOf { bars: self.bars, upto: self.upto.saturating_sub(n) }
    }

    /// Refuse unless at least `need` bars are visible.
    pub fn need(&self, need: usize) -> Answer<()> {
        if self.len() < need {
            return refuse(format!(
                "need at least {} bars, the view has {}",
                need,
                self.len()
            ));
        }
        Ok(())
    }

    /// Average true range over the last `n` visible bars.
    ///
    /// Only the last `n` are touched. It used to build a true range for
    /// **every visible bar** and then average the tail — an O(bars) walk for
    /// an O(n) answer, which in a walk-forward replay is another O(n²) and is
    /// the same shape as the two caches below. No cache is needed to fix it,
    /// because nothing outside the window was ever used: the values were
    /// computed and thrown away.
    ///
    /// Bar-for-bar identical to the old version, including its one edge:
    /// on a series shorter than `n`, bar 0 has no previous close and takes
    /// its own, which makes its true range the bar's own range.
    pub fn atr(&self, n: usize) -> f64 {
        let (h, l, c) = (self.high(), self.low(), self.close());
        let len = c.len();
        let take = if len < n { len } else { n };
        if take == 0 {
            return 0.0;
        }
        let first = len - take;
        let mut total = 0.0;
        for i in first..len {
            let prev = if i == 0 { c[0] } else { c[i - 1] };
            total += (h[i] - l[i]).max((h[i] - prev).abs()).max((l[i] - prev).abs());
        }
        total / take as f64
    }

    /// Mean absolute bar-to-bar close change.
    ///
    /// The level tolerance, and the reason it is this and not a pip count:
    /// it is the width at which a random walk bounces off its own levels at
    /// close to chance, so every measurement read against it has a built-in
    /// null. Measured at 56-58% on synthetic random walks, against Osler's
    /// 56.2% for artificial levels on real FX data.
    /// It is a **prefix mean**, and that is what makes the cost fixable
    /// without approximating anything: the sum over bars `0..=upto` is the
    /// sum over `0..=upto-1` plus one term. It used to re-sum the whole
    /// visible series on every call — O(bars) for a number asked once per
    /// `levels()` call and so twice per bar in a walk-forward, which made it
    /// the largest of the three O(n²) terms in this file. The running sums
    /// are kept on the bars and extended forward only; a narrower view takes
    /// its own entry, which is exact rather than a prefix of someone else's
    /// answer.
    pub fn gamma(&self) -> f64 {
        let c = self.close();
        if c.len() < 2 {
            return pip_size(self.now());
        }
        let total = {
            let all = self.bars.close.as_slice();
            let mut cache = self.bars.cache.borrow_mut();
            // Extend the running sums to cover this view.
            if cache.drift.is_empty() {
                cache.drift.push(0.0);
            }
            while cache.drift.len() <= self.upto && cache.drift.len() < all.len() {
                let i = cache.drift.len();
                let step = (all[i] - all[i - 1]).abs();
                let prev = cache.drift[i - 1];
                cache.drift.push(prev + step);
            }
            cache.drift[self.upto.min(cache.drift.len() - 1)]
        };
        let g = total / (c.len() - 1) as f64;
        if g > 0.0 {
            g
        } else {
            // A perfectly flat series has no scale of its own. Falling back to
            // zero would make "at a level" mean "exactly equal to it", and
            // nothing would ever be at anything.
            pip_size(self.now())
        }
    }

    /// Fractal swing highs and lows: a bar higher (lower) than `k` either side.
    ///
    /// The last `k` bars can never be swings, which is correct and is why a
    /// break is only ever confirmed against structure that had time to form.
    /// See `lookahead::confirmed_swings` for the consequence: a pivot at bar
    /// `i` is not KNOWABLE until bar `i + k`.
    ///
    /// Cached on the underlying bars, keyed by `(upto, k)`. Keying on `upto` is
    /// what makes the cache safe under truncation — a result computed over 500
    /// bars can never be handed to a view bounded at 200, which was the exact
    /// contamination path that had to be hunted with a detector in Python.
    pub fn swings(&self, k: usize) -> (Vec<usize>, Vec<usize>) {
        self.swings_since(k, 0)
    }

    /// The same pivots, from bar `since` onwards.
    ///
    /// `swings` copies **every** pivot the view can see into two fresh
    /// `Vec`s. That is O(pivots) per call; the pivot count grows with the
    /// history; and a walk-forward asks once per bar. So it is one more
    /// O(n²), and the last one in the chain that made `levels::nearest`
    /// unusable on a long file — the swing *scan* was fixed and the swing
    /// *copy* was not.
    ///
    /// The cached indices are ascending, so the start is found by a binary
    /// search and the work is the size of the answer rather than the size of
    /// the file. `swings` is this with `since = 0`, and pays for the lot
    /// because it asked for the lot.
    pub fn swings_since(&self, k: usize, since: usize) -> (Vec<usize>, Vec<usize>) {
        // The highest index this view can possibly have decided: `i` needs
        // `i + k` to exist, and this view sees up to `self.upto`.
        let decidable_through = self.upto.checked_sub(k);

        let (h, l) = (self.bars.high.as_slice(), self.bars.low.as_slice());
        let mut cache = self.bars.cache.borrow_mut();
        let i = match cache.grown.iter().position(|g| g.k == k) {
            Some(i) => i,
            None => {
                cache.grown.push(Grown { k, tested_through: k.wrapping_sub(1), highs: vec![], lows: vec![] });
                cache.grown.len() - 1
            }
        };
        let g = &mut cache.grown[i];

        // Extend the scan forward only. Nothing already decided is revisited,
        // which is the whole point — and is why this is identical to the old
        // full rescan rather than an approximation of it.
        if let Some(through) = decidable_through {
            let from = if g.tested_through == k.wrapping_sub(1) { k } else { g.tested_through + 1 };
            for i in from..=through {
                if i < k || i + k >= h.len() {
                    continue;
                }
                let w = &h[i - k..=i + k];
                if h[i] >= *w.iter().fold(&f64::MIN, |a, b| if b > a { b } else { a })
                    && w.iter().filter(|&&x| x == h[i]).count() == 1
                {
                    g.highs.push(i);
                }
                let w = &l[i - k..=i + k];
                if l[i] <= *w.iter().fold(&f64::MAX, |a, b| if b < a { b } else { a })
                    && w.iter().filter(|&&x| x == l[i]).count() == 1
                {
                    g.lows.push(i);
                }
                g.tested_through = i;
            }
        }

        // A view bounded earlier than the scan has reached takes the prefix.
        // `back_to` exists, so this is a real case and not defensive padding.
        match decidable_through {
            None => (Vec::new(), Vec::new()),
            // Both ends bounded by a binary search over ascending indices:
            // `since` at the front, `through` at the back. The old version
            // took a `take_while` prefix from the front of every pivot the
            // file had ever produced.
            Some(through) => (in_range(&g.highs, since, through), in_range(&g.lows, since, through)),
        }
    }

    /// How many SEPARATE occasions price came within `tol` of `level`, and
    /// the last bar it did, over everything this view can see.
    ///
    /// Two bars closer together than `separation` are one visit — a level
    /// hugged for twenty bars is one test of it, not twenty.
    ///
    /// Cached forward-only on the underlying bars. See `Cache::touched` for
    /// why that is exact rather than an approximation, and
    /// `levels::count_touches` for the plain implementation this must agree
    /// with bar for bar.
    pub fn touches(
        &self,
        level: f64,
        tol: f64,
        separation: usize,
        from: usize,
    ) -> (usize, Option<usize>) {
        let (h, l) = (self.bars.high.as_slice(), self.bars.low.as_slice());
        let to = self.upto.min(h.len().saturating_sub(1));
        let (mut n, mut last) = (0usize, None);
        fold_touches(h, l, level, tol, separation, from, to, &mut n, &mut last);
        (n, last)
    }
}

/// The slice of an ascending index list lying in `from..=to`, copied out.
///
/// Both ends by binary search, so the cost is the size of the answer. The
/// list is ascending by construction — `swings_since` only ever pushes
/// forward — which is what makes the search valid; a `debug_assert` would
/// check it on every call, and the property belongs to the one function that
/// builds the list rather than to every reader of it.
fn in_range(sorted: &[usize], from: usize, to: usize) -> Vec<usize> {
    let start = sorted.partition_point(|i| *i < from);
    let end = sorted.partition_point(|i| *i <= to);
    if start >= end {
        return Vec::new();
    }
    sorted[start..end].to_vec()
}

/// The fold itself, over `from..=to`, carrying the running state in.
///
/// One loop body, two entry points — extending a cached scan forward, and
/// starting from nothing for a view narrower than one. A second copy of this
/// rule would be a second place for the definition of "a separate visit" to
/// drift, and that definition is the whole of what `levels::TOUCH_SEPARATION`
/// is for.
///
/// `to` is inclusive, and `to < from` means there is nothing to do.
fn fold_touches(
    high: &[f64],
    low: &[f64],
    level: f64,
    tol: f64,
    separation: usize,
    from: usize,
    to: usize,
    n: &mut usize,
    last: &mut Option<usize>,
) {
    if to < from || from >= high.len() {
        return;
    }
    for i in from..=to.min(high.len() - 1) {
        if low[i] - tol <= level && level <= high[i] + tol {
            match *last {
                Some(prev) if i - prev <= separation => {}
                _ => *n += 1,
            }
            *last = Some(i);
        }
    }
}

/// The pip for a pair trading around this price.
///
/// A heuristic, and named as one. Quote convention is a property of the
/// instrument, not of the number — but the number separates the two families
/// that matter, and this function is never told the instrument. Override it
/// wherever the instrument IS known rather than letting the guess propagate.
pub fn pip_size(price: f64) -> f64 {
    if price >= 20.0 {
        0.01
    } else {
        0.0001
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::fixtures::{ramp as mkramp, walk};

    fn ramp(n: usize) -> Bars {
        mkramp(n, 0.001)
    }

    #[test]
    fn ragged_columns_are_refused_not_truncated() {
        let e = Bars::new(vec![1.0, 2.0], vec![1.0], vec![1.0], vec![1.0], vec![]);
        assert!(e.is_err());
        assert!(e.unwrap_err().0.contains("ragged"));
    }

    #[test]
    fn an_empty_series_is_refused() {
        assert!(Bars::new(vec![], vec![], vec![], vec![], vec![]).is_err());
    }

    #[test]
    fn a_view_ends_exactly_at_the_bar_asked_for() {
        let b = ramp(100);
        let v = b.as_of(40).unwrap();
        assert_eq!(v.len(), 41);
        assert_eq!(v.bar(), 40);
        assert_eq!(v.close().len(), 41);
        assert_eq!(v.now(), b.all_close()[40]);
    }

    #[test]
    fn a_view_cannot_reach_past_its_bound() {
        // The whole point of the type. Every accessor is cut at the bound, so
        // the later prices are not reachable from what a reader holds.
        let b = ramp(100);
        let v = b.as_of(40).unwrap();
        for series in [v.open(), v.high(), v.low(), v.close()] {
            assert_eq!(series.len(), 41);
        }
        assert!(v.close().iter().all(|&x| x <= b.all_close()[40] + 1e-12));
    }

    #[test]
    fn asking_for_a_bar_that_has_not_happened_refuses() {
        // Clamping would answer a different question quietly, which is how a
        // replay driver's off-by-one becomes a profitable-looking backtest.
        let b = ramp(50);
        let e = b.as_of(50);
        assert!(e.is_err());
        assert!(e.unwrap_err().0.contains("different question"));
    }

    #[test]
    fn a_view_cannot_be_widened() {
        let b = ramp(100);
        let v = b.as_of(40).unwrap();
        assert!(v.back_to(60).is_err());
        assert_eq!(v.back_to(20).unwrap().len(), 21);
        assert_eq!(v.back(10).len(), 31);
    }

    #[test]
    fn going_back_before_the_start_saturates_rather_than_panicking() {
        let b = ramp(30);
        assert_eq!(b.as_of(5).unwrap().back(999).len(), 1);
    }

    #[test]
    fn the_swing_cache_is_keyed_by_the_bound() {
        // The contamination path the Python version needed a leak detector to
        // hunt: a result computed over the whole series being handed to a view
        // that should not have seen it. Here the key makes it impossible.
        let b = walk(300, 7);
        let full = b.latest().unwrap().swings(2);
        let early = b.as_of(120).unwrap().swings(2);
        assert!(early.0.len() < full.0.len());
        assert!(early.0.iter().all(|&i| i <= 120));
        assert!(early.1.iter().all(|&i| i <= 120));
        // Asking again returns the same answer, from the cache, for the bound
        // that asked -- not for the other one.
        assert_eq!(b.as_of(120).unwrap().swings(2), early);
        assert_eq!(b.latest().unwrap().swings(2), full);
    }

    #[test]
    fn the_last_k_bars_can_never_be_swings() {
        let b = walk(200, 3);
        let v = b.latest().unwrap();
        let (hi, lo) = v.swings(2);
        let last = v.len() - 1;
        assert!(hi.iter().all(|&i| i + 2 <= last));
        assert!(lo.iter().all(|&i| i + 2 <= last));
    }

    #[test]
    fn a_flat_series_still_has_a_tolerance() {
        // Zero would make "at a level" mean "exactly equal to it", and nothing
        // would ever be at anything.
        let b = Bars::from_closes(&[1.1; 40], 0.0).unwrap();
        assert!(b.latest().unwrap().gamma() > 0.0);
    }

    #[test]
    fn the_tolerance_is_measured_from_the_bars() {
        let quiet = Bars::from_closes(
            &(0..60).map(|i| 1.1 + (i % 2) as f64 * 1e-5).collect::<Vec<_>>(),
            1e-5,
        )
        .unwrap();
        let wild = walk(60, 11);
        assert!(wild.latest().unwrap().gamma() > quiet.latest().unwrap().gamma() * 3.0);
    }

    #[test]
    fn atr_is_positive_on_a_real_series_and_zero_on_nothing() {
        let b = walk(100, 5);
        assert!(b.latest().unwrap().atr(14) > 0.0);
        let flat = Bars::from_closes(&[1.1; 30], 0.0).unwrap();
        assert_eq!(flat.latest().unwrap().atr(14), 0.0);
    }

    #[test]
    fn a_jpy_price_gets_a_jpy_pip() {
        assert_eq!(pip_size(151.23), 0.01);
        assert_eq!(pip_size(1.1023), 0.0001);
    }

    #[test]
    fn need_refuses_rather_than_letting_a_short_view_answer() {
        let b = ramp(10);
        assert!(b.latest().unwrap().need(25).is_err());
        assert!(b.latest().unwrap().need(5).is_ok());
    }
}
