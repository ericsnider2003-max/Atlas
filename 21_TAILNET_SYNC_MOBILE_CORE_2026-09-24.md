# Tailnet sync, a mobile-compilable core, and a guard audit

**24 Sep 2026.** Building the two no-cost, multi-person phone paths you chose —
iOS as a PWA, Android as a native app — with everything in-house and no Mac.
Three pieces built and tested, plus a reconciliation of eight guard tests the
previous session's committed work had quietly left red. Written against the
real tree; every claim below either passed a test in this session or is named
as blocked and why.

---

## 1. Reaching a peer by address — the tailnet case (built, tested)

**The gap.** Direct device-to-device sync only fired for a peer that answered a
LAN broadcast (`nearby::look`). Two devices on *different* networks — a phone
away from home, a rack Atlas on a tailnet — were never LAN-discovered, so they
could only sync through the shared folder, slowly and only when both were next
on it. But every `elsewhere` peer already carries a stable `host`, and the tree
already has WireGuard/Tailscale. Nothing dialed that address directly.

**The fix.** `Daemon::dial_configured_peers` (in `daemon.rs`): after the folder
write and the LAN attempt, it dials each configured `elsewhere` peer straight
at `host:sync_port` and does the same one-round-trip exchange the LAN path does.
A tailnet IP (100.x) in `host` is what carries it across networks — no relay, no
server, nothing hardcoded. It skips any peer the LAN path already reached this
pass (so a device both on the wifi and named by tailnet address isn't sent to
twice), re-makes the bundle per peer so one pass can chain, and stays silent
when a peer is asleep — the folder still carries it. Offline-first intact.

Added `Elsewhere::sync_port: Option<u16>` (defaults to the fixed `SYNC_PORT`, so
every real install is unchanged) — it also frees a peer whose 8790 is taken, and
lets the test point the dial at an ephemeral port instead of racing for the one
fixed one.

**Proof.** `settings_that_do_something_now::a_peer_named_by_address_syncs_straight_across_off_the_local_network`
stands a real socket up as the peer, points the daemon at it *by address alone*
(no LAN discovery), and asserts both that the daemon reports the direct sync and
that this machine's capture actually crossed the socket. Full sync suite still
green.

**What's still `Planned`, honestly:** the `mesh` capability — *automatic*
cross-network reach (NAT hole-punching, e.g. iroh) with no hand-configured
address — is untouched and still Planned. What's built is the manual
configured-address dial, which lives under the `sync` capability. You set up the
tailnet; Atlas uses it.

---

## 2. A core that compiles without the desktop GUI (built, tested)

For Android to run the core, and for any headless build, the crate had to
compile without `eframe` (the egui desktop window — a heavy GUI stack: glow,
winit). It was a hard dependency.

Now `eframe` is optional behind a **`desktop-ui`** feature (on by default, so the
desktop build is byte-for-byte unchanged). Only three files ever used egui —
`window`, `setupwin`, `look_paint` — and only the *drawing* half of `window`
(the `run`/`App`/`paint_mark` part); `Panel`/`Contents`/`open` are pure logic
the daemon uses and stay compiled always. `window::can_open()` returns false
when the feature is off, so a headless build never tries to spawn a window — the
message falls back to the outbox, exactly as when there's no display.

**Proof.** `cargo check --lib --no-default-features` compiles the whole core with
no eframe pulled in; the default build still compiles bins + lib; the full test
suite is green. This is the load-bearing piece: the Rust core can now be
cross-compiled for a phone.

---

## 3. A platform layer for a phone (built, tested)

`platform::here()` chose the Posix (X11/Wayland) layer for anything `unix` —
and a phone *is* unix, so on Android it would have reached for a desktop window
manager that isn't there. `platform::mobile::MobilePlatform` is the honest
answer: no monitors to arrange, and launching/placing/focusing/closing other
apps' windows refused *with a reason* (the OS forbids it — the same wall
`Kind::Standalone::cannot()` names as hardware, not permission). `sleep_ms` is
real. `here()` now selects it on `target_os = "android"` / `"ios"`, carved out
of the Posix branch. Three unit tests cover it. Everything above the platform
line — daemon, capture, model, log, sync — runs unchanged.

---

## 4. The guard audit — eight tests the prior commits left red

The previous session committed `hlc.rs` and `transport.rs` but didn't run the
full `tests/all` binary or the standalone guard targets, so eight honest-
accounting guards were red in the tree I inherited. Fixed, not silenced:

- **Two dead methods, deleted.** `hlc::resuming_from` and `sync::clock_at` were
  public, called by nothing, redundant with the serde persistence the clock
  already rides on (`Clock` derives Serialize and is a field on `Log`, which the
  daemon loads). Deleted both; re-pointed the two HLC tests that used
  `resuming_from` at the *real* serde round-trip, which makes them stronger —
  they now prove the actual restart path is safe, not a synthetic constructor.
- **`transport::bind_local_ephemeral`** is genuinely test-only (production binds
  the fixed `SYNC_PORT`). Registered in `TEST_ONLY_METHODS`, `KNOWN`, and the
  `dead_capabilities` ceiling (255 → 256), each with the reason.
- **Module accounting.** `MODULES_IN_TREE` was stale (298); real is 302 after
  `hlc`, `transport`, `mobile` and one earlier-unreconciled module. `hlc` and
  `transport` are now *claimed* under the `sync` capability (they're real
  capability modules, not plumbing); `mobile` is counted as plumbing like
  `posix`/`win`/`mock`. `UNCLAIMED_MAX` moved 173 → 175 with dated notes;
  `CAPABILITIES.md` regenerated.
- **The `metrics` dependency guard** counted `[features]` keys as crates. Made
  it section-aware so feature names aren't read as dependencies.
- **A source-scan guard** (`finding_the_other_machine`) split `daemon.rs` on a
  line the transport work had changed to a tuple; updated to match, same
  invariant defended (an empty peer list broadcasts nothing).

Whole suite now green: the `all` binary (5421 passing) and every standalone
`[[test]]` target. One flake — `integrated_verification`, which spawns a child
`cargo` — fails only under full parallel load and passes in isolation; not a
regression.

---

## 5. Tested vs blocked — plainly

- **Tested green now:** the configured-address dial (real socket), the no-eframe
  core build, the mobile platform layer, the HLC serde restart path, and the
  whole reconciled guard suite.
- **Blocked on hardware (unchanged):** the actual Android NDK cross-compile and
  side-load; a real two-network tailnet sync between two devices; on-device
  model tokens/sec. None can be proven in a cloud box with no phone and no NDK.

## 6. Next

- **Commit** all of the above to the durable repo (`~/Atlas/atlas-current`) — the
  device bridge was offline this session, so it's staged and waiting.
- **iOS PWA offline finish:** extend the hub service worker (`/hub/sw.js`) to
  cache the shell and queue writes, so the iPhone PWA is a usable offline face
  over the tailnet. No Mac.
- **Android:** the NDK cross-compile + the C-ABI shell (already drafted) now that
  the core compiles without the GUI.
