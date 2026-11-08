"""A poor man's name resolver, for when the compiler is out of reach.

`rustfmt` proves a file PARSES. It says nothing about whether the names in it
exist. That gap is not theoretical: a scripted edit in this project inserted a
call to `stop_is_in_noise` and silently failed to insert the function, and
every parse check passed.

This does the one check that would have caught it, twice over:

1.  Every `crate::module::Name` and `atlas::module::Name` reference anywhere in
    src/ or tests/ resolves to something declared `pub` in `src/module.rs`.
2.  Every bare call `name(...)` inside a module resolves to a function declared
    in that same file, imported by a `use`, or known to be from the standard
    library / a method.

It is deliberately crude and it errs toward silence rather than noise: a false
alarm here costs more attention than it saves. What it will not miss is a call
to something that does not exist anywhere.

Run:  python3 docs/resolve_check.py
"""

import os
import re
import sys

SRC = "src"
TESTS = "tests"

# Things that look like calls and are not this codebase's.
BUILTIN = {
    "format", "println", "print", "eprintln", "write", "writeln", "vec", "panic",
    "assert", "assert_eq", "assert_ne", "matches", "Some", "Ok", "Err", "None",
    "String", "Vec", "Box", "Option", "Result", "min", "max", "abs", "sqrt",
    "powi", "powf", "ln", "exp", "round", "floor", "ceil", "len", "iter", "map",
    "filter", "collect", "unwrap", "unwrap_or", "unwrap_or_else", "unwrap_or_default",
    "to_string", "trim", "split", "push", "push_str", "contains", "starts_with",
    "ends_with", "chars", "sum", "count", "sort", "sort_by", "sort_by_key",
    "is_empty", "clone", "into", "from", "as_str", "to_vec", "get", "insert",
    "remove", "retain", "extend", "join", "find", "position", "any", "all",
    "take", "skip", "rev", "zip", "enumerate", "last", "first", "next", "fold",
    "expect", "and_then", "or_else", "ok_or_else", "is_finite", "is_nan",
    "saturating_sub", "checked_mul", "total_cmp", "cmp", "then", "then_some",
    "to_uppercase", "to_lowercase", "to_owned", "lines", "replace", "parse",
    "read_to_string", "new", "default", "with_capacity", "truncate", "dedup",
    "copied", "cloned", "flat_map", "flatten", "chain", "once", "repeat",
    "max_by", "min_by", "max_by_key", "min_by_key", "partial_cmp", "signum",
    "eq_ignore_ascii_case", "split_whitespace", "strip_prefix", "strip_suffix",
    "is_some", "is_none", "is_ok", "is_err", "as_ref", "as_mut", "as_slice",
    "windows", "chunks", "chunks_exact", "rotate_right", "wrapping_add",
    "wrapping_mul", "to_be_bytes", "from_be_bytes", "checked_sub", "abs_diff",
    "trim_matches", "trim_start", "trim_end", "splitn", "rsplitn", "char_indices",
    "is_alphanumeric", "is_uppercase", "is_ascii_alphanumeric", "from_digit",
    "elapsed", "as_millis", "as_secs", "now", "read", "read_to_end", "exists",
    "join_lines", "args", "spawn", "output", "status", "success", "stdout",
    "stderr", "wait_with_output", "take_while", "skip_while", "step_by",
    "resize", "swap", "reverse", "binary_search", "is_sorted", "concat",
    "and", "or", "xor", "not", "add", "sub", "mul", "div", "rem", "neg",
}


# Rust keywords that can be followed by an open bracket. `let (a, b) = ...` is
# a destructuring bind, not a call, and it appears everywhere.
KEYWORDS = {
    "if", "for", "while", "match", "return", "in", "as", "move", "where",
    "let", "pub", "fn", "mut", "ref", "impl", "dyn", "else", "loop", "unsafe",
    "crate", "super", "self", "type", "use", "mod", "enum", "struct", "trait",
    "const", "static", "async", "await", "yield", "box",
}


def rust_files(*dirs):
    for d in dirs:
        for root, _, names in os.walk(d):
            for n in names:
                if n.endswith(".rs"):
                    yield os.path.join(root, n)


def blanked(span):
    """A span replaced by spaces, with its newlines kept.

    Keeping the newlines matters: without them every line number this tool
    reports is wrong by however much it stripped above, and a checker that
    points at the wrong line is one nobody trusts twice.
    """
    return "".join("\n" if c == "\n" else " " for c in span)


def strip_comments_and_strings(text):
    """Rough, but enough: we only need call sites, not exact spans."""
    out = []
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == '/' and i + 1 < n and text[i + 1] == '*':
            end = text.find('*/', i + 2)
            end = n if end < 0 else end + 2
            out.append(blanked(text[i:end]))
            i = end
        elif c == '/' and i + 1 < n and text[i + 1] == '/':
            start = i
            while i < n and text[i] != '\n':
                i += 1
            out.append(blanked(text[start:i]))
        elif c == '#' and i + 1 < n and text[i + 1] == '[':
            # An attribute. `#[repr(C)]` and `#[link(name = "user32")]` look
            # exactly like calls and are not.
            depth = 0
            start = i
            while i < n:
                if text[i] == '[':
                    depth += 1
                elif text[i] == ']':
                    depth -= 1
                    if depth == 0:
                        i += 1
                        break
                i += 1
            out.append(blanked(text[start:i]))
        elif c == 'r' and i + 1 < n and text[i + 1] in '#"':
            # A raw string. The CSS in `hub.rs` lives in one of these, and
            # `linear-gradient(` inside it is not a function this crate is
            # missing.
            j = i + 1
            hashes = 0
            while j < n and text[j] == '#':
                hashes += 1
                j += 1
            if j < n and text[j] == '"':
                close = '"' + '#' * hashes
                end = text.find(close, j + 1)
                stop = n if end < 0 else end + len(close)
                out.append(blanked(text[i:stop]))
                i = stop
            else:
                out.append(c)
                i += 1
        elif c == "'" and i + 2 < n and (
            text[i + 2] == "'" or (text[i + 1] == '\\' and i + 3 < n and text[i + 3] == "'")
        ):
            # A char literal. `'\"'` is one character, and treating its quote
            # as the start of a string flips the parity of every string after
            # it in the file — which is how CSS three hundred lines further
            # down got reported as missing functions.
            end = i + 3 if text[i + 2] == "'" else i + 4
            out.append(blanked(text[i:end]))
            i = end
        elif c == '"':
            start = i
            i += 1
            while i < n:
                if text[i] == '\\':
                    i += 2
                    continue
                if text[i] == '"':
                    i += 1
                    break
                i += 1
            out.append(blanked(text[start:i]))
        else:
            out.append(c)
            i += 1
    return ''.join(out)


def public_names(path):
    """Everything a module exposes."""
    text = open(path, encoding="utf-8").read()
    names = set()
    for m in re.finditer(
        r"^\s*pub (?:fn|struct|enum|trait|type|const|static|mod)\s+([A-Za-z_][A-Za-z0-9_]*)",
        text, re.M,
    ):
        names.add(m.group(1))
    # associated items, reachable as Type::name
    for m in re.finditer(r"^\s*pub fn\s+([A-Za-z_][A-Za-z0-9_]*)", text, re.M):
        names.add(m.group(1))
    # enum variants
    for block in re.finditer(r"pub enum\s+\w+[^{]*\{(.*?)\n\}", text, re.S):
        for v in re.finditer(r"^\s{4}([A-Z][A-Za-z0-9_]*)", block.group(1), re.M):
            names.add(v.group(1))
    return names


def declared_in_file(path):
    text = open(path, encoding="utf-8").read()
    names = set()
    for m in re.finditer(r"\bfn\s+([a-z_][A-Za-z0-9_]*)", text):
        names.add(m.group(1))
    for m in re.finditer(r"\b(?:let|const|static)\s+(?:mut\s+)?([a-z_][A-Za-z0-9_]*)", text):
        names.add(m.group(1))
    # imported names, including brace lists
    for m in re.finditer(r"^\s*use\s+([^;]+);", text, re.M):
        body = m.group(1)
        for part in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", body):
            names.add(part)
    # closures bound to a name
    for m in re.finditer(r"\blet\s+(?:mut\s+)?([a-z_][A-Za-z0-9_]*)\s*=\s*\|", text):
        names.add(m.group(1))
    return names


def every_fn_in_crate():
    """Every function name defined anywhere in src/, plus every local binding.

    Deliberately crate-wide rather than per-file. A per-file check fires on
    every legitimately imported helper and produces a thousand lines nobody
    reads, which is worse than no check. Crate-wide it fires only on a call to
    something that exists NOWHERE — which is the one failure a parse check
    cannot see, and the one that actually happened.
    """
    names = set()
    for p in rust_files(SRC):
        text = open(p, encoding="utf-8").read()
        for m in re.finditer(r"\bfn\s+([a-z_][A-Za-z0-9_]*)", text):
            names.add(m.group(1))
        for m in re.finditer(r"\blet\s+(?:mut\s+)?([a-z_][A-Za-z0-9_]*)\s*=", text):
            names.add(m.group(1))
        for m in re.finditer(r"\bconst\s+([A-Za-z_][A-Za-z0-9_]*)", text):
            names.add(m.group(1))
        for m in re.finditer(r"^\s*use\s+([^;]+);", text, re.M):
            for part in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", m.group(1)):
                names.add(part)
        for m in re.finditer(r"\b([a-z_][A-Za-z0-9_]*)\s*:\s*(?:&|impl|dyn|\w)", text):
            names.add(m.group(1))
    return names


def main():
    modules = {}
    for p in rust_files(SRC):
        rel = os.path.relpath(p, SRC)[:-3].replace(os.sep, "::")
        if rel.endswith("::mod"):
            rel = rel[: -len("::mod")]
        modules.setdefault(rel, set()).update(public_names(p))
        # a directory module re-exports what its files declare
        if os.sep in os.path.relpath(p, SRC):
            parent = os.path.relpath(p, SRC).split(os.sep)[0]
            modules.setdefault(parent, set()).update(public_names(p))

    problems = []

    for p in rust_files(SRC, TESTS):
        text = strip_comments_and_strings(open(p, encoding="utf-8").read())
        for m in re.finditer(r"\b(?:crate|atlas)::([a-z_][a-z0-9_]*)::([A-Za-z_][A-Za-z0-9_]*)", text):
            mod, name = m.group(1), m.group(2)
            if mod not in modules:
                problems.append(f"{p}: refers to module `{mod}`, which has no src/{mod}.rs")
            elif name not in modules[mod]:
                problems.append(f"{p}: `{mod}::{name}` -- nothing public by that name in src/{mod}.rs")

        # `use crate::market::{Market, Bar}` -- the brace form, which the
        # pattern above cannot see because `{` is not an identifier character.
        # Every one of these was invisible until a merge deleted the module
        # they pointed at and the checker reported the tree clean.
        for m in re.finditer(
            r"\buse\s+(?:crate|atlas)::([a-z_][a-z0-9_]*)::\{([^}]*)\}", text
        ):
            mod, names = m.group(1), m.group(2)
            if mod not in modules:
                problems.append(f"{p}: imports from module `{mod}`, which has no src/{mod}.rs")
                continue
            for raw in names.split(","):
                name = raw.strip().split(" as ")[0].strip()
                if not name or name in {"self", "*"} or not name[0].isupper() and "::" in name:
                    continue
                if name and name not in modules[mod]:
                    problems.append(
                        f"{p}: imports `{mod}::{name}` -- nothing public by that name in src/{mod}.rs"
                    )

    known = every_fn_in_crate() | BUILTIN
    for p in rust_files(SRC):
        raw = open(p, encoding="utf-8").read()
        text = strip_comments_and_strings(raw)
        for m in re.finditer(r"(?<![.:\w])([a-z_][a-z0-9_]{2,})\s*\(", text):
            name = m.group(1)
            if name in known or name.startswith("_"):
                continue
            if name in KEYWORDS:
                continue
            line = raw[: m.start()].count("\n") + 1
            problems.append(f"{p}:{line}: calls `{name}(` -- nothing by that name exists anywhere")

    seen, unique = set(), []
    for x in problems:
        if x not in seen:
            seen.add(x)
            unique.append(x)

    if unique:
        print(f"{len(unique)} unresolved name(s):")
        for x in unique:
            print("  " + x)
        return 1
    print("every cross-module reference resolves, and every call names something that exists")
    return 0


if __name__ == "__main__":
    sys.exit(main())
