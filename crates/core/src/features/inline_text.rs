//! Short text the compiler wrote into code as a constant, such as the " G" after a price.
//!
//! Two to four characters are appended to a text buffer with
//! `mov word [buf + len], ' G'` / `mov byte [buf + len + 2], 0`, so there is no string to
//! repoint. A replacement of the same length swaps the immediate. Another length, or a
//! separate form after a lone 1, needs the stores rebuilt in new code.
//!
//! Build-specific through and through: every step checks that the code around the store
//! has the one shape this was written for, and refuses anything else.

use memchr::memmem;

use super::stub::{RETURN, i32_at, signature};
use crate::pe::PeImage;
use crate::plan::Op;

/// A register-indirect ModRM operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Operand {
    /// The rm field and the SIB byte, which together name the registers.
    key: (u8, Option<u8>),
    disp: i32,
    /// Index of the byte after the operand.
    next: usize,
}

fn byte(code: &[u8], at: usize) -> u8 {
    code.get(at).copied().unwrap_or(0)
}

fn operand_at(code: &[u8], at: usize) -> Option<Operand> {
    if at + 6 >= code.len() {
        return None;
    }
    let (mode, rm) = (code[at] >> 6, code[at] & 7);
    if mode == 3 {
        return None;
    }
    let (mut next, mut sib) = (at + 1, None);
    if rm == 4 {
        sib = Some(code[next]);
        next += 1;
    }
    let disp = match mode {
        // absolute or RIP-relative: not an append to a text buffer
        0 if rm == 5 || sib.is_some_and(|s| s & 7 == 5) => return None,
        0 => 0,
        1 => {
            next += 1;
            i32::from(code[next - 1] as i8)
        }
        _ => {
            next += 4;
            i32_at(code, next - 4)
        }
    };
    Some(Operand {
        key: (rm, sib),
        disp,
        next,
    })
}

/// One place that writes the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineText {
    /// RVA of the immediate holding the characters.
    pub imm: u32,
    /// The stores of the characters and of the NUL behind them. None when the NUL is
    /// part of the immediate, which says nothing about how the text is appended.
    append: Option<Append>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Append {
    /// RVA of the first of the two stores, and just past the second.
    start: u32,
    end: u32,
    /// REX prefix of the stores, 0 if they have none.
    rex: u8,
    key: (u8, Option<u8>),
    /// Displacement of the characters.
    disp: i32,
}

/// Where `text`, two to four characters, is appended to a buffer as an immediate.
///
/// Recognised: a word store of two characters or a dword store of four with the store of
/// the NUL directly after or before it, and a dword store of three characters plus the
/// NUL. The NUL is what tells text from a number with the same bytes.
pub fn find(image: &PeImage, text: &[u8]) -> Vec<InlineText> {
    let n = text.len();
    let Some((text_va, code)) = image.data(".text").filter(|_| (2..=4).contains(&n)) else {
        return Vec::new();
    };
    let mut imm = text.to_vec();
    if n == 3 {
        imm.push(0);
    }
    let finder = memmem::Finder::new(&imm);
    let (mut found, mut from) = (Vec::new(), 0);
    // overlapping matches count, so step one byte past each hit
    while let Some(hit) = finder.find(&code[from..]) {
        let k = from + hit;
        from = k + 1;
        // 1 to 6 bytes of operand between the C7 opcode and the immediate
        for op in (1..=6).filter_map(|operand| k.checked_sub(operand + 1)) {
            if op < 2 || code[op] != 0xC7 {
                continue;
            }
            let Some(chars) = operand_at(code, op + 1).filter(|operand| operand.next == k) else {
                continue;
            };
            let rex = Some(code[op - 1])
                .filter(|b| (0x40..=0x47).contains(b))
                .unwrap_or(0);
            let mut start = op - usize::from(rex != 0);
            let word = code[start - 1] == 0x66;
            if word != (n == 2) {
                continue;
            }
            start -= usize::from(word);
            if n == 3 {
                found.push(InlineText {
                    imm: text_va + k as u32,
                    append: None,
                });
                break;
            }
            // the NUL behind the characters: [REX] C6 <same registers, disp + n> 00.
            // Gives the end of such a store whose C6 is at `at`
            let nul_store = |at: usize| {
                let nul = operand_at(code, at + 1).filter(|_| byte(code, at) == 0xC6)?;
                (nul.key == chars.key
                    && nul.disp == chars.disp + n as i32
                    && byte(code, nul.next) == 0)
                    .then_some(nul.next + 1)
            };
            let after = k + n + usize::from(rex != 0 && byte(code, k + n) == rex);
            let mut span = nul_store(after).map(|end| (start, end));
            if span.is_none() {
                // or the NUL was stored first
                span = (1..=6)
                    .filter_map(|size| start.checked_sub(2 + size).filter(|&at| at > 0))
                    .find(|&at| nul_store(at) == Some(start))
                    .map(|at| (at - usize::from(rex != 0 && code[at - 1] == rex), k + n));
            }
            if let Some((first, last)) = span {
                found.push(InlineText {
                    imm: text_va + k as u32,
                    append: Some(Append {
                        start: text_va + first as u32,
                        end: text_va + last as u32,
                        rex,
                        key: chars.key,
                        disp: chars.disp,
                    }),
                });
                break;
            }
        }
    }
    found
}

// The check in front of an append: lea r,[len+d] / cmp r,imm / jbe <overflow>, and
// sometimes a movzx eax,byte [rbp+x] after it
const CHECK: &str = r"[\x48\x49]\x8d[\x40-\x7f](.)(?:[\x48\x49]\x83[\xf8-\xff](.)|[\x48\x49]\x81[\xf8-\xff](....))(?:\x0f\x86....|\x76.)(?:\x0f\xb6\x85....)?\z";

/// (base register, index register, scale bits) of an operand.
fn base_index(key: (u8, Option<u8>), rex: u8) -> (u8, Option<u8>, u8) {
    match key {
        (rm, None) => (rm | ((rex & 1) << 3), None, 0),
        (_, Some(sib)) => {
            let index = ((sib >> 3) & 7) | ((rex & 2) << 2);
            (
                (sib & 7) | ((rex & 1) << 3),
                (index != 4).then_some(index),
                sib >> 6,
            )
        }
    }
}

/// Whether the code at `at` reads the length at [base + disp] back from memory and adds
/// `n` to it: `add qword [length], n`, or `mov r, [length]` and then `add r, n` on that
/// register or a copy of it, with only plain moves in between.
///
/// Reading it back is what lets the length be changed underneath: the code carries no
/// earlier copy of it past this point.
fn advances_length(code: &[u8], at: usize, n: u8, base: u8, disp: i32) -> bool {
    let is_length = |at: usize, rex: u8| {
        operand_at(code, at).filter(|operand| {
            let (register, index, _) = base_index(operand.key, rex);
            (register, index) == (base, None) && operand.disp == disp
        })
    };
    let rex = byte(code, at);
    if !(0x48..=0x4F).contains(&rex) {
        return false;
    }
    let (opcode, modrm) = (byte(code, at + 1), byte(code, at + 2));
    if opcode == 0x83 && (modrm >> 3) & 7 == 0 {
        // add qword [length], imm8
        return is_length(at + 2, rex).is_some_and(|operand| byte(code, operand.next) == n);
    }
    let Some(read) = is_length(at + 2, rex).filter(|_| opcode == 0x8B) else {
        return false;
    };
    // registers holding the length
    let mut holds = vec![((modrm >> 3) & 7) | ((rex & 4) << 1)];
    let mut at = read.next;
    for _ in 0..5 {
        let (rex, next, modrm) = (byte(code, at), byte(code, at + 1), byte(code, at + 2));
        let wide = (0x48..=0x4F).contains(&rex);
        if wide && matches!(next, 0x89 | 0x8B) && modrm >> 6 == 3 {
            // mov r64, r64
            let reg = ((modrm >> 3) & 7) | ((rex & 4) << 1);
            let rm = (modrm & 7) | ((rex & 1) << 3);
            let (source, target) = if next == 0x89 { (reg, rm) } else { (rm, reg) };
            if holds.contains(&source) {
                holds.push(target);
            } else {
                holds.retain(|&r| r != target);
            }
            at += 3;
        } else if wide && next == 0x83 && modrm >> 3 == 0x18 {
            // add r64, imm8
            return holds.contains(&((modrm & 7) | ((rex & 1) << 3))) && byte(code, at + 3) == n;
        } else if (0xB8..=0xBF).contains(&rex) {
            // mov r32, imm32
            holds.retain(|&r| r != rex & 7);
            at += 5;
        } else if rex == 0x41 && (0xB8..=0xBF).contains(&next) {
            holds.retain(|&r| r != (next & 7) | 8);
            at += 6;
        } else {
            return false;
        }
    }
    false
}

fn displacement(disp: i32) -> (u8, Vec<u8>) {
    match i8::try_from(disp) {
        Ok(short) => (1, vec![short as u8]),
        Err(_) => (2, disp.to_le_bytes().to_vec()),
    }
}

/// `add qword [base + disp], imm8`.
fn add_qword(base: u8, disp: i32, value: i8) -> Vec<u8> {
    let (mode, tail) = displacement(disp);
    let mut out = vec![0x48 | (base >> 3), 0x83, (mode << 6) | (base & 7)];
    if base & 7 == 4 {
        out.push(0x24);
    }
    out.extend(tail);
    out.push(value as u8);
    out
}

/// ModRM, SIB and displacement addressing the registers of `key` at another displacement.
fn operand(reg: u8, key: (u8, Option<u8>), disp: i32) -> Vec<u8> {
    let (rm, sib) = key;
    let (mode, tail) = if disp == 0 && rm != 5 && !sib.is_some_and(|s| s & 7 == 5) {
        (0, Vec::new())
    } else {
        displacement(disp)
    };
    let mut out = vec![(mode << 6) | (reg << 3) | rm];
    out.extend(sib);
    out.extend(tail);
    out
}

/// Stores of `text` and its NUL, four, two or one characters at a time.
fn stores(rex: u8, key: (u8, Option<u8>), disp: i32, text: &[u8]) -> Vec<u8> {
    let payload = [text, &[0]].concat();
    let (mut out, mut at) = (Vec::new(), 0);
    while at < payload.len() {
        let size = match payload.len() - at {
            4.. => 4,
            2.. => 2,
            _ => 1,
        };
        if size == 2 {
            out.push(0x66);
        }
        if rex != 0 {
            out.push(rex);
        }
        out.push(if size == 1 { 0xC6 } else { 0xC7 });
        out.extend(operand(0, key, disp + at as i32));
        out.extend_from_slice(&payload[at..at + size]);
        at += size;
    }
    out
}

enum Part {
    Code(Vec<u8>),
    Label(&'static str),
    /// A conditional jump to a label. Always the six-byte form, so nothing depends on
    /// distances.
    Jump(u8, &'static str),
    /// A jump back into the game.
    Return,
}

/// The code, and the offset of the rel32 of each jump back.
fn link(parts: &[Part]) -> (Vec<u8>, Vec<u32>) {
    let mut labels = Vec::new();
    let mut at = 0;
    for part in parts {
        match part {
            Part::Code(code) => at += code.len(),
            Part::Label(name) => labels.push((*name, at)),
            Part::Jump(..) => at += 6,
            Part::Return => at += 5,
        }
    }
    let (mut out, mut returns) = (Vec::new(), Vec::new());
    for part in parts {
        match part {
            Part::Code(code) => out.extend_from_slice(code),
            Part::Label(_) => {}
            Part::Jump(condition, name) => {
                let target = labels.iter().find(|(label, _)| label == name).unwrap().1;
                let relative = target as i32 - (out.len() as i32 + 6);
                out.extend_from_slice(&[0x0F, 0x80 | condition]);
                out.extend_from_slice(&relative.to_le_bytes());
            }
            Part::Return => {
                returns.push(out.len() as u32 + 1);
                out.push(0xE9);
                out.extend_from_slice(&[RETURN; 4]);
            }
        }
    }
    (out, returns)
}

// condition codes
const JB: u8 = 2;
const JAE: u8 = 3;
const JE: u8 = 4;
const JNE: u8 = 5;
const JA: u8 = 7;

/// What it takes to make one append write another text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resize {
    /// The buffer check, reserving room for the new text.
    pub check: Op,
    /// Code that replaces the stores.
    pub stub: Vec<u8>,
    /// Offsets in the stub of the rel32s that go back to `end`.
    pub returns: Vec<u32>,
    /// The stores being replaced.
    pub start: u32,
    pub end: u32,
    pub original: Vec<u8>,
    /// Characters the buffer holds, the NUL included.
    pub capacity: i32,
}

/// Makes the append at `site` write `new` where it wrote `old`, or says why it cannot.
///
/// The text sits in a `char[capacity]` followed by its length. The game checks there is
/// room, stores the characters and the NUL at the length, reads the length back and adds
/// the old size. Two things change. The check reserves room for the new text. The stores
/// become a jump to code that writes all of the new text and moves the stored length by
/// the difference, so the game's own add lands on the right total. That add is left
/// alone: the code after it works other things out from the length it read back, and
/// those stay true this way.
///
/// With `singular`, the new code first looks at what is in the buffer. If that ends in a
/// 1 that is a number of its own (nothing, or no digit, point or comma in front of it),
/// it writes `singular`.
pub fn resize(
    image: &PeImage,
    site: &InlineText,
    old: &[u8],
    new: &[u8],
    singular: Option<&[u8]>,
) -> Result<Resize, String> {
    let (text_va, code) = image.data(".text").ok_or("the client has no code")?;
    let append = site
        .append
        .as_ref()
        .ok_or("it is not written as an append")?;
    let n = old.len() as i32;
    // room is reserved for the longer form
    let m = new.len().max(singular.map_or(0, <[u8]>::len)) as i32;
    let (s, e) = (
        (append.start - text_va) as usize,
        (append.end - text_va) as usize,
    );
    let window = s.saturating_sub(40);
    let before = &code[window..s];
    let unusual = "no buffer check in the expected form in front of it";
    let check = signature(CHECK).captures(before).ok_or(unusual)?;
    let lea = check.get(1).ok_or(unusual)?;
    if before[lea.start() - 1] & 7 == 4 {
        return Err(unusual.into());
    }
    let d = i32::from(lea.as_bytes()[0] as i8);
    let limit = match (check.get(2), check.get(3)) {
        (Some(short), _) => i32::from(short.as_bytes()[0] as i8),
        (None, Some(long)) => i32_at(long.as_bytes(), 0),
        (None, None) => return Err(unusual.into()),
    };
    let capacity = n - d;
    if d >= 0 || limit != -capacity - 1 {
        return Err("the buffer check in front of it is not the usual one".into());
    }
    if m > capacity - 2 || !(-128..=-1).contains(&(d + (m - n))) {
        return Err(format!("the buffer only holds {capacity} characters"));
    }
    let (base, Some(index), 0) = base_index(append.key, append.rex) else {
        return Err("the buffer is not addressed as buffer + length".into());
    };
    // the length follows the characters
    let length_at = append.disp + capacity;
    if !advances_length(code, e, n as u8, base, length_at) {
        return Err("the length is not read back and advanced after it in the usual way".into());
    }

    let write = |text: &[u8]| {
        let mut code = stores(append.rex, append.key, append.disp, text);
        code.extend(add_qword(base, length_at, (text.len() as i32 - n) as i8));
        [Part::Code(code), Part::Return]
    };
    // cmp byte [buffer + length - back], char
    let compare = |back: i32, char: u8| {
        let mut code: Vec<u8> = Some(append.rex)
            .filter(|&rex| rex != 0)
            .into_iter()
            .collect();
        code.push(0x80);
        code.extend(operand(7, append.key, append.disp - back));
        code.push(char);
        Part::Code(code)
    };
    let mut parts = Vec::new();
    if let Some(singular) = singular {
        // cmp <length>, 1
        let length_is_one = || Part::Code(vec![0x48 | (index >> 3), 0x83, 0xF8 | (index & 7), 1]);
        parts.extend([
            length_is_one(),
            Part::Jump(JB, "plural"), // nothing in front of it
            compare(1, b'1'),
            Part::Jump(JNE, "plural"),
            length_is_one(),
            Part::Jump(JE, "singular"), // just "1"
            compare(2, b'9'),
            Part::Jump(JA, "singular"),
            compare(2, b','),
            Part::Jump(JAE, "plural"), // , - . / and the digits: the 1 ends a longer number
            Part::Label("singular"),
        ]);
        parts.extend(write(singular));
        parts.push(Part::Label("plural"));
    }
    parts.extend(write(new));
    let (stub, returns) = link(&parts);
    Ok(Resize {
        check: Op::Bytes {
            rva: text_va + (window + lea.start()) as u32,
            expect: lea.as_bytes().to_vec(),
            new: vec![(d + (m - n)) as i8 as u8],
        },
        stub,
        returns,
        start: append.start,
        end: append.end,
        original: code[s..e].to_vec(),
        capacity,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testing::TinyPe;

    /// Adds a price formatter's tail the way the avx2 client has it: the check, the two
    /// stores of " G" into a 32-character buffer at [rbx + r14 + 10h], and the length
    /// read back and advanced. Returns the RVAs of the check's displacement and the stores.
    pub(crate) fn append_g(pe: &mut TinyPe) -> (u32, u32) {
        pe.code(&[0x90; 8]);
        // lea rax,[r14-1Eh] / cmp rax,-21h / jbe +20h
        let check = pe.code(&[0x49, 0x8D, 0x46, 0xE2, 0x48, 0x83, 0xF8, 0xDF, 0x76, 0x20]) + 3;
        // mov word [rbx+r14+10h],' G' / mov byte [rbx+r14+12h],0
        let stores = pe.code(&[0x66, 0x42, 0xC7, 0x44, 0x33, 0x10, b' ', b'G']);
        pe.code(&[0x42, 0xC6, 0x44, 0x33, 0x12, 0x00]);
        // mov rax,[rbx+30h] / add rax,2 / mov [rbx+30h],rax
        pe.code(&[0x48, 0x8B, 0x43, 0x30, 0x48, 0x83, 0xC0, 0x02]);
        pe.code(&[0x48, 0x89, 0x43, 0x30, 0xC3]);
        pe.code(&[0x90; 16]);
        (check, stores)
    }

    #[test]
    fn finds_an_append_and_leaves_numbers_alone() {
        let mut pe = TinyPe::new();
        // the same two bytes as a plain number: mov word [rbx+10h], 4720h / ret
        pe.code(&[0x66, 0xC7, 0x43, 0x10, b' ', b'G', 0xC3]);
        let (_, stores) = append_g(&mut pe);
        // three characters carry their NUL in the immediate: mov dword [rcx+8], "OK!\0"
        pe.code(&[
            0xC7, 0x41, 0x08, b'O', b'K', b'!', 0x00, 0x90, 0x90, 0x90, 0x90,
        ]);
        let image = pe.build();
        let found = find(&image, b" G");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].imm, stores + 6);
        let append = found[0].append.as_ref().unwrap();
        assert_eq!((append.start, append.end), (stores, stores + 14));
        assert_eq!(
            (append.rex, append.key, append.disp),
            (0x42, (4, Some(0x33)), 0x10)
        );

        let three = find(&image, b"OK!");
        assert_eq!((three.len(), three[0].append.is_none()), (1, true));
        assert!(find(&image, b"LONGER").is_empty());
        assert!(resize(&image, &three[0], b"OK!", b"FINE", None).is_err());
    }

    #[test]
    fn a_longer_text_is_written_by_new_code_and_the_length_moved() {
        let mut pe = TinyPe::new();
        let (check, stores) = append_g(&mut pe);
        let image = pe.build();
        let site = &find(&image, b" G")[0];
        let change = resize(&image, site, b" G", b" RUPEES", None).unwrap();

        assert_eq!(change.capacity, 32);
        assert_eq!((change.start, change.end), (stores, stores + 14));
        assert_eq!(change.original.len(), 14);
        // room for seven characters where there was room for two
        assert_eq!(
            change.check,
            Op::Bytes {
                rva: check,
                expect: vec![0xE2],
                new: vec![0xE7]
            }
        );
        let expected = [
            // mov dword [rbx+r14+10h]," RUP" / mov dword [rbx+r14+14h],"EES\0"
            &[0x42, 0xC7, 0x44, 0x33, 0x10][..],
            b" RUP",
            &[0x42, 0xC7, 0x44, 0x33, 0x14],
            b"EES\0",
            // add qword [rbx+30h],5 / jmp back
            &[
                0x48, 0x83, 0x43, 0x30, 0x05, 0xE9, RETURN, RETURN, RETURN, RETURN,
            ],
        ]
        .concat();
        assert_eq!(change.stub, expected);
        assert_eq!(change.returns, [expected.len() as u32 - 4]);
    }

    #[test]
    fn a_singular_form_is_chosen_after_a_lone_one() {
        let mut pe = TinyPe::new();
        append_g(&mut pe);
        let image = pe.build();
        let site = &find(&image, b" G")[0];
        let change = resize(&image, site, b" G", b" RUPEES", Some(b" RUPEE")).unwrap();
        let stub = &change.stub;
        // cmp r14,1 / jb plural / cmp byte [rbx+r14+0Fh],'1' / jne plural
        assert!(stub.starts_with(&[0x49, 0x83, 0xFE, 0x01, 0x0F, 0x82]));
        assert_eq!(stub[10..16], [0x42, 0x80, 0x7C, 0x33, 0x0F, b'1']);
        assert_eq!(change.returns.len(), 2);
        // the singular writer comes first: mov word [rbx+r14+14h],"EE" ends before its jump back
        let singular = [0x66, 0x42, 0xC7, 0x44, 0x33, 0x14, b'E', b'E'];
        let written = stub.windows(8).position(|w| w == singular).unwrap();
        assert!(written < change.returns[0] as usize);
        // the first jb lands on the plural writer, which is the tail of the stub
        let plural = 10 + i32_at(stub, 6) as usize;
        assert_eq!(
            stub[plural..plural + 9],
            [0x42, 0xC7, 0x44, 0x33, 0x10, b' ', b'R', b'U', b'P']
        );
        // the singular is one shorter than the plural, so the length moves by four
        assert!(stub.windows(5).any(|w| w == [0x48, 0x83, 0x43, 0x30, 0x04]));
    }

    #[test]
    fn refuses_what_does_not_fit_or_has_another_shape() {
        let mut pe = TinyPe::new();
        append_g(&mut pe);
        let image = pe.build();
        let site = &find(&image, b" G")[0];
        let too_long = [b'X'; 31];
        assert_eq!(
            resize(&image, site, b" G", &too_long, None).unwrap_err(),
            "the buffer only holds 32 characters"
        );

        // without the check in front
        let mut pe = TinyPe::new();
        pe.code(&[0x90; 48]);
        pe.code(&[0x66, 0x42, 0xC7, 0x44, 0x33, 0x10, b' ', b'G']);
        pe.code(&[0x42, 0xC6, 0x44, 0x33, 0x12, 0x00, 0xC3]);
        pe.code(&[0x90; 16]);
        let image = pe.build();
        let site = &find(&image, b" G")[0];
        assert!(
            resize(&image, site, b" G", b" RUPEES", None)
                .unwrap_err()
                .starts_with("no buffer check")
        );
    }
}
