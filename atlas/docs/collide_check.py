"""Name collisions, within one tree or across two about to be merged.

The third of this codebase's four recurring failures is near-identical names in
different modules. It has cost real time here more than once — `learn` beside
`learned`, `Reading` in six files, three separate `Refused`, four `Found`.

Merging two independently-developed trees is the moment that failure is most
likely, because neither side could see the other's names while choosing its
own. Nothing about it produces an error: both types compile, both are correct,
and the confusion lands months later on whoever reads one and reasons about the
other.

    python3 docs/collide_check.py <tree>            within one tree
    python3 docs/collide_check.py <mine> <theirs>   across two

Across two trees it reports only names that appear in BOTH and are defined in
different modules — the ones a merge creates. Within one it reports any public
type name defined in more than one place.
"""

import os
import re
import sys

# Names that genuinely recur and mean the same thing everywhere. Listing them
# is better than lowering the bar: the point of this tool is that a NEW
# collision is visible, and a wall of known ones hides it.
FINE = {"Config", "Error", "Kind", "State", "Item", "Row", "Entry"}


def types_in(root):
    """Every public type, and every module that defines one by that name."""
    found = {}
    src = os.path.join(root, "src")
    if not os.path.isdir(src):
        src = root
    for r, dirs, files in os.walk(src):
        dirs[:] = [d for d in dirs if d not in {"target", ".git"}]
        for f in files:
            if not f.endswith(".rs"):
                continue
            p = os.path.join(r, f)
            mod = os.path.relpath(p, src).replace(os.sep, "::")[:-3]
            text = open(p, encoding="utf-8", errors="replace").read()
            for m in re.finditer(
                r"^\s*pub (?:struct|enum|type|trait)\s+([A-Z][A-Za-z0-9_]*)", text, re.M
            ):
                found.setdefault(m.group(1), set()).add(mod)
    return found


def modules_in(root):
    src = os.path.join(root, "src")
    if not os.path.isdir(src):
        return set()
    return {f[:-3] for f in os.listdir(src) if f.endswith(".rs")}


def near(a, b):
    """Two module names a keystroke apart, like `learn` and `learned`."""
    if a == b:
        return False
    if a.startswith(b) or b.startswith(a):
        return abs(len(a) - len(b)) <= 2
    return False


def main():
    if len(sys.argv) == 2:
        tree = sys.argv[1]
        found = types_in(tree)
        bad = {n: m for n, m in found.items() if len(m) > 1 and n not in FINE}
        print(f"{len(found)} public type names in {tree}")
        if not bad:
            print("no name is defined in two places")
        else:
            print(f"{len(bad)} defined in more than one module:")
            for n, mods in sorted(bad.items()):
                print(f"   {n:<22} {', '.join(sorted(mods))}")

        mods = sorted(modules_in(tree))
        close = [(a, b) for i, a in enumerate(mods) for b in mods[i + 1:] if near(a, b)]
        if close:
            print("\nmodule names a keystroke apart:")
            for a, b in close:
                print(f"   {a}  /  {b}")
        return 1 if bad else 0

    if len(sys.argv) != 3:
        print(__doc__)
        return 2

    mine_root, theirs_root = sys.argv[1], sys.argv[2]
    mine, theirs = types_in(mine_root), types_in(theirs_root)

    clashes = []
    for name in sorted(set(mine) & set(theirs)):
        if name in FINE:
            continue
        mine_mods, their_mods = mine[name], theirs[name]
        # Same name in the same module is the same type on both sides — that is
        # a file merge, not a collision.
        if mine_mods == their_mods:
            continue
        clashes.append((name, mine_mods, their_mods))

    print(f"mine: {len(mine)} type names.  theirs: {len(theirs)}.")
    if not clashes:
        print("no type name is defined in different modules on the two sides")
    else:
        print(f"\n{len(clashes)} name(s) defined in different places on each side.")
        print("Merged, these become two public types with one name:\n")
        for name, a, b in clashes:
            print(f"   {name}")
            print(f"      mine:   {', '.join(sorted(a))}")
            print(f"      theirs: {', '.join(sorted(b))}")

    mods_a, mods_b = modules_in(mine_root), modules_in(theirs_root)
    new = sorted(mods_b - mods_a)
    if new:
        print(f"\nmodules arriving from theirs ({len(new)}):")
        print("   " + ", ".join(new))
    close = [(a, b) for a in sorted(mods_a) for b in new if near(a, b)]
    if close:
        print("\nand these arrive a keystroke away from one that already exists:")
        for a, b in close:
            print(f"   {a}  /  {b}   <- this is how `learn` beside `learned` happened")

    return 1 if clashes else 0


if __name__ == "__main__":
    sys.exit(main())
