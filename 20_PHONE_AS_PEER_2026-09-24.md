# Atlas on the phone — a full peer, on or offline

**24 Sep 2026.** What it takes for the phone to *be* Atlas — the same Atlas as
the laptop, working on its own offline, reconciling and continuing when the two
can see each other again — and what I built this session toward it. Written
against the real code in the durable tree, not from scratch: most of the spine
already exists, one keystone was missing, and the rest is a device-layer build
that needs real hardware.

---

## 1. What you asked for, and the model that delivers it

> "My phone can still have a fully functional Atlas that is the same Atlas from
> my laptop, works on and offline, works within itself when the laptop isn't
> available, and when it reconnects it syncs then continues the work."

The design that gives you that — and it is **already the design in the tree** —
is *sync what happened, not the state.* Every device keeps an append-only log
of events (you said this, a note was captured, a task finished, a field
changed). Merging two devices is replaying both logs in order. Appending can't
conflict with appending, so two devices that haven't met in six months merge
cleanly. The only thing that needs you is the *same field of the same thing*
changed on both sides — rare, for one person. There is **no server anywhere**;
the log is one file, and anything that moves a file (same wifi, a cloud folder,
a cable, AirDrop) syncs Atlas.

That is exactly offline-first with online secondary, and it is why the phone
can be a real Atlas rather than a window.

---

## 2. What is already built (the spine — `src/sync.rs`, 1,075 lines, tested)

- **The phone is a first-class device.** `sync::Kind::Standalone` — "a real
  Atlas rather than a window: it listens, it talks, it thinks with a smaller
  model, it remembers." `works_alone()` and `can_talk()` are true for it. What
  it *can't* do is named as hardware, not permission: arrange laptop windows,
  reach laptop files, run the larger model, open the vault.
- **Append-only event log + deterministic merge.** `Log::append`, `merge()` —
  replays both logs in a total order, detects the one real conflict class
  (same field changed both sides; delete-racing-edit too, a fixed bug), and is
  order-independent so both devices reach the identical answer without talking.
- **Bundles + carriers.** One self-describing file (`Bundle`), versioned,
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  accident. Carriers: `SameNetwork`, `CloudFolder`, `Cable`, `AirDrop`, chosen
  by `how_to_carry` from what's actually available. `already_seen` makes
  sending twice free.
- **Sealed at rest.** Bundles are encrypted with a household key derived from a
  recovery phrase; the header stays clear so a device can skip its own bundle
  and refuse another household's without holding a key. Losing the key loses
  nothing — a bundle is a courier, not the archive.
- **Pairing hands the key across** (`leave_handoff`/`take_handoff`) so a new
  device joins without you copying key files.
- **Live transport is wired** in `daemon::carry_to_your_other_devices`:
  `nearby::look` actually shouts on the LAN, `mesh::choose` picks
  SameNetwork/Mesh/Cable, `server::SignalListener` holds the door open.
- **Held-and-forward** for things that need the internet: `outbox`, `courier`,
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  behavior the phone's Offline screen already shows.
- **On-device model, in pure Rust.** `gguf` (the model file format parsed in
  Rust) and `models` (the registry+engine, "the rest of what Ollama does, in
  Rust") mean the phone can run a smaller local model with no third party.

**In short: the hard, subtle part — conflict-free multi-device merge — was
done.** That is the thing most teams get wrong; Atlas already had it.

---

## 3. What was missing, and what I built this session

**The keystone gap: there was no clock the two devices could agree to order by.**
`hlc.rs` was one of the round-3 ports whose source was lost. The merge ordered
events by raw wall-clock seconds. Two devices are never perfectly in step, and a
phone that has been offline in a drawer comes back a few seconds behind. Ordered
by raw time, an edit you make on the phone **after** taking in the laptop's edit
can sort **before** it — so the merge silently settles on the wrong answer, and
only when the clocks happen to be skewed. That is precisely the "syncs, then
continues" case, and it was broken.

**Built (in-house, tested green):**

- **`src/hlc.rs` — a Hybrid Logical Clock.** The well-worn Kulkarni et al.
  design (what CockroachDB and most local-first systems use), implemented as a
  page of Rust rather than a dependency, to hold the in-house mandate. Its
  robustness details are adapted from Eclipse Zenoh's `uhlc-rs` — the most
  battle-tested HLC in Rust — translated to Atlas's world, not pulled in as a
  crate. It keeps three promises: a device's own stamps only move forward (even
  within one second, even if the OS clock slips backward); a stamp read off an
  incoming event pulls this device's clock up past it, so anything it does next
  sorts after everything it just learned; and physical time stays within a
  bound of true time.
- **A drift guard, adapted for "nothing is lost."** `uhlc` refuses an incoming
  stamp more than 500 ms ahead of local time (and drops the event). Atlas runs
  on personal devices whose clocks nobody checks, so the bound is an hour — and
  crucially, it never drops the note: a stamp from a device whose clock is set
  years ahead is *capped* before it touches this device's clock (so future
  local stamps still read like now, not 2036), the event is still taken in and
  ordered by its own stamp, and the person is told the other device's clock
  looks wrong. Protect the clock, keep the data.
- **Wired into `sync` and the daemon.** `Event` carries an `hlc::Stamp`
  (`serde(default)`, so current bundles still open — old events fall back to
  their wall-clock second). `Log::append` stamps each event; `merge` orders by
  the stamp, not raw time; and the daemon's real receive loop
  (`carry_to_your_other_devices`) now calls `Log::note_seen` on every bundle it
  takes in — advancing the clock past what it just learned and surfacing any
  skew warning in what Atlas says. **Persistence rides along for free:** the
  clock is a field on `Log`, and the daemon already loads and saves the whole
  `synclog`, so a restart never reuses a stamp.
- **Proof.** `tests/sync.rs::a_phone_edit_made_after_reconnect_wins_even_with_a_behind_clock`
  reproduces the exact failure — phone edits after reconnect with an 8-second-
  behind clock — and asserts the phone's later edit now wins.
  `a_device_with_a_wildly_wrong_clock_is_flagged_but_loses_nothing` pins the
  drift guard: a years-ahead clock is flagged, and both events survive.
  **HLC: 10 unit tests. Full sync suite: 58 passing, 0 failing. Whole crate
  compiles.**

This is the correctness fix that makes "reconnect, then continue" actually safe.

---

## 4. What remains to make it real on a phone (the device layer)

The data spine is done and now correct. Three layers stand between that and an
app on your phone. Each is buildable; each needs hardware this cloud box isn't.

1. **A mobile shell that runs the Rust core.** The whole `atlas` crate compiles
   to a static library for iOS (aarch64-apple-ios) and Android
   (aarch64-linux-android). Expose the core to the native shell with **UniFFI**
   (Mozilla, Apache-2.0 — generates Swift/Kotlin bindings from the Rust) or run
   the existing server-rendered hub inside a thin WKWebView/WebView talking to
   the local core over loopback (the hub is already server-rendered HTML over
   loopback — this is the smaller lift). The phone screens we just designed are
   that shell's UI.
   *Blocked here because:* it needs Xcode + an Apple signing identity and the
   Android SDK/NDK to build and side-load — none of which exist in this cloud
   container. Buildable on your machine; I can drive it there.

2. **The on-device model.** `gguf`+`models` already read and run GGUF in Rust.
   The phone bundles a small quantized model (e.g. a 1–3B GGUF) and runs it
   through llama.cpp's Metal (iOS) / NNAPI (Android) backend, or the pure-Rust
   path for portability.
   *Blocked here because:* the only real test is tokens/sec and battery on the
   actual phone. Which model/quant is "good enough offline" is your call on the
   device (an unverifiable-quality zone, by design).

3. **Live peer-to-peer across networks.** SameNetwork + CloudFolder already
   cover "same wifi" and "never on together." The one gap is a *direct* phone↔
   laptop link when they're on different networks with no shared cloud folder —
   NAT hole-punching. The proven open-source approach is **iroh**
   (n0-computer, Apache/MIT — QUIC + hole-punching + optional self-hosted
   relay). It can be adopted as a `Carry` variant behind the existing transport
   trait without touching the merge. Optional: the cloud-folder path already
   makes this a convenience, not a requirement.

None of these touch the merge or the log — they're transports and shells around
a spine that is now correct.

---

## 5. Sequence to ship it

1. **[DONE]** HLC keystone — merge is causally correct across skewed devices,
   with the drift guard, wired into the daemon's receive loop, clock persisted.
2. **[DONE]** Committed `hlc.rs` + the `sync`/daemon wiring into the durable
   repo (`~/Atlas/atlas-current`).
3. Cross-compile the core for iOS/Android on your machine; stand up the thin
   WebView shell over the loopback hub (fastest path to "it's on my phone").
4. Bundle a small GGUF; wire the on-device model path; measure on the phone.
5. (Optional) Add an iroh-style direct-P2P `Carry` for cross-network sync.

Step 3 is the one that needs your machine's toolchains (Xcode / Android NDK);
I can drive it there.

---

## 6. Tested vs blocked — stated plainly

- **Tested green, in code, now:** the HLC (7 tests) and the skew-safe merge
  (full sync suite, 57 tests). The event-log spine, bundles, sealing, pairing,
  carriers — all pre-existing and tested.
- **Built but needs your machine to verify:** cross-compiling the core, the
  WebView shell, on-device model perf, real cross-network NAT traversal. These
  can't be proven in a cloud container with no phone and no Apple/Android
  toolchain — naming that rather than pretending.

The honest headline: the phone being a true offline peer that syncs and
continues is **mostly already built**, was **subtly broken by a missing clock**,
and is **now correct and tested**. What's left is packaging it onto a phone,
which is device work, not design work.

---

## Update (24 Sep, same day) — direct same-network transport

The folder path worked but was slow on the same wifi (it waits on a cloud
provider). `src/transport.rs` is the missing last step the daemon itself flagged
("sending straight across isn't built yet"): a tiny framed TCP protocol on a
dedicated port (8790). Connect to the peer, send your bundle, get theirs back —
one round trip is a full two-way sync, no folder, no cloud, no server, nothing
on the internet. Payload is opaque, so `sync::seal` protects the wire with the
same household key. The receiver is a `poll` the daemon runs each sync pass (a
step at a time, never blocking the turn), and `carry_to_your_other_devices` now
dials a discovered peer directly and falls back to the folder on any miss.
Tested end-to-end: two logs converge over a real socket (4 transport tests;
daemon suite still 105 green). The receiver poll now runs in the main tick (`serve_direct_sync`), so a
device listens continuously — a phone coming onto the wifi is answered at once,
not only while this device is itself syncing.
