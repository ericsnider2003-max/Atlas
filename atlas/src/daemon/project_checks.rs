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


pub(super) fn check_in_project_controlled(root: &std::path::Path, rel: &str, test_cmd: &str, code: &str, base: &std::path::Path, budget: Option<&crate::tools::WorkBudget<'_>>) -> Option<crate::build_it::Check> {
    if let Some(b) = budget { if let Err(why) = b.check() { return Some(crate::build_it::Check::Failed(why)); } }
    let edits = vec![crate::selfwork::Edit { path: rel.to_string(), content: code.to_string(), reason: String::new() }];
    let proof = crate::selfwork::prove_in_project_controlled(root, test_cmd, &edits, base, budget).map_err(|why| why).ok()?;
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


pub(super) fn copy_compile_inputs_controlled(from: &std::path::Path, to: &std::path::Path, budget: Option<&crate::tools::WorkBudget<'_>>) -> std::io::Result<()> {
    let mut copied = (0usize, 0u64);
    const TOP: &[&str] = &["src", "tests", "config", "benches", "examples"];
    const FILES: &[&str] = &["Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"];
    std::fs::create_dir_all(to)?;
    for d in TOP {
        let src = from.join(d);
        if src.is_dir() {
            copy_dir_controlled(&src, &to.join(d), budget, &mut copied)?;
        }
    }
    for f in FILES {
        let src = from.join(f);
        if src.is_file() {
            copy_file_controlled(&src, &to.join(f), budget, &mut copied)?;
        }
    }
    Ok(())
}


fn copy_file_controlled(from: &std::path::Path, to: &std::path::Path, budget: Option<&crate::tools::WorkBudget<'_>>, copied: &mut (usize, u64)) -> std::io::Result<()> {
    use std::io::{Read, Write};
    let check = || budget.map_or(Ok(()), |b| b.check().map_err(std::io::Error::other));
    check()?;
    let size = std::fs::symlink_metadata(from)?;
    if !size.is_file() || size.file_type().is_symlink() { return Err(std::io::Error::other("repair inputs include a link or non-file; no complete copy was made")); }
    copied.0 = copied.0.saturating_add(1);
    copied.1 = copied.1.saturating_add(size.len());
    if copied.0 > 8192 || copied.1 > 128 * 1024 * 1024 { return Err(std::io::Error::other("repair inputs exceed the bounded copy budget; no complete copy was made")); }
    let mut input = std::fs::File::open(from)?;
    let mut output = std::fs::File::create(to)?;
    let mut bytes = [0u8; 64 * 1024];
    loop { check()?; let n = input.read(&mut bytes)?; if n == 0 { break; } output.write_all(&bytes[..n])?; }
    check()
}

fn copy_dir_controlled(from: &std::path::Path, to: &std::path::Path, budget: Option<&crate::tools::WorkBudget<'_>>, copied: &mut (usize, u64)) -> std::io::Result<()> {
    if let Some(b) = budget { b.check().map_err(std::io::Error::other)?; }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        if let Some(b) = budget { b.check().map_err(std::io::Error::other)?; }
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() { return Err(std::io::Error::other("repair inputs include a symbolic link; no complete copy was made")); }
        let name = entry.file_name();
        if matches!(name.to_str(), Some("target") | Some(".git")) { continue; }
        let dst = to.join(&name);
        if ty.is_dir() { copy_dir_controlled(&entry.path(), &dst, budget, copied)?; }
        else { copy_file_controlled(&entry.path(), &dst, budget, copied)?; }
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
    check_draft_in_sandbox_controlled(sandbox, lang, code, None)
}

pub(super) fn check_draft_in_sandbox_controlled(sandbox: &mut crate::sandbox::Sandbox, lang: crate::craft::Lang, code: &str, budget: Option<&crate::tools::WorkBudget<'_>>) -> crate::build_it::Check {
    if let Some(b) = budget { if let Err(why) = b.check() { return crate::build_it::Check::Failed(why); } }
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
            if let Err(missing) = install_python_deps_controlled(&sandbox.root, &deps, budget) {
                return crate::build_it::Check::CannotCheck(missing);
            }
        }
    }
    let check = run_ladder_in_controlled(&sandbox.root, lang, true, budget);
    // What the formatter and the toolchain's own fixes changed is the code
    // now: handed over as it passed, not as the model wrote it.
    let main = lang.draft_files(code).into_iter().find(|(_, c)| c == code).map(|(p, _)| p);
    match main.and_then(|p| std::fs::read_to_string(sandbox.root.join(p)).ok()) {
        Some(now) if !now.trim().is_empty() && now != code => crate::build_it::Check::Rewrote(now, Box::new(check)),
        _ => check,
    }
}


fn install_python_deps_controlled(dir: &std::path::Path, deps: &[String], budget: Option<&crate::tools::WorkBudget<'_>>) -> std::result::Result<(), String> {
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
    let (ok, said) = run_project_command(&uv.to_string_lossy(), &args, &[("UV_CACHE_DIR", cache.as_str())], dir, 300, 2000, budget);
    if ok {
        Ok(())
    } else {
        let why = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        Err(format!("{} (uv couldn't install it: {why})", deps.join(", ")))
    }
}


pub(super) fn run_ladder_in_controlled(dir: &std::path::Path, lang: crate::craft::Lang, rewrite_allowed: bool, budget: Option<&crate::tools::WorkBudget<'_>>) -> crate::build_it::Check {
    use crate::craft::{ladder, read_ladder, Next, Ran, Tells};
    let mut ran: Vec<Ran> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for gate in ladder(lang) {
        if let Some(b) = budget { if let Err(why) = b.check() { return crate::build_it::Check::Failed(why); } }
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
        let (mut passed, mut output) = run_project_command(&program, &args, &[], dir, limit, 4000, budget);
        // The toolchain's own fixes first (`craft::autofix_for`), in a copy
        // only: a check of a folder of yours never rewrites it. Run again
        // after; what's left is what the next round works on.
        if !passed && rewrite_allowed {
            if let Some(fix) = crate::craft::autofix_for(&gate) {
                if let Some(done) = toolchain_fix(dir, &fix, limit, budget) {
                    let (again, out) = run_project_command(&program, &args, &[], dir, limit, 4000, budget);
                    if again {
                        notes.push(format!("the toolchain fixed what it found itself ({done})"));
                    }
                    passed = again;
                    output = out;
                }
            }
        }
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

fn run_project_command(program: &str, args: &[String], env: &[(&str, &str)], dir: &std::path::Path, limit: u64, max: usize, budget: Option<&crate::tools::WorkBudget<'_>>) -> (bool, String) {
    if let Some(b) = budget { if let Err(why) = b.check() { return (false, format!("error: {why}")); } }
    let stop = || budget.is_some_and(|b| b.stopping());
    let limit = budget.map_or(limit, |b| b.remaining(std::time::Duration::from_secs(limit)).as_secs().max(1));
    let result = crate::sandbox::run_within_controlled(program, args, env, dir, limit, max, Some(&stop));
    if let Some(b) = budget { if let Err(why) = b.check() { return (false, format!("error: {why}")); } }
    result
}

/// Run a toolchain's own fixer (`craft::autofix_for`) in `dir`, resolving
/// its program as the gates do. `Some(command)` when it ran; `None` when its
/// program isn't here or wouldn't start -- then nothing was changed.
fn toolchain_fix(dir: &std::path::Path, fix: &str, limit: u64, budget: Option<&crate::tools::WorkBudget<'_>>) -> Option<String> {
    let mut parts = fix.split_whitespace();
    let first = parts.next()?;
    let args: Vec<String> = parts.map(str::to_string).collect();
    let mut program = crate::craft::program_in(dir, first);
    if let Some(p) = crate::codetools::llvm_program(&program, &crate::roots::install_root()) {
        program = p.to_string_lossy().into_owned();
    }
    let (_ok, out) = run_project_command(&program, &args, &[], dir, limit, 2000, budget);
    let low = out.to_lowercase();
    if low.starts_with("couldn't run") || low.starts_with("could not start") {
        return None;
    }
    Some(fix.split_whitespace().take(2).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod the_toolchain_fixes_first {
    use crate::build_it::Check;
    use crate::craft::Lang;

    #[test]
    fn canceled_project_check_starts_no_tool_or_scaffold() {
        let root = std::env::temp_dir().join(format!("atlas-check-stopped-{}", std::process::id()));
        let stopped = || true;
        let budget = crate::tools::WorkBudget::new(std::time::Duration::from_secs(10), &stopped);
        let result = super::run_ladder_in_controlled(&root, Lang::Rust, true, Some(&budget));
        assert!(matches!(result, Check::Failed(ref text) if text.contains("stop")));
        assert!(!root.exists());
    }

    fn have(program: &str, args: &[&str]) -> bool {
        std::process::Command::new(program).args(args).output().is_ok_and(|o| o.status.success())
    }

    #[test]
    fn what_clippy_can_fix_itself_is_fixed_and_handed_over_fixed() {
        // A real toolchain run: skipped, saying so, where there's no cargo.
        if !have("cargo", &["--version"]) || !have("cargo", &["clippy", "--version"]) {
            eprintln!("no cargo/clippy here: nothing to run");
            return;
        }
        // `len() == 0` is clippy's `len_zero`, whose suggestion is marked
        // machine-applicable: the toolchain knows the exact change.
        let draft = "pub fn empty(v: &[u8]) -> bool {\n    v.len() == 0\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it() {\n        assert!(super::empty(&[]));\n    }\n}\n";
        let base = std::env::temp_dir().join(format!("atlas-autofix-{}", std::process::id()));
        let mut sb = crate::sandbox::Sandbox::create(&base, "autofix").unwrap();
        let mut code = draft.to_string();
        let check = super::check_draft_in_sandbox(&mut sb, Lang::Rust, draft).settle(&mut code);
        let _ = std::fs::remove_dir_all(&base);
        match check {
            Check::Passed(notes) => {
                assert!(code.contains("is_empty()"), "the fix wasn't carried into the code handed over:\n{code}");
                assert!(notes.iter().any(|n| n.contains("toolchain fixed")), "notes: {}", notes.join(" | "));
            }
            Check::Failed(out) => panic!("it failed after the fixes: {out}"),
            Check::CannotCheck(p) => panic!("{p} isn't here"),
            Check::Rewrote(..) => panic!("settle leaves no rewrite behind"),
        }
    }
}
