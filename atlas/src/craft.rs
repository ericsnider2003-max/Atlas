//! Building your things, offline.
//!
//! The instinct for "make it better at coding" is a bigger model, and on this
//! machine that instinct has nowhere to go. A model that fits alongside
//! everything else is not going to write good Rust from a rough description,
//! and pretending otherwise produces confident code that does not compile.
//!
//! But generation is not where a local setup loses. It loses on the loop
//! around the generation, and that is entirely fixable, because the strongest
//! signal available is free, local, and instant: **the toolchain already knows
//! whether the code is right.**
//!
//! Two ideas do the work here.
//!
//! **Cheapest signal first.** A type error found by `cargo check` in two
//! seconds is the same error a test suite finds in forty, except it arrives
//! with a line number and a suggestion. A weak model iterating against precise
//! compiler errors converges. The same model iterating against test output
//! flails, because test output describes a symptom and a compiler describes
//! the cause.
//!
//! **A failed gate makes the later ones meaningless.** Running tests on code
//! that does not compile produces noise, and noise is worse than nothing —
//! it gives the next attempt something confident and wrong to work from.
//!
//! Everything here runs with no network. That is not a compromise: for this
//! job the local tools are the authority, and a model somewhere else is the
//! thing guessing.

use serde::{Deserialize, Serialize};

/// A language Atlas knows how to *check* — not merely generate.
///
/// The distinction is the whole point of this file. A model can emit text in
/// any language; what makes that text trustworthy offline is a proof ladder
/// (compile → lint → test) run by the language's own toolchain. A language is
/// in this enum when it has such a ladder here, which is a stronger claim than
/// "Atlas can write some". Languages without a standard project toolchain are
/// generated but don't get this guarantee, and saying so is the honest line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    Rust,
    Python,
    Go,
    JavaScript,
    TypeScript,
    /// C++, checked with LLVM's tools (clang++, clang-tidy, clang-format;
    /// research report item 27, 1 Oct 2026).
    Cpp,
}

impl Lang {
    /// Guess from a path. Used to pick a ladder, never to decide anything
    /// destructive.
    pub fn of_path(path: &str) -> Option<Lang> {
        let p = path.to_lowercase();
        if p.ends_with(".rs") {
            Some(Lang::Rust)
        } else if p.ends_with(".py") || p.ends_with(".pyi") {
            Some(Lang::Python)
        } else if p.ends_with(".go") {
            Some(Lang::Go)
        } else if p.ends_with(".ts") || p.ends_with(".tsx") {
            Some(Lang::TypeScript)
        } else if p.ends_with(".js") || p.ends_with(".mjs") || p.ends_with(".cjs") || p.ends_with(".jsx") {
            Some(Lang::JavaScript)
        } else if [".cpp", ".cc", ".cxx", ".hpp", ".hh", ".hxx", ".h"].iter().any(|e| p.ends_with(e)) {
            Some(Lang::Cpp)
        } else {
            None
        }
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Lang::Rust => "Rust",
            Lang::Python => "Python",
            Lang::Go => "Go",
            Lang::JavaScript => "JavaScript",
            Lang::TypeScript => "TypeScript",
            Lang::Cpp => "C++",
        }
    }

    /// The file extension a single-file draft of this language is written to.
    pub fn ext(&self) -> &'static str {
        match self {
            Lang::Rust => "rs",
            Lang::Python => "py",
            Lang::Go => "go",
            Lang::JavaScript => "js",
            Lang::TypeScript => "ts",
            Lang::Cpp => "cpp",
        }
    }

    /// The files to lay down so the toolchain has something to act on, for a
    /// single-unit draft of `code`. Kept here, beside the ladder, because the
    /// scaffold and the ladder's commands have to agree — a `tsc --noEmit`
    /// gate needs a `tsconfig.json`, a `go build ./...` needs a `go.mod` — and
    /// splitting that knowledge across files is how they drift apart.
    pub fn draft_files(&self, code: &str) -> Vec<(String, String)> {
        match self {
            Lang::Rust => vec![
                (
                    "Cargo.toml".into(),
                    "[package]\nname = \"build\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n".into(),
                ),
                ("src/lib.rs".into(), code.to_string()),
            ],
            // A test that imports the draft: pytest with no tests exits 5
            // ("no tests collected") and failed every Python build, so a
            // Python build could never be Built (report gap 4). Importing it
            // is the least a working module does -- it proves it loads.
            Lang::Python => vec![("main.py".into(), code.to_string()), ("test_main.py".into(), PY_SMOKE.into())],
            Lang::Go => vec![
                ("go.mod".into(), "module build\n\ngo 1.21\n".into()),
                ("main.go".into(), code.to_string()),
            ],
            // `npm test` with no test script always failed (report gap 4):
            // the script is node's own runner, and the test loads the draft.
            Lang::JavaScript => vec![
                (
                    "package.json".into(),
                    "{\n  \"name\": \"build\",\n  \"version\": \"0.0.0\",\n  \"scripts\": { \"test\": \"node --test\" }\n}\n".into(),
                ),
                ("main.js".into(), code.to_string()),
                ("main.test.js".into(), JS_SMOKE.into()),
            ],
            Lang::TypeScript => vec![
                (
                    "tsconfig.json".into(),
                    "{\n  \"compilerOptions\": { \"noEmit\": true, \"strict\": true },\n  \"files\": [\"main.ts\"]\n}\n".into(),
                ),
                ("main.ts".into(), code.to_string()),
                // node 24 runs TypeScript itself: its test runner loads the
                // draft, as for JavaScript (`npm test` with no package.json
                // failed every TypeScript build).
                (
                    "package.json".into(),
                    "{\n  \"name\": \"build\",\n  \"version\": \"0.0.0\",\n  \"type\": \"module\",\n  \"scripts\": { \"test\": \"node --test\" }\n}\n".into(),
                ),
                ("main.test.ts".into(), TS_SMOKE.into()),
            ],
            Lang::Cpp => vec![("main.cpp".into(), code.to_string())],
        }
    }
}

/// The Python draft's test: it loads.
const PY_SMOKE: &str = "import importlib\n\n\ndef test_it_loads():\n    importlib.import_module(\"main\")\n";

/// The JavaScript draft's test: it loads.
const JS_SMOKE: &str = "const test = require('node:test');\n\ntest('it loads', () => {\n  require('./main.js');\n});\n";

/// The TypeScript draft's test: it loads.
const TS_SMOKE: &str = "import { test } from 'node:test';\n\ntest('it loads', async () => {\n  await import('./main.ts');\n});\n";

/// The program a gate names, made absolute when it is a file the gate itself
/// built in `dir` (`atlas-check.exe`): Windows looks for a bare program name
/// beside the *parent* process and in its folder, never in the folder a
/// child is started in, so the C++ run gate said "isn't installed" for the
/// program it had just built (the laptop, 1 Oct 2026).
pub fn program_in(dir: &std::path::Path, program: &str) -> String {
    let local = dir.join(program.trim_start_matches("./").trim_start_matches(".\\"));
    if program == cpp_program() && local.is_file() {
        local.to_string_lossy().into_owned()
    } else {
        program.to_string()
    }
}

/// What the C++ draft is built to, and run as, for its Behaviour gate.
fn cpp_program() -> &'static str {
    if cfg!(windows) { "atlas-check.exe" } else { "./atlas-check" }
}

/// What a check tells you, which decides where it sits in the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tells {
    /// Whitespace and layout. Never blocks anything; fixed automatically.
    Shape,
    /// Does it parse and do the types line up. The most useful error a machine
    /// can give you, and the fastest.
    Sound,
    /// Would a careful reviewer object. Real, but not a reason to stop.
    Style,
    /// Does it do what it is meant to. The only one that answers the question,
    /// and the slowest and vaguest when it fails.
    Behaviour,
}

impl Tells {
    /// Does a failure here make later checks worth running?
    ///
    /// A style complaint does not stop the tests being meaningful. A syntax
    /// error does.
    pub fn blocks_later(&self) -> bool {
        matches!(self, Tells::Sound)
    }
}

/// One command and what its result means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    pub tells: Tells,
    pub command: String,
    /// Roughly how long, so the ladder can be ordered without timing it.
    pub seconds: u32,
    /// What to say when it fails, in your words rather than the tool's.
    pub on_fail: String,
}

/// The checks for a language, cheapest and most informative first.
///
/// Ordering is the whole design. It is not "run everything and report"; it is
/// "find the first thing that is wrong, because fixing it changes what
/// everything after it says".
pub fn ladder(lang: Lang) -> Vec<Gate> {
    match lang {
        Lang::Rust => vec![
            Gate {
                tells: Tells::Shape,
                command: "cargo fmt".into(),
                seconds: 1,
                on_fail: "formatting only — fixed it and carried on".into(),
            },
            Gate {
                tells: Tells::Sound,
                command: "cargo check --all-targets".into(),
                seconds: 5,
                on_fail: "it doesn't compile yet".into(),
            },
            Gate {
                tells: Tells::Style,
                command: "cargo clippy --all-targets -- -D warnings".into(),
                seconds: 15,
                on_fail: "it compiles, but there are things worth changing".into(),
            },
            Gate {
                tells: Tells::Behaviour,
                command: "cargo test".into(),
                seconds: 60,
                on_fail: "it builds and doesn't do the right thing yet".into(),
            },
        ],
        Lang::Python => vec![
            Gate {
                tells: Tells::Shape,
                command: "ruff format .".into(),
                seconds: 1,
                on_fail: "formatting only — fixed it and carried on".into(),
            },
            Gate {
                // Python has no compiler, so the type checker plays that part.
                // Without it the first real signal would be a test failure,
                // which is the vaguest and slowest one — the exact problem the
                // ladder exists to avoid.
                tells: Tells::Sound,
                command: "python -m compileall -q .".into(),
                seconds: 3,
                on_fail: "it doesn't parse yet".into(),
            },
            Gate {
                tells: Tells::Sound,
                command: "mypy .".into(),
                seconds: 20,
                on_fail: "the types don't line up".into(),
            },
            Gate {
                tells: Tells::Style,
                command: "ruff check .".into(),
                seconds: 3,
                on_fail: "it runs, but there are things worth changing".into(),
            },
            Gate {
                tells: Tells::Behaviour,
                command: "pytest -q".into(),
                seconds: 45,
                on_fail: "it runs and doesn't do the right thing yet".into(),
            },
        ],
        // Go's toolchain is the cleanest of the lot: format, build, vet and
        // test are all one command each and all project-wide, so the ladder is
        // almost a transcription of `go help`.
        Lang::Go => vec![
            Gate {
                tells: Tells::Shape,
                command: "gofmt -w .".into(),
                seconds: 1,
                on_fail: "formatting only — fixed it and carried on".into(),
            },
            Gate {
                tells: Tells::Sound,
                command: "go build ./...".into(),
                seconds: 10,
                on_fail: "it doesn't compile yet".into(),
            },
            Gate {
                tells: Tells::Style,
                command: "go vet ./...".into(),
                seconds: 10,
                on_fail: "it builds, but vet has something worth changing".into(),
            },
            Gate {
                tells: Tells::Behaviour,
                command: "go test ./...".into(),
                seconds: 45,
                on_fail: "it builds and doesn't do the right thing yet".into(),
            },
        ],
        // JavaScript has no compiler, and no project-wide syntax check the way
        // the typed languages do — `node --check` reads one file. So the Sound
        // gate here is genuinely weaker than Rust's or Go's, and that is the
        // honest shape of the language rather than a gap to paper over.
        Lang::JavaScript => vec![
            Gate {
                tells: Tells::Shape,
                command: "prettier --write .".into(),
                seconds: 1,
                on_fail: "formatting only — fixed it and carried on".into(),
            },
            Gate {
                tells: Tells::Sound,
                command: "node --check main.js".into(),
                seconds: 2,
                on_fail: "it doesn't parse yet".into(),
            },
            Gate {
                tells: Tells::Behaviour,
                command: "npm test".into(),
                seconds: 45,
                on_fail: "it runs and doesn't do the right thing yet".into(),
            },
        ],
        // TypeScript earns back what JavaScript lacks: `tsc --noEmit` is a real
        // project-wide compiler pass, so the Sound gate is as strong as the
        // typed languages'.
        Lang::TypeScript => vec![
            Gate {
                tells: Tells::Shape,
                command: "prettier --write .".into(),
                seconds: 1,
                on_fail: "formatting only — fixed it and carried on".into(),
            },
            Gate {
                tells: Tells::Sound,
                command: "tsc --noEmit".into(),
                seconds: 15,
                on_fail: "the types don't line up yet".into(),
            },
            // No eslint gate (1 Oct 2026): ESLint since v9 refuses to run
            // without a config file, and can't read TypeScript without a
            // separate parser package -- on a fresh draft it could only ever
            // fail. `tsc --strict` is the check that earns its place.
            Gate {
                tells: Tells::Behaviour,
                command: "npm test".into(),
                seconds: 45,
                on_fail: "it runs and doesn't do the right thing yet".into(),
            },
        ],
        // C++ (1 Oct 2026, report item 27): LLVM's tools, which the
        // official Windows installer ships together. The gate runner splits
        // on spaces and has no `&&`, so building and running are two gates;
        // a draft that builds and then crashes or returns non-zero fails
        // the second.
        Lang::Cpp => vec![
            Gate {
                tells: Tells::Shape,
                command: "clang-format -i main.cpp".into(),
                seconds: 1,
                on_fail: "formatting only — fixed it and carried on".into(),
            },
            Gate {
                tells: Tells::Sound,
                command: "clang++ -std=c++20 -Wall -fsyntax-only main.cpp".into(),
                seconds: 5,
                on_fail: "it doesn't compile yet".into(),
            },
            Gate {
                tells: Tells::Style,
                // Checks named: llvm-mingw's clang-tidy has none on by default
                // and refuses ("no checks enabled"), measured on the laptop 1
                // Oct 2026.
                command: "clang-tidy --checks=-*,bugprone-*,performance-*,portability-* --warnings-as-errors=* main.cpp -- -std=c++20".into(),
                seconds: 15,
                on_fail: "it compiles, but there are things worth changing".into(),
            },
            Gate {
                tells: Tells::Sound,
                command: format!("clang++ -std=c++20 -O1 -g main.cpp -o {}", cpp_program().trim_start_matches("./")),
                seconds: 15,
                on_fail: "it doesn't link yet".into(),
            },
            Gate {
                tells: Tells::Behaviour,
                command: cpp_program().into(),
                seconds: 10,
                on_fail: "it builds, and running it failed".into(),
            },
        ],
    }
}

/// The program a gate's command starts, when the gate couldn't start it
/// because it isn't installed: `Sandbox::run` says "could not start <cmd>",
/// and a shell says "not found" / "is not recognized".
fn missing_program(r: &Ran) -> Option<String> {
    let program = r.command.split_whitespace().next()?.to_string();
    let out = r.output.to_lowercase();
    // "couldn't run": `sandbox::run_within`'s words for a program that
    // wouldn't start, which the checks now run through (2 Oct 2026).
    let missing = out.starts_with("could not start")
        || out.starts_with("couldn't run")
        || out.contains("is not recognized as an internal or external command")
        || (out.contains("not found") && out.contains(&program.to_lowercase()) && out.lines().count() <= 2);
    missing.then_some(program)
}

/// How a gate went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ran {
    pub command: String,
    pub tells: Tells,
    pub passed: bool,
    /// The tool's own words, trimmed. Kept verbatim, because a paraphrased
    /// compiler error loses the line number, which is the useful part.
    pub output: String,
}

/// What to do next after running the ladder as far as it got.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Everything passed.
    Good,
    /// Fix this before anything else.
    Fix { gate: Gate, output: String },
    /// It works; these are worth a look but are not blocking.
    WorksWithNotes(Vec<String>),
    /// A check that decides whether it works couldn't run: its program isn't
    /// on this computer. Not a fault in the code, so never handed to the
    /// model to "fix" (report gap 2).
    CannotCheck { program: String, gate: Gate },
}

/// Walk the ladder and stop at the first thing that makes the rest meaningless.
///
/// Takes the results rather than running anything, so the decision is testable
/// without a toolchain and the running lives in one place.
pub fn read_ladder(lang: Lang, ran: &[Ran]) -> Next {
    let gates = ladder(lang);
    // The first failure that blocks. Ordered by the ladder, not by the order
    // things happen to have been run in. A blocking or behaviour check whose
    // program isn't here is "can't check", not "fix it".
    for g in &gates {
        if let Some(r) = ran.iter().find(|r| r.command == g.command) {
            if !r.passed && (r.tells.blocks_later() || r.tells == Tells::Behaviour) {
                if let Some(program) = missing_program(r) {
                    return Next::CannotCheck { program, gate: g.clone() };
                }
            }
            if !r.passed && r.tells.blocks_later() {
                return Next::Fix { gate: g.clone(), output: r.output.clone() };
            }
        }
    }
    // Then behaviour, which is not "blocking" in the sense above but is the
    // thing you actually asked for.
    for g in gates.iter().filter(|g| g.tells == Tells::Behaviour) {
        if let Some(r) = ran.iter().find(|r| r.command == g.command) {
            if !r.passed {
                return Next::Fix { gate: g.clone(), output: r.output.clone() };
            }
        }
    }
    let notes: Vec<String> = ran
        .iter()
        .filter(|r| !r.passed && r.tells == Tells::Style)
        .filter(|r| missing_program(r).is_none())
        .map(|r| r.output.clone())
        .collect();
    if notes.is_empty() {
        Next::Good
    } else {
        Next::WorksWithNotes(notes)
    }
}

/// Which gates are worth running, given what already failed.
///
/// Saves the forty seconds of test output that would be produced by code that
/// does not compile — and, more importantly, stops that output existing at
/// all, because a next attempt working from it is working from noise.
pub fn still_worth_running(lang: Lang, ran: &[Ran]) -> Vec<Gate> {
    let blocked = ran
        .iter()
        .any(|r| !r.passed && r.tells.blocks_later());
    ladder(lang)
        .into_iter()
        .filter(|g| !ran.iter().any(|r| r.command == g.command))
        .filter(|g| !blocked || g.tells <= Tells::Sound)
        .collect()
}

// ---------------------------------------------------------------------------
// Turning a rough idea into something buildable
// ---------------------------------------------------------------------------

/// The questions worth asking before writing anything.
///
/// Not a template to fill in. Each one exists because the answer changes the
/// code, and because these are the things people leave out of a description
/// and then are surprised about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// What does it do when it works.
    Shape,
    /// What is it allowed to assume about its input.
    Inputs,
    /// What should happen when that assumption is wrong.
    WhenItGoesWrong,
    /// What must it never do.
    NeverDo,
    /// How will we know it worked.
    DoneWhen,
}

impl Ask {
    pub fn all() -> [Ask; 5] {
        [Ask::Shape, Ask::Inputs, Ask::WhenItGoesWrong, Ask::NeverDo, Ask::DoneWhen]
    }

    pub fn question(&self) -> &'static str {
        match self {
            Ask::Shape => "When it's working, what does it do?",
            Ask::Inputs => "What can it assume about what it's given?",
            Ask::WhenItGoesWrong => "And when that isn't true — fail loudly, or carry on?",
            Ask::NeverDo => "Anything it must never do, however sensible it seems?",
            Ask::DoneWhen => "How will we know it's finished?",
        }
    }

    /// Why this one is worth the interruption.
    pub fn because(&self) -> &'static str {
        match self {
            Ask::Shape => "everything else is a detail of this",
            Ask::Inputs => "half of all bugs are an assumption nobody wrote down",
            Ask::WhenItGoesWrong => {
                "unanswered, this defaults to carrying on quietly, which is the \
                 worst of the options"
            }
            Ask::NeverDo => "cheaper to say now than to find out from what it did",
            Ask::DoneWhen => "without it there's no way to tell you it worked",
        }
    }
}

/// What has been settled about a thing being built.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    pub idea: String,
    pub shape: Option<String>,
    pub inputs: Option<String>,
    pub when_wrong: Option<String>,
    pub never: Vec<String>,
    pub done_when: Vec<crate::goal::Check>,
}

impl Spec {
    pub fn new(idea: &str) -> Spec {
        Spec { idea: idea.to_string(), ..Default::default() }
    }

    /// The next thing worth asking, or `None` when there is enough to start.
    ///
    /// One at a time. Handing over five questions at once gets four skimmed
    /// answers, and the skimmed ones are the assumptions that bite later.
    pub fn next_question(&self) -> Option<Ask> {
        if self.shape.is_none() {
            return Some(Ask::Shape);
        }
        if self.inputs.is_none() {
            return Some(Ask::Inputs);
        }
        if self.when_wrong.is_none() {
            return Some(Ask::WhenItGoesWrong);
        }
        if self.done_when.is_empty() {
            return Some(Ask::DoneWhen);
        }
        // `never` is allowed to be empty. Plenty of small things have no
        // prohibitions, and inventing one to fill the field would be worse
        // than leaving it out.
        None
    }

    pub fn ready(&self) -> bool {
        self.next_question().is_none()
    }

    /// What was settled, as something a loop can check itself against.
    pub fn as_goal(&self, lang: Lang, give_up_after: u32) -> crate::goal::Goal {
        let mut g = crate::goal::Goal::new(&self.idea, give_up_after);
        // The toolchain first: nothing else can be judged until it compiles.
        for gate in ladder(lang).into_iter().filter(|g| g.tells != Tells::Shape) {
            g = g.checking(crate::goal::Check::CommandPasses(gate.command));
        }
        for c in &self.done_when {
            g = g.checking(c.clone());
        }
        g
    }
}

/// Guess a project's language from what is sitting in its directory, rather
/// than from a single file's extension -- the ladder runs against the whole
/// project, not one file, so a `Cargo.toml` or `pyproject.toml` at the root
/// is a better signal than any one path inside it.
pub fn lang_of_dir(dir: &std::path::Path) -> Option<Lang> {
    if dir.join("Cargo.toml").is_file() {
        return Some(Lang::Rust);
    }
    if dir.join("go.mod").is_file() {
        return Some(Lang::Go);
    }
    // A TypeScript project is a JavaScript project with a tsconfig, so the
    // tsconfig is checked first — otherwise every TS repo would read as JS
    // on its package.json.
    if dir.join("tsconfig.json").is_file() {
        return Some(Lang::TypeScript);
    }
    for marker in ["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt"] {
        if dir.join(marker).is_file() {
            return Some(Lang::Python);
        }
    }
    if dir.join("package.json").is_file() {
        return Some(Lang::JavaScript);
    }
    if dir.join("CMakeLists.txt").is_file()
        || std::fs::read_dir(dir).ok().is_some_and(|d| d.flatten().any(|e| e.path().extension().is_some_and(|x| x == "vcxproj")))
    {
        return Some(Lang::Cpp);
    }
    None
}
