"""The arithmetic in src/words.rs, written again in Python and actually run.

Same purpose as docs/market_reference.py and docs/marks_reference.py: this
machine cannot compile Rust, so every number in tests/words.rs comes from here
rather than from me working it out in my head and writing down what I expect.

Run it with:  python3 docs/words_reference.py

It prints the fixtures and the expected answers. Those values are pasted into
tests/words.rs verbatim. If the Rust ever disagrees with them, one of the two
is wrong and the test says which value moved.
"""

import math

ALPHABET = "0123456789abcdefghijklmnopqrstuvwxyz"
CLASSES = len(ALPHABET) + 1  # blank sits at index 0


# ---------------------------------------------------------------------------
# Finding the lumps of ink
# ---------------------------------------------------------------------------

def blobs(prob, w, h, ink):
    """Eight-connected components over the map, thresholded at `ink`.

    Scanned row-major so the order of the boxes is fixed, which is what makes
    a test of this possible at all.
    """
    seen = [False] * (w * h)
    out = []
    for start in range(w * h):
        if seen[start] or prob[start] < ink:
            continue
        stack = [start]
        seen[start] = True
        x0 = x1 = start % w
        y0 = y1 = start // w
        while stack:
            at = stack.pop()
            ax, ay = at % w, at // w
            x0, x1 = min(x0, ax), max(x1, ax)
            y0, y1 = min(y0, ay), max(y1, ay)
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    if dx == 0 and dy == 0:
                        continue
                    nx, ny = ax + dx, ay + dy
                    if 0 <= nx < w and 0 <= ny < h:
                        n = ny * w + nx
                        if not seen[n] and prob[n] >= ink:
                            seen[n] = True
                            stack.append(n)
        out.append((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    return out


def grow(box, ratio, w, h):
    """The DB unclip, on a rectangle.

    The paper offsets the polygon outward by  area * ratio / perimeter.  For a
    rectangle that is exact rather than approximate, which is the one good
    reason to restrict this to rectangles.

    Text regions are deliberately shrunk during DB's training, so a box that is
    not grown back is a box that clips the tops and tails off letters. Skipping
    this step does not fail — it quietly returns 'hp' for 'hp' and 'n' for 'h'.
    """
    x, y, bw, bh = box
    area = bw * bh
    perimeter = 2 * (bw + bh)
    if perimeter == 0:
        return box
    d = int(round(area * ratio / perimeter))
    nx = max(0, x - d)
    ny = max(0, y - d)
    nr = min(w, x + bw + d)
    nb = min(h, y + bh + d)
    return (nx, ny, nr - nx, nb - ny)


def strength(prob, w, box):
    """Mean probability inside the box.

    The threshold that found the box is a floor on single pixels; this is the
    box's own case for existing. A box made of exactly-at-threshold noise and a
    box made of solid ink both survive `blobs` and are told apart here.
    """
    x, y, bw, bh = box
    if bw == 0 or bh == 0:
        return 0.0
    total = 0.0
    for yy in range(y, y + bh):
        for xx in range(x, x + bw):
            total += prob[yy * w + xx]
    return total / (bw * bh)


def find(prob, w, h, ink=0.3, keep_above=0.5, ratio=2.0, smallest=2, most=200):
    kept = []
    for b in blobs(prob, w, h, ink):
        if b[2] < smallest or b[3] < smallest:
            continue
        # Scored on the ORIGINAL box, then grown. Scoring the grown box
        # averages in the background it just swallowed, which drags every
        # score toward zero and drags it furthest for the smallest text.
        s = strength(prob, w, b)
        if s < keep_above:
            continue
        kept.append((grow(b, ratio, w, h), s))
    kept.sort(key=lambda t: -t[1])
    return reading_order(kept[:most])


def reading_order(kept):
    """Rows top to bottom, and left to right inside a row.

    Sorting by y alone puts the second word of a line before the first whenever
    it sits one pixel higher, which is most of the time.
    """
    items = sorted(kept, key=lambda t: (t[0][1], t[0][0]))
    rows = []
    for box, s in items:
        placed = False
        for row in rows:
            top = min(b[1] for b, _ in row)
            bottom = max(b[1] + b[3] for b, _ in row)
            over = min(bottom, box[1] + box[3]) - max(top, box[1])
            if over > 0 and over * 2 > min(bottom - top, box[3]):
                row.append((box, s))
                placed = True
                break
        if not placed:
            rows.append([(box, s)])
    out = []
    for row in rows:
        row.sort(key=lambda t: t[0][0])
        out.extend(row)
    return out


# ---------------------------------------------------------------------------
# Turning a strip of picture into letters
# ---------------------------------------------------------------------------

def looks_like_odds(rows):
    """Is this already a set of probabilities, or are they raw scores?

    Asked rather than assumed. Softmaxing something that is already softmaxed
    does not fail — it flattens every number toward 1/37 and hands back a
    confidence of four per cent for a word that was read perfectly. The text
    would be right and the number beside it would be wrong, which is worse than
    either being wrong alone.
    """
    for r in rows:
        if any(v < -0.001 or v > 1.001 for v in r):
            return False
        if abs(sum(r) - 1.0) > 0.01:
            return False
    return True


def ctc(values, steps, classes=CLASSES, alphabet=ALPHABET):
    """Greedy CTC: argmax per step, drop blanks, collapse repeats.

    Blank is index 0 and the alphabet starts at 1, which is how the model was
    exported. Collapsing is reset by a blank, so 'cool' survives: the two o's
    are separated by a blank step and stay two.
    """
    rows = [list(values[t * classes:(t + 1) * classes]) for t in range(steps)]
    if looks_like_odds(rows):
        odds = rows
    else:
        odds = []
        for r in rows:
            m = max(r)
            e = [math.exp(v - m) for v in r]
            total = sum(e)
            odds.append([v / total for v in e])

    text = ""
    picked = []
    last = -1
    for row in odds:
        best = max(range(classes), key=lambda i: row[i])
        if best != 0:
            ch = alphabet[best - 1]
            if best != last:
                text += ch
                picked.append(row[best])
        last = best
    sure = sum(picked) / len(picked) if picked else 0.0
    return text, sure


# ---------------------------------------------------------------------------
# Preparing a picture the way each model wants it
# ---------------------------------------------------------------------------

def grey(r, g, b):
    """The same weights OpenCV's BGR2GRAY uses."""
    return 0.299 * r + 0.587 * g + 0.114 * b


def finder_recipe():
    """PP-OCRv3 detection, as the OpenCV zoo configures it.

    OpenCV computes  scale * (pixel - mean)  with scale = 1/255/std, and does
    NOT swap red and blue, so channel nought is blue. Atlas's `arrange`
    computes  (pixel*top - mean) / deviation, so deviation is 255*std and
    blue_first is set.

    Worth stating because it looks wrong: the means are the ImageNet RGB ones
    but they land on a blue-first picture. That is not a slip — PaddleOCR
    trained it that way, so the model has only ever seen them in this order.
    Fixing it to look right would break it.
    """
    std = [0.229, 0.224, 0.225]
    return dict(
        width=736, height=736, planar=True, top=255.0,
        mean=[123.675, 116.28, 103.53],
        deviation=[255.0 * s for s in std],
        blue_first=True,
    )


def reader_recipe():
    """CRNN: one grey channel, 32 high by 100 wide, centred on 127.5."""
    return dict(width=100, height=32, grey=True, top=255.0,
                mean=[127.5] * 3, deviation=[127.5] * 3, blue_first=False)


def arrange_one(pixel, recipe, channel):
    c = 2 - channel if recipe["blue_first"] else channel
    return (pixel[c] * recipe["top"] / 255.0 - recipe["mean"][channel]) / recipe["deviation"][channel]


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

def fixture_map():
    """A 12x9 map with two lumps on one line and one lump below.

    Hand-built rather than random: every expected box below can be read off
    the picture by eye, so a wrong answer is obvious rather than plausible.
    """
    w, h = 12, 9
    m = [0.02] * (w * h)

    def fill(x0, y0, x1, y1, v):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                m[y * w + x] = v

    fill(1, 1, 3, 2, 0.90)   # left lump on the top line
    fill(6, 1, 8, 2, 0.80)   # right lump on the top line, same rows
    fill(2, 6, 5, 7, 0.70)   # a lump on the second line
    fill(10, 5, 11, 5, 0.31) # barely-ink noise: passes the threshold, fails the score
    return m, w, h


def main():
    m, w, h = fixture_map()

    print("=" * 68)
    print("blobs at ink=0.3, before any filtering")
    print("=" * 68)
    for b in blobs(m, w, h, 0.3):
        print(f"  x={b[0]} y={b[1]} w={b[2]} h={b[3]}   strength={strength(m, w, b):.4f}")

    print()
    print("=" * 68)
    print("grow(), the DB unclip, on each raw box  (ratio 2.0)")
    print("=" * 68)
    for b in blobs(m, w, h, 0.3):
        g = grow(b, 2.0, w, h)
        area, per = b[2] * b[3], 2 * (b[2] + b[3])
        print(f"  {b} -> {g}     d = round({area}*2/{per}) = {round(area * 2 / per)}")

    print()
    print("=" * 68)
    print("find(), everything, in reading order")
    print("=" * 68)
    for box, s in find(m, w, h):
        print(f"  x={box[0]} y={box[1]} w={box[2]} h={box[3]}   strength={s:.4f}")
    print("  (the 0.31 lump is gone: it passes ink=0.3 and fails keep_above=0.5)")

    print()
    print("=" * 68)
    print("CTC greedy decode")
    print("=" * 68)

    def steps_for(seq):
        """seq: list of class indices, one per timestep. Returns raw scores."""
        vals = []
        for idx in seq:
            row = [0.0] * CLASSES
            for i in range(CLASSES):
                row[i] = 4.0 if i == idx else 0.0
            vals.extend(row)
        return vals

    def ix(ch):
        return ALPHABET.index(ch) + 1

    cases = [
        ("plain 'at'", [ix("a"), ix("t")]),
        ("repeat with no blank collapses", [ix("a"), ix("a"), ix("t")]),
        ("repeat split by a blank survives", [ix("o"), 0, ix("o")]),
        ("leading and trailing blanks", [0, 0, ix("h"), ix("i"), 0]),
        ("all blank", [0, 0, 0]),
        ("'cool'", [ix("c"), ix("o"), 0, ix("o"), ix("l"), ix("l")]),
    ]
    for name, seq in cases:
        text, sure = ctc(steps_for(seq), len(seq))
        print(f"  {name:38}  -> {text!r:8}  sure={sure:.6f}")

    print()
    print("  the same, handed values that are ALREADY probabilities:")
    probs = []
    seq = [ix("a"), 0, ix("t")]
    for idx in seq:
        row = [0.01 / (CLASSES - 1)] * CLASSES
        row[idx] = 0.99
        probs.extend(row)
    text, sure = ctc(probs, len(seq))
    print(f"    -> {text!r}  sure={sure:.6f}")
    # what a wrongly-applied second softmax would have said, for the test
    rows = [probs[t * CLASSES:(t + 1) * CLASSES] for t in range(len(seq))]
    wrong = []
    for r in rows:
        mx = max(r)
        e = [math.exp(v - mx) for v in r]
        wrong.append(max(e) / sum(e))
    print(f"    a second softmax would have said {sum(wrong) / len(wrong):.6f} instead — "
          f"the text identical, the number beside it wrong")

    print()
    print("=" * 68)
    print("recipes")
    print("=" * 68)
    f = finder_recipe()
    print(f"  finder deviation = {[round(v, 6) for v in f['deviation']]}")
    print(f"  finder mean      = {f['mean']}")
    # one pixel, worked through
    px = (200, 100, 50)  # r, g, b
    vals = [round(arrange_one(px, f, c), 6) for c in range(3)]
    print(f"  rgb{px} through the finder recipe -> {vals}")
    r = reader_recipe()
    g = grey(*px)
    print(f"  rgb{px} as grey -> {g:.4f}, through the reader recipe -> "
          f"{(g - 127.5) / 127.5:.6f}")

    print()
    print("=" * 68)
    print("what scoring AFTER growing would have done instead")
    print("=" * 68)
    for b in blobs(m, w, h, 0.3):
        g = grow(b, 2.0, w, h)
        before, after = strength(m, w, b), strength(m, w, g)
        verdict = "kept" if after >= 0.5 else "THROWN AWAY"
        print(f"  {b}: {before:.4f} before growing, {after:.4f} after -> {verdict}")
    print("  every real box would be lost, and the smallest text first")

    print()
    print("=" * 68)
    print("the limit, at most=2")
    print("=" * 68)
    passed = []
    for b in blobs(m, w, h, 0.3):
        if b[2] < 2 or b[3] < 2:
            continue
        s = strength(m, w, b)
        if s < 0.5:
            continue
        passed.append((grow(b, 2.0, w, h), s))
    passed.sort(key=lambda t: -t[1])
    print(f"  {len(passed)} passed every test, over_the_limit = {len(passed) - 2}")
    print(f"  kept: {[b for b, _ in reading_order(passed[:2])]}")

    print()
    print("=" * 68)
    print("confidence with twenty certain blanks appended")
    print("=" * 68)
    seq = [ix("h"), ix("i")]
    vals = steps_for(seq)
    for _ in range(20):
        row = [0.0] * CLASSES
        row[0] = 20.0
        vals.extend(row)
    text, sure = ctc(vals, 22)
    print(f"  -> {text!r} sure={sure:.6f}  (unchanged: blanks never counted)")

    print()
    print("=" * 68)
    print("reading order, on boxes deliberately out of order")
    print("=" * 68)
    made_up = [
        ((50, 10, 20, 8), 0.9),   # top line, right
        ((5, 11, 20, 8), 0.9),    # top line, left, one pixel lower
        ((5, 40, 20, 8), 0.9),    # second line
    ]
    for box, _ in reading_order(made_up):
        print(f"  x={box[0]} y={box[1]}")
    print("  (the left box wins its row despite being lower)")


if __name__ == "__main__":
    main()
