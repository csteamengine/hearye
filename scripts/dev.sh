#!/usr/bin/env bash
# Dev workflow that ensures the binary is properly codesigned (with the
# embedded Info.plist bound to the signature) before macOS exec's it.
#
# Tauri's normal `tauri dev` does:  cargo build → exec immediately.
# The cargo linker auto-applies an adhoc signature that does NOT bind
# the Info.plist, which makes TCC abort on requestAuthorization calls.
#
# This script does:  cargo build → re-sign with plist bound → exec.
# Vite is started in the background; the Rust binary runs in the
# foreground so you can Ctrl-C cleanly.
#
# Re-run this script after any Rust source change.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/src-tauri/target/debug/hearye"
SIGN="$ROOT/scripts/sign-dev-binary.sh"

cd "$ROOT"

# 1. Vite dev server (background, reused across re-runs if already up).
if ! curl -s http://localhost:1420 >/dev/null 2>&1; then
    echo "[dev.sh] starting vite dev server"
    pnpm dev >/tmp/hearye-vite.log 2>&1 &
    VITE_PID=$!
    trap "kill $VITE_PID 2>/dev/null || true" EXIT INT TERM
    for _ in {1..30}; do
        if curl -s http://localhost:1420 >/dev/null 2>&1; then
            echo "[dev.sh] vite up"
            break
        fi
        sleep 0.5
    done
else
    echo "[dev.sh] vite already running on :1420"
fi

# 2. Build Rust.
echo "[dev.sh] cargo build"
cargo build --manifest-path src-tauri/Cargo.toml

# 3. Re-sign with embedded Info.plist bound to the signature.
echo "[dev.sh] re-signing"
"$SIGN" "$BIN"

# 4. Run the binary directly — dev URL is configured in tauri.conf.json.
echo "[dev.sh] launching $BIN"
exec "$BIN"
