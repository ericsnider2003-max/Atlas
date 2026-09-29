# First hour on the target laptop

> **Superseded by `docs/INSTALLING.md`.** Kept because the Windows-specific
> ambushes at the bottom are still accurate and hard-won.
>
> What changed: step 3 of this document was a page of PowerShell for reading
> your app paths, process names and monitor geometry by hand, and then
> pasting the numbers into `fake_monitors()` in `src/main.rs`. `atlas adapt`
> does all of that now and writes `config/machine.yaml`, so editing source to
> record your monitor layout is no longer a step anyone should take.
>
> `docs/INSTALLING.md` also separates installing from updating, which this
> document never did — updating Atlas does not mean reinstalling it, and
> `atlas update` will tell you exactly what it would and would not touch.

Ordered so each step unblocks the next. Do not skip ahead — step 3 is where
most of the unknowns actually live.

## 1. Toolchain (10 min)

```powershell
winget install Rustlang.Rustup
rustup default stable-msvc
cargo test        # must be 18/18 before you change anything
```

If `cargo test` passes on the laptop, the whole non-Windows core is confirmed
good on your actual machine.

## 2. Compile the Windows layer (30–60 min, expect errors)

```powershell
cargo build
```

`src/platform/win.rs` was written without a compiler. The `windows` crate
changes types between versions; expect signature mismatches around `HWND`,
`PWSTR`, and the `EnumWindows` callback. These are mechanical fixes. Pin the
version that works in `Cargo.toml` once it builds.

## 3. Capture real environment facts

Everything in `config/apps.yaml` is currently a guess. Replace each one:

```powershell
# Real executable paths
Get-ChildItem "$env:LOCALAPPDATA\AnthropicClaude" -Filter *.exe -Recurse
Get-ChildItem "$env:LOCALAPPDATA\Discord" -Filter *.exe -Recurse
(Get-Command chrome).Source

# Real process names, with each app open
Get-Process | Where-Object {$_.MainWindowTitle} |
  Select-Object ProcessName, MainWindowTitle

# Real monitor geometry — note that work area excludes the taskbar
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Screen]::AllScreens |
  Select-Object DeviceName, Primary, Bounds, WorkingArea
```

Write the monitor numbers into `fake_monitors()` in `src/main.rs` so your
dry runs match reality.

> Not any more: `atlas adapt` reads the real geometry and `atlas adapt
> fixture` prints it in exactly this shape. Nobody needs to edit source to
> record a monitor layout.

## 4. Prove placement on one app before four

```powershell
cargo run -- "open notepad"
```

Notepad is the easiest case: instant window, no splash, one process. If
placement is wrong here, it is wrong everywhere — fix it before adding apps.

Then:

```powershell
cargo run -- "boot workspace"
```

## 5. Known ambushes

- **Discord** launches through `Update.exe`, which exits and starts
  `Discord.exe`. The window arrives late. `retries: 40` is a guess — measure it.
- **Chrome** may reuse an existing process, so `find_window` can return a window
  that already existed. Title hints help.
- **Electron apps** (Claude, Discord) create an invisible window before the real
  one. `IsWindowVisible` filters most of these; title hints catch the rest.
- **DPI scaling.** If your laptop panel is 150% and the externals are 100%,
  `SetWindowPos` coordinates will be wrong until the process is
  per-monitor-DPI-aware. This is the single most likely cause of "it places
  windows, just in the wrong place."
- **Lid closed.** Confirm the laptop panel disappears from enumeration rather
  than reporting a zero-size monitor.

## 6. Only after all of the above

Voice. Not before — debugging a placement bug through a speech-to-text layer
means debugging two systems at once. Typed commands first, always.
