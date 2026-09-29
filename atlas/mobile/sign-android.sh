#!/usr/bin/env bash
# Sign the Android app on Eric's own machine, with the key that never leaves it.
#
#   ./sign-android.sh make-key            once, ever: makes the signing key
#   ./sign-android.sh sign app.apk        each release: signs a built APK
#
# The key is Atlas's Android identity. Every update must be signed with the
# same key or Android refuses it as a different app, so: made once, kept in
# ATLAS_SIGNING (default ~/Atlas/signing), backed up with the vault, never
# uploaded. The password is made here, kept in a file beside the key that
# only you can read, and never printed.
#
# Needs Java (keytool) and Android's apksigner: either ANDROID_BUILD_TOOLS
# pointing at a build-tools folder, or apksigner.jar (one file, Apache-2.0,
# from build-tools/lib) kept in $ATLAS_SIGNING/tools. Nothing is installed.
# zipalign is used when it's there; apksigner aligns the APK itself anyway.
set -euo pipefail
dir="${ATLAS_SIGNING:-$HOME/Atlas/signing}"
key="$dir/atlas-android.p12"
pass="$dir/atlas-android.pass"
alias=atlas

case "${1:-}" in
  make-key)
    mkdir -p "$dir" && chmod 700 "$dir" 2>/dev/null || true
    [ -e "$key" ] && { echo "There is already a key at $key. Refusing to replace it: a new key would make every installed Atlas refuse updates." >&2; exit 1; }
    umask 077
    head -c 32 /dev/urandom | base64 | tr -d '/+=\n' | head -c 32 > "$pass"
    keytool -genkeypair -storetype PKCS12 -keystore "$key" -alias "$alias" \
      -keyalg EC -groupname secp256r1 -sigalg SHA256withECDSA -validity 10000 \
      -dname "CN=Atlas, O=Eric Snider, C=US" \
      -storepass:file "$pass" -keypass:file "$pass" >/dev/null
    echo "Made the Android signing key: $key (valid about 27 years)."
    echo "Its password is in $pass. Back both up with the vault; if they're lost, phones can't take updates."
    keytool -list -keystore "$key" -storepass:file "$pass" -alias "$alias" | grep -i fingerprint || true
    ;;
  sign)
    in="${2:?give the unsigned APK}"
    [ -e "$key" ] || { echo "No key yet: run '$0 make-key' first." >&2; exit 1; }
    bt="${ANDROID_BUILD_TOOLS:-}"
    if [ -n "$bt" ] && [ -x "$bt/apksigner" ]; then apksigner=("$bt/apksigner")
    elif [ -f "$dir/tools/apksigner.jar" ]; then apksigner=(java -jar "$dir/tools/apksigner.jar")
    elif command -v apksigner >/dev/null; then apksigner=(apksigner)
    else echo "No apksigner: set ANDROID_BUILD_TOOLS, or put apksigner.jar in $dir/tools." >&2; exit 1; fi
    out="${in%-unsigned.apk}"; out="${out%.apk}-signed.apk"
    src="$in"
    if [ -n "$bt" ] && [ -x "$bt/zipalign" ]; then
      trap 'rm -f "$out.aligned"' EXIT
      "$bt/zipalign" -f -p 4 "$in" "$out.aligned"; src="$out.aligned"
    fi
    # One password for the store and the key (PKCS12), read from its file.
    "${apksigner[@]}" sign --ks "$key" --ks-type PKCS12 --ks-key-alias "$alias" \
      --ks-pass "file:$pass" --out "$out" "$src"
    rm -f "$out.idsig"
    "${apksigner[@]}" verify --print-certs "$out" | grep -E "Signer #1 certificate (DN|SHA-256)"
    echo "Signed: $out"
    ;;
  *)
    sed -n '2,16p' "$0"; exit 2;;
esac
