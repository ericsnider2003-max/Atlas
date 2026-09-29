# Installing Atlas, and updating it afterwards

> **Setting up as a person, not a developer?** Read `SETTING_UP.md` instead:
> download `atlas.exe`, double-click it, and Atlas does the rest in its own
> window. You don't need a folder, a terminal, or Rust. What follows is the
> developer's path: building from source.

Two separate things, and conflating them is what made this feel expensive.

**Installing** happens once per machine and does take a while, mostly
downloads. **Updating** is replacing one file, and it does not touch anything
you have accumulated. If you have Atlas working and want a newer version, you
want the second section, and you do not need to delete anything.

---

## Updating an install you already have

```
atlas update
```

That prints, by name, every file an update would replace and every file it
would leave alone, and says whether it is safe. It moves nothing — it is a
report you read before you do anything.

Then:

1. Stop Atlas.
2. Rename the current binary to `atlas-<version>.previous` (`atlas update`
   prints the exact name). This is your way back, without a download.
3. Put the new binary in its place.
4. Start Atlas.

That is the whole update. Nothing under `data/` is involved, and neither is
`config/machine.yaml`.

### What survives, and why you can believe it

Atlas keeps three things in three places, and only one of them comes from a
download:

| | what it is | what an update does |
|---|---|---|
| the program | the `atlas` binary | replaced — this is the update |
| the recipe | `config/*.yaml` | replaced, when the shipped recipe changed |
| your machine, and its memory | `config/machine.yaml`, `data/` | **untouched** |

`data/` holds your notes, the tray, the vault, your paired peers, your
profiles and the journal. `config/machine.yaml` holds what `atlas adapt`
worked out about this computer. Neither is part of any download, and
`.gitignore` keeps them out of the source tree so they cannot arrive in one
by accident.

This is asserted, not asserted-at-you: `tests/updating_without_reinstalling.rs`
builds an install with notes, a vault, peer tokens and logs in it, runs a real
update over the top, and checks every one of those files byte for byte
afterwards. It also checks the two lists — yours and shipped — never overlap,
because that property is the only thing the whole arrangement rests on.

### If an update goes wrong

Put the `.previous` binary back. Your data was never touched, so there is
nothing to restore. If you did lose `config/machine.yaml`, `atlas adapt`
rebuilds it in a few seconds.

---

## Installing on a machine for the first time

### 1. Toolchain

```
winget install Rustlang.Rustup     # Windows
rustup default stable-msvc         # Windows
```

On Linux or macOS, `rustup` from rustup.rs and the default stable toolchain.

### 2. Build

```
cargo build --release
cargo test --no-fail-fast
```

`--no-fail-fast` matters. Plain `cargo test` stops at the first failing
*binary*, so a tree with eighteen failures can report one.

Expect the whole suite green. If it is not, stop here — a failing suite on a
fresh clone is a real problem and not something to work around.

> **Windows only, first time:** `src/platform/win.rs` was written without a
> Windows compiler available. The `windows` crate moves types between
> versions, so expect signature mismatches around `HWND`, `PWSTR` and the
> `EnumWindows` callback. They are mechanical. Pin the version that works in
> `Cargo.toml` once it builds, and that is the last time anyone has to do it.

### 3. Let it look around

```
atlas adapt
```

This is the step that used to be a page of PowerShell. It finds your apps,
reads your monitor geometry, asks the system what microphones exist, and
writes `config/machine.yaml`. The shipped `config/*.yaml` stays generic —
`%LOCALAPPDATA%` placeholders, never a username — so it is the same file
everyone gets, and the machine-specific half lives separately and is never
shared.

```
atlas adapt show
```

says what it found and how much of your app list it could locate. It will
also tell you if your displays have changed since the last run.

### 4. Check it over

```
atlas doctor
```

Names anything missing — an external tool not installed, a model not
downloaded, a path that does not resolve. Work through what it says.

### 5. One app before four

```
atlas "open notepad"
```

Notepad is the easiest case: instant window, no splash, one process. If
placement is wrong here it is wrong everywhere, so fix it before adding apps.
Then `atlas "boot workspace"`.

### 6. Voice last

Not before. Debugging a window-placement bug through a speech-to-text layer
means debugging two systems at once. Typed commands first, always.
`docs/VOICE_SETUP.md` covers ffmpeg, whisper and piper.

Note that `voice` and `research` ship **off** — they are the parts that reach
the microphone and the network, so they wait to be asked for. Everything
underneath them is configured; turning them on in the hub is the only step.

---

## Known ambushes on Windows

- **Discord** launches through `Update.exe`, which exits and starts
  `Discord.exe`, so the window arrives late. `retries: 40` is a guess —
  measure it.
- **Chrome** may reuse an existing process, so `find_window` can return a
  window that already existed. Title hints help.
- **Electron apps** (Claude, Discord) create an invisible window before the
  real one. `IsWindowVisible` filters most; title hints catch the rest.
- **DPI scaling.** A 150% laptop panel next to 100% externals makes
  `SetWindowPos` coordinates wrong until the process is per-monitor-DPI
  aware. This is the most likely cause of "it places windows, just in the
  wrong place".
- **Lid closed.** Confirm the laptop panel disappears from enumeration rather
  than reporting a zero-size monitor.
