"""Merging two trees that grew from one base, without losing either.

Two development chats have been editing Atlas independently from the same
starting tree. That is a three-way merge, and doing it by eye is how one side's
work quietly disappears — not with an error, but with a file that looks fine
and is missing a module somebody spent a day on.

Because the common ancestor exists, none of this has to be guesswork:

    only theirs changed it   -> take theirs
    only mine changed it     -> keep mine
    both changed it the same -> nothing to do
    both changed it apart    -> CONFLICT, and a person decides
    they deleted, I changed  -> CONFLICT
    new on one side only     -> take it

What this refuses to do is pick a winner on a conflict. Every real collision in
this codebase lives in the same handful of files — `lib.rs`, `main.rs`,
`capability.rs`, `tests/guards.rs`, `config/tools.yaml`, `voice.rs` — and they
are all **lists**: modules declared, commands dispatched, guards named. The
right merge for a list is almost always the union, and the wrong one is
whichever side was written second. A script that guesses would be right most of
the time, which is the worst possible hit rate for something nobody checks.

Usage:
    python3 docs/three_way.py <base> <mine> <theirs> [--apply <out>]

Without --apply it only reports. With it, the unambiguous decisions are written
to a new tree and the conflicts are copied from `mine` and listed, so the tree
is always complete and never half-merged.
"""

import hashlib
import os
import shutil
import sys

SKIP_DIRS = {"target", ".git", "node_modules"}
SKIP_SUFFIX = {".exe", ".zip", ".onnx", ".bin", ".gguf"}

# The files where both sides almost certainly collided, and where the collision
# is nearly always additive rather than contradictory. Flagged so a person
# looks at them first rather than last.
LISTS = {
    "src/lib.rs",
    "src/main.rs",
    "src/capability.rs",
    "src/voice.rs",
    "tests/guards.rs",
    "config/tools.yaml",
    "config/commands.yaml",
    "ATLAS.bat",
    "docs/CAPABILITIES.md",
    "docs/OPEN_ITEMS.md",
}


def digest(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def walk(root):
    out = {}
    if not root or not os.path.isdir(root):
        return out
    for r, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for f in files:
            if os.path.splitext(f)[1] in SKIP_SUFFIX:
                continue
            p = os.path.join(r, f)
            rel = os.path.relpath(p, root).replace(os.sep, "/")
            try:
                out[rel] = digest(p)
            except OSError:
                pass
    return out


def decide(base, mine, theirs):
    """One file, three versions (any may be None). Returns (what, why)."""
    b, m, t = base, mine, theirs

    if m is None and t is None:
        return ("gone", "deleted on both sides")
    if m is None and t is not None:
        if b is None:
            return ("take-theirs", "new in theirs")
        if b == t:
            return ("gone", "I deleted it and they left it alone")
        return ("CONFLICT", "I deleted it and they changed it")
    if t is None and m is not None:
        if b is None:
            return ("keep-mine", "new in mine")
        if b == m:
            return ("gone", "they deleted it and I left it alone")
        return ("CONFLICT", "they deleted it and I changed it")

    if m == t:
        return ("same", "both sides agree")
    if b is not None and b == m:
        return ("take-theirs", "only they changed it")
    if b is not None and b == t:
        return ("keep-mine", "only I changed it")
    if b is None:
        return ("CONFLICT", "both sides added a different file at this path")
    return ("CONFLICT", "both sides changed it, differently")


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        return 2
    base_dir, mine_dir, theirs_dir = sys.argv[1], sys.argv[2], sys.argv[3]
    out_dir = None
    if "--apply" in sys.argv:
        i = sys.argv.index("--apply")
        if i + 1 >= len(sys.argv):
            print("--apply needs a directory to write to")
            return 2
        out_dir = sys.argv[i + 1]

    base, mine, theirs = walk(base_dir), walk(mine_dir), walk(theirs_dir)
    every = sorted(set(base) | set(mine) | set(theirs))

    buckets = {}
    for rel in every:
        what, why = decide(base.get(rel), mine.get(rel), theirs.get(rel))
        buckets.setdefault(what, []).append((rel, why))

    order = ["CONFLICT", "take-theirs", "keep-mine", "same", "gone"]
    print(f"{len(every)} paths across the three trees\n")
    for what in order:
        rows = buckets.get(what, [])
        print(f"{what:<12} {len(rows)}")
    print()

    conflicts = buckets.get("CONFLICT", [])
    if conflicts:
        lists = [(r, w) for r, w in conflicts if r in LISTS]
        rest = [(r, w) for r, w in conflicts if r not in LISTS]
        if lists:
            print("CONFLICTS in the files that are lists — these are almost always")
            print("a union, and are the ones to do first:")
            for rel, why in lists:
                print(f"   {rel:<28} {why}")
            print()
        if rest:
            print("CONFLICTS elsewhere — read these properly, one at a time:")
            for rel, why in rest:
                print(f"   {rel:<28} {why}")
            print()

    incoming = buckets.get("take-theirs", [])
    if incoming:
        print("Coming in from theirs:")
        for rel, why in incoming:
            print(f"   {rel:<28} {why}")
        print()

    if out_dir:
        if os.path.exists(out_dir):
            shutil.rmtree(out_dir)
        wrote = 0
        for what in ["take-theirs", "keep-mine", "same", "CONFLICT"]:
            for rel, _ in buckets.get(what, []):
                # A conflict is copied from MINE and listed above. The tree is
                # never left half-merged: it always builds, and the conflicts
                # are a to-do list rather than a set of holes.
                #
                # Except where mine does not exist — a file I deleted and they
                # changed. Writing nothing there would silently lose their
                # work, which is the single thing this tool exists to prevent,
                # so theirs goes in and the conflict says why.
                src_root = theirs_dir if what == "take-theirs" else mine_dir
                src = os.path.join(src_root, rel.replace("/", os.sep))
                if not os.path.exists(src):
                    src = os.path.join(theirs_dir, rel.replace("/", os.sep))
                if not os.path.exists(src):
                    src = os.path.join(mine_dir, rel.replace("/", os.sep))
                if not os.path.exists(src):
                    print(f"   !! {rel} exists in no tree to copy from")
                    continue
                dst = os.path.join(out_dir, rel.replace("/", os.sep))
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                shutil.copy2(src, dst)
                wrote += 1
        print(f"wrote {wrote} files to {out_dir}")
        if conflicts:
            print(f"{len(conflicts)} of them are MY version and still need merging by hand")

    return 1 if conflicts else 0


if __name__ == "__main__":
    sys.exit(main())
