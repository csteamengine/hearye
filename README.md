# HearYe

*Hear ye, hear ye!* A free dictation app for macOS, fit for any scribe. Replaceth Superwhisper and Wispr Flow when all thou dost truly desire is "hotkey → speak → paste."

## The Engines of Transcription

- **Local Whisper (the default, and free as the village well)** — `whisper.cpp` runneth on-device with Metal acceleration, by way of the `whisper-rs` crate. No API key, no errand to distant servers after the first use. The `ggml-base.en.bin` model (some ~140 MB of parchment) is fetched upon thy first PTT to `~/Library/Application Support/com.charlie.hearye/models/` and there cached for all eternity.
- **Groq Whisper (for the well-coined merchant)** — `whisper-large-v3-turbo` over HTTP, with thine own API key, at roughly ~$0.04 per hour of audio. Choose this path if thou cravest the finest accuracy in the land, or wouldst not host a model upon thine own machine.

Either engine doth paste by way of `NSPasteboard` and a synthesized ⌘V into the app last in thy favor — and lo, it worketh even in Alacritty, where the system dictation IME doth fail.

## The Optional Polish of Claude

Off by default. When summoned, thy transcript is dispatched to Claude Haiku 4.5 to mend punctuation and banish filler words. Requireth an Anthropic API key.

## How To Wield It

- **Toggle hotkey** (by default `Cmd+Shift+Space`) — press once to begin, press again to cease.
- **Push-to-talk hotkey** (by default `F18`) — hold whilst speaking, release to send forth.
- **Escape**, or the `×` upon the overlay — abandoneth the deed at any stage (recording, transcribing, or cleanup).
- A menu-bar sigil offereth Settings and Quit. The app dwelleth in the menu bar; closing the settings window shall not banish it.

## Preparations & Provisions

```bash
pnpm install
./scripts/dev.sh         # NOT `pnpm tauri dev` — see below
```

Upon first launch, the settings window openeth. Choose thine engine, set thy hotkeys, and save.

macOS shall demand **Microphone** and **Accessibility** (the latter for the synthesized ⌘V). Both art required.

The local Whisper engine forgeth `whisper.cpp` from source by way of `cmake`, so ensure `cmake` is among thy tools (`brew install cmake`).

### Wherefore `./scripts/dev.sh` and not `pnpm tauri dev`?

`tauri dev` doth `cargo build → exec` with no interlude betwixt. Cargo's linker applieth an adhoc code signature that doth **not** bind the embedded `Info.plist` to the signature (`codesign -dv` shall reveal `Info.plist=not bound`). macOS TCC demandeth the plist be bound by the signature ere it permitteth privacy-gated calls such as `SFSpeechRecognizer.requestAuthorization` — without it, TCC slayeth the process. `scripts/dev.sh` insertheth a `codesign --force --sign -` step betwixt build and exec to set this aright.

For a fully bundled `.app` (production), `pnpm tauri build` worketh properly, for Tauri's bundler signeth the bundle in due fashion.

After any Rust source change, Ctrl-C and run anew `./scripts/dev.sh`. The Vite dev server tarrieth in the background.

### A One-Time Rite: a stable dev code-signing identity

The macOS Keychain doth ACL its items by **code signature**, not by bundle ID. Without a stable signing identity, every dev rebuild begetteth a fresh ad-hoc signature, so the Keychain treateth each build as a stranger and prompteth thee anew, even after thou hast clicked "Always Allow." The remedy:

1. Open **Keychain Access** → **Certificate Assistant** → **Create a Certificate…**
2. Name: `HearYe Dev`  •  Identity Type: **Self Signed Root**  •  Certificate Type: **Code Signing**
3. Click Create. (No Apple Developer account is required; this cert is of the realm local-only.)

`scripts/sign-dev-binary.sh` shall auto-detect the cert by its name and put it to use. Override with `HEARYE_DEV_IDENTITY=...` shouldst thou desire another name. The first build following the cert's creation shall yet prompt thee once — click "Always Allow," and the deed is done.

## Permissions / Info.plist

`src-tauri/Info.plist` doth declare the usage strings macOS requireth for its permission prompts:

- `NSMicrophoneUsageDescription`
- `NSSpeechRecognitionUsageDescription`
- `LSUIElement` (that the dock icon stayeth hidden — for this is a menu-bar app)

Tauri v2 doth not yet merge custom Info.plist keys via `tauri.conf.json` for dev binaries. For a packaged build, after `pnpm tauri build`, the bundle's `Info.plist` should be merged with these keys ere signing. For dev, grant permissions by hand via System Settings → Privacy & Security if no prompt appeareth.

## A Word on the Safekeeping of API Keys

Keys are kept within the **macOS Keychain** under the service name `com.charlie.hearye` — never writ to disk in plaintext. The default native engine needeth no key, and thou mayst avoid this matter entirely.

`com.charlie.hearye` is the app's bundle identifier (a reverse-DNS namespace for the app itself), not specific to any user — 'tis the same string upon every install. Keychain entries dwell within the current user's **login keychain**, so each macOS account hath its own isolated copy of the key. The cached Whisper model beneath `~/Library/Application Support/com.charlie.hearye/models/` is likewise per-user, for `~` resolveth to each user's own home.

## The Stack of Many Tools

- Tauri v2, Rust, SvelteKit (TS)
- `cpal` (audio) → `hound` (WAV) → engine (local `whisper.cpp` via `whisper-rs` with Metal, or Groq Whisper) → optional Claude Haiku → `core-graphics` ⌘V
- `tauri-plugin-global-shortcut` (hotkeys), `tauri-plugin-store` (settings)
