mod audio;
mod enhance;
mod keychain;
#[cfg(target_os = "macos")]
mod media;
mod native_stt;
#[cfg(target_os = "macos")]
mod paste;
mod settings;
mod state;
mod transcribe;

use anyhow::Result;
use state::{AppState, Session};
use std::sync::Arc;
use tauri::async_runtime::JoinHandle;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

#[cfg(target_os = "macos")]
const ESCAPE_KEY_CODE: i64 = 53;

#[cfg(target_os = "macos")]
static ESCAPE_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "macos")]
static ESCAPE_APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

#[tauri::command]
fn list_input_devices() -> Vec<String> {
    audio::list_devices()
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    show_settings(&app);
}

#[tauri::command]
fn start_recording(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    begin_session(&app, state.inner().clone());
    Ok(())
}

#[tauri::command]
fn stop_recording(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    spawn_finish(&app, state.inner().clone());
    Ok(())
}

#[tauri::command]
fn cancel_recording(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    cancel_all(&app, state.inner().clone());
    Ok(())
}

fn begin_session(app: &AppHandle, state: Arc<AppState>) {
    {
        let mut slot = state.session.lock();
        if slot.is_some() {
            return;
        }
        #[cfg(target_os = "macos")]
        let focus = paste::capture_frontmost();

        let cfg = settings::Settings::load(app);
        let recording = audio::start(app.clone(), cfg.input_device.clone());

        *slot = Some(Session {
            recording,
            #[cfg(target_os = "macos")]
            focus,
            #[cfg(target_os = "macos")]
            paused_media: false,
        });
    }

    // Show overlay and emit state before the media-pause probe which can
    // take 200-700ms, so the user gets immediate visual feedback.
    show_overlay(app);
    let _ = app.emit("hearye://state", "recording");
    register_escape();

    #[cfg(target_os = "macos")]
    {
        let paused = media::pause_if_playing();
        if let Some(s) = state.session.lock().as_mut() {
            s.paused_media = paused;
        }
    }
}

fn spawn_finish(app: &AppHandle, state: Arc<AppState>) {
    if state.session.lock().is_none() {
        return;
    }
    if let Some(prev) = state.pipeline.lock().take() {
        prev.abort();
    }
    let app_clone = app.clone();
    let state_clone = state.clone();
    let handle: JoinHandle<()> = tauri::async_runtime::spawn(async move {
        if let Err(e) = finish_session(app_clone.clone(), state_clone.clone()).await {
            log::error!("finish failed: {e}");
            let _ = app_clone.emit("hearye://error", e.to_string());
            cancel_all(&app_clone, state_clone);
        }
    });
    *state.pipeline.lock() = Some(handle);
}

async fn finish_session(app: AppHandle, state: Arc<AppState>) -> Result<()> {
    let session = match state.session.lock().take() {
        Some(s) => s,
        None => return Ok(()),
    };
    #[cfg(target_os = "macos")]
    media::resume_if_paused(session.paused_media);
    let _ = app.emit("hearye://state", "transcribing");
    let wav = session.recording.into_wav_16k_mono()?;
    if wav.len() < 2_000 {
        let _ = app.emit("hearye://state", "idle");
        finish_cleanup(&app, &state);
        return Ok(());
    }

    let cfg = settings::Settings::load(&app);
    let text = match cfg.engine.as_str() {
        settings::ENGINE_GROQ => {
            transcribe::transcribe(&cfg.groq_api_key, cfg.whisper_model.as_deref(), wav).await?
        }
        _ => native_stt::transcribe_wav(app.clone(), wav).await?,
    };

    let final_text = if cfg.ai_cleanup_enabled && !text.is_empty() {
        let _ = app.emit("hearye://state", "cleaning");
        match enhance::cleanup(&cfg.anthropic_api_key, cfg.haiku_model.as_deref(), &text).await {
            Ok(t) => t,
            Err(e) => {
                log::warn!("cleanup failed, falling back to raw: {e}");
                text
            }
        }
    } else {
        text
    };

    if !final_text.is_empty() {
        #[cfg(target_os = "macos")]
        paste::paste_text(&final_text, session.focus)?;
        #[cfg(not(target_os = "macos"))]
        let _ = final_text;
    }
    let _ = app.emit("hearye://state", "idle");
    finish_cleanup(&app, &state);
    Ok(())
}

fn cancel_all(app: &AppHandle, state: Arc<AppState>) {
    let session = state.session.lock().take();
    #[cfg(target_os = "macos")]
    if let Some(s) = &session {
        media::resume_if_paused(s.paused_media);
    }
    let _ = session;
    if let Some(h) = state.pipeline.lock().take() {
        h.abort();
    }
    let _ = app.emit("hearye://state", "idle");
    finish_cleanup(app, &state);
}

fn finish_cleanup(app: &AppHandle, _state: &Arc<AppState>) {
    hide_overlay(app);
    unregister_escape();
}

fn show_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("overlay") {
        position_overlay_top_center(&w);
        #[cfg(target_os = "macos")]
        apply_overlay_window_level(&w);
        let _ = w.show();
        #[cfg(target_os = "macos")]
        order_front_regardless(&w);
    }
}

#[cfg(target_os = "macos")]
fn order_front_regardless(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let handle = window.app_handle().clone();
    let _ = handle.run_on_main_thread(move || unsafe {
        let ns_window = ns_window as *mut AnyObject;
        let _: () = msg_send![ns_window, orderFrontRegardless];
    });
}

fn position_overlay_top_center(w: &tauri::WebviewWindow) {
    // Place the pill near the top of the screen the user is currently on.
    // Prefer the monitor under the cursor so the overlay follows the active
    // display (and its active Space, including fullscreen apps) instead of
    // sticking to wherever the hidden window last sat.
    let monitor = w
        .cursor_position()
        .ok()
        .and_then(|p| w.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| w.current_monitor().ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let mon_size = monitor.size();
    let mon_pos = monitor.position();
    // Use the configured logical width so we don't depend on outer_size()
    // being settled before first show.
    let logical_w = 360.0_f64;
    let logical_y = 32.0_f64;
    let physical_w = (logical_w * scale) as i32;
    let physical_y = (logical_y * scale) as i32;
    let x = mon_pos.x + ((mon_size.width as i32) - physical_w) / 2;
    let y = mon_pos.y + physical_y;
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}

fn hide_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.hide();
    }
}

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        #[cfg(target_os = "macos")]
        {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        }
    }
}

fn parse_shortcut(spec: &str) -> Result<Shortcut> {
    spec.parse::<Shortcut>()
        .map_err(|e| anyhow::anyhow!("invalid shortcut '{spec}': {e}"))
}

#[cfg(target_os = "macos")]
fn setup_escape_tap(app: &AppHandle) {
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{CGEventTap, CGEventTapLocation, CGEventTapPlacement,
        CGEventTapOptions, CGEventType, CallbackResult, EventField};
    use std::sync::atomic::Ordering;

    let _ = ESCAPE_APP.set(app.clone());

    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::KeyDown],
        move |_proxy, _event_type, event| {
            if !ESCAPE_ACTIVE.load(Ordering::Relaxed) {
                return CallbackResult::Keep;
            }
            let key_code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            if key_code == ESCAPE_KEY_CODE {
                if let Some(app) = ESCAPE_APP.get() {
                    let app = app.clone();
                    let state = app.state::<Arc<AppState>>().inner().clone();
                    tauri::async_runtime::spawn_blocking(move || cancel_all(&app, state));
                }
                return CallbackResult::Drop;
            }
            CallbackResult::Keep
        },
    );

    let Ok(tap) = tap else {
        log::warn!("could not create CGEventTap for Escape");
        return;
    };

    let loop_source = tap.mach_port()
        .create_runloop_source(0)
        .expect("could not create run loop source from event tap");
    CFRunLoop::get_current().add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
    tap.enable();

    // Leak the tap so it lives for the process lifetime.
    std::mem::forget(tap);
    std::mem::forget(loop_source);
}

fn register_escape() {
    #[cfg(target_os = "macos")]
    ESCAPE_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn unregister_escape() {
    #[cfg(target_os = "macos")]
    ESCAPE_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
}

fn register_shortcuts(app: &AppHandle) -> Result<()> {
    let cfg = settings::Settings::load(app);
    let toggle = parse_shortcut(&cfg.toggle_hotkey)?;
    let ptt = parse_shortcut(&cfg.ptt_hotkey)?;

    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    let app_for_toggle = app.clone();
    let state_toggle = app.state::<Arc<AppState>>().inner().clone();
    gs.on_shortcut(toggle, move |_handle, _shortcut, event| {
        if matches!(event.state(), ShortcutState::Pressed) {
            let app = app_for_toggle.clone();
            let state = state_toggle.clone();
            tauri::async_runtime::spawn_blocking(move || on_toggle(&app, state));
        }
    })?;

    let app_for_ptt = app.clone();
    let state_ptt = app.state::<Arc<AppState>>().inner().clone();
    gs.on_shortcut(ptt, move |_handle, _shortcut, event: ShortcutEvent| {
        let app = app_for_ptt.clone();
        let state = state_ptt.clone();
        match event.state() {
            ShortcutState::Pressed => {
                tauri::async_runtime::spawn_blocking(move || begin_session(&app, state));
            }
            ShortcutState::Released => {
                tauri::async_runtime::spawn_blocking(move || spawn_finish(&app, state));
            }
        }
    })?;
    Ok(())
}

fn on_toggle(app: &AppHandle, state: Arc<AppState>) {
    let is_recording = state.session.lock().is_some();
    if is_recording {
        spawn_finish(app, state);
    } else {
        begin_session(app, state);
    }
}

#[tauri::command]
fn reload_hotkeys(app: AppHandle) -> Result<(), String> {
    register_shortcuts(&app).map_err(|e| e.to_string())
}

#[tauri::command]
fn suspend_hotkeys(app: AppHandle) -> Result<(), String> {
    let _ = app.global_shortcut().unregister_all();
    Ok(())
}

#[tauri::command]
fn has_api_key(name: String) -> Result<bool, String> {
    if !keychain::is_known(&name) {
        return Err(format!("unknown key: {name}"));
    }
    keychain::get(&name)
        .map(|v| v.map(|s| !s.is_empty()).unwrap_or(false))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_api_key(name: String, value: String) -> Result<(), String> {
    if !keychain::is_known(&name) {
        return Err(format!("unknown key: {name}"));
    }
    keychain::set(&name, &value).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            cancel_recording,
            reload_hotkeys,
            list_input_devices,
            open_settings,
            suspend_hotkeys,
            has_api_key,
            set_api_key
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                configure_overlay_window(app.handle());
                setup_escape_tap(app.handle());
            }
            build_tray(app.handle())?;
            // Show settings only on the very first launch (before user has saved anything).
            let cfg = settings::Settings::load(app.handle());
            if let Some(w) = app.get_webview_window("settings") {
                if !cfg.initialized {
                    let _ = w.show();
                    let _ = w.set_focus();
                    #[cfg(target_os = "macos")]
                    {
                        let _ = app.handle()
                            .set_activation_policy(tauri::ActivationPolicy::Regular);
                    }
                } else {
                    let _ = w.hide();
                }
            }
            // Hide settings instead of closing it so the app keeps running in the tray.
            if let Some(w) = app.get_webview_window("settings") {
                let w_clone = w.clone();
                w.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w_clone.hide();
                        #[cfg(target_os = "macos")]
                        {
                            let _ = w_clone
                                .app_handle()
                                .set_activation_policy(tauri::ActivationPolicy::Accessory);
                        }
                    }
                });
            }
            if let Err(e) = register_shortcuts(app.handle()) {
                log::warn!("could not register shortcuts at startup: {e}");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &AppHandle) -> Result<()> {
    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit HearYe", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings_item, &quit_item])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no default window icon"))?;

    TrayIconBuilder::with_id("hearye-tray")
        .icon(icon)
        .icon_as_template(false)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn configure_overlay_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        apply_overlay_window_level(&window);
    }
}

#[cfg(target_os = "macos")]
fn apply_overlay_window_level(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as *mut AnyObject;
    unsafe {
        // Level 1000 (kCGScreenSaverWindowLevel) sits above fullscreen apps,
        // which run at kCGNormalWindowLevel inside their own Space.
        let _: () = msg_send![ns_window, setLevel: 1000i64];
        // canJoinAllSpaces (1) | stationary (16) | fullScreenAuxiliary (256)
        // | canJoinAllApplications (1 << 18, macOS 13+) — the last bit is what
        // lets a plain NSWindow show on the fullscreen Space owned by another
        // app (Slack, Chrome) instead of being hidden behind it.
        let behavior: u64 = 1 | 16 | 256 | (1u64 << 18);
        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
        let _: () = msg_send![ns_window, setHidesOnDeactivate: false];
    }
}
