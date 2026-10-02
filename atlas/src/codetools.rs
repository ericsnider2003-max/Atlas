//! The programs Atlas checks code with, downloaded by Atlas itself (1 Oct
//! 2026). Eric: "everything Atlas needs to properly run and perform every
//! capability it has, Atlas needs to download itself."
//!
//! `craft`'s ladders run each language's own tools -- ruff, mypy, pytest,
//! prettier, tsc, go, clang++, cargo. Until now none of them was fetched,
//! so on a laptop without them every check said "can't check". These are
//! the official portable Windows x64 builds, pinned by SHA-256 like every
//! other piece (`getpieces`), unpacked under `tools/` -- nothing installed
//! system-wide, no administrator rights, no PATH edited outside Atlas's own
//! process (`use_them`):
//!
//! | piece | from | gives |
//! |---|---|---|
//! | uv 0.12.21 | astral-sh/uv (Apache-2.0/MIT) | Python 3.12 and ruff, mypy, pytest at pinned versions |
//! | Node.js 24.21.0 LTS | nodejs.org | node, npm, then prettier and TypeScript at pinned versions |
//! | Go 1.27.1 | go.dev | go, gofmt |
//! | llvm-mingw 20260224 (LLVM 22.1.0) | mstorsjo/llvm-mingw | clang++, clang-format, clang-tidy, with its own headers -- no Visual Studio needed |
//! | rustup-init 1.28.2 (gnu) | static.rust-lang.org | stable Rust with clippy and rustfmt, the GNU toolchain so no Visual Studio linker is needed |
//!
//! The archives are hash-pinned. The second step (`finish`) fetches what
//! those tools fetch themselves -- Python from python-build-standalone via
//! uv, the npm and PyPI packages at exact versions, Rust's toolchain via
//! rustup -- which are version-pinned, not hash-pinned by Atlas. That is
//! said here rather than hidden.

use crate::getpieces::{Lands, Piece};
use std::path::{Path, PathBuf};

const PY_PACKAGES: &[&str] = &["ruff==0.16.10", "mypy==2.3.1", "pytest==9.1.1"];
const NODE_PACKAGES: &[&str] = &["prettier@3.9.9", "typescript@7.0.2"];

/// The archives, on a computer they exist for (Windows x64). Elsewhere the
/// system's own toolchains are used and nothing is fetched.
pub fn tool_pieces() -> Vec<Piece> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Vec::new();
    }
    vec![
        Piece {
            name: "the Python tools' installer (uv)",
            for_what: "checking Python code Atlas writes",
            url: "https://github.com/astral-sh/uv/releases/download/0.12.21/uv-x86_64-pc-windows-msvc.zip",
            sha256: "5d223efa0bf00208c3853246af09420419dfbd352536aa6bb8163d6170e23890",
            bytes: 17_992_232,
            lands: Lands::Zip { inside: "", dir: "tools/uv", key: "tools/uv/uv.exe" },
        },
        Piece {
            name: "Node.js",
            for_what: "checking JavaScript and TypeScript Atlas writes",
            url: "https://nodejs.org/dist/v24.21.0/node-v24.21.0-win-x64.zip",
            sha256: "158f7685b44de51f6c0df1d153526cbcd3e1bc739a8dfc607721cef75de9e541",
            bytes: 37_618_919,
            lands: Lands::Zip { inside: "node-v24.21.0-win-x64", dir: "tools/node", key: "tools/node/node.exe" },
        },
        Piece {
            name: "Go",
            for_what: "checking Go code Atlas writes",
            url: "https://go.dev/dl/go1.27.1.windows-amd64.zip",
            sha256: "a3911b5e0e1b1053f25ed0675f4c1c6aad1e2bfcf253df2b9be4caabd2edd95d",
            bytes: 78_931_360,
            lands: Lands::Zip { inside: "go", dir: "tools/go", key: "tools/go/bin/go.exe" },
        },
        Piece {
            name: "the C++ compiler (LLVM)",
            for_what: "checking C++ code Atlas writes",
            url: "https://github.com/mstorsjo/llvm-mingw/releases/download/20260224/llvm-mingw-20260224-ucrt-x86_64.zip",
            sha256: "acbf018fc24e42c21c74bea1df83f77c1f3e23de7d69d7d73b1993b5954c97ba",
            bytes: 186_477_828,
            lands: Lands::Zip { inside: "llvm-mingw-20260224-ucrt-x86_64", dir: "tools/llvm", key: "tools/llvm/bin/clang++.exe" },
        },
        Piece {
            name: "the Rust installer",
            for_what: "checking Rust code Atlas writes",
            url: "https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-pc-windows-gnu/rustup-init.exe",
            sha256: "ccbfd951d8024856043b3a0c3903a59f39937bce8d3074768b0d3da55f21e817",
            bytes: 14_392_226,
            lands: Lands::File("tools/rust/rustup-init.exe"),
        },
    ]
}

/// The folders Atlas's own tools live in, put first on this process's
/// search path (`getpieces::use_own_tools`), for the ones that are here.
pub fn bin_dirs(root: &Path) -> Vec<PathBuf> {
    // Not tools/llvm/bin: its `x86_64-w64-mingw32-gcc` is clang, which Rust's
    // GNU toolchain then links with and fails (no libgcc) -- measured on the
    // laptop, 1 Oct 2026. The C++ tools are found by `llvm_program` instead.
    ["tools/pyenv/Scripts", "tools/pyenv/bin", "tools/node", "tools/go/bin", "tools/rust/cargo/bin", "tools/uv"]
        .iter()
        .map(|d| root.join(d))
        .filter(|d| d.is_dir())
        .collect()
}

/// Rust's own two folders, which its programs read from the environment.
pub fn rust_env(root: &Path) -> Vec<(&'static str, PathBuf)> {
    let rust = root.join("tools/rust");
    if !rust.join("cargo").is_dir() {
        return Vec::new();
    }
    vec![("RUSTUP_HOME", rust.join("rustup")), ("CARGO_HOME", rust.join("cargo"))]
}

/// The C++ tools by their full path, when Atlas fetched them (they are kept
/// off the search path: see `bin_dirs`).
pub fn llvm_program(program: &str, root: &Path) -> Option<PathBuf> {
    if !matches!(program, "clang++" | "clang" | "clang-format" | "clang-tidy") {
        return None;
    }
    let p = root.join("tools/llvm/bin").join(if cfg!(windows) { format!("{program}.exe") } else { program.to_string() });
    p.is_file().then_some(p)
}

/// A program a gate names that is a script for node, not a program of its
/// own (`npm`, `prettier`, `tsc` are `.cmd` shims on Windows, which a
/// process can't be started from): node itself, and the script to hand it.
pub fn by_node(program: &str, root: &Path) -> Option<(PathBuf, PathBuf)> {
    let node = root.join("tools/node").join(if cfg!(windows) { "node.exe" } else { "bin/node" });
    let script = match program {
        "npm" => root.join("tools/node/node_modules/npm/bin/npm-cli.js"),
        "prettier" => root.join("tools/node-tools/node_modules/prettier/bin/prettier.cjs"),
        "tsc" => root.join("tools/node-tools/node_modules/typescript/bin/tsc"),
        _ => return None,
    };
    (node.is_file() && script.is_file()).then_some((node, script))
}

/// uv, for installing a build's own Python packages and running it with
/// them (2 Oct 2026): the one Atlas fetched, else one on this machine's PATH.
pub fn uv_program(root: &Path) -> Option<PathBuf> {
    let own = root.join("tools/uv").join(if cfg!(windows) { "uv.exe" } else { "uv" });
    if own.is_file() {
        return Some(own);
    }
    crate::tools::which("uv").map(PathBuf::from)
}

/// The Python Atlas set up for its checks, when it did.
pub fn own_python(root: &Path) -> Option<PathBuf> {
    let p = if cfg!(windows) { root.join("tools/pyenv/Scripts/python.exe") } else { root.join("tools/pyenv/bin/python") };
    p.is_file().then_some(p)
}

/// A Python to run a build with: Atlas's own, else the machine's.
pub fn any_python(root: &Path) -> Option<String> {
    if let Some(p) = own_python(root) {
        return Some(p.to_string_lossy().into_owned());
    }
    ["python3", "python", "py"].iter().find_map(|p| crate::tools::which(p))
}

/// Is the second step (`finish`) still to do for an archive that's here?
pub fn unfinished(root: &Path) -> bool {
    (root.join("tools/uv/uv.exe").is_file() && !root.join("tools/pyenv/Scripts/pytest.exe").is_file())
        || (by_node("npm", root).is_some() && !root.join("tools/node-tools/node_modules/typescript").is_dir())
        || (root.join("tools/rust/rustup-init.exe").is_file() && !root.join("tools/rust/cargo/bin/cargo.exe").is_file())
}

/// Run one setup command, saying what went wrong in words.
fn run(program: &Path, args: &[String], env: &[(&str, PathBuf)]) -> Result<(), String> {
    let mut c = crate::tools::command(program);
    c.args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().map_err(|e| format!("couldn't start {}: {e}", program.display()))?;
    if out.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&out.stderr);
    Err(format!("{} failed: {}", program.display(), said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no reason given").trim()))
}

/// The second step, once the archives are here: Python and its checkers,
/// the node packages, and Rust's toolchain -- each only if it isn't there
/// yet. What couldn't be done, in words; empty when everything is ready.
pub fn finish(root: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    let uv = root.join("tools/uv/uv.exe");
    let pyenv = root.join("tools/pyenv");
    if uv.is_file() && !pyenv.join("Scripts/pytest.exe").is_file() {
        let env = [("UV_PYTHON_INSTALL_DIR", root.join("tools/python")), ("UV_CACHE_DIR", root.join("tools/uv-cache"))];
        let made = run(&uv, &["venv".into(), s(pyenv.clone()), "--python".into(), "3.12".into()], &env).and_then(|_| {
            let mut args = vec!["pip".into(), "install".into(), "--python".into(), s(pyenv.join("Scripts/python.exe"))];
            args.extend(PY_PACKAGES.iter().map(|p| p.to_string()));
            run(&uv, &args, &env)
        });
        if let Err(e) = made {
            problems.push(format!("Python's checkers: {e}"));
        }
    }
    if let Some((node, npm)) = by_node("npm", root) {
        if !root.join("tools/node-tools/node_modules/typescript").is_dir() {
            let mut args = vec![s(npm), "install".into(), "--prefix".into(), s(root.join("tools/node-tools")), "--no-audit".into(), "--no-fund".into()];
            args.extend(NODE_PACKAGES.iter().map(|p| p.to_string()));
            if let Err(e) = run(&node, &args, &[]) {
                problems.push(format!("JavaScript's checkers: {e}"));
            }
        }
    }
    let rustup = root.join("tools/rust/rustup-init.exe");
    if rustup.is_file() && !root.join("tools/rust/cargo/bin/cargo.exe").is_file() {
        let env = [("RUSTUP_HOME", root.join("tools/rust/rustup")), ("CARGO_HOME", root.join("tools/rust/cargo"))];
        let args: Vec<String> = ["-y", "--no-modify-path", "--profile", "minimal", "--default-host", "x86_64-pc-windows-gnu", "--default-toolchain", "stable", "-c", "clippy", "-c", "rustfmt"]
            .iter()
            .map(|a| a.to_string())
            .collect();
        if let Err(e) = run(&rustup, &args, &env) {
            problems.push(format!("Rust: {e}"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_scripts_are_run_by_node_only_when_both_are_here() {
        let root = std::env::temp_dir().join(format!("atlas-codetools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(by_node("prettier", &root).is_none());
        let node = root.join("tools/node").join(if cfg!(windows) { "node.exe" } else { "bin/node" });
        std::fs::create_dir_all(node.parent().unwrap()).unwrap();
        std::fs::write(&node, b"").unwrap();
        let script = root.join("tools/node-tools/node_modules/prettier/bin/prettier.cjs");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, b"").unwrap();
        assert_eq!(by_node("prettier", &root), Some((node, script)));
        assert!(by_node("cargo", &root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
