# HearYe

Free dictation app for macOS. Replaces Superwhisper / Wispr Flow when all you really want is "hotkey → talk → paste."

## Engines

- **Local Whisper (default)** — `whisper.cpp` running on-device with Metal acceleration, via the `whisper-rs` crate. No API key, no network after first use. The `ggml-base.en.bin` model (~140 MB) is downloaded on first PTT to `~/Library/Application Support/com.charlie.hearye/models/` and cached forever. Free.
- **Groq Whisper** — `whisper-large-v3-turbo` over HTTP, your API key, ~$0.04/hr of audio. Use this if you want best-in-class accuracy or don't want a local model.

Either engine pastes via `NSPasteboard` + synthesized ⌘V into the previously-focused app — works in Alacritty where the system dictation IME doesn't.

## Optional AI cleanup

Off by default. When on, the transcript is sent to Claude Haiku 4.5 to fix punctuation/filler. Requires an Anthropic API key.

## Usage

- **Toggle hotkey** (default `Cmd+Shift+Space`) — press to start, press again to stop.
- **Push-to-talk hotkey** (default `F18`) — hold to record, release to send.
- **Escape** or the `×` on the overlay — cancel at any phase (recording, transcribing, cleanup).
- Menu-bar icon for Settings and Quit. App stays running in the menu bar; closing the settings window does not quit it.

## Setup

```bash
pnpm install
./scripts/dev.sh         # NOT `pnpm tauri dev` — see below
```

On first launch the settings window opens. Choose your engine, set hotkeys, save.

macOS will prompt for **Microphone** and **Accessibility** (for synthesized ⌘V). Both required.

The local Whisper engine builds `whisper.cpp` from source via `cmake`, so make sure `cmake` is installed (`brew install cmake`).

### Why `./scripts/dev.sh` instead of `pnpm tauri dev`?

`tauri dev` does `cargo build → exec` with no hook in between. Cargo's linker auto-applies an adhoc code signature that does **not** bind the embedded `Info.plist` to the signature (`codesign -dv` will show `Info.plist=not bound`). macOS TCC requires the plist to be bound by the signature before allowing privacy-gated calls like `SFSpeechRecognizer.requestAuthorization` — without it, TCC kills the process. `scripts/dev.sh` inserts a `codesign --force --sign -` step between build and exec to fix this.

For a fully bundled `.app` (production), `pnpm tauri build` works correctly because Tauri's bundler properly signs the bundle.

After any Rust source change, Ctrl-C and re-run `./scripts/dev.sh`. The Vite dev server stays running in the background.

### One-time: stable dev code-signing identity

The macOS Keychain ACLs items by **code signature**, not bundle ID. Without a stable signing identity, every dev rebuild produces a fresh ad-hoc signature, so Keychain treats each build as a new app and re-prompts even after you click "Always Allow." Fix:

1. Open **Keychain Access** → **Certificate Assistant** → **Create a Certificate…**
2. Name: `HearYe Dev`  •  Identity Type: **Self Signed Root**  •  Certificate Type: **Code Signing**
3. Click Create. (No Apple Developer account needed; this cert is local-only.)

`scripts/sign-dev-binary.sh` auto-detects the cert by name and uses it. Override with `HEARYE_DEV_IDENTITY=...` if you want a different name. The first build after creating the cert will still prompt once — click "Always Allow" and you're done.

## Permissions / Info.plist

`src-tauri/Info.plist` declares the usage strings macOS needs for the permission prompts:

- `NSMicrophoneUsageDescription`
- `NSSpeechRecognitionUsageDescription`
- `LSUIElement` (so the dock icon stays hidden — menu-bar app)

Tauri v2 doesn't currently merge custom Info.plist keys via `tauri.conf.json` for dev binaries. For a packaged build, after `pnpm tauri build`, the bundle's `Info.plist` should be merged with these keys before signing. For dev, grant permissions manually via System Settings → Privacy & Security if a prompt doesn't appear.

## Security note on API keys

Keys are stored in the **macOS Keychain** under the service name `com.charlie.hearye` — never written to disk in plaintext. The default native engine needs no key, so you can avoid this entirely.

## Stack

- Tauri v2, Rust, SvelteKit (TS)
- `cpal` (audio) → `hound` (WAV) → engine (local `whisper.cpp` via `whisper-rs` with Metal, or Groq Whisper) → optional Claude Haiku → `core-graphics` ⌘V
- `tauri-plugin-global-shortcut` (hotkeys), `tauri-plugin-store` (settings)
