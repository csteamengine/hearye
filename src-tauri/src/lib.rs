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
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, IsMenuItem, CheckMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

#[cfg(target_os = "macos")]
const ESCAPE_KEY_CODE: i64 = 53;

#[cfg(target_os = "macos")]
static ESCAPE_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os = "macos")]
static ESCAPE_APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();
#[cfg(target_os = "macos")]
static ESCAPE_TAP_PORT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
        let cfg = settings::Settings::load(app);
        let (logical_w, logical_h) = overlay_dimensions(&cfg.overlay_size);
        let _ = w.set_size(tauri::LogicalSize::new(logical_w, logical_h));
        let _ = app.emit("hearye://overlay-size", cfg.overlay_size.as_str());
        position_overlay(app, &w);
        #[cfg(target_os = "macos")]
        apply_overlay_window_level(&w);
        let _ = w.show();
        #[cfg(target_os = "macos")]
        {
            force_display(&w);
            order_front_regardless(&w);
        }
    }
}

fn overlay_dimensions(size: &str) -> (f64, f64) {
    match size {
        "small" => (250.0, 110.0),
        "large" => (480.0, 200.0),
        _ => (420.0, 150.0), // medium (default)
    }
}

#[cfg(target_os = "macos")]
fn force_display(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as usize;
    let handle = window.app_handle().clone();
    let _ = handle.run_on_main_thread(move || unsafe {
        let ns_window = ns_window as *mut AnyObject;
        let _: () = msg_send![ns_window, setAlphaValue: 1.0f64];
        let _: () = msg_send![ns_window, display];
    });
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

fn position_overlay(app: &AppHandle, w: &tauri::WebviewWindow) {
    let cfg = settings::Settings::load(app);
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
    let (logical_w, logical_h) = overlay_dimensions(&cfg.overlay_size);
    let physical_w = (logical_w * scale) as i32;
    let physical_h = (logical_h * scale) as i32;
    let x = mon_pos.x + ((mon_size.width as i32) - physical_w) / 2;
    let y = match cfg.overlay_position.as_str() {
        "bottom" => mon_pos.y + (mon_size.height as i32) - physical_h - (32.0 * scale) as i32,
        _ => mon_pos.y + (32.0 * scale) as i32, // top (default)
    };
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
fn check_accessibility() -> bool {
    check_accessibility_inner(true)
}

#[cfg(target_os = "macos")]
fn check_accessibility_inner(prompt: bool) -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
    }

    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let value = if prompt { CFBoolean::true_value() } else { CFBoolean::false_value() };
    let options = CFDictionary::from_CFType_pairs(&[(key, value)]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const _) }
}

/// Open System Settings to the Accessibility pane so the user can toggle
/// the permission off and back on (fixes stale TCC after in-place update).
#[cfg(target_os = "macos")]
fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

/// Show a native alert explaining the user must toggle Accessibility off/on.
#[cfg(target_os = "macos")]
fn show_stale_accessibility_alert() {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    unsafe {
        let cls = AnyClass::get("NSAlert").unwrap();
        let alert: *mut AnyObject = msg_send![cls, new];
        let msg = NSString::from_str(
            "HearYe needs Accessibility permission re-granted after updating."
        );
        let info = NSString::from_str(
            "macOS invalidated the permission because the app binary changed.\n\n\
             In the System Settings window that opens:\n\
             1. Find \"HearYe\" in the list\n\
             2. Toggle it OFF\n\
             3. Toggle it back ON\n\
             4. Restart HearYe"
        );
        let btn = NSString::from_str("Open System Settings");
        let _: () = msg_send![alert, setMessageText: &*msg];
        let _: () = msg_send![alert, setInformativeText: &*info];
        let _: () = msg_send![alert, addButtonWithTitle: &*btn];
        let _: i64 = msg_send![alert, runModal];
    }
    open_accessibility_settings();
}

#[cfg(target_os = "macos")]
fn reenable_tap() {
    extern "C" {
        fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
    }
    let ptr = ESCAPE_TAP_PORT.load(std::sync::atomic::Ordering::Relaxed);
    if ptr != 0 {
        log::info!("re-enabling CGEventTap after OS disabled it");
        unsafe { CGEventTapEnable(ptr as *mut _, true) };
    }
}

#[cfg(target_os = "macos")]
fn setup_escape_tap(app: &AppHandle) {
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{CGEventTap, CGEventTapLocation, CGEventTapPlacement,
        CGEventTapOptions, CGEventType, CallbackResult, EventField};
    use std::sync::atomic::Ordering;

    let _ = ESCAPE_APP.set(app.clone());

    if !check_accessibility() {
        log::warn!("Accessibility permission not granted — Escape interception will not work. \
                    Please grant Accessibility access in System Settings > Privacy & Security > Accessibility.");
    }

    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![CGEventType::KeyDown, CGEventType::KeyUp],
        move |_proxy, event_type, event| {
            if matches!(
                event_type,
                CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
            ) {
                reenable_tap();
                return CallbackResult::Keep;
            }
            if !ESCAPE_ACTIVE.load(Ordering::Relaxed) {
                return CallbackResult::Keep;
            }
            let key_code = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            if key_code == ESCAPE_KEY_CODE {
                log::info!("CGEventTap: intercepted Escape (event_type={event_type:?})");
                if matches!(event_type, CGEventType::KeyDown) {
                    if let Some(app) = ESCAPE_APP.get() {
                        let app = app.clone();
                        let state = app.state::<Arc<AppState>>().inner().clone();
                        tauri::async_runtime::spawn_blocking(move || cancel_all(&app, state));
                    }
                }
                return CallbackResult::Drop;
            }
            CallbackResult::Keep
        },
    );

    let Ok(tap) = tap else {
        if check_accessibility_inner(false) {
            log::warn!("CGEventTap creation failed despite AXIsProcessTrusted=true — stale TCC entry after update");
            show_stale_accessibility_alert();
        } else {
            log::warn!("could not create CGEventTap for Escape — Accessibility permission not granted");
        }
        return;
    };

    let port = tap.mach_port();
    let loop_source = port
        .create_runloop_source(0)
        .expect("could not create run loop source from event tap");
    CFRunLoop::get_current().add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
    tap.enable();
    log::info!("CGEventTap for Escape created and enabled");

    use core_foundation::base::TCFType;
    ESCAPE_TAP_PORT.store(
        port.as_concrete_TypeRef() as usize,
        std::sync::atomic::Ordering::Relaxed,
    );
    // Leak the tap and source so they live for the process lifetime.
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
fn hide_settings(app: AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.hide();
        #[cfg(target_os = "macos")]
        {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        }
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Arc::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            start_recording,
            stop_recording,
            cancel_recording,
            reload_hotkeys,
            hide_settings,
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
                disable_app_nap();
                configure_overlay_window(app.handle());
                configure_settings_vibrancy(app.handle());
                setup_escape_tap(app.handle());
                setup_wake_listener(app.handle());
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
            if let Some(w) = app.get_webview_window("settings") {
                let app_handle = app.handle().clone();
                w.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        hide_settings(app_handle.clone());
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
    let menu = build_tray_menu(app)?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?.to_owned();

    TrayIconBuilder::with_id("hearye-tray")
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle_tray_event(app, &event))
        .on_tray_icon_event(|tray, event| {
            if matches!(event, tauri::tray::TrayIconEvent::Click { .. }) {
                rebuild_tray_menu(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn build_tray_menu(app: &AppHandle) -> Result<Menu<tauri::Wry>> {
    let cfg = settings::Settings::load(app);
    let current_device = cfg.input_device.unwrap_or_default();
    let devices = audio::list_devices();

    let mic_submenu = Submenu::with_id(app, "mic-submenu", "Microphone", true)?;
    let default_item = CheckMenuItem::with_id(
        app, "mic:", "System default", true, current_device.is_empty(), None::<&str>,
    )?;
    mic_submenu.append(&default_item)?;
    mic_submenu.append(&PredefinedMenuItem::separator(app)?)?;
    for d in &devices {
        let checked = d == &current_device;
        let item = CheckMenuItem::with_id(
            app, &format!("mic:{d}"), d, true, checked, None::<&str>,
        )?;
        mic_submenu.append(&item)?;
    }

    let settings_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit HearYe", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[
        &mic_submenu as &dyn IsMenuItem<tauri::Wry>,
        &separator,
        &settings_item,
        &PredefinedMenuItem::separator(app)?,
        &quit_item,
    ])?;
    Ok(menu)
}

fn rebuild_tray_menu(app: &AppHandle) {
    if let Ok(menu) = build_tray_menu(app) {
        if let Some(tray) = app.tray_by_id("hearye-tray") {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn handle_tray_event(app: &AppHandle, event: &MenuEvent) {
    let id = event.id.as_ref();
    match id {
        "settings" => show_settings(app),
        "quit" => app.exit(0),
        _ if id.starts_with("mic:") => {
            let device = &id[4..];
            set_input_device(app, device);
        }
        _ => {}
    }
}

fn set_input_device(app: &AppHandle, device: &str) {
    use tauri_plugin_store::StoreExt;
    if let Ok(store) = app.store(settings::STORE_FILE) {
        store.set(settings::KEY_INPUT_DEVICE, serde_json::Value::String(device.to_string()));
        let _ = store.save();
    }
    let _ = app.emit("hearye://device-changed", device);
    rebuild_tray_menu(app);
}

#[cfg(target_os = "macos")]
fn disable_app_nap() {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    unsafe {
        let cls = AnyClass::get("NSProcessInfo").unwrap();
        let info: *mut AnyObject = msg_send![cls, processInfo];
        // NSActivityUserInitiatedAllowingIdleSystemSleep = 0x00FFFFFF
        let options: u64 = 0x00FF_FFFF;
        let reason = NSString::from_str("HearYe must respond to hotkeys instantly");
        let activity: *mut AnyObject =
            msg_send![info, beginActivityWithOptions: options reason: &*reason];
        let _ = activity;
    }
}

#[cfg(target_os = "macos")]
fn configure_overlay_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        apply_overlay_window_level(&window);
        force_transparent_webview(&window);
    }
}

#[cfg(target_os = "macos")]
fn force_transparent_webview(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let Ok(ns_window) = window.ns_window() else { return };
    let ns_window = ns_window as *mut AnyObject;
    unsafe {
        let _: () = msg_send![ns_window, setOpaque: false];
        let content_view: *mut AnyObject = msg_send![ns_window, contentView];
        if !content_view.is_null() {
            let _: () = msg_send![content_view, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![content_view, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setOpaque: false];
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn configure_settings_vibrancy(app: &AppHandle) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    let Some(window) = app.get_webview_window("settings") else { return };
    let Ok(ns_window) = window.ns_window() else { return };
    let ns_window = ns_window as *mut AnyObject;
    unsafe {
        let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: true];
        let content_view: *mut AnyObject = msg_send![ns_window, contentView];
        let superview: *mut AnyObject = msg_send![content_view, superview];
        if !superview.is_null() {
            let cls = AnyClass::get("NSVisualEffectView").unwrap();
            let effect_view: *mut AnyObject = msg_send![cls, new];
            // NSVisualEffectMaterialHUDWindow = 13
            let _: () = msg_send![effect_view, setMaterial: 13i64];
            // NSVisualEffectBlendingModeBehindWindow = 0
            let _: () = msg_send![effect_view, setBlendingMode: 0i64];
            // NSVisualEffectStateActive = 1
            let _: () = msg_send![effect_view, setState: 1i64];
            // NSViewWidthSizable | NSViewHeightSizable = 2 | 16 = 18
            let _: () = msg_send![effect_view, setAutoresizingMask: 18u64];
            let _: () = msg_send![superview, addSubview: effect_view positioned: 1i64 /* below */ relativeTo: content_view];
        }
    }
}

#[cfg(target_os = "macos")]
fn setup_wake_listener(app: &AppHandle) {
    let app_clone = app.clone();
    std::thread::spawn(move || {
        loop {
            let before = std::time::Instant::now();
            std::thread::sleep(std::time::Duration::from_secs(10));
            let elapsed = before.elapsed();
            if elapsed > std::time::Duration::from_secs(15) {
                log::info!(
                    "detected system wake (slept {}s instead of 10s) — re-registering shortcuts",
                    elapsed.as_secs()
                );
                if let Err(e) = register_shortcuts(&app_clone) {
                    log::warn!("failed to re-register shortcuts after wake: {e}");
                }
                reenable_tap();
            }
        }
    });
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
