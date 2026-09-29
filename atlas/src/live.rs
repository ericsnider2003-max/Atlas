//! The bar that has not closed yet.
//!
//! ## What is left of this module after the merge
//!
//! Most of it went. It used to hold its own replay machinery — prime a window,
//! push bars through one at a time, and check that reading them that way gave
//! the same answer as reading them all at once. `market::bars::AsOf` does that
//! by construction: a view bounded at bar *n* cannot reach bar *n+1*, so there
//! is nothing left to check and nothing left to get wrong. A test that cannot
//! fail is worse than no test, so it went with the rest.
//!
//! What survives is the one thing `Bars` genuinely does not model: **a candle
//! that is still forming.**
//!
//! ## Why that matters more than it sounds
//!
//! Every closed-bar reader in existence is, at the moment it matters, looking
//! at a bar that has not finished. Price is through the level *right now*. The
//! break is on the screen. And somewhere between a third and half the time it
//! is not there when the bar closes.
//!
//! A system that reads the forming bar as though it were settled will take
//! those. A system that ignores the forming bar entirely will be told about
//! every move one bar late, which on H4 is four hours. Neither is right, and
//! the difference between them is not a threshold — it is **saying which one
//! you are looking at**.
//!
//! So `Now` carries both readings and names the gap between them. "The
//! structure has turned" and "the structure will have turned if this candle
//! closes here" are different sentences, and only one of them is a fact.

use crate::market::bars::{Answer, Bars};
use crate::market::structure::{self, Trend};

/// Which of the two readings this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Firmness {
    /// Closed bars only. What a backtest would have seen.
    Settled,
    /// Including the candle still being drawn. What the screen shows.
    Forming,
}

impl Firmness {
    pub fn plain(self) -> &'static str {
        match self {
            Firmness::Settled => "on closed bars",
            Firmness::Forming => "including the bar still forming",
        }
    }
}

/// One unfinished candle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Forming {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    /// Where it is now. Not a close — it has not closed.
    pub at: f64,
    pub started: i64,
}

impl Forming {
    pub fn open_at(price: f64, started: i64) -> Forming {
        Forming { open: price, high: price, low: price, at: price, started }
    }

    /// Price moved. Extends the high and low the way a real candle does.
    pub fn tick(&mut self, price: f64) {
        if !price.is_finite() || price <= 0.0 {
            return;
        }
        self.high = self.high.max(price);
        self.low = self.low.min(price);
        self.at = price;
    }
}

/// Both readings, and what separates them.
#[derive(Debug, Clone, PartialEq)]
pub struct Now {
    pub settled: Trend,
    /// `None` when no candle is forming, which is the honest answer between
    /// bars rather than a repeat of the settled reading.
    pub forming: Option<Trend>,
    /// True when the two disagree — the case worth saying out loud.
    pub only_if_it_closes_here: bool,
    pub settled_bars: usize,
}

impl Now {
    /// What to act on, given how firm you need it.
    ///
    /// Asking for `Settled` when a bar is forming returns the settled reading,
    /// which is the point: the caller chooses, and neither reading is silently
    /// substituted for the other.
    pub fn at(&self, how: Firmness) -> Trend {
        match how {
            Firmness::Settled => self.settled,
            Firmness::Forming => self.forming.unwrap_or(self.settled),
        }
    }

    pub fn spoken(&self) -> String {
        if !self.only_if_it_closes_here {
            return format!(
                "{} on {} closed bars{}.",
                self.settled.say(),
                self.settled_bars,
                if self.forming.is_some() { ", and the forming bar agrees" } else { "" }
            );
        }
        format!(
            "On closed bars: {}. Including the bar still forming: {}. That second one isn't a \
             fact yet — it's what will be true if this candle closes where it is.",
            self.settled.say(),
            self.forming.map(|t| t.say()).unwrap_or("nothing"),
        )
    }
}

/// A pair being watched as it moves.
///
/// Holds the closed bars as columns rather than as a `Bars`, because `Bars` is
/// immutable once built — which is the right shape for a reader and the wrong
/// one for something that grows. A `Bars` is built on demand from them.
#[derive(Debug, Clone)]
pub struct Live {
    pub pair: String,
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    time: Vec<i64>,
    forming: Option<Forming>,
    /// How many closed bars to keep.
    keep: usize,
    /// Bars of structure the trend is read over.
    window: usize,
    /// Bars either side of a turning point.
    reach: usize,
}

impl Live {
    pub fn new(pair: &str, keep: usize, window: usize, reach: usize) -> Live {
        Live {
            pair: pair.trim().to_string(),
            open: Vec::new(),
            high: Vec::new(),
            low: Vec::new(),
            close: Vec::new(),
            time: Vec::new(),
            forming: None,
            keep: keep.max(60),
            window: window.max(20),
            reach: reach.max(1),
        }
    }

    pub fn settled_bars(&self) -> usize {
        self.close.len()
    }

    pub fn is_forming(&self) -> bool {
        self.forming.is_some()
    }

    /// Load the history this starts from.
    pub fn prime(&mut self, o: &[f64], h: &[f64], l: &[f64], c: &[f64], t: &[i64]) -> Answer<()> {
        // Straight through `Bars::new`, so the same refusals apply to primed
        // history as to anything else — ragged columns, an empty series. A
        // second, laxer path into the same data is how one of them ends up
        // being the one that is actually used.
        let checked = Bars::new(o.to_vec(), h.to_vec(), l.to_vec(), c.to_vec(), t.to_vec())?;
        self.open = o.to_vec();
        self.high = h.to_vec();
        self.low = l.to_vec();
        self.close = c.to_vec();
        self.time = t.to_vec();
        let _ = checked;
        self.trim();
        Ok(())
    }

    /// A bar closed.
    ///
    /// Clears the forming candle. Leaving it would count the same price action
    /// twice — once as the bar that closed and again as the candle that had
    /// been drawing it.
    pub fn closed(&mut self, o: f64, h: f64, l: f64, c: f64, t: i64) {
        self.open.push(o);
        self.high.push(h);
        self.low.push(l);
        self.close.push(c);
        self.time.push(t);
        self.forming = None;
        self.trim();
    }

    /// Price moved inside the current bar.
    pub fn tick(&mut self, price: f64, started: i64) {
        match &mut self.forming {
            Some(f) => f.tick(price),
            None => self.forming = Some(Forming::open_at(price, started)),
        }
    }

    fn trim(&mut self) {
        while self.close.len() > self.keep {
            self.open.remove(0);
            self.high.remove(0);
            self.low.remove(0);
            self.close.remove(0);
            if !self.time.is_empty() {
                self.time.remove(0);
            }
        }
    }

    /// The closed bars, as a series a reader will accept.
    pub fn settled(&self) -> Answer<Bars> {
        Bars::new(
            self.open.clone(),
            self.high.clone(),
            self.low.clone(),
            self.close.clone(),
            self.time.clone(),
        )
    }

    /// The closed bars with the forming candle appended as though it had
    /// closed where it is.
    ///
    /// **This is a hypothetical and the type cannot say so.** It comes back as
    /// an ordinary `Bars`, indistinguishable from real history, which is why
    /// nothing outside this module is handed one — `now()` reads it and returns
    /// a `Trend`, and the series itself never escapes.
    fn as_though_closed(&self) -> Option<Answer<Bars>> {
        let f = self.forming?;
        let mut o = self.open.clone();
        let mut h = self.high.clone();
        let mut l = self.low.clone();
        let mut c = self.close.clone();
        let mut t = self.time.clone();
        o.push(f.open);
        h.push(f.high);
        l.push(f.low);
        c.push(f.at);
        if !t.is_empty() {
            t.push(f.started);
        }
        Some(Bars::new(o, h, l, c, t))
    }

    /// Both readings.
    pub fn now(&self) -> Answer<Now> {
        let settled_bars = self.settled()?;
        let view = settled_bars.latest()?;
        let settled = structure::recent(&view, self.window, self.reach).trend();

        let forming = match self.as_though_closed() {
            None => None,
            Some(Err(_)) => None,
            Some(Ok(with)) => {
                let v = with.latest()?;
                Some(structure::recent(&v, self.window, self.reach).trend())
            }
        };

        Ok(Now {
            settled,
            forming,
            only_if_it_closes_here: forming.is_some_and(|f| f != settled),
            settled_bars: self.close.len(),
        })
    }

    /// Is there enough history to read anything yet?
    pub fn not_ready(&self) -> Option<String> {
        if self.close.len() >= self.window {
            return None;
        }
        Some(format!(
            "I've got {} closed bars on {} and I read structure over {}. Saying anything now \
             would be saying it about a window I don't have",
            self.close.len(),
            self.pair,
            self.window
        ))
    }
}
