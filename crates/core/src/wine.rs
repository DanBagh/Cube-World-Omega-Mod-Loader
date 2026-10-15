//! The game away from Windows, where it runs under Wine or Proton.
//!
//! A Wine prefix is a folder holding a Windows drive. The official launcher keeps its files
//! in the user folder there. The in-game loader opens files the Windows way, and in a
//! prefix the root of the host's file system is drive Z:.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The official launcher's folder in local application data.
pub const LAUNCHER_ID: &str = "com.wollay.launcher";

/// Where Wine, Steam, Lutris, Heroic and Bottles put prefixes by default, from the home
/// folder. `*` is any folder.
const PREFIXES: [&str; 8] = [
    ".wine",
    "Games/*",
    "Games/Heroic/Prefixes/default/*",
    ".local/share/Steam/steamapps/compatdata/*/pfx",
    ".steam/steam/steamapps/compatdata/*/pfx",
    ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/compatdata/*/pfx",
    ".local/share/bottles/bottles/*",
    ".var/app/com.usebottles.bottles/data/bottles/bottles/*",
];
/// The launcher's folder in a prefix.
const IN_PREFIX: &str = "drive_c/users/*/AppData/Local";

/// The folders `pattern` names under `base`.
fn expand(base: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut found = vec![base.to_path_buf()];
    for part in pattern.split('/') {
        found = found
            .into_iter()
            .flat_map(|dir| match part {
                "*" => fs::read_dir(&dir)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect(),
                name => vec![dir.join(name)],
            })
            .collect();
    }
    found.into_iter().filter(|dir| dir.is_dir()).collect()
}

/// When the launcher at `dir` last installed or updated the game.
fn last_install(dir: &Path) -> Option<SystemTime> {
    expand(dir, "games/cube-world-omega/*")
        .iter()
        .filter_map(|channel| {
            fs::metadata(channel.join("installed.json"))
                .ok()?
                .modified()
                .ok()
        })
        .max()
}

/// The launcher's folder in the prefix `named`, else the one that last installed the
/// game among the prefixes under `home` and the folder a Linux build would use.
pub fn find(home: &Path, named: Option<&Path>) -> Option<PathBuf> {
    let in_prefix = |prefix: &Path| -> Vec<PathBuf> {
        expand(prefix, IN_PREFIX)
            .into_iter()
            .map(|local| local.join(LAUNCHER_ID))
            .filter(|dir| dir.is_dir())
            .collect()
    };
    if let Some(found) = named.and_then(|prefix| in_prefix(prefix).into_iter().next()) {
        return Some(found);
    }
    let mut found: Vec<PathBuf> = PREFIXES
        .iter()
        .flat_map(|pattern| expand(home, pattern))
        .flat_map(|prefix| in_prefix(&prefix))
        .collect();
    found.extend(expand(home, &format!(".local/share/{LAUNCHER_ID}")));
    found.into_iter().max_by_key(|dir| last_install(dir))
}

/// The official launcher's folder, by `WINEPREFIX` or else the usual places.
pub fn launcher_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let named = std::env::var_os("WINEPREFIX").map(PathBuf::from);
    find(Path::new(&home), named.as_deref())
}

/// A host path as Wine shows it to Windows programs.
fn on_drive_z(path: &str) -> String {
    format!("Z:{}", path.replace('/', "\\"))
}

/// `path` as the in-game loader opens it.
pub fn game_path(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) || !text.starts_with('/') {
        return text;
    }
    on_drive_z(&text)
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::time::Duration;

    use super::*;

    /// A launcher folder in `prefix` whose game was installed `age` seconds ago.
    fn launcher(prefix: &Path, user: &str, age: u64) -> PathBuf {
        let dir = prefix
            .join("drive_c/users")
            .join(user)
            .join("AppData/Local")
            .join(LAUNCHER_ID);
        let channel = dir.join("games/cube-world-omega/beta");
        fs::create_dir_all(&channel).unwrap();
        let installed = File::create(channel.join("installed.json")).unwrap();
        installed
            .set_modified(SystemTime::now() - Duration::from_secs(age))
            .unwrap();
        dir
    }

    #[test]
    fn finds_the_launcher_that_last_installed_the_game() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        assert_eq!(find(home, None), None);

        let wine = launcher(&home.join(".wine"), "dan", 600);
        assert_eq!(find(home, None).as_ref(), Some(&wine));
        let steam = home.join(".local/share/Steam/steamapps/compatdata/4242/pfx");
        let proton = launcher(&steam, "steamuser", 60);
        launcher(&home.join("Games/other"), "dan", 6000);
        assert_eq!(find(home, None), Some(proton));
        // the prefix WINEPREFIX names, however old
        assert_eq!(find(home, Some(&home.join(".wine"))), Some(wine));
    }

    #[test]
    fn finds_the_game_of_a_non_steam_game_under_proton() {
        // the players' setup: the installer added to Steam, the launcher installed beside
        let home = tempfile::tempdir().unwrap();
        let prefix = home
            .path()
            .join(".steam/steam/steamapps/compatdata/4137892324/pfx");
        let found = launcher(&prefix, "steamuser", 0);
        let installed = found.with_file_name("Wollay Launcher");
        fs::create_dir_all(&installed).unwrap();
        File::create(installed.join("wollay-launcher.exe")).unwrap();
        assert_eq!(find(home.path(), None), Some(found));
    }

    #[test]
    fn host_paths_are_on_drive_z() {
        assert_eq!(
            on_drive_z("/home/dan/cwoml-data/mods/a b.wav"),
            r"Z:\home\dan\cwoml-data\mods\a b.wav"
        );
        assert_eq!(game_path(Path::new("coin.cub")), "coin.cub");
    }
}
