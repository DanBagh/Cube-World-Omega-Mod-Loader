//! Finding the game install and what it contains.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{Error, Result, wine};

pub const GAME_EXE: &str = "cubeworld.exe";
pub const DEFAULT_CHANNEL: &str = "beta";
/// Beside the game since its version 0.1.0: the version number it shows players.
const VERSION_FILE: &str = "game-version.txt";

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
    /// The version number the game shows, such as 0.1.1. Releases before 0.1.0 have none.
    pub version: Option<String>,
    pub commit: String,
    /// Installed and updated by the official launcher, as opposed to a folder the user chose.
    pub managed: bool,
    pub modules: BTreeMap<Backend, PathBuf>,
}

impl Game {
    /// How the game is named to players: its version number, else its release.
    pub fn shown(&self) -> &str {
        self.version.as_deref().unwrap_or(&self.release)
    }

    /// The client DLL the game runs on this PC: the avx2 one where the processor has
    /// AVX2, which is taken to be the game's own rule when it is left to choose.
    pub fn client(&self) -> Option<Backend> {
        #[cfg(target_arch = "x86_64")]
        let avx2 = std::arch::is_x86_feature_detected!("avx2");
        #[cfg(not(target_arch = "x86_64"))]
        let avx2 = false;
        let wanted = if avx2 {
            Backend::Avx2
        } else {
            Backend::Baseline
        };
        if self.modules.contains_key(&wanted) {
            return Some(wanted);
        }
        self.modules.keys().next().copied()
    }
}

/// Where the official launcher keeps its files, if it can be found.
pub fn launcher_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        let local = std::env::var_os("LOCALAPPDATA")?;
        return Some(PathBuf::from(local).join(wine::LAUNCHER_ID));
    }
    wine::launcher_dir()
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
    let version = fs::read_to_string(root.join(VERSION_FILE))
        .ok()
        .and_then(|text| text.lines().next().map(|line| line.trim().to_owned()))
        .filter(|version| {
            (1..=32).contains(&version.len())
                && version
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
        });
    Ok(Game {
        release: field("release").unwrap_or_else(|| "unknown".into()),
        version,
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
        assert_eq!((game.version.as_deref(), game.shown()), (None, "gh-1"));
        fs::write(dir.path().join(VERSION_FILE), "0.1.1\r\n").unwrap();
        let game = find(Some(dir.path()), DEFAULT_CHANNEL).unwrap();
        assert_eq!(game.shown(), "0.1.1");
        fs::write(dir.path().join(VERSION_FILE), "not a version").unwrap();
        assert_eq!(
            find(Some(dir.path()), DEFAULT_CHANNEL).unwrap().version,
            None
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
