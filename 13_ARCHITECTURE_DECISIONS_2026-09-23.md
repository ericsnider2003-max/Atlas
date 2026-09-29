# Architecture decisions — reach, isolation, and single-errand control

**23 September 2026.** Eric's rulings from the decision-list pass, recorded
because they are durable constraints that shape reach, the model path, and the
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
corrected spec.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
personal server:

- When Eric has his own personal server (NOT a VPS) and chooses to run
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

**How this maps to what's built.** The local-LLM path already keeps these
separate, so honouring the rule is a matter of *what a config points at*, not
new mechanism:

- Higher-models-on-the-server is the `tools.llm_secondary` slot (the
  `FallbackLlm` secondary) — or `tools.llm` — pointing at a plain llama.cpp
  `/completion` endpoint on the server. It is a pure model endpoint: text in,
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  separate crate), reached the way it already is. Personal Atlas has no
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

**Guard-able invariant to add when the server exists:** the model endpoint
config must be a bare completion URL/tool, and nothing in personal Atlas may
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
enforced rather than trusted.

## 2. Reach: Tailscale for devices, "another way" for the server (B5)

- **Phone ↔ laptop: Tailscale.** Personal Atlas reaches the phone over
  Tailscale; that is the device-to-device path.
- **The personal server is reached another way** — explicitly *not* Tailscale.
  The mechanism is Eric's to choose later; recorded here so the two peer doors
  in the tree (`server::with_peers` and the separate `SignalListener`) are not
  wired until that choice is made. **B5 stays unwired pending that decision** —
  wiring either door now would bless a design by accident.
- Consequence for when the server lands: phone → laptop (Tailscale) and
  phone/laptop → server (another way) are distinct reach paths, and the
  server's model endpoint (§1) rides whichever the "another way" is, still as
  a pure model door.

### Update, 23 Sep (night): "another way" is WireGuard

Eric chose WireGuard. Built in `src/wireguard.rs` (doc 17): the tunnel, the
server's firewall fence (devices reach the model port and nothing else;
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
`atlas doctor`, and `models.listen_on` so the server's model server listens
on its tunnel address. The research found that an iPhone runs one VPN at a
time. **Eric ruled that the phone keeps Tailscale.** It reaches the laptop
over Tailscale and the server's models through the laptop. Its own WireGuard
config is only for reaching the server directly, by switching to it by hand.
`server::with_peers` stays unwired: the tunnel is WireGuard at the OS level,
and Atlas's own doors are unchanged.

## 3. Single-errand control is PAUSE, not stop (B1, corrected)

Eric's correction: the single-errand control should **pause an errand and
preserve what it is doing**, not stop-and-discard it. This is more than the
existing `crew::ask_to_stop` (which sets a stop flag and lets the errand end).

**What that needs (spec, not yet built):** a crew errand today runs a closure
on a thread that checks a stop flag and finishes. Pause-with-preserved-state
means an errand must be *resumable* — able to yield its progress and be
re-entered — which the current fire-and-settle model does not support. So this
is a real feature (resumable errands), not a wire of `ask_to_stop`. Recorded
as such; `ask_to_stop` stays on the orphan list until the resumable-errand
model exists, because wiring it would deliver stop-and-discard under a name
Eric has said he does not want.

---

## Status of the wider decision list (23 Sep)

Wired and green this pass: 7 of 9 trading primitives; B2 (opportunity
surfacing per Eric's spec); B3 (reminder/schedule intent); B4 (cloudsync
provider-compare). Deferred with reasons: the 2 remaining trading primitives
(`events::is_window`, `bars::back_to`) need a new consumer, not a wire; B6
`idle_but_on` is drafted and contained (next pass); B6 `cdp::links` needs the
research-browser cost decision; B6 `otherside::needs_evidence` needs the
"argue the other side" intent built first; B1 was built on 23 Sep (doc 16) and B5 is
WireGuard, built the same night (doc 17).

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
