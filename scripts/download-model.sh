#!/usr/bin/env bash
# Download ggml-base.en.bin for whisper.cpp into src-tauri/models/.
# Run this once before `pnpm tauri build` so the .app ships with the model
# bundled in Contents/Resources, making the local engine work fully offline
# from first launch — no network call ever from a packaged build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/src-tauri/models/ggml-base.en.bin"
URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"

mkdir -p "$(dirname "$DEST")"

if [[ -f "$DEST" ]]; then
    size=$(stat -f %z "$DEST")
    if (( size > 100000000 )); then
        echo "Model already present at $DEST ($size bytes)"
        exit 0
    fi
    echo "Existing model file is suspiciously small ($size bytes); redownloading."
fi

echo "Downloading $URL"
curl -L --progress-bar -o "$DEST" "$URL"
echo "Done: $DEST ($(stat -f %z "$DEST") bytes)"
