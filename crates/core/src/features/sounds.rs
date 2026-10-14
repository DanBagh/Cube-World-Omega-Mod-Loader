//! The client's numbered sounds: the id it loads each sound file under. Sounds a mod adds
//! are `added_sounds`.
//!
//! Build-specific: the ids are read from the client's sound loader.

use std::collections::BTreeMap;

use super::stub::signature;
use super::text::latin1;
use crate::pe::PeImage;
use crate::refs::{RefIndex, SiteKind, string_sites};

/// The sounds the mods add: the id each name plays under, and whether this client can play
/// them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Added {
    pub ids: BTreeMap<String, u16>,
    pub playable: bool,
}

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

/// The id of a sound a mod names: a game path, or the name of an added sound.
pub fn resolve(image: &PeImage, refs: &RefIndex, added: &Added, sound: &str) -> Option<u16> {
    match added.ids.get(sound) {
        Some(&id) => added.playable.then_some(id),
        None => sound_id(image, refs, sound),
    }
}

/// Why `resolve` found nothing, for a report line.
pub fn unknown(added: &Added, sound: &str) -> &'static str {
    if sound.contains('/') {
        "is not a sound the game loads"
    } else if added.ids.contains_key(sound) {
        "is an added sound, and this game version cannot play added sounds"
    } else {
        "is not a sound this mod set adds (see \"sounds\")"
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
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
    fn added_sounds_resolve_where_they_can_be_played() {
        let mut pe = TinyPe::new();
        loads(&mut pe, "sounds/pickup.wav", 0x3D);
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let mut added = Added {
            ids: BTreeMap::from([("chime".to_owned(), 0x8000)]),
            playable: true,
        };
        assert_eq!(resolve(&image, &refs, &added, "chime"), Some(0x8000));
        assert_eq!(
            resolve(&image, &refs, &added, "sounds/pickup.wav"),
            Some(0x3D)
        );
        added.playable = false;
        assert_eq!(resolve(&image, &refs, &added, "chime"), None);
        assert_eq!(
            unknown(&added, "chime"),
            "is an added sound, and this game version cannot play added sounds"
        );
        assert_eq!(
            unknown(&added, "other"),
            "is not a sound this mod set adds (see \"sounds\")"
        );
    }
}
