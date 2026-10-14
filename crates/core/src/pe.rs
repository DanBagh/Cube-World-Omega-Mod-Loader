//! Just enough PE32+ parsing to map addresses, list relocations and scan sections.
//!
//! Addresses are RVAs (offsets from the image base) throughout.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use memchr::memmem;

use crate::{Error, Result};

const PE32_PLUS: u16 = 0x20B;
const DIR_EXCEPTION: usize = 3;
const DIR_BASERELOC: usize = 5;
const RELOC_DIR64: u16 = 10;

#[derive(Debug)]
pub struct Section {
    pub name: String,
    pub va: u32,
    pub virtual_size: u32,
    raw_ptr: u32,
    raw_size: u32,
    // read on first use: the client carries a 100 MB section nothing here needs
    data: OnceLock<Vec<u8>>,
}

impl Section {
    /// Bytes of the section that exist in the file.
    fn backed(&self) -> u32 {
        self.virtual_size.min(self.raw_size)
    }
}

#[derive(Debug)]
enum Source {
    File(PathBuf),
    Memory(Vec<u8>),
}

#[derive(Debug)]
pub struct PeImage {
    source: Source,
    pub name: String,
    pub timestamp: u32,
    pub image_base: u64,
    pub size_of_image: u32,
    dirs: Vec<(u32, u32)>,
    pub sections: Vec<Section>,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

impl PeImage {
    pub fn open(path: &Path) -> Result<Self> {
        let mut head = vec![0; 0x1000];
        let mut file = File::open(path).map_err(|e| Error::io(path, e))?;
        let got = file.read(&mut head).map_err(|e| Error::io(path, e))?;
        head.truncate(got);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::parse(&head, name, Source::File(path.to_path_buf()))
    }

    /// An image held in memory, as the tests build them.
    pub fn from_bytes(bytes: Vec<u8>, name: &str) -> Result<Self> {
        let head = bytes[..bytes.len().min(0x1000)].to_vec();
        Self::parse(&head, name.to_owned(), Source::Memory(bytes))
    }

    fn parse(head: &[u8], name: String, source: Source) -> Result<Self> {
        let bad = |what: &str| Error::invalid(format!("{name} {what}"));
        let nt = u32_at(head, 0x3C)
            .filter(|_| head.starts_with(b"MZ"))
            .ok_or_else(|| bad("is not a PE file"))? as usize;
        if head.get(nt..nt + 4) != Some(b"PE\0\0") {
            return Err(bad("is not a PE file"));
        }
        let truncated = || bad("has a truncated header");
        let section_count = u16_at(head, nt + 6).ok_or_else(truncated)? as usize;
        let timestamp = u32_at(head, nt + 8).ok_or_else(truncated)?;
        let optional_size = u16_at(head, nt + 20).ok_or_else(truncated)? as usize;
        let optional = nt + 24;
        if u16_at(head, optional) != Some(PE32_PLUS) {
            return Err(bad("is not a 64-bit image"));
        }
        let image_base = u64_at(head, optional + 24).ok_or_else(truncated)?;
        let size_of_image = u32_at(head, optional + 56).ok_or_else(truncated)?;
        let dir_count = u32_at(head, optional + 108).ok_or_else(truncated)? as usize;
        let mut dirs = Vec::with_capacity(dir_count.min(16));
        for i in 0..dir_count.min(16) {
            let at = optional + 112 + 8 * i;
            dirs.push((
                u32_at(head, at).ok_or_else(truncated)?,
                u32_at(head, at + 4).ok_or_else(truncated)?,
            ));
        }
        let mut sections = Vec::with_capacity(section_count);
        for i in 0..section_count {
            let at = optional + optional_size + 40 * i;
            let raw_name = head.get(at..at + 8).ok_or_else(truncated)?;
            let end = raw_name.iter().position(|&b| b == 0).unwrap_or(8);
            sections.push(Section {
                name: String::from_utf8_lossy(&raw_name[..end]).into_owned(),
                virtual_size: u32_at(head, at + 8).ok_or_else(truncated)?,
                va: u32_at(head, at + 12).ok_or_else(truncated)?,
                raw_size: u32_at(head, at + 16).ok_or_else(truncated)?,
                raw_ptr: u32_at(head, at + 20).ok_or_else(truncated)?,
                data: OnceLock::new(),
            });
        }
        Ok(PeImage {
            source,
            name,
            timestamp,
            image_base,
            size_of_image,
            dirs,
            sections,
        })
    }

    fn load<'a>(&'a self, section: &'a Section) -> &'a [u8] {
        section.data.get_or_init(|| {
            let (start, len) = (section.raw_ptr as usize, section.backed() as usize);
            match &self.source {
                Source::Memory(bytes) => bytes
                    .get(start..start + len)
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default(),
                Source::File(path) => {
                    let mut data = vec![0; len];
                    let read = File::open(path).and_then(|mut file| {
                        file.seek(SeekFrom::Start(start as u64))?;
                        file.read_exact(&mut data)
                    });
                    // a section that cannot be read is an empty one: nothing is found in it
                    if read.is_ok() { data } else { Vec::new() }
                }
            }
        })
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// A section's RVA and its bytes.
    pub fn data(&self, name: &str) -> Option<(u32, &[u8])> {
        let section = self.section(name)?;
        Some((section.va, self.load(section)))
    }

    /// `len` bytes at `rva`, if one section holds all of them in the file.
    pub fn read(&self, rva: u32, len: usize) -> Option<&[u8]> {
        let section = self
            .sections
            .iter()
            .find(|s| s.va <= rva && rva - s.va < s.backed())?;
        let at = (rva - section.va) as usize;
        self.load(section).get(at..at + len)
    }

    /// Start of the function containing `rva`, from the exception directory.
    pub fn function_start(&self, rva: u32) -> Option<u32> {
        let &(dir, size) = self.dirs.get(DIR_EXCEPTION)?;
        let table = self.read(dir, size as usize)?;
        table.as_chunks::<12>().0.iter().find_map(|entry| {
            let (begin, end) = (u32_at(entry, 0)?, u32_at(entry, 4)?);
            (begin <= rva && rva < end).then_some(begin)
        })
    }

    /// RVAs of every 8-byte absolute pointer the OS loader relocates.
    pub fn reloc_sites(&self) -> Vec<u32> {
        let mut sites = Vec::new();
        let Some(&(dir, size)) = self.dirs.get(DIR_BASERELOC) else {
            return sites;
        };
        let Some(table) = self.read(dir, size as usize) else {
            return sites;
        };
        let mut at = 0;
        while at + 8 <= table.len() {
            let (Some(page), Some(block)) = (u32_at(table, at), u32_at(table, at + 4)) else {
                break;
            };
            let block = block as usize;
            if block < 8 || at + block > table.len() {
                break;
            }
            for entry in table[at + 8..at + block].as_chunks::<2>().0 {
                let entry = u16::from_le_bytes(*entry);
                if entry >> 12 == RELOC_DIR64 {
                    sites.push(page + u32::from(entry & 0xFFF));
                }
            }
            at += block;
        }
        sites
    }

    /// RVAs of every occurrence of `needle` in the named sections, in that order.
    pub fn find_bytes(&self, needle: &[u8], sections: &[&str]) -> Vec<u32> {
        let finder = memmem::Finder::new(needle);
        let mut found = Vec::new();
        for name in sections {
            if let Some((va, data)) = self.data(name) {
                let mut from = 0;
                // overlapping matches count, so step one byte past each hit
                while let Some(hit) = finder.find(&data[from..]) {
                    found.push(va + (from + hit) as u32);
                    from += hit + 1;
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TinyPe;

    #[test]
    fn rejects_files_that_are_not_64_bit_images() {
        assert!(PeImage::from_bytes(b"hello".to_vec(), "x").is_err());
        let mut pe = TinyPe::new().build_bytes();
        let optional = u32_at(&pe, 0x3C).unwrap() as usize + 24;
        pe[optional..optional + 2].copy_from_slice(&0x10Bu16.to_le_bytes());
        assert!(
            PeImage::from_bytes(pe, "x")
                .unwrap_err()
                .to_string()
                .contains("64-bit")
        );
    }

    #[test]
    fn maps_addresses_and_finds_bytes() {
        let mut pe = TinyPe::new();
        let hello = pe.rdata(b"HELLO\0");
        let again = pe.rdata(b"SAY HELLO\0");
        let image = pe.build();
        assert_eq!(image.read(hello, 6), Some(&b"HELLO\0"[..]));
        assert_eq!(
            image.find_bytes(b"HELLO\0", &[".rdata", ".data"]),
            [hello, again + 4]
        );
        assert_eq!(image.read(0x7000_0000, 1), None);
        assert_eq!(
            (image.timestamp, image.size_of_image),
            (TinyPe::TIMESTAMP, TinyPe::SIZE_OF_IMAGE)
        );
    }

    #[test]
    fn lists_relocated_pointers_and_function_starts() {
        let mut pe = TinyPe::new();
        let text = pe.rdata(b"X\0");
        let pointer = pe.pointer(text);
        let function = pe.code(&[0x90; 32]);
        pe.function(function, function + 32);
        let image = pe.build();
        assert_eq!(image.reloc_sites(), [pointer]);
        assert_eq!(image.function_start(function + 5), Some(function));
        assert_eq!(image.function_start(function + 32), None);
    }

    #[test]
    fn opens_from_a_file() {
        let mut pe = TinyPe::new();
        let hello = pe.rdata(b"HELLO\0");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("client.dll");
        std::fs::write(&path, pe.build_bytes()).unwrap();
        let image = PeImage::open(&path).unwrap();
        assert_eq!(image.name, "client.dll");
        assert_eq!(image.read(hello, 5), Some(&b"HELLO"[..]));
    }
}
