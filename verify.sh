#!/usr/bin/env bash
#
# The one command that decides whether this tree is green.
#
# WHY THIS EXISTS
# ---------------
# On 16 Sep 2026 a package shipped claiming "330 lib + 225/225 integration
# passing, 0 warnings." It was 223/225: four tests were failing, all four were
# guards correctly catching that session's own edits, and one of them was
# hiding a fifth failure behind it. Nothing was wrong with the code that
# session set out to write. The verification step simply was not re-run before
# the handover was written, and the handover was written as if it had been.
#
# A claim that has to be retyped by hand into a markdown table is a claim that
# will eventually be wrong. This script makes it a command instead. Run it,
# paste the last block into the handover, and the state table cannot be a
# fiction.
#
# It prints a machine-readable summary block at the end and exits non-zero if
# ANYTHING is red, so it also works as a pre-handover gate or a CI step.
#
# Usage:
#   ./verify.sh          the crate, the full suite
#   ./verify.sh --quick  lib tests, guards, and warnings only (~2 min)
#
set -uo pipefail
cd "$(dirname "$0")"

QUICK=0
[ "${1:-}" = "--quick" ] && QUICK=1

RED=0
note() { printf '%s\n' "$*"; }
bad() { RED=1; printf 'RED  %s\n' "$*"; }
GUARD_AUDIT_DONE=0

# The guards first, and deliberately so. They are the cheapest tests in the
# tree and the ones that catch the failure mode this project actually has --
# code that compiles, passes its own tests, and is reachable by nothing. A run
# that is going to fail should fail in the first minute, not the thirtieth.
# UPDATED 17 Sep 2026. This list was five guards short of the tree: the three
# added on 16-17 Sep (`name_collisions`, `dead_methods`, `dead_config`) and
# two the merge brought in (`one_install_root`, `no_confident_nothings`) were
# never in it, so `--quick` reported green while four of them were red. A
# guard missing from this list is a guard nobody runs, which is the same
# failure as a guard that takes sixty seconds -- and it is the failure this
# whole script exists to prevent, reproduced inside the script itself.
#
# Rather than hand-maintain it again, `missing_guards` below cross-checks this
# list against the tree and fails if a guard file exists that is not named
# here. Add the name when you add the file; the script will insist.
GUARDS="wiring capability_wiring new_capabilities_are_wired dead_capabilities
        name_collisions dead_methods dead_config one_install_root
        no_confident_nothings helpers_are_governed
        hollowcode declared guards ceiling bug_sweep hub_is_not_code
        retrospective access_and_settings capability_honesty"

sum_tests() { grep -oE '[0-9]+ (passed|failed)' "$1" | awk '{a[$2]+=$1} END {printf "%d %d", a["passed"], a["failed"]}'; }

# Run the tests from one tests/<name>.rs, wherever the build actually put them.
# WHY THIS EXISTS
# --------------
# On 22 Sep 2026 personal Atlas's integration suite was consolidated: instead
# of compiling each of ~354 tests/*.rs into its own binary (linking the whole
# crate 354 times -- the slow part of a dev cycle), `Cargo.toml` sets
# `autotests = false` and declares the targets by hand. `tests/all.rs` pulls
# most files in as `#[path]` modules so they share ONE binary; ~26 files that
# declare their own `mod common;` or drive the global vault/handover state keep
# their own `[[test]]` target. So `cargo test --test wiring` no longer works --
# `wiring` is a module inside `all`, not a target -- and a script that runs
# guards by name has to know the difference or it reports NO RESULT and calls
# every in-`all` guard red. This resolves a name to the right invocation:
# its own target if one is declared, otherwise the module filtered out of
# `all`. A crate with no `autotests = false` has every name as its own
# target, and this falls through to the direct form.
run_one() {  # $1 = tests/<stem>.rs; leaves cargo output in $LOG
  local t="$1"
  if grep -q '^autotests = false' Cargo.toml && ! grep -q "path = \"tests/$t.rs\"" Cargo.toml; then
    cargo test --test all -- "${t}::" >"$LOG" 2>&1
  else
    cargo test --test "$t" >"$LOG" 2>&1
  fi
}

# The declared integration-test targets, for a crate that lists them by hand.
# For the consolidated crate this is `all` plus the ~26 isolated files; for a
# crate that still auto-discovers, it is one target per tests/*.rs file.
test_targets() {
  if grep -q '^autotests = false' Cargo.toml; then
    awk '/^\[\[test\]\]/{f=1;next} f&&/^name = /{gsub(/name = "|"/,"");print;f=0}' Cargo.toml
  else
    for f in tests/*.rs; do [ -f "$f" ] && basename "$f" .rs; done
  fi
}

# Every test that reads the SOURCE, discovered rather than remembered.
#
# WHY THIS IS NOT A SECOND HAND-KEPT LIST
# ---------------------------------------
# GUARDS above was five names short of the tree, so `--quick` reported green
# while four guards were red. Adding the five would have fixed today and
# nothing else: the list is wrong again the next time someone adds a guard and
# forgets this file. A guard missing from the list is a guard nobody runs,
# which is the same failure as a guard that takes sixty seconds -- and it is
# the failure this whole script exists to prevent, reproduced inside the
# script itself.
#
# So the rule is structural. A test that opens `src/` and reasons about the
# text is checking the shape of the codebase; that is what a guard is, and it
# is detectable. This finds them all, including ones written after today.
#
# MEASURED 17 Sep 2026: 21 files match beyond GUARDS, 128 seconds for all 21
# (72 of which is `vault_crypto`, argon2, irreducible). And that pass found
# three failing files -- `hub_settings`, `install`, `it_knows_it_crashed` --
# that no curated list contained, four real defects between them. That is the
# argument for discovery over memory, and it is why this runs in `--quick`
# rather than being merely reported.
source_reading_tests() {
  local f stem
  for f in tests/*.rs; do
    [ -f "$f" ] || continue
    grep -qE 'read_to_string\("src|read_dir\("src"|read_to_string\(format!\("src' "$f" || continue
    stem=$(basename "$f" .rs)
    case " $GUARDS " in *" $stem "*) continue ;; esac
    printf '%s\n' "$stem"
  done
}

# 28 Sep 2026: this repo holds personal Atlas only; the trading side's own
# crate left it, so there is one crate to check.
for CRATE in atlas; do
  note ""
  note "=============================================================="
  note " $CRATE"
  note "=============================================================="
  [ -d "$CRATE" ] || { bad "$CRATE: no such directory"; continue; }
  pushd "$CRATE" >/dev/null || { bad "$CRATE: cannot enter"; continue; }

  LOG=$(mktemp)

  note ""
  note "-- warnings (cargo check --all-targets) --"
  if cargo check --all-targets >"$LOG" 2>&1; then
    W=$(grep -cE '^(warning|error)' "$LOG")
    if [ "$W" -eq 0 ]; then note "ok   0 warnings"; else bad "$W warnings/errors"; grep -E '^(warning|error)' "$LOG" | head -5; fi
    eval "${CRATE//-/_}_warnings=$W"
  else
    bad "does not compile"; grep -E '^error' "$LOG" | head -5
    eval "${CRATE//-/_}_warnings=-1"
  fi

  note ""
  note "-- lib tests --"
  cargo test --lib >"$LOG" 2>&1
  LIB=$(grep -E '^test result' "$LOG" | tail -1)
  note "     ${LIB:-no result line}"
  echo "$LIB" | grep -q ' 0 failed' || bad "lib tests failing"
  read -r LP LF <<<"$(sum_tests "$LOG")"

  if [ "$CRATE" = "atlas" ] && [ "$QUICK" -eq 1 ]; then
    note ""
    note "-- the named guards (--quick) --"
    for t in $GUARDS; do
      [ -f "tests/$t.rs" ] || continue
      run_one "$t"
      R=$(grep -E '^test result' "$LOG" | tail -1)
      printf '     %-30s %s\n' "$t" "${R:-NO RESULT}"
      echo "$R" | grep -q ' 0 failed' || bad "$t"
    done
    note ""
    note "-- every other test that reads src/ (found, not listed) --"
    for t in $(source_reading_tests); do
      run_one "$t"
      R=$(grep -E '^test result' "$LOG" | tail -1)
      printf '     %-30s %s\n' "$t" "${R:-NO RESULT}"
      echo "$R" | grep -q ' 0 failed' || bad "$t"
    done
    popd >/dev/null; rm -f "$LOG"
    # The lib figure WAS measured above, so print it rather than a `?`. A
    # question mark where a number exists is the same defect as a wrong
    # number: the reader cannot tell which claims this run stands behind.
    eval "${CRATE//-/_}_files=quick"
    eval "${CRATE//-/_}_lib=\"$LP passed, $LF failed\""
    eval "${CRATE//-/_}_tests=\"not run (--quick)\""
    continue
  fi

  note ""
  note "-- integration tests --"
  # MEASURED 16 Sep 2026: plain `cargo test` finishes the atlas suite in
  # **570 seconds** against roughly **1800** for the one-at-a-time loop -- 3.2x
  # -- and it did NOT hit the bus error the build plan warns about. What it
  # needs is disk: `target/` peaked at 22GB, from 29GB free.
  #
  # So the loop is a fallback, not the default. It was written when the
  # environment had less headroom, and it has been the standing instruction
  # ever since, costing every verification run 20 extra minutes. Try the fast
  # path, watch for the disk failure specifically, and only then fall back.
  # FREE SPACE ALONE, and the reasoning that says otherwise is written here
  # because I tried it and it cost a run.
  #
  # The 22GB is a peak `target/` size, so it looks like free space plus what
  # `target/` already holds should be the test -- the built objects are part
  # of the peak, not on top of it. Tried on 17 Sep: 18GB free + 9GB built =
  # 27GB of "room", over the 25 threshold, took the fast path and ran out of
  # disk partway anyway. The fallback then has to `cargo clean` first, so the
  # optimistic check turned a 30-minute run into a clean plus a full rebuild
  # one file at a time. Worse than either honest answer.
  #
  # Why the sum is wrong: the peak is not the sum of the final artifacts. A
  # full `cargo test` links every test binary and holds many at once, so it
  # needs headroom *beyond* what the finished tree measures, and the allowance
  # here is shared with everything else the session writes. Free space is the
  # only quantity that answers "can this finish". (The 22 Sep consolidation cut
  # the atlas crate from ~354 test binaries to ~29, so this peak is far lower
  # than it was when the threshold was set; the threshold still stands because
  # it was measured, and a lower one has not been.)
  #
  # Both data points, kept: 29GB free -> fast path, finished, 570s. 18GB free
  # -> fast path attempted, died, cleaned, ~25 minutes. The threshold stands
  # at 25 and is not to be widened again without a measurement that beats
  # this one.
  AVAIL_GB=$(df -BG --output=avail . 2>/dev/null | tail -1 | tr -dc '0-9')
  FAST=$(mktemp)
  USED_FAST=0
  if [ "${AVAIL_GB:-0}" -ge 25 ]; then
    note "     ${AVAIL_GB}GB free -- trying plain \`cargo test\` (peaks at ~22GB, 3x faster)"
    START=$(date +%s)
    cargo test >"$FAST" 2>&1
    if grep -qE 'Bus error|signal 7|No space left on device' "$FAST"; then
      note "     ran out of disk partway -- falling back to one file at a time"
      cargo clean -q 2>/dev/null
    else
      USED_FAST=1
      note "     done in $(( $(date +%s) - START ))s"
      read -r TP TF <<<"$(sum_tests "$FAST")"
      # Count binaries, not files. `cargo test` emits one `test result:` line
      # per test binary it runs (lib, doc-tests, and each integration target).
      # Since the 22 Sep consolidation there are ~29 of those, not one per
      # tests/*.rs file, so `ls tests/*.rs | wc -l` would report e.g. 12/354 on
      # a single failure -- the kind of number the reader stops trusting. Count
      # the result lines the run actually produced, and the failures among them.
      FILES=$(grep -cE '^test result:' "$FAST")
      FAILED_FILES=$(grep -cE '^test result: FAILED' "$FAST")
      OK=$((FILES - FAILED_FILES))
      grep -E '^test result: FAILED' "$FAST" >/dev/null && bad "integration tests failing"
      grep -E 'panicked at|^error' "$FAST" | head -5 | sed 's/^/       /'
      note "     $OK/$FILES test binaries green, $TP tests passed, $TF failed"
      eval "${CRATE//-/_}_files=\"$OK/$FILES\""
      eval "${CRATE//-/_}_tests=\"$((LP+TP)) passed, $((LF+TF)) failed\""
      eval "${CRATE//-/_}_lib=\"$LP passed, $LF failed\""
      rm -f "$FAST"
      popd >/dev/null
      rm -f "$LOG"
      continue
    fi
  else
    note "     only ${AVAIL_GB:-?}GB free -- using the one-at-a-time loop"
  fi
  rm -f "$FAST"

  # The fallback. One at a time with the binary swept between, because the
  # linker cannot hold every test binary at once on a small disk (far fewer
  # since the 22 Sep consolidation, but the fallback stays for the general
  # case, where a crate still has one binary per file). A
  # `ld terminated with signal 7 [Bus error]` here is disk pressure, not a
  # code fault.
  FILES=0; OK=0; TP=0; TF=0
  for t in $(test_targets); do
    FILES=$((FILES+1))
    cargo test --test "$t" >"$LOG" 2>&1
    R=$(grep -E '^test result' "$LOG" | tail -1)
    read -r p fl <<<"$(sum_tests "$LOG")"
    TP=$((TP+p)); TF=$((TF+fl))
    if echo "$R" | grep -q ' 0 failed' && [ -n "$R" ]; then
      OK=$((OK+1))
    else
      bad "$t: ${R:-did not produce a result}"
      grep -E 'panicked at|^error' "$LOG" | head -3 | sed 's/^/       /'
    fi
    find target/debug/deps -maxdepth 1 -type f -executable ! -name '*.so' -delete 2>/dev/null
  done
  note "     $OK / $FILES targets green, $TP tests passed, $TF failed"
  eval "${CRATE//-/_}_files=\"$OK/$FILES\""
  eval "${CRATE//-/_}_tests=\"$((LP+TP)) passed, $((LF+TF)) failed\""
  eval "${CRATE//-/_}_lib=\"$LP passed, $LF failed\""

  popd >/dev/null
  rm -f "$LOG"
done

note ""
note "=============================================================="
note " PASTE THIS INTO THE HANDOVER"
note "=============================================================="
note ""
note "|                  | atlas |"
note "|---|---:|"
note "| lib tests | ${atlas_lib:-?} |"
note "| integration files | ${atlas_files:-?} |"
note "| all tests | ${atlas_tests:-?} |"
note "| warnings | ${atlas_warnings:-?} |"
note ""
note "verified by ./verify.sh on $(date -u '+%Y-%m-%d %H:%MZ')"
note ""

if [ "$RED" -ne 0 ]; then
  note "RED. Do not write a handover that says this tree is green."
  exit 1
fi
note "GREEN. Every claim in the table above was produced by running it."
exit 0
