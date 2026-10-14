//! Who points at what in a client image: RIP-relative code references and relocated
//! data pointers. No disassembler is involved.

use std::collections::{BTreeMap, HashMap};

use crate::pe::PeImage;
use crate::{Error, Result};

/// One RIP-relative reference found in code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeRef {
    /// RVA of the 4-byte displacement.
    pub site: u32,
    /// The bytes in front of it are `REX.W 8D modrm`, a `lea r64, [rip + disp32]`. Random
    /// bytes essentially never imitate that while also landing exactly on a string, so
    /// only these count as safe to repoint.
    pub lea: bool,
}

#[derive(Debug, Default)]
pub struct RefIndex {
    /// Target RVA to the code references that reach it.
    pub code: HashMap<u32, Vec<CodeRef>>,
    /// Target RVA to the RVAs of 8-byte pointers holding it.
    pub data: HashMap<u32, Vec<u32>>,
}

impl RefIndex {
    pub fn build(image: &PeImage) -> Result<Self> {
        let missing = |name: &str| Error::invalid(format!("{} has no {name} section", image.name));
        let (text_va, text) = image.data(".text").ok_or_else(|| missing(".text"))?;
        let rdata = image.section(".rdata").ok_or_else(|| missing(".rdata"))?;
        let data = image.section(".data").ok_or_else(|| missing(".data"))?;
        let low = rdata.va.min(data.va);
        let high = (rdata.va + rdata.virtual_size).max(data.va + data.virtual_size);
        let mut index = RefIndex::default();

        // every byte that could be a ModRM for [rip + disp32]: mod 00, rm 101, any reg
        for (at, _) in text
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte & 0xC7 == 0x05)
        {
            let disp = at + 1;
            let Some(bytes) = text.get(disp..disp + 4) else {
                break;
            };
            let offset = i32::from_le_bytes(bytes.try_into().unwrap());
            let target = (text_va + disp as u32 + 4).wrapping_add_signed(offset);
            if low <= target && target < high {
                let lea = disp >= 3 && text[disp - 2] == 0x8D && text[disp - 3] & 0xF8 == 0x48;
                index.code.entry(target).or_default().push(CodeRef {
                    site: text_va + disp as u32,
                    lea,
                });
            }
        }

        for site in image.reloc_sites() {
            let Some(bytes) = image.read(site, 8) else {
                continue;
            };
            let pointer = u64::from_le_bytes(bytes.try_into().unwrap());
            if let Some(target) = pointer
                .checked_sub(image.image_base)
                .and_then(|t| u32::try_from(t).ok())
                && low <= target
                && target < high
            {
                index.data.entry(target).or_default().push(site);
            }
        }
        Ok(index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    /// A `lea` in code. Its displacement can be repointed.
    Lea,
    /// A relocated pointer in a data table. It can be repointed.
    Data,
    /// Some other instruction reading the string's bytes in place. It cannot.
    Inline,
}

/// One place that uses a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    pub kind: SiteKind,
    pub rva: u32,
    /// The string's length also appears to be written down beside the reference.
    pub sized: bool,
}

/// Every address holding `needle` as a C string that something references, with the
/// references to exactly that address.
///
/// Tails of longer strings count: the linker merges "CANDY" into the end of
/// "EUCALYPTUS CANDY", and the two are still referenced separately.
pub fn string_sites(image: &PeImage, index: &RefIndex, needle: &[u8]) -> BTreeMap<u32, Vec<Site>> {
    let mut terminated = needle.to_vec();
    terminated.push(0);
    let mut found = BTreeMap::new();
    for rva in image.find_bytes(&terminated, &[".rdata", ".data"]) {
        let mut sites = Vec::new();
        for reference in index.code.get(&rva).into_iter().flatten() {
            sites.push(if reference.lea {
                Site {
                    kind: SiteKind::Lea,
                    rva: reference.site,
                    sized: has_baked_length(image, reference.site, needle.len()),
                }
            } else {
                Site {
                    kind: SiteKind::Inline,
                    rva: reference.site,
                    sized: true,
                }
            });
        }
        for &site in index.data.get(&rva).into_iter().flatten() {
            // a string view in a table: the pointer, then the length
            let sized = image
                .read(site + 8, 8)
                .is_some_and(|b| u64::from_le_bytes(b.try_into().unwrap()) == needle.len() as u64);
            sites.push(Site {
                kind: SiteKind::Data,
                rva: site,
                sized,
            });
        }
        if !sites.is_empty() {
            found.insert(rva, sites);
        }
    }
    found
}

/// Whether code around a `lea` looks like it also hard-codes the string's length.
///
/// The compiler builds a string view as a pointer plus an immediate length. This looks for
/// `mov r32, imm32` or `mov r/m, imm32` carrying exactly that length shortly before or after
/// the `lea`. It errs towards saying yes, which only makes text patches stricter.
fn has_baked_length(image: &PeImage, site: u32, length: usize) -> bool {
    let Some((text_va, text)) = image.data(".text") else {
        return false;
    };
    if length == 0 || site < text_va {
        return false;
    }
    let at = (site - text_va) as usize;
    let start = at.saturating_sub(3 + 24);
    let window = &text[start..text.len().min(at + 4 + 40)];
    let immediate = (length as u32).to_le_bytes();
    window
        .windows(4)
        .enumerate()
        .filter(|(_, bytes)| *bytes == immediate)
        .any(|(k, _)| {
            // the lea's own displacement does not count
            if start + k < at + 4 && at < start + k + 4 {
                return false;
            }
            let mov_r32 = k >= 1 && (0xB8..=0xBF).contains(&window[k - 1]);
            let mov_rm = (2..8).any(|back| {
                k >= back && window[k - back] == 0xC7 && window[k - back + 1] & 0x38 == 0
            });
            mov_r32 || mov_rm
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TinyPe;

    fn kinds(sites: &[Site]) -> Vec<(SiteKind, bool)> {
        sites.iter().map(|s| (s.kind, s.sized)).collect()
    }

    #[test]
    fn finds_lea_and_table_references() {
        let mut pe = TinyPe::new();
        let name = pe.rdata(b"BUBBLE GUM\0");
        let lea = pe.lea(name);
        pe.code(&[0xC3]);
        let pointer = pe.pointer(name);
        let image = pe.build();
        let index = RefIndex::build(&image).unwrap();

        let found = string_sites(&image, &index, b"BUBBLE GUM");
        assert_eq!(found.keys().copied().collect::<Vec<_>>(), [name]);
        let sites = &found[&name];
        assert_eq!(
            (sites[0].kind, sites[0].rva, sites[0].sized),
            (SiteKind::Lea, lea, false)
        );
        assert_eq!(
            (sites[1].kind, sites[1].rva, sites[1].sized),
            (SiteKind::Data, pointer, false)
        );
    }

    #[test]
    fn a_merged_tail_is_its_own_string() {
        let mut pe = TinyPe::new();
        let long = pe.rdata(b"EUCALYPTUS CANDY\0");
        pe.lea(long);
        pe.lea(long + 11);
        let image = pe.build();
        let index = RefIndex::build(&image).unwrap();

        let candy = string_sites(&image, &index, b"CANDY");
        assert_eq!(candy.keys().copied().collect::<Vec<_>>(), [long + 11]);
        assert_eq!(candy[&(long + 11)].len(), 1);
        assert_eq!(
            string_sites(&image, &index, b"EUCALYPTUS CANDY")[&long].len(),
            1
        );
    }

    #[test]
    fn unreferenced_strings_are_left_out() {
        let mut pe = TinyPe::new();
        pe.rdata(b"NOBODY\0");
        pe.code(&[0x90; 16]);
        let image = pe.build();
        let index = RefIndex::build(&image).unwrap();
        assert!(string_sites(&image, &index, b"NOBODY").is_empty());
    }

    #[test]
    fn notices_a_length_written_beside_the_reference() {
        let mut pe = TinyPe::new();
        let name = pe.rdata(b"OFFLINE\0");
        pe.lea(name);
        pe.code(&[0xBA, 7, 0, 0, 0]); // mov edx, 7
        let view = pe.string_view(name, 7);
        let image = pe.build();
        let index = RefIndex::build(&image).unwrap();
        let sites = &string_sites(&image, &index, b"OFFLINE")[&name];
        assert_eq!(
            kinds(sites),
            [(SiteKind::Lea, true), (SiteKind::Data, true)]
        );
        assert_eq!(sites[1].rva, view);
    }

    #[test]
    fn other_instructions_reading_a_string_are_inline_uses() {
        let mut pe = TinyPe::new();
        let name = pe.rdata(b"saves/singleplayer-v1\0");
        pe.rip_relative(&[0x0F, 0x10, 0x05], name); // movups xmm0, [rip + name]
        let image = pe.build();
        let index = RefIndex::build(&image).unwrap();
        let sites = &string_sites(&image, &index, b"saves/singleplayer-v1")[&name];
        assert_eq!(kinds(sites), [(SiteKind::Inline, true)]);
    }
}
