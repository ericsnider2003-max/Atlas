#!/bin/sh
# The Windows build's own tests, run under Wine on a Linux machine.
#
# Atlas's target is Windows and its development machine is not. This builds
# the test binaries for x86_64-pc-windows-gnu and runs them with Wine, so
# the Windows-only code (the keyboard hook, DPAPI, toasts, the Win32
# monitor/memory/disk readers) is at least executed on every change — and
# the platform-neutral tests are checked for Windows path and newline bugs.
#
# Wine is not Windows. A pass here means "runs on Wine"; a failure may be
# Wine's. The list of tests below is the set that doesn't depend on a Unix
# shell, and each failure is printed for a person to judge.
#
# Needs: rustup target add x86_64-pc-windows-gnu; gcc-mingw-w64-x86-64; wine.
set -u
cd "$(dirname "$0")/../.."
export WINEDEBUG=-all
export WINEPREFIX="${WINEPREFIX:-/tmp/atlas-wine}"
T=x86_64-pc-windows-gnu
FILTER="${1:-round5}"
echo "== building the Windows test binaries ($T)"
cargo test --target $T --test all --no-run 2>&1 | tail -2
BIN=$(ls -t target/$T/debug/deps/all-*.exe | head -1)
echo "== running: $BIN $FILTER (under $(wine --version))"
wine "$BIN" "$FILTER" --test-threads=1 2>&1 | grep -vE "^(fixme|err):" | tail -40
