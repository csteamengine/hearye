use crate::keychain;
use serde::{Deserialize, Serialize};

pub const STORE_FILE: &str = "settings.json";

// Legacy keys — kept only so we can migrate existing plaintext values out of the
// store and into the macOS Keychain on first launch after upgrade.
const LEGACY_KEY_GROQ: &str = "groq_api_key";
const LEGACY_KEY_ANTHROPIC: &str = "anthropic_api_key";
pub const KEY_AI_CLEANUP: &str = "ai_cleanup_enabled";
pub const KEY_TOGGLE_HOTKEY: &str = "toggle_hotkey";
pub const KEY_PTT_HOTKEY: &str = "ptt_hotkey";
pub const KEY_WHISPER_MODEL: &str = "whisper_model";
pub const KEY_NATIVE_WHISPER_MODEL: &str = "native_whisper_model";
pub const KEY_HAIKU_MODEL: &str = "haiku_model";
pub const KEY_INPUT_DEVICE: &str = "input_device";
pub const KEY_ENGINE: &str = "engine";
pub const KEY_INITIALIZED: &str = "initialized";
pub const KEY_OVERLAY_SIZE: &str = "overlay_size";
pub const KEY_OVERLAY_POSITION: &str = "overlay_position";

pub const DEFAULT_TOGGLE_HOTKEY: &str = "Cmd+Shift+Space";
pub const DEFAULT_PTT_HOTKEY: &str = "F18";
pub const ENGINE_NATIVE: &str = "native";
pub const ENGINE_GROQ: &str = "groq";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub engine: String,
    pub groq_api_key: String,
    pub anthropic_api_key: String,
    pub ai_cleanup_enabled: bool,
    pub toggle_hotkey: String,
    pub ptt_hotkey: String,
    pub whisper_model: Option<String>,
    pub native_whisper_model: Option<String>,
    pub haiku_model: Option<String>,
    pub input_device: Option<String>,
    pub initialized: bool,
    pub overlay_size: String,
    pub overlay_position: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            engine: ENGINE_NATIVE.into(),
            groq_api_key: String::new(),
            anthropic_api_key: String::new(),
            ai_cleanup_enabled: false,
            toggle_hotkey: DEFAULT_TOGGLE_HOTKEY.into(),
            ptt_hotkey: DEFAULT_PTT_HOTKEY.into(),
            whisper_model: None,
            native_whisper_model: None,
            haiku_model: None,
            input_device: None,
            initialized: false,
            overlay_size: "medium".into(),
            overlay_position: "top".into(),
        }
    }
}

impl Settings {
    pub fn load(app: &tauri::AppHandle) -> Self {
        use tauri_plugin_store::StoreExt;
        let Ok(store) = app.store(STORE_FILE) else {
            return Self::default();
        };
        let mut s = Self::default();
        if let Some(v) = store.get(KEY_ENGINE).and_then(|v| v.as_str().map(str::to_owned)) {
            s.engine = v;
        }

        migrate_legacy_key(&store, LEGACY_KEY_GROQ, keychain::ACCOUNT_GROQ);
        migrate_legacy_key(&store, LEGACY_KEY_ANTHROPIC, keychain::ACCOUNT_ANTHROPIC);

        s.groq_api_key = keychain::get(keychain::ACCOUNT_GROQ)
            .unwrap_or_default()
            .unwrap_or_default();
        s.anthropic_api_key = keychain::get(keychain::ACCOUNT_ANTHROPIC)
            .unwrap_or_default()
            .unwrap_or_default();
        if let Some(v) = store.get(KEY_AI_CLEANUP).and_then(|v| v.as_bool()) {
            s.ai_cleanup_enabled = v;
        }
        if let Some(v) = store
            .get(KEY_TOGGLE_HOTKEY)
            .and_then(|v| v.as_str().map(str::to_owned))
        {
            s.toggle_hotkey = v;
        }
        if let Some(v) = store
            .get(KEY_PTT_HOTKEY)
            .and_then(|v| v.as_str().map(str::to_owned))
        {
            s.ptt_hotkey = v;
        }
        s.whisper_model = store
            .get(KEY_WHISPER_MODEL)
            .and_then(|v| v.as_str().map(str::to_owned));
        s.native_whisper_model = store
            .get(KEY_NATIVE_WHISPER_MODEL)
            .and_then(|v| v.as_str().map(str::to_owned));
        s.haiku_model = store
            .get(KEY_HAIKU_MODEL)
            .and_then(|v| v.as_str().map(str::to_owned));
        s.input_device = store
            .get(KEY_INPUT_DEVICE)
            .and_then(|v| v.as_str().map(str::to_owned))
            .filter(|s| !s.is_empty());
        s.initialized = store
            .get(KEY_INITIALIZED)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if let Some(v) = store.get(KEY_OVERLAY_SIZE).and_then(|v| v.as_str().map(str::to_owned)) {
            s.overlay_size = v;
        }
        if let Some(v) = store.get(KEY_OVERLAY_POSITION).and_then(|v| v.as_str().map(str::to_owned)) {
            s.overlay_position = v;
        }
        s
    }
}

fn migrate_legacy_key<R: tauri::Runtime>(
    store: &tauri_plugin_store::Store<R>,
    store_key: &str,
    account: &str,
) {
    let Some(value) = store.get(store_key).and_then(|v| v.as_str().map(str::to_owned)) else {
        return;
    };
    if !value.is_empty() {
        if let Err(e) = keychain::set(account, &value) {
            log::warn!("could not migrate {store_key} to keychain: {e}");
            return;
        }
    }
    store.delete(store_key);
    let _ = store.save();
}
