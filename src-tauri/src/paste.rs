#![cfg(target_os = "macos")]

use anyhow::{anyhow, Result};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_foundation::NSString;

const KEY_V: CGKeyCode = 9;
const NS_PASTEBOARD_TYPE_STRING: &str = "public.utf8-plain-text";
const NS_APPLICATION_ACTIVATE_ALL_WINDOWS: u64 = 1 << 1;

#[derive(Clone)]
pub struct FocusTarget {
    pub pid: i32,
}

pub fn capture_frontmost() -> Option<FocusTarget> {
    unsafe {
        let cls = objc2::class!(NSWorkspace);
        let ws: *mut AnyObject = msg_send![cls, sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        Some(FocusTarget { pid })
    }
}

pub fn paste_text(text: &str, target: Option<FocusTarget>) -> Result<()> {
    write_pasteboard(text)?;
    if let Some(t) = target {
        restore_focus(&t)?;
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    synthesize_cmd_v()
}

fn write_pasteboard(text: &str) -> Result<()> {
    unsafe {
        let cls = objc2::class!(NSPasteboard);
        let pb: *mut AnyObject = msg_send![cls, generalPasteboard];
        if pb.is_null() {
            return Err(anyhow!("no general pasteboard"));
        }
        let _: i64 = msg_send![pb, clearContents];
        let ns_text = NSString::from_str(text);
        let ns_type = NSString::from_str(NS_PASTEBOARD_TYPE_STRING);
        let ok: bool = msg_send![pb, setString: &*ns_text, forType: &*ns_type];
        if !ok {
            return Err(anyhow!("pasteboard setString failed"));
        }
    }
    Ok(())
}

fn restore_focus(target: &FocusTarget) -> Result<()> {
    unsafe {
        let cls = objc2::class!(NSRunningApplication);
        let app: *mut AnyObject = msg_send![cls, runningApplicationWithProcessIdentifier: target.pid];
        if app.is_null() {
            return Err(anyhow!("target pid {} not running", target.pid));
        }
        let _: bool = msg_send![app, activateWithOptions: NS_APPLICATION_ACTIVATE_ALL_WINDOWS];
    }
    Ok(())
}

fn synthesize_cmd_v() -> Result<()> {
    let src = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| anyhow!("CGEventSource::new failed"))?;
    let down = CGEvent::new_keyboard_event(src.clone(), KEY_V, true)
        .map_err(|_| anyhow!("keydown create failed"))?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);
    let up = CGEvent::new_keyboard_event(src, KEY_V, false)
        .map_err(|_| anyhow!("keyup create failed"))?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.post(CGEventTapLocation::HID);
    Ok(())
}
