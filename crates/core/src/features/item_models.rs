//! The item-model trampoline, for `ground_models`.
//!
//! The client loads the model of an ordinary item in a background job. That load is
//! diverted to open another file for the made-up subtypes that stand for an amount on the
//! ground.
//!
//! Build-specific: the job is found by its instructions.

use super::ModuleBuild;
use super::ground_models::GroundModel;
use super::stub::{ANY, CLIENT, RETURN, Stub, i32_at, signature};
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::report::Report;

/// Id of the plan group that installs the trampoline.
pub const GROUP: &str = "item-models";

// add rdi,10h / mov [rsp+N],rdi / mov [rsp+M],rax / lea rdx,[rsp+N] / mov rcx,rsi, then
// the replaced xor r8d,r8d / call load_cub / jmp finished
const SITE: &str = r"\x48\x83\xc7\x10\x48\x89\x7c\x24(.)\x48\x89\x44\x24.\x48\x8d\x54\x24(.)\x48\x89\xf1(\x45\x31\xc0\xe8(....)\xe9(....))";
// mov ecx,[rdi+<item type>] / cmp ecx,imm32, earlier in the job, before rdi is advanced
const ITEM: &str = r"\x8b\x8f(....)\x81\xf9";
// asm/item_model.asm
pub(super) const STUB: &str = "4c8d0d00000000418b0183f8ff74263b87222222227518418b41043b8733333333750c496341084903c1488902\
eb064983c10cebd24533c0e8cccccccce9dddddddd";

struct Hook {
    site: u32,
    expect: Vec<u8>,
    /// The client's .cub loader, which the replaced call goes to.
    loader: u32,
    /// Where the replaced jmp goes.
    finished: u32,
    /// Where the job keeps the item type, from rdi as it is at the hook.
    item: i32,
}

fn find_hook(module: &ModuleBuild) -> Option<Hook> {
    let image = module.image;
    let (text_va, text) = image.data(".text")?;
    let site = signature(SITE);
    // both moves use the same stack slot
    let mut sites = site.captures_iter(text).filter(|hit| hit[1] == hit[2]);
    let hit = sites.next()?;
    if sites.next().is_some() {
        return None;
    }
    let replaced = hit.get(3)?;
    let at = text_va + replaced.start() as u32;
    let job = image.function_start(text_va + hit.get(0)?.start() as u32)?;
    let before = text.get((job.checked_sub(text_va)?) as usize..hit.get(0)?.start())?;
    let item = signature(ITEM).captures(before)?;
    Some(Hook {
        site: at,
        expect: replaced.as_bytes().to_vec(),
        loader: (at + 8).wrapping_add_signed(i32_at(&hit[4], 0)),
        finished: (at + 13).wrapping_add_signed(i32_at(&hit[5], 0)),
        item: i32_at(&item[1], 0) - 0x10,
    })
}

/// Installs the trampoline for one client DLL. False if there was nothing to install or
/// the build has no place for it.
pub fn plan(module: &mut ModuleBuild, models: &[GroundModel], report: &mut Report) -> bool {
    if models.is_empty() {
        return false;
    }
    let Some(hook) = find_hook(module) else {
        report.warn(format!(
            "{}: could not locate the item model loader in this build; models by amount skipped",
            module.backend.key()
        ));
        return false;
    };
    let mut stub = Stub::new(STUB);
    stub.fill(0x22, hook.item);
    stub.fill(0x33, hook.item + 4);
    // after the code: the table, then the model paths
    let table_at = stub.size();
    let paths_at = table_at + 12 * models.len() + 4;
    let (mut table, mut paths) = (Vec::new(), Vec::new());
    for (n, model) in models.iter().enumerate() {
        let relative = (paths_at + paths.len()) as i32 - (table_at + 12 * n) as i32;
        table.extend_from_slice(&model.item_type.to_le_bytes());
        table.extend_from_slice(&model.subtype().to_le_bytes());
        table.extend_from_slice(&relative.to_le_bytes());
        paths.extend_from_slice(model.name.as_bytes());
        paths.push(0);
    }
    table.extend_from_slice(&ANY.to_le_bytes());
    stub.lea([0x4C, 0x8D, 0x0D], 0, table_at);
    let (loader, resume) = (stub.at(CLIENT), stub.at(RETURN));

    let start = module.plan.reserve();
    let blob = &mut module.plan.blob;
    blob.extend(stub.into_bytes());
    blob.extend(table);
    blob.extend(paths);
    blob.resize(blob.len() + BLOB_PAD, 0);

    module.plan.claims.push(Claim {
        start: hook.site,
        end: hook.site + hook.expect.len() as u32,
        owner: "item models".into(),
    });
    let mut who: Vec<&str> = Vec::new();
    for model in models {
        if !who.contains(&model.owner.as_str()) {
            who.push(&model.owner);
        }
    }
    module.plan.groups.push(Group {
        id: GROUP.into(),
        label: format!(
            "{} model file(s) by amount [{}]",
            models.len(),
            who.join("+")
        ),
        ops: vec![
            Op::BlobRel32 {
                blob_offset: start + loader,
                target: hook.loader,
            },
            Op::BlobRel32 {
                blob_offset: start + resume,
                target: hook.finished,
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
        finished: u32,
    }

    /// A client with the item job. `slot` is the stack slot the job uses, which moved
    /// between releases.
    fn client(slot: u8) -> Client {
        let mut pe = TinyPe::new();
        let loader = pe.code(&[0x55, 0x56, 0x57, 0xC3]);
        pe.function(loader, loader + 4);

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
        let finished = pe.code(&[0xC3]);
        pe.function(job, finished + 1);
        pe.patch(call + 1, &loader.wrapping_sub(call + 5).to_le_bytes());
        pe.patch(jump + 1, &finished.wrapping_sub(jump + 5).to_le_bytes());
        Client {
            pe,
            site,
            loader,
            finished,
        }
    }

    #[test]
    fn installs_the_table_and_makes_the_replaced_call() {
        for slot in [0x48, 0x38] {
            let client = client(slot);
            let image = client.pe.build();
            let refs = RefIndex::build(&image).unwrap();
            let mut module = ModuleBuild {
                backend: Backend::Avx2,
                image: &image,
                refs: &refs,
                plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
            };
            let models = [model(43, 20, "rupees"), model(43, 5, "other")];
            let mut report = Report::default();
            assert!(plan(&mut module, &models, &mut report));

            let plan = &module.plan;
            let group = &plan.groups[0];
            assert_eq!(group.id, GROUP);
            assert_eq!(group.label, "2 model file(s) by amount [rupees+other]");
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
                        target: client.finished
                    },
                ]
            );
            assert_eq!(plan.data_size, 0);

            let blob = &plan.blob;
            let code = *start as usize;
            let filled = |marker: u8| i32_at(blob, code + stub.at(marker) as usize);
            assert_eq!((filled(0x22), filled(0x33)), (0x9C, 0xA0));
            // xor r8d,r8d right before the call, as the client has it
            let call = code + stub.at(CLIENT) as usize - 1;
            assert_eq!(blob[call - 3..call], [0x45, 0x33, 0xC0]);
            // the table: the subtype for 20 or more, and its file
            let table = code + stub.size();
            assert_eq!(
                (i32_at(blob, table), i32_at(blob, table + 4)),
                (43, 0x4357_0014)
            );
            let path = (table as i32 + i32_at(blob, table + 8)) as usize;
            assert!(blob[path..].starts_with(b"sprites/cwo-ground-0000000000000014.cub\0"));
            assert_eq!(i32_at(blob, table + 24), -1);
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
        assert!(!plan(&mut module, &[model(43, 5, "a")], &mut report));
        assert!(!plan(&mut module, &[], &mut report));
        assert_eq!(report.warnings().count(), 1);
        assert!(module.plan.groups.is_empty());
    }
}
