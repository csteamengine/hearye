#!/usr/bin/env bash
# Re-sign the debug binary so its embedded Info.plist is bound to the signature.
# Tauri/cargo dev builds use linker-signed adhoc signatures that do NOT bind the
# Info.plist section, which causes macOS TCC to abort the process when calling
# SFSpeechRecognizer.requestAuthorization (and similar privacy-gated APIs).
#
# Usage: scripts/sign-dev-binary.sh [path/to/binary]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/src-tauri/target/debug/hearye}"
ENT="$ROOT/src-tauri/entitlements.plist"

if [[ ! -f "$BIN" ]]; then
    echo "binary not found: $BIN" >&2
    exit 1
fi

codesign --force --sign - --entitlements "$ENT" "$BIN"
echo "signed: $BIN"
codesign -dv "$BIN" 2>&1 | grep -i "Info.plist"
