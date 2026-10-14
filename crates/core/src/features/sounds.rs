//! The client's numbered sounds, and sounds a mod adds: the `"sounds"` manifest section.
//!
//! The client loads each sound file into a slot with a fixed id. Its table has a fixed
//! size, but it asks for a few files it does not ship. A mod's own sound is served under
//! such a name, which gives it an id without replacing anything the game plays today.
//!
//! Build-specific: the ids are read from the client's sound loader.

use std::collections::BTreeMap;

use super::files::Override;
use super::stub::signature;
use super::text::latin1;
use crate::game::Game;
use crate::mods::Mod;
use crate::pe::PeImage;
use crate::refs::{RefIndex, SiteKind, string_sites};
use crate::report::Report;

/// Name of an added sound to the game path of the slot it took.
pub type Added = BTreeMap<String, String>;

/// The id the client loads a sound file under: `lea reg, [path]` ... `mov dx, <id>`.
pub fn sound_id(image: &PeImage, refs: &RefIndex, path: &str) -> Option<u16> {
    let (text_va, text) = image.data(".text")?;
    let id = signature(r"\x66\xba(..)");
    let mut ids: Vec<u16> = Vec::new();
    for site in string_sites(image, refs, &latin1(path)?)
        .values()
        .flatten()
        .filter(|site| site.kind == SiteKind::Lea)
    {
        let after = (site.rva - text_va) as usize + 4;
        let window = &text[after..text.len().min(after + 24)];
        if let Some(found) = id.captures(window) {
            let found = u16::from_le_bytes([found[1][0], found[1][1]]);
            if !ids.contains(&found) {
                ids.push(found);
            }
        }
    }
    (ids.len() == 1).then(|| ids[0])
}

/// Every sound file the client loads into a numbered slot, with its id.
pub fn game_sounds(image: &PeImage, refs: &RefIndex) -> Vec<(String, u16)> {
    let path = signature(r"sounds/[\x21-\x7e]+?\.wav\x00");
    let mut found: Vec<(String, u16)> = Vec::new();
    for section in [".rdata", ".data"] {
        let Some((_, data)) = image.data(section) else {
            continue;
        };
        for hit in path.find_iter(data) {
            let bytes = hit.as_bytes();
            let name = String::from_utf8_lossy(&bytes[..bytes.len() - 1]).into_owned();
            if found.iter().any(|(known, _)| *known == name) {
                continue;
            }
            if let Some(id) = sound_id(image, refs, &name) {
                found.push((name, id));
            }
        }
    }
    found
}

fn valid_name(name: &str) -> bool {
    let allowed =
        |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-');
    name.chars().all(allowed) && name.starts_with(|c: char| c.is_ascii_alphanumeric())
}

/// Gives each sound the mods add one of the client's unused slots, and serves the mod's
/// file under that slot's name. When two mods add a sound of the same name, the later wins.
pub fn add(
    game: &Game,
    mods: &[Mod],
    order: &[usize],
    image: &PeImage,
    refs: &RefIndex,
    overrides: &mut BTreeMap<String, Override>,
    report: &mut Report,
) -> Added {
    let mut wanted: Vec<(String, Override)> = Vec::new();
    for found in order.iter().map(|&index| &mods[index]) {
        let id = &found.id;
        for (name, source) in found.object("sounds").into_iter().flatten() {
            let relative = source.as_str().unwrap_or_default();
            let file = found.file(relative).filter(|path| {
                path.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
            });
            if !valid_name(name) {
                report.warn(format!("{id}: sound name '{name}' must be lowercase letters, digits, '.', '_' or '-'; skipped"));
                continue;
            }
            let Some(source) = file else {
                report.warn(format!(
                    "{id}: sound '{relative}' must be a .wav file inside the mod folder; skipped"
                ));
                continue;
            };
            let sound = Override {
                source,
                owner: id.clone(),
            };
            match wanted.iter_mut().find(|(known, _)| known == name) {
                Some((_, earlier)) => {
                    if earlier.owner != *id {
                        report.warn(format!(
                            "sound '{name}' is defined by {} and {id}; {id} wins",
                            earlier.owner
                        ));
                    }
                    *earlier = sound;
                }
                None => wanted.push((name.clone(), sound)),
            }
        }
    }
    let mut added = Added::new();
    if wanted.is_empty() {
        return added;
    }
    let mut free: Vec<(u16, String)> = game_sounds(image, refs)
        .into_iter()
        .filter(|(path, _)| {
            !game.root.join(path).is_file() && !overrides.contains_key(&path.to_lowercase())
        })
        .map(|(path, id)| (id, path))
        .collect();
    free.sort();
    let slots = free.len();
    let mut free = free.into_iter();
    for (name, sound) in wanted {
        let Some((_, path)) = free.next() else {
            report.warn(format!(
                "{}: no unused sound slot left for '{name}' (this build has {slots}); skipped",
                sound.owner
            ));
            continue;
        };
        report.say(format!(
            "Added sound '{name}' ({}) uses the client's unused slot {path}",
            sound.owner
        ));
        overrides.insert(path.to_lowercase(), sound);
        added.insert(name, path);
    }
    added
}

/// The id of a sound a mod names: a game path, or the name of an added sound.
pub fn resolve(image: &PeImage, refs: &RefIndex, added: &Added, sound: &str) -> Option<u16> {
    sound_id(image, refs, added.get(sound).map_or(sound, String::as_str))
}

/// Why `resolve` found nothing, for a report line.
pub fn unknown(sound: &str) -> &'static str {
    if sound.contains('/') {
        "is not a sound the game loads"
    } else {
        "is not a sound this mod set adds (see \"sounds\")"
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::game::Backend;
    use crate::mods::load;
    use crate::mods::tests::write_mod;
    use crate::testing::TinyPe;

    /// Adds the client's loading of `path` under `id`: lea rcx, [path] / mov dx, id.
    pub(crate) fn loads(pe: &mut TinyPe, path: &str, id: u16) -> u32 {
        let text = pe.rdata(&[path.as_bytes(), b"\0"].concat());
        pe.rip_relative(&[0x48, 0x8D, 0x0D], text);
        pe.code(&[0x4C, 0x89, 0xF1]);
        pe.code(&[&[0x66, 0xBA][..], &id.to_le_bytes()].concat());
        pe.code(&[0x90; 28]);
        text
    }

    #[test]
    fn reads_ids_from_the_sound_loader() {
        let mut pe = TinyPe::new();
        loads(&mut pe, "sounds/pickup.wav", 0x3D);
        loads(&mut pe, "sounds/cry.wav", 0x6B);
        pe.rdata(b"sounds/unused.wav\0");
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        assert_eq!(sound_id(&image, &refs, "sounds/pickup.wav"), Some(0x3D));
        assert_eq!(sound_id(&image, &refs, "sounds/unused.wav"), None);
        assert_eq!(
            game_sounds(&image, &refs),
            [
                ("sounds/pickup.wav".to_owned(), 0x3D),
                ("sounds/cry.wav".to_owned(), 0x6B)
            ]
        );
    }

    #[test]
    fn added_sounds_take_the_slots_whose_files_are_missing() {
        let mut pe = TinyPe::new();
        loads(&mut pe, "sounds/pickup.wav", 0x3D);
        loads(&mut pe, "sounds/rain.wav", 0x70);
        loads(&mut pe, "sounds/cry.wav", 0x6B);
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("game");
        std::fs::create_dir_all(root.join("sounds")).unwrap();
        std::fs::write(root.join("sounds/pickup.wav"), b"x").unwrap();
        let game = Game {
            root,
            release: "gh-1".into(),
            version: None,
            commit: String::new(),
            managed: false,
            modules: BTreeMap::from([(Backend::Avx2, dir.path().join("client.dll"))]),
        };
        let mods_dir = dir.path().join("mods");
        for (folder, manifest) in [
            (
                "a",
                r#"{"schema": 1, "id": "a", "version": "1.0.0", "sounds": {
                    "chime": "chime.wav", "Bad Name": "chime.wav", "text": "notes.txt", "gone": "gone.wav"}}"#,
            ),
            (
                "b",
                r#"{"schema": 1, "id": "b", "version": "1.0.0", "sounds": {
                    "chime": "other.wav", "second": "other.wav", "third": "other.wav"}}"#,
            ),
        ] {
            let folder = write_mod(&mods_dir, folder, manifest);
            for file in ["chime.wav", "other.wav", "notes.txt"] {
                std::fs::write(folder.join(file), b"RIFF").unwrap();
            }
        }
        let mods = load(&mods_dir);
        let mut overrides = BTreeMap::new();
        let mut report = Report::default();
        let added = add(
            &game,
            &mods,
            &[0, 1],
            &image,
            &refs,
            &mut overrides,
            &mut report,
        );

        // the lower id first, in the order the names were first seen. b's chime replaced a's
        assert_eq!(
            added,
            Added::from([
                ("chime".to_owned(), "sounds/cry.wav".to_owned()),
                ("second".to_owned(), "sounds/rain.wav".to_owned()),
            ])
        );
        let chime = &overrides["sounds/cry.wav"];
        assert!(chime.owner == "b" && chime.source.ends_with("other.wav"));
        assert_eq!(resolve(&image, &refs, &added, "second"), Some(0x70));
        assert_eq!(
            resolve(&image, &refs, &added, "sounds/pickup.wav"),
            Some(0x3D)
        );
        let warnings: Vec<&str> = report.warnings().collect();
        assert_eq!(warnings.len(), 5);
        assert!(warnings[0].contains("sound name 'Bad Name' must be"));
        assert!(warnings[3].contains("is defined by a and b; b wins"));
        assert!(
            warnings[4].starts_with("b: no unused sound slot left for 'third' (this build has 2)")
        );
    }
}
