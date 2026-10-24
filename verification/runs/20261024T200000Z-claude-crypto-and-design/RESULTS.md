# RUSTSEC-2026-0387 fixed (p256 0.14) and Hub design tuning: 2026-10-24 (Claude, usage-max routed)

Branch `fix/crypto-p256-2026-10-24` (4e26b1a9, 9cbc2f01, ac30fb46, d07b726d) + merged `fix/hub-design-2026-10-24` (159d8b69, 1e6ba6b5), on candidate `81ffee8d`.

## Crypto
- p256 0.13 → 0.14.0 (stable 2026-07-03). The cascade stayed inside the signing crates: ecdsa 0.17, elliptic-curve 0.14.2, rfc6979 0.6, sec1 0.8, pkcs8 0.11, der 0.8, spki 0.8, signature 3, digest 0.11, hmac 0.13, hkdf 0.13, sha2 0.11 (sha2 0.10 still used elsewhere). No rand change, no toolchain change.
- crypto-bigint now 0.7.7. The advisory marks >= 0.7.6 as patched; 0.7.6 (23 Oct) and 0.7.7 (24 Oct) are brand new.
- `src/apns.rs` / `src/applewx.rs` unchanged (same 64-byte r||s API). Only API break: `to_encoded_point` → `to_sec1_point` in `src/webpush.rs`.
- Tests: RFC 6979 A.2.5 P-256/SHA-256 known answer (r exact, s up to low-s); tampered token and wrong key rejected; deterministic; 64-byte signature, base64url without padding.
- `cargo deny check advisories`: ok, RUSTSEC-2026-0387 gone. `cargo audit`: only the already-documented unmaintained crates.

## Pre-existing failures found and fixed
- `the_splitter_is_honest`: a `#[cfg(test)]` helper outside a module in `src/store.rs` (added by the Hub notice fix). Removed; the caller uses `note_set_aside`.
- `opportunity_hunting::tell_me_more_then_that_kind_by_voice`: not a product bug. A calendar time-bomb: the test dates its samples to 29 Sep while spoken asks read the real clock, so one sample passed its 30-day shelf (fails on 89a937da too). Samples are re-dated to now; original assertion restored; new test that the spoken list keeps its top three.
- `personal_atlas_is_its_own` fingerprint guard walked a build folder named `target-p256` and flagged a test's temp file. It now skips any cargo build folder by its `CACHEDIR.TAG`, so source folders are still read.

## Hub design (Warm Paper kept)
- No text under 12px (guard test parses hub.css including `font:` shorthands; proven to fail at 11px).
- Header solid, with a 2px blur only under `@supports`; no blur on the palette scrim.
- Tracked uppercase only on the three nav eyebrows.
- Type scale tokens `--t-page/section/card/body/meta`, all used (guard test).
- Decorative side borders removed; state borders kept (failed/interrupted, lanes).
- Icon link on every hand-written page (busy, 202, request pages, phone add, offline, update); runtime test on served heads.
- Lighthouse (release build, disposable home, 4 pages x desktop/mobile): performance, accessibility and best-practices 100 on all 8 runs; SEO 90 (missing meta description, unchanged).
- impeccable: tiny-text, undersized-ui-text and flat-type-hierarchy gone. Left: the calendar stripes gradient and some Home spacing flags (structural).
- Review: dark-theme parity and AA contrast pass; one miss (five sub-12px rules) fixed.

## Checks
- Full `--all-targets` on the merged code (before d07b726d): 9,369 passed, 4 failed. 3 env (camera/ffmpeg); 1 = the fingerprint guard, fixed in d07b726d (guard set + that test 82/82 after).
- Installed `11135EC4...` (verifier 14/14 on the first run, hash matched), rollback `atlas-0.1.0-dev.previous` = `131E451E...`.
- Real profile start/stop: Hub 10.3 s, clean stop 1.2 s. Min free memory 524 MB during the run, caused by other programs (a 1.2 GB python process, node). Atlas loaded no model and logged "memory is short: nothing idle to let go".

## Not verified / open
- whisper-server was still running at the instant Atlas exited ("0 helper(s) shut down"), gone within a minute. Earlier runs counted it as a helper. Possibly a late-started recogniser not tracked as a helper; not investigated.
- No full run after d07b726d (test-only change).
- crypto-bigint 0.7.7 is one day old.
- Design screenshots are static renders with the CSS inlined, not live pages.
