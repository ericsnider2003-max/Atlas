#!/usr/bin/env python3
"""Compile and RUN the part of Atlas that needs no crates.

Why this exists
---------------
crates.io is blocked from where this tree is being built, so `cargo test` has
never once run here. Everything up to now has been checked by proxy: files
parse, cross-module names resolve, guards are present, a Python reference
agrees with the numbers in the tests. All of that is real and none of it is a
compiler.

It turns out a large and important part of the tree needs no crates at all.
`market` was written that way on purpose -- pure `std`, so it builds on a
bare Windows machine with nothing installed -- and the market-facing modules built on
top of it inherited the property without anyone saying so. Those modules can be
assembled into a crate of their own and handed straight to `rustc`, which is
sitting right here.

So this builds an ISLAND: the std-only modules, their tests, one `rustc`
invocation, and a real pass/fail.

What the island is not
----------------------
It is not the shipped build. Two things are done to make the assembly work,
and both are worth knowing about:

1. `serde` is not available, so its derives and attributes are stripped. That
   means the island proves the LOGIC of a module and says nothing about
   whether it serialises. Anything under test here that matters on disk is
   still only checked by `ATLAS.bat` menu 7.
2. Modules are re-rooted, so `crate::` means the island rather than `atlas`.
   A module that reached outside the island would fail to compile here -- and
   that failure is information, not a defect in the harness.

Everything else -- the type checking, the borrow checking, the test bodies, the
assertions -- is the genuine article.

Why it stops where it stops
---------------------------
The line is drawn at `store`, which is `serde_json` and the filesystem. Modules
that only TOUCH the store in a `load`/`save` pair have that pair cut by name
(see ISLAND_TRIM) and the rest of them compiles; modules whose behaviour runs
THROUGH the store do not come in at all, because a stubbed store would make
their tests green while proving nothing about the thing they check. This
codebase has been bitten four times by tests that pass without reaching the code
they name. Adding a fifth on purpose, in the verification harness of all places,
is not a trade worth making. `config`, `platform`, `voice` and everything
downstream of them stay outside for the same reason, plus the crates.

What it found on its first run
------------------------------
Written after the market work was already "finished" by every proxy check in
the tree, and the first real compile turned up seven things that would have
failed `ATLAS.bat` menu 7 on Eric's machine:

- four in modules that have since left personal Atlas (28 Sep 2026): a
  function that did not compile, two tests asserting opposite things, and two
  broken call sites.
- `tests/levels.rs` called `cost_share` with two arguments; it takes three and
  returns an `Option`.
- `tests/together.rs` looked for "looks safer than it is" in a sentence that
  says "look safer than it is".
- `serde_json` was used by twelve assertions and was not a dev-dependency, so
  the whole `tests/` tree failed to build.

None of those are exotic. All seven are the ordinary cost of writing Rust
without a compiler, and all seven were invisible to parsing, name resolution and
the guard manifest.

    python3 docs/island.py            build and run
    python3 docs/island.py --keep     leave the assembled crate for reading
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Top-level modules with no crate dependency beyond `market` and each other.
# Deliberately a hand-kept list: a module drifting off it should be a decision
# somebody made, not something that happens quietly.
ISLAND = [
    "digest", "fxday", "asia", "rollover", "standdown", "live", "levels", "stale",
    "refusals", "together",
]

# Test files to fold in. Named rather than globbed, because a test file that
# reaches for `store` or `config` belongs to the mainland and its absence here
# is not a gap.
ISLAND_TESTS = [
    "digest", "fxday", "asia", "rollover", "standdown", "levels", "live", "stale",
    "refusals", "together",
    # Not a module's tests: the guard manifest. Folded in so every needle is
    # decoded the way rustc decodes a string literal, rather than the way a
    # Python script guesses it will.
    "guards",
    # Nor these two: they read `src/*.rs` from disk and check the module
    # reference graph and the capability registry, rather than testing a
    # module of their own. Both are pure `std` — no crate dependency at all —
    # so there was never a reason they were missing. This is exactly the gap
    # an outside audit found on 11 Sep 2026: `402 passed` proved the logic of
    # sixteen modules was correct and said nothing about whether anything
    # could actually reach them, because the one test that checks reachability
    # was not in the run.
    "wiring",
    "capability_wiring",
    # `bug_sweep` too, with the fit-machine tests cut (see ISLAND_SKIP_TESTS):
    # the dead-capability ceiling and the "ceilings only shrink" ratchet live
    # here, and neither had ever run either.
    "bug_sweep",
]


# Test files that must NOT have `atlas::` rewritten to `crate::`. The guard
# manifest is the case: its needles are strings to be found in the real source
# files, not paths to be resolved, so rewriting them rewrites the thing being
# looked for and every guard on a `atlas::`-prefixed call reports as missing.
ISLAND_NO_REWRITE = {"guards"}

# Tests that cannot run here, by name, and why. Each one is printed on every
# run: a skip nobody sees is a skip that becomes permanent. Each entry is
# (reason, [test names]) -- the reason varies (no serde; a real crate
# dependency the island doesn't carry), so it is stated per group rather than
# assumed.
ISLAND_SKIP_TESTS = {
    # These six need `atlas::fit`, which needs `health`, which needs `hollow`
    # -- a real dependency chain, not a trim candidate. Everything else in
    # `bug_sweep` (the dead-capability ceiling, the config/settings sweeps,
    # the documentation-vs-code check) is pure `std` and now runs for real.
    "bug_sweep": (
        "needs atlas::fit, a real dependency chain (fit -> health -> hollow) the island doesn't carry",
        [
            "atlas_measures_the_machine_rather_than_assuming_one",
            "a_bigger_machine_is_allowed_to_do_more",
            "a_machine_too_small_for_a_model_still_works",
            "integrated_graphics_are_not_counted_as_spare_memory",
            "atlas_can_say_what_it_is_holding_that_nothing_uses",
            "something_named_in_the_config_is_never_offered_up",
        ],
    ),
}

# Modules that are std-only APART FROM a named handful of methods that read or
# write the store. Those methods are cut, by name, and nothing else is touched.
#
# This is the one place the island removes CODE rather than attributes, so it is
# spelled out here rather than inferred: the method names are written down, a
# method that goes missing is reported, and everything cut is listed in the run
# output. What it costs is real -- the cut methods are not compiled here, so
# their signatures are checked only by `ATLAS.bat` menu 7. What it buys is that
# the logic around them is checked, which is otherwise checked by nothing.
ISLAND_TRIM = {
    "refusals": ["load", "save"],
}


def cut_methods(text: str, names: list, where: str) -> str:
    """Remove named `pub fn`s, and the store imports they needed."""
    lines = text.splitlines()
    out = []
    i = 0
    found = []
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        if stripped.startswith("use crate::store::") or stripped.startswith("use crate::error::"):
            i += 1
            continue
        hit = next(
            (n for n in names
             if stripped.startswith(f"pub fn {n}(") or stripped.startswith(f"fn {n}(")),
            None,
        )
        if hit is None:
            out.append(line)
            i += 1
            continue
        found.append(hit)
        # Drop the doc comment and any attributes immediately above it too.
        while out and (
            out[-1].strip().startswith("///") or out[-1].strip().startswith("#[")
        ):
            out.pop()
        depth = 0
        started = False
        while i < len(lines):
            depth += lines[i].count("{") - lines[i].count("}")
            started = started or "{" in lines[i]
            i += 1
            if started and depth <= 0:
                break
    missing = [n for n in names if n not in found]
    if missing:
        print(f"  note: {where} no longer has {missing} -- the trim list is out of date")
    return "\n".join(out) + "\n"


def strip_serde(text: str) -> str:
    """Remove serde so the island compiles without it."""
    out = []
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("use serde::") or s.startswith("use serde_"):
            continue
        if s.startswith("#[serde(") and s.endswith(")]"):
            continue
        if s.startswith("#[derive(") and s.endswith(")]"):
            inner = s[len("#[derive(") : -len(")]")]
            kept = [
                d.strip()
                for d in inner.split(",")
                if d.strip() not in ("Serialize", "Deserialize")
            ]
            if not kept:
                continue
            line = line[: len(line) - len(line.lstrip())] + "#[derive(" + ", ".join(kept) + ")]"
        out.append(line)
    return "\n".join(out) + "\n"


def build(work: str) -> str:
    os.makedirs(work, exist_ok=True)

    # market, re-rooted
    shutil.copytree(os.path.join(ROOT, "src", "market"), os.path.join(work, "market"))
    market_root = open(os.path.join(work, "market", "mod.rs"), encoding="utf-8").read()
    os.remove(os.path.join(work, "market", "mod.rs"))
    market_root = re.sub(
        r"^pub mod (\w+);", r'#[path = "market/\1.rs"] pub mod \1;', market_root, flags=re.M
    )
    open(os.path.join(work, "market_root.rs"), "w", encoding="utf-8").write(market_root)

    lines = ['#[path = "market_root.rs"] pub mod market;']
    for name in ISLAND:
        src = open(os.path.join(ROOT, "src", f"{name}.rs"), encoding="utf-8").read()
        if name in ISLAND_TRIM:
            print(f"  trimmed from {name}: {ISLAND_TRIM[name]} (store round-trip not covered here)")
            src = cut_methods(src, ISLAND_TRIM[name], name)
        open(os.path.join(work, f"{name}.rs"), "w", encoding="utf-8").write(strip_serde(src))
        lines.append(f'#[path = "{name}.rs"] pub mod {name};')

    for name in ISLAND_TESTS:
        path = os.path.join(ROOT, "tests", f"{name}.rs")
        if not os.path.exists(path):
            continue
        body = open(path, encoding="utf-8").read()
        if name not in ISLAND_NO_REWRITE:
            body = body.replace("atlas::", "crate::")
        # A test that reads its own module's source (`hollow`-style checks)
        # asks for it relative to `tests/`. The island lives somewhere else,
        # so the path is re-pointed at the real tree rather than the copy --
        # which also means such a test reads what ships, not what was stripped.
        body = body.replace('include_str!("../', f'include_str!("{ROOT}/')
        if name in ISLAND_SKIP_TESTS:
            reason, skipped = ISLAND_SKIP_TESTS[name]
            print(f"  skipped in {name}: {skipped} ({reason})")
            body = cut_methods(body, skipped, f"tests/{name}.rs")
        lines.append("#[cfg(test)]")
        lines.append(f"mod {name}_tests {{\n{body}\n}}")

    root = os.path.join(work, "island.rs")
    open(root, "w", encoding="utf-8").write("\n".join(lines) + "\n")
    return root


def main() -> int:
    keep = "--keep" in sys.argv
    work = tempfile.mkdtemp(prefix="atlas-island-")
    shutil.rmtree(work)
    root = build(work)
    binary = os.path.join(work, "island")
    cc = subprocess.run(
        ["rustc", "--edition", "2021", "-A", "dead_code", "-A", "unused", "--test", root, "-o", binary],
        capture_output=True,
        text=True,
    )
    if cc.returncode != 0:
        print(cc.stderr)
        print(f"the island did not compile. Assembled crate left in {work}")
        return 1
    # Run from the real tree: the file-reading guards use paths like
    # "src/levels.rs", which are relative to the crate root.
    run = subprocess.run([binary], capture_output=True, text=True, cwd=ROOT)
    tail = [l for l in run.stdout.splitlines() if l.startswith("test result") or " FAILED" in l]
    failed = [l for l in run.stdout.splitlines() if l.endswith("... FAILED")]
    for l in failed:
        print(l)
    if failed:
        print(run.stdout[-4000:])
    print("\n".join(tail))
    if not keep:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print(f"assembled crate: {work}")
    return run.returncode


if __name__ == "__main__":
    sys.exit(main())
