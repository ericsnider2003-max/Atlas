# Setup readiness: will anything unwired stop Atlas being set up?

**23 September 2026.** Eric asked: *"Is anything that isn't currently wired
going to prevent me from setting up Atlas?"* Checked against the tree rather
than answered from memory. Full suite afterwards: **30 targets, 6,176 passed,
0 failed, 0 warnings**.

## The unwired list: none of it is on the setup path

The guards name everything still unwired. They are two orphan functions
(`language::good_enough`, `server::with_peers`), three modules nothing reaches
(`look`, `grading`, `consent`), and about 255 functions that are built and
tested but have no caller. The setup path is the launcher, then
`atlas adapt`, `atlas doctor`, and a first command. It runs through wired
code only. By definition, nothing unwired is called during setup, so nothing
unwired can block it.

## What *would* have stopped setup, found by checking

None of these three was about wiring, and all three are fixed:

1. **The launcher couldn't get past its first check on a fresh copy.**
   `ATLAS.bat` stopped at "Could not find atlas.exe" before its own Build
   option could run. The cause: `cargo build` puts the exe in
   `target\release`, and the launcher never looked there. It now uses a build
   that's already there, or builds one if Rust is installed, and puts it
   beside the launcher. Guarded by
   `a_fresh_copy_builds_its_own_exe_instead_of_stopping`.
2. **Both batch files had Unix line endings.** cmd.exe can miss `goto` and
   `call :label` targets in an LF-only file, and ATLAS.bat jumps to `:menu`
   and calls `:setup`. Both files are now CRLF, guarded by
   `every_batch_file_is_crlf`.
3. **No prebuilt Windows program existed.** Setting up meant installing Rust
   and building it yourself. **`atlas.exe` is now built for Windows**
   (`x86_64-pc-windows-gnu`, release) and ships alongside the archive. The
   Windows platform layer also passes a full compile check against Windows.
   Smoke-tested under Wine: `atlas doctor` runs, enumerates the monitors
   through the real Win32 calls, and reports the install. `atlas wireguard`
   runs too.

One smaller fix went in too: `atlas doctor` used to end by printing Rust
source ("Paste into src/main.rs…") to whoever ran it. That now appears only
with `ATLAS_DEV` set, per Eric's rule that no code shows in what he sees.

## What a first `atlas doctor` will still show, and why none of it is a fault

These are expected on a fresh machine:

- **Voice tools missing** (ffmpeg, whisper, piper, their models). The
  launcher's first-run setup downloads them, about 260 MB, free, no account.
- **Apps not found.** `atlas adapt` finds your apps and writes
  `config/machine.yaml`.
- **"Settings that do nothing".** 21 sections of the shipped `tools.yaml`
  reach no code. This is the honesty check reporting the *shipped* config,
  not a problem with your machine. It's a known backlog, and it counts
  toward doctor's "problems" total, which will look alarming on day one.
  Cleaning it is a separate job.

## Still unproven: the first run on real Windows

The program has never run on the real laptop. Wine is not Windows. The
genuine unknowns are all ones the install guide already names:

- window placement and DPI scaling across the laptop panel and externals
- Smart App Control blocking an unsigned exe (`setup/reference/SMART_APP_CONTROL.md`)
- the test suite never having run on Windows (the launcher's *Test* option
  may show failures that are Linux assumptions, not real faults)

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
