//! Machine code the plan carries into the game, and how features find their place in a build.
//!
//! Each trampoline is assembled ahead of time from the `.asm` of the same name in `asm/`.
//! What is only known per build is left in it as four identical marker bytes, filled in
//! here or resolved by the in-game loader.

use regex::bytes::{Captures, Regex};
use serde_json::{Map, Value};

/// Marker for a rel32 the in-game loader points back into the client.
pub const RETURN: u8 = 0xDD;
/// Marker for a second rel32 into the client: a call or the other way out.
pub const CLIENT: u8 = 0xCC;
/// Marker for a rel32 to the plan's writable data area.
pub const DATA: u8 = 0xEE;

/// A byte signature: a regex over raw bytes, where `.` is any byte.
pub fn signature(pattern: &str) -> Regex {
    Regex::new(&format!("(?s-u){pattern}")).expect("signature")
}

/// The one place `pattern` matches in `haystack`. None if there is none, or more than one.
pub fn only<'h>(pattern: &str, haystack: &'h [u8]) -> Option<Captures<'h>> {
    let regex = signature(pattern);
    let mut hits = regex.captures_iter(haystack);
    let hit = hits.next()?;
    hits.next().is_none().then_some(hit)
}

pub fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The item a table entry is for: its type, and a subtype unless it stands for all of them.
pub fn item_key(patch: &Map<String, Value>) -> Result<(u32, Option<u32>), String> {
    let number = |key: &str| match patch.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|&n| n < u64::from(u32::MAX))
            .map(|n| Some(n as u32))
            .ok_or_else(|| format!("{key} must be a whole number below {}", u32::MAX)),
    };
    let item_type = number("type")?.ok_or("type is missing")?;
    Ok((item_type, number("subtype")?))
}

/// What a table holds for "any subtype".
pub const ANY: u32 = u32::MAX;

pub struct Stub {
    template: Vec<u8>,
    code: Vec<u8>,
}

impl Stub {
    pub fn new(hex: &str) -> Stub {
        let digit = |c: u8| (c as char).to_digit(16).expect("hex digit") as u8;
        let template: Vec<u8> = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[high, low]| digit(high) << 4 | digit(low))
            .collect();
        Stub {
            code: template.clone(),
            template,
        }
    }

    /// Length of the code. Tables a feature appends start here.
    pub fn size(&self) -> usize {
        self.code.len()
    }

    /// Offset of the first placeholder made of `marker`.
    pub fn at(&self, marker: u8) -> u32 {
        self.places(marker)[0] as u32
    }

    fn places(&self, marker: u8) -> Vec<usize> {
        let (mut found, mut at) = (Vec::new(), 0);
        while at + 4 <= self.template.len() {
            if self.template[at..at + 4] == [marker; 4] {
                found.push(at);
                at += 4;
            } else {
                at += 1;
            }
        }
        assert!(!found.is_empty(), "no {marker:#x} placeholder");
        found
    }

    /// Writes `value` over every placeholder made of `marker`.
    pub fn fill(&mut self, marker: u8, value: i32) {
        for at in self.places(marker) {
            self.code[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    /// Points the `nth` `lea reg, [rip + 0]` with these opcode bytes at `target`, an
    /// offset from the start of the code. Tables follow the code, so it may lie past it.
    pub fn lea(&mut self, opcode: [u8; 3], nth: usize, target: usize) {
        let pattern = [&opcode[..], &[0; 4]].concat();
        let at = self
            .template
            .windows(7)
            .enumerate()
            .filter(|(_, bytes)| *bytes == pattern)
            .map(|(at, _)| at + 3)
            .nth(nth)
            .expect("lea placeholder");
        let relative = target as i64 - (at as i64 + 4);
        self.code[at..at + 4].copy_from_slice(&(relative as i32).to_le_bytes());
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_placeholders_and_table_references() {
        // lea r9,[rip+0] / cmp eax,[rdi+22222222h] / jmp DDDDDDDD / cmp eax,[rdi+22222222h]
        let mut stub = Stub::new("4c8d0d000000003b8722222222e9dddddddd3b8722222222");
        assert_eq!((stub.at(RETURN), stub.at(0x22)), (14, 9));
        stub.fill(0x22, 0x9C);
        stub.lea([0x4C, 0x8D, 0x0D], 0, 24);
        let code = stub.into_bytes();
        assert_eq!(i32_at(&code, 3), 24 - 7);
        assert_eq!((i32_at(&code, 9), i32_at(&code, 20)), (0x9C, 0x9C));
        assert_eq!(code[14..18], [RETURN; 4]);
    }

    #[test]
    fn reads_the_item_of_an_entry() {
        let entry = |text: &str| serde_json::from_str::<Map<String, Value>>(text).unwrap();
        assert_eq!(item_key(&entry(r#"{"type": 95}"#)), Ok((95, None)));
        assert_eq!(
            item_key(&entry(r#"{"type": 95, "subtype": 40}"#)),
            Ok((95, Some(40)))
        );
        assert!(item_key(&entry(r#"{"subtype": 40}"#)).is_err());
        assert!(item_key(&entry(r#"{"type": -1}"#)).is_err());
        assert!(item_key(&entry(r#"{"type": "95"}"#)).is_err());
    }
}
