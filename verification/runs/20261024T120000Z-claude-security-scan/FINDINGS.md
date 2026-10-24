# Atlas security / quality scan, 24 Oct 2026
Worktree: C:\Users\erics\Atlas\wt-scan\atlas, branch fix/security-scan-2026-10-24 (off d55bef25). Not pushed.
Tools: cargo-audit 0.22.2 (built --locked), cargo-deny 0.20.2, gitleaks 8.30.1, osv-scanner 2.7.0 (offline DBs), Lighthouse 13.5.0, impeccable 4.7.0, taste-skill (design-taste-frontend + references/app-screens.md). All licences confirmed (MIT / Apache-2.0).

## Ranked findings
| # | Tool | Id | Location | Impact | Fix proposal | Status |
|---|------|----|----------|--------|--------------|--------|
| 1 | audit, deny, osv | RUSTSEC-2026-0387 | crypto-bigint 0.5.5 <- elliptic-curve 0.13.8 <- ecdsa 0.16.9 <- p256 0.13.2; reached via ECDSA signing in src/apns.rs:179-186 and src/applewx.rs:234-259 | Timing side channel in constant-time ops. Reachable, but only when signing JWTs with the owner's own Apple keys on the owner's machine; an attacker needs a local timing oracle. Low practical risk. | Needs p256 0.14 / ecdsa 0.17 (semver-major, API change). `cargo update -p crypto-bigint` cannot reach >=0.7.6. Owner decision. | OPEN, deny stays red on this one on purpose |
| 2 | audit, deny, osv | RUSTSEC-2024-0449 | unsafe-libyaml 0.2.11 <- serde_yaml 0.9 (deprecated) | Unmaintained; in real builds; parses Atlas's own YAML | Swap serde_yaml for a maintained fork (e.g. serde_yml / serde_norway): dependency swap, owner. | Documented in deny.toml |
| 3 | deny, osv | RUSTSEC-2026-0370/0371/0372 | zvariant, zbus_names, zvariant_derive 3.x <- accesskit_unix <- eframe 0.28 | Unmaintained, Linux-only, not compiled on Windows | Gone with eframe >= 0.36 | Documented in deny.toml |
| 4 | osv | RUSTSEC-2024-0429 (glib 0.18.5, CVSS 6.9) | wry Linux GTK backend | Not compiled on Windows (cargo tree on windows target empty) | wry upgrade | Not fixed, Linux-only |
| 5 | osv | RUSTSEC-2026-0097 (rand 0.7.3) | build-dep of selectors/phf_generator under wry on Linux | Not compiled on Windows; pinned by phf_generator 0.8 | wry/kuchikiki upgrade | Not fixed, build-time/Linux only |
| 6 | audit, deny | ttf-parser 0.25.1, paste 1.0.15, instant 0.1.13, derivative, proc-macro-error, fxhash | various | Unmaintained (already in deny.toml ignore list, except instant: dev/other targets) | eframe 0.36 upgrade | Known, unchanged |
| 7 | gitleaks | 35 history hits, 10 working-tree hits | see below | NO real secrets. All false positives or test fixtures: src/wire.rs:25 and tests/vault_crypto.rs:329 (cipher names in comments), tests/phone_link.rs:21 ("nodekey:0123456789abcdef"), tests/your_answers_of_the_25th.rs (scrub() test inputs), tests/fixtures/crawl/go-loopvar.html:1474 (third-party page), docs/SENDING_FROM_YOUR_PHONE.md:180 (curl example, no token), MODULE_REFERENCE_*.md (generated hashes), training/tests/test_scrub.py:40 (scrub test), tests/the_hub_is_reachable.rs, opportunity_hunting.rs, muse_and_thinking_hard.rs, src/colony/assets.rs:385 | none | Optional: add a .gitleaksignore / allowlist so a future CI run is quiet. | No action needed |
| 8 | Lighthouse (axe) | label-content-name-mismatch | /hub, div.living > section.rightnow > form.ask > button, aria-label=Find vs visible "Enter" | Voice-control users saying "click Enter" fail | aria-label="Enter to find" | FIXED, test added |
| 9 | Lighthouse | errors-in-console | all 4 pages: GET /favicon.ico 404 | Console error, best-practices 96 | inline `<link rel=icon href=data:,>` in hub::frame::shell | FIXED, test added |
| 10 | Lighthouse | bf-cache (MainResourceHasCacheControlNoStore) | all pages | Back/forward cache disabled | Intentional: Cache-Control: no-store protects the token-bearing pages. Leave. | Not a bug |
| 11 | Lighthouse | forced-reflow-insight, network-dependency-tree-insight | hub.js / stylesheet chain | Informational; performance score 100 on all runs | none | n/a |
| 12 | taste-skill (app-screens 13/14) | backdrop-filter blur on opaque-surface rule | hub.css:158 (header glass), :483 (palette scrim) | Banned "glass / backdrop blur" | Opaque header; owner design call | Owner |
| 13 | taste-skill | text under 11px | hub.css:204, 279-281, 619, 806, 879 (10 and 10.5px) | Rule: nothing under 11px; also small for legibility | Raise to 11px after a visual check | Owner (layout risk) |
| 14 | taste-skill | more than one tracked uppercase label | hub.css:150, 205, 225, 310, 323, 333, 380, 393 | Rule: at most one tracked uppercase label per screen | Sentence-case labels | Owner (redesign) |
| 15 | impeccable | side-tab (7) | hub.css:330-332, 577, 804, 981, 1012 | "Side-tab accent border" tell; these are status accents on rows | Keep or use tone step | Owner |
| 16 | impeccable | flat-type-hierarchy (hub, outstanding, settings), monotonous-spacing (settings) | rendered pages (stylesheet hashed link not resolvable offline, colour rules incomplete) | Advisory | Wider type scale steps | Owner |

## Fixed (2 commits, not pushed)
- eb174c59 deny.toml: reasons for four new advisories (items 2, 3). Re-run: `cargo deny check advisories bans licenses sources` now fails only on RUSTSEC-2026-0387; bans, licenses, sources ok.
- fc3ca8ce Hub: items 8 and 9. Unit tests: hub::deck::accessible_name_tests, hub::frame::icon_tests. Re-ran Lighthouse against the worktree build (disposable ATLAS_HOME): accessibility 100, best-practices 100, performance 100 on all 8 runs, with no failing axe or console audits except informational bf-cache/insights.
- Tests after fixes: `--test all -- dead_ catalogue guards wiring no_quiet_nothings retrospective hub_is_not_code` 117 passed; `--test all -- hub` 206 passed; `--lib -- hub hublive server` 82 passed; 0 failed.

## Needs the owner
1. p256 0.13 -> 0.14 (clears RUSTSEC-2026-0387; deny gate is red until then or until the owner chooses to ignore with a reason).
2. serde_yaml replacement (RUSTSEC-2024-0449).
3. eframe 0.36 / wry upgrade (clears zbus, ttf-parser, paste, glib, rand 0.7).
4. Hub visual rules (12-16) if the taste-skill app-screen rules are wanted.
5. osv-scanner skipped vendor/webview2-com-sys/Cargo.lock (not scanned) and mobile/ios/requirements-signing.txt was included in the scan with no hits.
