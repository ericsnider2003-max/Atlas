#!/usr/bin/env bash
# Fallback only: an Apple distribution certificate made without a Mac.
#
# The iPhone build normally uses Apple's cloud-managed certificate (nothing
# to make). If Apple refuses that, this makes your own, on your machine:
#
#   ./ios-certificate.sh request
#       makes a private key and a certificate request (CSR). Upload the .csr at
#       developer.apple.com → Certificates → + → "Apple Distribution", then
#       download the .cer it gives you into the same folder.
#   ./ios-certificate.sh finish distribution.cer
#       joins the key and the certificate into atlas-ios-dist.p12 (with a
#       password made here, kept in a file beside it, never printed), and
#       writes atlas-ios-dist.p12.b64 to paste into the DIST_CERT_P12 secret.
#
# Keys live in ATLAS_SIGNING (default ~/Atlas/signing), never in the repo.
set -euo pipefail
dir="${ATLAS_SIGNING:-$HOME/Atlas/signing}"
mkdir -p "$dir"; umask 077
case "${1:-}" in
  request)
    [ -e "$dir/atlas-ios-dist.key" ] && { echo "A key is already there ($dir/atlas-ios-dist.key); not replacing it." >&2; exit 1; }
    openssl req -new -newkey rsa:2048 -nodes -keyout "$dir/atlas-ios-dist.key" \
      -out "$dir/atlas-ios-dist.csr" -subj "/CN=Atlas iOS distribution/C=US" 2>/dev/null
    echo "Upload this at developer.apple.com (Apple Distribution): $dir/atlas-ios-dist.csr"
    ;;
  finish)
    cer="${2:?give the downloaded .cer}"
    openssl x509 -inform der -in "$cer" -out "$dir/atlas-ios-dist.pem"
    head -c 24 /dev/urandom | base64 | tr -d '/+=\n' > "$dir/atlas-ios-dist.pass"
    # -legacy: the Mac keychain can't read OpenSSL 3's default encryption.
    openssl pkcs12 -export -legacy -inkey "$dir/atlas-ios-dist.key" -in "$dir/atlas-ios-dist.pem" \
      -out "$dir/atlas-ios-dist.p12" -passout "file:$dir/atlas-ios-dist.pass"
    base64 -w0 "$dir/atlas-ios-dist.p12" > "$dir/atlas-ios-dist.p12.b64"
    echo "Made $dir/atlas-ios-dist.p12. Secrets: DIST_CERT_P12 = contents of atlas-ios-dist.p12.b64,"
    echo "DIST_CERT_PASSWORD = contents of atlas-ios-dist.pass."
    ;;
  *) sed -n '2,19p' "$0"; exit 2;;
esac
