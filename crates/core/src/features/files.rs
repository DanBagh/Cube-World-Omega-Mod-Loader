//! File overrides: the `"files"` manifest section.
//!
//! `"files"` maps a game-relative path to a file in the mod folder. The game's own file
//! stays on disk. The in-game loader hands the game the replacement when it opens that
//! path, so nothing here depends on how a build was compiled, only on the game still
//! asking for a file of that name.
//!
//! Models are read first, because a malformed one could crash the game, and MagicaVoxel
//! files are converted so a mod can ship the `.vox` it was made in.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::game::Game;
use crate::mods::Mod;
use crate::report::Report;
use crate::voxel::{MAX_CELLS, Model};

/// A file served in place of one of the game's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub source: PathBuf,
    /// Id of the mod it comes from.
    pub owner: String,
}

/// `target` as the lowercase, forward-slash path the game is matched against, or None if
/// it is not a path inside the game folder that may be overridden.
fn game_key(target: &str) -> Option<String> {
    let mut key = target.replace('\\', "/").to_lowercase();
    while let Some(rest) = key.strip_prefix("./") {
        key = rest.to_owned();
    }
    let outside = key.is_empty()
        || key.starts_with('/')
        || key.contains(':')
        || key.split('/').any(|part| part == "..");
    let executable = key.ends_with(".dll") || key.ends_with(".exe");
    (!outside && !executable).then_some(key)
}

fn name_for(data: &[u8]) -> String {
    // FNV-1a: the same model always gets the same file
    let hash = data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}.cub")
}

/// The `.cub` the game is given for a mod's model file: `source` itself once it has been
/// checked, or a conversion of it written to `converted`.
pub fn usable_model(
    source: &Path,
    converted: &Path,
    warn: &mut dyn FnMut(String),
) -> Result<PathBuf, String> {
    let model = Model::load(source)?;
    let [x, y, z] = model.size;
    if model.cells() > MAX_CELLS {
        return Err(format!(
            "{x}x{y}x{z} is more than the {MAX_CELLS} cells the game loads"
        ));
    }
    let keyed = model.keyed();
    if keyed > 0 {
        warn(format!(
            "{keyed} voxel(s) are pure red, green or blue, which the game leaves out of a model; use a slightly different colour"
        ));
    }
    if source
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cub"))
    {
        return Ok(source.to_path_buf());
    }
    let data = model.to_cub();
    let path = converted.join(name_for(&data));
    fs::create_dir_all(converted)
        .and_then(|()| fs::write(&path, data))
        .map_err(|e| e.to_string())?;
    Ok(path)
}

/// Every file the mods replace, by game path. Converted models are written to `converted`.
/// When two mods replace the same file, the later one wins and it is reported.
pub fn collect(
    game: &Game,
    mods: &[Mod],
    order: &[usize],
    converted: &Path,
    report: &mut Report,
) -> BTreeMap<String, Override> {
    let mut overrides: BTreeMap<String, Override> = BTreeMap::new();
    for found in order.iter().map(|&index| &mods[index]) {
        let id = &found.id;
        if found
            .manifest
            .get("files")
            .is_some_and(|v| !matches!(v, Value::Object(_) | Value::Null))
        {
            report.warn(format!(
                "{id}: \"files\" must be an object of game path to mod file; ignored"
            ));
        }
        for (target, source) in found.object("files").into_iter().flatten() {
            let Some(key) = game_key(target) else {
                report.warn(format!(
                    "{id}: \"{target}\" is not an overridable game-relative path; skipped"
                ));
                continue;
            };
            let relative = source.as_str().unwrap_or_default();
            let Some(mut path) = found.file(relative) else {
                report.warn(format!(
                    "{id}: replacement \"{relative}\" is missing or outside the mod folder; skipped"
                ));
                continue;
            };
            if key.ends_with(".cub") {
                let mut warn = |text: String| report.warn(format!("{id}: \"{relative}\": {text}"));
                match usable_model(&path, converted, &mut warn) {
                    Ok(usable) => path = usable,
                    Err(reason) => {
                        report.warn(format!(
                            "{id}: \"{relative}\" is not a usable model ({reason}); skipped"
                        ));
                        continue;
                    }
                }
            }
            if !game.root.join(&key).is_file() {
                report.warn(format!("{id}: the game has no file \"{key}\"; the override will only matter if the game asks for it"));
            }
            if let Some(earlier) = overrides.get(&key) {
                report.warn(format!(
                    "file conflict on {key}: {} and {id}; {id} wins",
                    earlier.owner
                ));
            }
            overrides.insert(
                key,
                Override {
                    source: path,
                    owner: id.clone(),
                },
            );
        }
    }
    overrides
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};
    use crate::voxel::tests::sample;

    struct Setup {
        _dir: tempfile::TempDir,
        game: Game,
        mods: Vec<Mod>,
        order: Vec<usize>,
        converted: PathBuf,
    }

    /// A mod to write: its manifest and the files beside it.
    struct Fixture(&'static str, Vec<(&'static str, Vec<u8>)>);

    /// A game folder holding `game_files` and a mods folder holding `mods`.
    fn setup(game_files: &[&str], mods: Vec<Fixture>) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("game");
        fs::create_dir_all(&root).unwrap();
        for file in game_files {
            fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            fs::write(root.join(file), b"game").unwrap();
        }
        let mods_dir = dir.path().join("mods");
        for (n, Fixture(manifest, files)) in mods.iter().enumerate() {
            let folder = write_mod(&mods_dir, &format!("m{n}"), manifest);
            for (name, data) in files {
                fs::create_dir_all(folder.join(name).parent().unwrap()).unwrap();
                fs::write(folder.join(name), data).unwrap();
            }
        }
        let mut mods = load(&mods_dir);
        let order = resolve(&mut mods, "gh-1");
        let game = Game {
            root,
            release: "gh-1".into(),
            commit: String::new(),
            managed: false,
            modules: BTreeMap::new(),
        };
        let converted = dir.path().join("converted");
        Setup {
            _dir: dir,
            game,
            mods,
            order,
            converted,
        }
    }

    fn run(setup: &Setup) -> (BTreeMap<String, Override>, Vec<String>) {
        let mut report = Report::default();
        let overrides = collect(
            &setup.game,
            &setup.mods,
            &setup.order,
            &setup.converted,
            &mut report,
        );
        (overrides, report.warnings().map(str::to_owned).collect())
    }

    #[test]
    fn maps_game_paths_to_mod_files() {
        let setup = setup(
            &["sounds/pickup.wav"],
            vec![Fixture(
                r#"{"schema": 1, "id": "a", "version": "1.0.0", "files": {"Sounds\\Pickup.WAV": "assets/a.wav", "./sounds/new.wav": "assets/a.wav"}}"#,
                vec![("assets/a.wav", b"A".to_vec())],
            )],
        );
        let (overrides, warnings) = run(&setup);
        assert_eq!(
            overrides.keys().collect::<Vec<_>>(),
            ["sounds/new.wav", "sounds/pickup.wav"]
        );
        let served = &overrides["sounds/pickup.wav"];
        assert_eq!(
            (fs::read(&served.source).unwrap(), served.owner.as_str()),
            (b"A".to_vec(), "a")
        );
        assert!(!served.source.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(
            warnings,
            [
                "a: the game has no file \"sounds/new.wav\"; the override will only matter if the game asks for it"
            ]
        );
    }

    #[test]
    fn refuses_paths_outside_the_game_and_missing_sources() {
        let setup = setup(
            &[],
            vec![Fixture(
                r#"{"schema": 1, "id": "a", "version": "1.0.0", "files": {
                    "../escape.wav": "a.wav", "C:/windows/x.wav": "a.wav", "/abs.wav": "a.wav", "cubeworld.exe": "a.wav",
                    "sounds/x.wav": "missing.wav", "sounds/y.wav": "../outside.wav"}}"#,
                vec![("a.wav", b"A".to_vec())],
            )],
        );
        let (overrides, warnings) = run(&setup);
        assert!(overrides.is_empty());
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.contains("not an overridable game-relative path"))
                .count(),
            4
        );
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.contains("missing or outside the mod folder"))
                .count(),
            2
        );
    }

    #[test]
    fn converts_vox_models_and_refuses_broken_ones() {
        let mut keyed = sample();
        keyed.voxels.insert([0, 0, 1], [255, 0, 0]);
        let setup = setup(
            &[
                "sprites/coin.cub",
                "sprites/gem.cub",
                "sprites/rock.cub",
                "sprites/key.cub",
            ],
            vec![Fixture(
                r#"{"schema": 1, "id": "a", "version": "1.0.0", "files": {
                    "sprites/coin.cub": "coin.vox", "sprites/gem.cub": "gem.cub", "sprites/rock.cub": "broken.cub", "sprites/key.cub": "keyed.cub"}}"#,
                vec![
                    ("coin.vox", sample().to_vox().unwrap()),
                    ("gem.cub", sample().to_cub()),
                    ("broken.cub", b"not a model".to_vec()),
                    ("keyed.cub", keyed.to_cub()),
                ],
            )],
        );
        let (overrides, warnings) = run(&setup);
        assert_eq!(
            overrides.keys().collect::<Vec<_>>(),
            ["sprites/coin.cub", "sprites/gem.cub", "sprites/key.cub"]
        );
        let converted = &overrides["sprites/coin.cub"].source;
        assert!(converted.starts_with(&setup.converted) && converted.extension().unwrap() == "cub");
        assert_eq!(fs::read(converted).unwrap(), sample().to_cub());
        assert!(overrides["sprites/gem.cub"].source.ends_with("gem.cub"));
        assert_eq!(warnings.len(), 2);
        assert!(
            warnings
                .iter()
                .any(|w| w.starts_with("a: \"broken.cub\" is not a usable model"))
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.starts_with("a: \"keyed.cub\": 1 voxel(s) are pure red, green or blue"))
        );
    }

    #[test]
    fn the_later_mod_wins_a_file() {
        let setup = setup(
            &["sounds/pickup.wav"],
            vec![
                Fixture(
                    r#"{"schema": 1, "id": "first", "version": "1.0.0", "files": {"sounds/pickup.wav": "a.wav"}}"#,
                    vec![("a.wav", b"A".to_vec())],
                ),
                Fixture(
                    r#"{"schema": 1, "id": "second", "version": "1.0.0", "priority": 1, "files": {"sounds/pickup.wav": "b.wav"}}"#,
                    vec![("b.wav", b"B".to_vec())],
                ),
            ],
        );
        let (overrides, warnings) = run(&setup);
        assert_eq!(overrides["sounds/pickup.wav"].owner, "second");
        assert_eq!(
            warnings,
            ["file conflict on sounds/pickup.wav: first and second; second wins"]
        );
    }

    #[test]
    fn a_files_section_of_the_wrong_shape_is_reported() {
        let setup = setup(
            &[],
            vec![Fixture(
                r#"{"schema": 1, "id": "a", "version": "1.0.0", "files": ["x"]}"#,
                vec![],
            )],
        );
        let (overrides, warnings) = run(&setup);
        assert!(overrides.is_empty());
        assert_eq!(
            warnings,
            ["a: \"files\" must be an object of game path to mod file; ignored"]
        );
    }
}
