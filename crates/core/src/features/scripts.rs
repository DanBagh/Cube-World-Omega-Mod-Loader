//! Scripts: the `"scripts"` manifest section, and a mod's strings in `lang/`.
//!
//! A mod's scripts run in the in-game loader's Luau runtime, each mod in a sandbox of its
//! own. The runtime starts from the frame hook `ui` plans.
//!
//! Strings live in `lang/<language>.json`, one flat object of keys and texts per
//! language. The runtime hands a script the text for the language the game is in, and
//! the English one where a language lacks the key, so scripts never pick a language
//! themselves.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::mods::{Mod, Notice};
use crate::plan::Scripts;
use crate::report::one_line;

/// The game shows English, and has no setting for its language.
pub const LANGUAGE: &str = "en";
const FALLBACK: &str = "en";
const LANG_DIR: &str = "lang";
const KINDS: [&str; 2] = ["luau", "lua"];
/// The longest string key.
const MAX_KEY: usize = 128;

/// A mod-relative path that stays inside the mod.
fn inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    let relative = Path::new(relative);
    relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then(|| dir.join(relative))
}

/// A mod's strings in one language, if it has a file for it.
fn strings(found: &Mod, language: &str) -> Result<BTreeMap<String, String>, String> {
    let file = found.dir.join(LANG_DIR).join(format!("{language}.json"));
    let Ok(text) = fs::read_to_string(&file) else {
        return Ok(BTreeMap::new());
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{LANG_DIR}/{language}.json is not JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("{LANG_DIR}/{language}.json is an object of keys and texts"))?;
    let mut texts = BTreeMap::new();
    for (key, text) in object {
        let fits = !key.is_empty() && key.len() <= MAX_KEY && !key.contains(['\t', '\n', '\r']);
        let text = text.as_str().filter(|_| fits).ok_or_else(|| {
            format!("{LANG_DIR}/{language}.json: '{key}' needs a text, and a key without tabs or line breaks")
        })?;
        texts.insert(key.clone(), text.to_owned());
    }
    Ok(texts)
}

/// Every loading mod's scripts and strings, in load order.
pub fn collect(mods: &mut [Mod], order: &[usize], language: &str) -> Scripts {
    let mut scripts = Scripts {
        language: language.to_owned(),
        ..Scripts::default()
    };
    for &index in order {
        let found = &mut mods[index];
        for entry in found.list("scripts").to_vec() {
            let file = entry
                .as_str()
                .ok_or_else(|| "an entry is a path in the mod".to_owned())
                .and_then(|path| {
                    let file = inside(&found.dir, path)
                        .ok_or_else(|| "the path leaves the mod".to_owned())?;
                    let kind = file
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or_default();
                    if !KINDS.contains(&kind.to_ascii_lowercase().as_str()) {
                        return Err("a script ends in .luau or .lua".to_owned());
                    }
                    if !file.is_file() {
                        return Err("the file is not in the mod".to_owned());
                    }
                    Ok(file)
                });
            match file {
                Ok(file) => scripts.files.push((found.id.clone(), file)),
                Err(reason) => found.warnings.push(
                    Notice::new("Script ignored")
                        .with("Entry", one_line(&entry.to_string(), 70))
                        .with("Reason", reason),
                ),
            }
        }
        let mut texts = BTreeMap::new();
        for wanted in [FALLBACK, language] {
            match strings(found, wanted) {
                Ok(more) => texts.extend(more),
                Err(reason) => found
                    .warnings
                    .push(Notice::new("Strings ignored").with("Reason", reason)),
            }
        }
        if !texts.is_empty() {
            scripts.texts.insert(found.id.clone(), texts);
        }
    }
    scripts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};

    #[test]
    fn reads_scripts_and_strings_with_english_where_a_language_lacks_a_key() {
        let dir = tempfile::tempdir().unwrap();
        write_mod(
            dir.path(),
            "a",
            r#"{"schema": 1, "id": "a", "version": "1.0.0",
                "scripts": ["scripts/main.luau", "../escape.luau", "scripts/gone.luau", 7]}"#,
        );
        let root = dir.path().join("a");
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("scripts/main.luau"), "cwo.log('hi')").unwrap();
        fs::create_dir_all(root.join("lang")).unwrap();
        fs::write(
            root.join("lang/en.json"),
            r#"{"title": "GEAR STATS", "close": "CLOSE"}"#,
        )
        .unwrap();
        fs::write(root.join("lang/de.json"), r#"{"title": "AUSRUESTUNG"}"#).unwrap();
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");

        let scripts = collect(&mut mods, &order, "de");
        assert_eq!(
            scripts.files,
            [("a".to_owned(), root.join("scripts/main.luau"))]
        );
        let texts = &scripts.texts["a"];
        assert_eq!(texts["title"], "AUSRUESTUNG");
        assert_eq!(texts["close"], "CLOSE");
        assert_eq!(mods[0].warnings.len(), 3);
    }
}
