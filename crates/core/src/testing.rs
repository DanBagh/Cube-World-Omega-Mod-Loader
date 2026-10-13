//! A small PE32+ image built in memory, so the analysis can be tested without the game.

use crate::pe::PeImage;

const SECTIONS: [(&str, u32); 5] = [
    (".text", 0x1000),
    (".rdata", 0x2000),
    (".data", 0x3000),
    (".pdata", 0x4000),
    (".reloc", 0x5000),
];
const HEADERS: usize = 0x400;
const FILE_ALIGN: usize = 0x200;

#[derive(Default)]
pub struct TinyPe {
    text: Vec<u8>,
    rdata: Vec<u8>,
    data: Vec<u8>,
    functions: Vec<(u32, u32)>,
    relocs: Vec<u32>,
}

impl TinyPe {
    pub const IMAGE_BASE: u64 = 0x1_8000_0000;
    pub const TIMESTAMP: u32 = 0x6ACA_4A12;
    pub const SIZE_OF_IMAGE: u32 = 0x6000;

    pub fn new() -> Self {
        Self::default()
    }

    /// Appends machine code and returns its RVA.
    pub fn code(&mut self, bytes: &[u8]) -> u32 {
        let rva = 0x1000 + self.text.len() as u32;
        self.text.extend_from_slice(bytes);
        rva
    }

    /// Appends read-only data and returns its RVA.
    pub fn rdata(&mut self, bytes: &[u8]) -> u32 {
        let rva = 0x2000 + self.rdata.len() as u32;
        self.rdata.extend_from_slice(bytes);
        rva
    }

    /// `lea rax, [rip + target]`. Returns the RVA of its displacement.
    pub fn lea(&mut self, target: u32) -> u32 {
        self.rip_relative(&[0x48, 0x8D, 0x05], target)
    }

    /// Any instruction ending in a RIP-relative displacement, e.g. `[0x0F, 0x10, 0x05]` for movups.
    pub fn rip_relative(&mut self, opcode: &[u8], target: u32) -> u32 {
        let site = self.code(opcode) + opcode.len() as u32;
        self.code(&target.wrapping_sub(site + 4).to_le_bytes());
        site
    }

    /// A relocated 8-byte pointer to `target` in .data, followed by `length` as the
    /// compiler lays out a string view. Returns the pointer's RVA.
    pub fn string_view(&mut self, target: u32, length: u64) -> u32 {
        let rva = self.pointer(target);
        self.data.extend_from_slice(&length.to_le_bytes());
        rva
    }

    /// A relocated 8-byte pointer to `target` in .data. Returns its RVA.
    pub fn pointer(&mut self, target: u32) -> u32 {
        let rva = 0x3000 + self.data.len() as u32;
        self.data
            .extend_from_slice(&(Self::IMAGE_BASE + u64::from(target)).to_le_bytes());
        self.relocs.push(rva);
        rva
    }

    /// Records a function in the exception directory.
    pub fn function(&mut self, begin: u32, end: u32) {
        self.functions.push((begin, end));
    }

    fn pdata(&self) -> Vec<u8> {
        let entry =
            |&(begin, end): &(u32, u32)| [begin.to_le_bytes(), end.to_le_bytes(), [0; 4]].concat();
        self.functions.iter().flat_map(entry).collect()
    }

    fn reloc(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut sites = self.relocs.clone();
        sites.sort_unstable();
        for page in sites.chunk_by(|a, b| a & !0xFFF == b & !0xFFF) {
            let mut entries: Vec<u16> = page
                .iter()
                .map(|rva| 0xA000 | (rva & 0xFFF) as u16)
                .collect();
            if entries.len() % 2 == 1 {
                entries.push(0); // padding entry
            }
            out.extend_from_slice(&(page[0] & !0xFFF).to_le_bytes());
            out.extend_from_slice(&(8 + 2 * entries.len() as u32).to_le_bytes());
            out.extend(entries.iter().flat_map(|e| e.to_le_bytes()));
        }
        out
    }

    pub fn build_bytes(&self) -> Vec<u8> {
        let contents = [
            self.text.clone(),
            self.rdata.clone(),
            self.data.clone(),
            self.pdata(),
            self.reloc(),
        ];
        assert!(
            contents.iter().all(|c| c.len() <= 0x1000),
            "a section outgrew its page"
        );
        let mut file = vec![0u8; HEADERS];
        let put = |file: &mut Vec<u8>, at: usize, bytes: &[u8]| {
            file[at..at + bytes.len()].copy_from_slice(bytes)
        };

        let nt = 0x40;
        put(&mut file, 0, b"MZ");
        put(&mut file, 0x3C, &(nt as u32).to_le_bytes());
        put(&mut file, nt, b"PE\0\0");
        put(&mut file, nt + 4, &0x8664u16.to_le_bytes());
        put(&mut file, nt + 6, &(SECTIONS.len() as u16).to_le_bytes());
        put(&mut file, nt + 8, &Self::TIMESTAMP.to_le_bytes());
        put(&mut file, nt + 20, &240u16.to_le_bytes());
        let optional = nt + 24;
        put(&mut file, optional, &0x20Bu16.to_le_bytes());
        put(&mut file, optional + 24, &Self::IMAGE_BASE.to_le_bytes());
        put(&mut file, optional + 32, &0x1000u32.to_le_bytes());
        put(&mut file, optional + 36, &(FILE_ALIGN as u32).to_le_bytes());
        put(&mut file, optional + 56, &Self::SIZE_OF_IMAGE.to_le_bytes());
        put(&mut file, optional + 60, &(HEADERS as u32).to_le_bytes());
        put(&mut file, optional + 108, &16u32.to_le_bytes());
        let dir = |index: usize| optional + 112 + 8 * index;
        put(
            &mut file,
            dir(3),
            &[
                0x4000u32.to_le_bytes(),
                (contents[3].len() as u32).to_le_bytes(),
            ]
            .concat(),
        );
        put(
            &mut file,
            dir(5),
            &[
                0x5000u32.to_le_bytes(),
                (contents[4].len() as u32).to_le_bytes(),
            ]
            .concat(),
        );

        for (i, ((name, va), content)) in SECTIONS.iter().zip(&contents).enumerate() {
            let header = optional + 240 + 40 * i;
            let raw_ptr = file.len();
            let raw_size = content.len().next_multiple_of(FILE_ALIGN);
            put(&mut file, header, name.as_bytes());
            put(&mut file, header + 8, &(content.len() as u32).to_le_bytes());
            put(&mut file, header + 12, &va.to_le_bytes());
            put(&mut file, header + 16, &(raw_size as u32).to_le_bytes());
            put(&mut file, header + 20, &(raw_ptr as u32).to_le_bytes());
            file.extend_from_slice(content);
            file.resize(raw_ptr + raw_size, 0);
        }
        file
    }

    pub fn build(&self) -> PeImage {
        PeImage::from_bytes(self.build_bytes(), "tiny.dll").unwrap()
    }
}
