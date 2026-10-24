# Security and quality scans of Atlas: 2026-10-24 (Claude, usage-max routed)

Owner-approved, defensive, against Atlas's own code and its own Hub on 127.0.0.1 only. Full ranked table: `FINDINGS.md`.

## PHP
Atlas contains, ships and runs no PHP. The only mentions are file-type lists for reading the owner's folders, plus one web-link pattern. Nothing to patch.

## Tools (licences checked)
cargo-audit (MIT/Apache-2.0), cargo-deny (MIT/Apache-2.0), gitleaks (MIT), osv-scanner (Apache-2.0, run offline), Lighthouse (Apache-2.0, bundles axe-core MPL-2.0; test use only), impeccable (pbakaus/impeccable, Apache-2.0, telemetry off), taste-skill (Leonxlnx/taste-skill, MIT; skill text read and applied as a checklist, not installed into any agent).
Not run:
- strix (usestrix/strix, Apache-2.0): needs Docker (not installed) and a capable local model. The owner asked for local-only.
- xtool: not found in the owner's GitHub account (7 repos, no stars) or as a clear public match.

## Findings by impact
1. RUSTSEC-2026-0387, crypto-bigint 0.5.5 timing side-channel, via p256 0.13 used to sign Apple JWTs (`src/apns.rs`, `src/applewx.rs`). Reachable only when signing with the owner's own key on his machine. Fix needs p256 0.14 (semver-major). Open, owner decision. `cargo deny` stays red on it on purpose.
2. Four newer advisories (unsafe-libyaml via deprecated serde_yaml; zvariant / zbus_names / zvariant_derive, Linux-only): documented ignores with removal conditions in `deny.toml`.
3. glib 0.18.5 and rand 0.7.3: Linux-only wry backend, not compiled on Windows.
4. gitleaks: 45 hits, all test fixtures, comments, a token-less curl example or generated hashes. No real secrets.
5. Lighthouse/axe on /hub, /hub/now, /hub/outstanding, /hub/settings, desktop and mobile: performance and accessibility 100; best-practices 96. The Find button's label didn't contain its visible text (axe label-content-name-mismatch), and every page 404'd on /favicon.ico. Both fixed (fc3ca8ce, unit tests). Re-run: 100/100/100 on all 8 runs.
6. Design (impeccable, taste-skill): backdrop blur (hub.css 158, 483), text under 11px, many tracked uppercase labels, side-border accents, weak type hierarchy. Style decisions, left for the owner.

## Checks
- Review: PASS. Gap: the 503 busy page and `requests.rs` pages have their own heads without the icon link (cosmetic 404).
- Guard tests 117, Hub tests 206, lib hub/hublive/server 82: all pass.
- Installed `131E451E...` (verifier 14/14, hash-checked), rollback `atlas-0.1.0-dev.previous` = `988E068A...`.
- Incident: a first release build was killed mid-compile (no error in the log, exit 127). The install step then copied the previous binary and rotated the rollback to the same build. Nothing was harmed (installed stayed on verified 988E068A; 487512C3 is in the Recycle Bin, as with every rotation). The install step now copies only on a successful build that produced a newer exe, and installs only when the verifier's hash matches.

## Not verified
osv-scanner skipped `vendor/webview2-com-sys/Cargo.lock`; impeccable's colour rules were incomplete offline; the design findings are rule readings, not tested changes; no full test run on this branch (scope: deny.toml plus two Hub head/label lines).
