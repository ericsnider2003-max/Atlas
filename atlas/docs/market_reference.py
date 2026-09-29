"""Reference implementation of Atlas's market-state arithmetic.

Written to be run, so the Rust can be written against numbers that were
actually computed rather than against my opinion of what they should be.
Every value printed at the bottom goes into tests/market.rs as a fixed
expectation.

Deliberately plain Python: no pandas, no talib. The point is that the rules
are visible and the Rust can mirror them line for line.
"""

import math

# ---------------------------------------------------------------------------
# A series where every feature is known to fire
# ---------------------------------------------------------------------------

def series():
    """Deterministic OHLC built from an explicit zigzag.

    Derived from turning points rather than from a smooth line, because a
    smooth line has no local extremes: an up-leg made of rising closes has
    monotonically rising lows, so its pullbacks are not fractal lows and none
    of the structure rules can fire. Real price turns; the fixture has to too.

    The path, and what each part is here to prove:
      100 -> 103   a leg up                       -> a swing high
      103 -> 101.5 a pullback                     -> a swing low
      101.5 -> 106 continuation, closes above 103 -> bullish CHoCH then BOS
      106 -> 104   a pullback                     -> a swing low
      104 -> 108   another leg up                 -> bullish BOS
      108 -> 105   a deeper pullback              -> a swing low
      105 -> 107   a LOWER high                   -> a swing high
      107 -> 102   closes below 105               -> bearish CHoCH
      102 -> 103.5 a bounce                       -> a swing high
      103.5 -> 100.8 a wick to 100.1 that closes back -> a SWEEP, not a break
      then twice up to exactly 102.00 and back  -> two equal highs, a pool
    """
    turns = [100.0, 103.0, 101.5, 106.0, 104.0, 108.0, 105.0, 107.0,
             102.0, 103.5, 100.8, 102.0, 100.8, 102.0, 101.3]
    steps = [3, 2, 4, 2, 4, 3, 2, 5, 2, 3, 3, 3, 3, 2]
    bars = []
    price = turns[0]
    for leg, (to, n) in enumerate(zip(turns[1:], steps)):
        frm = price
        for k in range(1, n + 1):
            o = price
            c = frm + (to - frm) * k / n
            hi = max(o, c) + 0.15
            lo = min(o, c) - 0.15
            # The turning bar overshoots, so the extreme is unambiguously
            # there rather than tied with its neighbour.
            if k == n:
                if to > frm:
                    hi = c + 0.35
                else:
                    lo = c - 0.35
            bars.append([o, hi, lo, c])
            price = c
    # One deliberate sweep: a wick under the swing low at 102.00 that closes
    # back above it. A wick through a level is not a break of it.
    for i, b in enumerate(bars):
        if abs(b[3] - 100.8) < 1e-9:
            bars[i][2] = 100.1
            break
    out = []
    for i, (o, hi, lo, c) in enumerate(bars):
        out.append({"open": o, "high": hi, "low": lo, "close": c,
                    "time": 1767600000 + i * 3600})
    return out


# ---------------------------------------------------------------------------
# Swings
# ---------------------------------------------------------------------------

def swings(bars, reach=2):
    """An n-bar fractal.

    A bar is a swing high when its high is strictly above the `reach` highs
    before it and at least as high as the `reach` highs after it. Strict on
    one side and not the other is what makes a tie resolve to the first bar
    rather than to both, which matters because two swing highs at the same
    price are a liquidity pool, not two separate structures.

    `confirmed` is the index at which Atlas could first have known: `reach`
    bars later. Nothing may claim a swing before that, or every backtest
    quietly reads the future.
    """
    out = []
    n = len(bars)
    for i in range(reach, n - reach):
        hi = bars[i]["high"]
        lo = bars[i]["low"]
        before_h = [bars[j]["high"] for j in range(i - reach, i)]
        after_h = [bars[j]["high"] for j in range(i + 1, i + reach + 1)]
        before_l = [bars[j]["low"] for j in range(i - reach, i)]
        after_l = [bars[j]["low"] for j in range(i + 1, i + reach + 1)]
        if hi > max(before_h) and hi >= max(after_h):
            out.append({"at": i, "confirmed": i + reach, "high": True, "price": hi})
        if lo < min(before_l) and lo <= min(after_l):
            out.append({"at": i, "confirmed": i + reach, "high": False, "price": lo})
    out.sort(key=lambda s: (s["confirmed"], s["at"]))
    return out


# ---------------------------------------------------------------------------
# Bias, breaks and sweeps
# ---------------------------------------------------------------------------

def shifts(bars, sw):
    """Walk the bars forward, carrying a bias, and record what happens.

    A break is judged on the CLOSE. A wick through a level that closes back
    is a sweep: the stops beyond that level were taken and price refused to
    stay there, which is a different event with a different meaning, and
    collapsing the two is how a system ends up long at the exact top.

    A swing only becomes usable on its `confirmed` bar, never on the bar it
    actually formed.
    """
    bias = "unknown"
    ref_high = None   # (price, index) the last usable swing high
    ref_low = None
    events = []
    for i, b in enumerate(bars):
        usable = [s for s in sw if s["confirmed"] <= i]
        highs = [s for s in usable if s["high"]]
        lows = [s for s in usable if not s["high"]]
        ref_high = highs[-1] if highs else None
        ref_low = lows[-1] if lows else None

        if ref_high is not None:
            if b["close"] > ref_high["price"]:
                kind = "bos" if bias == "up" else "choch"
                events.append({"at": i, "kind": kind, "up": True,
                               "level": ref_high["price"]})
                bias = "up"
                # That swing has been used. Drop it so the same level cannot
                # be broken twice and report two breaks.
                sw = [s for s in sw if s is not ref_high]
                continue
            if b["high"] > ref_high["price"]:
                events.append({"at": i, "kind": "sweep", "up": True,
                               "level": ref_high["price"]})
        if ref_low is not None:
            if b["close"] < ref_low["price"]:
                kind = "bos" if bias == "down" else "choch"
                events.append({"at": i, "kind": kind, "up": False,
                               "level": ref_low["price"]})
                bias = "down"
                sw = [s for s in sw if s is not ref_low]
                continue
            if b["low"] < ref_low["price"]:
                events.append({"at": i, "kind": "sweep", "up": False,
                               "level": ref_low["price"]})
    return events, bias


# ---------------------------------------------------------------------------
# Plain measurements
# ---------------------------------------------------------------------------

def true_range(bars, i):
    if i == 0:
        return bars[0]["high"] - bars[0]["low"]
    pc = bars[i - 1]["close"]
    return max(bars[i]["high"] - bars[i]["low"],
               abs(bars[i]["high"] - pc),
               abs(bars[i]["low"] - pc))


def atr(bars, period=14):
    """Wilder's. The first value is a plain mean of the first `period` ranges,
    then smoothed. Returns None until there are enough bars — an ATR computed
    from four bars is a number, not a measurement."""
    if len(bars) < period + 1:
        return None
    trs = [true_range(bars, i) for i in range(1, len(bars))]
    a = sum(trs[:period]) / period
    for tr in trs[period:]:
        a = (a * (period - 1) + tr) / period
    return a


def realised_vol(bars, window=14):
    """Standard deviation of log returns over the window. Sample (n-1)."""
    if len(bars) < window + 1:
        return None
    rets = [math.log(bars[i]["close"] / bars[i - 1]["close"])
            for i in range(len(bars) - window, len(bars))]
    m = sum(rets) / len(rets)
    var = sum((r - m) ** 2 for r in rets) / (len(rets) - 1)
    return math.sqrt(var)


def efficiency(bars, window=14):
    """Kaufman's ratio: how much of the travelling actually went somewhere.

    1.0 is a straight line, 0.0 is a lot of movement that ended where it
    started. This is the single most useful number for telling a trend from
    a chop, and it is three lines of arithmetic."""
    if len(bars) < window + 1:
        return None
    seg = bars[-(window + 1):]
    net = abs(seg[-1]["close"] - seg[0]["close"])
    path = sum(abs(seg[i]["close"] - seg[i - 1]["close"]) for i in range(1, len(seg)))
    return 0.0 if path == 0 else net / path


def voids(bars):
    """Fair value gaps: a three-bar window where the middle bar moved so fast
    that the first and third do not overlap."""
    out = []
    for i in range(2, len(bars)):
        if bars[i]["low"] > bars[i - 2]["high"]:
            out.append({"at": i, "up": True,
                        "low": bars[i - 2]["high"], "high": bars[i]["low"]})
        if bars[i]["high"] < bars[i - 2]["low"]:
            out.append({"at": i, "up": False,
                        "low": bars[i]["high"], "high": bars[i - 2]["low"]})
    return out


def pools(sw, tolerance):
    """Equal highs or lows: two or more swings at the same price, within a
    tolerance. Stops sit above equal highs, which is what makes them worth
    naming separately from any other level."""
    out = []
    for high in (True, False):
        side = [s for s in sw if s["high"] == high]
        used = set()
        for i, a in enumerate(side):
            if i in used:
                continue
            group = [a]
            for j in range(i + 1, len(side)):
                if j in used:
                    continue
                if abs(side[j]["price"] - a["price"]) <= tolerance:
                    group.append(side[j])
                    used.add(j)
            if len(group) > 1:
                out.append({"high": high,
                            "price": sum(g["price"] for g in group) / len(group),
                            "count": len(group)})
    return out


def cost_share(stop_distance, spread, target_r):
    """What the spread costs as a share of the R being aimed at.

    Charged on both legs. This is the number that makes a +0.10R target at
    M5 arithmetically impossible, which is a thing worth refusing rather
    than trading."""
    if stop_distance <= 0 or target_r <= 0:
        return None
    return (spread * 2.0) / (stop_distance * target_r)


# ---------------------------------------------------------------------------
# Print the golden values
# ---------------------------------------------------------------------------

if __name__ == "__main__":
    bars = series()
    sw = swings(bars)
    ev, bias = shifts(bars, list(sw))

    print(f"bars: {len(bars)}")
    print(f"swings: {len(sw)}")
    for s in sw:
        print(f"  at {s['at']:2d} confirmed {s['confirmed']:2d} "
              f"{'high' if s['high'] else 'low '} {s['price']:.4f}")
    print(f"events: {len(ev)}")
    for e in ev:
        print(f"  at {e['at']:2d} {e['kind']:5s} {'up' if e['up'] else 'down'} "
              f"level {e['level']:.4f}")
    print(f"final bias: {bias}")

    a = atr(bars)
    print(f"atr(14) over all bars: {a:.6f}")
    print(f"atr(14) over first 20:  {atr(bars[:20]):.6f}")
    print(f"realised vol(14):      {realised_vol(bars):.8f}")
    print(f"efficiency(14):        {efficiency(bars):.6f}")
    print(f"efficiency first 15:   {efficiency(bars[:15]):.6f}")
    print(f"true_range(bar 21):    {true_range(bars, 21):.6f}")

    v = voids(bars)
    print(f"voids: {len(v)}")
    for x in v[:6]:
        print(f"  at {x['at']:2d} {'up' if x['up'] else 'down'} "
              f"{x['low']:.4f}-{x['high']:.4f}")

    p = pools(sw, 0.01)
    print(f"pools: {len(p)}")
    for x in p:
        print(f"  {'high' if x['high'] else 'low '} {x['price']:.4f} x{x['count']}")

    print(f"cost share, 30 pip stop, 1.5 pip spread, 0.25R: "
          f"{cost_share(0.0030, 0.00015, 0.25):.4f}")
    print(f"cost share, 30 pip stop, 1.5 pip spread, 2.0R:  "
          f"{cost_share(0.0030, 0.00015, 2.0):.4f}")
