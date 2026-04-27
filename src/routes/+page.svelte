<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { load, type Store } from "@tauri-apps/plugin-store";

  const STORE_FILE = "settings.json";
  const RELEASE_REPO = "csteamengine/hearye";

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
  let updateUrl = $state("");
  let checkingUpdate = $state(false);

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
  });

  function compareSemver(a: string, b: string): number {
    const pa = a.replace(/^v/, "").split(".").map((x) => parseInt(x, 10) || 0);
    const pb = b.replace(/^v/, "").split(".").map((x) => parseInt(x, 10) || 0);
    for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
      const da = pa[i] ?? 0;
      const db = pb[i] ?? 0;
      if (da !== db) return da - db;
    }
    return 0;
  }

  async function checkForUpdates() {
    if (!RELEASE_REPO) {
      updateStatus = "Update channel not configured.";
      updateUrl = "";
      return;
    }
    checkingUpdate = true;
    updateStatus = "";
    updateUrl = "";
    try {
      const r = await fetch(`https://api.github.com/repos/${RELEASE_REPO}/releases/latest`, {
        headers: { Accept: "application/vnd.github+json" },
      });
      if (!r.ok) throw new Error(`GitHub returned ${r.status}`);
      const data = await r.json();
      const latest: string = data.tag_name ?? "";
      const url: string = data.html_url ?? "";
      if (!latest) throw new Error("no tag in release");
      if (compareSemver(latest, appVersion) > 0) {
        updateStatus = `New version ${latest} available (you have ${appVersion}).`;
        updateUrl = url;
      } else {
        updateStatus = `You're on the latest version (${appVersion}).`;
      }
    } catch (e) {
      updateStatus = `Update check failed: ${String(e)}`;
    } finally {
      checkingUpdate = false;
    }
  }

  async function refreshDevices() {
    devices = await invoke<string[]>("list_input_devices");
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
    savedNotice = "Saved.";
    setTimeout(() => (savedNotice = ""), 1500);
  }
</script>

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
      <input type="text" bind:value={toggleHotkey} />
    </label>
    <label>
      Push-to-talk (hold to record, release to send)
      <input type="text" bind:value={pttHotkey} />
    </label>
    <p class="hint">
      Tauri shortcut syntax — e.g. <code>Cmd+Shift+Space</code>, <code>F18</code>. Press
      <code>Esc</code> while recording to cancel.
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
      <button type="button" class="ghost" onclick={checkForUpdates} disabled={checkingUpdate}>
        {checkingUpdate ? "Checking…" : "Check for updates"}
      </button>
      {#if updateUrl}
        <button type="button" class="ghost" onclick={() => openUrl(updateUrl)}>
          Open release page
        </button>
      {/if}
    </div>
    {#if updateStatus}<p class="hint">{updateStatus}</p>{/if}
  </section>

  <div class="footer">
    <button onclick={save}>Save</button>
    <span class="ok">{savedNotice}</span>
  </div>
</main>

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
</style>
