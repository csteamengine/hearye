<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { load, type Store } from "@tauri-apps/plugin-store";
  import { check, type Update } from "@tauri-apps/plugin-updater";

  const STORE_FILE = "settings.json";

  let store: Store | null = null;
  let ready = $state(false);
  let engine = $state<"native" | "groq">("native");
  let groqKey = $state("");
  let anthropicKey = $state("");
  let groqStored = $state(false);
  let anthropicStored = $state(false);
  let aiCleanup = $state(false);
  let toggleHotkey = $state("Cmd+Shift+Space");
  let pttHotkey = $state("F18");
  let whisperModel = $state("whisper-large-v3-turbo");
  let haikuModel = $state("claude-haiku-4-5-20251001");
  let inputDevice = $state(""); // "" means system default
  let devices = $state<string[]>([]);
  let hotkeyError = $state("");
  let appVersion = $state("");
  let updateStatus = $state("");
  let checkingUpdate = $state(false);
  let availableUpdate = $state<Update | null>(null);
  let installing = $state(false);
  let recording = $state<null | "toggle" | "ptt">(null);
  let overlaySize = $state("medium");
  let overlayPosition = $state("top");

  let unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    store = await load(STORE_FILE, { defaults: {}, autoSave: false });
    engine = ((await store.get<string>("engine")) as "native" | "groq" | undefined) ?? "native";
    groqStored = await invoke<boolean>("has_api_key", { name: "groq_api_key" });
    anthropicStored = await invoke<boolean>("has_api_key", { name: "anthropic_api_key" });
    aiCleanup = (await store.get<boolean>("ai_cleanup_enabled")) ?? false;
    toggleHotkey = (await store.get<string>("toggle_hotkey")) ?? toggleHotkey;
    pttHotkey = (await store.get<string>("ptt_hotkey")) ?? pttHotkey;
    whisperModel = (await store.get<string>("whisper_model")) ?? whisperModel;
    haikuModel = (await store.get<string>("haiku_model")) ?? haikuModel;
    inputDevice = (await store.get<string>("input_device")) ?? "";
    overlaySize = (await store.get<string>("overlay_size")) ?? "medium";
    overlayPosition = (await store.get<string>("overlay_position")) ?? "top";
    devices = await invoke<string[]>("list_input_devices");
    appVersion = await getVersion();

    await tick();
    ready = true;

    unlisteners.push(await listen("hearye://close-requested", () => {
      invoke("hide_settings");
    }));

    unlisteners.push(await listen<string>("hearye://device-changed", (e) => {
      inputDevice = e.payload;
    }));

    unlisteners.push(await getCurrentWebviewWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) refreshDevices();
    }));
  });

  onDestroy(() => unlisteners.forEach(u => u()));

  $effect(() => {
    if (!ready || !store) return;
    // Touch all reactive values so this effect re-runs when any change.
    const _ = [engine, aiCleanup, toggleHotkey, pttHotkey, whisperModel,
               haikuModel, inputDevice, overlaySize, overlayPosition];
    void _;
    persistSettings();
  });

  async function persistSettings() {
    if (!store) return;
    await store.set("engine", engine);
    await store.set("initialized", true);
    await store.set("ai_cleanup_enabled", aiCleanup);
    await store.set("toggle_hotkey", toggleHotkey);
    await store.set("ptt_hotkey", pttHotkey);
    await store.set("whisper_model", whisperModel);
    await store.set("haiku_model", haikuModel);
    await store.set("input_device", inputDevice);
    await store.set("overlay_size", overlaySize);
    await store.set("overlay_position", overlayPosition);
    await store.save();
    hotkeyError = "";
    try {
      await invoke("reload_hotkeys");
    } catch (e) {
      hotkeyError = String(e);
    }
  }

  async function saveApiKey(name: "groq_api_key" | "anthropic_api_key") {
    if (name === "groq_api_key" && groqKey) {
      await invoke("set_api_key", { name, value: groqKey });
      groqStored = true;
      groqKey = "";
    } else if (name === "anthropic_api_key" && anthropicKey) {
      await invoke("set_api_key", { name, value: anthropicKey });
      anthropicStored = true;
      anthropicKey = "";
    }
  }

  async function checkForUpdates() {
    checkingUpdate = true;
    updateStatus = "";
    availableUpdate = null;
    try {
      const update = await check();
      if (update) {
        availableUpdate = update;
        updateStatus = `Version ${update.version} available (you have ${appVersion}).`;
      } else {
        updateStatus = `You're on the latest version (${appVersion}).`;
      }
    } catch (e) {
      updateStatus = `Update check failed: ${String(e)}`;
    } finally {
      checkingUpdate = false;
    }
  }

  async function installUpdate() {
    if (!availableUpdate) return;
    installing = true;
    updateStatus = "Downloading update…";
    try {
      await availableUpdate.downloadAndInstall((event) => {
        if (event.event === "Started" && event.data.contentLength) {
          updateStatus = `Downloading update (${Math.round(event.data.contentLength / 1024)} KB)…`;
        } else if (event.event === "Finished") {
          updateStatus = "Installing…";
        }
      });
      updateStatus = "Update installed. Restarting…";
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      updateStatus = `Update failed: ${String(e)}`;
      installing = false;
    }
  }

  async function refreshDevices() {
    devices = await invoke<string[]>("list_input_devices");
  }

  function eventToShortcut(e: KeyboardEvent): string | null {
    const modifierOnly = ["Meta", "Control", "Alt", "Shift"].includes(e.key);
    const code = e.code;

    let key = "";
    if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
    else if (/^Digit[0-9]$/.test(code)) key = code.slice(5);
    else if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) key = code;
    else if (code === "Space") key = "Space";
    else if (code === "Tab") key = "Tab";
    else if (code === "Enter") key = "Enter";
    else if (code === "Backspace") key = "Backspace";
    else if (code === "Delete") key = "Delete";
    else if (code === "ArrowUp") key = "Up";
    else if (code === "ArrowDown") key = "Down";
    else if (code === "ArrowLeft") key = "Left";
    else if (code === "ArrowRight") key = "Right";
    else if (code === "Minus") key = "-";
    else if (code === "Equal") key = "=";
    else if (code === "BracketLeft") key = "[";
    else if (code === "BracketRight") key = "]";
    else if (code === "Semicolon") key = ";";
    else if (code === "Quote") key = "'";
    else if (code === "Comma") key = ",";
    else if (code === "Period") key = ".";
    else if (code === "Slash") key = "/";
    else if (code === "Backslash") key = "\\";
    else if (code === "Backquote") key = "`";

    if (!key || modifierOnly) return null;

    const parts: string[] = [];
    if (e.metaKey) parts.push("Cmd");
    if (e.ctrlKey) parts.push("Ctrl");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");
    parts.push(key);
    return parts.join("+");
  }

  async function startRecord(which: "toggle" | "ptt", e: Event) {
    e.preventDefault();
    e.stopPropagation();
    if (recording === which) {
      recording = null;
      await invoke("reload_hotkeys").catch(() => {});
      return;
    }
    await invoke("suspend_hotkeys").catch(() => {});
    if (which === "toggle") toggleHotkey = "";
    else pttHotkey = "";
    recording = which;
  }

  async function endRecord() {
    recording = null;
    await invoke("reload_hotkeys").catch(() => {});
  }

  async function onRecordKeydown(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      await endRecord();
      return;
    }
    const shortcut = eventToShortcut(e);
    if (!shortcut) return;
    if (recording === "toggle") toggleHotkey = shortcut;
    else pttHotkey = shortcut;
    await endRecord();
  }

  async function clearKey(name: "groq_api_key" | "anthropic_api_key") {
    await invoke("set_api_key", { name, value: "" });
    if (name === "groq_api_key") {
      groqKey = "";
      groqStored = false;
    } else {
      anthropicKey = "";
      anthropicStored = false;
    }
  }
</script>

<main>
  <h1>HearYe</h1>
  <p class="sub">Press your hotkey, talk, release. Pastes into the focused app.</p>

  <section>
    <h2>Transcription</h2>
    <label>
      Engine
      <div class="row gap">
        <select bind:value={engine}>
          <option value="native">Local Whisper — offline, free</option>
          <option value="groq">Groq Whisper — cloud, needs API key</option>
        </select>
      </div>
    </label>
    {#if engine === "native"}
      <p class="hint">
        Runs <code>whisper.cpp</code> on-device. The ~140 MB model is downloaded on first use.
      </p>
    {/if}
    {#if engine === "groq"}
      <label>
        API key
        <div class="row gap">
          <input
            type="password"
            bind:value={groqKey}
            placeholder={groqStored ? "•••••••• (stored in Keychain)" : "gsk_..."}
            autocomplete="off"
            onblur={() => saveApiKey("groq_api_key")}
          />
          {#if groqStored}
            <button type="button" class="ghost" onclick={() => clearKey("groq_api_key")}>Clear</button>
          {/if}
        </div>
      </label>
      <label>
        Whisper model
        <input type="text" bind:value={whisperModel} />
      </label>
    {/if}
  </section>

  <section>
    <h2>Microphone</h2>
    <div class="row gap">
      <select bind:value={inputDevice}>
        <option value="">System default</option>
        {#each devices as d}
          <option value={d}>{d}</option>
        {/each}
      </select>
    </div>
  </section>

  <section>
    <h2>Hotkeys</h2>
    <label>
      Toggle (press to start, again to stop)
      <div class="hotkey-input" class:recording={recording === "toggle"}>
        <input
          type="text"
          readonly
          value={recording === "toggle" ? "Press keys… (Esc to cancel)" : toggleHotkey || "Not set"}
          class:placeholder={!toggleHotkey && recording !== "toggle"}
        />
        <button
          type="button"
          class="hotkey-action"
          aria-label={recording === "toggle" ? "Cancel recording" : "Re-record hotkey"}
          onclick={(e) => startRecord("toggle", e)}
        >
          {recording === "toggle" ? "✕" : "↻"}
        </button>
      </div>
    </label>
    <label>
      Push-to-talk (hold to record, release to send)
      <div class="hotkey-input" class:recording={recording === "ptt"}>
        <input
          type="text"
          readonly
          value={recording === "ptt" ? "Press keys… (Esc to cancel)" : pttHotkey || "Not set"}
          class:placeholder={!pttHotkey && recording !== "ptt"}
        />
        <button
          type="button"
          class="hotkey-action"
          aria-label={recording === "ptt" ? "Cancel recording" : "Re-record hotkey"}
          onclick={(e) => startRecord("ptt", e)}
        >
          {recording === "ptt" ? "✕" : "↻"}
        </button>
      </div>
    </label>
    <p class="hint">
      Click <code>↻</code> on a field, then press your key combo. Press <code>Esc</code> while the
      app is recording audio to cancel that recording.
    </p>
    {#if hotkeyError}<p class="err">{hotkeyError}</p>{/if}
  </section>

  <section>
    <h2>Overlay</h2>
    <label>
      Size
      <div class="row gap">
        <select bind:value={overlaySize}>
          <option value="small">Small — minimal indicator, no controls</option>
          <option value="medium">Medium — waveform + cancel button</option>
          <option value="large">Large — waveform, status, and hotkey hints</option>
        </select>
      </div>
    </label>
    <label>
      Position
      <div class="row gap">
        <select bind:value={overlayPosition}>
          <option value="top">Top of screen</option>
          <option value="bottom">Bottom of screen</option>
        </select>
      </div>
    </label>
  </section>

  <section>
    <h2>AI cleanup</h2>
    <label class="row">
      <input type="checkbox" bind:checked={aiCleanup} />
      Clean up transcript with Claude before pasting
    </label>
    {#if aiCleanup}
      <label>
        Anthropic API key
        <div class="row gap">
          <input
            type="password"
            bind:value={anthropicKey}
            placeholder={anthropicStored ? "•••••••• (stored in Keychain)" : "sk-ant-..."}
            autocomplete="off"
            onblur={() => saveApiKey("anthropic_api_key")}
          />
          {#if anthropicStored}
            <button type="button" class="ghost" onclick={() => clearKey("anthropic_api_key")}>Clear</button>
          {/if}
        </div>
      </label>
      <label>
        Claude model
        <input type="text" bind:value={haikuModel} />
      </label>
    {/if}
    <p class="hint">
      Keys are stored in the macOS Keychain. API keys are saved when you leave the field.
    </p>
  </section>

  <section>
    <h2>About</h2>
    <p class="hint">HearYe {appVersion}</p>
    <div class="row gap">
      <button type="button" class="ghost" onclick={checkForUpdates} disabled={checkingUpdate || installing}>
        {checkingUpdate ? "Checking…" : "Check for updates"}
      </button>
      {#if availableUpdate && !installing}
        <button type="button" onclick={installUpdate}>
          Install & restart
        </button>
      {/if}
    </div>
    {#if updateStatus}
      <p class="hint">{updateStatus}</p>
    {/if}
  </section>
</main>

<svelte:window onkeydown={onRecordKeydown} />

<style>
  :global(body) {
    background: transparent;
    color: #e8e8ec;
    font-family:
      -apple-system,
      BlinkMacSystemFont,
      "SF Pro Text",
      sans-serif;
    margin: 0;
  }
  main {
    max-width: 520px;
    margin: 0 auto;
    padding: 32px 24px 16px;
  }
  h1 {
    margin: 0 0 2px;
    font-size: 18px;
  }
  .sub {
    margin: 0 0 12px;
    color: #9aa0a6;
    font-size: 12px;
  }
  section {
    margin-bottom: 14px;
  }
  h2 {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #9aa0a6;
    margin: 0 0 4px;
  }
  label {
    display: block;
    margin-bottom: 6px;
    font-size: 12px;
  }
  label.row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .row.gap {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-top: 2px;
  }
  select {
    flex: 1;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #e8e8ec;
    padding: 5px 10px;
    border-radius: 6px;
    font-size: 12px;
    box-sizing: border-box;
    height: 30px;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'><path fill='%239aa0a6' d='M0 0l5 6 5-6z'/></svg>");
    background-repeat: no-repeat;
    background-position: right 10px center;
    padding-right: 28px;
  }
  select:focus {
    outline: none;
    border-color: rgba(99, 102, 241, 0.5);
  }
  button.ghost {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #c0c4cc;
    padding: 5px 12px;
    font-size: 11px;
  }
  button.ghost:hover {
    background: rgba(255, 255, 255, 0.12);
    border-color: rgba(255, 255, 255, 0.15);
    color: #e8e8ec;
  }
  .hotkey-input {
    position: relative;
    margin-top: 2px;
  }
  .hotkey-input input {
    margin-top: 0;
    padding-right: 36px;
    cursor: default;
  }
  .hotkey-input input.placeholder {
    color: #6b7280;
  }
  .hotkey-input.recording input {
    border-color: #f87171;
    color: #f87171;
  }
  .hotkey-action {
    position: absolute;
    top: 0;
    right: 0;
    height: 100%;
    width: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    color: #9aa0a6;
    font-size: 14px;
    padding: 0;
    cursor: pointer;
  }
  .hotkey-action:hover {
    color: #e8e8ec;
  }
  .hotkey-input.recording .hotkey-action {
    color: #f87171;
  }
  input[type="text"],
  input[type="password"] {
    display: block;
    width: 100%;
    margin-top: 2px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: #e8e8ec;
    padding: 5px 10px;
    border-radius: 6px;
    font-size: 12px;
    box-sizing: border-box;
  }
  input[type="text"]:focus,
  input[type="password"]:focus {
    outline: none;
    border-color: rgba(99, 102, 241, 0.5);
  }
  .hint {
    color: #6b7280;
    font-size: 11px;
    margin: 2px 0 0;
  }
  code {
    background: rgba(255, 255, 255, 0.08);
    padding: 1px 5px;
    border-radius: 3px;
  }
  button {
    background: rgba(79, 70, 229, 0.35);
    color: #c7c3ff;
    border: 1px solid rgba(99, 102, 241, 0.3);
    padding: 6px 14px;
    border-radius: 6px;
    font-size: 12px;
    cursor: pointer;
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
  }
  button:hover {
    background: rgba(79, 70, 229, 0.5);
    border-color: rgba(99, 102, 241, 0.45);
    color: #e0deff;
  }
  .err {
    color: #f87171;
    font-size: 12px;
  }
</style>
