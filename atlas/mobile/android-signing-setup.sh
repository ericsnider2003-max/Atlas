#!/usr/bin/env bash
# Once, ever: make Atlas's Android signing key and give it to the GitHub
# build, so every Android build signs itself (Eric, 24 Oct 2026: "do what's
# needed to make this happen").
#
#   cd atlas/mobile && ./android-signing-setup.sh
#
# Run it yourself, in Git Bash, on this PC. It:
#   1. makes the key with sign-android.sh make-key (refuses if one exists:
#      a new key would make every installed Atlas refuse updates);
#   2. sets the two repository secrets the Android workflow reads,
#      ANDROID_SIGNING_P12_B64 and ANDROID_SIGNING_PASS, piped straight from
#      the files into `gh secret set` -- neither value is printed;
#   3. starts the Android build on this branch, which now signs.
# Then put the two files in Atlas's vault (the last lines it prints say how).
#
# Needs a JDK (for keytool) and the gh CLI, logged in.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="${ATLAS_REPO:-ericsnider2003-max/Atlas}"
branch="${ATLAS_BRANCH:-feat/phone-standalone-2026-10-23}"
dir="${ATLAS_SIGNING:-$HOME/Atlas/signing}"
key="$dir/atlas-android.p12"
pass="$dir/atlas-android.pass"

command -v keytool >/dev/null || {
  echo "No Java here. Install a JDK first, e.g.:  winget install EclipseAdoptium.Temurin.21.JDK" >&2
  echo "then open a new Git Bash window and run this again." >&2
  exit 2
}
command -v gh >/dev/null && gh auth status >/dev/null 2>&1 || {
  echo "The gh CLI isn't logged in: run 'gh auth login' first." >&2
  exit 2
}

if [ -e "$key" ]; then
  echo "A key is already at $key -- using it, not making another."
else
  "$here/sign-android.sh" make-key
fi
[ -s "$key" ] && [ -s "$pass" ] || { echo "The key or its password file is missing in $dir." >&2; exit 3; }

# Straight from the files into GitHub's encrypted secrets; nothing echoed.
base64 -w0 "$key" | gh secret set ANDROID_SIGNING_P12_B64 -R "$repo"
gh secret set ANDROID_SIGNING_PASS -R "$repo" < "$pass"
echo "Set on $repo: $(gh secret list -R "$repo" | awk '{print $1}' | grep -E '^ANDROID_SIGNING_' | paste -sd' ' -)"

gh workflow run android.yml -R "$repo" --ref "$branch"
sleep 5
echo "Android build started: $(gh run list -R "$repo" --workflow android.yml --limit 1 --json url --jq '.[0].url')"

cat <<EOF

Now keep the key safe -- lose it and installed phones can't take updates:
  Atlas -> Settings -> Accounts -> Signing files -> Protect a file:
    $key        (name it atlas-android.p12)
    $pass       (name it atlas-android.pass)
  It asks for your vault passphrase; the originals stay where they are.
EOF
