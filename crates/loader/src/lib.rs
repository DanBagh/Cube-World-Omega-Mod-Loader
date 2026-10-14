//! The DLL that applies a plan inside the game. The app installs it beside
//! `cubeworld.exe` as `dbghelp.dll`.
//!
//! `cubeworld.exe` imports dbghelp.dll itself, and its own folder is searched first, so a
//! DLL of that name there is loaded when the process starts. A DLL the game only reaches
//! through another one will not do: Windows loads its own copy first when the game runs
//! under a compatibility setting, and the one in the game folder is then never looked for.
//!
//! The client DLL the plan is for is loaded later, so this waits for it (see `client`) and
//! applies the plan once the client's imports are bound and before any of its code runs.
//!
//! It does as little as it can. The app works everything out ahead of time and writes
//! a plan of exact patches that each name the bytes they expect to replace. This forwards
//! the real dbghelp.dll, applies the plan to the client, and serves replacement files by
//! hooking the client's imports. Nothing in the game folder is changed.

pub mod plan;

#[cfg(windows)]
mod client;
#[cfg(windows)]
mod forward;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod patch;
#[cfg(windows)]
mod redirect;

/// Folder the plan is read from and the log written to, for running without the app.
pub const HOME_VARIABLE: &str = "CWOML_HOME";
/// Beside the DLL: a text file whose first line is that folder. The app writes it.
pub const HOME_FILE: &str = "cwoml.txt";
pub const PLAN_NAME: &str = "plan.txt";
pub const LOG_NAME: &str = "loader.log";

#[cfg(windows)]
mod entry {
    use std::ffi::{OsString, c_char, c_void};
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};

    use windows_sys::Win32::Foundation::HMODULE;
    use windows_sys::Win32::System::LibraryLoader::{
        DisableThreadLibraryCalls, GetModuleFileNameW,
    };

    use super::log::{self, log};
    use super::{
        HOME_FILE, HOME_VARIABLE, LOG_NAME, PLAN_NAME, client, forward, patch, plan, redirect,
    };

    const DLL_PROCESS_DETACH: u32 = 0;
    const DLL_PROCESS_ATTACH: u32 = 1;
    /// More than any real plan. A file this size is something else.
    const MAX_PLAN: u64 = 64 << 20;

    /// What to apply once the client is loaded.
    static PLAN: OnceLock<plan::Plan> = OnceLock::new();
    /// The data area trampolines record into: where it is and how long.
    static DATA: Mutex<Option<(usize, u32)>> = Mutex::new(None);
    static DUMP: AtomicBool = AtomicBool::new(false);

    fn home(instance: HMODULE) -> Option<PathBuf> {
        if let Some(home) = std::env::var_os(HOME_VARIABLE).filter(|home| !home.is_empty()) {
            return Some(home.into());
        }
        let mut path = [0u16; 1024];
        // SAFETY: the buffer is as long as the length passed
        let length = unsafe { GetModuleFileNameW(instance, path.as_mut_ptr(), path.len() as u32) };
        if length == 0 || length as usize >= path.len() {
            return None;
        }
        let dll = PathBuf::from(OsString::from_wide(&path[..length as usize]));
        let pointer = std::fs::read_to_string(dll.with_file_name(HOME_FILE)).ok()?;
        let line = pointer
            .lines()
            .next()?
            .trim_start_matches('\u{feff}')
            .trim();
        (!line.is_empty()).then(|| line.into())
    }

    /// Applies the plan to the client, which has to be loaded with its imports bound.
    fn apply() {
        let Some(plan) = PLAN.get() else {
            return;
        };
        let done = patch::run(plan);
        if let Ok(mut data) = DATA.lock() {
            *data = done.data;
        }
        DUMP.store(plan.dump, Ordering::Relaxed);
        if let Some(client) = done.client.filter(|_| !plan.redirects.is_empty()) {
            redirect::install(client, &plan.redirects);
        }
        log::flush();
    }

    fn start(instance: HMODULE) {
        let Some(home) = home(instance) else {
            return;
        };
        log::open(&home.join(LOG_NAME));
        log!("cwoml loader {}", env!("CARGO_PKG_VERSION"));
        let path = home.join(PLAN_NAME);
        let read = std::fs::metadata(&path)
            .ok()
            .filter(|file| file.len() < MAX_PLAN)
            .and_then(|_| std::fs::read(&path).ok());
        let Some(text) = read else {
            log!("no plan at {}", path.display());
            return;
        };
        match plan::parse(&String::from_utf8_lossy(&text)) {
            Err(problem) => log!("{problem}"),
            Ok(plan) => {
                if let Some(release) = &plan.release {
                    log!("plan built for game release {release}");
                }
                let dlls: Vec<String> = plan.modules.iter().map(|m| m.dll.clone()).collect();
                let _ = PLAN.set(plan);
                // loaded already when a program loads the client first and this with it
                if dlls.iter().any(|dll| client::loaded(dll)) {
                    apply();
                } else if client::wait(dlls, apply) {
                    log!("waiting for the game to load its client");
                } else {
                    log!("cannot wait for the client to be loaded; nothing was patched");
                }
            }
        }
        log::flush();
    }

    /// Lets the app tell this file from someone else's dbghelp.dll before it replaces
    /// or removes it.
    #[unsafe(no_mangle)]
    pub extern "C" fn CwomlLoaderInfo() -> *const c_char {
        concat!("cwoml-loader ", env!("CARGO_PKG_VERSION"), "\0")
            .as_ptr()
            .cast()
    }

    /// Writes the rows of the data area that are not all zero to the log, 32 bytes a line,
    /// if the plan asked for it.
    #[unsafe(no_mangle)]
    pub extern "C" fn CwomlDumpProbes() {
        let data = DATA.lock().ok().and_then(|data| *data);
        let Some((start, size)) = data.filter(|_| DUMP.load(Ordering::Relaxed)) else {
            return;
        };
        log!("probe data, {size} bytes at {start:#x} (all-zero rows omitted)");
        // SAFETY: the loader allocated these pages itself and never frees them
        let bytes = unsafe { std::slice::from_raw_parts(start as *const u8, size as usize) };
        for (row, chunk) in bytes.chunks(32).enumerate() {
            if chunk.iter().any(|&byte| byte != 0) {
                let hex: String = chunk.iter().map(|byte| format!("{byte:02x}")).collect();
                log!("{:04x} {hex}", row * 32);
            }
        }
        log::flush();
    }

    #[unsafe(no_mangle)]
    #[allow(non_snake_case)]
    pub extern "system" fn DllMain(instance: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
        match reason {
            DLL_PROCESS_ATTACH => {
                // SAFETY: `instance` is this DLL, as the OS passes it
                unsafe { DisableThreadLibraryCalls(instance) };
                forward::load();
                // a panic must not unwind into the OS loader
                let _ = std::panic::catch_unwind(|| start(instance));
            }
            DLL_PROCESS_DETACH => {
                let _ = std::panic::catch_unwind(|| CwomlDumpProbes());
            }
            _ => {}
        }
        1
    }
}
