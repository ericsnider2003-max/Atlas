# Getting Atlas onto a phone

You asked how you'd "open Atlas" when there's no app. Fair — there isn't one
yet. There are three ways to get there and they cost very different amounts.

## 1. Shortcuts — free, works today, no app at all

Atlas already serves HTTP. The Shortcuts app can record audio, send it
somewhere, and speak the reply. That's a working voice assistant with nothing
built and nothing paid.

What you get:

- **A home screen icon** that starts listening on one tap
- **The Action Button** (iPhone 15 Pro+) mapped straight to it
- **"Hey Siri, ask Atlas…"** — Siri hands the request over
- **Back tap** to launch it
- Runs anywhere the laptop is reachable — wifi, Tailscale, or a queued
  request through the cloud folder

What you don't:

- No on-device model — this is a remote control for the laptop's Atlas, not
  an Atlas of its own
- Nothing when the laptop is off
- No lock screen widget, no background anything

**This is where to start.** It's an afternoon, it costs nothing, and it tells
you whether the shape is right before anyone builds an app.

## 2. A web app — free, better interface, still limited

Atlas serves a page; iOS can add it to the home screen with its own icon and
run it full screen. Add a service worker and it works offline.

Better than Shortcuts: a real interface, conversation history, notes, capture
that queues while offline.

Still can't: run in the background, use the Action Button properly, be a share
target, or run a local model well. And Safari's microphone handling in an
installed web app is workable but not something to depend on.

## 3. A real app — the full thing, and it costs

Everything in `ON_YOUR_PHONE.md` — on-device whisper, a local model, the
document scanner, widgets, Live Activities, share sheet, notifications.

What it needs:

- **An Apple Developer account, $99 a year.** Without it you can sideload with
  a free Apple ID, but the app expires every 7 days and has to be reinstalled.
- **A Mac**, for Xcode. There's no way around this one.
- Android has neither problem: $25 once, and it builds on Windows or Linux.

---

## What I'd actually do

Shortcuts now. It's free and it answers the question that matters — whether
talking to Atlas from your pocket is worth having.

If it is, the Android build is the cheaper and more capable one, so if a friend
has an Android that's the first real app worth making. The iPhone version is
the most expensive and the most restricted, which is an annoying way round but
it's how it is.
