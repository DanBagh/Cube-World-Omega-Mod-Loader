//! Mods on disk: one folder per mod with a `mod.json` manifest.

mod install;
mod notice;
mod resolve;

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::version::Version;

pub use install::{Imported, Inspected, import_zip, inspect_zip, remove};
pub use notice::Notice;
pub use resolve::{Installed, resolve};

pub const MANIFEST_NAME: &str = "mod.json";
pub const MANIFEST_SCHEMA: i64 = 1;
const DEFAULT_ICON: &str = "icon.png";
/// File extensions accepted for a mod's icon.
pub const ICON_KINDS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

#[derive(Debug, Clone)]
pub struct Mod {
    pub dir: PathBuf,
    pub id: String,
    pub name: String,
    pub version: Version,
    pub priority: i64,
    /// The whole manifest. Each feature reads its own section from it.
    pub manifest: Map<String, Value>,
    /// Why the mod cannot be loaded. Empty for a mod that can.
    pub errors: Vec<Notice>,
    /// What the user should know about a mod that still loads.
    pub warnings: Vec<Notice>,
    /// Turned off by the user's profile.
    pub enabled: bool,
    /// Place in the user's profile. Lower loads earlier, dependencies permitting.
    pub rank: usize,
}

impl Mod {
    pub fn active(&self) -> bool {
        self.enabled && self.errors.is_empty()
    }

    pub fn description(&self) -> &str {
        self.manifest
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    pub fn authors(&self) -> Vec<&str> {
        self.manifest
            .get("authors")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    /// A manifest value that should be a list. Anything else reads as empty.
    pub fn list(&self, key: &str) -> &[Value] {
        self.manifest
            .get(key)
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    /// A manifest value that should be an object. Anything else reads as None.
    pub fn object(&self, key: &str) -> Option<&Map<String, Value>> {
        self.manifest.get(key).and_then(Value::as_object)
    }

    /// The picture the mod ships for itself: the manifest's `"icon"`, or `icon.png` in its
    /// folder. None if there is no such file or it is not a kind of image that is shown.
    pub fn icon(&self) -> Option<PathBuf> {
        let named = self.manifest.get("icon").and_then(Value::as_str);
        let path = self.file(named.unwrap_or(DEFAULT_ICON))?;
        let kind = path.extension()?.to_str()?.to_ascii_lowercase();
        ICON_KINDS.contains(&kind.as_str()).then_some(path)
    }

    /// A file the manifest names, if it exists inside the mod's folder.
    pub fn file(&self, relative: &str) -> Option<PathBuf> {
        let root = self.dir.canonicalize().ok()?;
        let path = self.dir.join(relative).canonicalize().ok()?;
        (path.starts_with(&root) && path != root && path.is_file()).then(|| plain(path))
    }
}

/// Windows canonical paths start with `\\?\`, which the game's file calls are not
/// guaranteed to accept. This gives the ordinary spelling back.
fn plain(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

fn valid_id(id: &str) -> bool {
    let allowed =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-');
    (1..=64).contains(&id.len()) && id.chars().all(allowed) && !id.starts_with(['_', '.', '-'])
}

fn parse_manifest(text: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(manifest)) => Ok(manifest),
        Ok(_) => Err(format!("{MANIFEST_NAME} must be a JSON object")),
        Err(e) => Err(e.to_string()),
    }
}

fn read_manifest(path: &Path) -> Result<Map<String, Value>, String> {
    parse_manifest(&fs::read_to_string(path).map_err(|e| e.to_string())?)
}

/// Reads every mod folder in `mods_dir`, sorted by folder name. Mods that cannot be used
/// are returned too, with the reason in `errors`.
pub fn load(mods_dir: &Path) -> Vec<Mod> {
    let mut folders: Vec<PathBuf> = fs::read_dir(mods_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    let mut mods: Vec<Mod> = Vec::new();
    for dir in folders {
        let folder = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let manifest_path = dir.join(MANIFEST_NAME);
        if !dir.is_dir() || folder.starts_with(['_', '.']) || !manifest_path.is_file() {
            continue;
        }
        let mut found = Mod {
            dir,
            id: folder.clone(),
            name: folder.clone(),
            version: Version::default(),
            priority: 0,
            manifest: Map::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            enabled: true,
            rank: 0,
        };
        match read_manifest(&manifest_path) {
            Err(reason) => found
                .errors
                .push(Notice::new(format!("Unreadable {MANIFEST_NAME}")).with("Reason", reason)),
            Ok(manifest) => {
                let text = |key: &str| manifest.get(key).and_then(Value::as_str);
                if manifest.get("schema").and_then(Value::as_i64) != Some(MANIFEST_SCHEMA) {
                    found.errors.push(
                        Notice::new("Unsupported manifest schema")
                            .with("Supported", MANIFEST_SCHEMA),
                    );
                }
                match text("id") {
                    Some(id) if valid_id(id) => found.id = id.to_owned(),
                    other => {
                        found
                            .errors
                            .push(Notice::new("Invalid id").with("Allowed", "a-z 0-9 . _ -"));
                        if let Some(id) = other.filter(|id| !id.is_empty()) {
                            found.id = id.to_owned();
                        }
                    }
                }
                found.name = text("name").map_or_else(|| found.id.clone(), str::to_owned);
                match text("version").and_then(Version::parse) {
                    Some(version) => found.version = version,
                    None => found
                        .errors
                        .push(Notice::new("Invalid version").with("Expected", "1.2.3")),
                }
                match manifest.get("priority") {
                    None => {}
                    Some(value) => match value.as_i64() {
                        Some(priority) => found.priority = priority,
                        None => found
                            .errors
                            .push(Notice::new("Invalid priority").with("Expected", "an integer")),
                    },
                }
                found.manifest = manifest;
            }
        }
        if found.active()
            && let Some(first) = mods.iter().find(|m| m.id == found.id)
        {
            let other = first
                .dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            found
                .errors
                .push(Notice::new("Duplicate id").with("Also in", other));
        }
        mods.push(found);
    }
    mods
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Writes `manifest` as the mod.json of a new folder in `mods_dir`.
    pub(crate) fn write_mod(mods_dir: &Path, folder: &str, manifest: &str) -> PathBuf {
        let dir = mods_dir.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MANIFEST_NAME), manifest).unwrap();
        dir
    }

    #[test]
    fn reads_manifests_and_reports_broken_ones() {
        let dir = tempfile::tempdir().unwrap();
        write_mod(
            dir.path(),
            "good",
            r#"{"schema": 1, "id": "good", "name": "Good", "version": "1.2", "priority": 5, "authors": ["a", "b"], "description": "d"}"#,
        );
        write_mod(dir.path(), "broken", "{ not json");
        write_mod(
            dir.path(),
            "old",
            r#"{"schema": 9, "id": "old", "version": "1.0.0"}"#,
        );
        write_mod(
            dir.path(),
            "shouting",
            r#"{"schema": 1, "id": "Shouting", "version": "one"}"#,
        );
        write_mod(
            dir.path(),
            "off",
            r#"{"schema": 1, "id": "off", "version": "1.0.0", "enabled": false}"#,
        );
        write_mod(
            dir.path(),
            "_hidden",
            r#"{"schema": 1, "id": "hidden", "version": "1.0.0"}"#,
        );
        fs::create_dir(dir.path().join("empty")).unwrap();

        let mods = load(dir.path());
        let by_folder = |name: &str| mods.iter().find(|m| m.dir.ends_with(name)).unwrap();
        assert_eq!(mods.len(), 5);

        let good = by_folder("good");
        assert!(good.active());
        assert_eq!(
            (
                good.id.as_str(),
                good.name.as_str(),
                good.version,
                good.priority
            ),
            ("good", "Good", Version(1, 2, 0), 5)
        );
        assert_eq!((good.authors(), good.description()), (vec!["a", "b"], "d"));

        assert_eq!(by_folder("broken").errors[0].title, "Unreadable mod.json");
        assert_eq!(
            by_folder("old").errors[0].to_string(),
            "Unsupported manifest schema (Supported 1)"
        );
        assert_eq!(by_folder("shouting").errors.len(), 2);
        assert_eq!(by_folder("shouting").id, "Shouting");
        // a profile turns a mod off. The manifest's own "enabled" is not read any more
        assert!(by_folder("off").active());
    }

    #[test]
    fn the_second_mod_with_an_id_is_a_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        write_mod(
            dir.path(),
            "a",
            r#"{"schema": 1, "id": "same", "version": "1.0.0"}"#,
        );
        write_mod(
            dir.path(),
            "b",
            r#"{"schema": 1, "id": "same", "version": "1.0.0"}"#,
        );
        let mods = load(dir.path());
        assert!(mods[0].active());
        assert_eq!(
            mods[1].errors,
            [Notice::new("Duplicate id").with("Also in", "a")]
        );
    }

    #[test]
    fn files_must_be_inside_the_mod_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = write_mod(
            dir.path(),
            "m",
            r#"{"schema": 1, "id": "m", "version": "1.0.0"}"#,
        );
        fs::write(folder.join("a.txt"), b"x").unwrap();
        fs::write(dir.path().join("outside.txt"), b"x").unwrap();
        let found = &load(dir.path())[0];
        assert!(found.file("a.txt").is_some());
        assert!(found.file("../outside.txt").is_none());
        assert!(found.file("missing.txt").is_none());
    }

    #[test]
    fn finds_the_icon_a_mod_ships() {
        let dir = tempfile::tempdir().unwrap();
        let plain = write_mod(
            dir.path(),
            "a",
            r#"{"schema": 1, "id": "a", "version": "1.0.0"}"#,
        );
        let named = write_mod(
            dir.path(),
            "b",
            r#"{"schema": 1, "id": "b", "version": "1.0.0", "icon": "art/logo.webp"}"#,
        );
        let odd = write_mod(
            dir.path(),
            "c",
            r#"{"schema": 1, "id": "c", "version": "1.0.0", "icon": "logo.svg"}"#,
        );
        assert!(load(dir.path())[0].icon().is_none());

        fs::write(plain.join("icon.png"), b"x").unwrap();
        fs::create_dir(named.join("art")).unwrap();
        fs::write(named.join("art/logo.webp"), b"x").unwrap();
        fs::write(odd.join("logo.svg"), b"x").unwrap();
        let mods = load(dir.path());
        assert!(mods[0].icon().unwrap().ends_with("icon.png"));
        assert!(mods[1].icon().unwrap().ends_with("logo.webp"));
        assert!(mods[2].icon().is_none());
    }

    #[test]
    fn ids_are_lowercase_and_short() {
        assert!(valid_id("pet-food_labels.2"));
        assert!(!valid_id(""));
        assert!(!valid_id("-leading"));
        assert!(!valid_id("Upper"));
        assert!(!valid_id(&"x".repeat(65)));
    }
}
