# Session — 23 September 2026: settings in Atlas's own window

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## Eric's question

*"I still don't know how to access the hub to be able to get to my settings for Atlas."*

He was right that there was no findable way to do it. On the laptop, the
hub is a web page reached only by pasting a printed address with a token in
it into a browser. By Eric's own 17 Sep ruling, the desktop surface isn't a
browser at all. And the native settings panel, the one "show me settings"
opened, was a placeholder: it said "Settings are up." and listed nothing.

## How to get to settings now

- **Laptop:** open **Atlas** from the Start menu or the desktop icon, then
  press **Settings** at the top of the window. You can also say "show me
  settings", which opens the same window on that page, or run
  `atlas home settings`.
- **Phone:** use the Atlas phone app (added from the QR code under
  **Your phone**), then open its **Settings** page. That page is the hub's
  `/hub/settings`.

## What was built

**The Settings page (`settingswin`).** The window now has two pages, Atlas
and Settings, with a button for each at the top. The Settings page lists
every setting in `settings::registry`, which is the same list the hub page
renders. It uses the same groups and group notes, and shows what each
setting does and what it costs. Each setting gets a control that fits it:

- a switch for on/off settings
- a number kept inside its own range
- a drop-down list for a choice
- a text box for a name or a list

Changes follow the same path as the hub's form:

- **How a change is kept:** `Settings::set` validates it, and `Preferences`
  writes it to `config/settings.yaml`. `tools.yaml` is never edited, so an
  update never overwrites your changes.
- **Refusals:** a value that doesn't fit is refused in words, and nothing is
  written.
- **Asking first:** turning on a sensor or outside reach (Sensitive), or
  letting Atlas do more without asking (Permission), asks once before the
  change is kept. Turning either kind *off* never asks.
- **Put back:** any setting you've changed can be put back to what Atlas
  ships with.

**Restart.** Atlas reads its settings when it starts, so after a change the
page offers **Restart Atlas now**. There was no way to stop the background
Atlas from outside it, so one was added:

- The window writes a stop request, `data/state/please_stop`.
- The daemon checks for it on every pass. It stops through the normal
  goodbye path ("Everything's written down.").
- The window waits for the port to close, then starts Atlas again.
- If the request times out, it is removed so it can't stop the next start.

**Bug found along the way.** The new round-trip test found a setting whose
shipped value was outside its own range: `models.memory_budget_mb` ships as
0 ("measure this machine") but allowed only 512 and up. Opening Settings and
changing nothing would have failed. The minimum is now 0, and the setting's
description says what 0 means.

## Proved

Tested under a headless X server with real mouse clicks (xdotool):

- Ticking **Voice** wrote `voice.enabled: on` to `settings.yaml`.
- The sensor setting (voice ID) showed the confirm prompt, and **Yes, keep
  it** wrote the change.
- **Put back** removed the change again.
- **Restart Atlas now** stopped the running daemon gracefully and started a
  new one with a new process ID. The hub answered again, and the window said
  "Atlas restarted with your changes."

`tests/native_settings.rs` covers the rest, in six tests:

- a change is kept and read back
- put back
- an out-of-range value is refused with nothing written
- only widening asks first
- every value round-trips through its control
- the stop request is removed rather than left behind

## Still in the way, named

- **Not yet run on real Windows.** This includes the Settings page and the
  restart; Wine and Linux covered the rest.
- **Smart App Control and code signing.** These are unchanged from doc 19.
- **Tailscale's HTTPS switch.** This one-time step is still needed for the
  phone.
- **Changes wait for a restart.** A change takes effect on restart, not
  instantly. The button makes that one click.
- **The mark's speaking motion is unused, and the Windows desktop overlay is
  not built.** Both are unchanged from doc 19.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
