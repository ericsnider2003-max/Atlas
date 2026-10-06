//! Two things happen here: every build gets its own version, and Atlas.exe
//! gets its icon and its name in Windows (Task Manager, the security
//! prompts).
//!
//! **The version.** Until 28 Sep 2026 every build said it was 0.1.0 (the
//! Cargo.toml version, never bumped), so a new build arriving through the
//! courier was taken for the one already running and deleted, and was then
//! recorded as installed anyway. Now each CI build is stamped with its own
//! number: the workflow sets `ATLAS_BUILD_NUMBER` (GitHub's run number) and
//! the program's version is `0.1.<n>`. A build made anywhere else says
//! `0.1.0-dev`. The version is what a person reads; what a build *is* stays
//! its SHA-256 (`upgrade::build_tag`), so two dev builds are still told apart.
//!
//! **The Windows resource.** The icon and the version block (VERSIONINFO) are
//! compiled from a generated `.rc` when a resource compiler is on hand --
//! `rc.exe` from the Windows SDK on GitHub's Windows machines, `windres` from
//! MinGW for cross-builds -- so the version Windows shows, and the one
//! `release::version_in_exe` reads back, is this build's. With no resource
//! compiler, the precompiled `windows/atlas.res` / `atlas-res.o` (icon, and a
//! version block saying 0.1.0) are linked as before, with a warning, so a
//! build never fails for want of one. Nothing happens for any other system.
#![allow(clippy::expect_used, reason = "a build script: a variable cargo always sets that is missing fails the build, which is the right outcome")]

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=windows/atlas.res");
    println!("cargo:rerun-if-changed=windows/atlas-res.o");
    println!("cargo:rerun-if-changed=assets/atlas.ico");
    println!("cargo:rerun-if-env-changed=ATLAS_BUILD_NUMBER");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=WINDRES");

    let number = std::env::var("ATLAS_BUILD_NUMBER").ok().and_then(|n| n.trim().parse::<u32>().ok());
    let version = match number {
        Some(n) => format!("0.1.{n}"),
        None => "0.1.0-dev".to_string(),
    };
    println!("cargo:rustc-env=ATLAS_VERSION={version}");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets this"));
    let dir = manifest.join("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets this"));
    match compile_resource(&manifest, &out, &version, number.unwrap_or(0), msvc) {
        Ok(compiled) => println!("cargo:rustc-link-arg-bins={}", compiled.display()),
        Err(why) => {
            println!(
                "cargo:warning=Atlas's Windows version block could not be generated ({why}); \
                 linking the precompiled one, which says 0.1.0"
            );
            let file = if msvc { "atlas.res" } else { "atlas-res.o" };
            println!("cargo:rustc-link-arg-bins={}", dir.join(file).display());
        }
    }
}

/// The `.rc` text for this build: the icon, and a version block whose
/// ProductName is "Atlas" (what `release::version_in_exe` looks for).
fn rc_text(icon: &Path, version: &str, number: u32) -> String {
    // Both compilers take forward slashes, and a backslash would need escaping.
    let icon = icon.display().to_string().replace('\\', "/");
    let n = number.min(65535);
    format!(
        "1 ICON \"{icon}\"\n\
         1 VERSIONINFO\n\
         FILEVERSION 0,1,{n},0\n\
         PRODUCTVERSION 0,1,{n},0\n\
         BEGIN\n\
         \x20 BLOCK \"StringFileInfo\"\n\
         \x20 BEGIN\n\
         \x20   BLOCK \"040904b0\"\n\
         \x20   BEGIN\n\
         \x20     VALUE \"FileDescription\", \"Atlas\"\n\
         \x20     VALUE \"ProductName\", \"Atlas\"\n\
         \x20     VALUE \"FileVersion\", \"{version}\"\n\
         \x20     VALUE \"ProductVersion\", \"{version}\"\n\
         \x20     VALUE \"OriginalFilename\", \"atlas.exe\"\n\
         \x20   END\n\
         \x20 END\n\
         \x20 BLOCK \"VarFileInfo\"\n\
         \x20 BEGIN\n\
         \x20   VALUE \"Translation\", 0x409, 1200\n\
         \x20 END\n\
         END\n"
    )
}

fn compile_resource(manifest: &Path, out: &Path, version: &str, number: u32, msvc: bool) -> Result<PathBuf, String> {
    let rc = out.join("atlas-version.rc");
    std::fs::write(&rc, rc_text(&manifest.join("assets").join("atlas.ico"), version, number))
        .map_err(|e| format!("couldn't write {}: {e}", rc.display()))?;
    if msvc {
        let compiled = out.join("atlas-version.res");
        let tool = find_rc_exe().ok_or("no rc.exe (the Windows SDK's resource compiler) was found")?;
        let status = std::process::Command::new(&tool)
            .arg("/nologo")
            .arg("/fo")
            .arg(&compiled)
            .arg(&rc)
            .status()
            .map_err(|e| format!("{} didn't run: {e}", tool.display()))?;
        if !status.success() {
            return Err(format!("{} failed ({status})", tool.display()));
        }
        Ok(compiled)
    } else {
        let compiled = out.join("atlas-version-res.o");
        let target = std::env::var("TARGET").unwrap_or_default();
        let mut tried = Vec::new();
        let candidates: Vec<String> = std::env::var("WINDRES")
            .ok()
            .into_iter()
            .chain([
                format!("{}-w64-mingw32-windres", target.split('-').next().unwrap_or("x86_64")),
                "x86_64-w64-mingw32-windres".to_string(),
                "windres".to_string(),
            ])
            .collect();
        for tool in candidates {
            match std::process::Command::new(&tool)
                .arg("--input-format=rc")
                .arg("--output-format=coff")
                .arg("-i")
                .arg(&rc)
                .arg("-o")
                .arg(&compiled)
                .status()
            {
                Ok(s) if s.success() => return Ok(compiled),
                Ok(s) => tried.push(format!("{tool} failed ({s})")),
                Err(_) => tried.push(format!("no {tool}")),
            }
        }
        Err(tried.join(", "))
    }
}

/// `rc.exe`: `RC` if set, on PATH, or the newest Windows 10/11 SDK's x64 one.
fn find_rc_exe() -> Option<PathBuf> {
    if let Ok(rc) = std::env::var("RC") {
        let p = PathBuf::from(rc);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        if let Some(p) = std::env::split_paths(&path).map(|d| d.join("rc.exe")).find(|p| p.is_file()) {
            return Some(p);
        }
    }
    let base = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"))
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("x64").join("rc.exe").is_file())
        .collect();
    versions.sort();
    versions.pop().map(|p| p.join("x64").join("rc.exe"))
}
