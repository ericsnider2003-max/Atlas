"""Where the FX day ends, worked out in Python and actually run.

Eric's ruling: **17:00 New York.** That is a local wall-clock time, not a fixed
UTC hour, so the boundary moves twice a year with US daylight saving:

    winter (EST, UTC-5)   17:00 NY = 22:00 UTC
    summer (EDT, UTC-4)   17:00 NY = 21:00 UTC

Getting that wrong is not a small error. It puts every prior-day high and low an
hour out for roughly half the year, and it does it silently — the levels are
still levels, still plausible, still near price. Nothing looks broken.

## Why this convention and not midnight UTC

Because it produces **five 24-hour candles a week** instead of six. The FX week
opens 17:00 Sunday New York and closes 17:00 Friday New York, so the Sunday
evening session belongs to Monday's candle rather than forming a stub of its
own. Midnight UTC gives a tiny Sunday bar that every broker chart folds away —
so a reader using it would disagree with what Eric is looking at, on every
single day, while looking perfectly reasonable.

This is also why brokers run servers on GMT+2 in winter and GMT+3 in summer:
both put 17:00 New York at 00:00 server time, so their daily candle closes on
their own midnight all year.

Run:  python3 docs/fxday_reference.py
"""

MS_PER_HOUR = 3_600_000
MS_PER_DAY = 86_400_000


# ---------------------------------------------------------------------------
# Civil dates, the same arithmetic market::time uses
# ---------------------------------------------------------------------------

def days_from_civil(y, m, d):
    y -= m <= 2
    era = (y if y >= 0 else y - 399) // 400
    yoe = y - era * 400
    doy = (153 * (m + (-3 if m > 2 else 9)) + 2) // 5 + d - 1
    doe = yoe * 365 + yoe // 4 - yoe // 100 + doy
    return era * 146097 + doe - 719468


def civil_from_days(z):
    z += 719468
    era = (z if z >= 0 else z - 146096) // 146097
    doe = z - era * 146097
    yoe = (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365
    y = yoe + era * 400
    doy = doe - (365 * yoe + yoe // 4 - yoe // 100)
    mp = (5 * doy + 2) // 153
    d = doy - (153 * mp + 2) // 5 + 1
    m = mp + 3 if mp < 10 else mp - 9
    return (y + (1 if m <= 2 else 0), m, d)


def weekday(ms):
    """0 = Sunday."""
    return (ms // MS_PER_DAY + 4) % 7


def nth_weekday(year, month, wd, n):
    """Day of month of the nth `wd` (0 = Sunday)."""
    first = days_from_civil(year, month, 1)
    first_wd = (first + 4) % 7
    return 1 + (wd - first_wd) % 7 + (n - 1) * 7


# ---------------------------------------------------------------------------
# New York's offset, the same rule market::session uses
# ---------------------------------------------------------------------------

def new_york_offset(ms):
    """-5 in winter, -4 in summer.

    Second Sunday of March 02:00 local (07:00 UTC, since the offset BEFORE the
    change is -5) to the first Sunday of November 02:00 local (06:00 UTC, since
    the offset before THAT change is -4).

    The asymmetry is the whole trick and is easy to get wrong: you convert a
    local instant using the offset that was in force a moment earlier, not the
    one that is about to be.
    """
    y = civil_from_days(ms // MS_PER_DAY)[0]
    start = days_from_civil(y, 3, nth_weekday(y, 3, 0, 2)) * MS_PER_DAY + (2 - (-5)) * MS_PER_HOUR
    end = days_from_civil(y, 11, nth_weekday(y, 11, 0, 1)) * MS_PER_DAY + (2 - (-4)) * MS_PER_HOUR
    return -4 if start <= ms < end else -5


# ---------------------------------------------------------------------------
# The boundary
# ---------------------------------------------------------------------------

CLOSE_HOUR = 17  # New York local


def close_on(y, m, d):
    """The UTC instant of 17:00 New York on a given New York calendar date."""
    midnight_utc = days_from_civil(y, m, d) * MS_PER_DAY
    # Which offset applies is decided at roughly the moment in question, and
    # 17:00 local is never near a transition (they happen at 02:00), so one
    # pass is enough — but it is done from a guess at the instant rather than
    # from the date, because the date alone does not know its own offset.
    guess = midnight_utc + CLOSE_HOUR * MS_PER_HOUR
    off = new_york_offset(guess)
    return midnight_utc + (CLOSE_HOUR - off) * MS_PER_HOUR


def day_start(ms):
    """The most recent 17:00 New York at or before `ms`.

    Walks back at most two calendar days, which covers the offset either way.
    """
    for back in range(0, 3):
        y, m, d = civil_from_days((ms - back * MS_PER_DAY) // MS_PER_DAY)
        c = close_on(y, m, d)
        if c <= ms:
            return c
    raise AssertionError("no boundary found, which cannot happen")


def day_of(ms):
    start = day_start(ms)
    # The next boundary is the close of the following New York date.
    y, m, d = civil_from_days(start // MS_PER_DAY)
    nxt = civil_from_days(days_from_civil(y, m, d) + 1)
    return (start, close_on(*nxt))


def week_start(ms):
    """The most recent Sunday 17:00 New York at or before `ms`."""
    s = day_start(ms)
    while weekday(s) != 0:
        s = day_start(s - 1)
    return s


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

def say(ms):
    d = civil_from_days(ms // MS_PER_DAY)
    rest = ms % MS_PER_DAY
    return f"{d[0]:04d}-{d[1]:02d}-{d[2]:02d} {rest // MS_PER_HOUR:02d}:{(rest % MS_PER_HOUR) // 60000:02d}Z"


def main():
    line = "=" * 72
    print(line); print("The boundary in UTC, either side of both transitions"); print(line)
    for (y, m, d, what) in [
        (2026, 1, 15, "deep winter (EST)"),
        (2026, 3, 7, "the Saturday before spring forward"),
        (2026, 3, 9, "the Monday after"),
        (2026, 7, 15, "deep summer (EDT)"),
        (2026, 10, 30, "the Friday before fall back"),
        (2026, 11, 2, "the Monday after"),
    ]:
        c = close_on(y, m, d)
        off = new_york_offset(c)
        print(f"  {y}-{m:02d}-{d:02d}  {what:<34} 17:00 NY = {say(c)}  (UTC{off})")

    print()
    print(line); print("The week: five days, and no Sunday stub"); print(line)
    names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
    mid = days_from_civil(2026, 7, 15) * MS_PER_DAY + 12 * MS_PER_HOUR
    ws = week_start(mid)
    print(f"  week opens {say(ws)}  (a {names[weekday(ws)]} evening in New York)")
    at = ws
    for i in range(5):
        s, e = day_of(at + 1)
        print(f"    day {i + 1}: {say(s)} -> {say(e)}   {(e - s) / MS_PER_HOUR:.0f}h")
        at = e
    print("  five 24-hour days, Sunday evening to Friday evening")

    print()
    print(line); print("Where the clock change actually falls"); print(line)
    print("  I assumed a trading day would be 23 hours twice a year. It never is,")
    print("  and the sweep below is what said so.")
    print()
    for y in (2024, 2025, 2026, 2027):
        for (m, n, hour, off, what) in [
            (3, nth_weekday(y, 3, 0, 2), 2, -5, "spring forward"),
            (11, nth_weekday(y, 11, 0, 1), 2, -4, "fall back"),
        ]:
            t = days_from_civil(y, m, n) * MS_PER_DAY + (hour - off) * MS_PER_HOUR
            s, e = day_of(t)
            print(f"  {y} {what:<15} {say(t)}  is inside {names[weekday(s)]}->"
                  f"{names[weekday(e)]}, {(e - s) / MS_PER_HOUR:.0f}h")
    print()
    print("  The change happens at 02:00 on a Sunday, and the market is shut from")
    print("  17:00 Friday to 17:00 Sunday. So it always lands in the weekend, and")
    print("  every TRADING day is exactly 24 hours. That is a real property of")
    print("  this convention rather than a coincidence, and it is the opposite of")
    print("  what I wrote before running this.")

    print()
    print(line); print("What the weekend gap does instead"); print(line)
    for label, when in [
        ("an ordinary weekend", days_from_civil(2026, 7, 15)),
        ("spring forward", days_from_civil(2026, 3, 11)),
        ("fall back", days_from_civil(2026, 11, 4)),
    ]:
        ws = week_start(when * MS_PER_DAY + 12 * MS_PER_HOUR)
        fri = ws
        while names[weekday(fri)] != "Fri":
            fri = day_start(fri - 1)
        print(f"  {label:<22} shut {say(fri)} -> open {say(ws)}  "
              f"= {(ws - fri) / MS_PER_HOUR:.0f}h")
    print()
    print("  47 or 49 hours twice a year rather than 48. Worth knowing because a")
    print("  feed check that calls a gap a weekend has to accept all three, or it")
    print("  reports a hole in the data twice a year and the data is fine.")

    print()
    print(line); print("What midnight UTC would have given instead"); print(line)
    mid = days_from_civil(2026, 7, 15) * MS_PER_DAY + 12 * MS_PER_HOUR
    ws = week_start(mid)
    print(f"  the FX week opens {say(ws)}")
    naive = (ws // MS_PER_DAY + 1) * MS_PER_DAY
    print(f"  midnight UTC after it is {say(naive)}")
    print(f"  so a midnight-UTC reader splits the opening session into a "
          f"{(naive - ws) / MS_PER_HOUR:.0f}-hour Sunday stub")
    print("  and a short Monday -- six candles a week, and every broker chart shows five")


if __name__ == "__main__":
    main()
