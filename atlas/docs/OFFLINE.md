# Offline, and the language question

## Is this full Rust?

Yes. Every line of logic in this repo is Rust — routing, policy, memory,
indexing, scheduling, lanes, retention, the websocket client, the DevTools
client. There is **no Python, no C++, and no PowerShell**. The original spec
had PowerShell as a Windows glue layer; putting the OS behind a trait removed
the need, and `scripts/*.ps1` no longer exists.

Two things worth being precise about, because "full Rust" can mean two
different things:

**Dependencies.** Four crates: `serde`, `serde_yaml`, `serde_json`,
`thiserror`, plus `windows` on Windows only. No async runtime, no HTTP client,
no websocket crate. The websocket client is ~200 lines of `std::net` in
`src/ws.rs` — pulling in a full async stack for one localhost connection would
have cost more than writing the framing.

**External programs.** Atlas shells out to ffmpeg, whisper.cpp, piper, and
Chrome. Those are not written in Rust, but they are not part of this codebase
either — Atlas drives them the way it would drive `git`. Reimplementing speech
recognition in Rust to satisfy a language purity rule would be a bad trade, and
keeping them behind config means you can swap any of them without recompiling.

---

## Offline is a guarantee, not a hope

The rule: **a local capability never consults the network.** Not to check, not
to fall back, not at all. The connectivity probe exists solely to decide
whether something that genuinely cannot work offline may start yet.

### What works with the cable pulled out

Everything except web research:

- hearing you and speaking back (whisper.cpp and piper are local binaries)
- the wake word, push-to-talk, typing
- reasoning and conversation (the default model is a local Ollama)
- all workspace control — launching, placing, focusing, closing
- screen and webcam capture
- searching your files, by name and by content
- memory, learning, approval history
- scheduling and the task queue
- the proactive engine

There is a test that lists every intent and asserts it is classified `Local`.
If that list ever shrinks, Atlas has become internet-dependent and the suite
fails.

### The three classifications

| | meaning | offline behaviour |
|---|---|---|
| **Local** | works with no network, ever | runs normally |
| **PrefersInternet** | better online, fine offline | degrades, never blocks |
| **Internet** | genuinely impossible offline | deferred and retried |

Only web research is `Internet`. Conversation is `PrefersInternet` — the local
model answers offline; a hosted one would answer better if you configured one.

### Deferral, not failure

Ask for research with no connection and Atlas says *"No connection, so I can't
research that yet. I'll do it when we're back online"* — then actually does it
when the connection returns. The job stays queued.

Two properties that took explicit work:

- **A job waiting for the network is never expired for waiting.** Foreground
  work that never gets a screen gap does time out after an hour, because that
  is Atlas hoarding. Losing a connection is not the job's fault, so the
  timeout does not apply.
- **Being offline never stalls local work.** A queued research job sitting
  there waiting does not stop "open Chrome" from running immediately. Tested.

### The probe

A raw TCP connect to `1.1.1.1:53`, 800ms timeout, cached 30 seconds. No DNS
lookup, no HTTP request, no dependency on any particular service staying up.
Cheap enough to consult every tick, and it is only consulted on the path to
`Internet` work.

Set `connectivity.assume_offline: true` for an air-gapped machine and Atlas
never probes at all.

---

## The CDP backend

Built this round: a Chrome DevTools Protocol client on top of the websocket
layer. It clicks by CSS selector, reads the DOM, fills forms, waits for
elements, and reads links — against a **headless Chrome**, so your own browser
window is untouched.

Almost everything goes through `Runtime.evaluate` rather than the Input and DOM
domains. Telling the page `querySelector(x).click()` is one round trip and hits
the right element; synthesising a mouse event at computed coordinates is three
round trips and misses when the page scrolls in between.

Details that are easy to get wrong and are tested:

- **Selectors and typed text are escaped.** `input[name='q']` cannot break out
  of the JS string, and typing `</script><script>` into a form does not inject.
- **Filling a field fires `input` and `change` events.** React and Vue ignore a
  bare value assignment; without the events the form looks empty on submit.
- **Clicking scrolls the element into view first.**
- **A missing element is a named error**, not a silent no-op.
- **Responses are matched by id.** Chrome interleaves events with replies, so
  assuming the next message is yours is a race.
- **`wait_for` polls** rather than assuming the DOM is ready after navigate.

**Not verified:** the live connection to a real Chrome. The framing, the
handshake, the escaping, and the protocol handling are all tested; talking to
an actual browser needs the laptop.
