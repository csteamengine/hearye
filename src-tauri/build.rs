fn main() {
    #[cfg(target_os = "macos")]
    {
        // Tauri-build merges src-tauri/Info.plist into the binary's
        // __TEXT,__info_plist section. Just need to rerun on changes.
        println!("cargo:rerun-if-changed=Info.plist");
    }
    tauri_build::build()
}
