//! The plan: exact memory patches for each client DLL, written to a file the in-game
//! loader reads at startup.
//!
//! The loader allocates a blob of new strings, code and tables near the module and applies
//! groups of operations against it. Every operation names the bytes it expects to replace,
//! and a group is applied only if all of its operations match, so a plan made for another
//! build of the game does nothing instead of doing harm.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::game::Backend;
use crate::{Error, Result, wine};

/// Plan file format the in-game loader understands.
pub const PLAN_VERSION: u32 = 3;
pub const PLAN_NAME: &str = "plan.txt";
/// Zero bytes after everything placed in the blob, so code that still carries the length
/// of a longer original string reads NULs and not a neighbor's text.
pub const BLOB_PAD: usize = 16;

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
    /// Replace `expect` with a jump to a handler of the in-game loader, which runs before
    /// or `after` the replaced instructions. They run elsewhere, so they must not be
    /// RIP-relative or jump.
    Context {
        rva: u32,
        expect: Vec<u8>,
        after: bool,
        handler: String,
        data: Vec<u8>,
    },
}

/// Operations that are applied together or not at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub ops: Vec<Op>,
}

/// A module range a group rewrites, so two patches cannot land on the same bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub start: u32,
    pub end: u32,
    pub owner: String,
}

#[derive(Debug)]
pub struct ModulePlan {
    pub backend: Backend,
    /// Identity of the DLL the plan was made for.
    pub timestamp: u32,
    pub size_of_image: u32,
    pub blob: Vec<u8>,
    pub groups: Vec<Group>,
    pub claims: Vec<Claim>,
    /// Zeroed read/write memory the loader places on the pages right after the blob.
    pub data_size: u32,
    data_refs: Vec<(u32, u32)>,
    interned: HashMap<Vec<u8>, u32>,
}

impl ModulePlan {
    pub fn new(backend: Backend, timestamp: u32, size_of_image: u32) -> Self {
        ModulePlan {
            backend,
            timestamp,
            size_of_image,
            blob: Vec::new(),
            groups: Vec::new(),
            claims: Vec::new(),
            data_size: 0,
            data_refs: Vec::new(),
            interned: HashMap::new(),
        }
    }

    /// Offset of `text` as a NUL-terminated string in the blob, adding it if it is new.
    pub fn intern(&mut self, text: &[u8]) -> u32 {
        if let Some(&offset) = self.interned.get(text) {
            return offset;
        }
        if self.blob.is_empty() {
            self.blob.resize(BLOB_PAD, 0); // offset 0 stays unused
        }
        let offset = self.blob.len() as u32;
        self.blob.extend_from_slice(text);
        self.blob.resize(self.blob.len() + BLOB_PAD, 0);
        self.interned.insert(text.to_vec(), offset);
        offset
    }

    /// Offset where the next 16-byte aligned chunk of code or tables will start.
    pub fn reserve(&mut self) -> u32 {
        if self.blob.is_empty() {
            self.blob.resize(BLOB_PAD, 0);
        }
        self.blob.resize(self.blob.len().next_multiple_of(16), 0);
        self.blob.len() as u32
    }

    /// Records a rel32 in the blob that must reach `data_offset` in the data area.
    pub fn data_ref(&mut self, blob_offset: u32, data_offset: u32) {
        self.data_refs.push((blob_offset, data_offset));
    }

    /// Resolves the data references. Call once nothing more will be added to the blob.
    pub fn finalize(&mut self) {
        let data_start = self.blob.len().next_multiple_of(0x1000) as i64;
        for &(blob_offset, data_offset) in &self.data_refs {
            let at = blob_offset as usize;
            let relative = data_start + i64::from(data_offset) - (i64::from(blob_offset) + 4);
            self.blob[at..at + 4].copy_from_slice(&(relative as i32).to_le_bytes());
        }
    }

    /// Who already rewrites any byte of `start..end`, if anyone.
    pub fn claimed(&self, start: u32, end: u32) -> Option<&Claim> {
        self.claims.iter().find(|c| start < c.end && c.start < end)
    }
}

#[derive(Debug)]
pub struct Plan {
    /// Named in the file, so it shows up in the loader's log.
    pub generator: String,
    pub game_release: String,
    pub modules: Vec<ModulePlan>,
    /// Game-relative path, lowercase with forward slashes, to the file served in its place.
    pub redirects: BTreeMap<String, PathBuf>,
    /// Ask the loader to write its data area to the log on exit.
    pub dump: bool,
    pub scripts: Scripts,
}

/// What the in-game loader's script runtime runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scripts {
    /// The language the strings are in.
    pub language: String,
    /// Each mod's scripts in load order: the mod's id and the script's file.
    pub files: Vec<(String, PathBuf)>,
    /// Each mod's strings by key: in `language` where the mod has it, else in English.
    pub texts: BTreeMap<String, BTreeMap<String, String>>,
    /// The menus mods declare.
    pub menus: Vec<Menu>,
}

/// A menu a mod declares, as the in-game loader needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Menu {
    pub owner: String,
    pub id: String,
    pub slot: String,
    /// Its key's SDL scancode.
    pub key: Option<u16>,
    /// What the controls menu calls it.
    pub label: String,
}

/// A value as a plan line carries it: no tabs or line breaks of its own.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

impl Plan {
    /// The plan as the tab-separated text the in-game loader parses.
    pub fn to_text(&self) -> String {
        let flat = |text: &str, replacement: &str| {
            text.split_whitespace()
                .collect::<Vec<_>>()
                .join(replacement)
        };
        let mut out = format!(
            "cwoplan\t{PLAN_VERSION}\ngenerator\t{}\ngame\t{}\n",
            self.generator, self.game_release
        );
        if self.dump {
            out.push_str("dump\n");
        }
        for module in &self.modules {
            let data_size = if module.blob.is_empty() {
                0
            } else {
                module.data_size
            };
            let _ = writeln!(
                out,
                "module\t{}\t{:08X}\t{:08X}\t{}\t{data_size}",
                module.backend.dll_name(),
                module.timestamp,
                module.size_of_image,
                module.blob.len()
            );
            let _ = writeln!(out, "blob\t{}", hex(&module.blob));
            for group in &module.groups {
                let _ = writeln!(
                    out,
                    "group\t{}\t{}",
                    flat(&group.id, "_"),
                    flat(&group.label, " ")
                );
                for op in &group.ops {
                    let _ = match op {
                        Op::Rel32 {
                            site,
                            old_target,
                            blob_offset,
                        } => writeln!(out, "rel32\t{site:X}\t{old_target:X}\t{blob_offset:X}"),
                        Op::Ptr64 {
                            site,
                            old_target,
                            blob_offset,
                        } => writeln!(out, "ptr64\t{site:X}\t{old_target:X}\t{blob_offset:X}"),
                        Op::Bytes { rva, expect, new } => {
                            writeln!(out, "bytes\t{rva:X}\t{}\t{}", hex(expect), hex(new))
                        }
                        Op::Hook {
                            rva,
                            expect,
                            blob_offset,
                        } => writeln!(out, "hook\t{rva:X}\t{}\t{blob_offset:X}", hex(expect)),
                        Op::BlobRel32 {
                            blob_offset,
                            target,
                        } => writeln!(out, "blobrel32\t{blob_offset:X}\t{target:X}"),
                        Op::Context {
                            rva,
                            expect,
                            after,
                            handler,
                            data,
                        } => writeln!(
                            out,
                            "context\t{rva:X}\t{}\t{}\t{handler}\t{}",
                            hex(expect),
                            if *after { "after" } else { "before" },
                            hex(data)
                        ),
                    };
                }
                out.push_str("endgroup\n");
            }
            out.push_str("endmodule\n");
        }
        for (key, source) in &self.redirects {
            let _ = writeln!(out, "redirect\t{key}\t{}", wine::game_path(source));
        }
        let scripts = &self.scripts;
        if !scripts.files.is_empty() || !scripts.texts.is_empty() {
            let _ = writeln!(out, "language\t{}", scripts.language);
        }
        for (owner, file) in &scripts.files {
            let _ = writeln!(out, "script\t{owner}\t{}", wine::game_path(file));
        }
        for menu in &scripts.menus {
            let key = menu.key.map_or_else(|| "-".to_owned(), |k| k.to_string());
            let _ = writeln!(
                out,
                "menu\t{}\t{}\t{}\t{key}\t{}",
                menu.owner,
                menu.id,
                menu.slot,
                escape(&menu.label)
            );
        }
        for (owner, texts) in &scripts.texts {
            for (key, text) in texts {
                let _ = writeln!(out, "text\t{owner}\t{key}\t{}", escape(text));
            }
        }
        out.push_str("end\n");
        out
    }

    /// Writes the plan so the loader never sees half a file.
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, self.to_text()).map_err(|e| Error::io(&temporary, e))?;
        fs::rename(&temporary, path).map_err(|e| Error::io(path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_stored_once_and_padded() {
        let mut plan = ModulePlan::new(Backend::Avx2, 1, 2);
        let first = plan.intern(b"FAIRY FLOSS");
        assert_eq!(first, 16);
        assert_eq!(plan.intern(b"FAIRY FLOSS"), first);
        let second = plan.intern(b"X");
        assert_eq!(second as usize, 16 + 11 + BLOB_PAD);
        assert_eq!(plan.blob.len(), second as usize + 1 + BLOB_PAD);
        assert_eq!(
            &plan.blob[first as usize..first as usize + 12],
            b"FAIRY FLOSS\0"
        );
    }

    #[test]
    fn reserved_chunks_are_aligned() {
        let mut plan = ModulePlan::new(Backend::Avx2, 1, 2);
        plan.intern(b"abc");
        let chunk = plan.reserve();
        assert_eq!((chunk % 16, chunk as usize), (0, plan.blob.len()));
    }

    #[test]
    fn data_references_reach_the_page_after_the_blob() {
        let mut plan = ModulePlan::new(Backend::Avx2, 1, 2);
        let start = plan.reserve();
        plan.blob
            .extend_from_slice(&[0x4C, 0x8D, 0x15, 0xEE, 0xEE, 0xEE, 0xEE]);
        plan.data_ref(start + 3, 0x20);
        plan.finalize();
        let relative = i32::from_le_bytes(plan.blob[19..23].try_into().unwrap());
        assert_eq!(
            i64::from(start) + 3 + 4 + i64::from(relative),
            0x1000 + 0x20
        );
    }

    #[test]
    fn claims_overlap_by_range() {
        let mut plan = ModulePlan::new(Backend::Avx2, 1, 2);
        plan.claims.push(Claim {
            start: 10,
            end: 20,
            owner: "a".into(),
        });
        assert!(plan.claimed(19, 30).is_some());
        assert!(plan.claimed(20, 30).is_none());
    }

    #[test]
    fn writes_the_format_the_loader_parses() {
        let mut module = ModulePlan::new(Backend::Avx2, 0x6ACA_4A12, 0x06EB_A000);
        let offset = module.intern(b"HI");
        module.data_size = 0x1000;
        module.groups.push(Group {
            id: "text:1".into(),
            label: "two\twords".into(),
            ops: vec![
                Op::Rel32 {
                    site: 0x1234,
                    old_target: 0x45_0000,
                    blob_offset: offset,
                },
                Op::Ptr64 {
                    site: 0x5678,
                    old_target: 0x45_0000,
                    blob_offset: offset,
                },
                Op::Bytes {
                    rva: 0x10,
                    expect: vec![0x20, 0x47],
                    new: vec![0x20, 0x52],
                },
                Op::Hook {
                    rva: 0xABC,
                    expect: vec![0x84, 0xDB],
                    blob_offset: 0x40,
                },
                Op::BlobRel32 {
                    blob_offset: 0x44,
                    target: 0xDEF,
                },
            ],
        });
        let plan = Plan {
            generator: "test 1".into(),
            game_release: "gh-1".into(),
            modules: vec![module],
            redirects: BTreeMap::from([("sprites/coin.cub".to_owned(), PathBuf::from("coin.cub"))]),
            dump: true,
            scripts: Scripts::default(),
        };
        let blob = format!("{}4849{}", "00".repeat(16), "00".repeat(16));
        let expected = format!(
            "cwoplan\t3\ngenerator\ttest 1\ngame\tgh-1\ndump\n\
             module\tcubeworld-avx2.dll\t6ACA4A12\t06EBA000\t34\t4096\nblob\t{blob}\n\
             group\ttext:1\ttwo words\n\
             rel32\t1234\t450000\t10\nptr64\t5678\t450000\t10\nbytes\t10\t2047\t2052\n\
             hook\tABC\t84db\t40\nblobrel32\t44\tDEF\nendgroup\nendmodule\n\
             redirect\tsprites/coin.cub\tcoin.cub\nend\n"
        );
        assert_eq!(plan.to_text(), expected);
    }

    #[test]
    fn a_module_without_a_blob_gets_no_data_area() {
        let mut module = ModulePlan::new(Backend::Baseline, 1, 2);
        module.data_size = 0x1000;
        let plan = Plan {
            generator: "t".into(),
            game_release: "r".into(),
            modules: vec![module],
            redirects: BTreeMap::new(),
            dump: false,
            scripts: Scripts::default(),
        };
        assert!(
            plan.to_text()
                .contains("module\tcubeworld-baseline.dll\t00000001\t00000002\t0\t0\nblob\t\n")
        );
    }
}
