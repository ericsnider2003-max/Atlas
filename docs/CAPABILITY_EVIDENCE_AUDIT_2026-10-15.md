# Capability evidence audit — 2026-10-15

Last updated: 2026-10-15

The generated catalog currently exposes 247 Windows capabilities through the
shared `capability::records` projection:

- 58 available and source-proven as working;
- 23 built but switched off;
- 16 available only after an explicit dependency is installed;
- 149 available in code but still unproven on this machine.

The counts are intentionally not collapsed into a single “ready” number. The
typed record keeps source proof, installed/dependency proof, and owner/device
proof distinct. The catalog, Hub, voice/status summaries, and generated
capability document consume the same projection.

Foundation verification is currently stronger than the catalog’s owner/device
claims: the explicit Cargo-target run passed 8,915 tests across 44 targets with
0 failures and 26 intentionally ignored long-running measurements. The F7
sync/handoff slice passed 85/85, the adapter slice passed 163/163, and the F8
calendar/publication/finance slice passed 180/180. These are source and local
installed proofs; they do not silently promote real microphone, provider,
phone, or owner-selected-folder observations.

Remaining work is therefore evidence promotion and vertical implementation,
not a missing second capability catalog: collect installed and owner/device
observations where available, keep dependency-gated rows honest, and route any
new long-running adapter through the durable job/evidence/receipt path before
calling it ready.
