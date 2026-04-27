// Bridge to the private MediaRemote framework so we can pause/resume the
// system "now playing" media around a recording session. The framework is
// private, so symbols are looked up at runtime via dlsym; if MediaRemote is
// unavailable, the calls are silent no-ops.
//
// We always send Pause on begin and Play on end. MediaRemote treats a Play
// command with no recent now-playing app as a no-op, and a Pause command
// with no playing app as a no-op, so the worst case is resuming media that
// the user paused manually shortly before dictating — annoying but rare.
#![cfg(target_os = "macos")]

use std::ffi::{c_void, CString};
use std::os::raw::{c_char, c_int};
use std::sync::OnceLock;

const FRAMEWORK_PATH: &str =
    "/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote";
const RTLD_LAZY: c_int = 1;

const MR_CMD_PLAY: c_int = 0;
const MR_CMD_PAUSE: c_int = 1;

extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

type SendCommandFn = unsafe extern "C" fn(c_int, *mut c_void) -> bool;

fn send_command_fn() -> Option<SendCommandFn> {
    static FN: OnceLock<Option<usize>> = OnceLock::new();
    let raw = *FN.get_or_init(|| unsafe {
        let path = CString::new(FRAMEWORK_PATH).ok()?;
        let handle = dlopen(path.as_ptr(), RTLD_LAZY);
        if handle.is_null() {
            return None;
        }
        let sym = CString::new("MRMediaRemoteSendCommand").ok()?;
        let p = dlsym(handle, sym.as_ptr());
        if p.is_null() {
            None
        } else {
            Some(p as usize)
        }
    });
    raw.map(|p| unsafe { std::mem::transmute::<usize, SendCommandFn>(p) })
}

fn send_command(cmd: c_int) {
    if let Some(func) = send_command_fn() {
        unsafe {
            let _ = func(cmd, std::ptr::null_mut());
        }
    }
}

pub fn pause() {
    send_command(MR_CMD_PAUSE);
}

pub fn play() {
    send_command(MR_CMD_PLAY);
}
