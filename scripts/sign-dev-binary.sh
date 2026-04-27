#!/usr/bin/env bash
# Re-sign the debug binary so its embedded Info.plist is bound to the signature
# AND the signature is stable across builds. The stable identity matters for
# the macOS Keychain: ACLs are tied to the binary's code signature, so re-signing
# every dev build with a fresh ad-hoc signature ("--sign -") makes Keychain treat
# each build as a brand-new app and re-prompt for permission.
#
# One-time setup (creates a free, local-only signing cert in your login keychain):
#   1. Open Keychain Access → Certificate Assistant → Create a Certificate…
#   2. Name: "HearYe Dev"  (or set HEARYE_DEV_IDENTITY to override)
#      Identity Type: Self Signed Root
#      Certificate Type: Code Signing
#   3. Create. Done — no Apple Developer account needed.
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

# If the configured identity exists in any keychain, use it. Otherwise fall back
# to ad-hoc signing and warn — Keychain prompts will keep popping up until the
# user creates the cert.
if security find-identity -v -p codesigning 2>/dev/null | grep -q "\"$IDENTITY\""; then
    codesign --force --sign "$IDENTITY" --entitlements "$ENT" "$BIN"
    echo "signed: $BIN  (identity: $IDENTITY)"
else
    codesign --force --sign - --entitlements "$ENT" "$BIN"
    echo "signed: $BIN  (ad-hoc — see scripts/sign-dev-binary.sh for stable-identity setup)" >&2
fi
codesign -dv "$BIN" 2>&1 | grep -iE "Info.plist|Authority" || true
