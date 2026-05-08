#!/usr/bin/env bash
# Re-sign the debug binary so its embedded Info.plist is bound to the signature
# AND the signature is stable across builds. The stable identity matters for
# the macOS Keychain: ACLs are tied to the binary's code signature, so re-signing
# every dev build with a fresh ad-hoc signature ("--sign -") makes Keychain treat
# each build as a brand-new app and re-prompt for permission.
#
# On first run, creates a self-signed "HearYe Dev" code-signing certificate in
# the login keychain and prompts for the keychain password ONCE so that
# codesign can use the key without further GUI prompts.
#
# Usage: scripts/sign-dev-binary.sh [path/to/binary]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/src-tauri/target/debug/hearye}"
ENT="$ROOT/src-tauri/entitlements.plist"
IDENTITY="${HEARYE_DEV_IDENTITY:-HearYe Dev}"

if [[ ! -f "$BIN" ]]; then
    echo "binary not found: $BIN" >&2
    exit 1
fi

if ! security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$IDENTITY\""; then
    echo "=== One-time setup: creating code-signing certificate ===" >&2
    echo "This will ask for your macOS login password once." >&2

    CERTDIR="$(mktemp -d)"
    trap 'rm -rf "$CERTDIR"' EXIT

    cat > "$CERTDIR/cert.cfg" <<CERTCFG
[ req ]
distinguished_name = dn
x509_extensions    = codesign
prompt             = no

[ dn ]
CN = $IDENTITY

[ codesign ]
keyUsage         = digitalSignature
extendedKeyUsage = codeSigning
CERTCFG

    openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
        -config "$CERTDIR/cert.cfg" \
        -keyout "$CERTDIR/key.pem" -out "$CERTDIR/cert.pem" 2>/dev/null

    # -legacy is needed on newer macOS OpenSSL; a non-empty password avoids
    # PKCS12 MAC verification failures during import.
    openssl pkcs12 -export -inkey "$CERTDIR/key.pem" -in "$CERTDIR/cert.pem" \
        -out "$CERTDIR/cert.p12" -passout pass:hearye -name "$IDENTITY" -legacy 2>/dev/null

    security import "$CERTDIR/cert.p12" -k ~/Library/Keychains/login.keychain-db \
        -T /usr/bin/codesign -P "hearye"

    # Allow codesign to access the key without GUI prompts. This requires the
    # login keychain password — prompt once via the terminal.
    echo "" >&2
    echo "Enter your macOS login password to authorize codesign (one-time):" >&2
    read -r -s -p "Password: " KC_PASS
    echo "" >&2

    security set-key-partition-list -S apple-tool:,apple:,codesign: -s \
        -k "$KC_PASS" ~/Library/Keychains/login.keychain-db

    echo "certificate created: $IDENTITY (valid for 10 years)" >&2

    if ! security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$IDENTITY\""; then
        echo "warning: certificate not found after import — falling back to ad-hoc" >&2
        codesign --force --sign - --entitlements "$ENT" "$BIN"
        echo "signed: $BIN  (ad-hoc)" >&2
        codesign -dv "$BIN" 2>&1 | grep -iE "Info.plist|Authority" || true
        exit 0
    fi
fi

codesign --force --sign "$IDENTITY" --entitlements "$ENT" "$BIN"
echo "signed: $BIN  (identity: $IDENTITY)"
codesign -dv "$BIN" 2>&1 | grep -iE "Info.plist|Authority" || true
