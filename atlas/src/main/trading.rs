//! The market commands: reading bars, the Fed calendar, trading and market
//! readings, several time frames at once, and money.
//! 
//! Moved out of `main.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md §7).

use super::*;

/// Look for reclaimable space, show it, and act only if told to.
///
/// Two steps on purpose. `atlas reclaim` shows the list and stops; only
/// `atlas reclaim --do-it` moves anything, and even then it moves to Atlas's
/// trash rather than deleting. Nothing here happens on Atlas's own initiative.
/// Read something from outside, quote it, and say if it tried to give orders.
///
/// The tray rule, generalised: handing Atlas something says *look at this*,
/// never *do what this says*. Nothing read here reaches the parser — there is
/// no route from a `Read` to an intent, which is the actual protection. The
/// detector only says when somebody tried.
/// What Atlas has taken in from outside, and what tried to give it orders.
///
/// `untrusted.rs` states the promise as two halves: nothing Atlas reads may
/// instruct it, **and** the attempts are visible afterwards. `Inbox` holds
/// them and `spoken()` renders them, but until now the only thing that ever
/// printed either was `atlas read` -- so the record could be seen only by
/// adding to it, and there was no way to ask the question on its own. That is
/// the same half-kept shape the comment in `run_read` describes about `Inbox`
/// having had nothing put in it: the rule held and nobody could watch it hold.
pub(super) fn run_fed() {
    let store = atlas::roots::store();
    let inbox: atlas::untrusted::Inbox = store.load("read-from-outside");
    println!("{}", inbox.spoken());

    let tried = inbox.attempts();
    if tried.is_empty() {
        return;
    }
    println!();
    println!("What tried:");
    for r in tried {
        let who = if r.from.is_empty() { "an unnamed source" } else { &r.from };
        println!("  {who}: {}", r.orders_found().join(", "));
        // The text itself, quoted the same way it was quoted to the model --
        // so what is printed here cannot be mistaken for Atlas's own words
        // either.
        for line in r.quoted().lines().skip(1).take(3) {
            println!("    {line}");
        }
    }
}

pub(super) fn run_read(args: &[String]) {
    let (Some(from), Some(path)) = (args.first(), args.get(1)) else {
        println!("atlas read <where it came from> <file>");
        return;
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            println!("I couldn't read {path}: {e}");
            return;
        }
    };
    let r = atlas::untrusted::Read::new(from, &text, atlas::store::now());
    println!("{}", r.quoted());
    match r.worth_telling_him() {
        Some(said) => {
            println!();
            println!("{said}");
        }
        None => {
            println!();
            println!("Nothing in that was written as an instruction to me.");
        }
    }

    // Kept, so "what have you been fed" has an answer. The still-open item in
    // the spec is not just that nothing Atlas reads may instruct it — it is
    // that the attempts are visible afterwards. `Inbox` was written and
    // nothing ever put anything in it, so the promise was half kept: the rule
    // held, and nobody could see it holding.
    let store = atlas::roots::store();
    let mut inbox: atlas::untrusted::Inbox = store.load("read-from-outside");
    inbox.took_in(r, 500);
    println!();
    println!("{}", inbox.spoken());
    keep(store.save("read-from-outside", &inbox), "store");
}

/// A file of candles, as a series their reader will accept.
///
/// Lives here rather than in `market` because turning a file somebody named on
/// a command line into a series is a CLI concern, and `market::feed` is about
/// what happens to a series once it exists. Everything it produces goes
/// straight through `Bars::new` and then `feed::accept`, so a file gets exactly
/// the same refusals as any other source — there is no laxer path in for data
/// that arrived as text.
fn bars_from_file(path: &str) -> std::result::Result<atlas::market::bars::Bars, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("I couldn't read {path}: {e}"))?;
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
        if cells.len() < 5 {
            // The header row, most likely. Skipped only on the first line —
            // a short row further down is a broken file, and silently
            // dropping it would quietly shorten the series.
            if n == 0 {
                continue;
            }
            return Err(format!(
                "line {} has {} columns and I need five: time,open,high,low,close",
                n + 1,
                cells.len()
            ));
        }
        let num = |at: usize| -> std::result::Result<f64, String> {
            cells[at]
                .parse::<f64>()
                .map_err(|_| format!("line {}: '{}' isn't a number", n + 1, cells[at]))
        };
        let when = match cells[0].parse::<i64>() {
            Ok(v) => v,
            Err(_) => {
                if n == 0 {
                    continue; // a header
                }
                return Err(format!("line {}: '{}' isn't a time", n + 1, cells[0]));
            }
        };
        // Seconds in, milliseconds out: everything in `market` is in
        // milliseconds, and a series mixing the two is off by a thousand in a
        // way that reads as a plausible date.
        t.push(when * 1000);
        o.push(num(1)?);
        h.push(num(2)?);
        l.push(num(3)?);
        c.push(num(4)?);
    }
    atlas::market::bars::Bars::new(o, h, l, c, t).map_err(|e| e.0)
}

/// The trading settings as *you* wrote them, or the built-in ones when there
/// is no config to read: the `together:` section of tools.yaml (the
/// say-above/alarm-above thresholds).
fn trade_cfgs() -> atlas::together::TogetherConfig {
    let cfg = Config::load(&atlas::roots::config_dir()).ok();
    let tools = cfg.as_ref().and_then(|c| c.tools.as_ref());
    tools.map(|t| t.together.clone()).unwrap_or_default()
}

/// Your `trading:` section — the risk limits, as you wrote them.
///
/// `trading:` is lines of defended numbers in `tools.yaml` — what fraction of
/// the balance a trade may risk, how far beyond structure a stop sits — and
/// every reader of them used to build `Rules::default()` at the call site, so
/// the section parsed into a field and stopped there.
fn trading_cfg() -> atlas::voice::TradingConfig {
    Config::load(&atlas::roots::config_dir())
        .ok()
        .and_then(|c| c.tools.map(|t| t.trading))
        .unwrap_or_default()
}

/// `atlas trade` — general trading knowledge, for a trade you are in:
///
///   atlas trade exposure EURUSD:long:1 GBPUSD:long:1  what is really being bet on
///   atlas trade stale <pair> <long|short> <entry> <stop> <target> <opened-at> <bars.csv>
//
// 28 Sep 2026: `atlas trade ladder` went with `ladder.rs`, which was not general
// trading knowledge (tests/personal_atlas_is_its_own.rs keeps it out).
pub(super) fn run_trade(args: &[String]) {
    match args.first().map(|s| s.to_lowercase()).as_deref() {
        Some("exposure") => {
            // What is really being bet on, whatever the tickets say.
            // `atlas trade exposure EURUSD:long:1 GBPUSD:long:1 ...`
            let mut held = Vec::new();
            for a in &args[1..] {
                let bits: Vec<&str> = a.split(':').collect();
                if bits.len() != 3 {
                    println!("Each position is <pair>:<long|short>:<risk in R>, like EURUSD:long:1");
                    return;
                }
                let Ok(risk) = bits[2].parse::<f64>() else {
                    println!("'{}' isn't a number of R.", bits[2]);
                    return;
                };
                held.push(atlas::together::Position {
                    pair: bits[0].to_string(),
                    long: bits[1].eq_ignore_ascii_case("long"),
                    risk,
                });
            }
            let cfgt = trade_cfgs();
            print!("{}", atlas::together::spoken(&held, &cfgt));
        }
        Some("stale") => {
            // Whether a trade already running has gone nowhere.
            // atlas trade stale <pair> <long|short> <entry> <stop> <target> <opened-at> <bars.csv>
            //
            // Reads the position against the bars since it was opened.
            let (Some(pair), Some(side), Some(entry), Some(stop), Some(target), Some(opened), Some(path)) = (
                args.get(1),
                args.get(2),
                args.get(3),
                args.get(4),
                args.get(5),
                args.get(6),
                args.get(7),
            ) else {
                println!(
                    "atlas trade stale <pair> <long|short> <entry> <stop> <target> \
                     <opened at, seconds since epoch> <bars.csv>"
                );
                return;
            };
            let side = match side.to_lowercase().as_str() {
                "long" | "buy" => atlas::levels::Side::Buy,
                "short" | "sell" => atlas::levels::Side::Sell,
                other => {
                    println!("'{other}' isn't long or short.");
                    return;
                }
            };
            let (Ok(entry), Ok(stop), Ok(target)) =
                (entry.parse::<f64>(), stop.parse::<f64>(), target.parse::<f64>())
            else {
                println!("Entry, stop and target are all prices.");
                return;
            };
            let Ok(opened_at) = opened.parse::<i64>() else {
                println!("The opened-at time is seconds since the epoch, in UTC.");
                return;
            };
            let bars = match bars_from_file(path) {
                Ok(b) => b,
                Err(e) => {
                    println!("{e}");
                    return;
                }
            };
            let view = match bars.latest() {
                Ok(v) => v,
                Err(e) => {
                    println!("{}", e.0);
                    return;
                }
            };
            let open =
                atlas::stale::Open { side, entry, stop, target, opened_at: opened_at * 1000 };
            let cfg = atlas::stale::StaleConfig::default();
            // Same window `levels::propose` reads average range over, so a
            // pace read here means the same thing it would mean there.
            let range = view.atr(trading_cfg().levels.range_window);
            let average_range = (range.is_finite() && range > 0.0).then_some(range);
            match atlas::stale::how_its_going(&open, &view, average_range, &cfg) {
                Ok(going) => {
                    println!("{pair}: {}", going.plain());
                }
                Err(e) => println!("{e}"),
            }
        }
        _ => println!(
            "atlas trade exposure <pair:long|short:R> ..., \
             or atlas trade stale <pair> <long|short> <entry> <stop> <target> <opened-at> <bars.csv>"
        ),
    }
}

/// Read a file of bars and say what the market is doing.
///
/// The way in for the market-state work, and deliberately a file rather than
/// a broker connection. Atlas holds no prices and has no business holding
/// any. A file also means the same bars can be read twice and give the
/// same answer, which a live feed can never promise.
///
/// `atlas market bars.csv EURUSD` reads it. Adding `buy` or `sell` also works
/// out where the stop and target would go for that direction — never which
/// direction, which is decided by something that can be held to an outcome.
///
/// The pair is named rather than guessed. It could be pulled out of the
/// filename, and that would work for `EURUSD_M15.csv` and silently produce the
/// wrong calendar for `january.csv` — a US release moves AUD/JPY with no dollar
/// in it, so getting the instrument wrong is not a cosmetic error. A guess that
/// is right most of the time is the worst kind here, because the times it is
/// wrong are the times it matters.
pub(super) fn run_market(args: &[String]) {
    let Some(path) = args.first() else {
        println!("Give me a file of bars and the pair it is:");
        println!("  atlas market bars.csv EURUSD [buy|sell] [spread] [held positions...]");
        println!("Five columns, oldest first: time,open,high,low,close.");
        println!("Time is seconds since the epoch, in UTC.");
        println!(
            "Held positions, if any, are <pair>:<long|short>:<risk in R> -- same shape as \
             `atlas trade exposure`."
        );
        return;
    };
    let bars = match bars_from_file(path) {
        Ok(b) => b,
        Err(e) => {
            println!("{e}");
            return;
        }
    };

    // Through the door, before anything reads it as a market. Impossible OHLC
    // makes true range negative and every magnitude downstream wrong; bars out
    // of order mean "the last twenty" are not the recent ones. Neither
    // produces an error anywhere else — both produce a number.
    match atlas::market::feed::accept(&bars, None) {
        Ok(report) => {
            if !report.clean() {
                println!("{}", report.say());
                println!();
            }
        }
        Err(e) => {
            println!("I won't read these bars: {}", e.0);
            return;
        }
    }

    let view = match bars.latest() {
        Ok(v) => v,
        Err(e) => {
            println!("{}", e.0);
            return;
        }
    };

    // The replay check that used to live here is gone, and its absence is the
    // point. It compared reading one bar at a time against reading them all at
    // once -- and `AsOf` makes the second thing unrepresentable. A reader is
    // handed a borrow bounded at a bar and there is no accessor on it that
    // returns a later price, so the two cannot differ. A test that cannot fail
    // is worse than no test.
    if args.iter().any(|a| a.eq_ignore_ascii_case("live")) {
        println!("There's nothing to replay against any more.");
        println!();
        println!("That check compared reading bar by bar against reading the lot at once. A");
        println!("reader now takes a view bounded at one bar and there is no way to ask it for");
        println!("a later price -- not because it would be caught, but because the value isn't");
        println!("reachable from what it holds. The two readings cannot differ.");
        println!();
        println!("What is still worth watching is the bar that hasn't closed: `atlas::live`.");
        return;
    }

    // The pair, named. Everything below that consults the calendar needs it,
    // and there is nowhere honest to get it from except the command line.
    let Some(pair) = args.get(1).map(|s| s.to_uppercase()) else {
        println!("Which pair are these? atlas market {path} EURUSD [buy|sell] [spread]");
        println!("I need it to know which releases move this, and a US print moves AUD/JPY");
        println!("with no dollar in it at all — so it isn't something to guess at.");
        return;
    };
    if pair.eq_ignore_ascii_case("buy") || pair.eq_ignore_ascii_case("sell") {
        println!("That's the direction, and the pair comes first now:");
        println!("  atlas market {path} EURUSD {} <spread>", pair.to_lowercase());
        return;
    }

    println!("{} bars, up to {:.5}.", bars.len(), view.now());

    // Your `trading:` section, read once for the whole command. Everything
    // below that used to carry a literal takes its number from here: 120 and
    // 2 were `structure_bars` and `pivot_reach` written out again, and 50.0
    // was `level_span_pips`. Three copies of a config value, none of them the
    // config value -- change the file and this command read the same market
    // the same way. The pairing is not invented here either: `levels::propose`
    // already calls `structure::recent(view, rules.structure_bars,
    // rules.pivot_reach)`, so this is the same reading its own module does.
    let trading = trading_cfg();
    let structure = atlas::market::structure::recent(
        &view,
        trading.levels.structure_bars,
        trading.levels.pivot_reach,
    );
    // The full structure reading, not just the bare trend label: it names the
    // pivots, whether the run steps cleanly or rests on the last pair, and any
    // reversal. `structure.say()` is the module's own reading, and this is the
    // one place a person reads the market by hand.
    println!("{}", structure.say());
    // When it has turned, what it looked like just before -- read through a
    // view bounded at the bar before the break (`AsOf::back_to`), so it is
    // what could have been known then. Descriptive: before and after, no call.
    if let Some(before) = atlas::market::structure::before_the_turn(
        &view,
        trading.levels.structure_bars,
        trading.levels.pivot_reach,
    ) {
        println!("  {before}");
    }
    match atlas::market::regime::read(&view, None, 0.8, 0.3, None, 0) {
        Ok(r) => println!("{}", r.say()),
        Err(e) => println!("  (couldn't read the regime: {})", e.0),
    }
    match atlas::market::levels::levels(
        &view,
        trading.levels.pivot_reach,
        trading.levels.level_span_pips,
    ) {
        Ok(found) if !found.is_empty() => {
            println!("  levels in reach:");
            for l in found.iter().take(6) {
                println!("    {}", l.say());
            }
        }
        Ok(_) => println!("  no levels within reach of price"),
        Err(e) => println!("  (couldn't read levels: {})", e.0),
    }
    if let Some(tf) = atlas::market::timeframe::infer(&view) {
        println!("  these look like {} bars", tf.name());
    }

    // The last bar, read as though it were still drawing rather than closed
    // -- which, on a feed that is actually live, it is. Every closed-bar
    // reader above this line is, at the moment it matters, looking at a bar
    // that has not finished; this is the one place that says so rather than
    // treating the file's last row as settled fact.
    {
        let mut live = atlas::live::Live::new(&pair, view.len(), 120, 2);
        let last = view.len() - 1;
        let primed = live.prime(
            &view.open()[..last],
            &view.high()[..last],
            &view.low()[..last],
            &view.close()[..last],
            if view.time().is_empty() { &[] } else { &view.time()[..last] },
        );
        if let Err(e) = primed {
            println!("  (couldn't read the forming bar: {})", e.0);
        } else {
            let t = view.time().last().copied().unwrap_or(0);
            // Reconstructs the real bar's open/high/low/close by feeding them
            // through `tick()` in an order that lets the running high and low
            // land on the true extremes: the open first (which seeds both),
            // then the high and low (whichever order), then the close last so
            // it becomes `at` without disturbing either extreme.
            live.tick(view.open()[last], t);
            live.tick(view.high()[last], t);
            live.tick(view.low()[last], t);
            live.tick(view.close()[last], t);
            match live.not_ready() {
                Some(why) => println!("  {why}"),
                None => match live.now() {
                    Ok(now) => println!("  {}", now.spoken()),
                    Err(e) => println!("  (couldn't compare settled against forming: {})", e.0),
                },
            }
        }
    }

    // Yesterday's levels, off a day that ends at 17:00 New York.
    println!("{}", atlas::fxday::spoken(&view));
    let watched = atlas::fxday::levels(&view);
    if !watched.is_empty() {
        println!("  and the lines off it, nearest first:");
        for (at, what) in watched.iter().take(5) {
            println!("    {at:.5}  {what}");
        }
    }

    // Last night, and the two lines London opened into.
    println!("{}", atlas::asia::spoken(&view));
    if let Some(night) = atlas::asia::last_night(&view) {
        if !night.still_forming {
            println!("  price is {}", night.where_price_sits(view.now()));
        }
    }

    // The state of the book at this minute, and what holding this would cost.
    // Two nights out, because that is the shortest hold that can touch a
    // rollover at all and it is enough to show a Wednesday when there is one.
    if let Some(now) = view.opened_at() {
        println!(
            "{}",
            atlas::rollover::spoken(now, Some(now + 2 * atlas::market::time::MS_PER_DAY))
        );
    }

    // What the calendar says, before any of the above is treated as a reason
    // to act. Printed even when no direction was asked for, because "I'd have
    // no view here whatever the chart says" is the most useful single line on
    // the screen on the twelve minutes a month it is true.
    let standdown = atlas::standdown::StanddownConfig::default();
    println!();
    println!("{}", atlas::standdown::spoken(&view, &pair, &standdown));

    let side = match args.get(2).map(|s| s.to_lowercase()) {
        Some(s) if s == "buy" => Some(atlas::levels::Side::Buy),
        Some(s) if s == "sell" => Some(atlas::levels::Side::Sell),
        Some(other) => {
            println!("I don't know what \"{other}\" means here — say buy or sell.");
            None
        }
        None => None,
    };
    let Some(side) = side else { return };

    // The spread is not in a bars file, so it is asked for rather than
    // assumed. Assuming one would make every refusal that turns on cost
    // wrong, and cost is the check that matters most at small timeframes.
    let spread = args
        .get(3)
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    if spread <= 0.0 {
        println!();
        println!("Add the current spread as a fourth argument, in price rather than pips");
        println!("(atlas market bars.csv EURUSD buy 0.00015). Without it I'd be pricing the trade");
        println!("as though it were free, and the cost is the thing most likely to refuse it.");
        return;
    }
    // Anything after the spread is what's already held, in the same
    // <pair>:<long|short>:<risk in R> shape `atlas trade exposure` uses --
    // one syntax for "what's on" everywhere, rather than a second one
    // invented here. Typed inline rather than persisted: a held list that
    // can drift from what's actually open is worse than one that has to be
    // given fresh, and `exposure` already works this way.
    let mut held: Vec<atlas::together::Position> = Vec::new();
    for a in &args[4.min(args.len())..] {
        let bits: Vec<&str> = a.split(':').collect();
        if bits.len() != 3 {
            println!(
                "'{a}' isn't a held position -- each one is <pair>:<long|short>:<risk in R>, \
                 like EURUSD:long:1"
            );
            return;
        }
        let Ok(risk) = bits[2].parse::<f64>() else {
            println!("'{}' isn't a number of R.", bits[2]);
            return;
        };
        held.push(atlas::together::Position {
            pair: bits[0].to_string(),
            long: bits[1].eq_ignore_ascii_case("long"),
            risk,
        });
    }

    let purse = atlas::levels::Purse { balance: 10_000.0, value_per_point: 1.0 };
    let rules = trading.levels;
    println!();

    // What the claim vocabulary says about this side, checked against the
    // same view -- alongside the structural idea below, not instead of it.
    // These two readers have never been shown to each other in this command
    // before: `propose` works from swing structure, `claims::verify` from
    // its own vocabulary, and nothing printed both together. Sorted by
    // magnitude, since that's the number a claim carries and discarding it
    // is exactly the gap the 12 Sep report named -- a verdict is either
    // true or it isn't, but "true by a little" and "true by a lot" are not
    // the same sentence.
    let mut supporting: Vec<(atlas::market::claims::Kind, atlas::market::claims::Verdict)> =
        atlas::market::claims::ALL_KINDS
            .iter()
            .map(|&k| {
                let v = atlas::market::claims::verify(
                    &atlas::market::claims::Claim::new(k, side.as_claim()),
                    &view,
                );
                (k, v)
            })
            .filter(|(_, v)| v.holds())
            .collect();
    supporting.sort_by(|a, b| b.1.magnitude.total_cmp(&a.1.magnitude));
    if supporting.is_empty() {
        println!("Nothing in the claim vocabulary backs {} here.", side.as_claim().say());
    } else {
        println!("Claims backing {}, strongest first:", side.as_claim().say());
        for (k, v) in &supporting {
            println!("  {} — {}", k.name(), v.detail);
        }
    }
    println!();

    // Kept either way. A record of what Atlas did, with no record of what it
    // declined to do, cannot tell being careful from being broken — and those
    // two look identical from outside, because both of them produce silence.
    let store = atlas::roots::store();
    let mut turned_down = atlas::refusals::Refusals::load(&store);
    match atlas::levels::propose(&view, &pair, side, view.now(), spread, &purse, &rules) {
        Ok(idea) => {
            println!("{}", idea.spoken());
            println!("  on a 10,000 balance risking 1%: {:.0} units", idea.size);
            println!("  the spread costs {:.1}% of what it's aiming at", idea.cost_share * 100.0);
            turned_down.proposed(&pair);

            // Asked before the trade, which is the only time the answer is
            // useful. `propose` sizes every idea to exactly `risk_fraction`
            // of the balance by construction, so this is 1R by definition,
            // not a number that needs separately working out.
            let adding = atlas::together::Position {
                pair: pair.clone(),
                long: side == atlas::levels::Side::Buy,
                risk: 1.0,
            };
            let cfgt = trade_cfgs();
            if let Some(warning) = atlas::together::if_i_add(&held, &adding, &cfgt) {
                println!();
                println!("  {warning}");
            }
        }
        Err(no) => {
            println!("No trade there — {}.", no.plain());
            turned_down.declined(&pair, no.label(), no.plain(), atlas::store::now());
        }
    }
    keep(turned_down.save(&store), "that refusal");
    println!();
    println!("{}", turned_down.spoken(&pair));
}

/// Reads one bars file per timeframe for the same instrument and reports
/// whether they agree — general capability, not specific to any one
/// business. See `market::multiframe`'s own module doc for what this is
/// and, just as importantly, what it is not: a proven edge. It's the
/// ability to ask the question, confirmed against real data to actually
/// work, not a claim about what the answer is worth.
pub(super) fn run_multiframe(args: &[String]) {
    if args.len() < 2 {
        println!("Give me at least two bars files, one per timeframe:");
        println!("  atlas multiframe bars_h1.csv bars_h4.csv [bars_d.csv ...]");
        println!("Each file: five columns, oldest first: time,open,high,low,close.");
        return;
    }
    let mut all_bars = Vec::new();
    for path in args {
        match bars_from_file(path) {
            Ok(b) => all_bars.push(b),
            Err(e) => {
                println!("Couldn't read {path}: {e}");
                return;
            }
        }
    }
    let mut views = Vec::new();
    for b in &all_bars {
        let v = match b.latest() {
            Ok(v) => v,
            Err(e) => {
                println!("One of the files has no bars in it: {}", e.0);
                return;
            }
        };
        views.push(v);
    }
    let pairs: Vec<(&atlas::market::bars::AsOf, usize)> = views.iter().map(|v| (v, 2)).collect();
    let reads = atlas::market::multiframe::read_all(&pairs);
    println!("{}", atlas::market::multiframe::spoken(&reads));
}

/// `atlas mobile` — what Atlas can and cannot do on a phone, and what the
/// mirror would actually carry.
///
/// Three modules answer this and they disagree by platform, which is the
/// point: `ios` and `android` are not one list with a flag, because the
/// difference that matters (background listening) is a hard "no" on one and a
/// plain "yes" on the other.
/// What a month of statements actually says.
///
/// Every piece of this existed and none of it was reachable.
/// `finance::parse_csv` reads a bank export and had no caller;
/// `money::sort_one` puts a line in a bucket and had no caller;
/// `summarise`, `spoken`, `new_or_grown` and `work_spend` had no callers. The
/// daemon's money branch called `money::spoken(&money::summarise(&[]), &[])`
/// -- an empty slice, so "Nothing to go on." was the only answer it could
/// give, and the reason was that nothing had read a statement rather than
/// that there was nothing in it.
///
/// `money.work_words` was the dead setting underneath: it decides which lines
/// are the business rather than the household, and nothing sorted anything.
pub(super) fn run_money(cfg: &Config, args: &[String]) {
    use atlas::finance::{self, Source};
    use atlas::money::{self, Entry};

    let store = atlas::roots::store();
    let mcfg = cfg.tools.as_ref().map(|t| t.money.clone()).unwrap_or_default();
    let fcfg = cfg.tools.as_ref().map(|t| t.finance.clone()).unwrap_or_default();
    let now = atlas::store::now();
    let flag = |name: &str| atlas::cli::flag_value(args, name);

    let this: Vec<Entry> = store.load(money::THIS_MONTH);
    let last: Vec<Entry> = store.load(money::LAST_MONTH);
    let mut months: Vec<money::KeptMonth> = store.load(money::MONTHS);

    // `finance.category_jump` is read here rather than in `finance.rs`: it is
    // a fraction about spending over two months, and the comparing is
    // `money`'s. The setting stays where a person would look for it.
    let jump = fcfg.category_jump as f32;

    let say = |this: &[Entry], last: &[Entry]| {
        if this.is_empty() {
            println!("No statement read yet. `atlas money <export.csv>`.");
            return;
        }
        let month = money::summarise(this);
        let mut changes = money::new_or_grown(this, last, jump);
        changes.extend(money::buckets_that_jumped(this, last, jump));
        println!("{}", money::spoken(&month, &changes));
        println!();
        for (b, total) in &month.by_bucket {
            println!("  {:<34} {:.0}", b.plain(), total);
        }
        let work = money::work_spend(this);
        if work > 0.0 {
            println!();
            println!("{work:.0} of that was the business, kept apart on purpose.");
        }
        if changes.len() > 1 {
            println!();
            println!("Worth a look:");
            for c in changes.iter().skip(1) {
                println!("  {c}");
            }
        }
    };

    let Some(path) = args.first().filter(|a| !a.starts_with("--")).cloned() else {
        if !mcfg.enabled {
            println!("Reading statements is switched off (`money.enabled: true` in tools.yaml).");
            println!();
        }
        say(&this, &last);
        // Against your own retained months, not just against last month.
        // Two months can only say "different from October"; the baseline
        // says "different from you". The month on display was itself the
        // last one kept, so it is excluded from its own baseline.
        let past = if this.is_empty() || months.is_empty() {
            &months[..]
        } else {
            &months[..months.len() - 1]
        };
        for line in money::unusual_buckets(past, &this) {
            println!("  {line}");
        }
        println!();
        println!("atlas money <export.csv> [--from file|export|feed] [--keep]");
        println!("  --keep  make this the month I answer about, and keep the old one to compare");
        return;
    };

    let Ok(text) = std::fs::read_to_string(&path) else {
        println!("I can't read {path}.");
        return;
    };
    let source = match flag("--from").as_deref() {
        None | Some("file") => Source::LocalFile,
        Some("export") => Source::SiteExport,
        Some("feed") => Source::Aggregator,
        Some("email") => Source::Notification,
        Some(other) => {
            println!("I don't know a source called \"{other}\". Try file, export, feed or email.");
            return;
        }
    };
    let txns = finance::parse_csv(&text, source);
    if txns.is_empty() {
        println!("Nothing I could read in {path}.");
        println!("It needs a header row with a date column and a description column --");
        println!("the names vary by bank and are matched by meaning, not position.");
        return;
    }

    // Sorted with your `work_words`, which is the whole reason that setting
    // exists: a camera and a domain renewal are the business, and a summary
    // that mixes them with the groceries tells you what the content cost you
    // at the end of the year rather than as you go.
    let fresh: Vec<Entry> = txns
        .iter()
        .map(|t| Entry {
            description: t.description.clone(),
            amount: t.amount as f32,
            at: now,
            bucket: money::sort_one(&t.description, t.amount as f32, &mcfg.work_words),
            confirmed: false,
            account: t.source.describe().to_string(),
        })
        .collect();

    println!("{} lines read from {}.", fresh.len(), t_source(source));
    println!();
    say(&fresh, &this);

    // What the bank itself should have told you: anything large, anything
    // billed twice, anything quietly repeating.
    let flags = finance::review(&txns, &fcfg);
    println!();
    println!("{}", finance::summary(&flags));
    for line in money::unusual_buckets(&months, &fresh) {
        println!("  {line}");
    }

    if args.iter().any(|a| a == "--keep") {
        keep(store.save(money::LAST_MONTH, &this), "store");
        keep(store.save(money::THIS_MONTH, &fresh), "store");
        // The line that used to destroy history. The displaced month now
        // lands in the retained series instead of nowhere, which is what
        // makes the per-bucket baselines above possible at all.
        money::remember_month(&mut months, &fresh, now);
        keep(store.save(money::MONTHS, &months), "store");
        println!();
        println!("Kept. Ask me about the month and this is what I'll answer from.");
    } else {
        println!();
        println!("Not kept -- add `--keep` if this is the month I should answer about.");
    }
}

/// Where a statement came from, said rather than named.
fn t_source(s: atlas::finance::Source) -> &'static str {
    s.describe()
}
