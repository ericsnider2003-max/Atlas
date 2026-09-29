#!/usr/bin/env python3
"""The parts of the catalogue the module reference doesn't cover.

Writes, next to the repository root (the folder that holds atlas/):

    CATALOG_TESTS_<date>.md       every test target, file and test, and the ignored ones
    CATALOG_COMMANDS_<date>.md    every spoken/typed command (intent + phrases) and
                                  every `atlas <word>` command-line command
    CATALOG_SETTINGS_<date>.md    every setting in the Settings page, by group

Generated from the tree, like genreports.py and genref.py: regenerate rather
than edit.   python3 atlas/docs/gencatalog_extra.py
"""
import os, re, datetime

HERE = os.path.dirname(os.path.abspath(__file__))
ATLAS = os.path.dirname(HERE)
ROOT = os.path.dirname(ATLAS)
TODAY = datetime.date.today().isoformat()
FOOTER = "\n---\nMEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE.\n"


def read(p):
    with open(p, encoding="utf-8", errors="replace") as f:
        return f.read()


# ---------------------------------------------------------------- tests

def tests_catalog():
    tdir = os.path.join(ATLAS, "tests")
    cargo = read(os.path.join(ATLAS, "Cargo.toml"))
    targets = re.findall(r'\[\[test\]\]\s*name\s*=\s*"([^"]+)"\s*path\s*=\s*"([^"]+)"', cargo)
    allrs = read(os.path.join(tdir, "all.rs"))
    in_all = re.findall(r'#\[path = "([^"]+)"\]', allrs)
    files = {}
    ignored = []
    for f in sorted(os.listdir(tdir)):
        if not f.endswith(".rs"):
            continue
        s = read(os.path.join(tdir, f))
        names = re.findall(r"#\[test\]\s*(?:#\[[^\]]*\]\s*)*fn\s+([a-z0-9_]+)", s)
        for m in re.finditer(r"#\[ignore(?:\s*=\s*\"([^\"]*)\")?\]\s*(?:#\[[^\]]*\]\s*)*fn\s+([a-z0-9_]+)", s):
            ignored.append((f, m.group(2), m.group(1) or ""))
        doc = ""
        for line in s.splitlines():
            t = line.strip()
            if t.startswith("//!"):
                doc = t[3:].strip()
                if doc:
                    break
            elif t and not t.startswith("//"):
                break
        files[f] = (names, doc)
    # Unit tests inside src/.
    unit = 0
    unit_files = 0
    for base, _d, fs in os.walk(os.path.join(ATLAS, "src")):
        for f in fs:
            if f.endswith(".rs"):
                n = len(re.findall(r"#\[test\]", read(os.path.join(base, f))))
                if n:
                    unit += n
                    unit_files += 1
    where = {}
    for f in files:
        if f in in_all:
            where[f] = "all"
        else:
            t = [n for n, p in targets if p == f"tests/{f}"]
            where[f] = t[0] if t else ("helper" if f in ("common.rs",) or not files[f][0] else "—")
    total = sum(len(v[0]) for v in files.values())
    L = [f"# Test catalogue — personal Atlas\n**{TODAY}. Generated from `atlas/tests/` and `atlas/src/`.**\n"]
    L.append(f"- **{len(files)} test files**, **{total} integration tests** in them.")
    L.append(f"- **{unit} unit tests** inside {unit_files} files under `src/`.")
    L.append(f"- **{len(targets)} test targets** in Cargo.toml (`autotests = false`): `all` holds {len(in_all)} files;"
             " the rest have their own target because they share global install state or declare `mod common;`.")
    L.append(f"- **{len(ignored)} ignored** (listed below: each needs something this machine doesn't have).")
    L.append("\nRun everything: `./verify.sh` from the folder holding `atlas/`, or `cargo test --no-fail-fast` in `atlas/`."
             " A new test file needs a `mod` line in `tests/all.rs` or its own `[[test]]`, or it never runs.\n")
    L.append("## Ignored tests\n")
    if ignored:
        L.append("| file | test | why |\n|---|---|---|")
        for f, n, why in ignored:
            L.append(f"| `{f}` | `{n}` | {why or '(no reason given)'} |")
    else:
        L.append("*(none)*")
    L.append("\n## Test targets\n")
    L.append("| target | file |\n|---|---|")
    for n, p in targets:
        L.append(f"| `{n}` | `{p}` |")
    L.append("\n## Every test file\n")
    L.append("| file | target | tests | what it covers |\n|---|---|---:|---|")
    for f in sorted(files):
        names, doc = files[f]
        L.append(f"| `{f}` | {where[f]} | {len(names)} | {doc.replace('|', '/')} |")
    L.append("\n## Every test, by file\n")
    for f in sorted(files):
        names, _ = files[f]
        if not names:
            continue
        L.append(f"### `{f}` ({len(names)})\n")
        L += [f"- `{n}`" for n in names]
        L.append("")
    return "\n".join(L) + FOOTER


# ---------------------------------------------------------------- commands

def commands_catalog():
    y = read(os.path.join(ATLAS, "config", "commands.yaml"))
    # A comment above an entry belongs to that entry; one inside it, before
    # its phrases, too.
    entries = []
    pending = []
    cur = None
    for line in y.splitlines():
        t = line.strip()
        m = re.match(r"-\s*intent:\s*([a-z0-9_]+)", t)
        if m:
            cur = [m.group(1), "", list(pending), False]
            entries.append(cur)
            pending = []
        elif t.startswith("#"):
            if cur is not None and not cur[3]:
                cur[2].append(t)
            else:
                pending.append(t)
        elif cur is not None and t:
            cur[1] += line + "\n"
            if t.startswith("phrases"):
                cur[3] = True
        elif not t:
            pending = [] if cur is None or cur[3] else pending
    entries = [(n, b + "\n".join(c)) for n, b, c, _ in entries]
    rows = []
    for name, body in entries:
        ph = re.search(r"phrases:\s*\[(.*?)\]", body, re.S)
        phrases = re.findall(r'"((?:[^"\\]|\\.)*)"', ph.group(1)) if ph else []
        arg = "takes_argument: true" in body
        comment = " ".join(l.strip()[1:].strip() for l in body.splitlines() if l.strip().startswith("#"))
        rows.append((name, phrases, arg, comment))
    main = read(os.path.join(ATLAS, "src", "main.rs"))
    cli = set()
    for line in main.splitlines():
        code = line.split("//")[0]
        if "words.first()" not in code:
            continue
        cli.update(re.findall(r'Some\("([a-z][a-z0-9-]*)"\)', code))
    L = [f"# Command catalogue — personal Atlas\n**{TODAY}. Generated from `config/commands.yaml` and `src/main.rs`.**\n"]
    L.append(f"**{len(rows)} things you can say or type** (each an intent; the longest matching phrase wins),"
             f" and **{len(cli)} `atlas <word>` command-line commands.**\n")
    L.append("## What you can say or type\n")
    L.append("| intent | takes more words | phrases | note |\n|---|---|---|---|")
    for name, phrases, arg, comment in sorted(rows):
        L.append(f"| `{name}` | {'yes' if arg else ''} | {'; '.join(phrases).replace('|', '/')} | {comment.replace('|', '/')[:200]} |")
    L.append("\n## `atlas <word>` on the command line\n")
    L.append(", ".join(f"`atlas {c}`" for c in sorted(cli)))
    return "\n".join(L) + FOOTER


# ---------------------------------------------------------------- settings

def settings_catalog():
    s = read(os.path.join(ATLAS, "src", "settings.rs"))
    groups = re.findall(r'\("([^"]+)",\s*"([^"]+)"\),', s[s.index("GROUP_ORDER"):s.index("];", s.index("GROUP_ORDER"))])
    items = []
    for m in re.finditer(r'toggle\(\s*"([^"]+)",\s*"([^"]+)",\s*"((?:[^"\\]|\\.)*)"', s):
        tail = s[m.end():m.end() + 600]
        g = re.search(r',\s*(?:Weight::)?(Preference|Resource|Sensitive|Permission)\s*,\s*"([^"]+)"\)', tail)
        items.append((m.group(1), m.group(2), m.group(3), "switch", g.group(2) if g else "?", g.group(1) if g else "?"))
    for m in re.finditer(r'Setting\s*\{\s*key:\s*"([^"]+)"\.into\(\),\s*name:\s*"([^"]+)"\.into\(\),\s*what:\s*"((?:[^"\\]|\\.)*)"', s):
        tail = s[m.end():m.end() + 900]
        g = re.search(r'group:\s*"([^"]+)"', tail)
        w = re.search(r"weight:\s*(?:Weight::)?(\w+)", tail)
        v = re.search(r"value:\s*Value::(\w+)", tail)
        items.append((m.group(1), m.group(2), m.group(3), (v.group(1).lower() if v else "value"), g.group(1) if g else "?", w.group(1) if w else "?"))
    restart = re.findall(r'"([a-z_.]+)"', s[s.index("NEEDS_A_RESTART"):s.index("];", s.index("NEEDS_A_RESTART"))])
    L = [f"# Settings catalogue — personal Atlas\n**{TODAY}. Generated from `src/settings.rs`.**\n"]
    L.append(f"**{len(items)} settings** on the Settings page (Atlas window → Settings, or say \"show me settings\")."
             " Kept in `config/settings.yaml` over the shipped `config/tools.yaml`. Most apply within seconds;"
             f" the {len(restart)} marked *restart* wait for Atlas to start again. Items added in a loop"
             " (per-app permissions and similar) aren't listed individually.\n")
    order = [g for g, _ in groups]
    seen = order + sorted({i[4] for i in items} - set(order))
    for g in seen:
        rows = [i for i in items if i[4] == g]
        if not rows:
            continue
        note = dict(groups).get(g, "")
        L.append(f"## {g}\n\n{note}\n")
        L.append("| key | name | kind | weight | restart | what it does |\n|---|---|---|---|---|---|")
        for k, n, w, kind, _g, wt in rows:
            L.append(f"| `{k}` | {n} | {kind} | {wt} | {'restart' if k in restart else ''} | {w.replace('|', '/')} |")
        L.append("")
    return "\n".join(L) + FOOTER


def main():
    for name, body in [
        (f"CATALOG_TESTS_{TODAY}.md", tests_catalog()),
        (f"CATALOG_COMMANDS_{TODAY}.md", commands_catalog()),
        (f"CATALOG_SETTINGS_{TODAY}.md", settings_catalog()),
    ]:
        with open(os.path.join(ROOT, name), "w", encoding="utf-8") as f:
            f.write(body)
        print(f"wrote {name} ({len(body.splitlines())} lines)")


if __name__ == "__main__":
    main()
