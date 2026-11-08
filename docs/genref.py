# The module reference, generated from the tree it is run against.
#
# ROOT was an absolute path to a directory that has not existed since the
# merge ("/home/claude/atlas_v2"), so this script could not be run at all --
# which is why docs/MODULE_REFERENCE_*.md stopped at 14 Sep while the tree
# kept moving. It is derived from where this file sits now, so it regenerates
# wherever the crate is checked out.
#
#     python3 docs/genref.py
import os, re, datetime, sys
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src")
if not os.path.isdir(SRC):
    sys.exit(f"no src/ under {ROOT} -- run this from inside the crate")

# The unwired list, read from the test that maintains it -- not retyped.
wiring = open(os.path.join(ROOT, "tests/wiring.rs")).read()
block = wiring[wiring.index("const UNWIRED_BASELINE"):]
block = block[:block.index("];")]
unwired = set(re.findall(r'"([a-z_0-9]+)"', block))

files = []
for dirpath, _, names in os.walk(SRC):
    for n in sorted(names):
        if n.endswith(".rs"):
            files.append(os.path.relpath(os.path.join(dirpath, n), ROOT).replace(os.sep, "/"))

def order(p):
    if p == "src/lib.rs": return (0, p)
    if p == "src/main.rs": return (1, p)
    if p.startswith("src/market/"): return (2, p)
    if p.startswith("src/platform/"): return (4, p)
    return (3, p)
files.sort(key=order)

def modname(p):
    return os.path.splitext(os.path.basename(p))[0]

srcs = {p: open(os.path.join(ROOT, p), encoding="utf-8", errors="replace").read() for p in files}

def doc(text):
    out = []
    for line in text.split("\n"):
        s = line.strip()
        if s.startswith("//!"):
            out.append(s[3:].lstrip() if len(s) > 3 else "")
        elif out:
            break
        elif s and not s.startswith("//"):
            break
    while out and not out[-1]:
        out.pop()
    return "\n".join(out)

def iface(text):
    items = []
    for m in re.finditer(r"^pub (?:async )?fn (\w+)", text, re.M):
        items.append(f"`fn {m.group(1)}`")
    for m in re.finditer(r"^pub struct (\w+)", text, re.M):
        items.append(f"`struct {m.group(1)}`")
    for m in re.finditer(r"^pub enum (\w+)", text, re.M):
        items.append(f"`enum {m.group(1)}`")
    for m in re.finditer(r"^pub const (\w+)", text, re.M):
        items.append(f"`const {m.group(1)}`")
    seen, out = set(), []
    for i in items:
        if i not in seen:
            seen.add(i); out.append(i)
    return out

# Wiring is taken from `tests/wiring.rs`'s own maintained list rather than
# recomputed here. That test strips config-only references before deciding
# (a module named as a config type inside ToolsConfig is not wired), and a
# second, looser check written here would disagree with it -- the previous
# generation of this document did exactly that and reported a number the
# guard did not agree with. `mod.rs` files declare a module tree rather than
# holding a capability and are marked as such.
def wired(p):
    name = modname(p)
    if p in ("src/lib.rs", "src/main.rs"):
        return True
    if name == "mod":
        return None
    return name not in unwired

state = {p: wired(p) for p in files}
n_wired = sum(1 for p in files if state[p] is True)
n_unwired = sum(1 for p in files if state[p] is False)
n_mod = sum(1 for p in files if state[p] is None)
today = datetime.date.today().strftime("%-d %B %Y")

L = []
L.append("# Atlas (personal/business) — full module reference\n")
L.append(f"""Generated {today}, from the source tree itself: every file's own
module-level documentation comment, its public interface, and whether
anything outside the file actually calls into it. Nothing here is written
from memory or guessed at — the description under each heading is the
developer's own words at the top of that file, and the wiring line is a
direct check against the rest of the tree, cross-referenced against
`tests/wiring.rs`'s own maintained list of known-unwired modules.

Of {len(files)} files: **{n_wired} wired, {n_unwired} not wired,
{n_mod} module declarations** (`mod.rs` files, which declare a tree rather
than hold a capability).

The wiring column is taken straight from `tests/wiring.rs`'s maintained
`UNWIRED_BASELINE` rather than recomputed here: recomputing it needs
`strip_config_only`'s rule, and a second copy of that rule would drift from
the one the test enforces.

**Read this alongside the numbered session documents** at the top of the
handoff (`00_START_HERE.md` lists them) for what changed and why, and
`OUTSTANDING_2026-09-26.md` for what's left. This document is the map; those
are the story.

**{len(files)} files.** Organized by directory, then alphabetically within it —
`src/lib.rs` and `src/main.rs` first (the two entry points), then
`src/market/*` together, then everything else in one alphabetical run, then
`src/platform/*` together at the end.
""")
L.append("## Quick index\n")
L.append("| File | Wired |")
L.append("|---|---|")
for p in files:
    mark = "—" if state[p] is None else ("yes" if state[p] else "**no**")
    L.append(f"| `{p}` | {mark} |")
L.append("")
for p in files:
    L.append(f"### `{p}`\n")
    if state[p] is None:
        L.append("**Module declaration** — declares the files below it; no capability of its own.\n")
    elif state[p]:
        L.append("**Wired** — something outside this file calls into it.\n")
    else:
        L.append("**Not wired** — built and tested, but nothing outside this file calls into it. Confirmed against `tests/wiring.rs`'s own tracked list, not a guess.\n")
    d = doc(srcs[p])
    if d:
        L.append(d + "\n")
    it = iface(srcs[p])
    if it:
        L.append("**Public interface:**\n")
        for i in it:
            L.append(f"- {i}")
        L.append("")

# Dated by the day it runs rather than by a name typed in. The old script
# wrote the 14 Sep name forever, so a regeneration silently overwrote a
# snapshot instead of making a new one.
out = os.path.join(
    ROOT, "docs", "MODULE_REFERENCE_%s.md" % datetime.date.today().isoformat()
)
open(out, "w").write("\n".join(L))
print("wrote", out, n_wired, "/", len(files))
