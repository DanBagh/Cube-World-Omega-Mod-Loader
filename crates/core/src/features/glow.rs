//! Glowing voxels: the `"glow"` manifest section, and the glow of each ground model.
//!
//! A model slot in the client has one "glows" bit per voxel, and the mesher puts the
//! marked voxels into a second, emissive mesh, drawn in each voxel's own colour. Nothing
//! in a `.cub` can set those bits.
//!
//! Glow belongs to a model. An entry of the `"glow"` section names an item and the model
//! it shows. A `groundModels` entry lights its own model (see `ground_models`). Which
//! voxels glow: the ones of the colours given, else the ones with MagicaVoxel's "Emit"
//! material. With neither, a `"glow"` entry lights the whole model and a ground model
//! none. A ground model can also say `true` (all of it) or `false` (none). `item_models` sets the bits when the game loads a model of that size for the item.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

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
    /// Set for the model a `groundModels` entry gives this amount.
    pub amount: Option<u16>,
    /// Size of the model the entry was made from. It only applies to a model of that size.
    pub size: [u32; 3],
    /// One bit per cell, x running fastest, in whole quadwords. Empty for a ground model
    /// that does not glow, so the entry for every subtype of its item does not light it.
    pub mask: Vec<u8>,
    pub owner: String,
}

/// Which voxels of a model glow.
enum Choice {
    Colours(Vec<Rgb>),
    Whole,
    /// The ones with the Emit material, none if there are none.
    Emit,
    /// The ones with the Emit material, all of them if there are none.
    EmitOrWhole,
    Nothing,
}

/// The model, its glowing cells, and how they were chosen.
fn lit(path: &Path, choice: &Choice) -> Result<(Model, Vec<[u32; 3]>, String), String> {
    let data = fs::read(path).map_err(|e| e.to_string())?;
    let vox = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("vox"));
    let (model, emissive) = if vox {
        Model::from_vox_lit(&data)?
    } else {
        (Model::from_cub(&data)?, Vec::new())
    };
    if model.cells() > MAX_CELLS {
        return Err(format!(
            "the model has more than {MAX_CELLS} cells, which is all the game loads"
        ));
    }
    let whole = |model: Model| {
        let cells = model.voxels.keys().copied().collect();
        Ok((model, cells, "whole model".into()))
    };
    match choice {
        Choice::Colours(colours) => {
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
            Ok((model, cells, format!("{} colour(s)", colours.len())))
        }
        Choice::Whole => whole(model),
        Choice::Nothing => Ok((model, Vec::new(), "nothing".into())),
        Choice::EmitOrWhole if emissive.is_empty() => whole(model),
        Choice::Emit | Choice::EmitOrWhole => Ok((model, emissive, "Emit material".into())),
    }
}

/// Colours as `#rrggbb`.
fn colours(value: &Value) -> Option<Vec<Rgb>> {
    let colour = |value: &Value| {
        let hex = value.as_str()?.trim_start_matches('#');
        let number = u32::from_str_radix(hex, 16)
            .ok()
            .filter(|_| hex.len() == 6)?;
        Some([(number >> 16) as u8, (number >> 8) as u8, number as u8])
    };
    let list = value.as_array().filter(|list| !list.is_empty())?;
    list.iter().map(colour).collect()
}

/// The entry for a model, and how many of its voxels glow.
fn made(
    item_type: u32,
    subtype: Option<u32>,
    amount: Option<u16>,
    model: &Model,
    cells: &[[u32; 3]],
    owner: &str,
) -> (Glow, usize) {
    let [sx, sy, _] = model.size.map(|side| side as usize);
    let mut mask = Vec::new();
    if !cells.is_empty() {
        mask = vec![0u8; (model.cells() as usize).div_ceil(64) * 8];
        for [x, y, z] in cells.iter().map(|cell| cell.map(|at| at as usize)) {
            let bit = x + sx * (y + sy * z);
            mask[bit >> 3] |= 1 << (bit & 7);
        }
    }
    let glow = Glow {
        item_type,
        subtype,
        amount,
        size: model.size,
        mask,
        owner: owner.to_owned(),
    };
    (glow, cells.len())
}

/// The entry, how many voxels glow and how they were chosen.
fn parse(found: &Mod, patch: &Value) -> Result<(Glow, usize, String), String> {
    let patch = patch.as_object().ok_or("an entry is an object")?;
    let (item_type, subtype) = item_key(patch)?;
    if patch.get("amount").is_some_and(|v| !v.is_null()) {
        return Err(
            "amount is not used here: a groundModels entry has a \"glow\" of its own".into(),
        );
    }
    let relative = patch
        .get("model")
        .and_then(Value::as_str)
        .ok_or("model is missing")?;
    let path = found
        .file(relative)
        .ok_or_else(|| format!("model '{relative}' is missing or outside the mod folder"))?;
    let choice = match patch.get("colors") {
        None | Some(Value::Null) => Choice::EmitOrWhole,
        Some(value) => {
            Choice::Colours(colours(value).ok_or(r##"colors must be a list like ["#4fe74d"]"##)?)
        }
    };
    let (model, cells, how) = lit(&path, &choice)?;
    if cells.is_empty() {
        return Err("the model is empty".into());
    }
    let (glow, count) = made(item_type, subtype, None, &model, &cells, &found.id);
    Ok((glow, count, how))
}

/// What a `groundModels` entry's `"glow"` asks for: `true` for the whole model, `false`
/// for none, colours, or with no `"glow"` the voxels with the Emit material.
fn ground_choice(entry: &Map<String, Value>) -> Result<Choice, &'static str> {
    match entry.get("glow") {
        None | Some(Value::Null) => Ok(Choice::Emit),
        Some(Value::Bool(true)) => Ok(Choice::Whole),
        Some(Value::Bool(false)) => Ok(Choice::Nothing),
        Some(value) => colours(value)
            .map(Choice::Colours)
            .ok_or(r##"glow must be true or a list like ["#4fe74d"]"##),
    }
}

/// The entry for the model at `path` that a `groundModels` entry gives `amount` or more of
/// `item_type`, how many voxels glow and how they were chosen.
pub(super) fn of_ground_model(
    entry: &Map<String, Value>,
    path: &Path,
    (item_type, amount): (u32, u16),
    owner: &str,
) -> Result<(Glow, usize, String), String> {
    let choice = ground_choice(entry)?;
    let (model, cells, how) = lit(path, &choice)?;
    let subtype = ground_models::SUBTYPE | u32::from(amount);
    let (glow, count) = made(
        item_type,
        Some(subtype),
        Some(amount),
        &model,
        &cells,
        owner,
    );
    Ok((glow, count, how))
}

/// How many of a mod's models glow: each `"glow"` entry, and each ground model that lights
/// any of its voxels.
pub fn count(found: &Mod) -> usize {
    let lit_ground = found.list("groundModels").iter().filter(|entry| {
        let model = entry.get("model").and_then(Value::as_str);
        match (
            entry.as_object(),
            model.and_then(|relative| found.file(relative)),
        ) {
            (Some(entry), Some(path)) => {
                of_ground_model(entry, &path, (0, 1), "").is_ok_and(|(_, count, _)| count > 0)
            }
            _ => false,
        }
    });
    found.list("glow").len() + lit_ground.count()
}

/// The order the trampoline looks entries up in: an exact subtype before the entry for
/// every subtype of the same item.
pub fn in_lookup_order(glows: &mut [Glow]) {
    glows.sort_by_key(|g| (g.item_type, g.subtype.is_none(), g.subtype.unwrap_or(0)));
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
            let which = glow
                .subtype
                .map(|subtype| format!(" subtype {subtype}"))
                .unwrap_or_default();
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
    in_lookup_order(&mut chosen);
    chosen
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};
    use crate::voxel::tests::sample;

    /// The sample model as a `.vox` whose palette index 2 has the Emit material: its one
    /// voxel at (1, 0, 2) glows.
    pub(crate) fn emitting_vox() -> Vec<u8> {
        // MATL { id 2, { "_type": "_emit" } }
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
        vox
    }

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
        let masks: Vec<(Option<u32>, &[u8])> =
            glows.iter().map(|g| (g.subtype, &g.mask[..])).collect();
        // cells 0 and 1 are the first colour, cell 5 (x 1, z 2) the second
        assert_eq!(
            masks,
            [
                (Some(2), &[0b10_0000, 0, 0, 0, 0, 0, 0, 0][..]),
                (None, &[0b10_0011, 0, 0, 0, 0, 0, 0, 0][..]),
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
                "amount is not used here: a groundModels entry has a \"glow\" of its own",
                "no voxel has the colour #123456",
                r##"colors must be a list like ["#4fe74d"]"##,
                "model 'gone.cub' is missing or outside the mod folder",
                "amount is not used here: a groundModels entry has a \"glow\" of its own",
                "amount is not used here: a groundModels entry has a \"glow\" of its own",
            ]
        );
        assert_eq!(
            report.lines[0].text,
            "Glow: item type 43 (m): 3 voxel(s) of a 2x1x3 model, chosen by whole model"
        );
        assert_eq!(
            report.lines[1].text,
            "Glow: item type 43 subtype 2 (m): 1 voxel(s) of a 2x1x3 model, chosen by 1 colour(s)"
        );
    }

    #[test]
    fn the_emit_material_of_a_vox_chooses_the_voxels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lit.vox");
        fs::write(&path, emitting_vox()).unwrap();
        for choice in [Choice::Emit, Choice::EmitOrWhole] {
            let (_, cells, how) = lit(&path, &choice).unwrap();
            assert_eq!((cells, how.as_str()), (vec![[1, 0, 2]], "Emit material"));
        }
        let (_, cells, _) = lit(&path, &Choice::Whole).unwrap();
        assert_eq!(cells.len(), 3);
    }
}
