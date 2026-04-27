// Pause/resume the system "now playing" media around a recording session.
//
// Decision flow:
//   1. Probe MediaRemote's IsPlaying via a synchronous block call. This
//      catches Music.app, browser tabs that register with NowPlaying, etc.
//   2. Also ask Spotify directly via AppleScript — Spotify doesn't always
//      register with NowPlaying, so MediaRemote misses it. AppleScript
//      gives an accurate `player state` regardless.
//   3. Pause if either signal says something is playing; remember whether
//      we paused so we don't spuriously resume on session end.
//
// All MediaRemote work is via dlsym; if the framework is unavailable the
// calls become no-ops.
#![cfg(target_os = "macos")]

use std::ffi::{c_void, CString};
use std::os::raw::{c_char, c_int};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

const FRAMEWORK_PATH: &str =
    "/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote";
const RTLD_LAZY: c_int = 1;

const MR_CMD_PLAY: c_int = 0;
const MR_CMD_PAUSE: c_int = 1;

const DISPATCH_QUEUE_PRIORITY_DEFAULT: isize = 0;
const BLOCK_IS_GLOBAL: c_int = 1 << 28;

extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
    static _NSConcreteGlobalBlock: c_void;
}

type SendCommandFn = unsafe extern "C" fn(c_int, *mut c_void) -> bool;
type GetIsPlayingFn = unsafe extern "C" fn(*mut c_void, *mut c_void);

#[repr(C)]
struct BlockDescriptor {
    reserved: usize,
    size: usize,
}

#[repr(C)]
struct GlobalBlock {
    isa: *const c_void,
    flags: c_int,
    reserved: c_int,
    // ObjC BOOL: signed char on x86_64, _Bool on arm64. Take it as i8 and
    // treat any non-zero byte as true — ABI-safe on both archs.
    invoke: unsafe extern "C" fn(*mut c_void, i8),
    descriptor: *const BlockDescriptor,
}

unsafe impl Sync for GlobalBlock {}
unsafe impl Send for GlobalBlock {}

static BLOCK_DESCRIPTOR: BlockDescriptor = BlockDescriptor {
    reserved: 0,
    size: std::mem::size_of::<GlobalBlock>(),
};

static IS_PLAYING_RESULT: OnceLock<(Mutex<Option<bool>>, Condvar)> = OnceLock::new();

fn result_pair() -> &'static (Mutex<Option<bool>>, Condvar) {
    IS_PLAYING_RESULT.get_or_init(|| (Mutex::new(None), Condvar::new()))
}

unsafe extern "C" fn is_playing_cb(_block: *mut c_void, playing: i8) {
    let (m, cv) = result_pair();
    if let Ok(mut g) = m.lock() {
        *g = Some(playing != 0);
    }
    cv.notify_all();
}

fn is_playing_block() -> &'static GlobalBlock {
    static BLOCK: OnceLock<GlobalBlock> = OnceLock::new();
    BLOCK.get_or_init(|| GlobalBlock {
        isa: unsafe { &_NSConcreteGlobalBlock as *const c_void },
        flags: BLOCK_IS_GLOBAL,
        reserved: 0,
        invoke: is_playing_cb,
        descriptor: &BLOCK_DESCRIPTOR as *const _,
    })
}

fn framework_handle() -> Option<*mut c_void> {
    static HANDLE: OnceLock<Option<usize>> = OnceLock::new();
    let raw = *HANDLE.get_or_init(|| unsafe {
        let path = CString::new(FRAMEWORK_PATH).ok()?;
        let h = dlopen(path.as_ptr(), RTLD_LAZY);
        if h.is_null() {
            None
        } else {
            Some(h as usize)
        }
    });
    raw.map(|p| p as *mut c_void)
}

fn lookup<T: Copy>(name: &str) -> Option<T> {
    let handle = framework_handle()?;
    let sym = CString::new(name).ok()?;
    let p = unsafe { dlsym(handle, sym.as_ptr()) };
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
    }
}

fn send_command(cmd: c_int) {
    if let Some(func) = lookup::<SendCommandFn>("MRMediaRemoteSendCommand") {
        unsafe {
            let _ = func(cmd, std::ptr::null_mut());
        }
    }
}

/// Returns Some(true|false) if MediaRemote answered, None on timeout/unavailable.
fn media_remote_is_playing() -> Option<bool> {
    let func =
        lookup::<GetIsPlayingFn>("MRMediaRemoteGetNowPlayingApplicationIsPlaying")?;

    let (m, cv) = result_pair();
    if let Ok(mut g) = m.lock() {
        *g = None;
    }

    unsafe {
        let queue = dispatch_get_global_queue(DISPATCH_QUEUE_PRIORITY_DEFAULT, 0);
        let block_ptr = is_playing_block() as *const GlobalBlock as *mut c_void;
        func(queue, block_ptr);
    }

    let guard = m.lock().ok()?;
    let (guard, _timeout) = cv
        .wait_timeout_while(guard, Duration::from_millis(400), |v| v.is_none())
        .unwrap_or_else(|e| e.into_inner());
    *guard
}

// --- AppleScript probes ------------------------------------------------------

/// Returns "playing"/"paused"/"stopped"/None. None means osascript failed
/// (app not installed, permission denied, timeout, etc.).
fn applescript_player_state(app: &str) -> Option<String> {
    let script = format!(
        r#"if application "{app}" is running then
    tell application "{app}" to return (player state as string)
else
    return "stopped"
end if"#
    );
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn spotify_is_playing() -> Option<bool> {
    applescript_player_state("Spotify").map(|s| s == "playing")
}

fn music_is_playing() -> Option<bool> {
    applescript_player_state("Music").map(|s| s == "playing")
}

// --- Public API --------------------------------------------------------------

/// Pause now-playing media if anything is currently playing. Returns true if
/// a pause command was sent — pass that value to `resume_if_paused` to avoid
/// spuriously starting media at session end.
pub fn pause_if_playing() -> bool {
    let should_pause = media_remote_is_playing().unwrap_or(false)
        || spotify_is_playing().unwrap_or(false)
        || music_is_playing().unwrap_or(false);
    if !should_pause {
        return false;
    }
    send_command(MR_CMD_PAUSE);
    true
}

pub fn resume_if_paused(was_playing: bool) {
    if was_playing {
        send_command(MR_CMD_PLAY);
    }
}
