#!/usr/bin/env python3
"""Build a small multi-tile HEIC the way an iPhone lays one out.

An iPhone photo is not one HEVC picture: it is a `grid` item made of 512x512
HEVC tiles, and a reader has to assemble them. ffmpeg before 8.1 hands back
the first tile alone. This writes a 2x2 grid (1000x900 shown, 1024x1024
coded) with each quadrant a different colour, so a reader that returns one
tile, or assembles them in the wrong order, is caught. `--irot N` adds an
`irot` property (N x 90 degrees anticlockwise), as a phone held upright does.

Made for Atlas's tests, 29 Sep 2026: no freely licensed iPhone HEIC was
found. Needs an ffmpeg with libx265 on PATH or in $FFMPEG.

    python3 make_grid_heic.py out.heic [--irot 1]
"""
import os, struct, subprocess, sys, tempfile

TILE = 512
COLS, ROWS = 2, 2
OUT_W, OUT_H = 1000, 900
COLOURS = ["red", "green", "blue", "white"]  # row-major


def box(kind, payload):
    return struct.pack(">I", 8 + len(payload)) + kind + payload


def fullbox(kind, version, flags, payload):
    return box(kind, struct.pack(">I", (version << 24) | flags) + payload)


def children(data):
    i = 0
    while i + 8 <= len(data):
        size, kind = struct.unpack(">I4s", data[i:i + 8])
        yield kind, data[i + 8:i + size], i
        i += size


def find(data, path):
    for kind, body, _ in children(data):
        if kind == path[0]:
            if len(path) == 1:
                return body
            skip = {b"meta": 4, b"stsd": 8, b"hvc1": 78}.get(kind, 0)
            return find(body[skip:], path[1:])
    raise KeyError(path)


def tile(colour, ffmpeg):
    with tempfile.TemporaryDirectory() as d:
        mp4 = os.path.join(d, "t.mp4")
        subprocess.run([ffmpeg, "-v", "error", "-y", "-f", "lavfi", "-i",
                        f"color={colour}:s={TILE}x{TILE}", "-frames:v", "1",
                        "-pix_fmt", "yuv420p", "-c:v", "libx265", "-x265-params", "log-level=error",
                        "-tag:v", "hvc1", mp4], check=True)
        data = open(mp4, "rb").read()
    hvcc = find(data, [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd", b"hvc1", b"hvcC"])
    stsz = find(data, [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsz"])
    stco = find(data, [b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stco"])
    size = struct.unpack(">I", stsz[4:8])[0] or struct.unpack(">I", stsz[12:16])[0]
    off = struct.unpack(">I", stco[8:12])[0]
    return hvcc, data[off:off + size]


def build(irot, ffmpeg):
    tiles = [tile(c, ffmpeg) for c in COLOURS]
    grid_id = len(tiles) + 1
    grid = struct.pack(">BBBBHH", 0, 0, ROWS - 1, COLS - 1, OUT_W, OUT_H)

    # Properties: one hvcC per tile, then the tile size, the shown size, irot.
    props = [box(b"hvcC", h) for h, _ in tiles]
    ispe_tile = len(props) + 1
    props.append(fullbox(b"ispe", 0, 0, struct.pack(">II", TILE, TILE)))
    ispe_out = len(props) + 1
    props.append(fullbox(b"ispe", 0, 0, struct.pack(">II", OUT_W, OUT_H)))
    rot = None
    if irot:
        rot = len(props) + 1
        props.append(box(b"irot", struct.pack(">B", irot & 3)))
    ipco = box(b"ipco", b"".join(props))
    entries = b""
    for i in range(len(tiles)):
        entries += struct.pack(">HB", i + 1, 2) + bytes([0x80 | (i + 1), ispe_tile])
    extra = [ispe_out] + ([0x80 | rot] if rot else [])
    entries += struct.pack(">HB", grid_id, len(extra)) + bytes(extra)
    ipma = fullbox(b"ipma", 0, 0, struct.pack(">I", grid_id) + entries)
    iprp = box(b"iprp", ipco + ipma)

    infes = b"".join(fullbox(b"infe", 2, 0, struct.pack(">HH", i + 1, 0) + b"hvc1" + b"\0")
                     for i in range(len(tiles)))
    infes += fullbox(b"infe", 2, 0, struct.pack(">HH", grid_id, 0) + b"grid" + b"\0")
    iinf = fullbox(b"iinf", 0, 0, struct.pack(">H", grid_id) + infes)
    dimg = box(b"dimg", struct.pack(">HH", grid_id, len(tiles)) + b"".join(struct.pack(">H", i + 1) for i in range(len(tiles))))
    iref = fullbox(b"iref", 0, 0, dimg)
    hdlr = fullbox(b"hdlr", 0, 0, b"\0" * 4 + b"pict" + b"\0" * 12 + b"\0")
    pitm = fullbox(b"pitm", 0, 0, struct.pack(">H", grid_id))
    ftyp = box(b"ftyp", b"heic" + struct.pack(">I", 0) + b"mif1heic")

    payloads = [t for _, t in tiles] + [grid]

    def iloc(base):
        body = struct.pack(">BBH", 0x44, 0x00, len(payloads))
        at = base
        for i, p in enumerate(payloads):
            body += struct.pack(">HHHII", i + 1, 0, 1, at, len(p))
            at += len(p)
        return fullbox(b"iloc", 0, 0, body)

    def meta(base):
        return fullbox(b"meta", 0, 0, hdlr + pitm + iloc(base) + iinf + iref + iprp)

    head = len(ftyp) + len(meta(0)) + 8  # + mdat header
    mdat = box(b"mdat", b"".join(payloads))
    return ftyp + meta(head) + mdat


if __name__ == "__main__":
    out = sys.argv[1]
    irot = int(sys.argv[sys.argv.index("--irot") + 1]) if "--irot" in sys.argv else 0
    data = build(irot, os.environ.get("FFMPEG", "ffmpeg"))
    open(out, "wb").write(data)
    print(out, len(data), "bytes")
