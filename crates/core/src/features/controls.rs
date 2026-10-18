//! Mods' menus in the game's controls menu, each by its key after the game's own actions,
//! and rebound there like them.
//!
//! The controls menu shows 8 of its actions at a time and scrolls through the rest. Every
//! menu with a key adds a row after them: the scroll limit and the scrollbar's arithmetic
//! are patched for the longer list. The menu marks the row being rebound by its action,
//! and none by the number of actions, which is the first row of mods, so that mark moves
//! past them. Hooks give the new rows their names and keys, keep the keys the player gives
//! them, fill them as the menu opens and resets, save them as it saves, and spread a drag
//! of the scrollbar over every row. See the in-game loader's `controls`.
//!
//! Build-specific: every site is read from the controls menu's own code.

use regex::bytes::Captures;

use super::ModuleBuild;
use super::stub::signature;
use crate::plan::{Claim, Group, Op};

// mov edx,[rcx+scroll] / add edx,row / cmp dl,last / ... mov [rbp+N],edx / ... ja default:
// the action of a row, which the draw keeps on its stack, then its label
const SWITCH: &str = r"\x8b[\x80-\xbf](....)[\x01\x03][\xc0-\xff]\x80\xfa(.).{0,16}?\x89(?:\x55(.)|\x95(....)).{0,16}?\x0f\x87(....)";
// mov r12d,length / lea rdx,[text] / jmp: the label of an action it does not know, and
// where every label goes
const DEFAULT: &str = r"\x41\xbc....\x48\x8d\x15....(?:\xe9(....)|\xeb(.))";
// there, the store of xmm9 that starts the label's string, its length from r12, and its
// capacity: the label is copied into the string itself, so it can be no longer
const MERGE: &str = r"((?:\xc5\x79\x7f\x4d.|\x66\x44\x0f\x7f\x4d.|\x44\x0f\x29\x4d.))\x4c\x89(?:\x65.|\xa5....)\x48\xc7(?:\x45.|\x85....)\x0f\x00\x00\x00";
/// The longest label the controls menu takes.
pub const LABEL: usize = 15;
// movzx eax,action / mov rcx,[rbp+N] / lea rdx,[rcx+rax*8] / lea rcx,[rbp+M] / call: the
// name of a row's key, from its binding
const KEY: &str = r"(\x40\x0f\xb6[\xc0-\xff]\x48\x8b\x4d.)\x48\x8d\x14\xc1\x48\x8d\x4d.\xe8";
// mov byte [reg+rebind],none / cmp byte [reg+rebind],none / movzx eax,byte [reg+rebind]
// ... cmp al or rax,none: the mark of no row being rebound. NONE stands for it
const NONE_SET: &str = r"\xc6[\x80-\x87](....)(NONE)";
const NONE_TEST: &str = r"\x80[\xb8-\xbf](....)(NONE)";
const NONE_READ: &str = r"\x0f\xb6[\x80-\xbf](....).{0,16}?(?:\x3c|\x48\x83\xf8)(NONE)";
/// The mark moved past the rows: still a positive signed byte, for `cmp rax,imm8`.
const NONE: u8 = 0x7f;
// mov [rcx+rax*8+table],reg: a key or a button stored for the row being rebound
const STORE: &str = r"([\x48\x4c]\x89([\x84\x8c\x94\x9c\xa4\xac\xb4\xbc])\xc1(....))";
// mov byte [reg+rebind],none ... mov byte [reg+open],1: the menu opening, where its
// prologue is hooked
const OPEN: &str = r"\xc6[\x80-\x87](....)NONE.{0,48}?\xc6[\x80-\x87](....)\x01";
const PROLOGUE: &str = r"((?:\x41?[\x50-\x57])*\x48(?:\x83\xec.|\x81\xec....))";
// mov r8,r14 / call save / mov ebx,eax / mov rax,[rbp+N]: the menu saving the bindings,
// with al whether it could
const SAVE: &str = r"\x4d\x89\xf0\xe8....(\x89\xc3\x48\x8b\x45.)";
// mov qword [reg+one but last],binding / mov qword [reg+last],binding: the client making
// the default bindings. PREV and LAST stand for the offsets
const DEFAULTS: &str = r"\x48\xc7[\x80-\x84\x86\x87]PREV....(\x48\xc7[\x80-\x84\x86\x87]LAST....)";
// cmp reg,last / mov eax,last / cmovl or cmovb: the scroll kept to its limit
const CLAMP: &str = r"\x83[\xf8-\xff](.)\xb8(.)\x00\x00\x00\x0f[\x42\x4c][\xc0-\xff]";
// (v)mulss xmmN,[limit] / (v)addss xmmN,[0.5] ... the limit ... cmp eax,[reg+scroll]: the
// scroll a drag of the scrollbar comes to, and xmmN still holding it before the limit
const DRAG: &str = r"(?:\xc5([\x80-\xff])\x59|\xf3\x0f\x59)([\x05-\x3d])(....)(?:\xc5[\x80-\xff]\x58|\xf3\x0f\x58)[\x05-\x3d](....).{0,64}?\x83[\xf8-\xff](.)\xb8(.)\x00\x00\x00\x0f[\x42\x4c][\xc0-\xff](\x3b[\x80-\xbf](....))";
// movsxd rax,[reg+scroll] / imul rax,reg,84 / add rax,half / maybe mov reg,magic, then a
// multiply by it: the scroll times 84 over the limit, rounded, where the scrollbar's thumb
// is. The divide is a multiply by the magic and a shift right of 66 in all
const THUMB: &str = r"\x48\x63[\x80-\xbf](....)\x48\x6b[\xc0-\xff]\x54\x48\x83\xc0(.)(?:[\x48\x49][\xb8-\xbf](.{8}))?";
/// How close to the scroll's offset a limit must be to be the controls menu's.
const NEAR: usize = 0x40;
/// The longest the list may grow to, for a limit that is still a signed byte.
const MOST: u32 = 127;

/// Where the controls menu does what the rows of mods need.
#[derive(Debug, PartialEq)]
struct Found {
    actions: u8,
    /// The scroll's limit: the actions there are, less the rows shown.
    limit: u8,
    /// Where the draw keeps a row's action: rbp plus this.
    slot: i32,
    label: (u32, Vec<u8>),
    key: (u32, Vec<u8>),
    /// Where the mark of no row is, to move.
    nones: Vec<u32>,
    /// Each store of a binding, and the register with it, and where the table is.
    stores: Vec<((u32, Vec<u8>), u8)>,
    table: u32,
    open: (u32, Vec<u8>),
    save: (u32, Vec<u8>),
    defaults: Vec<(u32, Vec<u8>)>,
    /// Each drag, and the xmm register with its scroll as a float.
    drags: Vec<((u32, Vec<u8>), u8)>,
    clamps: Vec<u32>,
    /// Where the thumb's arithmetic adds half the limit, and where it loads the magic.
    halves: Vec<u32>,
    magics: Vec<u32>,
}

/// The one item there is, if there is just one.
fn only<T>(items: Vec<T>) -> Option<T> {
    (items.len() == 1)
        .then(|| items.into_iter().next())
        .flatten()
}

/// What dividing by `limit` multiplies by, for a shift right of 66 after.
fn magic(limit: u32) -> u64 {
    (1u128 << 66).div_ceil(u128::from(limit)) as u64
}

fn find(module: &ModuleBuild) -> Option<Found> {
    let image = module.image;
    let (text_va, text) = image.data(".text")?;
    let rva = |at: usize| text_va + at as u32;
    let rel = |at: usize, len: usize| -> Option<u32> {
        let relative = match len {
            1 => i32::from(*text.get(at)? as i8),
            _ => i32::from_le_bytes(text.get(at..at + 4)?.try_into().ok()?),
        };
        Some(rva(at + len).wrapping_add_signed(relative))
    };
    let u32_of = |hit: &Captures, group: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            hit.get(group)?.as_bytes().try_into().ok()?,
        ))
    };
    let replaced = |hit: &Captures, group: usize| {
        let found = hit.get(group)?;
        Some((rva(found.start()), found.as_bytes().to_vec()))
    };
    let float = |at: u32| Some(f32::from_le_bytes(image.read(at, 4)?.try_into().ok()?));

    let switch = only(signature(SWITCH).captures_iter(text).collect())?;
    let scroll = u32_of(&switch, 1)?;
    let actions = switch.get(2)?.as_bytes()[0].checked_add(1)?;
    let slot = match (switch.get(3), switch.get(4)) {
        (Some(near), _) => i32::from(near.as_bytes()[0] as i8),
        (_, Some(far)) => i32::from_le_bytes(far.as_bytes().try_into().ok()?),
        _ => return None,
    };
    let default = rel(switch.get(5)?.start(), 4)?.checked_sub(text_va)? as usize;
    let jump = signature(DEFAULT).captures(text.get(default..default + 0x20)?)?;
    let merge = match (jump.get(1), jump.get(2)) {
        (Some(far), _) => rel(default + far.start(), 4)?,
        (_, Some(near)) => rel(default + near.start(), 1)?,
        _ => return None,
    }
    .checked_sub(text_va)? as usize;
    let start = signature(MERGE).captures(text.get(merge..merge + 0x20)?)?;
    let first = start.get(1)?;
    let label = (first.start() == 0).then(|| (rva(merge), first.as_bytes().to_vec()))?;

    let key = only(signature(KEY).captures_iter(text).collect())?;
    let key = replaced(&key, 1)?;
    // the mark of no row, the one before the scroll, and the table before that
    let rebind = scroll.checked_sub(8)?;
    let table = rebind.checked_sub(u32::from(actions) * 8)?;
    let none = format!(r"\x{actions:02x}");
    let mut nones = Vec::new();
    for pattern in [NONE_SET, NONE_TEST, NONE_READ] {
        let pattern = pattern.replace("NONE", &none);
        for hit in signature(&pattern).captures_iter(text) {
            if u32_of(&hit, 1)? == rebind {
                nones.push(rva(hit.get(2)?.start()));
            }
        }
    }
    let stores: Vec<_> = signature(STORE)
        .captures_iter(text)
        .filter(|h| u32_of(h, 3) == Some(table))
        .filter_map(|h| {
            let whole = h.get(1)?.as_bytes();
            let high = if whole[0] == 0x4c { 8 } else { 0 };
            Some((replaced(&h, 1)?, (h.get(2)?.as_bytes()[0] >> 3) & 7 | high))
        })
        .collect();
    let mouse = stores
        .iter()
        .map(|s| image.function_start(s.0.0))
        .collect::<Vec<_>>();
    let open_flag = rebind.checked_add(4)?;
    let opening = signature(&OPEN.replace("NONE", &none))
        .captures_iter(text)
        .filter(|h| u32_of(h, 1) == Some(rebind) && u32_of(h, 2) == Some(open_flag))
        .filter_map(|h| image.function_start(rva(h.get(0)?.start())))
        .collect();
    let opening = only(opening)?.checked_sub(text_va)? as usize;
    let prologue = signature(PROLOGUE).captures(text.get(opening..opening + 0x10)?)?;
    let prologue = prologue.get(1).filter(|p| p.start() == 0 && p.len() >= 5)?;
    let open = (rva(opening), prologue.as_bytes().to_vec());
    let save = only(
        signature(SAVE)
            .captures_iter(text)
            .filter_map(|h| {
                let start = image.function_start(rva(h.get(0)?.start()));
                mouse.contains(&start).then(|| replaced(&h, 1))?
            })
            .collect(),
    )?;
    let offset = |n: u32| {
        n.to_le_bytes()
            .iter()
            .map(|b| format!(r"\x{b:02x}"))
            .collect::<String>()
    };
    let last = u32::from(actions - 1) * 8;
    let pattern = DEFAULTS
        .replace("PREV", &offset(last - 8))
        .replace("LAST", &offset(last));
    let defaults: Vec<_> = signature(&pattern)
        .captures_iter(text)
        .filter_map(|h| replaced(&h, 1))
        .collect();
    if nones.is_empty() || stores.is_empty() || defaults.is_empty() {
        return None;
    }

    let mut limit = None;
    let mut drags = Vec::new();
    for hit in signature(DRAG).captures_iter(text) {
        let modrm = hit.get(2)?.as_bytes()[0];
        if modrm & 0xc7 != 0x05 || u32_of(&hit, 8)? != scroll {
            continue;
        }
        // each constant is RIP-relative, its displacement ending the instruction
        let constant = |group: usize| float(rel(hit.get(group)?.start(), 4)?);
        let high = hit.get(1).is_some_and(|vex| vex.as_bytes()[0] & 0x80 == 0);
        let xmm = (modrm >> 3) & 7 | if high { 8 } else { 0 };
        let (cap, also) = (hit.get(5)?.as_bytes()[0], hit.get(6)?.as_bytes()[0]);
        if cap != also || constant(3)? != f32::from(cap) || constant(4)? != 0.5 {
            return None;
        }
        if *limit.get_or_insert(cap) != cap {
            return None;
        }
        drags.push((replaced(&hit, 7)?, xmm));
    }
    let limit = limit?;

    let scrolled = scroll.to_le_bytes();
    let near_scroll = |start: usize, end: usize| {
        let around = &text[start.saturating_sub(NEAR)..text.len().min(end + NEAR)];
        around.windows(4).any(|w| w == scrolled)
    };
    let clamps: Vec<u32> = signature(CLAMP)
        .captures_iter(text)
        .filter_map(|h| {
            let whole = h.get(0)?;
            let caps = (h.get(1)?.as_bytes()[0], h.get(2)?.as_bytes()[0]);
            (caps == (limit, limit) && near_scroll(whole.start(), whole.end()))
                .then(|| rva(whole.start()))
        })
        .collect();
    // the drags' own limits are among them, and one more for the wheel at least
    if clamps.len() <= drags.len() {
        return None;
    }

    let (mut halves, mut magics) = (Vec::new(), Vec::new());
    for hit in signature(THUMB).captures_iter(text) {
        if u32_of(&hit, 1)? != scroll {
            continue;
        }
        if hit.get(2)?.as_bytes()[0] != limit / 2 {
            return None;
        }
        halves.push(rva(hit.get(2)?.start()));
        if let Some(loaded) = hit.get(3) {
            if loaded.as_bytes() != magic(limit.into()).to_le_bytes() {
                return None;
            }
            magics.push(rva(loaded.start()));
        }
    }
    if halves.is_empty() || magics.is_empty() {
        return None;
    }
    Some(Found {
        actions,
        limit,
        slot,
        label,
        key,
        nones,
        stores,
        table,
        open,
        save,
        defaults,
        drags,
        clamps,
        halves,
        magics,
    })
}

/// Plans `rows` more rows in the controls menu, one for each menu with a key, in the
/// order the plan lists them. False if this build's controls menu is not as expected.
pub fn plan(module: &mut ModuleBuild, rows: usize) -> bool {
    let Some(found) = find(module) else {
        return false;
    };
    let limit = u32::from(found.limit) + rows as u32;
    if limit > MOST {
        return false;
    }
    let (old, new) = (u32::from(found.limit), limit as u8);
    let mut ops = Vec::new();
    for &at in &found.clamps {
        let bytes = |cap: u8| {
            let mut b = module
                .image
                .read(at, 8)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
            if b.len() == 8 {
                (b[2], b[4]) = (cap, cap);
            }
            b
        };
        ops.push((at, bytes(found.limit), bytes(new)));
    }
    for &at in &found.halves {
        ops.push((at, vec![found.limit / 2], vec![new / 2]));
    }
    for &at in &found.magics {
        let (from, to) = (magic(old), magic(limit));
        ops.push((at, from.to_le_bytes().to_vec(), to.to_le_bytes().to_vec()));
    }
    for &at in &found.nones {
        ops.push((at, vec![found.actions], vec![NONE]));
    }
    let mut planned: Vec<Op> = ops
        .into_iter()
        .filter(|(_, expect, new)| expect != new)
        .map(|(rva, expect, new)| Op::Bytes { rva, expect, new })
        .collect();
    let actions = found.actions;
    let mut slot = found.slot.to_le_bytes().to_vec();
    slot.push(actions);
    let mut hooks = vec![
        (&found.label, false, "ui-controls-label", slot),
        (&found.key, true, "ui-controls-key", vec![actions]),
        (&found.open, false, "ui-controls-open", Vec::new()),
        (&found.save, false, "ui-controls-save", Vec::new()),
    ];
    for (site, value) in &found.stores {
        let mut data = vec![actions, *value];
        data.extend(found.table.to_le_bytes());
        hooks.push((site, false, "ui-controls-store", data));
    }
    for site in &found.defaults {
        hooks.push((site, false, "ui-controls-defaults", Vec::new()));
    }
    for (site, xmm) in &found.drags {
        hooks.push((
            site,
            false,
            "ui-controls-drag",
            vec![*xmm, found.limit, new],
        ));
    }
    for ((rva, expect), after, handler, data) in hooks {
        planned.push(Op::Context {
            rva: *rva,
            expect: expect.clone(),
            after,
            handler: handler.into(),
            data,
        });
    }
    for op in &planned {
        let (start, len) = match op {
            Op::Bytes { rva, expect, .. } | Op::Context { rva, expect, .. } => (*rva, expect.len()),
            _ => continue,
        };
        module.plan.claims.push(Claim {
            start,
            end: start + len as u32,
            owner: "menus".into(),
        });
    }
    module.plan.groups.push(Group {
        id: "ui-controls".into(),
        label: format!("{rows} menu key(s) in the controls menu"),
        ops: planned,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the thumb's arithmetic multiplies the scroll by.
    const TRAVEL: u64 = 84;

    #[test]
    fn divides_by_any_limit_as_the_thumb_does() {
        for limit in 22..=MOST {
            let m = u128::from(magic(limit));
            for x in 0..=u128::from(TRAVEL * u64::from(limit) + u64::from(limit)) {
                assert_eq!((x * m) >> 66, x / u128::from(limit), "{x} / {limit}");
            }
        }
        assert_eq!(magic(22), 0x2E8B_A2E8_BA2E_8BA3);
    }
}
