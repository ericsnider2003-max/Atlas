#!/usr/bin/env python3
"""Every icon and mark file, generated from src/mark.rs (the Folded A).

The shapes and colours are read out of src/mark.rs itself, so there is one
source: change a number there, run this, and every platform's icon follows.
tests/the_mark_is_one_mark.rs fails when a generated file no longer matches.

Writes (paths relative to the crate):
  design/mark/folded-a.svg, folded-a-dark.svg      the mark alone
  assets/icon-192.png, icon-512.png                 the web app (maskable-safe)
  assets/apple-touch-icon.png                       iOS home screen for the web app
  assets/mark-256.png                               Atlas's own windows' icon
  assets/atlas.ico                                  Windows: the program's icon
  windows/atlas.rc, atlas.res, atlas-res.o          that icon, compiled for the
                                                    MSVC and GNU linkers (build.rs)
  mobile/ios/Atlas/Assets.xcassets/...              the iPhone and iPad app icon,
                                                    light, dark and tinted
  mobile/android/app/src/main/res/...               Android's adaptive launcher icon
                                                    (with its themed layer) and the
                                                    notification icon

Needs: cairosvg and Pillow (pip install cairosvg pillow), and, for the Windows
resource, x86_64-w64-mingw32-windres. Run from the crate: python3 design/mark/make_marks.py
"""
import io
import json
import os
import re
import shutil
import subprocess
import sys

import cairosvg
from PIL import Image

CRATE = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = open(os.path.join(CRATE, "src", "mark.rs"), encoding="utf-8").read()


def points(name):
    body = re.search(rf"pub const {name}: \[\(f32, f32\); \d\] = \[(.*?)\];", SRC).group(1)
    return [(float(x), float(y)) for x, y in re.findall(r"\(([\d.]+), ([\d.]+)\)", body)]


def colours(name):
    body = re.search(rf"pub const {name}: Colours = Colours \{{(.*?)\}};", SRC).group(1)
    return dict(re.findall(r'(\w+): "(#[0-9A-Fa-f]{6})"', body))


FRONT, BACK, FOLD = points("FRONT"), points("BACK"), points("FOLD")
DOT = tuple(float(v) for v in re.search(r"pub const DOT: \(f32, f32, f32\) = \(([\d.]+), ([\d.]+), ([\d.]+)\)", SRC).groups())
PAPER = re.search(r'pub const PAPER: &str = "(#\w+)"', SRC).group(1)
EMBER = re.search(r'pub const EMBER: &str = "(#\w+)"', SRC).group(1)
LIGHT, DARK = colours("WARM_PAPER"), colours("EMBER_DARK")


def path(pts):
    return "".join(f"{'M' if i == 0 else 'L'}{x:.2f} {y:.2f} " for i, (x, y) in enumerate(pts)) + "Z"


def shapes(c, small=False):
    s = f'<path d="{path(FRONT)}" fill="{c["front"]}"/><path d="{path(BACK)}" fill="{c["back"]}"/>'
    if not small:
        s += f'<path d="{path(FOLD)}" fill="{c["fold"]}"/>'
    s += f'<circle cx="{DOT[0]:g}" cy="{DOT[1]:g}" r="{DOT[2]:g}" fill="{c["dot"]}"/>'
    return s


def mark_svg(c):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">{shapes(c)}</svg>\n'


def icon_svg(size, c, tile, pad, radius=0.0, small=False, transparent=False):
    """The mark on a tile. `pad` is the margin around the mark's 100-unit box,
    as a fraction of the side; `radius` rounds the tile (0 = full bleed, for
    iOS, which rounds it itself)."""
    inner = size * (1 - 2 * pad)
    bg = "" if transparent else f'<rect width="{size}" height="{size}" rx="{size * radius:.2f}" fill="{tile}"/>'
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">{bg}'
        f'<g transform="translate({size * pad:.3f},{size * pad:.3f}) scale({inner / 100:.5f})">{shapes(c, small)}</g></svg>'
    )


def png(svg, size):
    return cairosvg.svg2png(bytestring=svg.encode(), output_width=size, output_height=size)


def write(rel, data, mode="wb"):
    p = os.path.join(CRATE, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, mode) as f:
        f.write(data)
    print("wrote", rel)


def main():
    # The mark alone.
    write("design/mark/folded-a.svg", mark_svg(LIGHT), "w")
    write("design/mark/folded-a-dark.svg", mark_svg(DARK), "w")

    # The web app. Its own icons keep the rounded tile (Eric: "I like the
    # rounded edges"); the maskable one is square, because the system that
    # uses it cuts its own shape, with the mark inside the centre 80% circle.
    for size in (192, 512):
        write(f"assets/icon-{size}.png", png(icon_svg(size, LIGHT, PAPER, 0.16, radius=0.225), size))
    write("assets/icon-maskable-512.png", png(icon_svg(512, LIGHT, PAPER, 0.2), 512))
    write("assets/apple-touch-icon.png", png(icon_svg(180, LIGHT, PAPER, 0.19), 180))

    # Atlas's own windows (egui's window icon): light, and dark for when
    # Atlas's appearance is dark (Eric: the colour follows the person's settings).
    write("assets/mark-256.png", png(icon_svg(256, LIGHT, PAPER, 0.16, radius=0.225), 256))
    write("assets/mark-256-dark.png", png(icon_svg(256, DARK, EMBER, 0.16, radius=0.225), 256))

    # Windows: one .ico with every size Windows asks for; the fold tone drops
    # out at 32 px and below, the dot stays.
    frames = []
    for size in (16, 20, 24, 32, 40, 48, 64, 128, 256):
        frames.append(Image.open(io.BytesIO(png(icon_svg(size, LIGHT, PAPER, 0.12 if size <= 32 else 0.16, radius=0.2, small=size <= 32), size))).convert("RGBA"))
    ico = io.BytesIO()
    frames[-1].save(ico, format="ICO", sizes=[(f.width, f.height) for f in frames], append_images=frames[:-1])
    write("assets/atlas.ico", ico.getvalue())

    # The Windows resource: the icon, compiled once here for both linkers.
    version = re.search(r'^version = "([\d.]+)"', open(os.path.join(CRATE, "Cargo.toml")).read(), re.M).group(1)
    parts = (version.split(".") + ["0", "0", "0"])[:4]
    rc = (
        "// Atlas.exe's icon, and the name Windows shows for it (Task Manager, its\n"
        "// security prompts). design/mark/make_marks.py writes this and the two\n"
        "// compiled forms beside it; build.rs links the right one.\n"
        '1 ICON "../assets/atlas.ico"\n'
        f"1 VERSIONINFO\nFILEVERSION {','.join(parts)}\nPRODUCTVERSION {','.join(parts)}\nBEGIN\n"
        '  BLOCK "StringFileInfo"\n  BEGIN\n    BLOCK "040904b0"\n    BEGIN\n'
        '      VALUE "FileDescription", "Atlas"\n      VALUE "ProductName", "Atlas"\n'
        f'      VALUE "FileVersion", "{version}"\n      VALUE "ProductVersion", "{version}"\n'
        '      VALUE "OriginalFilename", "atlas.exe"\n    END\n  END\n'
        '  BLOCK "VarFileInfo"\n  BEGIN\n    VALUE "Translation", 0x409, 1200\n  END\nEND\n'
    )
    write("windows/atlas.rc", rc, "w")
    windres = shutil.which("x86_64-w64-mingw32-windres") or shutil.which("windres")
    if windres:
        wdir = os.path.join(CRATE, "windows")
        subprocess.run([windres, "atlas.rc", "-O", "res", "-o", "atlas.res"], cwd=wdir, check=True)
        subprocess.run([windres, "atlas.rc", "-O", "coff", "-o", "atlas-res.o"], cwd=wdir, check=True)
        print("wrote windows/atlas.res, windows/atlas-res.o")
    else:
        print("no windres: windows/atlas.res and atlas-res.o NOT rewritten", file=sys.stderr)

    # iPhone and iPad: one 1024 image per appearance, and iOS picks the one
    # that matches the person's own setting (Light, Dark, Tinted). Each is a
    # solid tile, full bleed, which iOS rounds into the same rounded square
    # (Eric: the colour follows the settings, the rounded edges stay).
    icons = "mobile/ios/Atlas/Assets.xcassets/AppIcon.appiconset"
    write(f"{icons}/icon-1024.png", png(icon_svg(1024, LIGHT, PAPER, 0.19), 1024))
    write(f"{icons}/icon-1024-dark.png", png(icon_svg(1024, DARK, EMBER, 0.19), 1024))
    grey = {"front": "#FFFFFF", "back": "#B8B8B8", "fold": "#8A8A8A", "dot": "#B8B8B8"}
    write(f"{icons}/icon-1024-tinted.png", png(icon_svg(1024, grey, "#000000", 0.19), 1024))
    contents = {
        "images": [
            {"filename": "icon-1024.png", "idiom": "universal", "platform": "ios", "size": "1024x1024"},
            {"appearances": [{"appearance": "luminosity", "value": "dark"}], "filename": "icon-1024-dark.png",
             "idiom": "universal", "platform": "ios", "size": "1024x1024"},
            {"appearances": [{"appearance": "luminosity", "value": "tinted"}], "filename": "icon-1024-tinted.png",
             "idiom": "universal", "platform": "ios", "size": "1024x1024"},
        ],
        "info": {"author": "xcode", "version": 1},
    }
    write(f"{icons}/Contents.json", json.dumps(contents, indent=2) + "\n", "w")
    write("mobile/ios/Atlas/Assets.xcassets/Contents.json", json.dumps({"info": {"author": "xcode", "version": 1}}, indent=2) + "\n", "w")

    # Android: an adaptive icon. The foreground is 108 dp with the mark in the
    # 66 dp safe zone; the background is the paper; the monochrome layer is
    # what Android 13+ tints for themed icons.
    res = "mobile/android/app/src/main/res"

    def vector(c, size_dp, box, offset, small=False, tile=None, tile_colour=None):
        k = box / 100.0
        def pd(pts):
            return "".join(f"{'M' if i == 0 else 'L'}{offset + x * k:.2f},{offset + y * k:.2f} " for i, (x, y) in enumerate(pts)) + "Z"
        cx, cy, r = offset + DOT[0] * k, offset + DOT[1] * k, DOT[2] * k
        dot = f"M{cx - r:.2f},{cy:.2f} a{r:.2f},{r:.2f} 0 1,0 {2 * r:.2f},0 a{r:.2f},{r:.2f} 0 1,0 {-2 * r:.2f},0 Z"
        parts = [(pd(FRONT), c["front"]), (pd(BACK), c["back"])]
        if not small:
            parts.append((pd(FOLD), c["fold"]))
        parts.append((dot, c["dot"]))
        if tile:
            # The rounded paper tile, drawn as part of the icon (Eric chose the
            # rounded square), so every launcher shows that shape whatever mask
            # it uses: the launcher's own background layer is left clear.
            x0, side, rad = tile
            x1 = x0 + side
            rr = (f"M{x0 + rad:.2f},{x0:.2f} L{x1 - rad:.2f},{x0:.2f} A{rad:.2f},{rad:.2f} 0 0,1 {x1:.2f},{x0 + rad:.2f} "
                  f"L{x1:.2f},{x1 - rad:.2f} A{rad:.2f},{rad:.2f} 0 0,1 {x1 - rad:.2f},{x1:.2f} L{x0 + rad:.2f},{x1:.2f} "
                  f"A{rad:.2f},{rad:.2f} 0 0,1 {x0:.2f},{x1 - rad:.2f} L{x0:.2f},{x0 + rad:.2f} A{rad:.2f},{rad:.2f} 0 0,1 {x0 + rad:.2f},{x0:.2f} Z")
            parts.insert(0, (rr, (tile_colour or PAPER).upper().replace("#", "#FF")))
        body = "".join(f'\n    <path android:fillColor="{col}" android:pathData="{d}"/>' for d, col in parts)
        return (
            '<?xml version="1.0" encoding="utf-8"?>\n<!-- Generated by design/mark/make_marks.py from src/mark.rs. -->\n'
            f'<vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="{size_dp}dp" android:height="{size_dp}dp"'
            f' android:viewportWidth="{size_dp}" android:viewportHeight="{size_dp}">{body}\n</vector>\n'
        )

    # The tile fills 68 of the 72 dp a launcher shows; the mark sits in it
    # with the same margins as the Windows icon's.
    write(f"{res}/drawable/ic_launcher_foreground.xml", vector(LIGHT, 108, 46.24, 30.88, tile=(20.0, 68.0, 15.3)), "w")
    # In dark mode, the Ember tile: Android picks it by the phone's own setting.
    write(f"{res}/drawable-night/ic_launcher_foreground.xml", vector(DARK, 108, 46.24, 30.88, tile=(20.0, 68.0, 15.3), tile_colour=EMBER), "w")
    mono = {"front": "#FF000000", "back": "#FF000000", "fold": "#FF000000", "dot": "#FF000000"}
    write(f"{res}/drawable/ic_launcher_monochrome.xml", vector(mono, 108, 58, 25, small=True), "w")
    white = {"front": "#FFFFFFFF", "back": "#FFFFFFFF", "fold": "#FFFFFFFF", "dot": "#FFFFFFFF"}
    write(f"{res}/drawable/ic_stat_atlas.xml", vector(white, 24, 22, 1, small=True), "w")
    adaptive = (
        '<?xml version="1.0" encoding="utf-8"?>\n<!-- Atlas\'s launcher icon: the Folded A on paper (src/mark.rs). -->\n'
        '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
        '    <background android:drawable="@android:color/transparent"/>\n'
        '    <foreground android:drawable="@drawable/ic_launcher_foreground"/>\n'
        '    <monochrome android:drawable="@drawable/ic_launcher_monochrome"/>\n'
        '</adaptive-icon>\n'
    )
    write(f"{res}/mipmap-anydpi-v26/ic_launcher.xml", adaptive, "w")
    write(f"{res}/mipmap-anydpi-v26/ic_launcher_round.xml", adaptive, "w")


if __name__ == "__main__":
    main()
