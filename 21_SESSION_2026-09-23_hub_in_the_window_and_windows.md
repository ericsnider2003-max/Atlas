# Session — 23 September 2026: the hub in Atlas's window, and Windows without a certificate

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## Eric's question

*"What was the hub designed for, or the settings page in the hub designed
for, if it's not going to get used or accessible on the laptop? … that's how
everything can be tracked, things get accessed, and settings changed."*

He was right. The hub is Atlas's whole set of pages: the dashboard, the
board, projects, the calendar, what's blocked, history, activity, thinking,
health, connections, improvements, settings, permissions, accounts, access
and your devices. It was built as the one place where everything Atlas
tracks can be seen and changed. The 17 Sep ruling ("the hub should not be a
browser, you say that if Atlas has a dependency on the internet") was read
as "the laptop doesn't get the hub". That left every one of those pages
reachable only from the phone.

**His ruling (23 Sep):** the hub shows inside Atlas's own window, drawn by
the web view that comes with Windows. It isn't shown in a browser, and it
doesn't depend on the internet.

## What was built

**The hub inside the window (`hubwin`).** The Atlas window now has three
pages: **Atlas**, **Hub** and **Settings**. The Hub page shows the hub
itself as a region of the window, drawn by WebView2 (the web view that ships
with Windows 11 and Edge; version 153 was already on Eric's laptop).

- **Not a browser.** There's no address bar and no other program is started.
- **No internet.** The region only loads Atlas's own hub on this machine.
  One rule (`hubwin::allowed`) refuses everything else: other sites, other
  ports, `file:`, `https:`, a `@` or `.` after the port, and any link that
  tries to open a new window.
- **The same pages as the phone.** Nothing is drawn twice, so the laptop and
  the phone can't disagree.
- **When Atlas isn't running**, the Hub page says so and offers **Start
  Atlas**.
- **Ways in:** the Hub button, saying "show me the hub", or
  `atlas home hub [page]`. A page name that isn't a plain word can't smuggle
  an address in.

**One file still means one file (`webview2_loader`).** Reading the built
exe's import table turned up a problem before it ever reached the laptop.
The WebView2 bindings had made `atlas.exe` depend on `WebView2Loader.dll`, so
Windows would have refused to start Atlas at all without that DLL beside it.

- The bindings are now vendored with that import removed
  (`vendor/webview2-com-sys/ATLAS_VENDORED.md`).
- Atlas defines the five loader functions itself.
- On first use it writes Microsoft's own signed loader, which is carried
  inside atlas.exe, to `tools/webview2/`.
- If that fails, only the Hub page is affected, and it says why in words.

**Windows without a certificate.** Research first
([Smart App Control FAQ](https://support.microsoft.com/en-us/windows/smart-app-control-frequently-asked-questions-285ea03d-fa88-4d56-882e-6698afdb7003),
[Smart App Control for developers](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview),
[Eric Lawrence on how it evaluates code](https://textslashplain.com/2026/04/28/smart-app-control/),
[KB5083769 toggle](https://blog-en.topedia.com/2026/04/smart-app-control-in-windows-11-can-now-be-re-enabled-without-reinstalling/)).
The research separated two different checks.

- **SmartScreen** ("Windows protected your PC") only looks at files that
  carry the download mark, and **Run anyway** always works. Atlas now takes
  the mark off its installed copy (`firstlaunch::forget_download_mark`), so
  SmartScreen asks once, at the download, and never again.
- **Smart App Control** checks every piece of code. It accepts a program only
  if it's signed through Microsoft's Trusted Root Program or already known to
  Microsoft's cloud. It has no per-program exception, and a self-made
  certificate doesn't count. Unblocking the zip, building from source, or a
  signed launcher that starts unsigned code can't get past it.
  - So the honest fix without a certificate is: know its state, and switch
    it off when it would block Atlas.
  - Since the April 2026 update (KB5083769) it can be switched back on
    without reinstalling Windows.
  - Atlas reads the state from the registry (`VerifiedAndReputablePolicyState`)
    during setup. When it's **On**, or in **Evaluation** (where it may switch
    itself on later and start blocking Atlas with no warning), the setup
    window lists it first under "Worth a look" and says where the switch is.
  - `setup/reference/SMART_APP_CONTROL.md` was rewritten. The old version
    claimed unblocking the zip would "almost certainly" work, which was wrong.

## Eric's laptop, checked read-only

Checked on the laptop itself (Windows 11 25H2, build 26200.9457):

- **Smart App Control is off.** It won't block anything.
- **The zip had no download mark**, because Atlas delivered it straight to
  Downloads. SmartScreen won't even ask.
- **WebView2 153 is installed.**
- **Tailscale is running, and HTTPS certificates are already on** for
  `le3o.tail3534d2.ts.net`. That was the "one click only Eric can do" from
  doc 19, and it's already done. The phone link needs nothing more from
  him.

## Proved on the real laptop, not assumed

All of this ran in a throwaway folder under `%TEMP%`, with `ATLAS_HOME`
pointed at it. The real install location and the Start menu weren't touched.
Everything was removed afterwards, including a second throwaway folder used
for the download test.

**The background Atlas and the window**

- **First real Windows run of the background Atlas.** It started, wrote its
  settings, served the hub, and went to typing-only because the voice pieces
  weren't there yet.
- **The download mark came off.** A mark was put on the trial exe on
  purpose; after the window opened, only the file's own contents stream was
  left.
- **The loader.** It was written to `tools/webview2/`, and Windows reports its
  signature as Valid, Microsoft Corporation.

**The hub region**

- **It failed the first time, and the failure was found and fixed.** Creating
  it failed with "the parameter is incorrect". Testing each part in turn
  traced it to WebView2 being asked to take keyboard focus as it's created.
  That fails whenever Atlas's window isn't the one in front, which will be
  true often. The failure threw the whole hub away. It's now created
  unfocused.
- **It rendered.** A capture of the region, taken through the web view's own
  inspection port because the laptop was locked, showed the hub's
  dashboard.
- **It refused to leave.** Told to go to `https://example.com/`, it refused,
  stayed on the hub, and logged the refusal.

**A hub bug the laptop caught.** On arrival, the "What do you want to do?"
palette was open over the page. Its `display:flex` rule outranked the browser's
own rule for `hidden`, so it never hid, on the phone too. Fixed with
`[hidden]{display:none!important}`. The web view then confirmed the palette's
display is `none`.

**Real downloads on Windows.** `atlas get` downloaded all six pieces (about
330 MB) on the laptop with Windows' own `curl.exe` and unpacked them with
`tar.exe`, and every hash matched ("Everything's here."). None of the
downloaded files carries a download mark.

- ffmpeg 7.1.1 runs.
- `whisper-cli` runs.
- **piper spoke** "Hello Eric, this is Atlas." into a real WAV file.

## Tests

`tests/hub_in_the_window.rs` (10 tests) covers:

- the loopback-only rule, including look-alike addresses
- one way to build a hub address
- the window's page words round-trip
- the region only moves when it has really moved
- the region isn't another program, and the web view is a Windows-only
  dependency
- Smart App Control read from real `reg` output
- when it's mentioned, and where the switch is
- the download mark
- `hidden` always wins
- written-down addresses never carry the token

## Eric's upload: the hub files from the other chat

The tarball holds older copies of the hub's source and tests. The current
tree already has everything in them plus today's changes (checked line by
line), so nothing needed merging. It also holds two mock-ups:
`atlas-hub-preview.html`, which matches the current hub, and
`atlas-hub-redesign.html`, a proposed "command deck" layout. The command deck
has a warm accent, a "right now" panel, a timeline of the day, "waiting on
you", and "what I did without being asked". **The redesign isn't built.** It
would reshape the dashboard page, and now that the hub shows on the laptop
too, it would show there as well.

## Still open, named

- **A real double-click install on this laptop.** Everything was tested in a
  throwaway folder so Eric's machine was left as it was. The move into
  `%LOCALAPPDATA%\Atlas` and the Start-menu shortcuts have run under Wine but
  not yet here. That's Eric's double-click.
- **Other people's machines with Smart App Control on.** There's no way past
  it without signing. Atlas says so and says where the switch is. Signing
  (about $10 a month through Azure Artifact Signing) or the Microsoft Store
  would remove the need, and neither is required on Eric's laptop.
- **The mark's speaking motion is unused, and the Windows desktop overlay
  isn't built.** Both are unchanged from doc 19.
- **The command-deck redesign** is waiting on Eric's say-so.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
