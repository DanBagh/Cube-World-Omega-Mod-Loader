//! Menus for mods: the `"menus"` manifest section, and the hooks the in-game loader
//! draws them with.
//!
//! A mod declares its menus, each with an id, the slot it goes in and a key that opens
//! and closes it, and its scripts draw them (see `scripts`). The in-game loader makes
//! them menus of the game's own kind: it places them in the game's slots, tilts them
//! with the cursor, closes them as the player starts to move or another menu takes
//! their place, and has them count as open menus wherever the client asks, which frees
//! the cursor. These hooks are planned whenever a mod has scripts, as the runtime starts
//! from the frame one.
//!
//! Build-specific: every function and offset the in-game loader needs is read from the
//! code that uses it, the controls menu and the HP bar for the most part. See `Found`.

use regex::bytes::Captures;
use serde_json::Value;

use super::ModuleBuild;
use super::stub::signature;
use crate::mods::{Mod, Notice};
use crate::plan::{Claim, Group, Op, Scripts};
use crate::report::{Report, one_line};

/// The slots a menu can go in, the longest menu id, and the most menus.
const SLOTS_NAMED: [&str; 2] = ["left", "center"];
const MAX_ID: usize = 32;
const MAX_MENUS: usize = 16;
/// Gear slots, and the size of an item in them: what the tooltips copy of the gear.
const SLOTS: u8 = 10;
const ITEM_SIZE: u8 = 0x14;

// mov rax,[game] / (v)movss xmm,[rax+ui scale] / (v)mulss xmm3,[factor] / pos, rotation,
// z 8.0, valign 1 / call move_2d: a centered menu placing itself. The controls menu
// and one other do
const PLACE: &str = r"\x48\x8b\x05(....)(?:\xc5\xfa\x10|\xf3\x0f\x10)[\x80-\xbf](....)(?:\xc5\xfa\x59|\xf3\x0f\x59)[\x05\x0d\x15\x1d](....)\x48\xc7\x44\x24.\x00\x00\x00\x00\xc7\x44\x24.\x00\x00\x00\x00\x48\xc7\x44\x24.\x00\x00\x00\x00\xc7\x44\x24.\x00\x00\x00\x41\xc7\x44\x24\x20\x01\x00\x00\x00\x48\x8d\x54\x24.\x4c\x8d\x44\x24.(?:\x48\x89[\xc0-\xff])?\xe8(....)";
// call <place fn> / mov rax,[rbp+N] / lea rcx,[rax+M] / call <place fn>: once a frame
const FRAME: &str = r"\xe8(....)(\x48\x8b\x85....)\x48\x8d\x88....\xe8(....)";
// a 236x180 menu's constructor: background frame, commit it, build the rest
const CONSTRUCTOR: &str = r"\xc7\x44\x24\x28\xb4\x00\x00\x00\xc7\x44\x24\x20\xec\x00\x00\x00\x48\x89\xf1\x31\xd2\x45\x31\xc0\x45\x31\xc9\xe8(....)\x48\x89\xf1\xe8(....)\x48\x89\xf1\xe8(....)";
// and in its build: the panel, a glyph of the font, and the commit before the epilogue
const PANEL: &str = r"\xc7\x44\x24\x28\xb4\x00\x00\x00\xc7\x44\x24\x20\xec\x00\x00\x00\x48\x89[\xc0-\xff]\x31\xd2\x45\x31\xc0\x45\x31\xc9\xe8(....)";
const GLYPH: &str = r"\x48\x8d\x0d(....)(?:[\x40-\x4f]?\x89[\xc0-\xff]){3}\xe8(....)";
const COMMIT: &str = r"\xe8(....)(?:\xc5\xf8|\x0f)\x28\xb5";
/// How far into a build its panel, glyph and commit are looked for.
const BUILD: usize = 0x3000;
// mov word [rbp+N],'HP' ... mov edx,50h / mov r8d,11h / call: the HP bar's constructor
const BAR: &str =
    r"\x66\xc7\x45.\x48\x50.{0,48}?\xba\x50\x00\x00\x00\x41\xb8\x11\x00\x00\x00\xe8(....)";
// in it, the grid headers of the two layers, the mask, and the base's size: the byte
// after the vtable is the bar's first own field
const GRID: &str =
    r"\xc7\x46(.)\x00\x00\x00\x00\x89[\x46-\x7e].\x89[\x46-\x7e].\xc7\x46.\x05\x00\x00\x00";
const BACKGROUND_GRID: &str = r"\xc7\x86(....)\x00\x00\x00\x00\x89[\x86-\xbe]....\x89[\x86-\xbe]....\xc7\x86....\x02\x00\x00\x00";
const MASK: &str = r"\x48\x8d\x8e(....)\x41\xb8\x00\x40\x00\x00";
const BASE: &str = r"\x48\x89\x06\xc6\x86(....)\x00";
// cmp dl,6 / ja / movzx eax,dl / lea rdx,[table] / jump table: the item stat getter
const STAT: &str =
    r"\x80\xfa\x06\x77.\x0f\xb6\xc2\x48\x8d\x15....\x48\x63\x04\x82\x48\x01\xd0\xff\xe0";
// mov r15,[game] / mov rcx,r15 / call check / lea rcx,[r15+alt] / add r15,gear / test
// al,al / cmove r15,rcx, then the gear is copied: the tooltips' gear
const GEAR: &str = r"\x4c\x8b\x3d(....)\x4c\x89\xf9\xe8(....)\x49\x8d\x8f(....)\x49\x81\xc7(....)\x84\xc0\x4c\x0f\x44\xf9(?:\xc4\xc1\x7c\x10\x07|.{0,6}\x41\xb8\xc8\x00\x00\x00)";
// movzx eax,[rsp+N] / cmp al,1 / jne / cmp byte [rcx+M],1: the menu hotkeys, when on
const HOTKEYS: &str = r"\x0f\xb6\x44\x24(.)\x3c\x01\x0f\x85....\x80\x79(.)\x01";
// cmp byte [rcx+key],1 / jne / maybe lea of the menu / test byte [rsi+flag],1: a
// menu's key, then whether that menu is open. KEY stands for the key's byte
const KEY_MENU: &str = r"\x80\x79KEY\x01(?:\x75.|\x0f\x85....)(?:[\x48\x4c]\x8d[\x80-\xbf]....)?[\x40-\x4f]?\xf6(?:\x84\x24|[\x80-\x87])(....)\x01";
// shl rax,4 / lea rcx,[names] / mov reg,[rax+rcx]: an item's name by its type, English
// then German, 16 bytes a type
const NAMES: &str = r"\x48\xc1\xe0\x04\x48\x8d\x0d(....)[\x48\x4c]\x8b[\x04-\x3c]\x08";
// mov eax,[reg+rarity] / divide by 5 / dec eax / cmp eax,3 / ja / then the red, green and
// blue of each of the 4 rarities from their tables: an item's name in its rarity's color.
// Some read an item that sits 4 bytes into a record, so the smallest offset is the item's
const RARITY: &str = r"\x8b[\x40-\x7f](.)\xb9\xcd\xcc\xcc\xcc\x48\x0f\xaf\xc1\x48\xc1\xe8\x22\xff\xc8.{0,24}?\x83\xf8\x03\x77.{1,8}?\x48\x8d\x0d(....).{4,6}?\x48\x8d\x0d(....).{4,6}?\x48\x8d\x0d(....)";
// mov rcx,rsi / call notice / mov dx,63h: a menu has opened, and the client notes
// whether the player was moving then, in the flag the function sets
const NOTICE: &str = r"\x48\x89\xf1\xe8(....)\x66\xba\x63\x00";
const NOTICE_SETS: &str = r"\x66\xc7\x81(....)\x01\x00";
// call moving / test al,al / je / test byte [rsi+flag],1 / jne / movzx: the player has
// started to move since the menus opened, and the client closes them
const WALK: &str =
    r"\xe8....\x84\xc0\x0f\x84....\xf6\x86(....)\x01\x0f\x85....(\x0f\xb6[\x80-\xbf]....)";
// test byte [rsi+flag],1 / je / lea rcx,[rsi+menu] / call close: the hotkeys closing a
// menu that is open
const CLOSER: &str = r"[\x40-\x4f]?\xf6(?:\x84\x24|[\x80-\x87])(....)\x01\x74.[\x48\x49]\x8d(?:\x8c\x24|[\x88-\x8f])(....)\xe8(....)";
/// How far into the hotkey handler its closes are looked for.
const CLOSERS_WITHIN: usize = 0x1000;
/// The skills menu's key, after the inventory's.
const SKILLS_AFTER_INVENTORY: u8 = 4;
// lea rdx,[rsp+N] / call: early in the controls menu's update, where the cursor is on it
const LOCAL: &str = r"\x48\x8d\x54\x24.\xe8(....)";
/// How far into a menu's update the cursor is looked for.
const LOCAL_WITHIN: usize = 0x60;
// mov dx,63h ... call: the sound of a menu opening, after it
const OPEN: &str = r"\x66\xba\x63\x00.{0,24}?\xe8(....)";
/// How far after the hotkey test the open sound is looked for.
const SOUND_AFTER: usize = 0x200;
// mov rcx,[r9+mouse] (or through r12) / test / je / mov rax,[rcx] / lea rdx,[rbp+N] / call
// [rax+slot] ... mov eax,[width] ... mov esi,[height]: a menu tilting with the cursor
const MOUSE: &str = r"(?:\x49\x8b\x89|\x49\x8b\x8c\x24)(....)\x48\x85\xc9\x0f\x84....\x48\x8b\x01\x48\x8d\x95....\xff\x50(.).{0,40}?\x8b\x05(....).{0,40}?\x8b\x35(....)";
// and its scale: (v)mulss xmm3,[factor], or movss xmm3,[factor] / mulss xmm3,xmmN, then
// valign 1
const SCALE: &str = r"(?:\xc5[\x80-\xff]\x59\x1d(....)|\xf3\x0f\x10\x1d(....)\xf3\x41?\x0f\x59[\xc0-\xff])\xc7\x44\x24\x20\x01\x00\x00\x00";
/// How far after where it reads the cursor a tilting menu's scale is looked for.
const TILTING: usize = 0x400;
// the menu settings in a row: ui scale, one more, the tilt, one more
const SETTINGS: &str = r"(?:\xc4\xc1\x7a\x10[\x80-\xbf]|\xf3?\x41\x0f\x10(?:\x84\x24|[\x80-\xbf]))(....).{0,20}?(?:\xc4\xc1\x7a\x10[\x80-\xbf]|\xf3?\x41\x0f\x10(?:\x84\x24|[\x80-\xbf]))(....).{0,20}?(?:\xc4\xc1\x7a\x10[\x80-\xbf]|\xf3?\x41\x0f\x10(?:\x84\x24|[\x80-\xbf]))(....).{0,20}?(?:\xc4\xc1\x7a\x10[\x80-\xbf]|\xf3?\x41\x0f\x10(?:\x84\x24|[\x80-\xbf]))(....)";
// lea r12,[font] / lea rcx,[rbp+N] / lea r8,[rbp+M] / mov rdx,r12: the inventory's title
// font
const TITLE_FONT: &str = r"\x4c\x8d\x25(....)\x48\x8d\x4d.\x4c\x8d\x85....\x4c\x89\xe2";
// cmp bl,1 / mov rcx,rsi / jne / cmp byte [rcx+was],0 / jne / cmp byte [rcx+system],1 /
// jne / mov rcx,rsi / call close_top: the system menu's flag, and the client closing its
// top menu
const CLOSE_TOP: &str = r"\x80\xfb\x01\x48\x89\xf1\x75.\x80\xb9....\x00\x75.\x80\xb9(....)\x01\x75.\x48\x89\xf1\xe8(....)";
// test byte [reg+flag],1 (or cmp byte [reg+flag],0 or 1) / jne, over and over, then the
// system menu's flag: cmp byte [reg+system],0 or 1, or movzx eax,byte [reg+system]. The
// client asking whether any menu is open, wherever it does. SYSTEM stands for the
// system menu flag's offset
const CHAIN: &str = r"((?:(?:[\x40-\x4f]?\xf6(?:\x84\x24|[\x80-\x8f])....\x01|[\x40-\x4f]?\x80(?:\xbc\x24|[\xb8-\xbf])....[\x00\x01])(?:\x75.|\x0f\x85....)(?:\xb0\x01)?){8,}?)([\x40-\x4f]?(?:\x80(?:\xbc\x24|[\xb8-\xbf])SYSTEM([\x00\x01])|\x0f\xb6(?:\x84\x24|[\x80-\xbf])SYSTEM))";
const OPEN_FLAG: &str = r"\xf6(?:\x84\x24|[\x80-\x8f])(....)\x01";
// cmp byte [rcx+base],1 / jne / mov byte [rcx+base],0 / jmp hide: a bar hiding itself
const HIDE: &str = r"\x80\xb9(....)\x01\x75.\xc6\x81(....)\x00\xe9(....)";

/// A menu a mod declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub owner: String,
    pub id: String,
    pub slot: String,
    pub key: Option<Key>,
}

/// The SDL scancode of a key: a letter, a digit or F1 to F12.
pub fn scancode(key: &str) -> Option<u16> {
    let key = key.to_ascii_uppercase();
    match key.as_bytes() {
        &[c @ b'A'..=b'Z'] => Some(4 + u16::from(c - b'A')),
        &[b'0'] => Some(39),
        &[c @ b'1'..=b'9'] => Some(30 + u16::from(c - b'1')),
        [b'F', rest @ ..] => {
            let n: u16 = std::str::from_utf8(rest).ok()?.parse().ok()?;
            (1..=12).contains(&n).then_some(57 + n)
        }
        _ => None,
    }
}

/// A key as a mod names it, and its SDL scancode.
type Key = (String, u16);

fn parse(entry: &Value) -> Result<(String, String, Option<Key>), String> {
    let entry = entry.as_object().ok_or("a menu is an object")?;
    let id = entry
        .get("id")
        .and_then(Value::as_str)
        .ok_or("id is missing")?;
    let plain = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
    if id.is_empty() || id.len() > MAX_ID || !id.chars().all(plain) {
        return Err(format!(
            "id is up to {MAX_ID} lowercase letters, digits, - and _"
        ));
    }
    let slot = match entry.get("slot") {
        None | Some(Value::Null) => "left",
        Some(slot) => {
            let slot = slot.as_str().unwrap_or_default();
            SLOTS_NAMED
                .into_iter()
                .find(|s| *s == slot)
                .ok_or_else(|| format!("slot is one of {}", SLOTS_NAMED.join(", ")))?
        }
    };
    let key = match entry.get("key") {
        None | Some(Value::Null) => None,
        Some(key) => {
            let key = key.as_str().ok_or("key is text")?;
            let code = scancode(key).ok_or("key is a letter, a digit or F1 to F12")?;
            Some((key.to_ascii_uppercase(), code))
        }
    };
    Ok((id.to_owned(), slot.to_owned(), key))
}

/// The menus the loading mods declare. A key two menus ask for goes to the one that
/// loads later.
pub fn collect(mods: &mut [Mod], order: &[usize], report: &mut Report) -> Vec<Declared> {
    let mut declared: Vec<Declared> = Vec::new();
    for &index in order {
        let found = &mut mods[index];
        let scripted = !found.list("scripts").is_empty();
        for entry in found.list("menus").to_vec() {
            let parsed = parse(&entry).and_then(|parsed| {
                let taken = declared
                    .iter()
                    .any(|w| w.owner == found.id && w.id == parsed.0);
                if taken {
                    Err(format!("the mod has two menus called '{}'", parsed.0))
                } else if declared.len() >= MAX_MENUS {
                    Err(format!("the game has room for {MAX_MENUS} menus from mods"))
                } else {
                    Ok(parsed)
                }
            });
            let (id, slot, key) = match parsed {
                Ok(parsed) => parsed,
                Err(reason) => {
                    found.warnings.push(
                        Notice::new("Menu ignored")
                            .with("Entry", one_line(&entry.to_string(), 70))
                            .with("Reason", reason),
                    );
                    continue;
                }
            };
            if !scripted {
                found.warnings.push(
                    Notice::new("Menu without a script")
                        .with("Menu", &id)
                        .with("Reason", "no script of the mod draws it"),
                );
            }
            if let Some((name, code)) = &key {
                for earlier in declared.iter_mut() {
                    if earlier.key.as_ref().is_some_and(|(_, c)| c == code) {
                        report.warn(format!(
                            "key {name} is asked for by {}'s menu {} and {}'s menu {id}; {} gets it",
                            earlier.owner, earlier.id, found.id, found.id
                        ));
                        earlier.key = None;
                    }
                }
            }
            declared.push(Declared {
                owner: found.id.clone(),
                id,
                slot,
                key,
            });
        }
    }
    declared
}

/// What the in-game loader is told about a client.
#[derive(Debug, Clone, PartialEq)]
struct Found {
    /// The instructions the hooks replace: once a frame, and where the hotkeys are
    /// handled.
    frame: (u32, Vec<u8>),
    hotkeys: (u32, Vec<u8>),
    /// Where the client asks whether a menu is open, and how each answers: 0 as a
    /// value, 1 or 2 by comparing to 1 or to 0.
    asks: Vec<((u32, Vec<u8>), u8)>,
    /// Where the hotkey handler keeps on its stack whether hotkeys work.
    gate: u8,
    sound: u32,
    close_top: u32,
    system: u32,
    menus: Vec<u32>,
    /// The menus that may stay open with this one: the inventory.
    keep: Vec<u32>,
    /// The menus of the slot this one takes, crafting and skills: each one's open flag,
    /// where it is in the game object, and its close.
    slot: Vec<[u32; 3]>,
    /// `notice(game)`: notes whether the player is moving as a menu opens. And where
    /// the client closes its menus as the player starts to move.
    notice: u32,
    walk: (u32, Vec<u8>),
    hide: u32,
    /// `local(widget, out)`: where the cursor is on a widget, in voxels from its bottom
    /// left. And the item names: where the table is, and how many types it has.
    local: u32,
    names: u32,
    name_count: u32,
    /// Where an item keeps its rarity, and the colors of rarities 1 to 4, red, green and
    /// blue in turn.
    rarity: u32,
    rarity_colors: [u32; 12],
    game: u32,
    ui_scale: u32,
    scale: f32,
    tilt: u32,
    place: u32,
    mouse: u32,
    mouse_slot: u8,
    width: u32,
    height: u32,
    gear_check: u32,
    gear_alt: u32,
    gear: u32,
    stat: u32,
    bar: u32,
    base: u32,
    grid: u32,
    background_grid: u32,
    mask: u32,
    background: u32,
    commit_background: u32,
    panel: u32,
    glyph: u32,
    font: u32,
    title_font: u32,
    commit: u32,
}

/// The one value every item of `found` has, if they agree and there is one.
fn agreed<T: PartialEq>(found: impl IntoIterator<Item = T>) -> Option<T> {
    let mut found = found.into_iter();
    let first = found.next()?;
    found.all(|other| other == first).then_some(first)
}

fn find(module: &ModuleBuild) -> Option<Found> {
    let image = module.image;
    let (text_va, text) = image.data(".text")?;
    // where the rel32 at `at` in .text goes
    let rel = |at: usize| -> Option<u32> {
        let relative = i32::from_le_bytes(text.get(at..at + 4)?.try_into().ok()?);
        Some((text_va + at as u32 + 4).wrapping_add_signed(relative))
    };
    let rel_of = |hit: &Captures, group: usize| rel(hit.get(group)?.start());
    let u32_of = |hit: &Captures, group: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            hit.get(group)?.as_bytes().try_into().ok()?,
        ))
    };
    let replaced = |hit: &Captures, group: usize| {
        let found = hit.get(group)?;
        Some((text_va + found.start() as u32, found.as_bytes().to_vec()))
    };
    let constant = |rva: u32| Some(u32::from_le_bytes(image.read(rva, 4)?.try_into().ok()?));

    // the menus that place themselves: game, ui scale, factor, move_2d
    let places: Vec<(u32, (u32, u32, u32, u32))> = signature(PLACE)
        .captures_iter(text)
        .filter_map(|hit| {
            let factor = constant(rel_of(&hit, 3)?)?;
            let at = text_va + hit.get(0)?.start() as u32;
            Some((
                image.function_start(at)?,
                (rel_of(&hit, 1)?, u32_of(&hit, 2)?, factor, rel_of(&hit, 4)?),
            ))
        })
        .collect();
    let (game, ui_scale, _, place) = agreed(places.iter().map(|p| p.1))?;
    let placing = |rva: u32| places.iter().any(|p| p.0 == rva);
    let frame = agreed(signature(FRAME).captures_iter(text).filter_map(|hit| {
        (placing(rel_of(&hit, 1)?) && placing(rel_of(&hit, 3)?)).then(|| replaced(&hit, 2))?
    }))?;

    // the menus that tilt with the cursor: where it is, the screen, their scale
    let tilting: Vec<_> = signature(MOUSE)
        .captures_iter(text)
        .filter_map(|hit| {
            let end = hit.get(0)?.end();
            let after = text.get(end..text.len().min(end + TILTING))?;
            let scale = signature(SCALE).captures(after)?;
            let scale = scale.get(1).or(scale.get(2))?;
            Some((
                u32_of(&hit, 1)?,
                hit.get(2)?.as_bytes()[0],
                rel_of(&hit, 3)?,
                rel_of(&hit, 4)?,
                constant(rel(end + scale.start())?)?,
            ))
        })
        .collect();
    let (mouse, mouse_slot, width, height, scale) = agreed(tilting)?;
    let tilt = agreed(
        signature(SETTINGS)
            .captures_iter(text)
            .filter_map(|hit| (u32_of(&hit, 1)? == ui_scale).then(|| u32_of(&hit, 3))?),
    )?;

    // the constructors of 236x180 menus, and in their builds what they draw with
    let builds: Vec<_> = signature(CONSTRUCTOR)
        .captures_iter(text)
        .filter_map(|hit| {
            let (background, commit_background) = (rel_of(&hit, 1)?, rel_of(&hit, 2)?);
            let build = rel_of(&hit, 3)?.checked_sub(text_va)? as usize;
            let body = text.get(build..text.len().min(build + BUILD))?;
            let panel = signature(PANEL).captures(body)?;
            let glyph = signature(GLYPH).captures(body)?;
            let commit = signature(COMMIT).captures(body)?;
            let at = |hit: &Captures, group: usize| rel(build + hit.get(group)?.start());
            Some((
                background,
                commit_background,
                at(&panel, 1)?,
                at(&glyph, 2)?,
                at(&glyph, 1)?,
                at(&commit, 1)?,
            ))
        })
        .collect();
    let (background, commit_background, panel, glyph, font, commit) = agreed(builds)?;
    let title_font = agreed(
        signature(TITLE_FONT)
            .captures_iter(text)
            .filter_map(|h| rel_of(&h, 1)),
    )?;

    let bar = agreed(
        signature(BAR)
            .captures_iter(text)
            .filter_map(|h| rel_of(&h, 1)),
    )?;
    let body = {
        let at = bar.checked_sub(text_va)? as usize;
        text.get(at..text.len().min(at + 0x200))?
    };
    let grid = u32::from(signature(GRID).captures(body)?.get(1)?.as_bytes()[0]);
    let first = |pattern: &str| u32_of(&signature(pattern).captures(body)?, 1);
    let (background_grid, mask, base) = (first(BACKGROUND_GRID)?, first(MASK)?, first(BASE)?);

    let stat = agreed(
        signature(STAT)
            .find_iter(text)
            .filter_map(|hit| image.function_start(text_va + hit.start() as u32)),
    )?;
    let (gear_game, gear_check, gear_alt, gear) =
        agreed(signature(GEAR).captures_iter(text).filter_map(|h| {
            Some((
                rel_of(&h, 1)?,
                rel_of(&h, 2)?,
                u32_of(&h, 3)?,
                u32_of(&h, 4)?,
            ))
        }))?;

    let (at, gate, inventory_key) =
        agreed(signature(HOTKEYS).captures_iter(text).filter_map(|h| {
            Some((
                h.get(0)?.start(),
                h.get(1)?.as_bytes()[0],
                h.get(2)?.as_bytes()[0],
            ))
        }))?;
    // the menu of each key, the crafting key following the inventory's
    let menu_of = |key: u8| {
        let pattern = KEY_MENU.replace("KEY", &format!(r"\x{key:02x}"));
        agreed(
            signature(&pattern)
                .captures_iter(text)
                .filter_map(|h| u32_of(&h, 1)),
        )
    };
    let keep = vec![menu_of(inventory_key)?];
    let slot_flags = [
        menu_of(inventory_key.checked_add(1)?)?,
        menu_of(inventory_key.checked_add(SKILLS_AFTER_INVENTORY)?)?,
    ];
    let handler = text.get(at..text.len().min(at + CLOSERS_WITHIN))?;
    let closers: Vec<[u32; 3]> = signature(CLOSER)
        .captures_iter(handler)
        .filter_map(|h| Some([u32_of(&h, 1)?, u32_of(&h, 2)?, rel(at + h.get(3)?.start())?]))
        .collect();
    let slot = slot_flags
        .iter()
        .map(|flag| agreed(closers.iter().filter(|c| c[0] == *flag).copied()))
        .collect::<Option<Vec<[u32; 3]>>>()?;
    let notice = agreed(
        signature(NOTICE)
            .captures_iter(text)
            .filter_map(|h| rel_of(&h, 1)),
    )?;
    let notice_sets = {
        let at = notice.checked_sub(text_va)? as usize;
        let body = text.get(at..text.len().min(at + 0x60))?;
        u32_of(&signature(NOTICE_SETS).captures(body)?, 1)?
    };
    let walk = agreed(
        signature(WALK)
            .captures_iter(text)
            .filter_map(|h| (u32_of(&h, 1)? == notice_sets).then(|| replaced(&h, 2))?),
    )?;
    let local = agreed(places.iter().filter_map(|&(start, _)| {
        let at = start.checked_sub(text_va)? as usize;
        let body = text.get(at..text.len().min(at + LOCAL_WITHIN))?;
        rel(at + signature(LOCAL).captures(body)?.get(1)?.start())
    }))?;
    let (names, name_count) = signature(NAMES)
        .captures_iter(text)
        .filter_map(|h| {
            let table = rel_of(&h, 1)?;
            let count = (1..0x400)
                .take_while(|&kind| {
                    image
                        .read(table + 16 * kind, 8)
                        .and_then(|p| p.try_into().ok())
                        .map(u64::from_le_bytes)
                        .and_then(|p| p.checked_sub(image.image_base))
                        .is_some_and(|rva| rva > 0 && rva < u64::from(image.size_of_image))
                })
                .count() as u32;
            (count > 0).then_some((table, count + 1))
        })
        .min()?;
    let after = text.get(at..text.len().min(at + SOUND_AFTER))?;
    let sound = rel(at + signature(OPEN).captures(after)?.get(1)?.start())?;
    let hotkeys = (text_va + at as u32, text.get(at..at + 5)?.to_vec());

    let (system, close_top) = agreed(
        signature(CLOSE_TOP)
            .captures_iter(text)
            .filter_map(|h| Some((u32_of(&h, 1)?, rel_of(&h, 2)?))),
    )?;
    // the menus that close this one are those every ask tests
    let mut asks = Vec::new();
    let mut menus: Option<Vec<u32>> = None;
    let system_bytes: String = system
        .to_le_bytes()
        .iter()
        .map(|b| format!(r"\x{b:02x}"))
        .collect();
    for hit in signature(&CHAIN.replace("SYSTEM", &system_bytes)).captures_iter(text) {
        let compared = hit.get(3).map(|value| value.as_bytes()[0]);
        let tested: Vec<u32> = signature(OPEN_FLAG)
            .captures_iter(hit.get(1)?.as_bytes())
            .filter_map(|flag| u32_of(&flag, 1))
            .collect();
        menus = Some(match menus {
            None => tested,
            Some(both) => both.into_iter().filter(|w| tested.contains(w)).collect(),
        });
        asks.push((replaced(&hit, 2)?, compared.map_or(0, |value| 2 - value)));
    }
    let menus = menus?;
    let rarities: Vec<(u8, [u32; 12])> = signature(RARITY)
        .captures_iter(text)
        .filter_map(|h| {
            let mut colors = [0u32; 12];
            for (channel, group) in (2..5).enumerate() {
                let table = rel_of(&h, group)?;
                for rarity in 0..4 {
                    colors[channel * 4 + rarity] = constant(table + 4 * rarity as u32)?;
                }
            }
            Some((h.get(1)?.as_bytes()[0], colors))
        })
        .collect();
    let rarity_colors = agreed(rarities.iter().map(|r| r.1))?;
    let rarity = u32::from(rarities.iter().map(|r| r.0).min()?);
    if !keep.iter().all(|k| menus.contains(k)) {
        return None;
    }
    let hide = agreed(signature(HIDE).captures_iter(text).filter_map(|h| {
        (u32_of(&h, 1)? == base && u32_of(&h, 2)? == base).then(|| rel_of(&h, 3))?
    }))?;

    (gear_game == game && !menus.is_empty() && asks.len() >= 2).then_some(Found {
        frame,
        hotkeys,
        asks,
        gate,
        sound,
        close_top,
        system,
        menus,
        keep,
        slot,
        notice,
        walk,
        hide,
        local,
        names,
        name_count,
        rarity,
        rarity_colors,
        game,
        ui_scale,
        scale: f32::from_bits(scale),
        tilt,
        place,
        mouse,
        mouse_slot,
        width,
        height,
        gear_check,
        gear_alt,
        gear,
        stat,
        bar,
        base,
        grid,
        background_grid,
        mask,
        background,
        commit_background,
        panel,
        glyph,
        font,
        title_font,
        commit,
    })
}

/// The `ui-frame` handler's data, in the order the in-game loader reads it.
fn layout(found: &Found) -> Vec<u8> {
    let mut data = Vec::new();
    let words = |data: &mut Vec<u8>, values: &[u32]| {
        for value in values {
            data.extend(value.to_le_bytes());
        }
    };
    words(
        &mut data,
        &[
            found.game,
            found.ui_scale,
            found.scale.to_bits(),
            found.tilt,
            found.place,
            found.mouse,
        ],
    );
    data.push(found.mouse_slot);
    words(
        &mut data,
        &[
            found.width,
            found.height,
            found.gear_check,
            found.gear_alt,
            found.gear,
        ],
    );
    data.extend([SLOTS, ITEM_SIZE]);
    words(
        &mut data,
        &[
            found.stat,
            found.bar,
            found.base,
            found.grid,
            found.background_grid,
            found.mask,
            found.background,
            found.commit_background,
            found.panel,
            found.glyph,
            found.font,
            found.title_font,
            found.commit,
            found.sound,
            found.close_top,
            found.system,
            found.hide,
            found.local,
            found.names,
            found.name_count,
            found.rarity,
            found.notice,
        ],
    );
    words(&mut data, &found.rarity_colors);
    data.push(found.menus.len() as u8);
    words(&mut data, &found.menus);
    data.push(found.keep.len() as u8);
    words(&mut data, &found.keep);
    data.push(found.slot.len() as u8);
    for menu in &found.slot {
        words(&mut data, menu);
    }
    data
}

/// Plans the hooks the in-game loader runs scripts and draws menus with, if any mod
/// has scripts. False if it has none, or this build's code could not be read for them.
/// What the controls menu calls a menu: its mod's `controls.<id>` string, or the mod's name,
/// cut to what the menu takes.
pub fn label(scripts: &Scripts, mods: &[Mod], owner: &str, id: &str) -> String {
    let named = scripts
        .texts
        .get(owner)
        .and_then(|t| t.get(&format!("controls.{id}")));
    let label = named.map_or_else(
        || {
            let name = mods.iter().find(|m| m.id == owner).map(|m| m.name.as_str());
            name.unwrap_or(owner)
        },
        String::as_str,
    );
    let mut cut = label.to_owned();
    while cut.len() > super::controls::LABEL {
        cut.pop();
    }
    cut.trim_end().to_owned()
}

pub fn plan(module: &mut ModuleBuild, scripts: &Scripts, report: &mut Report) -> bool {
    let (menus, files) = (scripts.menus.len(), scripts.files.len());
    if files == 0 {
        return false;
    }
    let Some(found) = find(module) else {
        report.warn(format!(
            "{}: could not locate the menu code in this build; scripts do not run",
            module.backend.key()
        ));
        return false;
    };
    let mut hooks = vec![
        (&found.frame, false, "ui-frame", layout(&found)),
        (&found.hotkeys, false, "ui-hotkey", vec![found.gate]),
        (&found.walk, false, "ui-walk", Vec::new()),
    ];
    for (ask, answer) in &found.asks {
        hooks.push((ask, true, "ui-open", vec![*answer]));
    }
    let mut ops = Vec::new();
    for ((site, replaced), after, handler, data) in hooks {
        module.plan.claims.push(Claim {
            start: *site,
            end: site + replaced.len() as u32,
            owner: "scripts".into(),
        });
        ops.push(Op::Context {
            rva: *site,
            expect: replaced.clone(),
            after,
            handler: handler.into(),
            data,
        });
    }
    module.plan.groups.push(Group {
        id: "ui".into(),
        label: format!("{files} script(s), {menus} menu(s)"),
        ops,
    });
    let keyed = scripts.menus.iter().filter(|w| w.key.is_some()).count();
    if keyed > 0 && !super::controls::plan(module, keyed) {
        report.warn(format!(
            "{}: could not locate the controls menu's list in this build; menu keys are not listed there",
            module.backend.key()
        ));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_keys_by_their_sdl_scancodes() {
        assert_eq!(scancode("a"), Some(4));
        assert_eq!(scancode("K"), Some(14));
        assert_eq!(scancode("1"), Some(30));
        assert_eq!(scancode("0"), Some(39));
        assert_eq!(scancode("F1"), Some(58));
        assert_eq!(scancode("f12"), Some(69));
        assert_eq!(scancode("F13"), None);
        assert_eq!(scancode("ESC"), None);
    }

    #[test]
    fn reads_menus_and_gives_a_shared_key_to_the_later_mod() {
        use crate::mods::tests::write_mod;
        use crate::mods::{load, resolve};

        let dir = tempfile::tempdir().unwrap();
        write_mod(
            dir.path(),
            "a",
            r#"{"schema": 1, "id": "a", "version": "1.0.0", "scripts": ["s.luau"],
                "menus": [{"id": "gear", "key": "k"}, {"id": "gear"}, {"id": "Bad"},
                            {"id": "map", "slot": "top"}, {"id": "quiet", "slot": "center"}]}"#,
        );
        write_mod(
            dir.path(),
            "b",
            r#"{"schema": 1, "id": "b", "version": "1.0.0",
                "menus": [{"id": "other", "key": "K"}]}"#,
        );
        let mut mods = load(dir.path());
        let order = resolve(&mut mods, "gh-1");
        let mut report = Report::default();
        let menus = collect(&mut mods, &order, &mut report);
        let summary: Vec<_> = menus
            .iter()
            .map(|w| {
                (
                    w.owner.as_str(),
                    w.id.as_str(),
                    w.slot.as_str(),
                    w.key.clone(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("a", "gear", "left", None),
                ("a", "quiet", "center", None),
                ("b", "other", "left", Some(("K".to_owned(), 14))),
            ]
        );
        // the duplicate, the bad id and the bad slot; b's menu has no script
        assert_eq!(mods[0].warnings.len(), 3);
        assert_eq!(mods[1].warnings.len(), 1);
        assert_eq!(report.warnings().count(), 1);
    }
}
