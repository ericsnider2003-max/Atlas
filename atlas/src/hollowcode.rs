//! Hollow code, in whatever language it arrives in.
//!
//! `hollow.rs` judges an answer: did Atlas say something, or did it produce a
//! well-formed sentence about nothing. This is the same question one level
//! down, asked of code: **does this compile, run, pass, and do nothing?**
//!
//! It is the failure this whole codebase keeps hitting. A timing window
//! nothing filled. A board nothing added to. A vault function nothing called.
//! Every one of those compiled, every one had passing tests, and every one was
//! found by reading rather than by running. This is an attempt to find that
//! shape by reading, automatically, in anyone's code.
//!
//! ## Why it reads rather than parses
//!
//! No parser, no syntax tree, no language server. Those are per-language and
//! Eric will hand over whatever he happens to be looking at — a Go file from a
//! repo, a shell script, some JavaScript from a page. A real parser for each
//! is years of work and a wrong parse is worse than no parse.
//!
//! What is being looked for does not need one. A function whose whole body is
//! `pass`, an empty `catch`, a result assigned and never used — these are
//! visible in the shape of the text in every language that has them. Reading
//! for shape is shallow, and shallow is honest here: it finds the thing it
//! claims to find and says nothing about what it hasn't looked at.
//!
//! ## What it will not do
//!
//! It does not say the code is correct. It cannot: it has not run it, and it
//! does not know what it was supposed to do. Everything below is "this looks
//! like the shape of something unfinished" — a place to look, not a verdict.

use serde::{Deserialize, Serialize};

/// A language, as far as reading for shape needs to know.
///
/// Wider than `craft::Lang`, which only lists what Atlas can build and test.
/// This is only about recognising what is in front of it, and recognising far
/// more than you can build for is the honest position: Atlas can tell you
/// where a Go file looks unfinished without pretending it could compile one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tongue {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    CSharp,
    C,
    Cpp,
    Ruby,
    Php,
    Shell,
    Sql,
    Yaml,
    Unknown,
}

impl Tongue {
    pub fn plain(self) -> &'static str {
        match self {
            Tongue::Rust => "Rust",
            Tongue::Python => "Python",
            Tongue::JavaScript => "JavaScript",
            Tongue::TypeScript => "TypeScript",
            Tongue::Go => "Go",
            Tongue::Java => "Java",
            Tongue::CSharp => "C#",
            Tongue::C => "C",
            Tongue::Cpp => "C++",
            Tongue::Ruby => "Ruby",
            Tongue::Php => "PHP",
            Tongue::Shell => "a shell script",
            Tongue::Sql => "SQL",
            Tongue::Yaml => "YAML",
            Tongue::Unknown => "something I don't recognise",
        }
    }

    /// From a filename.
    pub fn of_name(name: &str) -> Tongue {
        let ext = name
            .rsplit('.')
            .next()
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "rs" => Tongue::Rust,
            "py" | "pyi" => Tongue::Python,
            "js" | "jsx" | "mjs" | "cjs" => Tongue::JavaScript,
            "ts" | "tsx" => Tongue::TypeScript,
            "go" => Tongue::Go,
            "java" => Tongue::Java,
            "cs" => Tongue::CSharp,
            "c" | "h" => Tongue::C,
            "cpp" | "cc" | "cxx" | "hpp" => Tongue::Cpp,
            "rb" => Tongue::Ruby,
            "php" => Tongue::Php,
            "sh" | "bash" | "zsh" => Tongue::Shell,
            "sql" => Tongue::Sql,
            "yaml" | "yml" => Tongue::Yaml,
            _ => Tongue::Unknown,
        }
    }

    /// From the code itself, for when it arrives pasted rather than as a file.
    ///
    /// Scored rather than first-match: a Rust file mentioning `function` in a
    /// comment should not become JavaScript because that word appeared once.
    pub fn of_code(code: &str) -> Tongue {
        let marks: &[(Tongue, &[&str])] = &[
            (Tongue::Rust, &["fn ", "let mut ", "impl ", "->", "pub fn", "use crate"]),
            (Tongue::Python, &["def ", "import ", "self.", "elif ", "__init__"]),
            (Tongue::TypeScript, &["interface ", ": string", ": number", "export type"]),
            (Tongue::JavaScript, &["function ", "const ", "=>", "console.log", "require("]),
            (Tongue::Go, &["func ", "package ", ":=", "import (", "nil"]),
            (Tongue::Java, &["public class", "void ", "import java", "System.out"]),
            (Tongue::CSharp, &["namespace ", "using System", "public class", "var "]),
            (Tongue::Cpp, &["#include", "std::", "template<", "::"]),
            (Tongue::C, &["#include", "int main", "printf(", "malloc("]),
            (Tongue::Ruby, &["def ", "end", "puts ", "require '"]),
            (Tongue::Php, &["<?php", "$this->", "echo ", "function "]),
            (Tongue::Shell, &["#!/bin/", "echo ", "fi", "esac", "$1"]),
            (Tongue::Sql, &["SELECT ", "INSERT INTO", "CREATE TABLE", "WHERE "]),
        ];
        let upper = code.to_uppercase();
        let mut best = (Tongue::Unknown, 0usize);
        for (tongue, tells) in marks {
            let score = tells
                .iter()
                .filter(|t| {
                    if t.chars().any(|c| c.is_uppercase()) {
                        upper.contains(&t.to_uppercase())
                    } else {
                        code.contains(*t)
                    }
                })
                .count();
            if score > best.1 {
                best = (*tongue, score);
            }
        }
        // One tell is a coincidence. `->` appears in a Python type hint, `::`
        // in a Ruby constant, `nil` in half a dozen languages.
        if best.1 < 2 {
            return Tongue::Unknown;
        }
        best.0
    }

    /// Both, with the filename winning when it says anything at all.
    ///
    /// An extension is a statement of intent; the contents are a guess. When
    /// they disagree, the disagreement is itself worth saying rather than
    /// silently picking one.
    pub fn of(name: &str, code: &str) -> Tongue {
        match Tongue::of_name(name) {
            Tongue::Unknown => Tongue::of_code(code),
            named => named,
        }
    }
}

/// The shape of something unfinished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    /// A body that does nothing: `pass`, `return null`, `{}`.
    DoesNothing,
    /// Says so itself: TODO, FIXME, unimplemented, not yet.
    SaysSoItself,
    /// An error caught and thrown away. The one that hurts most later,
    /// because the program carries on as though nothing happened.
    SwallowsTheError,
    /// A result worked out and then dropped on the floor.
    ThrowsAwayTheAnswer,
    /// Always gives the same answer whatever it is asked.
    AlwaysTheSame,
    /// Code that can never run: `if false`, a return above it.
    NeverRuns,
    /// A private function nothing in the file calls — built, and never used.
    /// Only private ones: a public one may be called from another file.
    NeverCalled,
    /// A public function nothing anywhere in the project calls
    /// (`never_called_in_project`): the cross-file half of `NeverCalled`.
    NeverCalledInProject,
    /// Imports a package the project never declares — named from memory,
    /// often by a model, and not something that's installed.
    MadeUpDependency,
}

impl Shape {
    /// What it means, said the way you would say it out loud.
    pub fn plain(self) -> &'static str {
        match self {
            Shape::DoesNothing => "does nothing at all",
            Shape::SaysSoItself => "says itself that it isn't finished",
            Shape::SwallowsTheError => {
                "catches an error and throws it away, so the program carries on \
                 as though nothing went wrong"
            }
            Shape::ThrowsAwayTheAnswer => "works something out and then ignores it",
            Shape::AlwaysTheSame => "gives the same answer whatever it's asked",
            Shape::NeverRuns => "can never run",
            Shape::NeverCalled => "is never called by anything in this file",
            Shape::NeverCalledInProject => "is never called by anything in the whole project",
            Shape::MadeUpDependency => "imports a package the project doesn't list",
        }
    }

    /// What to do about it, one line. From wshobson's `doc_gardener`: a
    /// finding with its fix beside it gets fixed; one without gets read.
    pub fn fix(self) -> &'static str {
        match self {
            Shape::DoesNothing => "write the body, or delete it and whatever calls it",
            Shape::SaysSoItself => "finish it, or file it somewhere it'll be seen and take the note out",
            Shape::SwallowsTheError => "handle the error, or pass it up — at least log what went wrong",
            Shape::ThrowsAwayTheAnswer => "use the result, or stop computing it",
            Shape::AlwaysTheSame => "make it depend on its input, or replace it with the constant",
            Shape::NeverRuns => "delete it, or fix the condition that keeps it from running",
            Shape::NeverCalled => "call it where it was meant to be used, or delete it",
            Shape::NeverCalledInProject => "wire it to whatever was meant to use it, or delete it",
            Shape::MadeUpDependency => "add it to the project's dependencies if it's real; if it isn't, replace the import",
        }
    }

    /// How much it matters. Not every shape is a bug.
    pub fn how_bad(self) -> &'static str {
        match self {
            // The whole reason this file exists. It passes every test that
            // checks it did not crash.
            Shape::SwallowsTheError => "worth fixing",
            Shape::DoesNothing | Shape::AlwaysTheSame => "worth looking at",
            Shape::NeverRuns | Shape::ThrowsAwayTheAnswer => "worth looking at",
            Shape::NeverCalled | Shape::NeverCalledInProject => "worth looking at",
            // Won't even build, or builds against something nobody chose.
            Shape::MadeUpDependency => "worth fixing",
            Shape::SaysSoItself => "already known",
        }
    }
}

/// One place worth looking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Line number, counting from one, the way an editor does.
    pub line: usize,
    pub shape: Shape,
    /// The line itself, trimmed, so it can be recognised without opening the
    /// file.
    pub code: String,
}

/// Bodies that mean "nothing here", per language.
const EMPTY_BODIES: &[&str] = &[
    "pass",
    "return null;",
    "return null",
    "return nil",
    "return none",
    "return;",
    "return {}",
    "return []",
    "return \"\"",
    "return 0;",
    "todo!()",
    "unimplemented!()",
    "raise notimplementederror",
    "throw new notimplementedexception();",
    "throw new error(\"not implemented\");",
    "panic(\"not implemented\")",
];

/// Says so itself.
const ADMITS: &[&str] = &[
    "todo", "fixme", "hack:", "xxx:", "not implemented", "unimplemented",
    "stub", "placeholder", "for now", "temporary",
];

/// A caught error going nowhere.
const SWALLOWS: &[&str] = &[
    "except: pass",
    "except exception: pass",
    "catch {}",
    "catch (e) {}",
    "catch {  }",
    "} catch (ignored) {",
    ".unwrap_or_default()",
    "let _ =",
    "_ = err",
    "if err != nil {}",
    "rescue nil",
    "on error resume next",
];

/// Read a piece of code for the shape of something unfinished.
///
/// One pass, line by line. Deliberately not clever: a check that guesses at
/// intent flags ordinary code, and a tool that cries wolf gets switched off
/// after the second time — which is worse than not having it, because now
/// nobody is looking at all.
pub fn read(code: &str, tongue: Tongue) -> Vec<Finding> {
    let mut found = Vec::new();
    let lines: Vec<&str> = code.lines().collect();

    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let lower = line.to_lowercase();
        if line.is_empty() {
            continue;
        }
        let at = i + 1;
        let note = |shape| Finding { line: at, shape, code: line.to_string() };

        // Says so itself, but only in a comment or a marker — the word "todo"
        // inside a string that a program prints to a user is that program
        // working, not that program unfinished.
        if is_comment(line, tongue) && ADMITS.iter().any(|a| lower.contains(a)) {
            found.push(note(Shape::SaysSoItself));
            continue;
        }

        // An error caught and dropped.
        if SWALLOWS.iter().any(|p| lower.replace(' ', "").contains(&p.replace(' ', ""))) {
            found.push(note(Shape::SwallowsTheError));
            continue;
        }

        // A body that does nothing. Only counted when it is the whole of a
        // body — a bare `return null` inside a longer function is often
        // exactly right.
        if EMPTY_BODIES.contains(&lower.trim_end_matches(&[';', ' '][..]))
            && is_only_body(&lines, i)
        {
            found.push(note(Shape::DoesNothing));
            continue;
        }

        // Never runs.
        if lower.starts_with("if false")
            || lower.starts_with("if (false")
            || lower.starts_with("if 0 ")
            || lower.starts_with("while false")
        {
            found.push(note(Shape::NeverRuns));
            continue;
        }
    }

    found.extend(never_called(&lines, tongue));
    found.sort_by_key(|f| f.line);
    found
}

/// Private functions the file defines and never mentions again.
///
/// "Private" by each language's own marker, because a public function is
/// allowed to be called from somewhere this file can't see: Rust without
/// `pub`, Python with a leading underscore, Go with a lower-case first
/// letter, JavaScript/TypeScript not exported. Test functions and entry
/// points are left alone.
fn never_called(lines: &[&str], tongue: Tongue) -> Vec<Finding> {
    let mut defs: Vec<(usize, String, String)> = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let name_after = |prefix: &str| -> Option<String> {
            let rest = line.strip_prefix(prefix)?;
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            (!name.is_empty()).then_some(name)
        };
        let found = match tongue {
            Tongue::Rust => {
                let before_test = i > 0 && lines[i - 1].trim().starts_with("#[");
                if before_test || in_trait_impl(lines, i) { None } else { name_after("fn ") }
            }
            Tongue::Python => name_after("def _").map(|n| format!("_{n}")).filter(|n| !n.starts_with("__")),
            Tongue::Go => name_after("func ").filter(|n| n.chars().next().is_some_and(|c| c.is_lowercase())),
            Tongue::JavaScript | Tongue::TypeScript => name_after("function "),
            _ => None,
        };
        if let Some(name) = found {
            if ["main", "init", "new", "default", "drop", "fmt", "from"].contains(&name.as_str()) || name.starts_with("test") {
                continue;
            }
            defs.push((i + 1, name, line.to_string()));
        }
    }
    let whole = lines.join("\n");
    defs.into_iter()
        .filter(|(line, name, _)| {
            // Mentioned anywhere other than its own definition line — a call,
            // a reference passed along, an export list.
            let used = whole
                .lines()
                .enumerate()
                .filter(|(i, _)| i + 1 != *line)
                .any(|(_, l)| mentions(l, name));
            !used
        })
        .map(|(line, _, code)| Finding { line, shape: Shape::NeverCalled, code })
        .collect()
}

/// Is line `i` inside `impl Trait for Type { … }`? Methods there are
/// called through the trait, from anywhere — never "unused" by this file.
fn in_trait_impl(lines: &[&str], i: usize) -> bool {
    let indent = |s: &str| s.len() - s.trim_start().len();
    let here = indent(lines[i]);
    lines[..i]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty() && indent(l) < here)
        .is_some_and(|l| {
            let t = l.trim_start();
            (t.starts_with("impl") || t.starts_with("unsafe impl")) && t.contains(" for ")
        })
}

/// `name` appears in `line` as a whole word.
fn mentions(line: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(i) = line[from..].find(name) {
        let at = from + i;
        let before = line[..at].chars().next_back();
        let after = line[at + name.len()..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word(before) && !word(after) {
            return true;
        }
        from = at + name.len();
    }
    false
}

/// Public functions this file defines that nothing in its project mentions
/// -- not this file past the definition, not any other source file of the
/// same language.
///
/// Research report, 30 Sep 2026, Stage 2 item 17: `never_called` only looked
/// at private functions, because a public one "may be called from another
/// file" -- and nothing looked at the other files, so a public function
/// called from nowhere (the shape this codebase kept finding in itself) was
/// never found in anyone's code. The project is the folder holding the
/// nearest manifest, else the file's own folder; at most `PROJECT_FILES`
/// files are read. Entry points and tests are left alone, as in-file.
pub fn never_called_in_project(file: &std::path::Path, code: &str, tongue: Tongue) -> Vec<Finding> {
    let lines: Vec<&str> = code.lines().collect();
    let mut defs: Vec<(usize, String, String)> = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let name_after = |prefix: &str| -> Option<String> {
            let rest = line.strip_prefix(prefix)?;
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            (!name.is_empty()).then_some(name)
        };
        let found = match tongue {
            Tongue::Rust => name_after("pub fn ").or_else(|| name_after("pub(crate) fn ")),
            Tongue::Python => name_after("def ").filter(|n| !n.starts_with('_')),
            Tongue::Go => name_after("func ").filter(|n| n.chars().next().is_some_and(|c| c.is_uppercase())),
            Tongue::JavaScript | Tongue::TypeScript => name_after("export function ").or_else(|| name_after("export async function ")),
            _ => None,
        };
        if let Some(name) = found {
            if ["main", "init", "new", "default", "setup", "handler"].contains(&name.as_str()) || name.starts_with("test") {
                continue;
            }
            defs.push((i + 1, name, line.to_string()));
        }
    }
    if defs.is_empty() {
        return Vec::new();
    }
    // Mentioned in this file past its own definition?
    defs.retain(|(line, name, _)| !lines.iter().enumerate().any(|(i, l)| i + 1 != *line && mentions(l, name)));
    if defs.is_empty() {
        return Vec::new();
    }
    let root = project_root(file);
    let mine = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut others = Vec::new();
    collect_sources(&root, tongue, &mut others, 0);
    let mut unseen: Vec<(usize, String, String)> = defs;
    for p in others {
        if unseen.is_empty() {
            break;
        }
        if std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone()) == mine {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        unseen.retain(|(_, name, _)| !text.lines().any(|l| mentions(l, name)));
    }
    unseen.into_iter().map(|(line, _, code)| Finding { line, shape: Shape::NeverCalledInProject, code }).collect()
}

/// The most source files read for `never_called_in_project`.
pub const PROJECT_FILES: usize = 400;

fn project_root(file: &std::path::Path) -> std::path::PathBuf {
    let mut dir = file.parent();
    for _ in 0..4 {
        let Some(d) = dir else { break };
        if ["Cargo.toml", "package.json", "pyproject.toml", "requirements.txt", "go.mod"].iter().any(|m| d.join(m).is_file()) {
            return d.to_path_buf();
        }
        dir = d.parent();
    }
    file.parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

fn collect_sources(dir: &std::path::Path, tongue: Tongue, out: &mut Vec<std::path::PathBuf>, depth: usize) {
    if depth > 8 || out.len() >= PROJECT_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || ["target", "node_modules", "__pycache__", "dist", "build", "vendor"].contains(&name.as_str()) {
            continue;
        }
        if p.is_dir() {
            collect_sources(&p, tongue, out, depth + 1);
        } else if Tongue::of_name(&name) == tongue && e.metadata().map(|m| m.len() <= 512 * 1024).unwrap_or(false) {
            out.push(p);
            if out.len() >= PROJECT_FILES {
                return;
            }
        }
    }
}

/// A project's list of what it depends on, found near a file: the nearest
/// `Cargo.toml`, `requirements.txt`, `pyproject.toml` or `package.json` in
/// its folder or up to three above it.
pub fn manifest_near(file: &std::path::Path) -> Option<String> {
    let mut dir = file.parent();
    for _ in 0..4 {
        let d = dir?;
        for name in ["Cargo.toml", "requirements.txt", "pyproject.toml", "package.json"] {
            if let Ok(text) = std::fs::read_to_string(d.join(name)) {
                return Some(text);
            }
        }
        dir = d.parent();
    }
    None
}

/// Imports of packages the project doesn't list. Only asked with a manifest
/// in hand: without one there's nothing to check against, and guessing which
/// names are real is exactly the mistake this is looking for.
pub fn made_up_dependencies(code: &str, tongue: Tongue, manifest: &str) -> Vec<Finding> {
    let listed = manifest.to_lowercase().replace('-', "_");
    let known = |pkg: &str| -> bool {
        let p = pkg.to_lowercase().replace('-', "_");
        listed.contains(&p)
    };
    let mut out = Vec::new();
    for (i, raw) in code.lines().enumerate() {
        let line = raw.trim();
        let pkg: Option<String> = match tongue {
            Tongue::Rust => line
                .strip_prefix("use ")
                .or_else(|| line.strip_prefix("pub use "))
                .or_else(|| line.strip_prefix("extern crate "))
                .map(|r| r.split([':', ';', ' ', '{']).next().unwrap_or("").to_string())
                .filter(|p| !p.is_empty() && !["std", "core", "alloc", "crate", "self", "super"].contains(&p.as_str())),
            Tongue::Python => {
                let first = line
                    .strip_prefix("import ")
                    .or_else(|| line.strip_prefix("from "))
                    .map(|r| r.split(['.', ' ', ',']).next().unwrap_or("").to_string());
                first.filter(|p| !p.is_empty() && !PYTHON_STDLIB.contains(&p.as_str()))
            }
            Tongue::JavaScript | Tongue::TypeScript => {
                let quoted = line
                    .find("from '")
                    .or_else(|| line.find("from \""))
                    .map(|i| &line[i + 6..])
                    .or_else(|| line.find("require('").or_else(|| line.find("require(\"")).map(|i| &line[i + 9..]));
                quoted
                    .map(|q| q.split(['\'', '"']).next().unwrap_or("").to_string())
                    .filter(|p| !p.is_empty() && !p.starts_with('.') && !p.starts_with('/') && !p.starts_with("node:"))
                    .map(|p| {
                        // "@scope/pkg/sub" -> "@scope/pkg"; "pkg/sub" -> "pkg".
                        let mut parts = p.split('/');
                        match (parts.next(), parts.next()) {
                            (Some(s), Some(n)) if s.starts_with('@') => format!("{s}/{n}"),
                            (Some(s), _) => s.to_string(),
                            _ => p,
                        }
                    })
                    .filter(|p| !NODE_BUILTINS.contains(&p.as_str()))
            }
            _ => None,
        };
        if let Some(p) = pkg {
            if !known(&p) {
                out.push(Finding { line: i + 1, shape: Shape::MadeUpDependency, code: line.to_string() });
            }
        }
    }
    out
}

/// Python's own modules — the ones no project lists because they come with it.
const PYTHON_STDLIB: &[&str] = &[
    "__future__", "abc", "argparse", "array", "ast", "asyncio", "base64", "bisect", "builtins", "calendar",
    "collections", "concurrent", "contextlib", "copy", "csv", "ctypes", "dataclasses", "datetime", "decimal",
    "difflib", "enum", "errno", "fnmatch", "fractions", "functools", "gc", "getpass", "glob", "gzip", "hashlib",
    "heapq", "hmac", "html", "http", "importlib", "inspect", "io", "ipaddress", "itertools", "json", "logging",
    "math", "mimetypes", "multiprocessing", "operator", "os", "pathlib", "pickle", "platform", "pprint",
    "queue", "random", "re", "secrets", "select", "shlex", "shutil", "signal", "socket", "sqlite3", "ssl",
    "statistics", "string", "struct", "subprocess", "sys", "tempfile", "textwrap", "threading", "time",
    "timeit", "traceback", "types", "typing", "unittest", "urllib", "uuid", "warnings", "weakref", "xml",
    "zipfile", "zlib", "zoneinfo",
];

/// Node's own modules.
const NODE_BUILTINS: &[&str] = &[
    "assert", "buffer", "child_process", "crypto", "dns", "events", "fs", "http", "https", "net", "os", "path",
    "process", "querystring", "readline", "stream", "string_decoder", "timers", "tls", "url", "util", "zlib",
];

/// Is this line a comment?
fn is_comment(line: &str, tongue: Tongue) -> bool {
    let starts = match tongue {
        Tongue::Python | Tongue::Ruby | Tongue::Shell | Tongue::Yaml => &["#"][..],
        Tongue::Sql => &["--"][..],
        Tongue::Unknown => &["//", "#", "--", "/*", "*"][..],
        _ => &["//", "/*", "*"][..],
    };
    starts.iter().any(|s| line.starts_with(s))
}

/// Is this line the entire body of whatever it sits in?
///
/// Approximate, by indentation and braces. Getting this wrong in the cautious
/// direction means missing a finding; getting it wrong the other way means
/// flagging correct code, and one of those makes the tool useless.
fn is_only_body(lines: &[&str], i: usize) -> bool {
    let indent = |s: &str| s.len() - s.trim_start().len();
    let here = indent(lines[i]);

    let before = lines[..i]
        .iter()
        .rev()
        .find(|l| !l.trim().is_empty() && !l.trim().starts_with("//") && !l.trim().starts_with('#'));
    let after = lines[i + 1..]
        .iter()
        .find(|l| !l.trim().is_empty());

    // Opens directly above — and opens a *function*, not a branch.
    //
    // This is the line that decides whether the tool is usable. `return nil`
    // under `if x < 0 {` is a guard clause and completely correct; the same
    // line under `func Save() error {` is a function that does nothing. Told
    // apart by what opened the block, because flagging every guard clause in a
    // codebase is how a tool gets switched off on its first run.
    let control = [
        "if ", "if(", "else", "while ", "while(", "for ", "for(", "switch",
        "case ", "match ", "loop", "try", "catch", "except", "with ", "do ",
        "when ", "unless ",
    ];
    let opened_above = before.is_some_and(|b| {
        let t = b.trim_end();
        let opens = t.ends_with('{') || t.ends_with(':') || indent(b) < here;
        let head = t.trim_start().to_lowercase();
        opens && !control.iter().any(|c| head.starts_with(c))
    });
    // Closes directly below, or the file ends.
    let closed_below = match after {
        None => true,
        Some(a) => {
            let t = a.trim();
            t == "}" || t.starts_with('}') || indent(a) < here
        }
    };
    opened_above && closed_below
}

/// What to say about a file.
///
/// Leads with the language, because "I can see this is Go" is the first thing
/// worth knowing when you have handed over a file without saying what it is.
pub fn spoken(name: &str, tongue: Tongue, found: &[Finding]) -> String {
    let what = if name.trim().is_empty() {
        format!("That's {}.", tongue.plain())
    } else {
        format!("{name} is {}.", tongue.plain())
    };

    if found.is_empty() {
        return format!(
            "{what} Nothing in it looks unfinished to me — though I've read it \
             for shape rather than run it, so that's not the same as saying \
             it's right."
        );
    }

    let mut worst: Vec<&Finding> = found
        .iter()
        .filter(|f| f.shape.how_bad() == "worth fixing")
        .collect();
    if worst.is_empty() {
        worst = found.iter().take(4).collect();
    }

    let mut out = format!(
        "{what} {} place{} worth looking at.\n",
        found.len(),
        if found.len() == 1 { "" } else { "s" }
    );
    for f in worst.iter().take(6) {
        out.push_str(&format!("Line {}: {} — {}\n  Fix: {}\n", f.line, f.shape.plain(), f.code, f.shape.fix()));
    }
    out.push_str(
        "I read it for shape rather than running it, so this is where to look, \
         not a verdict.",
    );
    out
}

/// What it would take to make this work inside Atlas.
///
/// Asked because Eric asked it directly: hand Atlas some Python, can it turn
/// that into something Atlas itself runs? The honest answer is no, and saying
/// so is worth more than a translation that compiles and quietly means
/// something else.
///
/// What is real: Atlas can tell you what it is, what it appears to do, what it
/// depends on, and which parts have no equivalent on this side — which is the
/// list you would need anyway before rewriting it yourself.
pub fn porting_notes(tongue: Tongue, code: &str) -> String {
    if tongue == Tongue::Rust {
        return "That's already Rust — I can work on it directly rather than \
                port it."
            .into();
    }
    let mut hard = Vec::new();
    let lower = code.to_lowercase();
    for (needle, why) in [
        ("import ", "it pulls in libraries that would each need a Rust equivalent"),
        ("require(", "it pulls in packages that would each need a Rust equivalent"),
        ("async ", "it's asynchronous, which is a different shape here"),
        ("class ", "it's built around classes, which don't map across directly"),
        ("eval(", "it builds code at runtime, which has no equivalent here"),
        ("global ", "it uses shared mutable state, which is the hard part of any port"),
    ] {
        if lower.contains(needle) {
            hard.push(why);
        }
    }

    let mut out = format!(
        "That's {}. I can read it and tell you what it does, and where it looks \
         unfinished. I can't turn it into working Rust — a translation that \
         compiles and quietly means something else is worse than no translation, \
         and I'd have no way to tell you which one you got.",
        tongue.plain()
    );
    if !hard.is_empty() {
        out.push_str("\n\nIf you're porting it by hand, these are the parts that won't come across cleanly:");
        for h in hard {
            out.push_str(&format!("\n- {h}"));
        }
    }
    out
}

#[cfg(test)]
mod across_files {
    use super::*;

    #[test]
    fn a_public_function_nothing_in_the_project_calls_is_found_and_one_that_is_called_isnt() {
        let d = std::env::temp_dir().join(format!("atlas-hollow-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let lib = "pub fn used_elsewhere() -> u32 { 1 }\npub fn called_by_nobody() -> u32 { 2 }\n";
        std::fs::write(d.join("src/lib.rs"), lib).unwrap();
        std::fs::write(d.join("src/main.rs"), "fn main() { println!(\"{}\", x::used_elsewhere()); }\n").unwrap();
        let found = never_called_in_project(&d.join("src/lib.rs"), lib, Tongue::Rust);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].line, 2);
        assert_eq!(found[0].shape, Shape::NeverCalledInProject);
    }
}

