//! Pickup sounds: the `"pickupSounds"` manifest section.
//!
//! When the player picks an item up, the client queues its one pickup sound and then
//! copies the item and its count out of the event record. That store is diverted, so the
//! items in the table get a sound of their own. A later mod replaces an earlier one's
//! choice for the same item.
//!
//! Build-specific: the store is found by the sound id it writes. The register holding
//! the event record, and the offsets of the item and the count in it, are read from the
//! code after it.

use memchr::memmem;
use regex::bytes::{Captures, Regex};
use serde_json::Value;

use super::ModuleBuild;
use super::sounds::{self, Added};
use super::stub::{ANY, CLIENT, DATA, RETURN, Stub, item_key, signature};
use crate::mods::{Mod, Notice};
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::report::{Report, one_line};

/// The trampoline keeps a count of pickups and the last eight of them here.
pub const DATA_SIZE: u32 = 0x1000;

// mov eax,[REC+N+16] ... (v)movups xmm0,[REC+N]: the 20-byte item leaves the event record.
// REC is rdi, r14 or r12 so far. An operand is a ModRM byte, with a SIB byte for r12
const ITEM_COPY: &str = r"(\x41?)\x8b(\x43|\x44\x24|[\x45-\x47])(.).{0,10}?(\xc5\xf8|\xc4\xc1\x78|\x41?\x0f)\x10(\x43|\x44\x24|[\x45-\x47])(.)";
// mov ecx,[REC+N]: and so does the count
const COUNT: &str = r"(\x41?)\x8b(\x4b|\x4c\x24|[\x4d-\x4f])(.)";
/// Set, the pickup sounds go through a context hook instead of the trampoline. For the
/// pilot of context hooks only.
const CONTEXT_PILOT: &str = "CWOML_CONTEXT_HOOKS";
/// How far behind the store, or behind where the code jumps to, the copy is looked for.
const AFTER: usize = 140;
/// The registers a function keeps across the calls it makes, less the stack's two. The
/// record is in one of them from the hook to where its register is read.
const KEPT: [u8; 7] = [3, 6, 7, 12, 13, 14, 15];
// asm/pickup_hook.asm
pub(super) const STUB: &str = "4c8bc8458b9122222222458b9933333333458b8144444444415041b8cccccccc4c8d0d00000000418339ff74\
1f453911751441837904ff7406453959047507450fb74108eb064983c10cebdb66448904014c8d0deeeeeeee\
50418b0141ff0183e007c1e0044d8d4c01105845891145895904415a458951084589410ce9dddddddd";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub item_type: u32,
    /// None for every subtype of the type.
    pub subtype: Option<u32>,
    /// A game path, or the name of a sound a mod adds.
    pub sound: String,
    pub owner: String,
}

fn parse(patch: &Value) -> Result<(u32, Option<u32>, String), String> {
    let patch = patch.as_object().ok_or("an entry is an object")?;
    let (item_type, subtype) = item_key(patch)?;
    let sound = patch
        .get("sound")
        .and_then(Value::as_str)
        .ok_or("sound is missing")?;
    Ok((item_type, subtype, sound.replace('\\', "/")))
}

/// One sound per item, exact subtypes first: the trampoline takes the first match.
pub fn collect(mods: &mut [Mod], order: &[usize], report: &mut Report) -> Vec<Entry> {
    let mut chosen: Vec<Entry> = Vec::new();
    for &index in order {
        let found = &mut mods[index];
        for patch in found.list("pickupSounds").to_vec() {
            let (item_type, subtype, sound) = match parse(&patch) {
                Ok(entry) => entry,
                Err(reason) => {
                    found.warnings.push(
                        Notice::new("Pickup sound ignored")
                            .with("Entry", one_line(&patch.to_string(), 70))
                            .with("Reason", reason),
                    );
                    continue;
                }
            };
            let entry = Entry {
                item_type,
                subtype,
                sound,
                owner: found.id.clone(),
            };
            match chosen
                .iter_mut()
                .find(|e| (e.item_type, e.subtype) == (item_type, subtype))
            {
                Some(earlier) => {
                    if *earlier != entry {
                        report.warn(format!(
                            "pickup sound conflict for item type {item_type}: {} and {}; {} wins",
                            earlier.owner, entry.owner, entry.owner
                        ));
                    }
                    *earlier = entry;
                }
                None => chosen.push(entry),
            }
        }
    }
    chosen.sort_by_key(|e| (e.subtype.is_none(), e.item_type, e.subtype.unwrap_or(0)));
    chosen
}

struct Hook {
    site: u32,
    expect: Vec<u8>,
    /// The id the replaced store writes.
    stock: u16,
    /// The number of the register the event record is in.
    record: u8,
    /// Where the event record keeps the item.
    item: u8,
    /// And the count.
    count: u8,
}

/// The register a `[base + disp8]` operand goes through: the low bits of its ModRM
/// byte, or of its SIB byte where it has one, under the REX.B bit in front of it.
fn base(extended: bool, operand: &[u8]) -> u8 {
    (operand[operand.len() - 1] & 7) | u8::from(extended) << 3
}

/// Where the first `jmp rel32` in the code from `from` on goes, if inside `text`.
fn jumped(text: &[u8], from: usize) -> Option<usize> {
    let window = text.get(from..text.len().min(from + AFTER))?;
    let at = from + memchr::memchr(0xE9, window)?;
    let relative = i32::from_le_bytes(text.get(at + 1..at + 5)?.try_into().ok()?);
    let target = (at + 5).checked_add_signed(relative as isize)?;
    (target < text.len()).then_some(target)
}

/// What `fits` makes of the first match of `signature` in `code` it accepts, wherever
/// that match starts, and where the match ends.
fn first<'h, T>(
    signature: &Regex,
    code: &'h [u8],
    fits: impl Fn(&Captures<'h>) -> Option<T>,
) -> Option<(T, usize)> {
    let mut from = 0;
    while let Some(hit) = signature.captures_at(code, from) {
        let whole = hit.get(0)?;
        if let Some(found) = fits(&hit) {
            return Some((found, whole.end()));
        }
        from = whole.start() + 1;
    }
    None
}

fn find_hook(module: &ModuleBuild) -> Option<Hook> {
    let (text_va, text) = module.image.data(".text")?;
    let stock = sounds::sound_id(module.image, module.refs, "sounds/pickup.wav")?;
    // mov word [rcx+rax], <stock>. Several places queue this sound. The pickup event is
    // the one that then copies an item and a count out of the event record
    let store = [&[0x66, 0xC7, 0x04, 0x01][..], &stock.to_le_bytes()].concat();
    let (item_copy, count) = (signature(ITEM_COPY), signature(COUNT));
    let mut found = memmem::find_iter(text, &store).filter_map(|at| {
        let end = at + store.len();
        let copy = |from: usize| {
            let after = &text[from..text.len().min(from + AFTER)];
            let (found, copied) = first(&item_copy, after, |hit| {
                let record = base(!hit[1].is_empty(), &hit[2]);
                // 41h is a REX.B prefix, C4h opens a VEX prefix that carries the same bit
                let both = base(matches!(hit[4][0], 0x41 | 0xC4), &hit[5]) == record;
                (both && KEPT.contains(&record) && hit[3][0] == hit[6][0].wrapping_add(16))
                    .then(|| (record, hit[6][0]))
            })?;
            Some((found, from + copied))
        };
        // right after the store, or since game 0.1.4 where the code jumps to from there
        let ((record, item), copied) = copy(end).or_else(|| copy(jumped(text, end)?))?;
        let rest = &text[copied..text.len().min(copied + 48)];
        let (count, _) = first(&count, rest, |hit| {
            (base(!hit[1].is_empty(), &hit[2]) == record).then(|| hit[3][0])
        })?;
        Some(Hook {
            site: text_va + at as u32,
            expect: store.clone(),
            stock,
            record,
            item,
            count,
        })
    });
    let hook = found.next()?;
    found.next().is_none().then_some(hook)
}

/// Plans the entries whose sound this client has. Returns how many that was.
pub fn plan(
    module: &mut ModuleBuild,
    entries: &[Entry],
    added: &Added,
    report: &mut Report,
) -> usize {
    if entries.is_empty() {
        return 0;
    }
    let backend = module.backend.key();
    let Some(hook) = find_hook(module) else {
        report.warn(format!(
            "{backend}: could not locate the pickup sound code in this build; pickup sounds skipped"
        ));
        return 0;
    };
    let mut chosen = Vec::new();
    for entry in entries {
        let Some(id) = sounds::resolve(module.image, module.refs, added, &entry.sound) else {
            report.warn(format!(
                "{backend}: {}: '{}' {}; entry skipped",
                entry.owner,
                entry.sound,
                sounds::unknown(added, &entry.sound)
            ));
            continue;
        };
        chosen.push((entry.item_type, entry.subtype.unwrap_or(ANY), id));
    }
    let planned = chosen.len();
    if planned == 0 {
        return 0;
    }
    let mut who: Vec<&str> = Vec::new();
    for entry in entries {
        if !who.contains(&entry.owner.as_str()) {
            who.push(&entry.owner);
        }
    }
    let label = format!("{planned} pickup sound(s) [{}]", who.join("+"));
    if std::env::var_os(CONTEXT_PILOT).is_some() {
        plan_context(module, hook, &chosen, label);
        return planned;
    }
    let mut table = Vec::new();
    for &(kind, subtype, id) in &chosen {
        table.extend_from_slice(&kind.to_le_bytes());
        table.extend_from_slice(&subtype.to_le_bytes());
        table.extend_from_slice(&id.to_le_bytes());
        table.extend_from_slice(&[0, 0]);
    }
    table.extend_from_slice(&ANY.to_le_bytes());

    let mut stub = Stub::new(STUB);
    stub.fill(CLIENT, i32::from(hook.stock));
    stub.fill(0x22, i32::from(hook.item));
    stub.fill(0x33, i32::from(hook.item) + 4);
    stub.fill(0x44, i32::from(hook.count));
    stub.lea([0x4C, 0x8D, 0x0D], 0, stub.size()); // the table follows the code
    let (data, resume) = (stub.at(DATA), stub.at(RETURN));
    let mut code = stub.into_bytes();
    // it opens with `mov r9, rax`, where rax stands for the record's register
    debug_assert_eq!(code[..3], [0x4C, 0x8B, 0xC8]);
    code[0] |= hook.record >> 3;
    code[2] |= hook.record & 7;
    let start = module.plan.reserve();
    let blob = &mut module.plan.blob;
    blob.extend(code);
    blob.extend(table);
    blob.resize(blob.len() + BLOB_PAD, 0);
    module.plan.data_size = module.plan.data_size.max(DATA_SIZE);
    module.plan.data_ref(start + data, 0);

    let end = hook.site + hook.expect.len() as u32;
    module.plan.claims.push(Claim {
        start: hook.site,
        end,
        owner: "pickup sounds".into(),
    });
    module.plan.groups.push(Group {
        id: "pickup-sounds".into(),
        label,
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
    planned
}

/// The same through the in-game loader's `pickup-sound` handler, for the context hook
/// pilot: it replaces the stock store and runs after it, overwriting the id it stored.
fn plan_context(module: &mut ModuleBuild, hook: Hook, chosen: &[(u32, u32, u16)], label: String) {
    let mut data = vec![hook.record, hook.item];
    for &(kind, subtype, id) in chosen {
        data.extend(kind.to_le_bytes());
        data.extend(subtype.to_le_bytes());
        data.extend(id.to_le_bytes());
    }
    module.plan.claims.push(Claim {
        start: hook.site,
        end: hook.site + hook.expect.len() as u32,
        owner: "pickup sounds".into(),
    });
    module.plan.groups.push(Group {
        id: "pickup-sounds".into(),
        label,
        ops: vec![Op::Context {
            rva: hook.site,
            expect: hook.expect,
            after: true,
            handler: "pickup-sound".into(),
            data,
        }],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::sounds::tests::loads;
    use crate::features::stub::i32_at;
    use crate::game::Backend;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};
    use crate::plan::ModulePlan;
    use crate::refs::RefIndex;
    use crate::testing::TinyPe;

    fn entry(item_type: u32, subtype: Option<u32>, sound: &str, owner: &str) -> Entry {
        Entry {
            item_type,
            subtype,
            sound: sound.into(),
            owner: owner.into(),
        }
    }

    #[test]
    fn a_later_mod_replaces_the_sound_of_an_item() {
        let dir = tempfile::tempdir().unwrap();
        write_mod(
            dir.path(),
            "a",
            r#"{"schema": 1, "id": "a", "version": "1.0.0", "pickupSounds": [
                {"type": 43, "sound": "sounds\\drop-coin.wav"},
                {"type": 95, "subtype": 40, "sound": "chime"},
                {"type": 95},
                {"sound": "chime"}]}"#,
        );
        write_mod(
            dir.path(),
            "b",
            r#"{"schema": 1, "id": "b", "version": "1.0.0", "pickupSounds": [
                {"type": 43, "sound": "chime"}, {"type": 7, "subtype": 1, "sound": "chime"}]}"#,
        );
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let mut report = Report::default();
        let entries = collect(&mut mods, &order, &mut report);
        assert_eq!(
            entries,
            [
                entry(7, Some(1), "chime", "b"),
                entry(95, Some(40), "chime", "a"),
                entry(43, None, "chime", "b"),
            ]
        );
        assert_eq!(mods[0].warnings.len(), 2);
        assert_eq!(
            report.warnings().collect::<Vec<_>>(),
            ["pickup sound conflict for item type 43: a and b; b wins"]
        );
    }

    /// A client that queues the pickup sound twice. The second time it goes on to read
    /// the item and the count out of the event record with `copy` and `count`. Returns
    /// the client and where that second store is.
    fn client(copy: &[u8], count: &[u8]) -> (crate::pe::PeImage, u32) {
        let mut pe = TinyPe::new();
        loads(&mut pe, "sounds/pickup.wav", 0x3D);
        loads(&mut pe, "sounds/cry.wav", 0x6B);
        pe.code(&[0x66, 0xC7, 0x04, 0x01, 0x3D, 0x00]);
        pe.code(&[0x90; 200]);
        let site = pe.code(&[0x66, 0xC7, 0x04, 0x01, 0x3D, 0x00]);
        pe.code(&[0x90; 8]);
        pe.code(copy);
        pe.code(&[0x90; 4]);
        pe.code(count);
        pe.code(&[0x90; 64]);
        (pe.build(), site)
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
    fn reads_the_record_through_the_register_a_build_keeps_it_in() {
        // how a build reads the item and the count, then the record's register and the
        // trampoline's first instruction for it. None where the code is not the pickup's
        type Build = (&'static [u8], &'static [u8], Option<(u8, [u8; 3])>);
        let builds: [Build; 5] = [
            // mov eax,[rdi+20h] / movups xmm0,[rdi+10h] / mov ecx,[rdi+40h]
            (
                &[0x8B, 0x47, 0x20, 0x0F, 0x10, 0x47, 0x10],
                &[0x8B, 0x4F, 0x40],
                Some((7, [0x4C, 0x8B, 0xCF])),
            ),
            // the same through r14, with vmovups
            (
                &[0x41, 0x8B, 0x46, 0x20, 0xC4, 0xC1, 0x78, 0x10, 0x46, 0x10],
                &[0x41, 0x8B, 0x4E, 0x40],
                Some((14, [0x4D, 0x8B, 0xCE])),
            ),
            // and through r12, which takes a SIB byte
            (
                &[
                    0x41, 0x8B, 0x44, 0x24, 0x20, 0x41, 0x0F, 0x10, 0x44, 0x24, 0x10,
                ],
                &[0x41, 0x8B, 0x4C, 0x24, 0x40],
                Some((12, [0x4D, 0x8B, 0xCC])),
            ),
            // the item through rdi and the count through rsi is not one record
            (
                &[0x8B, 0x47, 0x20, 0x0F, 0x10, 0x47, 0x10],
                &[0x8B, 0x4E, 0x40],
                None,
            ),
            // r10 does not outlast the call between the store and here
            (
                &[0x41, 0x8B, 0x42, 0x20, 0x41, 0x0F, 0x10, 0x42, 0x10],
                &[0x41, 0x8B, 0x4A, 0x40],
                None,
            ),
        ];
        for (copy, count, wanted) in builds {
            let (image, site) = client(copy, count);
            let refs = RefIndex::build(&image).unwrap();
            let mut module = module(&image, &refs);
            let hook = find_hook(&module);
            let found = hook.as_ref().map(|hook| (hook.site, hook.record));
            assert_eq!(found, wanted.map(|(record, _)| (site, record)));
            let Some((_, opening)) = wanted else {
                continue;
            };
            let entries = [entry(43, None, "sounds/cry.wav", "a")];
            let planned = plan(
                &mut module,
                &entries,
                &Added::default(),
                &mut Report::default(),
            );
            assert_eq!(planned, 1);
            let Op::Hook { blob_offset, .. } = &module.plan.groups[0].ops[1] else {
                panic!("the hook comes last")
            };
            let start = *blob_offset as usize;
            assert_eq!(module.plan.blob[start..start + 3], opening);
        }
    }

    #[test]
    fn follows_the_jump_to_where_the_record_is_copied() {
        // game 0.1.4: the store, the rest of the queue entry, then a jump to the copy
        let mut pe = TinyPe::new();
        loads(&mut pe, "sounds/pickup.wav", 0x3D);
        let site = pe.code(&[0x66, 0xC7, 0x04, 0x01, 0x3D, 0x00]);
        pe.code(&[0x90; 16]);
        let jump = pe.code(&[0xE9, 0, 0, 0, 0]);
        pe.code(&[0x90; 200]);
        // mov eax,[r15+20h] / vmovups xmm0,[r15+10h] / mov ecx,[r15+40h]
        let copy = pe.code(&[0x41, 0x8B, 0x47, 0x20, 0xC4, 0xC1, 0x78, 0x10, 0x47, 0x10]);
        pe.code(&[0x90; 8]);
        pe.code(&[0x41, 0x8B, 0x4F, 0x40]);
        pe.code(&[0x90; 64]);
        pe.patch(jump + 1, &copy.wrapping_sub(jump + 5).to_le_bytes());
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let hook = find_hook(&module(&image, &refs)).unwrap();
        assert_eq!(
            (hook.site, hook.record, hook.item, hook.count),
            (site, 15, 0x10, 0x40)
        );
    }

    #[test]
    fn hooks_the_store_of_the_pickup_sound() {
        // the record in rdi: mov eax,[rdi+20h] / movups xmm0,[rdi+10h] / mov ecx,[rdi+40h]
        let (image, site) = client(
            &[0x8B, 0x47, 0x20, 0x0F, 0x10, 0x47, 0x10],
            &[0x8B, 0x4F, 0x40],
        );
        let refs = RefIndex::build(&image).unwrap();
        let mut module = module(&image, &refs);
        let added = Added {
            ids: [("chime".to_owned(), 0x6B)].into(),
            playable: true,
        };
        let entries = [
            entry(95, Some(40), "chime", "a"),
            entry(43, None, "sounds/pickup.wav", "b"),
            entry(7, None, "sounds/nope.wav", "a"),
            entry(8, None, "nope", "a"),
        ];
        let mut report = Report::default();
        assert_eq!(plan(&mut module, &entries, &added, &mut report), 2);

        let plan = &module.plan;
        let group = &plan.groups[0];
        assert_eq!(
            (group.id.as_str(), group.label.as_str()),
            ("pickup-sounds", "2 pickup sound(s) [a+b]")
        );
        let Op::Hook {
            rva,
            expect,
            blob_offset: start,
        } = &group.ops[1]
        else {
            panic!("the hook comes last")
        };
        assert_eq!((*rva, expect.len()), (site, 6));
        let stub = Stub::new(STUB);
        assert_eq!(
            group.ops[0],
            Op::BlobRel32 {
                blob_offset: start + stub.at(RETURN),
                target: site + 6
            }
        );
        assert_eq!(plan.data_size, DATA_SIZE);

        let blob = &plan.blob;
        let code = *start as usize;
        let at = |marker: u8| i32_at(blob, code + stub.at(marker) as usize);
        assert_eq!(
            (at(CLIENT), at(0x22), at(0x33), at(0x44)),
            (0x3D, 0x10, 0x14, 0x40)
        );
        let table = code + stub.size();
        assert_eq!(
            blob[table..table + 28],
            [
                &95u32.to_le_bytes()[..],
                &40u32.to_le_bytes(),
                &[0x6B, 0, 0, 0],
                &43u32.to_le_bytes(),
                &ANY.to_le_bytes(),
                &[0x3D, 0, 0, 0],
                &ANY.to_le_bytes(),
            ]
            .concat()
        );
        assert_eq!(
            report.warnings().collect::<Vec<_>>(),
            [
                "avx2: a: 'sounds/nope.wav' is not a sound the game loads; entry skipped",
                "avx2: a: 'nope' is not a sound this mod set adds (see \"sounds\"); entry skipped",
            ]
        );
    }
}
