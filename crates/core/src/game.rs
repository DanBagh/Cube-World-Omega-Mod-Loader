//! Finding the game install and what it contains.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{Error, Result};

pub const GAME_EXE: &str = "cubeworld.exe";
pub const DEFAULT_CHANNEL: &str = "beta";

/// The client ships once per CPU level. `cubeworld.exe` picks one at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Backend {
    Avx2,
    Baseline,
}

impl Backend {
    pub const ALL: [Backend; 2] = [Backend::Avx2, Backend::Baseline];

    pub fn key(self) -> &'static str {
        match self {
            Backend::Avx2 => "avx2",
            Backend::Baseline => "baseline",
        }
    }

    pub fn dll_name(self) -> &'static str {
        match self {
            Backend::Avx2 => "cubeworld-avx2.dll",
            Backend::Baseline => "cubeworld-baseline.dll",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Game {
    pub root: PathBuf,
    /// Release name from `source.json`, or "unknown".
    pub release: String,
    pub commit: String,
    /// Installed and updated by the official launcher, as opposed to a folder the user chose.
    pub managed: bool,
    pub modules: BTreeMap<Backend, PathBuf>,
}

/// Where the official launcher keeps its files, if this OS has a known place for it.
pub fn launcher_dir() -> Option<PathBuf> {
    // only set on Windows. A Wine or Proton prefix is not looked up yet
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("com.wollay.launcher"))
}

/// The release the official launcher has active on `channel`.
pub fn managed_root(channel: &str) -> Result<PathBuf> {
    let launcher = launcher_dir().ok_or_else(|| {
        Error::invalid("the official launcher's folder is not known on this system")
    })?;
    let channel_dir = launcher
        .join("games")
        .join("cube-world-omega")
        .join(channel);
    let installed = channel_dir.join("installed.json");
    let text = fs::read_to_string(&installed).map_err(|_| {
        Error::invalid(format!(
            "no managed install found at {}",
            channel_dir.display()
        ))
    })?;
    let active = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|info| {
            info.get("active")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| {
            Error::invalid(format!("{} names no active release", installed.display()))
        })?;
    Ok(channel_dir.join("releases").join(active))
}

/// Opens the install at `game_dir`, or the one the official launcher manages when None.
pub fn find(game_dir: Option<&Path>, channel: &str) -> Result<Game> {
    let root = match game_dir {
        Some(dir) => dir.to_path_buf(),
        None => managed_root(channel)?,
    };
    if !root.join(GAME_EXE).is_file() {
        return Err(Error::invalid(format!(
            "{} does not contain {GAME_EXE}",
            root.display()
        )));
    }
    let source = fs::read_to_string(root.join("source.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let field = |name: &str| {
        source
            .as_ref()
            .and_then(|info| info.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let modules: BTreeMap<Backend, PathBuf> = Backend::ALL
        .into_iter()
        .map(|backend| (backend, root.join(backend.dll_name())))
        .filter(|(_, path)| path.is_file())
        .collect();
    if modules.is_empty() {
        return Err(Error::invalid(format!(
            "{} contains no client DLL",
            root.display()
        )));
    }
    Ok(Game {
        release: field("release").unwrap_or_else(|| "unknown".into()),
        commit: field("gitCommit").unwrap_or_default(),
        managed: game_dir.is_none(),
        modules,
        root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_game_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find(Some(dir.path()), DEFAULT_CHANNEL).is_err());

        fs::write(dir.path().join(GAME_EXE), b"").unwrap();
        assert!(
            find(Some(dir.path()), DEFAULT_CHANNEL)
                .unwrap_err()
                .to_string()
                .contains("no client DLL")
        );

        fs::write(dir.path().join(Backend::Baseline.dll_name()), b"").unwrap();
        fs::write(
            dir.path().join("source.json"),
            r#"{"release": "gh-1", "gitCommit": "abc"}"#,
        )
        .unwrap();
        let game = find(Some(dir.path()), DEFAULT_CHANNEL).unwrap();
        assert_eq!(
            (game.release.as_str(), game.commit.as_str(), game.managed),
            ("gh-1", "abc", false)
        );
        assert_eq!(
            game.modules.keys().copied().collect::<Vec<_>>(),
            [Backend::Baseline]
        );
    }

    #[test]
    fn a_missing_source_file_is_an_unknown_release() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(GAME_EXE), b"").unwrap();
        fs::write(dir.path().join(Backend::Avx2.dll_name()), b"").unwrap();
        assert_eq!(
            find(Some(dir.path()), DEFAULT_CHANNEL).unwrap().release,
            "unknown"
        );
    }
}
