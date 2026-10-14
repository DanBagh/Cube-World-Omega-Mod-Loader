//! Reading the plan file. `cwoml-core` writes it, in `plan.rs` there.
//!
//! Tab-separated lines. A line that cannot be read spoils only the group it is in, and a
//! line of a kind this version does not know is skipped, so a newer launcher can add to
//! the format.

/// Plan file format this loader understands.
pub const PLAN_VERSION: u32 = 3;
/// A hook replaces at least a five-byte jump and at most this many bytes.
pub const MAX_HOOK: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Repoint a RIP-relative displacement from `old_target` to the blob.
    Rel32 {
        site: u32,
        old_target: u32,
        blob_offset: u32,
    },
    /// Repoint an 8-byte absolute pointer from `old_target` to the blob.
    Ptr64 {
        site: u32,
        old_target: u32,
        blob_offset: u32,
    },
    /// Replace bytes in the module.
    Bytes {
        rva: u32,
        expect: Vec<u8>,
        new: Vec<u8>,
    },
    /// Replace `expect` with a jump to the blob, padded with NOPs.
    Hook {
        rva: u32,
        expect: Vec<u8>,
        blob_offset: u32,
    },
    /// Point a rel32 inside the blob at an address in the module.
    BlobRel32 { blob_offset: u32, target: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Id and label, for the log.
    pub name: String,
    /// None when one of its lines could not be read. Such a group is skipped whole.
    pub ops: Option<Vec<Op>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub dll: String,
    /// Identity of the DLL the plan was made for.
    pub timestamp: u32,
    pub size_of_image: u32,
    /// None when the blob line is missing or is not as long as the module line says.
    pub blob: Option<Vec<u8>>,
    /// Zeroed read/write memory wanted on the pages right after the blob.
    pub data_size: u32,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// The game release it was made for.
    pub release: Option<String>,
    /// Write the data area to the log when the game exits.
    pub dump: bool,
    pub modules: Vec<Module>,
    /// Game-relative path, lowercase with forward slashes, to the file served in its place.
    pub redirects: Vec<(String, String)>,
}

fn hex(text: &str) -> Option<u32> {
    (!text.is_empty())
        .then(|| u32::from_str_radix(text, 16).ok())
        .flatten()
}

fn bytes(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    pairs
        .iter()
        .map(|&[high, low]| Some(digit(high)? << 4 | digit(low)?))
        .collect()
}

fn op(fields: &[&str]) -> Option<Op> {
    Some(match *fields {
        ["rel32", site, old_target, blob_offset, ..] => Op::Rel32 {
            site: hex(site)?,
            old_target: hex(old_target)?,
            blob_offset: hex(blob_offset)?,
        },
        ["ptr64", site, old_target, blob_offset, ..] => Op::Ptr64 {
            site: hex(site)?,
            old_target: hex(old_target)?,
            blob_offset: hex(blob_offset)?,
        },
        ["bytes", rva, expect, new, ..] => {
            let (expect, new) = (bytes(expect)?, bytes(new)?);
            if expect.is_empty() || expect.len() != new.len() {
                return None;
            }
            Op::Bytes {
                rva: hex(rva)?,
                expect,
                new,
            }
        }
        ["hook", rva, expect, blob_offset, ..] => {
            let expect = bytes(expect)?;
            if !(5..=MAX_HOOK).contains(&expect.len()) {
                return None;
            }
            Op::Hook {
                rva: hex(rva)?,
                expect,
                blob_offset: hex(blob_offset)?,
            }
        }
        ["blobrel32", blob_offset, target, ..] => Op::BlobRel32 {
            blob_offset: hex(blob_offset)?,
            target: hex(target)?,
        },
        _ => return None,
    })
}

/// Reads a plan. The error is a line for the log.
pub fn parse(text: &str) -> Result<Plan, &'static str> {
    let mut lines = text.lines().map(|line| line.trim_end_matches('\r'));
    let header: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
    if header.first() != Some(&"cwoplan")
        || header.get(1).and_then(|v| v.parse().ok()) != Some(PLAN_VERSION)
    {
        return Err("plan: unsupported header, nothing applied");
    }
    let mut plan = Plan::default();
    let mut in_group = false;
    let mut blob_size_wanted = None;
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        match *fields {
            ["game", release, ..] => plan.release = Some(release.to_owned()),
            ["dump", ..] => plan.dump = true,
            [
                "module",
                dll,
                timestamp,
                size_of_image,
                blob_size,
                ref rest @ ..,
            ] => {
                in_group = false;
                let number = |text: &str| text.parse::<u32>().ok();
                blob_size_wanted = number(blob_size);
                plan.modules.push(Module {
                    dll: dll.to_owned(),
                    // an identity that cannot be read is one no loaded DLL has
                    timestamp: hex(timestamp).unwrap_or(0),
                    size_of_image: hex(size_of_image).unwrap_or(0),
                    blob: None,
                    data_size: rest.first().and_then(|text| number(text)).unwrap_or(0),
                    groups: Vec::new(),
                });
            }
            ["blob", ref rest @ ..] => {
                if let Some(module) = plan.modules.last_mut() {
                    module.blob = bytes(rest.first().copied().unwrap_or_default())
                        .filter(|blob| Some(blob.len() as u32) == blob_size_wanted);
                }
            }
            ["group", ref rest @ ..] => {
                in_group = true;
                if let Some(module) = plan.modules.last_mut() {
                    module.groups.push(Group {
                        name: rest.join(" "),
                        ops: Some(Vec::new()),
                    });
                }
            }
            ["endgroup", ..] => in_group = false,
            ["endmodule", ..] => in_group = false,
            ["redirect", key, target, ..] => {
                plan.redirects.push((key.to_owned(), target.to_owned()))
            }
            ["rel32" | "ptr64" | "bytes" | "hook" | "blobrel32", ..] if in_group => {
                if let Some(group) = plan.modules.last_mut().and_then(|m| m.groups.last_mut()) {
                    match (op(&fields), &mut group.ops) {
                        (Some(op), Some(ops)) => ops.push(op),
                        _ => group.ops = None,
                    }
                }
            }
            _ => {}
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_launcher_writes() {
        use std::collections::BTreeMap;

        use cwoml_core::game::Backend;
        use cwoml_core::plan as written;

        let mut module = written::ModulePlan::new(Backend::Avx2, 0x6ACD_2DC2, 0x06ED_0000);
        let text = module.intern(b"RUPEE");
        module.data_size = 0x1000;
        module.groups.push(written::Group {
            id: "text:1".into(),
            label: "\"GOLD COIN\" -> \"RUPEE\"".into(),
            ops: vec![
                written::Op::Rel32 {
                    site: 0x1234,
                    old_target: 0x45_0000,
                    blob_offset: text,
                },
                written::Op::Ptr64 {
                    site: 0x5678,
                    old_target: 0x45_0000,
                    blob_offset: text,
                },
                written::Op::Bytes {
                    rva: 0x10,
                    expect: vec![0x20, 0x47],
                    new: vec![0x20, 0x52],
                },
                written::Op::Hook {
                    rva: 0xABC,
                    expect: vec![0x84, 0xDB, 0x0F, 0x84, 0x00],
                    blob_offset: 0x20,
                },
                written::Op::BlobRel32 {
                    blob_offset: 0x24,
                    target: 0xDEF,
                },
            ],
        });
        let blob = module.blob.clone();
        let plan = written::Plan {
            generator: "test".into(),
            game_release: "gh-1".into(),
            modules: vec![module],
            redirects: BTreeMap::from([("sprites/coin.cub".to_owned(), "C:/mods/coin.cub".into())]),
            dump: true,
        };

        let read = parse(&plan.to_text()).unwrap();
        assert_eq!((read.release.as_deref(), read.dump), (Some("gh-1"), true));
        assert_eq!(
            read.redirects,
            [("sprites/coin.cub".to_owned(), "C:/mods/coin.cub".to_owned())]
        );
        let module = &read.modules[0];
        assert_eq!(module.dll, Backend::Avx2.dll_name());
        assert_eq!(
            (module.timestamp, module.size_of_image, module.data_size),
            (0x6ACD_2DC2, 0x06ED_0000, 0x1000)
        );
        assert_eq!(module.blob.as_deref(), Some(&blob[..]));
        assert_eq!(module.groups[0].name, "text:1 \"GOLD COIN\" -> \"RUPEE\"");
        assert_eq!(
            module.groups[0].ops.as_deref().unwrap(),
            [
                Op::Rel32 {
                    site: 0x1234,
                    old_target: 0x45_0000,
                    blob_offset: text
                },
                Op::Ptr64 {
                    site: 0x5678,
                    old_target: 0x45_0000,
                    blob_offset: text
                },
                Op::Bytes {
                    rva: 0x10,
                    expect: vec![0x20, 0x47],
                    new: vec![0x20, 0x52]
                },
                Op::Hook {
                    rva: 0xABC,
                    expect: vec![0x84, 0xDB, 0x0F, 0x84, 0x00],
                    blob_offset: 0x20
                },
                Op::BlobRel32 {
                    blob_offset: 0x24,
                    target: 0xDEF
                },
            ]
        );
        // names the two crates have to agree on
        assert_eq!(PLAN_VERSION, written::PLAN_VERSION);
        assert_eq!(crate::PLAN_NAME, written::PLAN_NAME);
        assert_eq!(crate::HOME_FILE, cwoml_core::loader::HOME_FILE);
    }

    #[test]
    fn reads_every_kind_of_line() {
        let text = "cwoplan\t3\ngenerator\tcwoml 0.1.0\ngame\tgh-1\ndump\n\
            module\tcubeworld-avx2.dll\t6ACA4A12\t06EBA000\t4\t4096\nblob\t00ff10ab\n\
            group\ttext:1\ttwo words\n\
            rel32\t1234\t450000\t10\nptr64\t5678\t450000\t10\nbytes\t10\t2047\t2052\n\
            hook\tABC\t84db0f8400\t40\nblobrel32\t44\tDEF\nendgroup\nendmodule\n\
            redirect\tsprites/coin.cub\tC:\\mods\\coin.cub\nend\n";
        let plan = parse(text).unwrap();
        assert_eq!((plan.release.as_deref(), plan.dump), (Some("gh-1"), true));
        assert_eq!(
            plan.redirects,
            [(
                "sprites/coin.cub".to_owned(),
                "C:\\mods\\coin.cub".to_owned()
            )]
        );
        let module = &plan.modules[0];
        assert_eq!(
            (
                module.dll.as_str(),
                module.timestamp,
                module.size_of_image,
                module.data_size
            ),
            ("cubeworld-avx2.dll", 0x6ACA_4A12, 0x06EB_A000, 4096)
        );
        assert_eq!(module.blob.as_deref(), Some(&[0x00, 0xFF, 0x10, 0xAB][..]));
        assert_eq!(module.groups[0].name, "text:1 two words");
        assert_eq!(
            module.groups[0].ops.as_deref().unwrap(),
            [
                Op::Rel32 {
                    site: 0x1234,
                    old_target: 0x45_0000,
                    blob_offset: 0x10
                },
                Op::Ptr64 {
                    site: 0x5678,
                    old_target: 0x45_0000,
                    blob_offset: 0x10
                },
                Op::Bytes {
                    rva: 0x10,
                    expect: vec![0x20, 0x47],
                    new: vec![0x20, 0x52]
                },
                Op::Hook {
                    rva: 0xABC,
                    expect: vec![0x84, 0xDB, 0x0F, 0x84, 0x00],
                    blob_offset: 0x40
                },
                Op::BlobRel32 {
                    blob_offset: 0x44,
                    target: 0xDEF
                },
            ]
        );
    }

    #[test]
    fn refuses_another_format_version() {
        assert!(parse("cwoplan\t4\nend\n").is_err());
        assert!(parse("").is_err());
        assert!(parse("plan\t3\n").is_err());
        assert_eq!(parse("cwoplan\t3\r\nend\r\n"), Ok(Plan::default()));
    }

    #[test]
    fn a_bad_line_spoils_only_its_group() {
        let text = "cwoplan\t3\nmodule\ta.dll\t1\t2\t0\t0\nblob\t\n\
            group\tgood\nbytes\t10\t20\t21\nendgroup\n\
            group\tshort hook\nhook\t10\t84db\t40\nendgroup\n\
            group\tuneven\nbytes\t10\t2047\t20\nrel32\t1\t2\t3\nendgroup\n\
            group\tnot hex\nrel32\tzz\t2\t3\nendgroup\n\
            rel32\t1\t2\t3\n\
            group\tlater\nfuture-op\t1\nblobrel32\t4\t5\nendgroup\nendmodule\n";
        let plan = parse(text).unwrap();
        let read: Vec<(&str, Option<usize>)> = plan.modules[0]
            .groups
            .iter()
            .map(|g| (g.name.as_str(), g.ops.as_ref().map(Vec::len)))
            .collect();
        assert_eq!(
            read,
            [
                ("good", Some(1)),
                ("short hook", None),
                ("uneven", None),
                ("not hex", None),
                ("later", Some(1)),
            ]
        );
        assert_eq!(plan.modules[0].blob.as_deref(), Some(&[][..]));
    }
}
