//! Tooltip lines: the `"tooltips"` manifest section.
//!
//! The client builds the hint text under an item tooltip ("M1 EQUIP", "CTRL + M1 DROP")
//! in a 127-character buffer. It is diverted just before those hints are appended, so the
//! lines of the matching entries come first. Lines never conflict: when several mods
//! describe an item, all of their lines show, in load order.
//!
//! Build-specific: the place is found by the instructions around it.

use serde_json::Value;

use super::ModuleBuild;
use super::stub::{ANY, CLIENT, RETURN, Stub, i32_at, item_key, only};
use crate::mods::{Mod, Notice};
use crate::plan::{BLOB_PAD, Claim, Group, Op};
use crate::refs::{SiteKind, string_sites};
use crate::report::{Report, one_line};

/// One line. The buffer also has to hold the game's own hint lines.
const LINE_MAX: usize = 60;
/// All added lines of one item together.
const ITEM_MAX: usize = 72;

// mov byte [rsi+r14],0 / test bl,bl / je <return> / mov rcx,rdi / mov dl,1 / call
const SITE: &str = r"\x42\xc6\x04\x36\x00(\x84\xdb\x0f\x84(....))\x48\x89\xf9\xb2\x01\xe8";
// asm/tooltip_hook.asm
pub(super) const STUB: &str = "4c8d0d000000008b078b5704418b0983f9ff745e3bc87554418b490483f9ff74043bca7547496349084903c9\
4c8b86800000004d85c0740e4983f87e732e42c604060a49ffc0448a114584d274124983f87e770c46881406\
48ffc149ffc0ebe642c60406004c8986800000004983c10ceb9a84db0f84cccccccce9dddddddd";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub item_type: u32,
    /// None for every subtype of the type.
    pub subtype: Option<u32>,
    pub text: Vec<u8>,
    /// Id of the mod it comes from.
    pub owner: String,
}

fn parse(patch: &Value) -> Result<(u32, Option<u32>, Vec<u8>), String> {
    let patch = patch.as_object().ok_or("an entry is an object")?;
    let (item_type, subtype) = item_key(patch)?;
    let line = patch
        .get("line")
        .and_then(Value::as_str)
        .ok_or("line is missing")?
        .as_bytes();
    if line.is_empty() || line.iter().any(|c| !(0x20..=0x7E).contains(c)) {
        return Err("line must be printable ASCII, one line per entry".into());
    }
    if line.len() > LINE_MAX {
        return Err(format!("line is longer than {LINE_MAX} characters"));
    }
    Ok((item_type, subtype, line.to_vec()))
}

/// Every line the mods add, in load order, without exact duplicates.
pub fn collect(mods: &mut [Mod], order: &[usize], report: &mut Report) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for &index in order {
        let found = &mut mods[index];
        for patch in found.list("tooltips").to_vec() {
            match parse(&patch) {
                Ok((item_type, subtype, text)) => {
                    let same =
                        |l: &Line| (l.item_type, l.subtype, &l.text) == (item_type, subtype, &text);
                    if !lines.iter().any(same) {
                        lines.push(Line {
                            item_type,
                            subtype,
                            text,
                            owner: found.id.clone(),
                        });
                    }
                }
                Err(reason) => found.warnings.push(
                    Notice::new("Tooltip entry ignored")
                        .with("Entry", one_line(&patch.to_string(), 70))
                        .with("Reason", reason),
                ),
            }
        }
    }
    let mut items: Vec<(u32, u32)> = lines
        .iter()
        .filter_map(|l| Some((l.item_type, l.subtype?)))
        .collect();
    items.sort_unstable();
    items.dedup();
    for (item_type, subtype) in items {
        let total: usize = lines
            .iter()
            .filter(|l| l.item_type == item_type && l.subtype.is_none_or(|s| s == subtype))
            .map(|l| l.text.len() + 1)
            .sum();
        if total > ITEM_MAX {
            report.warn(format!("tooltip lines for item type {item_type} subtype {subtype} total more than {ITEM_MAX} characters; the game may drop its own hint lines"));
        }
    }
    lines
}

struct Hook {
    site: u32,
    /// Where the replaced `je` goes.
    leave: u32,
    expect: Vec<u8>,
}

fn find_hook(module: &ModuleBuild) -> Option<Hook> {
    let (text_va, text) = module.image.data(".text")?;
    let hit = only(SITE, text)?;
    let replaced = hit.get(1)?;
    let site = text_va + replaced.start() as u32;
    // the same function goes on to reference this hint
    let hinted = string_sites(module.image, module.refs, b"M1 EQUIP")
        .values()
        .flatten()
        .any(|s| s.kind == SiteKind::Lea && site < s.rva && s.rva < site + 0x400);
    hinted.then(|| Hook {
        site,
        leave: (site + 8).wrapping_add_signed(i32_at(&hit[2], 0)),
        expect: replaced.as_bytes().to_vec(),
    })
}

/// Plans all of the lines for one client DLL, or none. Returns how many that was.
pub fn plan(module: &mut ModuleBuild, lines: &[Line], report: &mut Report) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let Some(hook) = find_hook(module) else {
        report.warn(format!(
            "{}: could not locate the tooltip hint builder in this build; tooltip lines skipped",
            module.backend.key()
        ));
        return 0;
    };
    let texts: Vec<u32> = lines
        .iter()
        .map(|line| module.plan.intern(&line.text))
        .collect();
    let start = module.plan.reserve();
    let mut stub = Stub::new(STUB);
    stub.lea([0x4C, 0x8D, 0x0D], 0, stub.size()); // the table follows the code
    let (leave, resume) = (stub.at(CLIENT), stub.at(RETURN));
    let mut table = Vec::new();
    for (line, text) in lines.iter().zip(texts) {
        let entry = start as usize + stub.size() + table.len();
        table.extend_from_slice(&line.item_type.to_le_bytes());
        table.extend_from_slice(&line.subtype.unwrap_or(ANY).to_le_bytes());
        table.extend_from_slice(&(i64::from(text) - entry as i64).to_le_bytes()[..4]);
    }
    table.extend_from_slice(&ANY.to_le_bytes());
    let blob = &mut module.plan.blob;
    blob.extend(stub.into_bytes());
    blob.extend(table);
    blob.resize(blob.len() + BLOB_PAD, 0);

    let end = hook.site + hook.expect.len() as u32;
    module.plan.claims.push(Claim {
        start: hook.site,
        end,
        owner: "tooltip lines".into(),
    });
    let mut who: Vec<&str> = Vec::new();
    for line in lines {
        if !who.contains(&line.owner.as_str()) {
            who.push(&line.owner);
        }
    }
    module.plan.groups.push(Group {
        id: "tooltips".into(),
        label: format!("{} tooltip line(s) [{}]", lines.len(), who.join("+")),
        ops: vec![
            Op::BlobRel32 {
                blob_offset: start + leave,
                target: hook.leave,
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
    lines.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Backend;
    use crate::mods::tests::write_mod;
    use crate::mods::{load, resolve};
    use crate::plan::ModulePlan;
    use crate::refs::RefIndex;
    use crate::testing::TinyPe;

    fn collected(manifests: &[&str]) -> (Vec<Line>, Vec<Mod>, Report) {
        let dir = tempfile::tempdir().unwrap();
        for (n, manifest) in manifests.iter().enumerate() {
            write_mod(dir.path(), &format!("m{n}"), manifest);
        }
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let mut report = Report::default();
        let lines = collect(&mut mods, &order, &mut report);
        (lines, mods, report)
    }

    fn line(item_type: u32, subtype: Option<u32>, text: &str, owner: &str) -> Line {
        Line {
            item_type,
            subtype,
            text: text.as_bytes().to_vec(),
            owner: owner.into(),
        }
    }

    #[test]
    fn lines_of_every_mod_are_kept_in_load_order() {
        let (lines, mods, report) = collected(&[
            r#"{"schema": 1, "id": "a", "version": "1.0.0", "tooltips": [
                {"type": 95, "subtype": 40, "line": "TAMES COLLIE"},
                {"type": 95, "line": "PET FOOD"},
                {"type": 95, "subtype": 40, "line": ""},
                {"type": 95, "subtype": 40, "line": "TWO\nLINES"},
                {"subtype": 40, "line": "NO TYPE"},
                "nope"]}"#,
            r#"{"schema": 1, "id": "b", "version": "1.0.0", "tooltips": [
                {"type": 95, "subtype": 40, "line": "LIKES WALKS"},
                {"type": 95, "line": "PET FOOD"}]}"#,
        ]);
        assert_eq!(
            lines,
            [
                line(95, Some(40), "TAMES COLLIE", "a"),
                line(95, None, "PET FOOD", "a"),
                line(95, Some(40), "LIKES WALKS", "b"),
            ]
        );
        assert_eq!(mods[0].warnings.len(), 4);
        assert_eq!(mods[0].warnings[0].title, "Tooltip entry ignored");
        assert_eq!(report.warnings().count(), 0);
    }

    #[test]
    fn warns_when_an_item_gets_more_than_the_buffer_spares() {
        let long = "X".repeat(40);
        let manifest = format!(
            r#"{{"schema": 1, "id": "a", "version": "1.0.0", "tooltips": [
                {{"type": 1, "subtype": 2, "line": "{long}"}}, {{"type": 1, "line": "{long}"}},
                {{"type": 1, "subtype": 3, "line": "SHORT"}},
                {{"type": 1, "line": "{}"}}]}}"#,
            "Y".repeat(61)
        );
        let (lines, mods, report) = collected(&[&manifest]);
        assert_eq!((lines.len(), mods[0].warnings.len()), (3, 1));
        let warnings: Vec<&str> = report.warnings().collect();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("tooltip lines for item type 1 subtype 2 total"));
    }

    /// A client with the hint builder: the store of the NUL, the replaced test and jump,
    /// and the "M1 EQUIP" hint further on. Returns the image and the hook's place.
    fn client() -> (TinyPe, u32) {
        let mut pe = TinyPe::new();
        let hint = pe.rdata(b"M1 EQUIP\0");
        pe.code(&[0x90; 16]);
        pe.code(&[0x42, 0xC6, 0x04, 0x36, 0x00]);
        let site = pe.code(&[0x84, 0xDB, 0x0F, 0x84, 0x40, 0x01, 0x00, 0x00]);
        pe.code(&[0x48, 0x89, 0xF9, 0xB2, 0x01, 0xE8, 0, 0, 0, 0]);
        pe.code(&[0x90; 32]);
        pe.lea(hint);
        (pe, site)
    }

    #[test]
    fn hooks_the_hint_builder_and_points_the_table_at_the_lines() {
        let (pe, site) = client();
        let image = pe.build();
        let refs = RefIndex::build(&image).unwrap();
        let mut module = ModuleBuild {
            backend: Backend::Avx2,
            image: &image,
            refs: &refs,
            plan: ModulePlan::new(Backend::Avx2, image.timestamp, image.size_of_image),
        };
        let lines = [
            line(95, Some(40), "TAMES COLLIE", "pets"),
            line(95, None, "PET FOOD", "other"),
        ];
        let mut report = Report::default();
        assert_eq!(plan(&mut module, &lines, &mut report), 2);

        let plan = &module.plan;
        let group = &plan.groups[0];
        assert_eq!(
            (group.id.as_str(), group.label.as_str()),
            ("tooltips", "2 tooltip line(s) [pets+other]")
        );
        let Op::Hook {
            rva,
            expect,
            blob_offset: start,
        } = &group.ops[2]
        else {
            panic!("the hook comes last")
        };
        assert_eq!((*rva, expect.len()), (site, 8));
        let stub = Stub::new(STUB);
        assert_eq!(
            group.ops[..2],
            [
                Op::BlobRel32 {
                    blob_offset: start + stub.at(CLIENT),
                    target: site + 8 + 0x140
                },
                Op::BlobRel32 {
                    blob_offset: start + stub.at(RETURN),
                    target: site + 8
                },
            ]
        );
        assert_eq!(plan.claimed(site, site + 1).unwrap().owner, "tooltip lines");

        // lea r9 reaches the table, and each entry reaches its line
        let blob = &plan.blob;
        let code = *start as usize;
        let table = code + 7 + i32_at(blob, code + 3) as usize;
        assert_eq!(table, code + stub.size());
        let text_of = |entry: usize| {
            let at = (entry as i64 + i64::from(i32_at(blob, entry + 8))) as usize;
            let end = blob[at..].iter().position(|&b| b == 0).unwrap();
            String::from_utf8_lossy(&blob[at..at + end]).into_owned()
        };
        assert_eq!((i32_at(blob, table), i32_at(blob, table + 4)), (95, 40));
        assert_eq!(text_of(table), "TAMES COLLIE");
        assert_eq!(
            (i32_at(blob, table + 12), i32_at(blob, table + 16)),
            (95, -1)
        );
        assert_eq!(text_of(table + 12), "PET FOOD");
        assert_eq!(i32_at(blob, table + 24), -1);
    }

    #[test]
    fn a_build_without_the_place_is_skipped_and_reported() {
        let mut pe = TinyPe::new();
        pe.rdata(b"M1 EQUIP\0");
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
        let lines = [line(95, None, "PET FOOD", "pets")];
        assert_eq!(plan(&mut module, &lines, &mut report), 0);
        assert!(module.plan.groups.is_empty() && module.plan.blob.is_empty());
        assert!(
            report
                .warnings()
                .next()
                .unwrap()
                .starts_with("baseline: could not locate the tooltip hint builder")
        );
    }
}
