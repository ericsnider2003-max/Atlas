#!/usr/bin/env bash
# Fetch the models the phone app carries (bundled-models.json) into
# bundled-cache/, checking each one's size and SHA-256. A file that doesn't
# match is deleted and the build stops: the app never ships a model nobody
# checked. Files already here and correct are kept, so a second run is quick.
# Used by build-android.sh and the iOS workflow before the app is built.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cache="$here/bundled-cache"
mkdir -p "$cache"

sha() { if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -c1-64; else shasum -a 256 "$1" | cut -c1-64; fi; }

node -e '
const m = require(process.argv[1]);
for (const f of m.files) console.log([f.name, f.bytes, f.sha256, f.url].join(" "));
' "$here/bundled-models.json" | while read -r name bytes want url; do
  out="$cache/$name"
  if [ -f "$out" ] && [ "$(wc -c < "$out" | tr -d ' ')" = "$bytes" ] && [ "$(sha "$out")" = "$want" ]; then
    echo "have  $name"
    continue
  fi
  echo "fetch $name ($bytes bytes)"
  curl -fL --retry 3 -o "$out.part" "$url"
  got="$(sha "$out.part")"
  if [ "$got" != "$want" ] || [ "$(wc -c < "$out.part" | tr -d ' ')" != "$bytes" ]; then
    rm -f "$out.part"
    echo "REFUSED $name: its fingerprint is $got, not $want" >&2
    exit 3
  fi
  mv "$out.part" "$out"
  echo "ok    $name"
done
