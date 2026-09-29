# Atlas across everyone's devices — updates, the plugin boundary, notifications, widgets

**25 Sep 2026. Spec, being built in steps — §20 is the build log and says exactly what exists.** The consolidated record of everything decided about running
Atlas for Eric and his friends across phones and desktops, so nothing discussed is lost. Eric's frame:
*"Atlas is fully customizable and built upon by my friends. Send updates through Atlas over the chats —
phone and desktop — without anyone deleting/reinstalling, and without wiping what they built (hub,
preferences, added capabilities, integrated software). I host nothing; everyone runs their own Atlas.
I supply updates. It has to be simple. The plugin boundary has to be airtight — no loopholes."*

Mandate throughout: in-house, minimal third-party, offline-first / online-secondary, no central host.
Updates ride the same peer-to-peer mesh Atlas already uses for sync; each person's own Atlas applies its own.

---

## 1. Invariants (all device types)

1. **No reinstall, ever** — phone or desktop, every OS. Updates replace the engine in place.
2. **No data loss.** Log, vault, machine config, notes, memory — untouched by an update.
3. **No customization loss.** Hub layout, preferences, enabled capabilities, integrations, and **friend-added plugins** survive every update (§4, §5).
4. **No central host.** Signed builds propagate device-to-device; each node applies its own install locally.
5. **Authorship proven.** A device installs only what verifies against Eric's release public key (§7).
6. **Offline-first.** An update reaches someone when their device next appears (held-and-forward). No one has to be online at once.
7. **Least authority.** A plugin gets only what it declares and the person approves — no ambient access (§5).
8. **Simple.** For the receiver: a notification, a tap, done.

---

## 2. Two signatures, two jobs — never conflate

- **Atlas release signature (new, in-house).** Eric signs each release manifest with a private ed25519 key; every Atlas verifies against Eric's public key (baked in) before staging. Proves authorship across the mesh, no shared secret. New dependency: `ed25519-dalek` (§12).
- **OS install signature (platform tooling, existing).** Android keystore; iOS distribution cert + ad-hoc profile; desktop just replaces the file. Created once, reused per build (§13). Lets the OS do an in-place install and preserve data.

Both needed, independent. A build carries both. Platform signing keys live only on Eric's build machine.

---

## 3. What already exists to build on

`upgrade.rs` (+ `tests/updating_without_reinstalling.rs`) — in-place desktop swap, `SHIPPED` vs `YOURS`,
`keep_old_at` rollback, `version()`. `transport.rs` — the wire. `sync.rs`/`courier.rs` — sealed versioned
payloads with held-and-forward. `capability.rs` — the registry. `nearby.rs`/`mesh.rs` — discovery/route.
Chat mesh (group chats) exists; **chat roles are the new addition** needed for the release channel (§8).

---

## 4. What survives an update — customization safety

Three layers, each handled:

- **Layer 1 — data & preferences** (hub layout, settings, state, notes, memory): in the `YOURS` set (`config/machine.yaml`, `data/`), never touched by an engine swap. **Requirement:** preferences live in a *user-overrides* file, never a shipped default; when a new engine ships changed defaults, the courier does a **three-way merge** — new defaults land *underneath* the person's overrides, never over them.
- **Layer 2 — integrated software** (connectors, credentials, linked services): in their data + sealed vault. Untouched.
- **Layer 3 — added capabilities**: handled by the plugin boundary (§5), which is the mechanism that makes "fully customizable + central updates that never wipe" actually hold.

---

## 5. The plugin boundary (airtight by design)

**Goal:** a friend's added capabilities are never wiped by an update and can never become a security hole.
The way both are true at once is hard separation plus least authority.

### 5.1 Three hard-separated things
- **Engine** — the Atlas binary. Eric-signed, replaced on update. Contains no per-person capability.
- **Plugins** — per-person added capabilities. Live in the `YOURS` set (e.g. `data/plugins/`), so an engine swap never touches them; the engine *re-attaches* them on start.
- **Data** — per-person state the plugins and engine read/write.

An update replaces only the engine. Plugins and data persist and re-attach. This is the whole anti-wipe guarantee.

### 5.2 Two tiers of plugin (open a code surface only when forced)
- **Tier 1 — declarative (default, safest, no code execution).** A capability expressed as *data the engine interprets*: a flow, a saved routine, a prompt/skill, a connector definition, a composition of existing engine primitives. There is **no arbitrary code** here, so the largest loophole class simply does not exist. Most of what friends add should be expressible this way. Ship this first.
- **Tier 2 — sandboxed code (only if a real need appears).** Genuinely novel logic runs in a **capability-restricted WASM sandbox** (pure-interpreter runtime, e.g. `wasmi` — no JIT, so it is iOS-safe). A plugin is a `.wasm` module in the `YOURS` set. It has **no ambient authority**: it can call only the host functions its manifest declares and the person approved, with resource limits, isolated so a crash can't take the engine down. Designed now so it can slot in later without redesign; **it comes only after Tier 1 proves insufficient.**

### 5.3 Tier 2 in depth — what it is and how it's used
- **When you reach for it.** Tier 1 composes existing engine primitives; it can't express *novel computation* — a bespoke algorithm, a proprietary parse, a custom decision rule, a transform no primitive covers. That's Tier 2.
- **What a plugin is.** A small module compiled to WebAssembly (from Rust, or any WASM-target language) that exports functions the engine calls at defined **hook points**: a new tool/capability Atlas can invoke; a transform step inside a Tier 1 flow; a trigger evaluator ("fire when this custom condition holds"); a projection/renderer for a custom panel or widget. It ships as a `.wasm` file in `data/plugins/` (the `YOURS` set), so updates never touch it.
- **How it runs.** Loaded into a `wasmi` sandbox with **no ambient authority** — it can call only the host functions its manifest declares and the person approved (§5.3), under memory + fuel limits, isolated so a crash or hang can't take the engine down. It cannot see the filesystem, network, other plugins, or engine internals except through those mediated calls.
- **Calling back into Tier 1 / the engine (and staying fast).** A Tier 2 plugin is meant to be a *thin brain over native muscle*: the novel decision/orchestration logic runs in WASM, and everything heavy — running the model, hitting a feed, reading the store/vault, the mesh, and **invoking Tier 1 capabilities and flows by name** — runs in the native engine at native speed, reached through the mediated host API. So a plugin gets *full use* of what Atlas can already do without re-implementing it, and net performance stays essentially native because WASM only does the glue. Two rules make this hold: (1) **coarse-grained calls** — cross the sandbox boundary at the level of a whole capability ("summarize this document," "run my X flow"), never in a tight inner loop, since each crossing has a small marshalling cost that's negligible when coarse and deadly when chatty; (2) **calling a capability is itself a permission** — a plugin may invoke only the Tier 1 capabilities/flows it declared and the person approved, so "full use" means full use of what it was granted, and cross-plugin orchestration can't happen silently (this is the privilege-escalation defense in §5.5). Safety is unchanged: the call goes through the same permissioned host API, the plugin never gets native pointers or ambient authority — it asks the host to run capability X, the host checks the grant, runs it natively, returns the result.
- **Who authors it.** Anyone who can produce WASM — Eric, a technical friend, or **Atlas itself**: a non-coding friend asks Atlas for a capability, Atlas writes the plugin, and the sandbox means even Atlas-generated code is confined to approved permissions. This is how "fully customizable by friends" holds without every friend being a programmer.
- **The use flow.** author module against the plugin API → declare needed capabilities in its manifest → compile to `.wasm` → install (drop in the dir, or receive over the mesh and explicitly enable) → approve the capability list → Atlas loads it sandboxed and runs it at its hooks → it survives updates, re-verified on load.
- **Worked examples.** (a) *Trading:* a plugin that takes a market feed through an approved host call and runs Eric's own signal/risk rule — bespoke logic, sandboxed, no network beyond the granted feed. (b) *A friend's custom capability:* parsing their gym's schedule format or computing their own habit score — logic only they need, riding in their `YOURS` set. (c) *Custom integration:* a Tier 1 connector fetches raw data; a Tier 2 function reshapes it with bespoke rules.
- **Honest limits.** Interpreted WASM is slower than native (fine for logic, not heavy ML); it is a real code-execution surface, so it stays in reserve until Tier 1 proves insufficient; authoring needs code (or Atlas writing it); it adds the `wasmi` dependency only when built.

### 5.4 Capability-based permission model (the anti-loophole core)
Every plugin ships a manifest naming exactly the host capabilities it needs (e.g. "read calendar," "post to a chat," "read notes tagged X"). On install the person sees and approves that list; grants are **per-plugin, least-authority, and revocable**. There is no way for a plugin to reach the filesystem, network, other plugins, or engine internals except through a declared, approved, mediated host call. This is the same shape as OS app permissions, enforced in-process.

### 5.5 Gap analysis — the loopholes, each with its defense
- **An update wipes a plugin.** Prevented: plugins live in `YOURS`; the `upgrade` check treats `data/plugins/` as sacred; the release manifest never references plugin paths; three-way merge for any engine-shipped default a plugin overrode.
- **A malicious or broken plugin harms the device.** Prevented: Tier 1 executes no code; Tier 2 runs in the WASM sandbox with only approved host calls and resource limits; no ambient authority; isolation so a crash is contained.
- **A plugin is silently swapped for a malicious file.** Prevented: the engine records the identity (content hash, and author signature if shared) of each installed plugin and **re-verifies on load**, rather than blindly loading whatever is in the folder.
- **The mesh injects a fake plugin or a fake update.** Prevented for updates by the release signature (§7). A plugin arriving over the mesh is **never auto-installed** — it is offered with its author's identity and requires the person's explicit enable and permission grant (no silent capability gain).
- **Privilege escalation (plugin→plugin or plugin→engine).** Prevented: the mediated host API is the only bridge; grants are per-plugin; plugins cannot enumerate or call each other.
- **Downgrade / replay of an old signed build with a known bug.** Prevented: the manifest carries a monotonic version; the engine refuses an update at or below the installed version unless the person explicitly rolls back to a `keep_old_at` build.
- **Release-key compromise.** Mitigated: the private release key lives only on Eric's build machine, ideally offline; a **signed key-rotation** path lets the baked-in public key be replaced by a message signed with the current (or a cold backup) key. Named as hardening.

---

## 6. Update delivery & apply, per device type

All paths: verify release signature → check manifest min-compatible data version vs local log → `upgrade`-style
safety check → apply → keep previous for rollback where allowed. Data + plugins + customization preserved (§4–5).

- **Desktop (Windows/Mac/Linux, friends included).** Courier delivers Eric's signed desktop artifact; verify; `upgrade` swaps the binary in place; restart. **No delete/reinstall — for friends' desktops exactly as for Eric's.** Can be fully automatic (§8).
- **Android.** Receive signed `.apk` over the mesh → verify → prompt → `PackageInstaller`. Same keystore ⇒ in-place, data kept. "Install unknown apps" granted once. No host.
- **iOS.** The person's **own** core serves an OTA `itms-services` manifest + verified `.ipa` to their own iPhone over **their own tailnet** with a Tailscale HTTPS cert (Safari-trusted, so no central host). Tap the link → in-place update, data kept (same bundle ID + ad-hoc profile). Fallback: plug into their own computer, Sideloadly/AltStore.

---

## 7. One release, every platform — manifest & key custody

A **release** = one signed manifest listing one artifact per platform (win/mac/linux/android/ios), each
OS-signed at build time with its own platform identity, plus **one** Atlas release signature (Eric's ed25519)
over the whole manifest. Eric posts the manifest once; **each device pulls the artifact matching its own
platform** and verifies the single release signature. So one push updates everything at once, and:
- Platform signing keys (Android keystore, iOS cert, desktop codesign) never leave Eric's build machine.
- Friends hold **no** signing secrets — only Eric's *public* release key, baked into every Atlas for verification.
- The manifest carries per-artifact hashes + the monotonic version (feeds the downgrade defense, §5.4).

---

## 8. The release channel — group chat with roles as the update trigger

A dedicated **release channel** in the chat mesh, with the new **chat-role model**: Eric holds a `publisher`
role; everyone else is `subscriber` (read-only — they cannot post). Each person's Atlas **watches** the channel;
a new signed manifest posted there triggers the courier automatically.
- **Auto-update behavior:** desktop can be fully automatic (detect → verify → swap → restart, rollback kept),
  gated by a per-person `auto_update` preference (default on, always overridable). Mobile is automatic up to the
  OS-required tap (detect, download, verify, notify with one-tap apply).
- **Security:** roles are ergonomics; the release signature is the guarantee. A spoofed channel post without
  Eric's private key fails verification and is never installed.
- **Chat roles are a general addition**, not just for releases: owner/publisher/subscriber (and per-channel
  read/post/manage) is the same model group chats need anyway. Named here because the release channel is its
  first hard requirement.

---

## 9. In-house notifications — V1 LOCKED (iOS + Android)

**Decided:** notifications are in-house and **user-toggleable by category** — a settings surface of notification
types (reminders, task-done, a group-chat message, something Atlas noticed, a sync/skew warning, an available
update). Categories reuse the desktop panel logic (`window::Panel`/`Contents`, `notify`), so the phone surfaces
what the desktop already decides is worth surfacing.
- **Android — fully in-house.** The running core (foreground service) posts notifications itself. **No Google/FCM.** Toggles honored locally.
- **iOS — local, in-house (V1).** The on-device core posts **local notifications** (no Apple service) for everything V1 needs. Toggles honored locally.
- **Wake notifications — owner-only capability (decided).** Waking a *fully-closed* iOS app needs APNs, whose push key + entitlement are tied to the developer account that **signed the app** — Eric's. This is modeled as an **owner-only capability**, gated by APNs-key custody, not by the OS:
  - The push *entitlement* is in every ad-hoc build (same Team), so the entitlement alone doesn't separate Eric's install from a friend's. What separates them is the **sender side**: a wake-push needs the APNs auth key (`.p8`) *and* a registered device token. Only Eric holds the auth key (kept with him, like the signing keys, never distributed), and by default only Eric's phone token is registered with a sender (Eric's own core, which hits Apple's APNs endpoint directly — Apple is only the required relay).
  - So wake-push fires only where key + registered token both exist = Eric's core → Eric's phone. A friend on the identical build never receives one (no sender points at them, their token isn't registered) and silently uses V1 local notifications. It's a config feature that no-ops without an auth key, so a friend toggling it does nothing.
  - Optional per-person extension: Eric *can* register a specific friend's token with his sender to extend wake-push to them — an explicit opt-in Eric controls, never a default.
  - Honest caveat: "owner-only" is enforced by key/token custody, not an OS per-copy lock (all ad-hoc builds share the Team entitlement). That's a solid boundary — no `.p8`, no push — just not mistaken for the OS distinguishing copies.
  - **Net:** V1 stays local-only for everyone (zero setup); wake-push is an optional owner-only add-on Eric switches on for himself without touching anyone else.

---

## 10. Widgets — design

Native only (a PWA can't put a real widget on an iOS home screen), so widgets are a reason the native path earns its place. Design:
- **What they show (read-only glances, no secrets):** (a) *Now/Next* — the next thing on the calendar or the top waiting item; (b) *Count* — how many things are waiting (mirrors the desktop Outstanding panel); (c) *Status* — synced/offline and last-sync age; (d) *Quick capture* — a tap that deep-links into Atlas's capture, not an inline text field (widgets can't run the core).
- **Data source:** the widget reads a small, safe **projection file** the core writes on each tick (never the vault, never raw data) — so the widget process shows current glances without running Atlas or holding secrets.
- **Refresh model:** iOS WidgetKit is a scheduled timeline (the OS controls cadence, minutes not seconds) — the core writes the projection; the widget reads the latest. Android widgets update on the core's own schedule (more freedom). Both degrade gracefully to "as of HH:MM" when the core hasn't run recently.
- **Privacy:** widgets are visible on a lock screen, so they show only what the person marks lock-screen-safe (a per-category toggle, tied to the §9 notification categories).

---

## 11. Internet & group chats from the phone, when connected

On a **native** phone (on-device core), whenever the phone has wifi/cell/any connection it uses Atlas's online
(secondary) capabilities directly — web search/fetch, online research, mail, and the **group-chat mesh** beyond
the local network — with **no dependence on Eric's machines**, because the phone is a full core. Offline, it
falls back to fully local (on-device model, local data, `outbox` queues online actions and flushes on reconnect).
On the **iOS PWA** path, the phone is online-capable only while it can reach a core that has internet.

---

## 12. Dependencies

- **`ed25519-dalek`** (pure Rust, MIT/Apache) — the release signature (§2, §7). The single crate the whole update effort requires. Add to `EVERY_DEPENDENCY` in `tests/metrics.rs` with its reason.
- **`wasmi`** (pure-Rust WASM interpreter, iOS-safe) — **only if Tier 2 plugins (§5.2) are built.** Not added for Tier 1. A deliberate, deferred decision.
- Current tiny dep set for context: serde, serde_yaml, serde_json, thiserror, eframe (optional), argon2, chacha20poly1305, tract-onnx (optional), native-tls, qrcode, windows.
- APNs (if ever added) is an Apple service over the internet, not a Rust dependency.

---

## 13. Re-signing, precisely

Every build is signed (unsigned installs on neither mobile OS), but signing is an automated build step using
credentials created once — **not** re-enrollment. **Android:** one keystore, created once, ~25-yr validity, no
per-device registration, no renewal. **iOS:** cert + ad-hoc profile created once, reused per build; regenerate
only ~yearly (membership) or when adding a new friend's device. **Hard rule:** keep the OS signing identity
constant (same keystore; same Team + bundle ID), or the OS treats the update as a different app and wipes/refuses.
**Desktop:** no OS signing needed for the swap (optional codesigning for SmartScreen/Gatekeeper is a later nicety).

---

## 14. Tested vs blocked

- **Buildable & testable in-house now:** `release` sign/verify + tests; signed manifest format + verify/min-version/downgrade tests; carrying it as a mesh payload; three-way config merge (§4); the `YOURS` plugin directory + re-verify-on-load; Tier 1 declarative plugin loading + the capability/permission model + its tests; chat roles + release-channel watch logic; desktop apply (extends `upgrade`) incl. the friend case; notification-category model + toggles as data.
- **Blocked on real hardware (named):** Android `PackageInstaller` install; iOS OTA-over-tailnet install; on-device notification posting; widgets on a real device. Code here, proof on a phone. The build/sign pipeline is the mobile-core cloud-CI work.
- **Deferred by decision:** Tier 2 WASM plugin runtime (until Tier 1 proves insufficient); iOS cold-wake APNs; release-key rotation hardening.

---

## 15. Open decisions (parked, none block the foundation)

1. When (if ever) to add Tier 2 sandboxed-code plugins + `wasmi` — driven by whether Tier 1 declarative covers what friends actually build.
2. iOS cold-wake APNs — *decided* as an owner-only capability (§9), gated by Eric's APNs-key custody; V1 stays local-only for everyone. Only the build timing is open (add whenever Eric wants wake-push for himself).
3. Release-key rotation scheme — worth designing before the key is ever at risk, not urgent.

---

## 16. Build sequence

1. `ed25519-dalek`; `release.rs` sign/verify + tests; register the dep reason in `metrics.rs`.
2. Signed manifest + per-platform artifact format; carry as a mesh payload; tests for verify-before-stage, min-version, and downgrade refusal.
3. Three-way config merge so shipped-default changes never overwrite user overrides (§4); tests. **Built — see §20.**
4. The plugin boundary Tier 1: `data/plugins/` in the `YOURS` set; declarative plugin loader; the capability/permission model (declare → approve → mediated host calls → revocable); re-verify-on-load; tests for the §5.4 gaps. **Built — see §20.**
5. `update_courier` capability in `capability.rs`; chat roles + release-channel watch; wire through the daemon's sync pass; update guard counts / `CAPABILITIES.md`. **Built — see §20** (with group ownership signed by per-Atlas keys).
6. Desktop apply via `upgrade` — Eric's and friends' desktops, end-to-end in-house, with the `auto_update` preference. **Begun — release key, signing and announcing built; see §20.**
7. Notification-category model + toggles as data, reusing `notify`/`window`; in-house posting hooks per platform (proof on device).
8. Widget projection file + the two platform widgets (proof on device).
9. Android `PackageInstaller` apply hook (proof on device); iOS local-OTA endpoint (proof on device).
10. (Deferred) Tier 2 WASM plugins; iOS APNs. (Key rotation was pulled forward and is built — §20 gap G.)

Steps 1–6 are fully in-house and testable here. 7–9 are code here, proof on a phone. 10 is deferred by §15.
(§17 bootstrap and the §18 gap resolutions fold into this sequence — the fleet-safety, plugin-versioning, and plugin-sync items below are must-haves, not extras.)

---

## 17. Bootstrap & onboarding — first install without a stack of files

**DECIDED delivery model (Eric, 25 Sep):** deliver every platform as a *single, signed, normal-app install* — one packaged installer that unpacks itself and drops an app icon, signed so no security warnings and no disabling Smart App Control. Desktop = a signed installer (`.exe`/MSIX on Windows via Azure Artifact Signing ~$10/mo; notarized `.app`/`.dmg` on Mac under the $99 Apple account; plain binary on Linux). Only Eric pays, once, on the build side; friends install for free with no account and no toggles. After the first install, the courier updates in place — icon, data, and customizations all stay.


Eric: *"For desktop I don't want to hand over a stack of files."* The way around it:

- **One file per OS, not a stack.** A friend gets a single artifact — a self-contained `.exe` (Windows), a single binary/`.app` (Mac), a single binary (Linux). On first run it scaffolds its own `config/` and `data/` (the existing `firstlaunch.rs`), so the friend never arranges anything. Internally Atlas still has many files; the *delivery* is one.
- **The models are the real "stack."** The large ML models are the awkward part, and the mandate says no hosting. Two no-hosting paths: (a) **over the mesh** — the one binary joins the household/mesh with a pairing code and pulls the engine's models as signed payloads from Eric's or a peer's node during onboarding (no file hand-off, in-house); (b) **public source on first online run** — fetch the model from its origin (e.g. a model hub) when online. Prefer (a) for offline-first/in-house; (b) is the fallback. The `install.rs` guided-fetch already exists to build on.
- **Delivery of that one file can be a link or QR**, not a literal hand-off — Atlas already generates QR codes for the hub, so onboarding can be "scan this."
- **After first run, the courier does everything.** No files are ever handed over again; updates (§5–8) keep it current.
- **Honest floor:** there must be exactly *one* first artifact, because you can't courier-update something not yet installed. But it's one file, and everything after is automatic.

**Trusted install without EV certs — SmartScreen & Smart App Control.** Eric had to *disable Smart App Control* to install Atlas; friends must not be forced into that. Two separate Windows gates, different severity:
- **SmartScreen** ("Windows protected your PC") warns on downloaded, low-reputation executables. It is click-through ("More info → Run anyway"), annoying but not a wall.
- **Smart App Control (SAC)** — on by default on fresh Win11 — is a *wall*: it blocks unknown/unsigned apps outright with no easy "run anyway," which is exactly why Eric had to turn it off. Unsigned apps essentially never pass SAC.
- **The cheap fix (recommended): Azure Artifact Signing (formerly Trusted Signing), ~$9.99/month** — Microsoft's cloud code-signing. No hardware dongle, no EV cert, signs via cloud API (so the existing CI signs Windows builds with no extra hardware). It gives **instant SmartScreen reputation** on everything signed under the identity, and is the path SAC is designed to recognize. Individual/self-employed developers are eligible as of 2026 (US/CA/EU/UK — Eric is US), no 3-year-business requirement. This is ~1/10th the cost of an EV cert and needs no token.
- **Honest caveat:** SAC is aggressive; Artifact Signing is the intended, strongest non-EV route and normally satisfies it, but a brand-new signer can occasionally see SAC reputation take a little time. An EV cert ($250–700/yr + dongle) is the only *instant-maximal* SAC trust — not worth it here; Artifact Signing gets ~all of it for ~$120/yr.
- **Mandate note:** this is an OS-trust formality for *distribution*, not a runtime dependency — Atlas still runs fully in-house/offline. It's an optional ~$120/yr operational cost so friends never touch a security toggle; without it they *can* still install by clicking through SmartScreen and (only if SAC is on) allowing it, which is the friction Eric wants gone.
- **Other platforms:** macOS has the same gate (Gatekeeper/**notarization**), but notarization uses the **$99 Apple account Eric already needs for iOS** — no extra cost. Linux has no such gate. iOS/Android are handled by their own signing (§13).
- **Packaging is separate from signing:** the "one file, Install, Allow changes, done" experience is just proper installer packaging (a signed installer `.exe`/MSIX) — a build step needing no cert of its own; signing is what removes the *warnings*, packaging is what makes it *one click*.

**Initial trust — the release-key chicken-and-egg (security-critical).** Friends verify updates against Eric's public key baked into the app, but that only helps if the *first* binary was authentic. So the first install's authenticity is confirmed **out-of-band**: Atlas shows the friend a short fingerprint (a safety code) of the build/key, and Eric confirms the same value over a trusted channel (in person, a call) — the same shape as Atlas's existing ten-character pairing code. A tampered first binary shows a different fingerprint and is caught before it's trusted.

---

## 18. Gaps found in a pre-build review (and their resolutions)

A deliberate hole-hunt before building. Each gap has a resolution; the starred ones become build steps, not afterthoughts.

1. **★ Fleet-wide bad update.** Auto-update means one broken build could brick everyone at once. Resolution: **staged rollout** (Eric's own devices update first as a canary, then friends after a soak window); a **post-update health gate** — the new engine must pass a self-check on launch or the desktop auto-reverts to the `keep_old_at` binary; and a **known-good pin** the fleet can fall back to. No release reaches friends until it has run clean on Eric's devices.
2. **★ Mobile rollback is weak (honesty).** Desktop reverts cleanly (kept old binary). iOS ad-hoc can reinstall an older `.ipa`, but Android usually blocks version-code downgrades. So on phones, recovery from a bad build is mostly **roll forward** (push a fixed higher version), not roll back — which is exactly why the health gate + canary (gap 1) matter more for mobile. Stated plainly rather than pretended away.
3. **★ Model / large-asset delivery + delta updates.** Full binaries and models over a phone's connection are large. Resolution: **delta updates** (ship only changed bytes, bsdiff-style) with a **full-build fallback** for a device many versions behind or where a diff won't apply; models are versioned and delivered separately from the engine (a model update needn't reship the binary, and vice-versa), over the mesh in resumable chunks.
4. **★ Plugin API versioning.** A plugin written against engine API v1 must not crash or silently vanish on engine v2. Resolution: every plugin manifest declares the **plugin-API version** it targets; the engine loads compatible ones, and for an incompatible one it **disables it visibly with a reason** (never a silent drop, never a crash) and tells the person the plugin needs an update. The host API is versioned and additive within a major version.
5. **★ Plugins sync across a person's own devices.** If a friend adds a capability on their laptop it should appear on their phone — "same Atlas everywhere." Resolution: plugins live in the `YOURS` set and **ride the sync log** like other personal state, so a person's capabilities follow them across their devices. Tier 1 is pure data; a Tier 2 `.wasm` is portable and runs sandboxed on every platform, so this is safe on mobile too.
6. **Cross-household trust boundary.** Friends are separate households (their own data, not Eric's notes), yet updates cross from Eric to them. Resolution: the **release channel is separate from household sync** — it carries only public, signed build artifacts, never anyone's private log. Authorship across households is the ed25519 release signature (§2); personal data never traverses the release channel.
7. **Friend-to-friend plugin sharing policy.** If friends can pass plugins to each other, one could pass a malicious one. Resolution (already in §5.5, made explicit): a shared plugin is **never auto-installed** — it arrives tagged with its author identity, and the recipient must explicitly enable it and approve its permission list; the sandbox + least-authority contain any damage regardless of author.
8. **Permission management surface.** "Revocable" needs a place to revoke. Resolution: a hub panel lists every installed plugin, its granted capabilities, and its author, with revoke per grant — so the permission model is visible and auditable, not buried.
9. **iOS yearly re-provision cadence (operational).** The ad-hoc provisioning profile expires ~yearly; installed apps keep running, but new installs/updates break until Eric ships a re-signed build. Resolution: track the expiry and **re-sign + push before it lapses**; surface a reminder to Eric ahead of the date. Not a code gap, a calendar one — named so it isn't forgotten.
10. **Long-offline device catching up.** A device offline for months may be many versions behind. Resolution: each release is a **complete signed build**, so a stale device just takes the latest (deltas from gap 3 are an optimization with a full-build fallback) — no chain of intermediate updates must replay.

These fold into the build sequence: gaps 1, 3, 4, 5 are added steps (fleet safety; delta+model delivery; plugin-API versioning; plugin sync); gaps 2, 6, 7, 8, 9, 10 are design rules already satisfied by the architecture or by a small surface, captured here so none is lost.

---

## 19. The person's-eye view — getting Atlas onto a device

What each person actually does, once the build/sign pipeline exists. (Eric does the one-time build/sign; each friend does only the simple part.)

**Android (native app, cleanest).**
- *One-time (Eric):* finish the Android shell (a thin native app running the core, UI = the hub in a WebView), build the `.apk` from Windows + the Android NDK, sign with the one keystore.
- *Each person:* receives the signed `.apk` — over the Atlas chat/mesh or a link/QR — taps it, allows "install unknown apps" once, installs → **Atlas app icon** on the phone. Opens it: a full on-device Atlas, works offline, syncs to their other devices over the tailnet. Updates arrive through the courier and install in place — no reinstall.

**iOS — two routes (pick per person or offer both):**
- *Route A — PWA (fastest, $0, no Apple account, no build):* the person opens their Atlas hub URL (their own core, over the tailnet) in Safari → **Share → Add to Home Screen** → an Atlas icon appears. It's a full-capability *face* to their core. Limits: needs a reachable core (not a phone-only standalone), no home-screen widgets, notifications are limited. Good for anyone who doesn't need a standalone phone core.
- *Route B — native app (full standalone; needs Eric's $99 Apple account):* Eric builds + signs the `.ipa` in cloud CI (no Mac to own) and registers each friend's device ID in the ad-hoc profile. *Each person* installs via a one-tap **OTA link** their own core serves over the tailnet (or, as a fallback, via Sideloadly/AltStore from their own computer) → **Atlas icon**, a full on-device Atlas. Gets widgets, local notifications (and owner-only wake-push for Eric), and updates via the courier/OTA. Costs: the $99/yr (Eric only), a device-ID registration per friend, and a yearly re-sign.

**Desktop (Windows/Mac/Linux, per §17):** one signed installer → click Install → Allow changes → **app icon**; the courier keeps it current in place.

**In every case:** first run pairs the person into their household with the ten-character code (their Atlas is theirs, syncs across their own devices), and the release channel (§8) is watched for updates thereafter.

**Honest current status (so nothing is oversold):** the Rust core already *compiles* for mobile (the desktop-GUI was made optional this project), which is the load-bearing prerequisite. **Not yet built:** the Android/iOS app shells, the cloud build/sign pipeline, and the courier itself (steps 1–9). The on-device installs (Android `PackageInstaller`, iOS OTA) are code-here / proof-on-a-real-phone. So the phone apps do not exist today — the path above is the plan, and the in-house foundation (courier steps 1–6) is buildable now without any phone or Apple account.

---

## 20. Build log — what's built, and the gaps found while building

### Decisions made during the build (Eric, 25 Sep)
- **Private release key lives in the vault** (Argon2 + ChaCha20, auto-relock, passphrase never written down). Key generation will use the vault's existing OS random source (`BCryptGenRandom` on Windows).
- **Order of operations is fixed: key first, then any friend's copy.** Every Atlas has the public key baked in; the shipped placeholder means "trust nothing." A friend given a placeholder build could never accept an update and would need a manual reinstall. So the release key is generated in the same sitting as the Apple-account and Windows-signing setup, and only after that is any copy handed out.

### Step 1 — release signature (commit `0436ddf`)
ed25519 sign/verify + trust anchor in `src/release.rs`; placeholder anchor = refuse everything. Tested.
**Correction:** that commit's message said the whole suite was green; two standalone guard targets (`dead_capabilities`, `new_capabilities_are_wired`) were red because the final check ran the `all` suite and lib only. Root cause: modules in `wiring::UNWIRED_BASELINE` are skipped by the per-function deadness scans, so the `KNOWN` entries and the 256→259 ceiling added before baselining went stale. Fixed in step 2. **Process change:** after the final edit of every step, the complete `cargo test` (every target) is run, not a subset.

### Step 2 — signed release manifest (commit `d369458`)
One release = one signed document: release number (`sequence`, only ever compared value), display version, oldest data format it can open, and per-platform `{platform, file, size, sha256}`. `seal_manifest` (build side), `accept`/`accept_against` (device side), `check_artifact` (the downloaded file vs its signed entry). New dependency `sha2` (already compiled in via ed25519-dalek). Every refusal tested.

**Gaps closed by the design (each has a test):**
1. *Parse-before-verify* — the signature is checked **before** the JSON is parsed, so untrusted bytes never reach the parser.
2. *Cross-purpose replay* — domain separation: the key signs `atlas-release-manifest-v1\n || manifest`, so anything else the key ever signs can't be replayed as a release.
3. *Path traversal* — a signed file name like `../../Startup/evil.exe` is refused; names must be bare, safe characters only.
4. *Oversized input* — anything over 256 KB is refused before any work.
5. *Re-serialization mismatch* — the exact signed bytes travel verbatim; nothing is re-encoded before checking.
6. *Ambiguous builds* — two entries for one platform, empty lists, zero sizes, and non-hex fingerprints are refused even when signed.
7. *Downgrade / replay* — an older release is refused unless explicitly rolling back; the same release twice is "already installed," not reinstalled.
8. *Version-string games* — "newer" is decided only by the integer release number, never by comparing version text.
9. *Silent failure* — every refusal has a plain-language message, pinned by a test.

**Open gaps found (not closed yet) and how each will be solved:**
- **A. No single data-format number exists.** The manifest's "oldest data format it can open" needs a counterpart on the device, and Atlas has none today (only the sync bundle version and a text version label). *Fix:* add one `DATA_FORMAT` constant for the stored-data layout, bump it whenever stored data changes shape, and have `upgrade` refuse/migrate by it. Do before the courier is wired.
- **B. Large files are checked in memory.** `check_artifact` takes the whole file; an iPhone/Android build is tens to hundreds of MB. *Fix:* in the courier step, hash while downloading (SHA-256 is incremental) and never hold the whole file.
- **C. Where the installed release number is kept.** The device must remember which `sequence` it has, in the "yours" set (survives updates), or downgrade protection resets. *Fix:* store it with `upgrade`'s kept state; courier step.
- **D. "Roll back" must come only from the person.** `allow_older` must be set only by a local, deliberate action — never by anything arriving over the mesh — or the downgrade defense can be switched off remotely. *Fix:* the courier takes it only from the local UI/command; a test will assert no message field can set it.
- **E. One name for "this device's platform."** Build and device must spell platforms identically (`windows-x86_64`, not `win64`). *Fix:* a single `this_platform()` derived from the compile target, used by both the build command and the device; courier step, with a test that every shipped platform name round-trips.
- **F. Freeze attack (being held back, not rolled back).** The release number stops going backwards, but a device that never *hears* about a new release stays on an old one indefinitely. Low risk among friends, still real. *Fix:* the release channel carries a signed "latest release" notice with a date; a device that hasn't seen one in N days says so plainly, and asks more than one peer.
- **G. Key rotation** — still parked (§15), needs designing before the key is ever at risk.

### Gaps A–G — closed (commit `5dda9b2`)
Every open gap from step 2 is now built and tested in `src/release.rs` / `src/upgrade.rs`.
- **A. Data format.** `upgrade::DATA_FORMAT` is the one number for the shape of stored data. A release states the oldest it can open and the one it writes; a device refuses a release that can't open its data (`DataTooOld`), and a release that claims to write an older format than it needs is refused as malformed.
- **B. Big files.** `Fingerprint` hashes while downloading, chunk by chunk, and refuses the moment the bytes pass the signed size — the whole file is never held in memory.
- **C. Installed release remembered.** `Installed` (release number, key era, version, fingerprint, newest notice seen) lives in `data/state`, which updates never touch, so downgrade protection cannot be reset by an update.
- **D. Only the person rolls back.** `Direction::Rollback` needs a `LocalApproval`, which can only be made on the device itself (`given_by_the_person_at_this_device`). Every signed document has `deny_unknown_fields`, so a message smuggling in an extra "and roll back" field is refused outright — tested.
- **E. One platform name.** `KNOWN_PLATFORMS` + `this_platform()` from the compile target; a manifest naming any other platform is refused.
- **F. Freeze attack.** Each release carries a signed `next_word_by` date. `freshness()` says plainly when that date has passed with nothing newer heard ("Overdue by N days") — being quietly held back is now visible.
- **G. Key rotation + recovery.** A second, offline **recovery key** (`RECOVERY_PUBLIC_KEY`, kept off every machine) can replace the release key. Planned rotations are signed by the current key; emergency ones by the recovery key. The new key can't be the placeholder, invalid, or the recovery key itself. *Found while building G:* a thief holding the release key could sign release number ~2⁶⁴ and lock you out even after you recover. Fixed by counting release numbers **per key era** — after a rotation the count starts over, so a stolen key's inflated number dies with that key. Tested end to end (theft → recovery → your next release is accepted, the thief's is not).
- **Design correction:** setting renames were first put in the signed release notice. That declared one fact in two places and did nothing for a manual update, so they moved into the program itself (`upgrade::RENAMED_SETTINGS`), applied at startup however the program arrived.

### Step 3 — your edits survive every update (commit `5dda9b2`)
New module `src/yourchanges.rs`; `Config::load` and startup wired; unit tests plus end-to-end tests (`tests/hand_edits_survive_updates.rs`) that load the real config the way Atlas runs.

Three layers per shipped file, the same three a careful merge tool uses:
- **Base** `config/local/base/<file>` — exactly what Atlas last shipped there.
- **Yours** `config/local/<file>` — each setting you changed: the value you chose and the shipped value it replaced. Readable, with a header saying delete an entry to go back to the default.
- **Theirs** — the new shipped text, which is built into the program.

At every start, before the config is read, `keep_hand_edits` compares each shipped file with its base. Your changes move into `config/local/`, and the shipped file goes back to exactly this build's text. `Config::load` then layers them every time: shipped → your hand edits → your Settings-page choices (most deliberate last). Result, each pinned by a test:
- A default you never touched follows the release; a value you set stays yours.
- When a release changes a default you had overridden, yours is kept and you're told **once per version**, with the one line to delete to take the new default.
- A setting whose section a release removed is reported by `atlas doctor`, never recreated.
- An edit that would make a file unreadable can't stop Atlas starting: it runs on the shipped file and `doctor` says why.
- Any file it replaces that you had touched is kept byte-for-byte in `config/local/previous/`.
- If your `config/local` file won't parse, nothing for that file is touched.
- A renamed setting carries your hand edit and your Settings choice to the new name.
- Atlas's own source tree is never rewritten (its config files *are* what gets built in).
- `config/local` is in `upgrade::YOURS`; `atlas update` reports how many edits it's keeping.

**Simplification this buys step 6:** because the program carries its own shipped config and brings the files up to date on start, the desktop update only ever replaces **one file — the program**. Config files never need to travel with an update.

**Gaps found in step 3, and how each is solved:**
- **H. First start of this version on an existing install has no base on record.** Differences then might be yours or an older release's defaults. It keeps them all, marks them `unsure`, and says so — never loses an edit, at the cost of possibly pinning an old default. Affects only installs that predate this build; no friend has a copy yet (key-first rule), so every friend install starts with a base.
- **I. Undoing an edit by editing the shipped file back does nothing** (the edit already lives in `config/local`). *Fix:* a "your edits" panel on the hub listing every kept edit with a per-entry "back to default" — lands with the Settings/permissions hub work in step 4. Until then: delete the entry, as the file header says.
- **J. The two label lists (`labels/*.txt`) are not covered,** and first launch never overwrites them, so today a release can't update them either. *Fix:* a line-based version of the same three-way merge (lines you added/removed kept, the release's lines arrive), in step 6.
- **K. Comments on edited lines don't travel** — only values do. The previous file is kept in `config/local/previous/`, so nothing is lost; noted, not fixed.
- **L. Settings-page choices don't record the default they replaced,** so a release changing a default you chose in Settings isn't flagged the way a hand edit is. *Fix:* store the shipped value beside each choice in `settings.yaml` and reuse `conflicts()`; small, in step 4.
- **M. Edits made while Atlas is running are captured on next start.** Until then they're in the shipped file and still in effect, so nothing is lost; only an update applied without a start in between would matter, and the program itself performs the capture before reading its config.

### Gaps H–M from step 3 — closed or settled (commit `715b53c`)
- **H (first start has no base on record).** Kept as designed — never lose an edit — and now visible: every edit marked *unsure* shows on the hub's **Your edits** page with a note and a one-press "back to the default".
- **I (undoing an edit by editing the file back does nothing).** Fixed: the **Your edits** page (hub, works with Atlas stopped) lists every kept edit — yours, what Atlas ships now, and whether the default moved since — with "back to the default" on each. Same from a terminal: `atlas edits`, `atlas edits revert <file> <setting>`.
- **J (label lists).** The root cause was different from the gap as written: the two label lists are part of the vision models, compiled in and **never read from disk**, so the copies first launch wrote out were files that looked editable and did nothing. First launch no longer writes them. A test now holds that every file written out for you to edit is one whose edits are kept.
- **K (comments on edited lines).** Settled, not fixed: values are kept; the whole previous file is kept in `config/local/previous/`.
- **L (Settings choices didn't remember the default they replaced).** Fixed: `config/local/settings-was.yaml` records the shipped value under each Settings choice (read from what was shipped *before* the update, never after), and a release that moves one says so once, like a hand edit.
- **M.** Settled: nothing can be lost, because the program captures edits before it reads its config.

### Step 4 — add-ons, Tier 1 of the plugin boundary (commit `715b53c`)
New module `src/plugins.rs`; wired into the daemon, the hub, `atlas plugins`, and `atlas doctor`; declared in the catalogue as the `addons` capability. Tested by unit tests and by end-to-end tests that speak to the real daemon (`tests/add_ons_do_only_what_you_allowed.rs`).

**What an add-on is.** One file, `data/plugins/<id>/plugin.yaml` (in `upgrade::YOURS`, so updates never touch it): a name, an author, the add-on format it was written for, the permissions it needs, and one or more named sequences of ordinary Atlas commands with the words that start each. No code — Atlas interprets it.

**Permissions** are plain categories (basics, read notes, write notes, calendar, desktop, online, thinking, making, screen, files, read messages, send messages, drafts), each a fixed list of commands. **Some commands are never available to any add-on** (`plugins::NEVER`, each with its reason): the vault, pairing, sync, accounts and sign-in, handing Atlas over, Atlas changing its own code or standing instructions, undo, muting what Atlas tells you, dictating into other apps. A test fails if any command is in neither list, so every new command gets decided the day it's added.

**The defences, each pinned by a test:**
- Nothing runs until you approve it — on the hub's **Add-ons** page or `atlas plugins approve`. There is no spoken command or message that approves an add-on.
- The approval records the file's SHA-256. The approve button carries the fingerprint of what the page showed you, so a file swapped between looking and pressing is refused.
- A step needing a permission the add-on didn't declare, a forbidden command, or something that isn't a command at all makes it refuse to load, visibly — never half-loaded.
- **Every step is re-checked as it runs:** the file must still be the one you approved, the add-on must be on, and the command the step turned out to be must be in a permission you still grant. Taking a permission away stops it at its very next step, even mid-sequence, even after you said yes to a question it was waiting on.
- **An earlier step's result can fill in a step but never change which command it is** ("open {x}" stays an open whatever a web page said). *Found and closed while building.*
- It still goes through Atlas's ordinary approval gate, and the question says which add-on is asking.
- Its words can't be an answer ("yes", "go ahead" — the same yes/no reading the daemon uses), a phrase Atlas already has, or anything that reads as a forbidden command; it must be at least two words and matches only exactly. It is never matched while Atlas has asked you something and not yet had your reply. Your own saved sequences win over an add-on's. Two add-ons claiming the same words start neither.
- Add-ons cannot call each other or your sequences: a step is parsed as a command, never matched as a trigger.
- Unknown fields, a file over 64 KB, an id that doesn't match its folder, or a format for a newer Atlas are all refused with a reason (format versioning is the §18 gap 4 design, built). An unreadable approvals record approves nothing.
- `atlas doctor` reports add-ons that are off without you switching them off, and ones waiting for you.

**Gaps found in step 4, and how each is solved:**
- **N. Add-ons don't follow you to your other devices yet** (§18 gap 5). *Fix:* the add-on files and your approvals ride the household sync log; an approval applies on another device only if the file's fingerprint matches. Built with the sync work in step 5.
- **O. A friend can't send you an add-on through Atlas yet** — only `atlas plugins add <file>` today. The `author` field is what the file says about itself, and the page says so. *Fix:* when one arrives from a friend, it lands switched off until you approve it, with the **verified identity of the paired device that sent it** shown beside the self-declared author. Step 5, with chat roles.
- **P. Add-ons only start when you say their words.** No "every morning at 8" yet. *Fix:* a schedule trigger that goes through the scheduler and the very same per-step checks. Next Tier 1 addition.
- **Q. No "remove".** Switching off keeps the approval; deleting the folder removes it (the stale approval can't match any new file). *Fix:* `remove` that moves the folder to Atlas's trash and forgets the approval. Small; step 5.
- **R. Within what you granted, content can still choose the target.** A step "open {x}" filled from a web page can open whichever app the page named — the command can't change, but its argument can. Named, not hidden: grant `online` and `desktop` together only to add-ons you trust, and the approval gate still asks before anything consequential.

### Step 4 addition — asking less (commit `6b7e938`)
Eric's rule: approval must not go so far that Atlas asks about everything. Built as:
- **Approving an add-on is the one decision.** Its permission checks never ask — each step is either allowed (silently) or stopped with a reason. A benign add-on runs with no questions at all (tested).
- **"Always."** When an add-on's step would normally ask ("close Chrome — go ahead?"), the question now offers *say "always" and I won't ask about this step again*. Saying it answers yes and records a standing yes for exactly that step of that add-on. Also on the hub (**don't ask me each time** / **ask again**) and `atlas plugins trust|untrust`.
- **Where it is never offered:** a step whose text is filled in from an earlier result (it isn't the same thing each time), and sending a message as you (it speaks to other people). Trust carries to a new version of the add-on only for a step still there word for word; a changed file skips nothing until it is approved again.
- **Refused before asked.** Atlas no longer asks "go ahead?" about something that would be refused whatever you answered (first case: posting in a group where you're a reader). A question whose yes can only lead to "you can't" teaches you to answer without reading.

### Gaps N–R from step 4 — closed (commit `6b7e938`)
- **N. Add-ons follow you to your other devices.** Found on the way: **sync carried events and nothing applied them** — a note captured on the phone crossed the wire, sat in the laptop's log, and never reached its notebook. Fixed: synced captures land in the notebook (tested with two Atlases sharing a folder). Add-on files and approvals now ride sync as ordinary events, worked out by comparing at sync time (so hub, terminal and friend changes are all caught, and nothing echoes back). An **approval is taken only from a bundle sealed with your household key**; from an unsealed one the add-on arrives switched off.
- **O. A friend can send you an add-on.** `atlas plugins send <id> <paired name>`. On their end it lands switched off, marked **sent by** the paired Atlas that sent it — the one fact about its origin the pairing proves. It never replaces an add-on they already have with that name.
- **P. Add-ons can run by themselves**: `schedule: every 30 minutes | every 2 hours | daily at 08:00` (local time; nothing faster than every 15 minutes). Approving the add-on is the consent, the rule scheduled jobs already follow; every step is still checked, and a step that asks still asks. Never fires on first sight; runs once per period, one at a time. *Found on the way:* local time on Windows was always read as UTC (`date +%z` doesn't exist there), which also mis-stamped chat times — now asked of Windows itself, cached for ten minutes. **Not yet proven on your Windows machine.**
- **Q. Remove**: `atlas plugins remove <id>` / hub button — to Atlas's trash, approval forgotten.
- **R.** Closed as far as it can be: content can still choose the target of an "open {x}" step, but a filled-in step can never be trusted to skip its question, and an app Atlas doesn't know still asks (the grants gate).

### Step 5 — chat roles, the release channel, and hearing about updates (commit `6b7e938`)
**Groups with an owner** (`src/groups.rs`, `src/peerkey.rs`). The first group chats had no owner: any member could bring anyone in by naming them, and nobody could take anybody out. Now a group you make is yours:
- You **add, remove, and change roles** — **member** (reads and posts) or **reader** (reads only) — and rename it. Hub **Groups** page (works with Atlas stopped) and `atlas group new|release|add|remove|role|rename|list`.
- **Why everyone can trust it:** each Atlas now has its own signing key, introduced to each paired Atlas over the pairing channel (`/hello`) and pinned — a different key later is refused and said, never swapped. Your Atlas writes the group's whole membership as a numbered list and **signs it**; every member's Atlas checks the signature and takes only a newer number. The group's id is made from your key, so nobody else can publish a list for it. Tested: a member forging a list, claiming ownership, replaying an old list, or tampering after signing — all refused.
- **What each member's Atlas enforces:** a message from someone not on the list, or on it as a reader, is not filed; you can't post where you're a reader (refused before being asked); taken out means the group closes on your end, and being added back re-opens it; a message that arrives before its list waits for it. A member leaving is taken off your list automatically, so everyone agrees; you, the owner, can't walk out of your own group.
- **Delivery:** your Atlas hands each member the latest list (and anyone just removed, the list without them), retrying an offline one every few minutes.

**The release channel** is a group where only the owner posts (everyone else is a reader, and can't be made otherwise). **The update courier** (`src/update_courier.rs`) reads it: only a message from the channel's owner is read; the notice must verify against the release key built into this Atlas; a verified notice counts as having heard from you (the freeze defence) even for the release already installed; a newer one that fits this device and can open its data is recorded and said once — *"Atlas 1.3.0 is out, signed by your release key. I'll ask you before installing it."* `atlas update` shows what's been heard. Nothing downloads or installs yet (step 6). With no release key set, nothing is trusted, and it says so once.

**Gaps found in step 5, and how each is solved:**
- **S. Members only reach members they're paired with** (the mesh has no relay). In a group where not everyone is paired with everyone, some messages don't reach some people. *Fix:* the owner's Atlas relays members' messages to the rest, marked as relayed by you. Step 6 or 7.
- **T. Losing the owner's device loses control of its groups** (the key is on it). *Fix:* keep this Atlas's key in your backups and vault, and a signed hand-over of ownership. Before friends depend on a group.
- **U. You on two devices are two identities** — your phone can't manage a group your laptop made. *Fix:* your devices share one key through sealed household sync, so "you" is one key everywhere. Same step as T.
- **V. Posting a release notice has no command yet** — it needs the release key, which is made in the sitting with the Apple and Windows signing setup. `atlas release keygen/sign/announce` is step 6; the receiving side is done and tested with a test key.
- **W. No voice commands for managing groups** — hub and terminal only. *Fix:* "add Sam to Friends", "make Maya a reader in Friends", as commands decided as forbidden to add-ons.
- **X. Groups made before this stay ownerless** (anyone can add). Make a new group to get an owner; the old ones are left exactly as they were.

### Sharing add-ons with friends (Eric, 25 Sep: "if it helps and the others like it, it gets picked up by choice — or sent privately in chats") (commit `773454a`)
- **Share** one of your add-ons **in a group** or **privately with one person** — hub (Add-ons page → *Share it with*) or `atlas plugins share <id> <group or person>`. In a group, a line goes in the chat too.
- **It arrives on a shelf, not installed.** The Add-ons page shows *Shared with you*: what it is, who sent it (proven by the pairing), who it says wrote it (not proven), and in plain words what it would be allowed to do. Nothing on the shelf runs.
- **Its steps are shown**, not just its description — every sequence, how it starts, and what it does.
- **Taking it is one decision:** *Add it and allow that* installs and approves exactly the file you were shown. *No thanks* removes it. The same file shared twice is one offer. It never replaces an add-on of yours with the same name.
- **Picked up by choice:** once you use one, *Recommend it in <group>* posts a line saying you use it and recommend it — a person vouching, not a counter.

### Gaps S–X from step 5 — closed (commit `773454a`)
- **S. Members who aren't paired with each other now hear each other.** The owner's Atlas passes each member's message on to the rest, carrying the author's key. A member's Atlas believes "this is from Sam" only when the group's owner says it, and files it under Sam's name. A member can't speak for anyone else (tested).
- **T/U. Your other devices can manage your groups.** Your devices introduce their keys to each other through sealed household sync; your Atlas vouches for each one (signed by your key) in every group you own, and in every group you make later. Members accept a change signed by a vouched-for device, and nobody else can vouch or sneak in as one (tested). A group's list also reaches your other devices through sync. Losing the device that made a group no longer loses the group, if another of your devices is vouched for. The key itself is in `data/state`, which your backups already copy (not if you use separate profiles — named).
- **V. Posting a release notice:** see step 6 below.
- **W. Voice:** "add Sam to the Friends group", "take Sam out of the Friends group", "make Maya a reader in the Friends group", "let Maya post in the Friends group". Done and said, not asked about; add-ons can never use it.
- **X. Older groups:** the Groups page lists groups without an owner, with *Give it an owner (you)* — it starts again with the same people under your list, and the old one is renamed "(before)" and told where the conversation went.

### Step 6 — begun: the release key and announcing a release (commit `773454a`)
- `atlas release keygen` makes your release key **in your vault** (never on disk anywhere else) and a **recovery key** shown once to write down and keep offline, and prints the two lines to bake into the build. Refuses to make a second release key (it would lock every copy out).
- `atlas release sign <version> <platform>=<file> ...` measures each file (size and fingerprint are never typed), numbers the release, promises the next notice within 30 days, and signs it with the key from the vault. It warns if this build doesn't carry your key.
- `atlas release announce <file>` queues it; the running Atlas posts it into every release channel you own — no second question, since the command was the decision. Every friend's Atlas then checks it as in step 5.
- `atlas release show` says which key this build trusts and what's been heard.
- **Still to do in step 6:** installing with the previous version kept, rollback by your own hand, and key rotation (their code is built and tested; their callers are what's next). The key-making sitting is still yours to do with the Apple and Windows signing setup.

**New gaps found here, and how each is solved:**
- **Y. The files themselves don't travel yet** — a friend's Atlas hears "1.3.0 is out" but has no way to fetch it. *Fix:* the announcing Atlas serves the signed files to paired devices in chunks over the pairing channel, checked while downloading (`Fingerprint`), resumable. Next.
- **Z. A vouched-for device can change a group but can't post in it as you** — friends aren't paired with your phone. *Fix:* your phone's messages to a group go through your laptop, the same relay as S.
- **AA. A shared add-on was judged by its description.** Closed in the same change: the shelf now lists every sequence and its steps, and how each starts.

### Adding friends in one step (Eric, 25 Sep: "not a whole process of getting an id code then pairing then waiting for a confirmation — it needs to be simpler") (commit `e6975c4`)
- **Like any app: you send a link, they open it, done.** Say *"add a friend"* (or press **Make a friend link** on the hub's new **Friends** page, or `atlas friend`). Send the link any way you like — text, email, in person as a QR code. When their Atlas opens it (paste the whole message, or press **Add them**), the two Atlases pair **both ways at once**, each pins the other's key from the exchange, and both of you are told *"you're friends now"*. Nothing to send back, nobody waits to confirm: sending the link was your decision, opening it was theirs.
- **Safe with nothing in the middle.** The link carries a one-time secret (128 random bits) good for **one use and seven days**. It's the only thing that opens the new *friend door* — the one door on the peer listener with no token, checked first and alone, with a budget of 30 knocks an hour from everyone together. A forwarded link is refused the second time and the half-made friendship on that side is undone (tested). Nothing else on the listener answers without a token (tested).
- **From a group:** tap *Send a friend request* next to someone you share a group with (or say *"send Maya a friend request"*). It goes **to them alone** through the group's owner, never shown as a group message, and carries your friend link; they press **Accept** (or say *"accept friend request from Sam"*) — underneath, that is opening your link. A request whose link isn't the sender's own is ignored (tested).
- **Offline friends:** if their Atlas can't be reached when you open their link, you're told, and Atlas keeps trying every few minutes for the week the link lasts, then says so if it never got through. Unfriending stops it.
- **Unfriend** is one button on the Friends page (both directions, and the open door, at once).
- **Where friends see you from:** `kin.my_name`, else your login name. **How they reach you:** `kin.my_host`, else this machine's Tailscale name — found by Atlas, not typed.

**Found while building it (root causes, fixed here):**
- **The peer door was never reachable from another machine.** It listens on loopback (by design) and nothing forwarded it onto the tailnet, so pairings completed on paper and then never connected. Making a friend link now runs `tailscale serve --bg --tcp=<port> tcp://127.0.0.1:<port>` (tailnet only, idempotent). *Not yet proven on your machines.*
- **The door only opened at startup, and only if you already had a pairing**, so your first friend could never knock. Atlas now always opens it (it lets nobody in until you've paired or made a link), and making a link opens it at once if it isn't.
- **"message the Friends group" failed in any owned group with one other person in it** ("I don't know anyone called the Friends group") because a group was recognised by counting members. Groups with an owner are groups whatever their size.

### Gaps Y and Z — closed (commit `e6975c4`)
- **Y. The files travel.** `atlas release sign` puts each file aside under its fingerprint. When a friend's Atlas hears a verified notice it fetches the file from the Atlas that announced it, in 256 KB pieces over the pairing, a few pieces a tick, **resuming where it stopped**. It refuses a sender whose size differs from the signed notice, and keeps the file only if the whole thing's SHA-256 is the one your key signed; otherwise it throws it away and fetches again. Then: *"Atlas 1.3.0 has arrived and matches what your release key signed. I'll ask you before installing it."* Only paired devices get pieces, only files you signed are handed out, and only by fingerprint (tested over real sockets).
- **Z. Your phone posts in your group as you.** A device you vouched for speaks as the owner: its message is filed as yours on your laptop and passed on to everyone else (not back to the phone). Your laptop also passes members' messages — and its own — to your phone, and hands it the group's list over the pairing as well as through sync (tested).

**Still to do in step 6:** installing with the previous version kept, rollback by your own hand, and key rotation.

**New gaps found here:**
- **AB. (Superseded twice -- friends no longer use Tailscale at all; see the Tor section.) Friends on unrelated networks.** Tailscale is how one Atlas reaches another. A friend on their own tailnet needs your machine shared with them in Tailscale (*Machines → Share*); a friend without Tailscale can't reach you yet. *Fix:* the planned `mesh` transport — until then the Friends page says this plainly.
- **AC. The QR code is read as text.** A phone camera shows the link as text to paste; the phone app doesn't open it directly yet. *Fix:* the phone app registers the link and adds on scan (with the PWA work).
- **AD. Only the announcing Atlas hands out a release.** A friend who already has it can't pass it on to another friend. *Fix:* any device holding a verified file serves it the same way (the check is the signature, not who sent it).

### Friends without Tailscale (Eric, 25 Sep: "Tailscale can connect one user's own devices. It should not be used to connect one user's Atlas to another user's Atlas." Fallback chosen: a mutual friend's Atlas) (commit `24bf4b4`)
**The rule now:** Tailscale may join *your own* devices together. Between two people, nothing is shared: your Atlas talks to your friend's over the open internet, and nobody joins anybody's network.
- **Atlas's own encryption (`wire`).** Tailscale had been quietly encrypting everything; off it, this is required. Every request between two people is sealed *to* the key the sender pinned for the receiver and *by* the sender's own key (X25519 on the same keys, HKDF-SHA256, ChaCha20-Poly1305 -- the shape of Noise's "K" pattern), with a fresh one-time key per envelope, a ten-minute freshness window and each envelope accepted once. The answer comes back sealed too. A token copied onto someone else's envelope is refused (tested).
- **The door only reads sealed envelopes from the internet.** It now listens on every address (it was loopback-only), and a request in the clear is honoured only from this machine, the home network or your own private network -- from anywhere else it's refused before its token is read. Atlas never sends in the clear to anything but your own networks.
- **Opening the door itself (`portmap`).** Atlas asks the home router to forward its one port -- NAT-PMP, then UPnP -- and renews it; offers this machine's own IPv6 address where there is one; and the home address for friends on the same wifi. A router whose own address is carrier-shared is recognised and not advertised. Tested against a NAT-PMP router and a UPnP router run in the test.
- **Checked by friends.** Every half hour Atlas asks a few friends to try its public addresses (`/check`) and keeps only what they could reach; friends are told whenever it changes (`/where`), and friend links carry all of it.
- **The mutual-friend fallback (`mailbox`).** When friends can't reach your Atlas at all, one or two friends whose Atlases *can* be reached hold your mail: senders hand them the sealed envelope, and your Atlas collects it every 30 seconds, confirming each piece so nothing is lost and nothing kept. The helper can't read, change or forge what it holds (tested: it can't open it). It holds only for a friend who asked, from its own friends (or a small, rate-limited first knock with a friend link), at most 500 envelopes / 16 MB per person for a week. A friend link to someone who can't be reached goes through their helper and finishes when their Atlas says hello (tested end to end, three Atlases).
- **The Friends page** says in one sentence where friends reach you and, if a friend is holding your mail, who.

**Named, not yet proven:** none of this has met a real router or two real homes -- the router protocols are tested against stand-ins. On Windows the door now listening beyond this machine will raise a firewall question the first time; the installer should add the rule, and doesn't yet.

**New gaps found here:**
- **AE. Both behind shared addresses, and no reachable friend in common.** Those two can't connect until one of them has a friend whose Atlas can be reached (messages wait with the sender). This is the case the mutual-friend choice accepts.
- **AF. Mail through a helper is up to 30 seconds late,** and a helper sees who is writing to whom (not what). *Fix:* direct connection by hole punching, introduced by the same helper -- about 70% success in the largest published measurement; not built.
- **AG. A release can't be fetched through a helper** -- it needs a live answer. A friend whose Atlas can't be reached gets releases from a friend who has already verified it (gap AD).
- **AH. "Delivered" through a helper means "handed to the helper".** The tick should wait for their Atlas to collect it. *Fix:* a delivery receipt on collection.
- **AI. IPv6 behind a router firewall.** Many routers block incoming IPv6 unless asked (PCP), which Atlas doesn't speak yet. The friends' check keeps an unreachable IPv6 address out of your links.
- **AJ. Windows firewall rule** for the door, added by the installer.

### Friends through Tor -- nobody in the middle (Eric, 25 Sep: "there has to be a better process ... without using Tailscale or a server in between"; he chose Tor/Arti and the removal of friends holding mail) (commit `03d7e7e`)
**Replaced:** the router door-opening (`portmap`), friends checking each other's reachability, address-change messages, and friends' Atlases holding each other's sealed mail (`mailbox`) -- all removed.

**Now:** every desktop Atlas is a Tor onion service (`onion`), the same way Briar and Ricochet work.
- Its onion address is made from its own key (a separate secret derived from it, not the signing key reused), and Atlas writes the keys where Tor reads them. Tested against the real `tor` program: Tor accepts Atlas's keys and serves exactly the address Atlas worked out.
- Atlas starts `tor` itself (the program ships beside Atlas; nothing for anyone to set up or pay for), and restarts it if it stops. Tor forwards the onion address to the door's **sealed-only** socket -- what Tor delivers looks as if it came from this machine, so it gets a socket where only sealed envelopes (`wire`) are read.
- Reaching a friend: straight across when you're on the same wifi, otherwise through Tor to their onion address (SOCKS5, written in-house, tested against a stand-in). A friend link carries the onion address and the home address.
- **When their Atlas is off, yours keeps it** and sends it when theirs is back -- nobody else holds anything, and "delivered" means their Atlas took it (closes AH).
- **Proven end to end over a real Tor network:** two Atlases with no network in common become friends through Tor on Tor's own private test network (`chutney`, hs-v3-min). The build machine can't reach the public Tor network, so the same test on the public network is yours to run once Tor ships beside Atlas.
- Closes AE (both behind shared addresses -- Tor doesn't care), AF (helpers seeing who writes to whom -- there are none), AH, AI (IPv6/router firewalls -- not needed). **AJ stays open:** Tor only connects out, but the ordinary door still listens beyond this machine for your own devices and same-wifi friends, so Windows will still ask about it once -- the installer should add that rule.

**New gaps found here:**
- **AK. Tor has to ship in the installer** -- the Tor Project's expert bundle (`tor/tor.exe`); not in the installer yet. Without it, only friends on your home network reach you, and the Friends page says so.
- **AL. Arti.** Tor in Rust, compiled into Atlas with no separate program. Its onion-service hosting was still being hardened in its release notes; to be tested inside Atlas and switched to when it holds up.
- **AM. Networks that block Tor.** `kin.tor_extra` takes bridge lines today; Atlas noticing a block and switching to Tor's bundled bridges by itself isn't done.
- **AN. Speed.** A first message to a friend through Tor takes a few seconds; a release file takes minutes, not seconds. Keeping each friend's Tor connection open between messages isn't done yet.
- **AO. Antivirus** sometimes flags `tor.exe`; your code-signing covers Atlas, not Tor.
- **AP. Phones** don't run Tor: they reach their own desktop Atlas over their own Tailscale, as decided.

### Step 6 — O1: a new version proves itself or the old one comes back (26 Sep)

- **Before it starts:** when a new build is put in place, the running Atlas first runs it once with `--health-check`. The new build checks that its own shipped settings load, that your settings folder reads, that it can write and read back its state folder, and that nothing of yours sits where the program goes. It has 60 seconds. If it fails, hangs, or never says "healthy", it is set aside as `atlas-<version>.failed`, the kept previous binary goes back, and the running Atlas carries on as if nothing happened.
- **Its first starts:** a build that passes is on trial. Each start counts; a start that gets through (returns normally, or stays up 90 seconds) ends the trial and the build is kept. Three starts that never get through and the fourth puts the previous version back and starts it instead.
- **Never tried twice:** a version that failed is written to `update-known-bad.txt`; the same version dropped in again is refused.
- **Accountable:** every step lands in `updates.log` beside the program, and `atlas update` shows the trial and the last five entries.
- **Fits gap D:** this is the device undoing its own failed install, decided on the device. No message from anyone can cause it, so "only the person rolls back" still holds for choosing an older release.
- **Proven:** 10 new tests (`tests/an_update_that_fails_goes_back.rs`), with real processes for pass, fail, hang and silent builds. The real binary was also run end to end: a broken 9.9.9 was refused and set aside, and a real start after three counted failures went back to the previous build and ran it. Full suite: 6718 passed, 0 failed.
- **AQ (open).** Going back restores the program, not your data. Nothing needs this yet, because the data format has never changed (it is still 1). The first release that changes the data format needs a copy of the data taken before its trial, and the rollback has to put that copy back.

### Step 6 — O2: installing a release, going back, and key changes (26 Sep, branch `step2-apply`)

- **Installing:**
  - Right before installing, everything is checked again, not trusted from download time: the notice against the key trusted *now*, the release order, the data format, and the downloaded file's own size and fingerprint.
  - A file changed on disk after it arrived is thrown away.
  - The file is then copied to `updates/`, the one way in, so O1's health check and probation still decide.
  - The release number moves only once the new build has got through its probation.
  - A version that failed here, or one you went back from, is never offered or installed again. A newer one is.
- **When, decided by Eric on 26 Sep:**
  - It's automatic on his own devices (the release channel's owner key, or a device it vouched for) and asks first on friends' copies.
  - `atlas update auto on|ask|off|default` changes it, and nothing arriving over the network can.
  - An automatic install waits for a quiet moment: 10 minutes away from the keyboard and mouse, not presenting, gaming or on a call.
  - A "not now" asks again a day later.
  - A yes installs straight away.
- **Restarting:** the daemon stages the release, saves its state, and starts itself again, so the new start swaps the build in.
  - It restarts once per staged build. If the build didn't go in, it says so and doesn't loop.
- **Going back:**
  - `atlas update undo` goes back only on a typed yes. That yes is what makes the `LocalApproval`.
  - The kept previous build returns, the current one is set aside as `atlas-<v>.undone`, and the release record goes back to what it was, with key changes kept.
  - The background Atlas restarts onto the previous build.
- **Key changes:**
  - `atlas release rotate` makes a new key and signs the change with the old one. The old key is kept in the vault, renamed.
  - `atlas release recover` signs with the recovery key you typed. Its number jumps by 100, past anything a thief could have signed.
  - `atlas release announce` posts either kind, and every device's courier takes a change it can verify and says a refusal once.
- **Proven:**
  - 15 tests in `tests/installing_a_release.rs`, with real ed25519 signatures, real files and O1's real trial records. They include the real program running `atlas update install`, `auto`, the report, and `undo` with "no" and then "yes".
- **Not proven yet:**
  - A real restart into a new build by the daemon, on the laptop.
  - Whether the restarting daemon and the new one ever fight over the only-one lock.
- **Not built yet:**
  - "Project work mid-phase" as a quiet-moment signal. It's `false` until the merged tree's `phases` module is wired in.
  - The hub's Updates page and the voice phrases ("install the update", "go back to the last version").



### When an update fails, it gets fixed, not dropped; friends tell you through feedback (Eric, 26 Sep)

Eric's two rulings:
- "If an update fails it needs to be reworked, bugs identified and fixed, not just dropped."
- "I don't want my friends' Atlas to tell me. I want a way for my friends to be able to submit feedback to me to tell me that there is a bug when the friend makes that determination, then I can get a report. Kind of like a feedback loop."

- **Written up where it happened, sent nowhere by itself.**
  - A failure at the health check, during probation, or on the restart is written down on that device as a report:
    - the version, the build's fingerprint and the version it was replacing;
    - the platform and the stage;
    - the build's own error lines and the crash note.
  - Your home folder and user name are scrubbed out. The device tells its owner what happened.
- **Sorted honestly.**
  - It only counts as a machine problem when the health check itself names one: a state folder that can't be written, a settings folder that can't be read, your files where the program goes, or a full disk. Then you're told what to put right, and the same build is tried again 6 hours later.
  - Anything else is the build's, including every crash and every failure with no stated reason. That exact build, by SHA-256, waits for a fix.
- **Your own devices:** a failure goes straight into your list (`atlas update failures`), and that build stops being handed out.
- **Friends' devices: they decide, through feedback (`src/feedback.rs`).**
  - The friend runs `atlas feedback send` and writes what's wrong in their own words.
  - If an update failed on their Atlas, they're shown exactly what was written down and asked whether to attach it.
  - They see the whole message, and it's sent only on a yes.
  - It travels over the pairing to the release channel's owner through its own door, `/feedback`: token-checked, 24 KB cap, notice budget. Its only destination is filing.
- **You get a report.**
  - `atlas feedback` numbers each piece: who, which Atlas version and device, their words, and anything attached.
  - An attached failure is re-sorted on your side and goes into `atlas update failures`, so `atlas update failures brief <version>` includes it.
  - A friend's feedback never stops your releases by itself. `atlas release hold <version>` is yours to use after reading it.
- **You answer; they hear it.**
  - `atlas feedback reply <n> seen|fixing|fixed <version>|wont [note]` goes back through `/feedback-answer`.
  - Their Atlas takes an answer only about feedback it really sent, and only from the person it went to, and tells them in one sentence.
  - `atlas feedback` on their side shows each piece they sent, where it stands, and your notes.
- **The fix is a new signed release, offered as usual.** The block is on the failing build, not its version label.
- **Damaged downloads:** a download damaged on disk is fetched again. A swap refused at start is logged with its reason (`not swapped in: ...`).
- **Proven:**
  - `tests/feedback_from_friends.rs` (6): the whole loop through both ends' functions; answers taken only from the right person about real feedback; the sender is the pairing, not the body; holding a build; both doors; the real `atlas feedback` and `atlas feedback reply`.
  - `adding_a_friend_is_one_step.rs`: feedback to Eric and his answer back, over real doors on real sockets between two daemons.
  - `tests/installing_a_release.rs`: nothing leaves a device by itself; sorting, scrubbing, the pause before a retry, the fix brief.
- **Not built yet:**
  - The hub's Feedback page, the voice phrases ("report a bug", "tell Eric the brief is broken"), and a desktop "send feedback" button. These are after the three-way merge.
  - Feedback about something other than Atlas's own behaviour, such as feature requests. It works today (any words), but isn't sorted or labelled.

### 27 Sep (second session): Tor ships, gets round blocks, stays open; releases pass friend to friend; updates and feedback by voice

- **AK closed -- Tor ships with Atlas.** The Tor Project's expert bundle 15.0.23 (tor 0.4.9.12 and its pluggable transports), checked against the Tor Browser Developers signing key and pinned by SHA-256 from `archive.torproject.org` (which keeps every version; `dist.torproject.org` drops old ones). The setup window fetches it with the voice pieces (`getpieces::tor`, `atlas get tor`); `windows.yml` ships it as `dist/tor/` beside `atlas.exe`, checks it, and runs `tor.exe --version` on real Windows. A test keeps the two pins identical.
- **AJ built -- the firewall rule.** The setup's new step "Letting your own devices reach Atlas" adds one rule with Windows' own `netsh`, through the Windows "allow changes" prompt, asked once (a no is remembered): only `atlas.exe`, incoming TCP, private and work networks only, from your own networks only (this subnet and Tailscale's ranges). Not yet run on Windows.
- **AM closed -- networks that block Tor.** When Tor sits without getting further for two minutes (or says "Problem bootstrapping" three times), Atlas starts it again through the bridges that ship inside the bundle (`pt_config.json`): obfs4, then Snowflake, then meek. The one that gets through is remembered for the next start. If none does, it says so once, goes back to direct and tries the round again in half an hour. Your own `kin.tor_extra` lines are never switched. Bridge programs are started from Tor's own folder, so a Windows user folder with a space in it doesn't break the line. Proven: real tor accepts every kind's lines and starts `lyrebird` (conn_done_pt); the switching, with a stand-in tor.
- **AN built -- each friend's connection kept open.** One connection per onion address, at most 16, let go after four idle minutes; every request still sealed on its own. The sealed door keeps a connection open when asked (and only the sealed door). A kept connection the far side closed is replaced once, sealed afresh. Proven with a stand-in tor: four release pieces over one connection. Not yet timed on the real Tor network.
- **AD closed -- any device holding a verified release hands it on.** A release that arrived and matched is kept to pass on. The fetching side asks whoever answered last, then the releaser, then any friend -- at most three a tick -- and keeps the file only if the whole matches the fingerprint your key signed, whoever sent the pieces. Friends whose pieces made a bad file aren't asked for it again (the releaser's own files are the signed ones and are never blamed). A passed-on file stops being handed out once a newer release is heard, or if it failed on that device. Proven over real doors: Eric's Atlas off, Sam's hands the file to Maya.
- **New gap AR (OPEN_GAPS 8.15).** `atlas release hold` isn't announced, so friends who already have a held build keep passing it on until a newer release is heard. Fix: a signed hold notice in the channel, after the key sitting.
- **Step 6 voice (8.2) and feedback voice (8.14).** "any updates", "install the update", "go back to the last version" (asked first; the yes is the person's local approval). "report a bug …" reads back exactly what will go -- with the written-down update failure if there is one, or "report a bug without the failure …" -- and sends only on yes; "any feedback", "answer feedback 2 fixing". Atlas's own window has **Report a problem with Atlas**: write it, see it, send it. These replies are read whole: "exactly what will go" had been cut to a few sentences like every other spoken reply. The friend's "install it now?" question had no way to answer it; it now says "say 'install the update'".
