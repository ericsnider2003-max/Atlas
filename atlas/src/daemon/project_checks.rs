//! Checking a change to a project: verifying, compiling in a sandbox, the ladder of checks.
//!
//! Moved out of `daemon.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.


/// A little context from a project folder to write the draft against — the
/// names and first lines of a few source files, not the whole tree. Empty
/// when the folder is unknown or unreadable. Deliberately bounded: this is
/// orientation for the model, not the whole codebase.
/// Turn a generated draft for a project into a queued change, verified as
/// strongly as the project allows, with a summary that never overstates what
/// was proven.
///
/// Two levels, and the honest gap between them is the whole reason this exists:
/// - **Integrated.** When the request names a file the project already has, and
///   the project is on disk and buildable, the change is proved *inside a copy
///   of the project* — the project still builds and its own tests still pass
///   with the change in it. This is real verification for that project.
/// - **Isolated.** Otherwise (a new file, no named target, or the project
///   can't be reached), the check is only that the code compiles on its own,
///   and the summary says exactly that — a proposal to read, not a proven
///   change. `Outcome::in_project` carries that wording.
///
/// Returns `(verified, summary, files)`. `verified` is only true for an
/// integrated pass that replaced real code; an isolated compile is never
/// `verified` for a project.
pub(super) fn verify_project_change(
    project: &str,
    folder: &str,
    title: &str,
    lang: crate::craft::Lang,
    request: &str,
    code: &str,
    isolated: &crate::build_it::Outcome,
    base: &std::path::Path,
    proven_in_place: bool,
) -> (bool, String, Vec<crate::workshop::FileEdit>) {
    let fallback_path = format!("proposed_change.{}", ext_for(lang));
    let root = std::path::Path::new(folder);

    // The file this change belongs in, if the request names one the project
    // actually has. Deterministic — a real path in the words, not a guess —
    // because integrated verification is only sound when the change replaces
    // code the project already compiles and tests.
    let target = named_existing_file(request, root);

    // The project's own test command, from its language's ladder.
    let test_cmd = crate::craft::ladder(lang)
        .into_iter()
        .find(|g| g.tells == crate::craft::Tells::Behaviour)
        .map(|g| g.command);

    // The path the queued change writes to: the real file when known, else a
    // clearly-named proposal file.
    let write_path = target.clone().unwrap_or(fallback_path);
    let files = vec![crate::workshop::FileEdit { path: write_path.clone(), content: code.to_string() }];
    // A whole-file replacement of a file you have is queued only once it
    // has passed -- its own checks, and the project's where they can run.
    // Anything less goes beside it as `<file>.proposed`, so "implement" can
    // never write a half-finished or failing file over yours (2 Oct 2026).
    let beside = || -> Vec<crate::workshop::FileEdit> {
        match &target {
            Some(rel) => vec![crate::workshop::FileEdit { path: format!("{rel}.proposed"), content: code.to_string() }],
            None => files.clone(),
        }
    };
    let beside_note = |rel: &Option<String>| match rel {
        Some(r) => format!(" It would replace all of {r}, so it's kept beside it as {r}.proposed rather than over it."),
        None => String::new(),
    };

    // Already proven in place: the fix rounds ran the project's own tests
    // with this file in it (2 Oct 2026), and the last run passed -- not run
    // a second time.
    if proven_in_place && isolated.is_built() && target.is_some() {
        let summary = format!(
            "Queued \"{title}\" on {project}. With it in place, {project} builds and its own tests pass. Say \
             \"implement {title}\" and I'll write it with a .before backup; nothing running is touched."
        );
        return (true, summary, files);
    }
    // Integrated verification is possible only for a built draft that replaces
    // a real file in a buildable project on disk.
    match (isolated.is_built(), target.as_ref(), test_cmd, root.is_dir()) {
        (true, Some(rel), Some(cmd), true) => {
            let edits = vec![crate::selfwork::Edit {
                path: rel.clone(),
                content: code.to_string(),
                reason: String::new(),
            }];
            match crate::selfwork::prove_in_project(root, &cmd, &edits, base) {
                Ok(proof) => {
                    let verified = proof.built_and_passed && proof.replaced_existing;
                    if !proof.built_and_passed {
                        let summary = format!(
                            "Queued \"{title}\" on {project} as a draft. {}{}",
                            proof.plain(project),
                            beside_note(&target)
                        );
                        return (false, summary, beside());
                    }
                    let summary = format!(
                        "Queued \"{title}\" on {project}. {} Say \"implement {title}\" and I'll \
                         write it with a .before backup; nothing running is touched.",
                        proof.plain(project)
                    );
                    (verified, summary, files)
                }
                // The project's suite couldn't be run — fall back to the honest
                // isolated wording rather than claiming a project check that
                // didn't happen.
                // Nor is it written over your file: unproven in place, it
                // goes beside it (2 Oct 2026).
                Err(why) => {
                    let mut s = isolated.in_project(project, title, lang);
                    s.push_str(&format!(
                        " (I couldn't run {project}'s own tests to check it in place: {why}.)"
                    ));
                    s.push_str(&beside_note(&target));
                    (false, s, beside())
                }
            }
        }
        // Isolated only: a new file, no named target, an unbuildable/absent
        // project, or a draft that didn't even pass its own checks. An isolated
        // compile is never "verified" for a project — that flag is reserved for
        // an integrated pass.
        // A draft that didn't pass its own checks never replaces a file.
        _ if !isolated.is_built() && target.is_some() => {
            (false, format!("{}{}", isolated.in_project(project, title, lang), beside_note(&target)), beside())
        }
        _ => (false, isolated.in_project(project, title, lang), files),
    }
}

/// A source file the request names that the project actually has, as a
/// project-relative path. `None` when the words name no such file — the signal
/// that this is a new-file change, not a replacement.
pub(super) fn named_existing_file(request: &str, root: &std::path::Path) -> Option<String> {
    for tok in request.split(|c: char| c.is_whitespace() || c == '"' || c == '`' || c == '(' || c == ')') {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-');
        if t.is_empty() || crate::craft::Lang::of_path(t).is_none() {
            continue;
        }
        if root.join(t).is_file() {
            return Some(t.to_string());
        }
    }
    None
}

/// Does the request point at something Atlas built recently, rather than a file
/// or pasted code? Tied to Atlas's own action ("you built", "you made") so
/// "explain the code that builds X" isn't caught by the bare word "build".
pub(super) fn references_a_build(low: &str) -> bool {
    [
        "you built", "you made", "you just built", "just built", "last build",
        "recently built", "the build", "thing you built",
    ]
    .iter()
    .any(|p| low.contains(p))
}

/// Does the request point at a change waiting to be implemented? "the change"
/// and "waiting" are broad, but the fallback when nothing is queued is a plain
/// "nothing waiting", so a false match is harmless.
pub(super) fn references_a_queued_change(low: &str) -> bool {
    [
        "waiting", "queued", "to implement", "to be implemented", "the change",
        "proposed change", "on implementation", "change you",
    ]
    .iter()
    .any(|p| low.contains(p))
}

/// What of the project a code change is written against (2 Oct 2026: it
/// was the first 40 lines of the first six files `read_dir` listed): the
/// file tree and the pieces the request points at, within a budget from the
/// writing model's context (`projectread`).
pub(super) fn read_project_context(folder: &str, request: &str, context_tokens: Option<u32>) -> String {
    if folder.trim().is_empty() {
        return String::new();
    }
    let root = std::path::Path::new(folder);
    crate::projectread::relevant(root, request, crate::projectread::budget_for(context_tokens)).text
}

/// The project's own tests, run with the draft in place of `rel` in a copy
/// of the project, as a fix round's check (2 Oct 2026: the fix rounds only
/// ever saw the draft compiled on its own, and the project's tests ran once,
/// after the last round, with nothing to fix from them). `None` when they
/// can't tell anything -- they couldn't run, or there are none -- and the
/// draft is checked on its own instead.
pub(super) fn check_in_project(root: &std::path::Path, rel: &str, test_cmd: &str, code: &str, base: &std::path::Path) -> Option<crate::build_it::Check> {
    let edits = vec![crate::selfwork::Edit { path: rel.to_string(), content: code.to_string(), reason: String::new() }];
    let proof = crate::selfwork::prove_in_project(root, test_cmd, &edits, base).ok()?;
    if proof.built_and_passed {
        return Some(crate::build_it::Check::Passed(Vec::new()));
    }
    if proof.output.contains("no tests ran") || proof.output.contains("could not start") {
        return None;
    }
    Some(crate::build_it::Check::Failed(proof.output))
}

/// The file extension for a generated draft, so it lands under a name you can
/// open. The language itself is the authority on this, so there is no match to
/// keep in step here.
pub(super) fn ext_for(lang: crate::craft::Lang) -> &'static str {
    lang.ext()
}

/// Copy the tree's compile inputs into a scratch directory, so a candidate
/// self-fix can be built and tested in isolation from the tree you run.
///
/// Everything a `cargo test` needs and nothing it produces: `src/`, `tests/`,
/// `config/`, `benches/`, the manifests and `build.rs` — never `target/`,
/// `.git/`, or another sandbox, which are huge and rebuildable. A cold build is
/// the cost of not touching your real files; that is the trade this makes on
/// purpose.
pub(super) fn copy_compile_inputs(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    const TOP: &[&str] = &["src", "tests", "config", "benches", "examples"];
    const FILES: &[&str] = &["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"];
    std::fs::create_dir_all(to)?;
    for d in TOP {
        let src = from.join(d);
        if src.is_dir() {
            copy_dir_shallowly(&src, &to.join(d))?;
        }
    }
    for f in FILES {
        let src = from.join(f);
        if src.is_file() {
            std::fs::copy(&src, to.join(f))?;
        }
    }
    Ok(())
}

/// Recursively copy a directory, following the same "no symlinks, no target"
/// rule the rest of the tree uses for copies.
pub(super) fn copy_dir_shallowly(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        // Never carry a build dir or a nested VCS/scratch tree along.
        if matches!(name.to_str(), Some("target") | Some(".git")) {
            continue;
        }
        let dst = to.join(&name);
        if ty.is_dir() {
            copy_dir_shallowly(&entry.path(), &dst)?;
        } else {
            std::fs::copy(entry.path(), dst)?;
        }
    }
    Ok(())
}

/// Scaffold a draft into a sandbox and run `craft`'s ladder against it — the
/// local fact-check that decides whether a draft is trusted. Returns the
/// ladder's verdict as a `build_it::Check`.
///
/// A Rust draft is written as a tiny crate (a `Cargo.toml` plus `src/lib.rs`)
/// so `cargo check`/`clippy`/`test` have something to build; a Python draft is
/// written as a single file. The gates run in the ladder's order and stop at
/// the first blocking failure, because a next attempt working from the output
/// of code that does not compile is working from noise.
pub(super) fn check_draft_in_sandbox(
    sandbox: &mut crate::sandbox::Sandbox,
    lang: crate::craft::Lang,
    code: &str,
) -> crate::build_it::Check {
    // Lay the draft down as something the toolchain can act on. The scaffold
    // is the language's own (a Rust crate, a Go module, a tsconfig beside the
    // file), so the ladder's commands always have what they expect.
    for (path, contents) in lang.draft_files(code) {
        if let Err(e) = sandbox.write(&path, &contents) {
            return crate::build_it::Check::Failed(format!("couldn't scaffold the draft: {e}"));
        }
    }
    // Packages a Python draft declares, installed into the sandbox -- not
    // into your Python -- through Atlas's own uv (2 Oct 2026). Without them
    // the checks fail on the import, which isn't the code's fault.
    if lang == crate::craft::Lang::Python {
        let deps = crate::build_it::python_deps(code);
        if !deps.is_empty() {
            if let Err(missing) = install_python_deps(&sandbox.root, &deps) {
                return crate::build_it::Check::CannotCheck(missing);
            }
        }
    }
    run_ladder_in(&sandbox.root, lang, true)
}

/// A Python draft's packages into `dir/.deps`, with the files that point the
/// checks at them. `Err` names what couldn't be had, as the thing that isn't
/// on this computer.
pub(super) fn install_python_deps(dir: &std::path::Path, deps: &[String]) -> std::result::Result<(), String> {
    let root = crate::roots::install_root();
    let Some(uv) = crate::codetools::uv_program(&root) else {
        return Err(format!("uv (to install {})", deps.join(", ")));
    };
    for (path, text) in crate::build_it::python_dep_files() {
        if std::fs::write(dir.join(&path), text).is_err() {
            return Err(format!("room to install {}", deps.join(", ")));
        }
    }
    let mut args: Vec<String> = vec!["pip".into(), "install".into(), "--target".into(), crate::build_it::PY_DEPS_DIR.into()];
    if let Some(py) = crate::codetools::own_python(&root) {
        args.push("--python".into());
        args.push(py.to_string_lossy().into_owned());
    }
    args.extend(deps.iter().cloned());
    let cache = root.join("tools/uv-cache");
    let cache = cache.to_string_lossy().into_owned();
    let (ok, said) = crate::sandbox::run_within(&uv.to_string_lossy(), &args, &[("UV_CACHE_DIR", cache.as_str())], dir, 300, 2000);
    if ok {
        Ok(())
    } else {
        let why = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        Err(format!("{} (uv couldn't install it: {why})", deps.join(", ")))
    }
}

/// Run `craft`'s ladder for `lang` in `dir`, as a `build_it::Check`. In a
/// sandbox the formatting gates run too; in a folder of yours
/// (`run_ladder_in(folder, lang, false)`, 2 Oct 2026, after a coding agent
/// changed it) they don't -- a check reads your files, it doesn't rewrite
/// them. A project with no tests isn't failed for having none.
pub(super) fn run_ladder_in(dir: &std::path::Path, lang: crate::craft::Lang, rewrite_allowed: bool) -> crate::build_it::Check {
    use crate::craft::{ladder, read_ladder, Next, Ran, Tells};
    let mut ran: Vec<Ran> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for gate in ladder(lang) {
        if !rewrite_allowed && gate.tells == Tells::Shape {
            continue;
        }
        // Build a runnable tool from the gate's command line.
        let mut parts = gate.command.split_whitespace();
        let Some(program) = parts.next() else { continue };
        let mut args: Vec<String> = parts.map(str::to_string).collect();
        // npm, prettier and tsc are node scripts (`.cmd` on Windows, which
        // can't be started directly): run by the node Atlas fetched.
        // The program the C++ ladder just built, by its full path.
        let mut program = crate::craft::program_in(dir, program);
        if let Some(p) = crate::codetools::llvm_program(&program, &crate::roots::install_root()) {
            program = p.to_string_lossy().into_owned();
        }
        if let Some((node, script)) = crate::codetools::by_node(&program, &crate::roots::install_root()) {
            args.insert(0, script.to_string_lossy().into_owned());
            program = node.to_string_lossy().into_owned();
        }
        // Generous against the gate's rough estimate; a compile that runs
        // far past it is stuck, not slow.
        let limit = (gate.seconds as u64) * 4 + 30;
        let (mut passed, output) = crate::sandbox::run_within(&program, &args, &[], dir, limit, 4000);
        // pytest with nothing to collect exits 5: in a sandbox the smoke test
        // is always there, so only a folder of yours can have none.
        if !passed && !rewrite_allowed && gate.tells == Tells::Behaviour && output.contains("no tests ran") {
            passed = true;
            notes.push("there are no tests to run".into());
        }
        ran.push(Ran { command: gate.command.clone(), tells: gate.tells, passed, output });
        // Stop after the first blocking failure — the rest would be noise.
        if !passed && gate.tells == Tells::Sound {
            break;
        }
    }

    match read_ladder(lang, &ran) {
        Next::Good => crate::build_it::Check::Passed(notes),
        Next::WorksWithNotes(mut more) => {
            more.extend(notes);
            crate::build_it::Check::Passed(more)
        }
        Next::Fix { output, .. } => crate::build_it::Check::Failed(output),
        Next::CannotCheck { program, .. } => crate::build_it::Check::CannotCheck(program),
    }
}
