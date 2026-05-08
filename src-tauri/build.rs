fn main() {
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rerun-if-changed=Info.plist");
    }
    tauri_build::build()
}
