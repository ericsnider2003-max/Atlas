#!/usr/bin/env python3
# Generates atlas/tests/fixtures/what_each_version_built.tsv (26 Sep 2026).
# usage: python3 inventory_versions.py <repo holding 8d507ee, bd51961 and c069ce1> > atlas/tests/fixtures/what_each_version_built.tsv
# c069ce1 is branch chat3 from atlas-project-chat-25b-25j.bundle (25b, 25h, 25i, 25j as delivered).
# 0523505 is branch project-line: 25k, 26a and 26b imported on top of c069ce1, as delivered.
"""Inventory everything each Atlas version added over its base, as rows for
tests/nothing_from_any_version_was_lost.rs.

Row format (tab-separated):  version  kind  where  name
kinds: file, item (pub item in a src module), test (a #[test] fn), intent,
command (a top-level `atlas <word>`), page (a hub Page variant).
"""
import re, subprocess, sys

REPO = sys.argv[1]
VERSIONS = [  # label, base, tip
    ("main chat (master 8d507ee)", "4a1164a", "8d507ee"),
    ("rounds chat (tenth-sep bd51961)", "22958f1", "bd51961"),
    ("Atlas Project chat (25j, c069ce1)", "4a1164a", "c069ce1"),
    # 27 Sep 2026: that chat's 25k, 26a and 26b, imported as delivered on
    # project-line (f8c8e68, bcb2273, 0523505) and merged as 0689e9e/b462956.
    ("Atlas Project chat (25k-26b, 0523505)", "c069ce1", "0523505"),
]

def git(*a):
    return subprocess.run(["git", "-C", REPO, *a], capture_output=True, text=True, check=True).stdout

def files(rev):
    return set(git("ls-tree", "-r", "--name-only", rev).split("\n")) - {""}

def show(rev, path):
    try:
        return git("show", f"{rev}:{path}")
    except subprocess.CalledProcessError:
        return ""

ITEM = re.compile(r"^\s*pub(?:\([a-z]+\))?\s+(?:async\s+)?(fn|struct|enum|trait|const|static|type|mod)\s+([A-Za-z_][A-Za-z0-9_]*)", re.M)
TEST = re.compile(r"#\[test\][^\n]*\n(?:\s*#\[[^\n]*\n)*\s*(?:pub\s+)?fn\s+([a-z_0-9]+)", re.M)
INTENT = re.compile(r"^\s*-\s*intent:\s*([A-Za-z_0-9]+)", re.M)
COMMAND = re.compile(r'words\.first\(\)\.map\(\|s\| s\.as_str\(\)\) == Some\("([a-z][a-z-]*)"\)')
PAGE_ENUM = re.compile(r"pub enum Page \{(.*?)\n\}", re.S)

def items(text):
    return {f"{k} {n}" for k, n in ITEM.findall(text)}

def tests(text):
    return set(TEST.findall(text))

def pages(text):
    m = PAGE_ENUM.search(text)
    if not m:
        return set()
    return set(re.findall(r"^\s*([A-Z][A-Za-z0-9]*)\s*[,({]", m.group(1), re.M))

rows = []
for label, base, tip in VERSIONS:
    fb, ft = files(base), files(tip)
    for f in sorted(ft - fb):
        if f.startswith("atlas/vendor/") and not f.endswith(("Cargo.toml", ".md")):
            continue  # a vendored crate is checked by its Cargo.toml and notes, not file by file
        rows.append((label, "file", f, ""))
    changed = set(git("diff", "--name-only", base, tip).split("\n")) - {""}
    for f in sorted(changed):
        if not f.endswith(".rs") or f.startswith("atlas/vendor/"):
            continue
        before, after = show(base, f), show(tip, f)
        if f.startswith("atlas/src/"):
            for it in sorted(items(after) - items(before)):
                rows.append((label, "item", f, it))
        for t in sorted(tests(after) - tests(before)):
            rows.append((label, "test", f, t))
    y = "atlas/config/commands.yaml"
    for i in sorted(set(INTENT.findall(show(tip, y))) - set(INTENT.findall(show(base, y)))):
        rows.append((label, "intent", y, i))
    m = "atlas/src/main.rs"
    for c in sorted(set(COMMAND.findall(show(tip, m))) - set(COMMAND.findall(show(base, m)))):
        rows.append((label, "command", m, c))
    h = "atlas/src/hub.rs"
    for p in sorted(pages(show(tip, h)) - pages(show(base, h))):
        rows.append((label, "page", h, p))

for r in rows:
    print("\t".join(r))
