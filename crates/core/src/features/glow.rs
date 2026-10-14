//! Glowing voxels: the `"glow"` manifest section.
//!
//! A model slot in the client has one "glows" bit per voxel, and the mesher puts the
//! marked voxels into a second, emissive mesh. Nothing in a `.cub` can set those bits. An
//! entry here names a model and which of its voxels glow: the ones of the given colours,
//! or the ones with MagicaVoxel's "Emit" material, or all of them. `item_models` sets the
//! bits when the game loads a model of that size for the entry's item.

use std::fs;
use std::path::Path;

use serde_json::Value;

use super::ground_models;
use super::stub::item_key;
use crate::mods::{Mod, Notice};
use crate::report::{Report, one_line};
use crate::voxel::{MAX_CELLS, Model, Rgb};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glow {
    pub item_type: u32,
    /// None for every subtype of the type.
    pub subtype: Option<u32>,
    /// Set when the entry is for the model a `groundModels` entry gives this amount.
    pub amount: Option<u16>,
    /// Size of the model the entry was made from. It only applies to a model of that size.
    pub size: [u32; 3],
    /// One bit per cell, x running fastest, in whole quadwords.
    pub mask: Vec<u8>,
    pub owner: String,
}

/// The model, its glowing cells, and how they were chosen.
fn lit(path: &Path, colours: Option<&[Rgb]>) -> Result<(Model, Vec<[u32; 3]>, String), String> {
    let data = fs::read(path).map_err(|e| e.to_string())?;
    let vox = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("vox"));
    let (model, emissive) = if vox {
        Model::from_vox_lit(&data)?
    } else {
        (Model::from_cub(&data)?, Vec::new())
    };
    if let Some(colours) = colours {
        let missing: Vec<String> = colours
            .iter()
            .filter(|colour| !model.voxels.values().any(|rgb| rgb == *colour))
            .map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"))
            .collect();
        if !missing.is_empty() {
            return Err(format!("no voxel has the colour {}", missing.join(", ")));
        }
        let cells = model
            .voxels
            .iter()
            .filter(|(_, rgb)| colours.contains(rgb))
            .map(|(cell, _)| *cell)
            .collect();
        return Ok((model, cells, format!("{} colour(s)", colours.len())));
    }
    if !emissive.is_empty() {
        return Ok((model, emissive, "Emit material".into()));
    }
    let cells = model.voxels.keys().copied().collect();
    Ok((model, cells, "whole model".into()))
}

fn colour(value: &Value) -> Option<Rgb> {
    let hex = value.as_str()?.trim_start_matches('#');
    let number = u32::from_str_radix(hex, 16)
        .ok()
        .filter(|_| hex.len() == 6)?;
    Some([(number >> 16) as u8, (number >> 8) as u8, number as u8])
}

/// The entry, how many voxels glow and how they were chosen.
fn parse(found: &Mod, patch: &Value) -> Result<(Glow, usize, String), String> {
    let patch = patch.as_object().ok_or("an entry is an object")?;
    let (item_type, mut subtype) = item_key(patch)?;
    let amount = match patch.get("amount") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let amount = value.as_u64().and_then(|n| u16::try_from(n).ok());
            match amount.filter(|&n| n > 0 && subtype.is_none()) {
                Some(amount) => {
                    subtype = Some(ground_models::SUBTYPE | u32::from(amount));
                    Some(amount)
                }
                None => {
                    return Err("amount is 1 to 65535 and cannot be combined with subtype".into());
                }
            }
        }
    };
    let relative = patch
        .get("model")
        .and_then(Value::as_str)
        .ok_or("model is missing")?;
    let path = found
        .file(relative)
        .ok_or_else(|| format!("model '{relative}' is missing or outside the mod folder"))?;
    let colours = match patch.get("colors") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_array()
                .filter(|list| !list.is_empty())
                .and_then(|list| list.iter().map(colour).collect::<Option<Vec<Rgb>>>())
                .ok_or(r##"colors must be a list like ["#4fe74d"]"##)?,
        ),
    };
    let (model, cells, how) = lit(&path, colours.as_deref())?;
    if model.cells() > MAX_CELLS {
        return Err(format!(
            "the model has more than {MAX_CELLS} cells, which is all the game loads"
        ));
    }
    if cells.is_empty() {
        return Err("the model is empty".into());
    }
    let [sx, sy, _] = model.size.map(|side| side as usize);
    let mut mask = vec![0u8; (model.cells() as usize).div_ceil(64) * 8];
    for [x, y, z] in cells.iter().map(|cell| cell.map(|at| at as usize)) {
        let bit = x + sx * (y + sy * z);
        mask[bit >> 3] |= 1 << (bit & 7);
    }
    let glow = Glow {
        item_type,
        subtype,
        amount,
        size: model.size,
        mask,
        owner: found.id.clone(),
    };
    Ok((glow, cells.len(), how))
}

/// One entry per item, a later mod replacing an earlier one. An exact subtype comes
/// before the entry for every subtype, which is the order the trampoline looks in.
pub fn collect(mods: &mut [Mod], order: &[usize], report: &mut Report) -> Vec<Glow> {
    let mut chosen: Vec<Glow> = Vec::new();
    for &index in order {
        for patch in mods[index].list("glow").to_vec() {
            let found = &mut mods[index];
            let (glow, count, how) = match parse(found, &patch) {
                Ok(entry) => entry,
                Err(reason) => {
                    found.warnings.push(
                        Notice::new("Glow entry ignored")
                            .with("Entry", one_line(&patch.to_string(), 70))
                            .with("Reason", reason),
                    );
                    continue;
                }
            };
            let which = match (glow.amount, glow.subtype) {
                (Some(amount), _) => format!(" ({amount} or more)"),
                (None, Some(subtype)) => format!(" subtype {subtype}"),
                (None, None) => String::new(),
            };
            let (item_type, [x, y, z], id) = (glow.item_type, glow.size, &found.id);
            report.say(format!("Glow: item type {item_type}{which} ({id}): {count} voxel(s) of a {x}x{y}x{z} model, chosen by {how}"));
            match chosen
                .iter_mut()
                .find(|g| (g.item_type, g.subtype) == (glow.item_type, glow.subtype))
            {
                Some(earlier) => {
                    if earlier.owner != glow.owner {
                        report.warn(format!(
                            "glow conflict for item type {item_type}{which}: {} and {id}; {id} wins",
                            earlier.owner
                        ));
                    }
                    *earlier = glow;
                }
                None => chosen.push(glow),
            }
        }
    }
    chosen.sort_by_key(|g| (g.item_type, g.subtype.is_none(), g.subtype.unwrap_or(0)));
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};
    use crate::voxel::tests::sample;

    fn collected(manifest: &str) -> (Vec<Glow>, Vec<Mod>, Report) {
        let dir = tempfile::tempdir().unwrap();
        let folder = write_mod(dir.path(), "m", manifest);
        // 2x1x3: two voxels of one colour at the bottom, one of another at the top
        fs::write(folder.join("coin.cub"), sample().to_cub()).unwrap();
        fs::write(folder.join("coin.vox"), sample().to_vox().unwrap()).unwrap();
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let mut report = Report::default();
        let glows = collect(&mut mods, &order, &mut report);
        (glows, mods, report)
    }

    #[test]
    fn voxels_glow_by_colour_or_all_of_them() {
        let (glows, mods, report) = collected(
            r##"{"schema": 1, "id": "m", "version": "1.0.0", "glow": [
                {"type": 43, "model": "coin.cub"},
                {"type": 43, "subtype": 2, "model": "coin.vox", "colors": ["#C86400"]},
                {"type": 43, "amount": 5, "model": "coin.cub", "colors": ["#0a141e"]},
                {"type": 7, "model": "coin.cub", "colors": ["#123456"]},
                {"type": 7, "model": "coin.cub", "colors": ["red"]},
                {"type": 7, "model": "gone.cub"},
                {"type": 7, "subtype": 1, "amount": 5, "model": "coin.cub"},
                {"type": 7, "amount": 0, "model": "coin.cub"}]}"##,
        );
        let masks: Vec<(Option<u32>, Option<u16>, &[u8])> = glows
            .iter()
            .map(|g| (g.subtype, g.amount, &g.mask[..]))
            .collect();
        // cells 0 and 1 are the first colour, cell 5 (x 1, z 2) the second
        assert_eq!(
            masks,
            [
                (Some(2), None, &[0b10_0000, 0, 0, 0, 0, 0, 0, 0][..]),
                (
                    Some(ground_models::SUBTYPE | 5),
                    Some(5),
                    &[0b11, 0, 0, 0, 0, 0, 0, 0][..]
                ),
                (None, None, &[0b10_0011, 0, 0, 0, 0, 0, 0, 0][..]),
            ]
        );
        assert!(
            glows
                .iter()
                .all(|g| g.item_type == 43 && g.size == [2, 1, 3])
        );
        let reasons: Vec<&str> = mods[0]
            .warnings
            .iter()
            .map(|notice| notice.details[1].1.as_str())
            .collect();
        assert_eq!(
            reasons,
            [
                "no voxel has the colour #123456",
                r##"colors must be a list like ["#4fe74d"]"##,
                "model 'gone.cub' is missing or outside the mod folder",
                "amount is 1 to 65535 and cannot be combined with subtype",
                "amount is 1 to 65535 and cannot be combined with subtype",
            ]
        );
        assert_eq!(
            report.lines[0].text,
            "Glow: item type 43 (m): 3 voxel(s) of a 2x1x3 model, chosen by whole model"
        );
        assert_eq!(
            report.lines[2].text,
            "Glow: item type 43 (5 or more) (m): 2 voxel(s) of a 2x1x3 model, chosen by 1 colour(s)"
        );
    }

    #[test]
    fn the_emit_material_of_a_vox_chooses_the_voxels() {
        // palette index 2 gets the material: MATL { id 2, { "_type": "_emit" } }
        let mut vox = sample().to_vox().unwrap();
        let entry = |text: &[u8]| [&(text.len() as u32).to_le_bytes()[..], text].concat();
        let body = [
            &2u32.to_le_bytes()[..],
            &1u32.to_le_bytes(),
            &entry(b"_type"),
            &entry(b"_emit"),
        ]
        .concat();
        vox.extend_from_slice(b"MATL");
        vox.extend_from_slice(&(body.len() as u32).to_le_bytes());
        vox.extend_from_slice(&[0; 4]);
        vox.extend_from_slice(&body);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lit.vox");
        fs::write(&path, vox).unwrap();
        let (_, cells, how) = lit(&path, None).unwrap();
        assert_eq!((cells, how.as_str()), (vec![[1, 0, 2]], "Emit material"));
    }
}
