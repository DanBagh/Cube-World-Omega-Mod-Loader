fn main() {
    // tauri-build does not watch the icons
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
