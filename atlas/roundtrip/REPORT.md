# Code audit: atlas
_2026-10-09 · scope: the whole project · checks loaded: trading-money, windows-host, scripts-ops, web-app_

**Readiness: Blocked — 1 action only you can take**

## Do today
- Confirm whether any flagged value is real and rotate it outside Atlas if so. (SEC-001)
- Do not use any flagged credential; revoke it if real. (SEC-001)

## The short version
- **What's broken:** Atlas must complete jobs truthfully and protect data.
- **What it could cost:** A failed job can lose data, mislead users, expose private information, or trigger unauthorized action.
- **What fixing takes:** about 66 hours (44–88 h, 5.5–11 working days) across 8 fixes.
- **What we need from you:** Prioritize reliability defects; block protected actions until explicitly trusted.

## Fix order
Most serious tier first; quick fixes lead only inside a tier.
1. **Critical — fix first** (row 1): SEC-001 · about 9 hours (6–12 h, 1–1.5 working days)
2. **High — next** (rows 2–7): BUG-002 first (quick), then BUG-001, BUG-003, BUG-004, BUG-005, BUG-006 · about 48 hours (32–64 h, 4–8 working days)
3. **Medium — before it grows** (row 8): 1 fix · about 9 hours (6–12 h, 1–1.5 working days)

## What could happen
### Long job cancellation
What happens · A long job is cancelled and restarted. · How likely · Frequent. · Would you notice? · Sometimes. · How recover · Restart. · Fixed by · Truthful cancellation receipts. (BUG-003, BUG-005)
### File recovery
What happens · A file operation stops. · How likely · Common. · Would you notice? · Not always. · How recover · Search manually. · Fixed by · Recovery receipts. (BUG-007)
### Restricted action
What happens · Atlas is asked to access a protected account. · How likely · Expected in testing. · Would you notice? · If status is truthful. · How recover · Revoke manually. · Fixed by · Trust gates. (BUG-001)

## All findings
In fix order. Effort: **S** = 2–4 hours, **M** = 6–12 hours, **L** = 16–32 hours. Severity: **critical** can lose money or hand over control now · **high** breaks normal operation · **medium** needs narrower conditions · **low** is housekeeping.

| # | Id | Severity | Effort | What's wrong | How you'll know it's fixed |
|--:|---|---|---|---|---|
| 1 | SEC-001 | critical | M | **Secret-like values are present in tracked material.** _(unconfirmed)_ | Inspect flagged values without exposing them and confirm they are synthetic or rotate any real credential. |
| 2 | BUG-002 | high | S | **Generated command documentation may use stale state.** _(unconfirmed)_ | Change one command entry, regenerate, and confirm the new wording appears. |
| 3 | BUG-001 | high | M | **Mobile registration accepts a caller-provided web address.** _(unconfirmed)_ | Run it with an untrusted address and confirm it is rejected or limited to Apple hosts. |
| 4 | BUG-003 | high | M | **Input paths still contain panic-prone unwraps.** _(unconfirmed)_ | Exercise flagged paths with missing and malformed values and confirm an error is returned. |
| 5 | BUG-004 | high | M | **Numeric conversions may silently lose values.** _(unconfirmed)_ | Exercise boundary values and confirm invalid conversions fail clearly. |
| 6 | BUG-005 | high | M | **Some operation failures may be ignored.** _(unconfirmed)_ | Force the flagged operation to fail and confirm the failure reaches status and logs. |
| 7 | BUG-006 | high | M | **State imports may accept unknown fields silently.** _(unconfirmed)_ | Load state containing an unknown field and confirm the chosen compatibility behavior is explicit. |
| 8 | BUG-007 | medium | M | **Missing state can fall back to defaults without clear status.** _(unconfirmed)_ | Remove or corrupt state and confirm Atlas reports the condition and preserves recoverability. |

---
## For your engineer

#### SEC-001 · critical · M
- What it means: A secret-like value appears in tracked Atlas material; if real, it could expose private data.
- Where: `src/worklog.rs:580`, `tests/ports_round3.rs:302`, `tests/round11.rs:82`
- Evidence (not yet confirmed): Secret-shaped values were found in a work-log test and two fixtures; confirm they are synthetic.
- Control: Secrets must not be committed or retained in logs. (ASVS V6)
- Fix: Remove real credentials, replace fixtures with placeholders, and add a scan gate.
- Test first: Run the secret scan after replacement and confirm no usable credential remains.

#### BUG-002 · high · S
- What it means: The command reference may describe earlier settings, so Atlas can appear to support a command that no longer works.
- Where: `docs/gencatalog_extra.py:115`
- Evidence (not yet confirmed): The generator carries pending entries while reading the catalog; a flagged state pattern needs a focused regeneration check.
- Fix: Reload the working entry at each boundary and add a regeneration check.
- Test first: Regenerate after a temporary catalog change and compare output.

#### BUG-001 · high · M
- What it means: A mobile setup helper may contact an address supplied by the caller, exposing a private service or credentials.
- Where: `mobile/ios/asc_profiles.py:70`, `mobile/ios/register_devices.py:81`
- Evidence (not yet confirmed): Both helpers build requests from a caller-provided path or address; allowed host and redirect rules need confirmation.
- Fix: Allow only fixed Apple hosts and reject absolute addresses before requests.
- Test first: Test allowed Apple paths and rejected private or external addresses.

#### BUG-003 · high · M
- What it means: Some input or state paths may panic instead of returning a controlled failure.
- Where: `src/childjob.rs:1`
- Evidence (not yet confirmed): The scanner found unwrap calls in input-handling code; each call needs a focused check.
- Fix: Replace unsafe unwraps with explicit error propagation where input is untrusted.
- Test first: Run targeted failure cases and the existing Rust test suite.

#### BUG-004 · high · M
- What it means: Some numeric conversions may truncate or overflow without a user-visible error.
- Where: `src/connection_removal.rs:1`
- Evidence (not yet confirmed): The scanner found lossy casts in shipped Rust code; boundary behavior needs confirmation.
- Fix: Use checked conversions or explicit range handling.
- Test first: Run boundary-value tests for each affected path.

#### BUG-005 · high · M
- What it means: Ignored results can make Atlas report progress when an operation actually failed.
- Where: `src/childjob.rs:1`
- Evidence (not yet confirmed): The scanner found ignored results in shipped Rust code.
- Fix: Handle or explicitly document every result according to the operation contract.
- Test first: Run forced-failure checks for affected operations.

#### BUG-006 · high · M
- What it means: Persisted state may load while ignoring fields it does not understand, hiding corruption or version drift.
- Where: `src/store.rs:1`
- Evidence (not yet confirmed): The scanner found many deserializers without an explicit unknown-field policy.
- Fix: Choose and test strict rejection or deliberate migration handling.
- Test first: Run state compatibility tests with unknown and missing fields.

#### BUG-007 · medium · M
- What it means: A missing or unreadable state file may be treated as a fresh default, risking silent loss of continuity.
- Where: `src/store.rs:1`
- Evidence (not yet confirmed): The scanner found defaulting state reads in shipped Rust code.
- Fix: Surface the read failure and require an explicit recovery path.
- Test first: Run missing, corrupt, and permission-denied state checks.

## Script inventory
| Script | Destructive | Dry run | Stops on error | Lock | Logs | Exit code | Started by |
|---|---|---|---|---|---|---|---|
| `ATLAS.bat` | no | n/a | n/a | **no** | **no** | **no** | config/tools.yaml, scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json |
| `docs/live/LIVE_TESTS_R6.sh` | **yes** | **no** | **no** | **no** | **no** | **no** | scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json, scratch/atlas-addons-schedule-corrupt-18172/state/index.json |
| `mobile/android/gradlew.bat` | no | n/a | n/a | **no** | **no** | **no** | scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json, scratch/atlas-addons-schedule-corrupt-18172/state/index.json |
| `mobile/build-android.sh` | no | n/a | yes | **no** | **no** | yes | scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json, scratch/atlas-addons-schedule-corrupt-18172/state/index.json |
| `mobile/ios-certificate.sh` | no | n/a | yes | **no** | **no** | yes | scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json, scratch/atlas-addons-schedule-corrupt-18172/state/index.json |
| `mobile/sign-android.sh` | no | n/a | yes | **no** | **no** | yes | mobile/build-android.sh, scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json |
| `scratch/atlas-finishing-media-approval-16220/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-approval-18172/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-approval-20072/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-approval-26888/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-approval-7688/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-failure-16220/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-failure-18172/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-failure-20072/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-failure-26888/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-failure-7688/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-no-output-16220/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-no-output-18172/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-no-output-20072/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-no-output-26888/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-finishing-media-no-output-7688/render.cmd` | no | n/a | n/a | **no** | **no** | **no** | not found |
| `scratch/atlas-pathext-40460/npm.cmd` | no | n/a | n/a | **no** | **no** | **no** | scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json, scratch/atlas-addons-schedule-corrupt-18172/state/index.json |
| `scratch/atlas-pathext-7464/npm.cmd` | no | n/a | n/a | **no** | **no** | **no** | scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json, scratch/atlas-addons-schedule-corrupt-18172/state/index.json |
| `setup/reference/RUN-TESTS.bat` | no | n/a | n/a | **no** | **no** | **no** | scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json, scratch/atlas-addons-schedule-corrupt-18172/state/index.json |
| `setup/wine/test-windows-build.sh` | no | n/a | **no** | **no** | **no** | **no** | scratch/atlas-addons-auto-sync-laptop-18172/state/index.json, scratch/atlas-addons-auto-sync-phone-18172/state/index.json, scratch/atlas-addons-schedule-corrupt-18172/state/index.json |
| `tests/fixtures/speech/wake/make_wake.sh` | no | n/a | yes | **no** | **no** | **no** | not found |

## Coverage
| Area | Result | Note |
|---|---|---|
| Reliability and job lifecycle | Needs verification | Confirm high leads |
| Data and file safety | Needs verification | Confirm recovery paths |
| Protected external actions | Needs verification | Confirm trust gates |
| Cross-device operation | Gap | Other platforms untested |
| Optional security scanners | Gap | Unavailable in baseline |
| Baseline scan | Complete | 948 triaged |

**Not checked**
- pip-audit not installed — Python dependency advisories NOT checked (dev machine: pip install pip-audit)
- cargo-audit not installed — Rust dependency advisories NOT checked
- semgrep not installed — data-flow security rules across languages NOT checked (dev machine only: pip install semgrep)
- gitleaks not installed — secrets in files and git history NOT checked (dev machine only: https://github.com/gitleaks/gitleaks/releases)
- osv-scanner not installed — known-vulnerable dependencies in every lockfile NOT checked (dev machine only: https://google.github.io/osv-scanner/installation/)
- ruff not installed — Python bugs and security lint NOT checked (dev machine only: pip install ruff)
- bandit not installed — Python security (syntax-tree based) NOT checked (dev machine only: pip install bandit)
- shellcheck not installed — bash and sh script bugs NOT checked (dev machine only: apt install shellcheck  (or: brew install shellcheck / winget install koalaman.shellcheck))
- .: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-craft-e2e-both: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-craft-e2e-compiles: test — no automated tests (running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)
- scratch/atlas-craft-e2e-py-pyproject.toml: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-craft-e2e-py-requirements.txt: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-craft-e2e-py-setup.py: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-craft-e2e-stray: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-craft-e2e-working: test — no automated tests (running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)
- scratch/atlas-hollow-project-40460: test — no automated tests (running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)
- scratch/atlas-hollow-project-7464: test — no automated tests (running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s)
- scratch/atlas-model-for-code-budget-18172: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-model-for-code-error-18172: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-model-for-code-named-18172: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-model-for-code-words-18172: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-real-code-proj-18172/receipts-app: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-real-code-proj-31488/receipts-app: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-real-code-proj-7688/receipts-app: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-runbook-manifest-18172: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/atlas-runbook-manifest-7688: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/cargo-mutants-atlas-3E8wLe.tmp: test (python) — no automated tests (Python code but no test_*.py files)
- scratch/cargo-mutants-atlas-fouPP4.tmp: test (python) — no automated tests (Python code but no test_*.py files)
- Other-platform and live-account runs untested.

**Leads:** 948 of 948 scanner leads triaged: 7 became findings, 385 dismissed with a reason, 556 unconfirmed; 0 accepted risks suppressed. Each dismissal and its reason: [LEADS.md](LEADS.md).

**Assumptions** (each decides something):
- Runs on mixed (confirmed) — from PowerShell or batch scripts; decides which host checks apply
- Not connected to real money (confirmed) — from no live markers; decides whether tests may run, and severity
- This audit runs on a development machine, not the live host (confirmed) — from default until the owner confirms; decides whether anything may be installed

**Challenge pass:** Re-hunt critical/high leads; revisit recovery and trust.

## Baseline
| Check | Command | Result |
|---|---|---|
| test | `cargo test --quiet` | FAIL —   The system cannot find the path specified. (os error 3) |
| build | `cargo build --quiet` | FAIL —   The system cannot find the path specified. (os error 3) |
| test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-craft-e2e-both: test | `cargo test --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-craft-e2e-both: build | `cargo build --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-craft-e2e-both: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-craft-e2e-broken: test | `cargo test --quiet` | FAIL — error: could not compile `toy` (bin "toy" test) due to 1 previous error |
| scratch/atlas-craft-e2e-broken: build | `cargo build --quiet` | FAIL — error: could not compile `toy` (bin "toy") due to 1 previous error |
| scratch/atlas-craft-e2e-compiles: test | `cargo test --quiet` | NO_TESTS — test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| scratch/atlas-craft-e2e-compiles: build | `cargo build --quiet` | PASS |
| scratch/atlas-craft-e2e-py-pyproject.toml: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-craft-e2e-py-requirements.txt: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-craft-e2e-py-setup.py: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-craft-e2e-rust-empty: test | `cargo test --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-craft-e2e-rust-empty: build | `cargo build --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-craft-e2e-stray: test | `cargo test --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-craft-e2e-stray: build | `cargo build --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-craft-e2e-stray: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-craft-e2e-working: test | `cargo test --quiet` | NO_TESTS — test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| scratch/atlas-craft-e2e-working: build | `cargo build --quiet` | PASS |
| scratch/atlas-gtd-not-source-18172: test | `cargo test --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-gtd-not-source-18172: build | `cargo build --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-gtd-not-source-7688: test | `cargo test --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-gtd-not-source-7688: build | `cargo build --quiet` | FAIL —   either src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present |
| scratch/atlas-hollow-project-40460: test | `cargo test --quiet` | NO_TESTS — test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| scratch/atlas-hollow-project-40460: build | `cargo build --quiet` | PASS |
| scratch/atlas-hollow-project-7464: test | `cargo test --quiet` | NO_TESTS — test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s |
| scratch/atlas-hollow-project-7464: build | `cargo build --quiet` | PASS |
| scratch/atlas-model-for-code-budget-18172: test | `cargo test --quiet` | FAIL — error: could not compile `ledger` (bin "ledger" test) due to 1 previous error |
| scratch/atlas-model-for-code-budget-18172: build | `cargo build --quiet` | FAIL — error: could not compile `ledger` (bin "ledger") due to 1 previous error |
| scratch/atlas-model-for-code-budget-18172: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-model-for-code-error-18172: test | `cargo test --quiet` | FAIL — error: could not compile `ledger` (bin "ledger" test) due to 1 previous error |
| scratch/atlas-model-for-code-error-18172: build | `cargo build --quiet` | FAIL — error: could not compile `ledger` (bin "ledger") due to 1 previous error |
| scratch/atlas-model-for-code-error-18172: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-model-for-code-named-18172: test | `cargo test --quiet` | FAIL — error: could not compile `ledger` (bin "ledger" test) due to 1 previous error |
| scratch/atlas-model-for-code-named-18172: build | `cargo build --quiet` | FAIL — error: could not compile `ledger` (bin "ledger") due to 1 previous error |
| scratch/atlas-model-for-code-named-18172: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-model-for-code-own-tests-18172/doubling: test | `cargo test --quiet` | FAIL — error: test failed, to rerun pass `--test doubles` |
| scratch/atlas-model-for-code-own-tests-18172/doubling: build | `cargo build --quiet` | PASS |
| scratch/atlas-model-for-code-words-18172: test | `cargo test --quiet` | FAIL — error: could not compile `ledger` (bin "ledger" test) due to 1 previous error |
| scratch/atlas-model-for-code-words-18172: build | `cargo build --quiet` | FAIL — error: could not compile `ledger` (bin "ledger") due to 1 previous error |
| scratch/atlas-model-for-code-words-18172: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-real-code-proj-18172/receipts-app: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-real-code-proj-31488/receipts-app: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-real-code-proj-7688/receipts-app: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-runbook-manifest-18172: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/atlas-runbook-manifest-7688: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/cargo-mutants-atlas-3E8wLe.tmp: test | `cargo test --quiet` | FAIL —   The system cannot find the path specified. (os error 3) |
| scratch/cargo-mutants-atlas-3E8wLe.tmp: build | `cargo build --quiet` | FAIL —   The system cannot find the path specified. (os error 3) |
| scratch/cargo-mutants-atlas-3E8wLe.tmp: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
| scratch/cargo-mutants-atlas-fouPP4.tmp: test | `cargo test --quiet` | FAIL —   The system cannot find the path specified. (os error 3) |
| scratch/cargo-mutants-atlas-fouPP4.tmp: build | `cargo build --quiet` | FAIL —   The system cannot find the path specified. (os error 3) |
| scratch/cargo-mutants-atlas-fouPP4.tmp: test (python) | `—` | NO_TESTS — Python code but no test_*.py files |
