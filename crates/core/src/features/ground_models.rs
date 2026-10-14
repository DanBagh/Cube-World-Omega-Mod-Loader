//! Models by amount: the `"groundModels"` manifest section.
//!
//! A gold coin on the ground is the same item whether it is worth 1 or 500. Only the
//! amount behind it in the ground record differs, so every coin looks alike. The client
//! keeps one model per distinct item, so where it fetches the model of an item on the
//! ground it is handed a copy of the item with a subtype no real item has, standing for
//! "this many or more". `item_models` loads the entry's file for that subtype, and the
//! in-game loader serves the file under a name of its own.
//!
//! Each model glows by itself, as its entry's `"glow"` says (see `glow`): `true` for the
//! whole model, a list of colours, or with none the voxels of its `.vox` that have the
//! Emit material. It does not take the glow of the item's own model.
//!
//! Build-specific: the call is found from the code that compares ground records. The two
//! clients keep the record in different registers, so the signatures name none.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::ModuleBuild;
use super::files::{content_hash, usable_model};
use super::glow::{self, Glow};
use super::stub::{ANY, CLIENT, RETURN, Stub, i32_at, signature};
use crate::mods::{Mod, Notice};
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::refs::{SiteKind, string_sites};
use crate::report::{Report, one_line};

/// Or'ed with an amount: the subtype that stands for "this many or more".
pub const SUBTYPE: u32 = 0x4357_0000;
/// Where a ground record keeps the amount, counted from its item.
const AMOUNT: i32 = 0x2C - 0x0C;

// cmp dword [REC+0Ch],0 / je: a ground record with an item in it. REC is the register
// holding the record, rbx in the avx2 client and r13 in the baseline one
const RECORD: &str = r"(\x41?)\x83([\x78-\x7b\x7d-\x7f])\x0c\x00\x0f\x84....";
// movzx eax,word [REC+2Ch] / cmp ax,[<copy>+2Ch]: its amount against the record's last copy
const FIELD: &str =
    r"(\x41?)\x0f\xb7([\x40-\x43\x45-\x47])\x2c\x66\x41?\x3b[\x40-\x43\x45-\x47]\x2c";
/// How far in front of the record check the comparison of the amount is looked for.
const BEFORE: usize = 0x90;
/// How far behind it the item is taken and its model fetched.
const AFTER: usize = 64;
// asm/ground_model.asm
pub(super) const STUB: &str = "4883ec40410f1000418b40100f11442420894424304c8d1500000000418b0283f8ff74243b4424207518410f\
b78044444444413b4204720a418b420889442424eb064983c20cebd44c8d442420e8cccccccc4883c440e9dd\
dddddd";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundModel {
    pub item_type: u32,
    /// The model is for this many or more.
    pub amount: u16,
    /// The `.cub` to serve.
    pub file: PathBuf,
    /// The game path it is served under.
    pub name: String,
    pub owner: String,
    /// Its glowing voxels, none for a model that does not glow.
    pub glow: Glow,
}

impl GroundModel {
    pub fn subtype(&self) -> u32 {
        SUBTYPE | u32::from(self.amount)
    }
}

fn parse(found: &Mod, patch: &Value) -> Result<(u32, u16, String, PathBuf), String> {
    let patch = patch.as_object().ok_or("an entry is an object")?;
    let number = |key: &str| patch.get(key).and_then(Value::as_u64);
    let item_type = number("type").and_then(|n| u32::try_from(n).ok());
    let amount = number("amount").and_then(|n| u16::try_from(n).ok());
    let (Some(item_type), Some(amount)) = (
        item_type.filter(|&n| n != u32::MAX),
        amount.filter(|&n| n > 0),
    ) else {
        return Err("type out of range, or amount not 1 to 65535".into());
    };
    let relative = patch
        .get("model")
        .and_then(Value::as_str)
        .ok_or("model is missing")?;
    let path = found
        .file(relative)
        .ok_or_else(|| format!("model '{relative}' is missing or outside the mod folder"))?;
    Ok((item_type, amount, relative.to_owned(), path))
}

/// One model per item type and amount, a later mod replacing an earlier one. The highest
/// amount of a type comes first: the trampoline takes the first entry the amount reaches.
pub fn collect(
    mods: &mut [Mod],
    order: &[usize],
    converted: &Path,
    report: &mut Report,
) -> Vec<GroundModel> {
    let mut chosen: Vec<GroundModel> = Vec::new();
    for &index in order {
        for patch in mods[index].list("groundModels").to_vec() {
            let found = &mut mods[index];
            let ignored = |reason: String| {
                Notice::new("Ground model ignored")
                    .with("Entry", one_line(&patch.to_string(), 70))
                    .with("Reason", reason)
            };
            let (item_type, amount, relative, path) = match parse(found, &patch) {
                Ok(entry) => entry,
                Err(reason) => {
                    found.warnings.push(ignored(reason));
                    continue;
                }
            };
            let id = found.id.clone();
            let mut warn = |text: String| report.warn(format!("{id}: \"{relative}\": {text}"));
            let served = usable_model(&path, converted, &mut warn)
                .and_then(|file| Ok((fs::read(&file).map_err(|e| e.to_string())?, file)));
            let (data, file) = match served {
                Ok(served) => served,
                Err(reason) => {
                    report.warn(format!(
                        "{id}: \"{relative}\" is not a usable model ({reason}); skipped"
                    ));
                    continue;
                }
            };
            let entry = patch
                .as_object()
                .expect("parse took the entry as an object");
            let (glow, count, how) =
                match glow::of_ground_model(entry, &path, (item_type, amount), &id) {
                    Ok(lit) => lit,
                    Err(reason) => {
                        found.warnings.push(ignored(reason));
                        continue;
                    }
                };
            if count > 0 {
                let [x, y, z] = glow.size;
                report.say(format!("Glow: item type {item_type} ({amount} or more) ({id}): {count} voxel(s) of a {x}x{y}x{z} model, chosen by {how}"));
            }
            let model = GroundModel {
                item_type,
                amount,
                file,
                name: format!("sprites/cwo-ground-{}.cub", content_hash(&data)),
                owner: id.clone(),
                glow,
            };
            match chosen
                .iter_mut()
                .find(|m| (m.item_type, m.amount) == (item_type, amount))
            {
                Some(earlier) => {
                    if earlier.owner != id {
                        report.warn(format!("ground model conflict for {amount} or more of item type {item_type}: {} and {id}; {id} wins", earlier.owner));
                    }
                    *earlier = model;
                }
                None => chosen.push(model),
            }
        }
    }
    chosen.sort_by_key(|m| (m.item_type, std::cmp::Reverse(m.amount)));
    chosen
}

struct Hook {
    site: u32,
    expect: Vec<u8>,
    /// model_of_item, which the replaced call goes to.
    lookup: u32,
}

fn find_hook(module: &ModuleBuild) -> Option<Hook> {
    let (text_va, text) = module.image.data(".text")?;
    // model_of_item is the function that names the model files
    let named = string_sites(module.image, module.refs, b"sprites/gold-coin.cub")
        .values()
        .flatten()
        .find(|s| s.kind == SiteKind::Lea)?
        .rva;
    let lookup = module.image.function_start(named)?;
    // a register from an optional REX.B prefix and the low bits of a ModRM byte
    let register = |rex: &[u8], modrm: &[u8]| (modrm[0] & 7) | (u8::from(!rex.is_empty()) << 3);
    let (record, field) = (signature(RECORD), signature(FIELD));
    let mut found = record.captures_iter(text).filter_map(|hit| {
        let (start, end) = (hit.get(0)?.start(), hit.get(0)?.end());
        let rec = register(&hit[1], &hit[2]);
        // the amount of this same record was just compared
        field
            .captures_iter(&text[start.saturating_sub(BEFORE)..start])
            .find(|amount| register(&amount[1], &amount[2]) == rec)?;
        let after = &text[end..text.len().min(end + AFTER)];
        // lea <reg>,[REC+0Ch]: the item in the record, which is what the call is given
        let item = after.windows(4).position(|code| {
            code[0] & 0xF9 == 0x48 | (rec >> 3)
                && code[1] == 0x8D
                && code[2] & 0xC7 == 0x40 | (rec & 7)
                && code[3] == 0x0C
        })?;
        let call = (item + 4..after.len().saturating_sub(4)).find(|&at| {
            let next = text_va + (end + at) as u32 + 5;
            after[at] == 0xE8 && next.wrapping_add_signed(i32_at(after, at + 1)) == lookup
        })?;
        Some(Hook {
            site: text_va + (end + call) as u32,
            expect: after[call..call + 5].to_vec(),
            lookup,
        })
    });
    let hook = found.next()?;
    found.next().is_none().then_some(hook)
}

/// Plans all of the models for one client DLL, or none. Returns how many that was.
///
/// The item-model trampoline has to be in the plan already: without it a made-up subtype
/// would have no model at all.
pub fn plan(module: &mut ModuleBuild, models: &[GroundModel], report: &mut Report) -> usize {
    if models.is_empty() {
        return 0;
    }
    let item_models = module
        .plan
        .groups
        .iter()
        .any(|g| g.id == super::item_models::GROUP);
    let Some(hook) = find_hook(module).filter(|_| item_models) else {
        report.warn(format!("{}: the ground-item code has another shape in this build; models by amount skipped (every amount keeps the normal model)", module.backend.key()));
        return 0;
    };
    let mut stub = Stub::new(STUB);
    stub.fill(0x44, AMOUNT);
    stub.lea([0x4C, 0x8D, 0x15], 0, stub.size()); // the table follows the code
    let (lookup, resume) = (stub.at(CLIENT), stub.at(RETURN));
    let mut table = Vec::new();
    for model in models {
        table.extend_from_slice(&model.item_type.to_le_bytes());
        table.extend_from_slice(&u32::from(model.amount).to_le_bytes());
        table.extend_from_slice(&model.subtype().to_le_bytes());
    }
    table.extend_from_slice(&ANY.to_le_bytes());
    let start = module.plan.reserve();
    let blob = &mut module.plan.blob;
    blob.extend(stub.into_bytes());
    blob.extend(table);
    blob.resize(blob.len() + BLOB_PAD, 0);

    let end = hook.site + hook.expect.len() as u32;
    module.plan.claims.push(Claim {
        start: hook.site,
        end,
        owner: "models by amount".into(),
    });
    let mut who: Vec<&str> = Vec::new();
    for model in models {
        if !who.contains(&model.owner.as_str()) {
            who.push(&model.owner);
        }
    }
    module.plan.groups.push(Group {
        id: "ground-models".into(),
        label: format!("{} model(s) by amount [{}]", models.len(), who.join("+")),
        ops: vec![
            Op::BlobRel32 {
                blob_offset: start + lookup,
                target: hook.lookup,
            },
            Op::BlobRel32 {
                blob_offset: start + resume,
                target: end,
            },
            Op::Hook {
                rva: hook.site,
                expect: hook.expect,
                blob_offset: start,
            },
        ],
    });
    models.len()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::game::Backend;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};
    use crate::plan::ModulePlan;
    use crate::refs::RefIndex;
    use crate::testing::TinyPe;
    use crate::voxel::tests::sample;

    pub(crate) fn model(item_type: u32, amount: u16, owner: &str) -> GroundModel {
        GroundModel {
            item_type,
            amount,
            file: PathBuf::from("model.cub"),
            name: format!("sprites/cwo-ground-{amount:016x}.cub"),
            owner: owner.into(),
            glow: Glow {
                item_type,
                subtype: Some(SUBTYPE | u32::from(amount)),
                amount: Some(amount),
                size: [14, 6, 24],
                mask: Vec::new(),
                owner: owner.into(),
            },
        }
    }

    #[test]
    fn each_model_glows_by_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let folder = write_mod(
            dir.path(),
            "m",
            r##"{"schema": 1, "id": "m", "version": "1.0.0", "groundModels": [
                {"type": 43, "amount": 5, "model": "lit.vox"},
                {"type": 43, "amount": 20, "model": "coin.cub", "glow": ["#0a141e"]},
                {"type": 43, "amount": 50, "model": "lit.vox", "glow": true},
                {"type": 43, "amount": 100, "model": "coin.cub"},
                {"type": 43, "amount": 300, "model": "coin.cub", "glow": ["#123456"]},
                {"type": 43, "amount": 400, "model": "coin.cub", "glow": "yes"},
                {"type": 43, "amount": 500, "model": "lit.vox", "glow": false}]}"##,
        );
        fs::write(folder.join("coin.cub"), sample().to_cub()).unwrap();
        fs::write(
            folder.join("lit.vox"),
            crate::features::glow::tests::emitting_vox(),
        )
        .unwrap();
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let mut report = Report::default();
        let models = collect(
            &mut mods,
            &order,
            &dir.path().join("converted"),
            &mut report,
        );

        let masks: Vec<(u16, u32, &[u8])> = models
            .iter()
            .map(|m| (m.amount, m.glow.subtype.unwrap(), &m.glow.mask[..]))
            .collect();
        // the sample: cells 0 and 1 are one colour, cell 5 (x 1, z 2) another, with Emit
        assert_eq!(
            masks,
            [
                (500, SUBTYPE | 500, &[][..]),
                (100, SUBTYPE | 100, &[][..]),
                (50, SUBTYPE | 50, &[0b10_0011, 0, 0, 0, 0, 0, 0, 0][..]),
                (20, SUBTYPE | 20, &[0b11, 0, 0, 0, 0, 0, 0, 0][..]),
                (5, SUBTYPE | 5, &[0b10_0000, 0, 0, 0, 0, 0, 0, 0][..]),
            ]
        );
        assert_eq!(crate::features::glow::count(&mods[0]), 3);
        let reasons: Vec<&str> = mods[0]
            .warnings
            .iter()
            .map(|notice| notice.details[1].1.as_str())
            .collect();
        assert_eq!(
            reasons,
            [
                "no voxel has the colour #123456",
                r##"glow must be true or a list like ["#4fe74d"]"##
            ]
        );
    }

    #[test]
    fn the_highest_amount_of_an_item_comes_first() {
        let dir = tempfile::tempdir().unwrap();
        for (folder, manifest) in [
            (
                "a",
                r#"{"schema": 1, "id": "a", "version": "1.0.0", "groundModels": [
                    {"type": 43, "amount": 5, "model": "blue.vox"},
                    {"type": 43, "amount": 20, "model": "red.cub"},
                    {"type": 43, "amount": 0, "model": "red.cub"},
                    {"type": 43, "amount": 7, "model": "gone.cub"},
                    {"type": 43, "amount": 8, "model": "broken.cub"}]}"#,
            ),
            (
                "b",
                r#"{"schema": 1, "id": "b", "version": "1.0.0", "groundModels": [
                    {"type": 43, "amount": 5, "model": "red.cub"},
                    {"type": 7, "amount": 2, "model": "red.cub"}]}"#,
            ),
        ] {
            let folder = write_mod(dir.path(), folder, manifest);
            fs::write(folder.join("red.cub"), sample().to_cub()).unwrap();
            fs::write(folder.join("blue.vox"), sample().to_vox().unwrap()).unwrap();
            fs::write(folder.join("broken.cub"), b"nope").unwrap();
        }
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let converted = dir.path().join("converted");
        let mut report = Report::default();
        let models = collect(&mut mods, &order, &converted, &mut report);

        let brief: Vec<(u32, u16, &str)> = models
            .iter()
            .map(|m| (m.item_type, m.amount, m.owner.as_str()))
            .collect();
        assert_eq!(brief, [(7, 2, "b"), (43, 20, "a"), (43, 5, "b")]);
        // the same model, whether it came as a .cub or was converted from a .vox
        assert!(models.iter().all(|m| m.name == models[0].name));
        assert!(models[0].name.starts_with("sprites/cwo-ground-") && models[0].name.len() == 39);
        assert_eq!(models[1].subtype(), 0x4357_0014);
        assert_eq!(mods[0].warnings.len(), 2);
        let warnings: Vec<&str> = report.warnings().collect();
        assert_eq!(warnings.len(), 2);
        assert!(warnings[0].starts_with("a: \"broken.cub\" is not a usable model"));
        assert_eq!(
            warnings[1],
            "ground model conflict for 5 or more of item type 43: a and b; b wins"
        );
    }

    /// Adds the client code that fetches the model of an item on the ground, the way
    /// the avx2 client or the baseline one has it. Returns the call and the function it
    /// goes to.
    fn ground_code(pe: &mut TinyPe, backend: Backend) -> (u32, u32) {
        let name = pe.rdata(b"sprites/gold-coin.cub\0");
        let lookup = pe.code(&[0x90; 8]);
        pe.lea(name);
        let end = pe.code(&[0xC3]);
        pe.function(lookup, end + 1);
        // a check of some other record, with no amount compared in front of it
        pe.code(&[0x83, 0x7B, 0x0C, 0x00, 0x0F, 0x84, 0, 1, 0, 0]);
        pe.code(&[0x90; 0xA0]);
        if backend == Backend::Avx2 {
            // the record is in rbx, its last copy in r9
            pe.code(&[0x0F, 0xB7, 0x43, 0x2C, 0x66, 0x41, 0x3B, 0x41, 0x2C]);
            pe.code(&[0x75, 0x14, 0x90, 0x90]);
            pe.code(&[0x83, 0x7B, 0x0C, 0x00, 0x0F, 0x84, 0, 1, 0, 0]);
            pe.code(&[
                0x4C, 0x89, 0x8D, 0, 1, 0, 0, 0x4C, 0x89, 0x85, 0xC0, 1, 0, 0,
            ]);
            // mov r15,rsi / lea rsi,[rbx+0Ch] / ... / mov r8,rsi / vzeroupper
            pe.code(&[0x49, 0x89, 0xF7, 0x48, 0x8D, 0x73, 0x0C]);
            pe.code(&[0x4C, 0x89, 0xF9, 0x48, 0x89, 0xFA, 0x49, 0x89, 0xF0]);
            pe.code(&[0xC5, 0xF8, 0x77]);
        } else {
            // the record is in r13, its last copy in rdi
            pe.code(&[0x41, 0x0F, 0xB7, 0x45, 0x2C, 0x66, 0x3B, 0x47, 0x2C]);
            pe.code(&[0x75, 0x12, 0x90, 0x90]);
            pe.code(&[0x41, 0x83, 0x7D, 0x0C, 0x00, 0x0F, 0x84, 0, 1, 0, 0]);
            // lea r8,[r13+0Ch] / mov rcx,rsi / mov rdx,r15 / mov [rbp+188h],r8
            pe.code(&[0x4D, 0x8D, 0x45, 0x0C, 0x48, 0x89, 0xF1, 0x4C, 0x89, 0xFA]);
            pe.code(&[0x4C, 0x89, 0x85, 0x88, 1, 0, 0]);
        }
        let call = pe.code(&[0xE8, 0, 0, 0, 0]);
        let relative = lookup.wrapping_sub(call + 5).to_le_bytes();
        pe.patch(call + 1, &relative);
        pe.code(&[0x90; 16]);
        (call, lookup)
    }

    #[test]
    fn finds_the_lookup_in_both_clients() {
        for backend in Backend::ALL {
            let mut pe = TinyPe::new();
            let (call, lookup) = ground_code(&mut pe, backend);
            let image = pe.build();
            let refs = RefIndex::build(&image).unwrap();
            let module = ModuleBuild {
                backend,
                image: &image,
                refs: &refs,
                plan: ModulePlan::new(backend, image.timestamp, image.size_of_image),
            };
            let hook = find_hook(&module).unwrap();
            assert_eq!((hook.site, hook.lookup), (call, lookup));
            assert_eq!(hook.expect.len(), 5);
        }
    }

    #[test]
    fn hooks_the_lookup_of_a_ground_items_model() {
        let mut pe = TinyPe::new();
        let (call, lookup) = ground_code(&mut pe, Backend::Avx2);
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = ModuleBuild {
            backend: Backend::Avx2,
            image: &image,
            refs: &refs,
            plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
        };
        let models = [model(43, 20, "rupees"), model(43, 5, "rupees")];
        let mut report = Report::default();

        // not without the trampoline that knows the subtypes
        assert_eq!(plan(&mut module, &models, &mut report), 0);
        assert!(
            report
                .warnings()
                .next()
                .unwrap()
                .contains("models by amount skipped")
        );

        module.plan.groups.push(Group {
            id: super::super::item_models::GROUP.into(),
            label: String::new(),
            ops: Vec::new(),
        });
        assert_eq!(plan(&mut module, &models, &mut report), 2);
        let plan = &module.plan;
        let group = &plan.groups[1];
        assert_eq!(
            (group.id.as_str(), group.label.as_str()),
            ("ground-models", "2 model(s) by amount [rupees]")
        );
        let Op::Hook {
            rva,
            expect,
            blob_offset: start,
        } = &group.ops[2]
        else {
            panic!("the hook comes last")
        };
        assert_eq!((*rva, expect.len()), (call, 5));
        let stub = Stub::new(STUB);
        assert_eq!(
            group.ops[..2],
            [
                Op::BlobRel32 {
                    blob_offset: start + stub.at(CLIENT),
                    target: lookup
                },
                Op::BlobRel32 {
                    blob_offset: start + stub.at(RETURN),
                    target: call + 5
                },
            ]
        );
        let code = *start as usize;
        assert_eq!(i32_at(&plan.blob, code + stub.at(0x44) as usize), 0x20);
        let table = code + stub.size();
        let row = |at: usize| [0, 4, 8].map(|field| i32_at(&plan.blob, table + at + field) as u32);
        assert_eq!(row(0), [43, 20, 0x4357_0014]);
        assert_eq!(row(12), [43, 5, 0x4357_0005]);
        assert_eq!(i32_at(&plan.blob, table + 24), -1);
    }
}
