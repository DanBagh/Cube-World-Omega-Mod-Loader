//! The in-game loader: putting it into a game folder and taking it out again.
//!
//! The loader is the `cwoml-loader` crate, built into this one. The game loads it under
//! the name of a system DLL, so a folder can already hold a file of that name that is
//! someone else's. Such a file is never replaced or removed.
//!
//! The name is one `cubeworld.exe` imports itself. See `cwoml-loader` for why.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::features::{self, Build};
use crate::game::Game;
use crate::mods::Mod;
use crate::plan::PLAN_NAME;
use crate::{Error, Result};

/// The name the game loads the loader under.
pub const DLL_NAME: &str = "dbghelp.dll";
/// The name it was installed under before. Windows does not always load it from there.
const EARLIER_NAME: &str = "version.dll";
/// Beside the DLL: a text file whose first line is the folder the loader reads its plan
/// from. `cwoml-loader` has the same name.
pub const HOME_FILE: &str = "cwoml.txt";
/// An export only the loader has. Its name in a file is how the loader is recognised.
pub const MARKER: &str = "CwomlLoaderInfo";

/// The loader as built with this launcher. None on a system it is not built for.
pub fn embedded() -> Option<&'static [u8]> {
    let dll: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/loader.dll"));
    (!dll.is_empty()).then_some(dll)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The game folder has no DLL of that name.
    Absent,
    /// The loader is there. `current` says whether it is the one given to compare with.
    Installed { current: bool },
    /// A DLL of that name is there and it is not the loader.
    Foreign,
}

fn is_loader(dll: &[u8]) -> bool {
    memchr::memmem::find(dll, MARKER.as_bytes()).is_some()
}

/// Whether `game_dir` holds the loader under the name it had before.
fn has_earlier(game_dir: &Path) -> bool {
    fs::read(game_dir.join(EARLIER_NAME)).is_ok_and(|found| is_loader(&found))
}

/// Removes the loader under the name it had before, so it is not loaded a second time.
fn remove_earlier(game_dir: &Path) -> Result<bool> {
    let earlier = game_dir.join(EARLIER_NAME);
    let ours = has_earlier(game_dir);
    if ours {
        fs::remove_file(&earlier).map_err(|e| Error::io(&earlier, e))?;
    }
    Ok(ours)
}

/// What `game_dir` has in the loader's place, compared with the build `dll`.
pub fn state(game_dir: &Path, dll: &[u8]) -> State {
    match fs::read(game_dir.join(DLL_NAME)) {
        Err(_) if has_earlier(game_dir) => State::Installed { current: false },
        Err(_) => State::Absent,
        Ok(found) if is_loader(&found) => State::Installed {
            current: found == dll,
        },
        Ok(_) => State::Foreign,
    }
}

/// Puts the loader `dll` into `game_dir` and tells it to read its plan from `home`.
pub fn install(game_dir: &Path, dll: &[u8], home: &Path) -> Result<()> {
    let target = game_dir.join(DLL_NAME);
    match state(game_dir, dll) {
        State::Foreign => {
            return Err(Error::invalid(format!(
                "{} is not the mod loader and was left alone",
                target.display()
            )));
        }
        State::Installed { current: true } => {}
        // the game holds the file open while it runs, and then this fails
        _ => fs::write(&target, dll).map_err(|e| Error::io(&target, e))?,
    }
    remove_earlier(game_dir)?;
    let pointer = game_dir.join(HOME_FILE);
    let home = home.display().to_string();
    fs::write(&pointer, home + "\n").map_err(|e| Error::io(&pointer, e))
}

/// Takes the loader out of `game_dir`. False if it was not there.
pub fn remove(game_dir: &Path) -> Result<bool> {
    let target = game_dir.join(DLL_NAME);
    let ours = fs::read(&target).is_ok_and(|found| is_loader(&found));
    if ours {
        fs::remove_file(&target).map_err(|e| Error::io(&target, e))?;
    }
    let earlier = remove_earlier(game_dir)?;
    if ours || earlier {
        // without the loader the pointer means nothing
        let _ = fs::remove_file(game_dir.join(HOME_FILE));
    }
    Ok(ours || earlier)
}

/// Plans `mods` for `game`, writes the plan and any converted models into `home`, and
/// puts the loader into the game folder to apply it the next time the game starts.
pub fn apply(game: &Game, mods: &mut [Mod], home: &Path) -> Result<Build> {
    let dll = embedded()
        .ok_or_else(|| Error::invalid("the in-game loader is not built for this system"))?;
    let home = std::path::absolute(home).map_err(|e| Error::io(home, e))?;
    let build = features::build(game, mods, &home)?;
    build.plan.write(&home.join(PLAN_NAME))?;
    install(&game.root, dll, &home)?;
    Ok(build)
}

/// Takes the loader out of the game folder and removes the plan. False if the loader was
/// not there.
pub fn clear(game: &Game, home: &Path) -> Result<bool> {
    let removed = remove(&game.root)?;
    let plan = home.join(PLAN_NAME);
    if plan.is_file() {
        fs::remove_file(&plan).map_err(|e| Error::io(&plan, e))?;
    }
    Ok(removed)
}

/// The Wollay Launcher's executable. It is the only program that can start the game
/// signed in, so applying mods never starts the game.
pub fn official_launcher() -> Option<PathBuf> {
    let exe = install_dir()?.join("wollay-launcher.exe");
    exe.is_file().then_some(exe)
}

/// Starts `exe` the way a double click does, as a program of its own.
///
/// Started from here it would belong to this program: closed with it by whatever ends a
/// program together with all it started, and run with this program's environment, which
/// can carry a Windows compatibility setting on to it. The Wollay Launcher would pass
/// both on to the game.
pub fn open_apart(exe: &Path) -> Result<()> {
    // Explorer hands the request to the desktop, which starts the program itself
    #[cfg(windows)]
    let mut command = {
        let windows = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        let mut explorer = Command::new(Path::new(&windows).join("explorer.exe"));
        explorer.arg(exe);
        explorer
    };
    #[cfg(not(windows))]
    let mut command = Command::new(exe);
    command.spawn().map(drop).map_err(|e| Error::io(exe, e))
}

#[cfg(windows)]
fn install_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};

    // its installer records the folder as the default value of this key
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain([0]).collect() };
    let key = wide(r"Software\Wollay\Wollay Launcher");
    let mut buffer = [0u16; 1024];
    let mut size = size_of_val(&buffer) as u32;
    // SAFETY: the strings are NUL-terminated and `size` is the buffer's length in bytes
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let length = buffer.iter().position(|&unit| unit == 0)?;
    let dir = std::ffi::OsString::from_wide(&buffer[..length]);
    Some(PathBuf::from(
        dir.to_string_lossy().trim_matches('"').to_owned(),
    ))
}

#[cfg(not(windows))]
fn install_dir() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(version: u8) -> Vec<u8> {
        [b"MZ...".as_slice(), MARKER.as_bytes(), &[0, version]].concat()
    }

    #[test]
    fn installs_updates_and_removes_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let (game, home) = (dir.path().join("game"), dir.path().join("data"));
        fs::create_dir_all(&game).unwrap();
        let (old, new) = (fake(1), fake(2));
        assert_eq!(state(&game, &new), State::Absent);
        assert!(!remove(&game).unwrap());

        install(&game, &old, &home).unwrap();
        assert_eq!(state(&game, &old), State::Installed { current: true });
        assert_eq!(state(&game, &new), State::Installed { current: false });
        let pointer = fs::read_to_string(game.join(HOME_FILE)).unwrap();
        assert_eq!(pointer.trim_end(), home.display().to_string());

        install(&game, &new, &home).unwrap();
        assert_eq!(fs::read(game.join(DLL_NAME)).unwrap(), new);
        assert!(remove(&game).unwrap());
        assert!(!game.join(DLL_NAME).exists() && !game.join(HOME_FILE).exists());
    }

    #[test]
    fn leaves_someone_elses_dll_alone() {
        let dir = tempfile::tempdir().unwrap();
        let theirs = b"MZ another dbghelp.dll".to_vec();
        fs::write(dir.path().join(DLL_NAME), &theirs).unwrap();
        assert_eq!(state(dir.path(), &fake(1)), State::Foreign);
        assert!(install(dir.path(), &fake(1), dir.path()).is_err());
        assert!(!remove(dir.path()).unwrap());
        assert_eq!(fs::read(dir.path().join(DLL_NAME)).unwrap(), theirs);
        assert!(!dir.path().join(HOME_FILE).exists());
    }

    #[test]
    fn replaces_the_loader_under_its_earlier_name() {
        let dir = tempfile::tempdir().unwrap();
        let (game, earlier) = (dir.path(), dir.path().join(EARLIER_NAME));
        fs::write(&earlier, fake(1)).unwrap();
        assert_eq!(state(game, &fake(2)), State::Installed { current: false });
        install(game, &fake(2), game).unwrap();
        assert!(!earlier.exists());
        assert_eq!(state(game, &fake(2)), State::Installed { current: true });

        fs::write(&earlier, fake(1)).unwrap();
        assert!(remove(game).unwrap());
        assert!(!earlier.exists() && !game.join(DLL_NAME).exists());

        // a version.dll that is someone else's is none of the loader's business
        fs::write(&earlier, b"MZ another version.dll").unwrap();
        assert_eq!(state(game, &fake(2)), State::Absent);
        install(game, &fake(2), game).unwrap();
        assert!(remove(game).unwrap());
        assert!(earlier.exists());
    }

    #[test]
    fn the_embedded_loader_is_recognised() {
        if let Some(dll) = embedded() {
            assert!(dll.starts_with(b"MZ") && is_loader(dll));
        }
    }
}
