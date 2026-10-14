//! The item-model trampoline, shared by `glow` and `ground_models`.
//!
//! The client loads the model of an ordinary item in a background job. That load is
//! diverted to do two things: open another file for the made-up subtypes that stand for
//! an amount on the ground, and set the "glows" bits of a model once it is loaded.
//!
//! Build-specific: the job is found by its instructions, and where a model slot keeps its
//! size and its voxel bits is read from the client's `.cub` loader.

use super::ModuleBuild;
use super::glow::Glow;
use super::ground_models::GroundModel;
use super::pickup_sounds::DATA_SIZE;
use super::stub::{ANY, CLIENT, DATA, RETURN, Stub, i32_at, only, signature};
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::report::Report;

/// Id of the plan group that installs the trampoline.
pub const GROUP: &str = "glow";

// In the .cub loader: lea rcx,[rsi+<bits>] / mov r8d,4000h / xor edx,edx / call memset,
// then mov eax,<colours + 2> to walk the colours. The colours follow the three sizes
const LOADER: &str =
    r"\x48\x8d\x8e(....)\x41\xb8\x00\x40\x00\x00\x31\xd2\xe8....\xb8([\x0e-\x7f])\x00\x00\x00";
const SIZES: i32 = 12;
// add rdi,10h / mov [rsp+N],rdi / mov [rsp+M],rax / lea rdx,[rsp+N] / mov rcx,rsi, then
// the replaced xor r8d,r8d / call load_cub / jmp finished
const SITE: &str = r"\x48\x83\xc7\x10\x48\x89\x7c\x24(.)\x48\x89\x44\x24.\x48\x8d\x54\x24(.)\x48\x89\xf1(\x45\x31\xc0\xe8(....)\xe9(....))";
// mov ecx,[rdi+<item type>] / cmp ecx,imm32, earlier in the job, before rdi is advanced
const ITEM: &str = r"\x8b\x8f(....)\x81\xf9";
// mov rcx,rsi / mov dl,1 / call build_mesh, right in front of `finished`
const TAIL: [u8; 6] = [0x48, 0x89, 0xF1, 0xB2, 0x01, 0xE8];
// asm/item_model.asm
pub(super) const STUB: &str = "4c8d0d00000000418b0183f8ff74263b87222222227518418b41043b8733333333750c496341084903c14889\
02eb064983c10cebd241b801000000e8cccccccc4c8d15eeeeeeeef041ff82a00000004c8d0d00000000418b\
0183f8ff74753b87222222227567418b410483f8ff74083b873333333375564c8d9e66666666418b4108413b\
037546418b410c413b4304753c418b4110413b43087532418b4914496351184903d14c8d865555555585c974\
12488b024909004883c2084983c008ffc9ebeaf041ff82a4000000eb064983c11ceb83e9dddddddd";

struct Hook {
    site: u32,
    expect: Vec<u8>,
    /// The client's .cub loader.
    loader: u32,
    /// The mesh call the special items end with.
    tail: u32,
    /// Where the job keeps the item type, from rdi as it is at the hook.
    item: i32,
    /// Where a model slot keeps its size.
    size: i32,
    /// And its voxel bits.
    bits: i32,
}

fn find_hook(module: &ModuleBuild) -> Option<Hook> {
    let image = module.image;
    let (text_va, text) = image.data(".text")?;
    let loader = only(LOADER, text)?;
    let site = signature(SITE);
    // both moves use the same stack slot
    let mut sites = site.captures_iter(text).filter(|hit| hit[1] == hit[2]);
    let hit = sites.next()?;
    if sites.next().is_some() {
        return None;
    }
    let replaced = hit.get(3)?;
    let at = text_va + replaced.start() as u32;
    let target = (at + 8).wrapping_add_signed(i32_at(&hit[4], 0));
    let finished = (at + 13).wrapping_add_signed(i32_at(&hit[5], 0));
    if image.function_start(text_va + loader.get(0)?.start() as u32) != Some(target) {
        return None;
    }
    let tail = finished.checked_sub(10)?;
    if image.read(tail, TAIL.len())? != TAIL {
        return None;
    }
    let job = image.function_start(text_va + hit.get(0)?.start() as u32)?;
    let before = text.get((job.checked_sub(text_va)?) as usize..hit.get(0)?.start())?;
    let item = signature(ITEM).captures(before)?;
    Some(Hook {
        site: at,
        expect: replaced.as_bytes().to_vec(),
        loader: target,
        tail,
        item: i32_at(&item[1], 0) - 0x10,
        size: i32::from(loader[2][0]) - 2 - SIZES,
        bits: i32_at(&loader[1], 0),
    })
}

/// Installs the trampoline for one client DLL with both tables. False if there was
/// nothing to install or the build has no place for it.
pub fn plan(
    module: &mut ModuleBuild,
    glows: &[Glow],
    models: &[GroundModel],
    report: &mut Report,
) -> bool {
    if glows.is_empty() && models.is_empty() {
        return false;
    }
    let Some(hook) = find_hook(module) else {
        report.warn(format!("{}: could not locate the item model loader in this build; glowing voxels and models by amount skipped", module.backend.key()));
        return false;
    };
    let mut stub = Stub::new(STUB);
    stub.fill(0x22, hook.item);
    stub.fill(0x33, hook.item + 4);
    stub.fill(0x55, hook.bits);
    stub.fill(0x66, hook.size);
    // after the code: the model table, the glow table, the model paths, the masks
    let model_table_at = stub.size();
    let glow_table_at = model_table_at + 12 * models.len() + 4;
    let paths_at = glow_table_at + 28 * glows.len() + 4;
    let (mut model_table, mut paths) = (Vec::new(), Vec::new());
    for (n, model) in models.iter().enumerate() {
        let relative = (paths_at + paths.len()) as i32 - (model_table_at + 12 * n) as i32;
        model_table.extend_from_slice(&model.item_type.to_le_bytes());
        model_table.extend_from_slice(&model.subtype().to_le_bytes());
        model_table.extend_from_slice(&relative.to_le_bytes());
        paths.extend_from_slice(model.name.as_bytes());
        paths.push(0);
    }
    model_table.extend_from_slice(&ANY.to_le_bytes());
    let masks_at = paths_at + paths.len();
    let (mut glow_table, mut masks) = (Vec::new(), Vec::new());
    for (n, glow) in glows.iter().enumerate() {
        let relative = (masks_at + masks.len()) as i32 - (glow_table_at + 28 * n) as i32;
        let words = [
            glow.item_type,
            glow.subtype.unwrap_or(ANY),
            glow.size[0],
            glow.size[1],
            glow.size[2],
            (glow.mask.len() / 8) as u32,
            relative as u32,
        ];
        glow_table.extend(words.iter().flat_map(|word| word.to_le_bytes()));
        masks.extend_from_slice(&glow.mask);
    }
    glow_table.extend_from_slice(&ANY.to_le_bytes());
    stub.lea([0x4C, 0x8D, 0x0D], 0, model_table_at);
    stub.lea([0x4C, 0x8D, 0x0D], 1, glow_table_at);
    let (loader, data, tail) = (stub.at(CLIENT), stub.at(DATA), stub.at(RETURN));

    let start = module.plan.reserve();
    let blob = &mut module.plan.blob;
    blob.extend(stub.into_bytes());
    for part in [model_table, glow_table, paths, masks] {
        blob.extend(part);
    }
    blob.resize(blob.len() + BLOB_PAD, 0);
    module.plan.data_size = module.plan.data_size.max(DATA_SIZE);
    module.plan.data_ref(start + data, 0);

    module.plan.claims.push(Claim {
        start: hook.site,
        end: hook.site + hook.expect.len() as u32,
        owner: "item models".into(),
    });
    let mut who: Vec<&str> = Vec::new();
    for owner in glows
        .iter()
        .map(|g| g.owner.as_str())
        .chain(models.iter().map(|m| m.owner.as_str()))
    {
        if !who.contains(&owner) {
            who.push(owner);
        }
    }
    module.plan.groups.push(Group {
        id: GROUP.into(),
        label: format!(
            "{} glowing model(s), {} model file(s) by amount [{}]",
            glows.len(),
            models.len(),
            who.join("+")
        ),
        ops: vec![
            Op::BlobRel32 {
                blob_offset: start + loader,
                target: hook.loader,
            },
            Op::BlobRel32 {
                blob_offset: start + tail,
                target: hook.tail,
            },
            Op::Hook {
                rva: hook.site,
                expect: hook.expect,
                blob_offset: start,
            },
        ],
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::ground_models::tests::model;
    use crate::game::Backend;
    use crate::plan::ModulePlan;
    use crate::refs::RefIndex;
    use crate::testing::TinyPe;

    struct Client {
        pe: TinyPe,
        site: u32,
        loader: u32,
        tail: u32,
    }

    /// A client with the .cub loader and the item job. `colours` is where a model slot
    /// keeps its colours and `slot` the stack slot the job uses, both of which moved
    /// between releases.
    fn client(colours: u8, slot: u8) -> Client {
        let mut pe = TinyPe::new();
        let loader = pe.code(&[0x55, 0x56, 0x57, 0x90]);
        pe.code(&[0x48, 0x8D, 0x8E, 0x10, 0x00, 0x06, 0x00]);
        pe.code(&[0x41, 0xB8, 0x00, 0x40, 0x00, 0x00, 0x31, 0xD2]);
        pe.code(&[0xE8, 0, 0, 0, 0, 0xB8, colours + 2, 0, 0, 0]);
        let end = pe.code(&[0xC3]);
        pe.function(loader, end + 1);

        let job = pe.code(&[0x48, 0x89, 0xCF]);
        pe.code(&[0x8B, 0x8F, 0xAC, 0, 0, 0, 0x81, 0xF9, 0x82, 0, 0, 0]);
        pe.code(&[0x90; 8]);
        pe.code(&[0x48, 0x83, 0xC7, 0x10, 0x48, 0x89, 0x7C, 0x24, slot]);
        pe.code(&[
            0x48,
            0x89,
            0x44,
            0x24,
            slot + 8,
            0x48,
            0x8D,
            0x54,
            0x24,
            slot,
        ]);
        pe.code(&[0x48, 0x89, 0xF1]);
        let site = pe.code(&[0x45, 0x31, 0xC0]);
        let call = pe.code(&[0xE8, 0, 0, 0, 0]);
        let jump = pe.code(&[0xE9, 0, 0, 0, 0]);
        pe.code(&[0x90; 24]);
        let tail = pe.code(&TAIL);
        pe.code(&[0, 0, 0, 0]);
        let finished = pe.code(&[0xC3]);
        pe.function(job, finished + 1);
        pe.patch(call + 1, &loader.wrapping_sub(call + 5).to_le_bytes());
        pe.patch(jump + 1, &finished.wrapping_sub(jump + 5).to_le_bytes());
        Client {
            pe,
            site,
            loader,
            tail,
        }
    }

    fn glow(item_type: u32, subtype: Option<u32>, mask: &[u8]) -> Glow {
        Glow {
            item_type,
            subtype,
            amount: None,
            size: [14, 6, 24],
            mask: mask.to_vec(),
            owner: "rupees".into(),
        }
    }

    #[test]
    fn installs_both_tables_and_reads_the_slot_layout_from_the_loader() {
        // the size is at +4 since release gh-38293344120, and was at +0 before
        for (colours, slot, size) in [(0x10, 0x48, 4), (0x0C, 0x38, 0)] {
            let client = client(colours, slot);
            let image = client.pe.build();
            let refs = RefIndex::build(&image).unwrap();
            let mut module = ModuleBuild {
                backend: Backend::Avx2,
                image: &image,
                refs: &refs,
                plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
            };
            let glows = [glow(43, None, &[0xAA; 16])];
            let models = [model(43, 20, "other")];
            let mut report = Report::default();
            assert!(plan(&mut module, &glows, &models, &mut report));

            let plan = &module.plan;
            let group = &plan.groups[0];
            assert_eq!(group.id, GROUP);
            assert_eq!(
                group.label,
                "1 glowing model(s), 1 model file(s) by amount [rupees+other]"
            );
            let Op::Hook {
                rva,
                expect,
                blob_offset: start,
            } = &group.ops[2]
            else {
                panic!("the hook comes last")
            };
            assert_eq!((*rva, expect.len()), (client.site, 13));
            let stub = Stub::new(STUB);
            assert_eq!(
                group.ops[..2],
                [
                    Op::BlobRel32 {
                        blob_offset: start + stub.at(CLIENT),
                        target: client.loader
                    },
                    Op::BlobRel32 {
                        blob_offset: start + stub.at(RETURN),
                        target: client.tail
                    },
                ]
            );
            assert_eq!(plan.data_size, DATA_SIZE);

            let blob = &plan.blob;
            let code = *start as usize;
            let filled = |marker: u8| i32_at(blob, code + stub.at(marker) as usize);
            assert_eq!(
                (filled(0x22), filled(0x33), filled(0x55), filled(0x66)),
                (0x9C, 0xA0, 0x60010, size)
            );
            // the model table: the subtype for 20 or more, and its file
            let models_at = code + stub.size();
            assert_eq!(
                (i32_at(blob, models_at), i32_at(blob, models_at + 4)),
                (43, 0x4357_0014)
            );
            let path = (models_at as i32 + i32_at(blob, models_at + 8)) as usize;
            assert!(blob[path..].starts_with(b"sprites/cwo-ground-0000000000000014.cub\0"));
            // the glow table: any subtype, the size, two quadwords of mask
            let glow_at = models_at + 16;
            let row: Vec<i32> = (0..6).map(|n| i32_at(blob, glow_at + 4 * n)).collect();
            assert_eq!(row, [43, -1, 14, 6, 24, 2]);
            let mask = (glow_at as i32 + i32_at(blob, glow_at + 24)) as usize;
            assert_eq!(blob[mask..mask + 16], [0xAA; 16]);
            assert_eq!(i32_at(blob, glow_at + 28), -1);
        }
    }

    #[test]
    fn a_build_without_the_job_is_skipped_and_reported() {
        let mut pe = TinyPe::new();
        pe.code(&[0x90; 64]);
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = ModuleBuild {
            backend: Backend::Baseline,
            image: &image,
            refs: &refs,
            plan: ModulePlan::new(Backend::Baseline, image.timestamp, image.size_of_image),
        };
        let mut report = Report::default();
        assert!(!plan(
            &mut module,
            &[glow(43, None, &[1; 8])],
            &[],
            &mut report
        ));
        assert!(!plan(&mut module, &[], &[], &mut report));
        assert_eq!(report.warnings().count(), 1);
        assert!(module.plan.groups.is_empty());
    }
}
