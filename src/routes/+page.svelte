<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { load, type Store } from "@tauri-apps/plugin-store";
  import { check, type Update } from "@tauri-apps/plugin-updater";

  const STORE_FILE = "settings.json";

  let store: Store | null = null;
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
  let savedNotice = $state("");
  let hotkeyError = $state("");
  let appVersion = $state("");
  let updateStatus = $state("");
  let checkingUpdate = $state(false);
  let availableUpdate = $state<Update | null>(null);
  let installing = $state(false);
  let recording = $state<null | "toggle" | "ptt">(null);
  let showCloseConfirm = $state(false);

  let saved = {
    engine: "native", aiCleanup: false, toggleHotkey: "Cmd+Shift+Space",
    pttHotkey: "F18", whisperModel: "whisper-large-v3-turbo",
    haikuModel: "claude-haiku-4-5-20251001", inputDevice: "",
  };

  let dirty = $derived(
    engine !== saved.engine || aiCleanup !== saved.aiCleanup ||
    toggleHotkey !== saved.toggleHotkey || pttHotkey !== saved.pttHotkey ||
    whisperModel !== saved.whisperModel || haikuModel !== saved.haikuModel ||
    inputDevice !== saved.inputDevice || !!groqKey || !!anthropicKey
  );

  let unlisten: UnlistenFn | null = null;

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
    devices = await invoke<string[]>("list_input_devices");
    appVersion = await getVersion();
    snapshotSaved();

    unlisten = await listen("hearye://close-requested", () => {
      if (dirty) {
        showCloseConfirm = true;
      } else {
        invoke("hide_settings");
      }
    });
  });

  onDestroy(() => unlisten?.());

  async function saveAndClose() {
    await save();
    showCloseConfirm = false;
    invoke("hide_settings");
  }

  function discardAndClose() {
    engine = saved.engine as "native" | "groq";
    aiCleanup = saved.aiCleanup;
    toggleHotkey = saved.toggleHotkey;
    pttHotkey = saved.pttHotkey;
    whisperModel = saved.whisperModel;
    haikuModel = saved.haikuModel;
    inputDevice = saved.inputDevice;
    groqKey = "";
    anthropicKey = "";
    showCloseConfirm = false;
    invoke("hide_settings");
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

  function snapshotSaved() {
    saved = {
      engine, aiCleanup, toggleHotkey, pttHotkey,
      whisperModel, haikuModel, inputDevice,
    };
  }

  async function refreshDevices() {
    devices = await invoke<string[]>("list_input_devices");
  }

  function eventToShortcut(e: KeyboardEvent): string | null {
    // Bare modifier presses don't form a shortcut by themselves.
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
      // toggle off — restore the saved value
      recording = null;
      await invoke("reload_hotkeys").catch(() => {});
      return;
    }
    // Suspend the global shortcut listener so the user can re-press an existing
    // hotkey without it firing the recording action.
    await invoke("suspend_hotkeys").catch(() => {});
    if (which === "toggle") toggleHotkey = "";
    else pttHotkey = "";
    recording = which;
  }

  async function endRecord() {
    recording = null;
    // Re-register whatever is currently saved (or whatever the user just typed).
    await invoke("reload_hotkeys").catch(() => {});
  }

  async function onRecordKeydown(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      // Cancel — leave the field as-is (empty if user just clicked record).
      await endRecord();
      return;
    }
    const shortcut = eventToShortcut(e);
    if (!shortcut) return; // wait for the non-modifier key
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

  async function save() {
    if (!store) return;
    await store.set("engine", engine);
    await store.set("initialized", true);
    if (groqKey) {
      await invoke("set_api_key", { name: "groq_api_key", value: groqKey });
      groqStored = true;
      groqKey = "";
    }
    if (anthropicKey) {
      await invoke("set_api_key", { name: "anthropic_api_key", value: anthropicKey });
      anthropicStored = true;
      anthropicKey = "";
    }
    await store.set("ai_cleanup_enabled", aiCleanup);
    await store.set("toggle_hotkey", toggleHotkey);
    await store.set("ptt_hotkey", pttHotkey);
    await store.set("whisper_model", whisperModel);
    await store.set("haiku_model", haikuModel);
    await store.set("input_device", inputDevice);
    await store.save();
    hotkeyError = "";
    try {
      await invoke("reload_hotkeys");
    } catch (e) {
      hotkeyError = String(e);
    }
    snapshotSaved();
    savedNotice = "Saved.";
    setTimeout(() => (savedNotice = ""), 1500);
  }
</script>

{#if showCloseConfirm}
  <div class="close-confirm">
    <span>You have unsaved changes.</span>
    <div class="close-confirm-actions">
      <button type="button" class="ghost" onclick={discardAndClose}>Discard</button>
      <button type="button" onclick={saveAndClose}>Save & close</button>
      <button type="button" class="ghost" onclick={() => (showCloseConfirm = false)}>Cancel</button>
    </div>
  </div>
{/if}

<main>
  <h1>HearYe</h1>
  <p class="sub">Press your hotkey, talk, release. Pastes into the focused app.</p>

  <section>
    <h2>Transcription</h2>
    <label>
      Engine
      <select bind:value={engine}>
        <option value="native">Local Whisper — offline, free</option>
        <option value="groq">Groq Whisper — cloud, needs API key</option>
      </select>
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
      <button type="button" class="ghost" onclick={refreshDevices}>Refresh</button>
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
      Keys are stored in the macOS Keychain. Leave a key blank when saving to keep the existing value.
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

  <div class="footer">
    <button onclick={save}>Save</button>
    <span class="ok">{savedNotice}</span>
  </div>
</main>

<svelte:window onkeydown={onRecordKeydown} />

<style>
  :global(body) {
    background: #0e0f12;
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
    padding: 24px;
  }
  h1 {
    margin: 0 0 4px;
    font-size: 22px;
  }
  .sub {
    margin: 0 0 20px;
    color: #9aa0a6;
    font-size: 13px;
  }
  section {
    margin-bottom: 22px;
  }
  h2 {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #9aa0a6;
    margin: 0 0 8px;
  }
  label {
    display: block;
    margin-bottom: 10px;
    font-size: 13px;
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
    margin-top: 4px;
  }
  select {
    flex: 1;
    background: #1a1c20;
    border: 1px solid #2a2d33;
    color: #e8e8ec;
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 13px;
    box-sizing: border-box;
    height: 36px;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='10' height='6' viewBox='0 0 10 6'><path fill='%239aa0a6' d='M0 0l5 6 5-6z'/></svg>");
    background-repeat: no-repeat;
    background-position: right 10px center;
    padding-right: 28px;
  }
  select:focus {
    outline: none;
    border-color: #4f46e5;
  }
  button.ghost {
    background: transparent;
    border: 1px solid #2a2d33;
    color: #e8e8ec;
    padding: 8px 12px;
    font-size: 12px;
  }
  button.ghost:hover {
    background: #1a1c20;
  }
  .hotkey-input {
    position: relative;
    margin-top: 4px;
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
    margin-top: 4px;
    background: #1a1c20;
    border: 1px solid #2a2d33;
    color: #e8e8ec;
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 13px;
    box-sizing: border-box;
  }
  .hint {
    color: #6b7280;
    font-size: 12px;
    margin: 4px 0 0;
  }
  code {
    background: #1a1c20;
    padding: 1px 5px;
    border-radius: 3px;
  }
  .footer {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 8px;
  }
  button {
    background: #4f46e5;
    color: white;
    border: none;
    padding: 8px 16px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }
  button:hover {
    background: #6366f1;
  }
  .ok {
    color: #34d399;
    font-size: 12px;
  }
  .err {
    color: #f87171;
    font-size: 12px;
  }
  .close-confirm {
    position: sticky;
    top: 0;
    z-index: 100;
    background: #1c1a2e;
    border-bottom: 1px solid #4f46e5;
    padding: 12px 24px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 13px;
  }
  .close-confirm-actions {
    display: flex;
    gap: 8px;
    flex-shrink: 0;
  }
</style>
