# Current state, 21 October 2026

Last updated: 2026-10-21
Status: current

This is the single place for the installed build, test totals, owner readiness, the secrets vault, model planning and parked work. Every number names its source. Older numbers are history, not current.

## (a) Installed build

Installed `C:\Users\erics\AppData\Local\Atlas\atlas.exe`, SHA-256 `B0CA6B2F85DBD1C261FE9ACAE7EF126281DC86C575897B453EC7FD979B589427` (Get-FileHash, 21 Oct). Built from `candidate/2026-10-17-foundation` at `6af6238b` (on origin), which merges `fix/desktop-e2e-1020` and `fix/outstanding-2026-10-21`. Installed-runtime verifier 14/14. Rollback copy: `atlas.exe.before-merge-20261021` (3B38DE03...). Record: `verification/runs/20261021T142000Z-claude-merged-candidate/RESULTS.md`.

History, oldest first (all superseded except the last):

| Date | Hash (prefix) | Note | Source |
|---|---|---|---|
| 12 Oct | ac4692b8 | build of c797d247 | `00_SOURCE_OF_TRUTH/CURRENT_CODE_STATUS.md` (archived once replaced) |
| 17-18 Oct | FA61A885 | candidate, installed verifier runs | `.filing/ledger.jsonl` notes on i922b, 18 Oct |
| 20 Oct | 9DB17... | corrected release candidate, verifier 14/14 | `docs/STATUS.md` (before the 21 Oct edit) |
| 20 Oct | ACEF04... | label fix | `.filing/ledger.jsonl` |
| 21 Oct | 575B64E7 | intermediate, real-profile check | `.filing/ledger.jsonl` |
| 21 Oct | 3B38DE03 | e2e fixes | `verification/runs/20261021T110000Z-claude-e2e1020-fixes/RESULTS.md` |
| 21 Oct | B0CA6B2F | merged candidate, current | `verification/runs/20261021T142000Z-claude-merged-candidate/RESULTS.md` |

## (b) Test totals

Current: all targets, 9,155 passed / 20 failed, run under heavy machine load. 3 failures need a system ffmpeg (not installed on this PC); the other 17 pass when re-run alone (116/116). Source: `verification/runs/20261021T142000Z-claude-merged-candidate/RESULTS.md` and `full-suite-summary.txt` in the same folder.

History of what each number measured:

| Date | Total | What it measured | Source |
|---|---|---|---|
| 14-15 Oct | 7,136 | explicit-target aggregate (the 8,915 figure of 15 Oct had no per-target inventory) | `docs/TEST_COUNT_RECONCILIATION_2026-10-18.md` |
| 17 Oct | 7,175 / 7,184 | `tests/all.rs` target only | same |
| 18 Oct | 7,159 passed / 16 failed | `tests/all.rs` only, not the whole repo | `docs/VERIFICATION_CAMPAIGN_2026-10-18.md` |
| 18 Oct | 9,030 discovered | `cargo test --all-targets -- --list`, 45 targets (1,343 lib, 1 bin, 7,184 `all.rs`, 501 other) | `docs/TEST_COUNT_RECONCILIATION_2026-10-18.md` |
| 19 Oct | 7,005 passed / 16 failed | serialized lean all-target campaign | `docs/STATUS.md` (before the 21 Oct edit) |
| 20 Oct | 7,171 / 4, lib 1,346 | focused campaign | `.filing/ledger.jsonl` |
| 21 Oct | 9,138 | after e2e fixes | `verification/runs/20261021T110000Z-claude-e2e1020-fixes/RESULTS.md` |
| 21 Oct | 9,155 passed / 20 failed | merged candidate, all targets, current | merged-candidate RESULTS.md |

## (c) Owner readiness

The gate is `docs/OWNER_READINESS_RESET_2026-10-16.md`, tracked as ledger item `i922b`. Atlas is not to be called an operating system until the installed build demonstrates the minimum operating foundation there and Eric accepts it. Green tests and the 14/14 verifier are supporting evidence, not acceptance. Still unaccepted: the rest of the minimum operating-foundation journey, resource and dependency readiness, live-state restore evidence policy, live hardware, visual UI and provider checks, and Eric's own acceptance (next step: try Atlas for a real day, then decide the items in `owner-after-e2e1020`, see `verification/runs/20261021T110000Z-claude-e2e1020-fixes/RESULTS.md`).

## (d) Secrets vault

Source: `00_SOURCE_OF_TRUTH/STATE/open_items.tsv`.
- Q7 "One vault for secrets": status doing, owner ERIC. Secrets were moved to `C:\Users\erics\Atlas-Vault\secrets`; the folder is not EFS-encrypted and BitLocker status was never checked (8 Oct note).
- CARRY1: status open, owner ERIC. a stray `dev2.key` test key (path in the control ledger, item CARRY1) sits outside any repo, untracked, never opened; move it to the vault or ignore it.
- Android keystore: `C:/Users/erics/Atlas-Vault/signing/` holds `atlas-android.p12` and `atlas-android.pass` (metadata check in `00_SOURCE_OF_TRUTH/STATE/changelog.tsv`, 2026-10-08 13:32, item S1; no contents read). Losing it means no in-place Android updates (`00_SOURCE_OF_TRUTH/RELEASE_SIGNING.md`).
- The off-laptop protected copy still does not exist and needs Eric. No backup or recovery is claimed.
- ID collisions: the control-ledger item Q7 (this vault) is a different thing from SEC6's note "Audit Q7" (Microsoft refresh tokens, done) and from the "Q7" owner question in `docs/ATLAS_EFFICIENCY_AND_ADAPTATION_DESIGN_2026-10-20.md` (device tier T0, "provisional until Q7 is answered"). Say which one you mean.

## (e) Model planning

| Item | Doc | One line |
|---|---|---|
| llm-missing | `verification/runs/20261020T040916Z-claude-typed-acceptance/RESULTS.md` | Installed Atlas had no local language model; the decision to install one. |
| own-model | `docs/OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md` | From-scratch training is unrealistic; small open Apache-2.0 base plus adapters; no Claude or ChatGPT outputs as training data. |
| offline-assistant | `docs/OFFLINE_ASSISTANT_DESIGN_2026-10-20.md` | Full offline typed and voice assistant that keeps the PC usable; section 8 lists what to download. |
| efficiency | `docs/ATLAS_EFFICIENCY_AND_ADAPTATION_DESIGN_2026-10-20.md` | Device tiers T0-T3, budgets and adaptation. |
| model-training | `docs/ATLAS_MODEL_TRAINING_PLAN_2026-10-21.md` | Staged plan to train Atlas's own model from Eric's data and clean open datasets; nothing trained. |
| deferred 18 Oct plan | `00_SOURCE_OF_TRUTH/deferred-atlas-plans/ATLAS_QUALITY_SPEED_AND_LOCAL_MODELS_PLAN_2026-10-18.md` | Earlier quality, speed and local-model plan, deferred. |

ANSWER from Eric (21 Oct, via Dispatch): he chose the smaller core set of about 4.3 GB, the 4B plus 1.7B models (`docs/OFFLINE_ASSISTANT_DESIGN_2026-10-20.md`, section 8). Not the 16.2 GB pieces set, and not the 2.5 GB single 4B. The online backup (`online_second`) stays ON. The download itself is being handled by another session and is not claimed done here.

## (f) Parked under the feature freeze

Charter section 4 (`00_SOURCE_OF_TRUTH/ATLAS_DIRECTION_AND_RECOVERY_CHARTER.md`):
- BIZ1 Opportunity-to-Offer Engine (`01_DESIGN_INTENT/OPPORTUNITY_TO_OFFER_ENGINE.md`).
- BIZ2 Product Recipe Library (`01_DESIGN_INTENT/PRODUCT_RECIPE_LIBRARY.md`).
- Creative Workspace Core (`docs/HANDOFF_2026-10-19_CREATIVE_WORKSPACE.md`, scope in `docs/CREATIVE_WORKSPACE_CORE_SCOPE_2026-10-18.md`); source slice kept, unwired.

## (g) Historical documents

- `atlas/docs/OPEN_ITEMS.md` is frozen by design ("STALE ... last true 8 Sep"). It stays in place because `atlas/tests/bug_sweep.rs` references its section names. Read it as history.
- The dated handoffs (`docs/ATLAS_FULL_HANDOFF_2026-10-17.md`, `docs/ATLAS_FULL_HANDOFF_2026-10-18.md`, `docs/ATLAS_NEXT_CHAT_HANDOFF_2026-10-17.md`, `docs/CHAT_HANDOFF_2026-10-18.md`, `docs/HANDOFF_2026-10-19_CREATIVE_WORKSPACE.md`, `docs/CLAUDE_DESKTOP_ATLAS_ACCEPTANCE_HANDOFF_2026-10-20.md`) describe the state on their dates. Where they disagree with this page, this page wins.
