//! The item-model trampoline, for `ground_models`.
//!
//! The client fetches the model of every item through one function, handing it the item
//! and the model file to load. Since game 0.1.2 it keeps one model per file, so items
//! that are to look different need files of their own before that. For the made-up
//! subtypes that stand for an amount on the ground, the start of that function puts in
//! the entry's file.
//!
//! Build-specific: the function is the one model_of_item calls with the coin's file, and
//! where it keeps the file name's length is read from its code.

use super::ModuleBuild;
use super::ground_models::{GroundModel, coin_named};
use super::stub::{ANY, RETURN, Stub, i32_at, signature};
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::report::Report;

/// Id of the plan group that installs the trampoline.
pub const GROUP: &str = "item-models";

// lea r9,[rbp+N]: the model file, handed on after the coin's is put together
const FILE_ARGUMENT: &str = r"\x4c\x8d(?:\x4d.|\x8d....)";
/// push rbp / push r15 / push r14: the start of the function, which the hook replaces.
const PROLOGUE: [u8; 5] = [0x55, 0x41, 0x57, 0x41, 0x56];
// mov REG,r9: the model file, kept
const KEPT: &str = r"([\x4c\x4d])\x89([\xc8-\xcf])";
// mov or cmp of 64 bits at [REG+N]: the length of its name. The avx2 client reads it into
// rax, the baseline one compares it in place
const LENGTH: &str = r"([\x48-\x4f])[\x8b\x39\x3b]([\x80-\xbf])(....)";
/// How far behind the coin's file name the call is looked for.
const AFTER: usize = 0x40;
/// How far into the function the length is looked for.
const BODY: usize = 0x400;
// asm/item_model.asm
pub(super) const STUB: &str = "4c8d1500000000418b0283f8ff743b413b007530418b4204413b40047526496342084903c24533db460fb61418\
478814194585d2740549ffc3ebed4d899955555555eb064983c20cebbd5541574156e9dddddddd";

struct Hook {
    /// The function every model is fetched through.
    site: u32,
    /// Where the file it is handed keeps the length of its name, and so how long a name
    /// can be.
    length: i32,
}

/// A register from a REX prefix's B bit and the low bits of a ModRM byte.
fn register(rex: u8, modrm: u8) -> u8 {
    (modrm & 7) | ((rex & 1) << 3)
}

fn find_hook(module: &ModuleBuild) -> Option<Hook> {
    let image = module.image;
    let (text_va, text) = image.data(".text")?;
    let named = coin_named(module)?.checked_sub(text_va)? as usize;
    let window = text.get(named..text.len().min(named + AFTER))?;
    let handed = signature(FILE_ARGUMENT).find(window)?.end();
    // the first call after it that goes to a function starting the expected way
    let site = (handed..window.len().saturating_sub(4))
        .filter(|&at| window[at] == 0xE8)
        .map(|at| (text_va + (named + at) as u32 + 5).wrapping_add_signed(i32_at(window, at + 1)))
        .find(|&target| {
            image.function_start(target) == Some(target)
                && image.read(target, PROLOGUE.len()) == Some(&PROLOGUE[..])
        })?;
    let start = (site - text_va) as usize;
    let body = text.get(start..text.len().min(start + BODY))?;
    let kept = signature(KEPT).captures(body)?;
    let reg = register(kept[1][0], kept[2][0]);
    let after = &body[kept.get(0)?.end()..];
    let read = signature(LENGTH)
        .captures_iter(after)
        .find(|hit| register(hit[1][0], hit[2][0]) == reg && hit[2][0] & 7 != 4)?;
    let length = i32_at(&read[3], 0);
    (0x10..=0x400)
        .contains(&length)
        .then_some(Hook { site, length })
}

/// Installs the trampoline for one client DLL. False if there was nothing to install or
/// the build has no place for it.
pub fn plan(module: &mut ModuleBuild, models: &[GroundModel], report: &mut Report) -> bool {
    if models.is_empty() {
        return false;
    }
    let backend = module.backend.key();
    let Some(hook) = find_hook(module) else {
        report.warn(format!(
            "{backend}: could not locate where the client fetches item models in this build; models by amount skipped"
        ));
        return false;
    };
    // a name and its end must fit in front of the length
    if let Some(long) = models.iter().find(|m| m.name.len() >= hook.length as usize) {
        report.warn(format!(
            "{backend}: the model file name {} is longer than this build allows; models by amount skipped",
            long.name
        ));
        return false;
    }
    let mut stub = Stub::new(STUB);
    stub.fill(0x55, hook.length);
    // after the code: the table, then the model file names
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
    stub.lea([0x4C, 0x8D, 0x15], 0, table_at);
    let resume = stub.at(RETURN);

    let start = module.plan.reserve();
    let blob = &mut module.plan.blob;
    blob.extend(stub.into_bytes());
    blob.extend(table);
    blob.extend(paths);
    blob.resize(blob.len() + BLOB_PAD, 0);

    let end = hook.site + PROLOGUE.len() as u32;
    module.plan.claims.push(Claim {
        start: hook.site,
        end,
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
                blob_offset: start + resume,
                target: end,
            },
            Op::Hook {
                rva: hook.site,
                expect: PROLOGUE.to_vec(),
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

    /// A client with the function every model is fetched through, and model_of_item
    /// calling it with the coin's file, the way game 0.1.2 has them. `kept` is the
    /// `mov REG,r9` and `read` the `mov rax,[REG+80h]` of the build. Returns the client and
    /// the function.
    fn client(kept: [u8; 3], read: [u8; 3]) -> (TinyPe, u32) {
        let mut pe = TinyPe::new();
        let fetch = pe.code(&[0x55, 0x41, 0x57, 0x41, 0x56, 0x41, 0x55, 0x53]);
        pe.code(&[0x48, 0x81, 0xEC, 0xF8, 3, 0, 0]);
        pe.code(&kept);
        pe.code(&[0x48, 0x89, 0xD6]);
        pe.code(&read);
        pe.code(&[0x80, 0, 0, 0]);
        let end = pe.code(&[0xC3]);
        pe.function(fetch, end + 1);
        // something that puts the file together, and does not start like the function
        let build = pe.code(&[0x48, 0x89, 0xC8, 0xC3]);
        pe.function(build, build + 4);

        let name = pe.rdata(b"sprites/gold-coin.cub\0");
        let lookup = pe.code(&[0x55, 0x48, 0x89, 0xE5]);
        pe.rip_relative(&[0x48, 0x8D, 0x15], name);
        pe.code(&[0x48, 0x8D, 0x4D, 0xB0]);
        let call = pe.code(&[0xE8, 0, 0, 0, 0]);
        pe.patch(call + 1, &build.wrapping_sub(call + 5).to_le_bytes());
        pe.code(&[0x4C, 0x89, 0x74, 0x24, 0x20, 0x4C, 0x8D, 0x4D, 0xB0]);
        pe.code(&[0x48, 0x89, 0xD9, 0x48, 0x89, 0xF2, 0x49, 0x89, 0xF8]);
        let call = pe.code(&[0xE8, 0, 0, 0, 0]);
        pe.patch(call + 1, &fetch.wrapping_sub(call + 5).to_le_bytes());
        let end = pe.code(&[0xC3]);
        pe.function(lookup, end + 1);
        (pe, fetch)
    }

    fn module<'a>(image: &'a crate::pe::PeImage, refs: &'a RefIndex) -> ModuleBuild<'a> {
        ModuleBuild {
            backend: Backend::Avx2,
            image,
            refs,
            plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
        }
    }

    #[test]
    fn puts_the_entrys_file_in_before_the_client_looks_it_up() {
        // mov r15,r9 / mov rax,[r15+80h] in the avx2 client of game 0.1.2,
        // mov rbx,r9 / mov rax,[rbx+80h] before, and cmp [r15+80h],rax in the baseline one
        for (kept, read) in [
            ([0x4D, 0x89, 0xCF], [0x49, 0x8B, 0x87]),
            ([0x4C, 0x89, 0xCB], [0x48, 0x8B, 0x83]),
            ([0x4D, 0x89, 0xCF], [0x49, 0x39, 0x87]),
        ] {
            let (pe, fetch) = client(kept, read);
            let image = pe.build();
            let refs = RefIndex::build(&image).unwrap();
            let mut module = module(&image, &refs);
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
            } = &group.ops[1]
            else {
                panic!("the hook comes last")
            };
            assert_eq!((*rva, &expect[..]), (fetch, &PROLOGUE[..]));
            let stub = Stub::new(STUB);
            assert_eq!(
                group.ops[0],
                Op::BlobRel32 {
                    blob_offset: start + stub.at(RETURN),
                    target: fetch + 5
                }
            );
            assert_eq!(plan.data_size, 0);

            let blob = &plan.blob;
            let code = *start as usize;
            assert_eq!(i32_at(blob, code + stub.at(0x55) as usize), 0x80);
            // the replaced instructions, right before the jump back
            let jump = code + stub.at(RETURN) as usize - 1;
            assert_eq!(blob[jump - 5..jump], PROLOGUE);
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
    fn a_build_without_the_function_is_skipped_and_reported() {
        let mut pe = TinyPe::new();
        pe.code(&[0x90; 64]);
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = module(&image, &refs);
        let mut report = Report::default();
        assert!(!plan(&mut module, &[model(43, 5, "a")], &mut report));
        assert!(!plan(&mut module, &[], &mut report));
        assert_eq!(report.warnings().count(), 1);
        assert!(module.plan.groups.is_empty());
    }
}
