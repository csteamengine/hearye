use crate::audio::Recording;
#[cfg(target_os = "macos")]
use crate::paste::FocusTarget;
use parking_lot::Mutex;

#[derive(Default)]
pub struct AppState {
    pub session: Mutex<Option<Session>>,
    pub pipeline: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

pub struct Session {
    pub recording: Recording,
    #[cfg(target_os = "macos")]
    pub focus: Option<FocusTarget>,
}
