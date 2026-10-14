//! The start screen: a note after the game's version in its corner, saying how many mods
//! the game runs and which profile was loaded. It is not a manifest section: every plan has
//! it.
//!
//! Build-specific: the client puts the version together in code, a 'v' and then its
//! digits as constants, and hands it to the text drawing as a pointer and a length stored
//! side by side. Those two stores are diverted. They address the frame through rbp, so
//! the trampoline makes them itself, byte for byte.

use memchr::memmem;

use super::ModuleBuild;
use super::stub::{DATA, RETURN, Stub, signature};
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::report::Report;

/// Where in the data area the version and its note are put together.
const BUFFER: u32 = 0x800;
/// The data area has to reach past the buffer.
const DATA_SIZE: u32 = 0x1000;
/// The longest note. With the 64 bytes of version the trampoline copies, it fits the
/// buffer.
const MAX_NOTE: usize = 60;
// mov byte [rbp+X],'v' ... mov dword [rbp+X+1],"0.1.": the version being put together
const VERSION: &str = r"\xc6\x45(.)v.{0,16}?\xc7\x45(.)[0-9.]{4}";
// mov [rbp+N],rcx / mov [rbp+N+8],rax: its text and length, handed on
const HANDED: &str = r"\x48\x89\x4d(.)\x48\x89\x45(.)";
/// How far behind the version the hand-over is looked for.
const AFTER: usize = 0x100;
// asm/start_screen.asm
pub(super) const STUB: &str = "4c8d15eeeeeeee4533db4c3bd873134983fb40730d428a14194388141a49ffc3ebe84c8d0500000000458b08\
4983c0044585c97412418a104388141a49ffc049ffc341ffc9ebe9498bca498bc39090909090909090e9dddd\
dddd";

/// What follows the version: ` | 2 mods | Loaded Default`. A character the game's font
/// does not have becomes '?'.
pub fn note(loaded: usize, profile: Option<&str>) -> String {
    let mods = match loaded {
        1 => "1 mod".to_owned(),
        n => format!("{n} mods"),
    };
    let Some(profile) = profile else {
        return format!(" | {mods}");
    };
    let room = MAX_NOTE - format!(" | {mods} | Loaded ").len();
    let name: String = profile
        .chars()
        .map(|c| if (' '..='~').contains(&c) { c } else { '?' })
        .take(room)
        .collect();
    format!(" | {mods} | Loaded {name}")
}

struct Hook {
    site: u32,
    /// The two stores.
    expect: Vec<u8>,
}

fn find_hook(module: &ModuleBuild) -> Option<Hook> {
    let (text_va, text) = module.image.data(".text")?;
    let (version, handed) = (signature(VERSION), signature(HANDED));
    let mut found = version.captures_iter(text).filter_map(|hit| {
        // the digits go right after the 'v'
        if hit[2][0] != hit[1][0].wrapping_add(1) {
            return None;
        }
        let end = hit.get(0)?.end();
        let window = &text[end..text.len().min(end + AFTER)];
        let stores = handed
            .captures_iter(window)
            .find(|pair| pair[2][0] == pair[1][0].wrapping_add(8))?;
        let at = end + stores.get(0)?.start();
        // lea rdx,[rbp+N]: the pair is what the drawing is given
        let given = [0x48, 0x8D, 0x55, stores[1][0]];
        memmem::find(&text[at + 8..text.len().min(at + 32)], &given)?;
        Some(Hook {
            site: text_va + at as u32,
            expect: text[at..at + 8].to_vec(),
        })
    });
    let hook = found.next()?;
    found.next().is_none().then_some(hook)
}

/// Plans the note after the version on the start screen. False if this client has no
/// version there the way it is looked for.
pub fn plan(module: &mut ModuleBuild, note: &str, report: &mut Report) -> bool {
    let Some(hook) = find_hook(module) else {
        report.warn(format!(
            "{}: could not locate the version on the start screen in this build; no note there",
            module.backend.key()
        ));
        return false;
    };
    let mut stub = Stub::new(STUB);
    stub.lea([0x4C, 0x8D, 0x05], 0, stub.size()); // the note follows the code
    let (data, resume) = (stub.at(DATA), stub.at(RETURN));
    let mut code = stub.into_bytes();
    let stores = code
        .windows(8)
        .position(|bytes| bytes == [0x90; 8])
        .expect("room for the replaced stores");
    code[stores..stores + 8].copy_from_slice(&hook.expect);

    let start = module.plan.reserve();
    let blob = &mut module.plan.blob;
    blob.extend(code);
    blob.extend((note.len() as u32).to_le_bytes());
    blob.extend(note.as_bytes());
    blob.resize(blob.len() + BLOB_PAD, 0);
    module.plan.data_size = module.plan.data_size.max(DATA_SIZE);
    module.plan.data_ref(start + data, BUFFER);

    let end = hook.site + hook.expect.len() as u32;
    module.plan.claims.push(Claim {
        start: hook.site,
        end,
        owner: "start screen".into(),
    });
    module.plan.groups.push(Group {
        id: "start-screen".into(),
        label: format!(
            "note after the version on the start screen: \"{}\"",
            note.trim_start()
        ),
        ops: vec![
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
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::stub::i32_at;
    use crate::game::Backend;
    use crate::pe::PeImage;
    use crate::plan::ModulePlan;
    use crate::refs::RefIndex;
    use crate::testing::TinyPe;

    #[test]
    fn the_note_counts_the_mods_and_names_the_profile() {
        assert_eq!(note(2, Some("Default")), " | 2 mods | Loaded Default");
        assert_eq!(note(1, Some("Hard mode")), " | 1 mod | Loaded Hard mode");
        assert_eq!(note(0, None), " | 0 mods");
        assert_eq!(note(3, Some("Café ✓")), " | 3 mods | Loaded Caf? ?");
        let long = note(12, Some(&"x".repeat(80)));
        assert_eq!((long.len(), long.ends_with('x')), (MAX_NOTE, true));
    }

    /// The start screen's version as both clients put it together, `v0.1.1`, handed to a
    /// call as a pointer and a length at [rbp-48h]. Returns the client and the stores.
    fn client() -> (PeImage, u32) {
        let mut pe = TinyPe::new();
        // something else written to a stack buffer, not the version
        pe.code(&[
            0xC6, 0x45, 0x10, 0x76, 0xC7, 0x45, 0x30, b'a', b'b', b'c', b'd',
        ]);
        pe.code(&[0x90; 0x120]);
        // mov qword [rbp+28h],0Fh / mov byte [rbp+10h],'v' / mov qword [rbp+20h],6
        pe.code(&[0x48, 0xC7, 0x45, 0x28, 0x0F, 0, 0, 0]);
        pe.code(&[0xC6, 0x45, 0x10, b'v']);
        pe.code(&[0x48, 0xC7, 0x45, 0x20, 6, 0, 0, 0]);
        // mov dword [rbp+11h],"0.1." / mov byte [rbp+15h],'1'
        pe.code(&[
            0xC7, 0x45, 0x11, b'0', b'.', b'1', b'.', 0xC6, 0x45, 0x15, b'1',
        ]);
        pe.code(&[0x90; 0x40]);
        // lea rcx,[rbp-20h] / mov [rbp-48h],rcx / mov [rbp-40h],rax / ... / lea rdx,[rbp-48h]
        pe.code(&[0x48, 0x8D, 0x4D, 0xE0]);
        let stores = pe.code(&[0x48, 0x89, 0x4D, 0xB8, 0x48, 0x89, 0x45, 0xC0]);
        pe.code(&[0x48, 0x8D, 0x45, 0xC8, 0x48, 0x89, 0x44, 0x24, 0x20]);
        pe.code(&[0x48, 0x8D, 0x55, 0xB8, 0x48, 0x89, 0xF9, 0x45, 0x31, 0xC0]);
        pe.code(&[0xE8, 0, 0, 0, 0]);
        pe.code(&[0x90; 32]);
        (pe.build(), stores)
    }

    #[test]
    fn diverts_the_hand_over_of_the_version() {
        let (image, stores) = client();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = ModuleBuild {
            backend: Backend::Avx2,
            image: &image,
            refs: &refs,
            plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
        };
        let note = note(2, Some("Default"));
        assert!(plan(&mut module, &note, &mut Report::default()));

        let group = &module.plan.groups[0];
        assert_eq!(group.id, "start-screen");
        let Op::Hook {
            rva,
            expect,
            blob_offset: start,
        } = &group.ops[1]
        else {
            panic!("the hook comes last")
        };
        let replaced = [0x48, 0x89, 0x4D, 0xB8, 0x48, 0x89, 0x45, 0xC0];
        assert_eq!((*rva, &expect[..]), (stores, &replaced[..]));
        assert_eq!(
            group.ops[0],
            Op::BlobRel32 {
                blob_offset: start + Stub::new(STUB).at(RETURN),
                target: stores + 8
            }
        );
        let blob = &module.plan.blob;
        let code = *start as usize;
        let size = Stub::new(STUB).size();
        // the stores are made by the trampoline, right before it jumps back
        let jump = code + Stub::new(STUB).at(RETURN) as usize - 1;
        assert_eq!(blob[jump - 8..jump], replaced);
        // the note follows the code: its length, then its text
        let table = code + size;
        assert_eq!(i32_at(blob, table) as usize, note.len());
        assert_eq!(&blob[table + 4..table + 4 + note.len()], note.as_bytes());
        // and the lea in front of the note's length reaches it
        let lea = Stub::new(STUB)
            .into_bytes()
            .windows(7)
            .position(|bytes| bytes == [0x4C, 0x8D, 0x05, 0, 0, 0, 0])
            .unwrap();
        let reach = lea + 7 + i32_at(blob, code + lea + 3) as usize;
        assert_eq!(reach, size);
        assert_eq!(module.plan.data_size, DATA_SIZE);
    }

    #[test]
    fn a_build_without_the_version_is_reported() {
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
        assert!(!plan(&mut module, " (0 mods)", &mut report));
        assert!(module.plan.groups.is_empty());
        assert_eq!(
            report.warnings().collect::<Vec<_>>(),
            [
                "baseline: could not locate the version on the start screen in this build; no note there"
            ]
        );
    }
}
