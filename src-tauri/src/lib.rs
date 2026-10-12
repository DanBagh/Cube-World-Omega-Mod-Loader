use std::path::PathBuf;

use serde::Serialize;
use tauri::{Manager, WebviewWindowBuilder};

const DATA_DIR: &str = "cwoml-data";

#[derive(Serialize)]
struct AppInfo {
    name: String,
    version: String,
    os: &'static str,
    arch: &'static str,
}

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    let package = app.package_info();
    AppInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

// portable: data sits beside the launcher, falling back to the per-user dir
fn data_dir(app: &tauri::AppHandle) -> PathBuf {
    // a .app bundle must not be written into
    if !cfg!(target_os = "macos") {
        // an AppImage runs from a read-only mount, APPIMAGE is the file itself
        let exe = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok());
        let dir = exe
            .as_deref()
            .and_then(|path| path.parent())
            .map(|parent| parent.join(DATA_DIR));
        if let Some(dir) = dir
            && std::fs::create_dir_all(&dir).is_ok()
        {
            return dir;
        }
    }
    app.path().app_data_dir().expect("no app data directory")
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data = data_dir(app.handle());
            // the window is built here so the webview keeps its files in the data dir
            let config = &app.config().app.windows[0];
            WebviewWindowBuilder::from_config(app.handle(), config)?
                .data_directory(data.join("webview"))
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("failed to run app");
}
