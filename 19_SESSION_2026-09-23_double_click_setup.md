# Session — 23 September 2026: setup you can hand to someone

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## Eric's complaint, and it was right

*"I have to ensure Atlas is in a very specific folder … a file directly next to
another … walk them through the terminal … troubleshooting … getting to the hub
(which I don't even know how to do) … we don't have a PWA … I don't know if
Atlas animations are working."*

Before building anything I checked each part of that against the code. Every
one of them was true, and the check turned up worse:

- **Folder layout.** `atlas.exe` only worked beside a `config/` folder. On its
  own it exited with an error that named no file, and the console window
  closed immediately.
- **Downloads.** The only way to get the voice pieces was `ATLAS.bat`'s
  PowerShell downloads. Checking them against the live internet showed they
  **could not have worked**:
  - whisper's "latest" release (v1.9.4) no longer ships a Windows program, so
    that download returned 404;
  - the zip downloads were quoted `'$env:TEMP\…'`, which PowerShell never
    expands;
  - nothing was checked against a known hash.
- **The hub.** It is only reachable by copying a printed URL with a token in
  it into a browser by hand. From a phone you also have to edit `tools.yaml`
  and type the address. There was no QR code, no PWA and no HTTPS. The native
  settings panel is still a placeholder.

## What was built

**One download, one double-click (`firstlaunch`).**

- All shipped `config/*.yaml` files are built into the program. A copy that
  finds no settings writes the defaults out, and it never overwrites a file
  that's already there.
- Double-clicked from anywhere that isn't already an install, Atlas copies
  itself to `%LOCALAPPDATA%\Atlas`, writes its settings, and adds itself to
  the Start menu and the desktop with the shell's own IShellLink. It then
  opens its window from there. It needs no admin rights and no choice of
  folder.
- An install that already works (a developer tree, an unzipped copy,
  `ATLAS_HOME`) stays where it is.
- It detects a double-click (the only process on its console) and closes the
  console window. The start-with-Windows task does the same, so the
  background Atlas has no window.

**Atlas fetches its own pieces (`getpieces`).**

- Six pieces, each pinned to an exact file, byte size and SHA-256. I measured
  them by downloading each one on 23 Sep:
  - whisper v1.9.2
  - the listening model
  - piper 2023.11.14-2
  - the Amy voice and its settings file
  - ffmpeg 7.1.1
- Downloads use Windows' own `curl.exe` and zips are unpacked with Windows'
  own `tar.exe`, both called by full path from System32. Downloads resume if
  interrupted. A file that doesn't match its hash is thrown away and reported
  in words.
- The sound tools stay in Atlas's own folder and go on Atlas's own search
  path. Nothing is installed system-wide.
- `atlas get` does the same thing from a terminal. ATLAS.bat's setup now just
  calls it, and its broken PowerShell zip code is gone.

**Atlas's own setup window (`setupwin`).** The setup is a native window, not
a browser, per the 17 Sep ruling. It walks the steps and shows each one's
state as it goes: a home, each piece with a progress bar and "checking it's
the right file", getting to know this computer, and checking everything.
- **Worth a look:** doctor's findings, but only the ones you can act on.
  Atlas's own settings backlog isn't yours, apps you don't have are grouped
  into one line, the optional camera features are one line, and a missing
  voice piece isn't reported twice.
- **Controls:** a button to try the unfinished steps again, **Start Atlas**,
  and **Start Atlas when I sign in**.
- **Your phone:** a QR code, or one sentence on what's missing.
- **The mark:** the mark sits at the top. It draws in when the window opens,
  moves in the slow thinking wave while work runs, and goes still when done.

**The phone as an app.**

- **Getting the link (`phonelink`).** Atlas runs `tailscale serve` itself, so
  the hub reaches the phone over HTTPS at the laptop's tailnet name while the
  server stays on loopback. It reads the name from `tailscale status`, and
  classifies failures (HTTPS certificates not enabled, not running, not
  installed) into the one thing to do. The QR code is drawn from the link;
  `atlas phone` / `atlas phone off` do the same from a terminal.
- **Installing it (PWA on the hub).** The hub now serves a manifest (standalone
  display; its start_url carries the token, because an iOS home-screen app
  gets its own cookie jar), plus a service worker, icons drawn from the mark,
  and the Apple meta tags.
- **Offline.** The service worker stores nothing on the phone, as
  `server_safety` requires. When the laptop can't be reached, it shows one
  sentence and reloads until it can.
- **Your devices page.** It shows the same code, for adding the iPad.

## Proved, not assumed

- **Linux, headless X server, real downloads.** Double-clicking a copy in
  `Downloads/` moved it into its home and added an applications-menu entry. It
  then downloaded all six pieces from the real internet, checked every hash,
  unpacked the zips into `tools/`, ran adapt and doctor, and showed "Atlas is
  ready". Pressing **Start Atlas** (by a real mouse click) started the
  background Atlas, and the window switched to "Atlas is running".
- **The live hub.** It served the manifest with the token (200), refused a
  wrong token (401), served the worker and icons without a token, put the
  manifest link, Apple tags and worker registration on the page, and showed
  the phone block on Your devices.
- **The Windows build under Wine.** Run from a Downloads folder, it moved into
  `C:\users\…\AppData\Local\Atlas`, wrote real `Atlas.lnk` files to the Start
  menu and the Desktop through COM, ran adapt through the Windows platform
  layer, and drew the window. The downloads failed there only because Wine
  has no `curl.exe` (Windows has shipped one since 2018). The message now says
  that in words.
- **Test suite:** `tests/easy_setup.rs`, `tests/phone_app.rs` and
  `tests/phone_link.rs`.

## Animations (the mark)

The "animations" are **the mark**: Atlas's line, a hairline cable that hangs
still when idle, moves in one slow wave when thinking, draws in on waking, and
should move when speaking.
- **Seen working on screen this session.** It draws in and waves in the setup
  window, in screenshots.
- **In the panels:** it was already wired into the brief and "How I got there"
  panels.
- **Never used:** the *speaking* motion (nothing measures an audio level).
- **Never built for Windows:** the desktop-overlay version.

## Still in the way, named

- **Smart App Control.** An unsigned `atlas.exe` is blocked outright where
  it's on; the usual SmartScreen warning just needs "More info → Run anyway".
  The fix is a code-signing certificate, which is a cost decision.
- **Tailscale's HTTPS switch.** One click in the Tailscale admin console,
  once, and only Eric can do it. Atlas detects when it's off and says where the
  switch is.
- **The native settings panel is still a placeholder.** On the laptop,
  settings still means the typed prompt or the hub page. This is next.
- **Not yet run on real Windows.** The real double-click (console detection)
  and the real `curl`/`tar` downloads have not run on Eric's laptop. Wine
  covered the rest.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE

Sources: [Tailscale serve](https://tailscale.com/docs/reference/tailscale-cli/serve) ·
[Tailscale HTTPS certificates](https://tailscale.com/docs/how-to/set-up-https-certificates) ·
[iOS home-screen apps don't share storage with Safari](https://bugs.webkit.org/show_bug.cgi?id=181849) ·
[Sharing state between Safari and an installed PWA on iOS](https://www.netguru.com/blog/how-to-share-session-cookie-or-state-between-pwa-in-standalone-mode-and-safari-on-ios)
