# What runs where

"It's Rust so it's portable" is the answer that gets people into trouble. The
logic is portable. The layer that touches a machine is not, and that layer is
about a dozen files out of 170.

## The honest table

| | Windows | Mac | Linux | iPhone | Android | Browser |
|---|---|---|---|---|---|---|
| Everything that only thinks | works | works | works | works | works | works |
| Move your windows | works | time | time* | **never** | **never** | **never** |
| Read your screen | works | time* | time* | **never** | time* | **never** |
| Act inside other apps | works | time* | time | **never** | time* | **never** |
| Wake word while closed | works | time | time | **never** | time | **never** |
| Audio in and out | works | time | time | time | time | time |
| Protect a secret properly | works | time | time* | time | time | **never** |
| Run in the background | works | time | time | catch | time | **never** |
| Read your files | works | time | time | catch | time | catch |

**time** = the platform allows it, Atlas hasn't written it.
**time\*** = allowed, with something worth knowing about.
**never** = the platform forbids it. No amount of work changes this.

## The distinction that matters

When someone asks "can you make it do X", the answer is either **time** or
**no**, and those are completely different conversations. Nine of the entries
above are walls. Everything else is work.

**A Mac and Linux have no walls at all.** Every capability is reachable; the
work is a platform layer, not a rewrite.

## The catches worth knowing

**Mac** — screen reading and acting in apps need Screen Recording and
Accessibility permission, granted once in System Settings. Keychain is the
equivalent of Windows DPAPI.

**Linux** — window management needs different code for X11 and Wayland, and
Wayland deliberately restricts what one app can do to another. There's no single
equivalent of DPAPI; it depends which keyring is installed.

**iPhone** — an app that isn't in front of you gets no microphone, and one app
cannot see or touch another. These are Apple's decisions and they are not going
to change. Shortcuts is the sanctioned way round and it only reaches what each
app chose to expose.

**Android** — the wake word works, which is the one real difference from iOS.
Screen reading and acting in apps go through an accessibility service, which the
user turns on knowing what it means.

**Browser** — a page only sees itself, and nothing in a browser can protect a
secret from the browser.

## What to hand a friend

**On Windows:** the same thing you're running.

**On a Mac or Linux:** everything that thinks, and none of the machine control
until that layer exists. A dozen files, not a rewrite.

**On a phone:** the phone shape rather than the desktop one — it asks, it
captures, it syncs. It can't watch their screen or move their windows, and
pretending otherwise would just disappoint them.

## Why this is a module rather than a note

Because a capability list that lies about what works is the drift that took
longest to find last time. `honest_about_walls` cannot be configured off, and
there's a test asserting nothing but Windows claims anything is already built.
