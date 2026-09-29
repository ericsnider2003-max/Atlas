# Session — 23 September 2026 (night): your own server, over WireGuard

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
`16_SESSION_2026-09-23_pause_one_errand.md`.

Eric's ruling (B5): the personal server (his own machine, not a VPS) is
reached **over WireGuard**, not Tailscale. This sits alongside the hard rule
in doc 13 §1: personal Atlas may use that server **for higher models only**,
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## What the research changed

Two facts shaped the design. Both are sourced, not assumed:

- **An iPhone (and an Android phone) runs one VPN at a time.** Tailscale's own
  documentation: *"iOS and Android enforce a limit of running only one VPN at
  a time."* So the phone can't be on Tailscale (to reach the laptop) and on
  WireGuard (to reach the server) together. The layout had to allow for that.
  Once the server exists, the phone uses WireGuard alone and reaches the laptop
  *through the server*. The laptop joins the same tunnel and keeps it open from
  its own side. Until the server exists, nothing changes and the phone stays on
  Tailscale. This behaviour is a switch
  (`phone_reaches_laptop_through_server`), on by default. It's for Eric to
  confirm; see below.
- **WireGuard needs one UDP port the outside world can reach.** At home that
  means a port forward on the router, plus a dynamic DNS name that follows the
  home address. If the internet provider uses carrier-grade NAT (CGNAT), the
  router's outside address sits in 100.64–100.127 or doesn't match the public
  address. In that case no port can be forwarded, and the only fix is a relay
  on a public machine. Eric has ruled out using a VPS as the server, so a relay
  would be a new decision. Atlas can't see the router's outside address from
  inside the house, so this check stays with Eric, written as a step.

## What was built (`src/wireguard.rs`)

**The tunnel.**

- **Addresses.** The tunnel is `10.77.0.0/24`: the server is `.1`, the laptop
  `.2`, the phone `.3`. Atlas refuses a tunnel range that would collide with
  Tailscale's range (the laptop runs both), a range that isn't private, and
  `192.168.0/1.x`, which most home routers already hand out.
- **Keys.** The keys are made by WireGuard's own tool (`wg genkey` / `wg
  pubkey`). On Windows that's `C:\Program Files\WireGuard\wg.exe`, found
  automatically.
- **Server config.** Each device is listed as a peer that may use only its own
  address, so one device can't pretend to be another.
- **Device configs.** Laptop and phone route only the tunnel's own addresses
  through it, never all their traffic, so the laptop's Tailscale connection and
  everything else are untouched. The laptop keeps the tunnel open (every 25
  seconds) so the server can pass the phone through to it. The phone doesn't,
  to save battery.
- **Private keys.** Private keys never appear in a debug line. The configs that
  hold them are deleted by `atlas wireguard tidy` once imported.

**The fence: the server's firewall.** WireGuard decides who can reach the
server, not what they can reach on it, so Atlas writes the server's firewall
rules too:

- **Allowed.** From the tunnel, the laptop and phone can reach the model server
  on port 8080. The phone can also pass through to the laptop's Atlas on port
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  network level, so even a mistake in Atlas's own settings can't open them.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **Linux.** An nftables table of its own. A drop in any nftables table is
  final, so no other table's accept can reopen it. **Checked with real
  `nft -c`: the file parses.**
- **Windows.** `netsh` rules. On Windows a block rule beats any allow rule, so
  the fence blocks every port *except* the open ones from the tunnel's
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **Named Windows gap.** Windows Firewall can't filter by port the traffic
  Windows passes between tunnel devices. The phone-to-laptop pass-through
  relies on the laptop's own Atlas door (token, bound to its tunnel address)
  instead.

**The model door, on this side too.**

- **Where the model server listens.** New setting `models.listen_on`. On the
  server it's set to its tunnel address, so the model server listens there.
  It accepts private addresses only (`server::bind_address`); `0.0.0.0` or a
  public address falls back to this machine only. Atlas on the server then
  talks to the model server where it actually listens.
- **Doctor check.** `atlas doctor` has a new "own-server" finding. It checks
  that the model settings (`llm`, `llm_secondary`) reach the server only at its
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  model."* A setting aimed at any other port is refused with the reason.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  in `src/`.

**Commands.**

- **`atlas wireguard`:** shows the layout, the fence in words, and the five
  steps that stay Eric's.
- **`atlas wireguard configs`:** makes the keys, the three configs, and both
  fences.
- **`atlas wireguard check`:** says which devices have connected and when, by
  reading WireGuard's handshake times. On Windows it needs to run as
  administrator.
- **`atlas wireguard tidy`:** deletes the configs holding private keys.

**Proved end to end here with real WireGuard tools.** `configs` produced all
six files. Each private key derives to the public key recorded for it. Each
device's config names the server's real public key, and the server's config
names each device's. All three configs pass `wg-quick strip`, the fence parses
under `nft -c`, `check` reads correctly, and `tidy` removes exactly the three
key-holding files.

**Settings (`mesh.wireguard`):** subnet, endpoint, listen_port, model_port,
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
laptop_atlas_port. The `mesh` explanations updated with them. WireGuard's
setup now says what Atlas does (keys, configs, fence, the connection check)
and what stays Eric's (the router, the phone's import).

## What stays Eric's, in order

1. Install WireGuard on the server and the laptop, and the WireGuard app on
   the phone.
2. Forward the UDP port on the home router, and give the server a fixed
   address on the home network.
3. Get a dynamic DNS name for the home address and put it in
   `mesh.wireguard.endpoint`.
4. Check the router's outside address for CGNAT (see above).
5. Import the server's and the laptop's configs. The phone's goes in the
   WireGuard app for direct use only, and using it switches Tailscale off
   while it's on.

## Named gaps

- **The server itself doesn't exist yet.** Nothing has run over a real tunnel
  between real machines. The capability is listed as "built, never run for
  real".
- **Applying the fence is manual for now.** Atlas writes the fence files; it
  doesn't yet apply them itself. That would mean running `netsh` or `nft` with
  admin rights on the server, behind Eric's approval, and gets built once
  Atlas actually runs there.
- **The laptop's side of the pass-through isn't done.** The laptop's own
  firewall has to let the phone's tunnel address reach Atlas's port 8787.
  Atlas doesn't write that rule yet.
- **Atlas's own route-picker doesn't use the tunnel yet.** `mesh::choose`
  still gets `mesh_up = false`, so its "use the direct route" choice doesn't
  use the tunnel. Reaching the server's models works as soon as the tunnel is
  up (it's just an address). Atlas-to-Atlas over the tunnel is still the kin
  and elsewhere doors, as before.
- **CGNAT isn't handled.** If the house is behind CGNAT, a relay is needed, and
  that's a decision.

## Eric's ruling on the phone (23 Sep): the phone keeps Tailscale

Applied the same night. `phone_reaches_laptop_through_server` now defaults to
**off**, so:

- **Phone to laptop:** Tailscale, unchanged.
- **Phone to the server's models:** through the laptop. The phone asks the
  laptop's Atlas over Tailscale, and the laptop, which runs both tunnels,
  calls the server's model over WireGuard. The phone never has to switch
  anything for this.
- **Phone directly to the server:** its WireGuard config is still made, for
  the times it's wanted. Because an iPhone runs one VPN at a time, switching
  to that config takes Tailscale down until Eric switches back.
- **Tunnel routes:** each device's WireGuard config routes the server's
  address only. Nothing passes through the server, the fence has no
  pass-through rule, and the laptop no longer holds the tunnel open from its
  side.
- **The earlier pass-through layout:** kept behind the switch, and still
  tested.

Gate after the change: **30 targets, 6,174 passed, 0 failed, 0 warnings**.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE

Sources: [Tailscale — Can I use Tailscale alongside other VPNs?](https://tailscale.com/docs/reference/faq/other-vpns) ·
[WireGuard port forwarding, router, CGNAT](https://natchecker.com/blog/wireguard-port-forwarding) ·
[Hetzner — bypassing CGNAT with a WireGuard relay](https://community.hetzner.com/tutorials/bypass-cgnat-with-a-wireguard-vps-relay/)
