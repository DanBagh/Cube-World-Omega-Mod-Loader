//! Mods' menus in the client's controls menu: a row for each menu with a key, after the
//! client's own actions, which the player can rebind like the client's.
//!
//! The client keeps its bindings in a table of one per action, and marks a row being
//! rebound by its action, with the number of actions for none. The plan moves that mark
//! out of the way of the rows and patches the scroll for the longer list. The rows have a
//! table of their own here, `shown`, and these handlers stand it in for the client's:
//! - `ui-controls-label` names the rows where a row's label is picked.
//! - `ui-controls-key` has the client read their bindings from `shown`, so it names their
//!   keys itself.
//! - `ui-controls-store` keeps a key or button the player gives a row in `shown`.
//! - `ui-controls-open` fills `shown` with the saved keys as the menu opens, and
//!   `ui-controls-defaults` with the mods' own where the client makes its defaults, as
//!   its reset does.
//! - `ui-controls-save` saves `shown` when the client saves its own, to a file next to
//!   the log. Cancel needs nothing: the menu fills `shown` again as it opens.
//! - `ui-controls-drag` spreads a drag of the scrollbar over every row.
//!
//! The menus open on the saved keys, read once a frame like the keys of the plan.

use std::ffi::{CString, c_void};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::context::Context;
use crate::ui::Declared;

/// A binding of the client's: a GLFW key code, or a mouse button with `MOUSE` set.
pub type Binding = u64;
pub const MOUSE: Binding = 1 << 32;

/// Where the saved keys are, next to the log.
const SAVED: &str = "menu-keys.txt";
/// The longest label the controls menu takes: it copies one into a string's own bytes.
const LABEL: usize = 15;

/// A row: whose menu, its label and the key its mod gives it.
struct Row {
    owner: String,
    id: String,
    label: CString,
    default: Binding,
}

/// The rows of mods' menus, in the order the plan lists them, their bindings as the
/// controls menu shows them and as saved, and where the client's store of a binding goes
/// when it is one of theirs.
struct Rows {
    rows: Vec<Row>,
    shown: &'static [AtomicU64],
    saved: Vec<AtomicU64>,
    scratch: &'static [AtomicU64; 2],
}

static ROWS: OnceLock<Rows> = OnceLock::new();

/// The GLFW key code of an SDL scancode, for the keys a mod can give a menu.
pub fn glfw(scancode: u16) -> Option<u32> {
    match scancode {
        // A to Z
        4..=29 => Some(65 + u32::from(scancode - 4)),
        // 1 to 9, then 0
        30..=38 => Some(49 + u32::from(scancode - 30)),
        39 => Some(48),
        // F1 to F12
        58..=69 => Some(290 + u32::from(scancode - 58)),
        _ => None,
    }
}

/// The SDL scancode of a GLFW key code, for any key the player can bind.
pub fn scancode(glfw: u32) -> Option<u16> {
    let code = match glfw {
        65..=90 => 4 + (glfw - 65),
        49..=57 => 30 + (glfw - 49),
        48 => 39,
        290..=301 => 58 + (glfw - 290),
        302..=313 => 104 + (glfw - 302),
        320 => 98,
        321..=329 => 89 + (glfw - 321),
        _ => match glfw {
            32 => 44,
            39 => 52,
            44 => 54,
            45 => 45,
            46 => 55,
            47 => 56,
            59 => 51,
            61 => 46,
            91 => 47,
            92 => 49,
            93 => 48,
            96 => 53,
            161 | 162 => 100,
            256 => 41,
            257 => 40,
            258 => 43,
            259 => 42,
            260 => 73,
            261 => 76,
            262 => 79,
            263 => 80,
            264 => 81,
            265 => 82,
            266 => 75,
            267 => 78,
            268 => 74,
            269 => 77,
            280 => 57,
            281 => 71,
            282 => 83,
            283 => 70,
            284 => 72,
            330 => 99,
            331 => 84,
            332 => 85,
            333 => 86,
            334 => 87,
            335 => 88,
            336 => 103,
            340 => 225,
            341 => 224,
            342 => 226,
            343 => 227,
            344 => 229,
            345 => 228,
            346 => 230,
            347 => 231,
            348 => 101,
            _ => return None,
        },
    };
    u16::try_from(code).ok()
}

/// The SDL mouse button of a binding's, GLFW's left, right and middle being 0, 1 and 2.
pub fn button(glfw: u32) -> Option<u32> {
    match glfw {
        0 => Some(1),
        1 => Some(3),
        2 => Some(2),
        3..=31 => Some(glfw + 1),
        _ => None,
    }
}

/// The saved keys: a line of owner, id and binding for each menu.
fn read_saved(text: &str) -> Vec<(String, String, Binding)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let (owner, id) = (fields.next()?, fields.next()?);
            let binding = Binding::from_str_radix(fields.next()?.trim(), 16).ok()?;
            Some((owner.to_owned(), id.to_owned(), binding))
        })
        .collect()
}

fn saved_path() -> Option<std::path::PathBuf> {
    crate::HOME.get().map(|home| home.join(SAVED))
}

/// Notes the rows, every declared menu with a key, and their saved keys.
pub fn declare(menus: &[Declared]) {
    let saved = saved_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| read_saved(&text))
        .unwrap_or_default();
    let mut rows = Vec::new();
    for menu in menus {
        let Some(code) = menu.key.and_then(glfw) else {
            continue;
        };
        let mut label = menu.label.replace('\0', "").into_bytes();
        label.truncate(LABEL);
        rows.push(Row {
            owner: menu.owner.clone(),
            id: menu.id.clone(),
            label: CString::new(label).unwrap_or_default(),
            default: Binding::from(code),
        });
    }
    let saved: Vec<AtomicU64> = rows
        .iter()
        .map(|row| {
            let found = saved.iter().find(|s| s.0 == row.owner && s.1 == row.id);
            AtomicU64::new(found.map_or(row.default, |s| s.2))
        })
        .collect();
    let shown = saved
        .iter()
        .map(|s| AtomicU64::new(s.load(Ordering::Relaxed)));
    let _ = ROWS.set(Rows {
        rows,
        shown: Box::leak(shown.collect()),
        saved,
        scratch: Box::leak(Box::new([AtomicU64::new(0), AtomicU64::new(0)])),
    });
}

/// The key a menu opens on, as saved.
pub fn binding(owner: &str, id: &str) -> Option<Binding> {
    let rows = ROWS.get()?;
    let index = rows
        .rows
        .iter()
        .position(|r| r.owner == owner && r.id == id)?;
    Some(rows.saved[index].load(Ordering::Relaxed))
}

fn save(rows: &Rows) -> bool {
    let mut text = String::new();
    for (row, saved) in rows.rows.iter().zip(&rows.saved) {
        let binding = saved.load(Ordering::Relaxed);
        text.push_str(&format!("{}\t{}\t{binding:x}\n", row.owner, row.id));
    }
    saved_path().is_some_and(|path| std::fs::write(path, text).is_ok())
}

/// Which of the rows an action is, past the client's `actions`.
fn row(action: u32, actions: u8) -> Option<usize> {
    let index = action.checked_sub(u32::from(actions))? as usize;
    (index < ROWS.get()?.rows.len()).then_some(index)
}

/// Copies one table of bindings into another.
fn copy(from: impl Iterator<Item = Binding>, to: &[AtomicU64]) {
    for (binding, slot) in from.zip(to) {
        slot.store(binding, Ordering::Relaxed);
    }
}

/// `ui-controls-label`'s data: where the draw keeps a row's action, and how many the
/// client has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Label {
    slot: i32,
    actions: u8,
}

impl Label {
    pub fn read(data: &[u8]) -> Option<Label> {
        match data {
            [a, b, c, d, actions] => Some(Label {
                slot: i32::from_le_bytes([*a, *b, *c, *d]),
                actions: *actions,
            }),
            _ => None,
        }
    }
}

/// How many actions the client has, for the handlers with no more to know.
pub fn actions(data: &[u8]) -> Option<u8> {
    match data {
        [actions] if *actions > 0 => Some(*actions),
        _ => None,
    }
}

/// `ui-controls-store`'s data: the actions, the register with the binding, and where the
/// client's table is in the controls menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Store {
    actions: u8,
    value: u8,
    table: u32,
}

impl Store {
    pub fn read(data: &[u8]) -> Option<Store> {
        match data {
            [actions, value, a, b, c, d] if *actions > 0 && *value < 16 => Some(Store {
                actions: *actions,
                value: *value,
                table: u32::from_le_bytes([*a, *b, *c, *d]),
            }),
            _ => None,
        }
    }
}

/// `ui-controls-drag`'s data: the xmm register with the scroll as a float, the client's
/// limit it was worked out for, and the limit now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Drag {
    xmm: u8,
    old: u8,
    new: u8,
}

impl Drag {
    pub fn read(data: &[u8]) -> Option<Drag> {
        match data {
            [xmm, old, new] if *xmm < 16 && *old > 0 && new >= old => Some(Drag {
                xmm: *xmm,
                old: *old,
                new: *new,
            }),
            _ => None,
        }
    }

    /// The scroll a drag comes to, from the client's float: the drag's fraction times its
    /// limit, plus a half.
    pub fn scroll(self, value: f32) -> u32 {
        let fraction = ((value - 0.5) / f32::from(self.old)).max(0.0);
        ((fraction * f32::from(self.new) + 0.5) as u32).min(u32::from(self.new))
    }
}

/// For `context`: at the store that starts a row's label, with rdx the label and r12 its
/// length.
///
/// # Safety
/// From there, with `state` a `Label`.
pub unsafe fn label(context: &mut Context, state: *const c_void) {
    // SAFETY: the caller's word, and the draw keeps the action in that slot
    let (label, action) = unsafe {
        let label = *state.cast::<Label>();
        let slot = (context.rbp as usize).wrapping_add_signed(label.slot as isize);
        (label, (slot as *const u32).read_unaligned())
    };
    let (Some(index), Some(rows)) = (row(action, label.actions), ROWS.get()) else {
        return;
    };
    let text = &rows.rows[index].label;
    context.rdx = text.as_ptr() as u64;
    context.r12 = text.as_bytes().len() as u64;
}

/// For `context`: right after the client has a row's action in eax and its bindings in
/// rcx, before it takes the binding at rcx + rax * 8.
///
/// # Safety
/// From there, with `state` the actions as a `u8`.
pub unsafe fn key(context: &mut Context, state: *const c_void) {
    // SAFETY: the caller's word
    let actions = unsafe { *state.cast::<u8>() };
    let (Some(_), Some(rows)) = (row(context.rax as u32, actions), ROWS.get()) else {
        return;
    };
    // so rcx + rax * 8 is the row's binding, as if the client's table went on
    let first = rows.shown.as_ptr() as u64;
    context.rcx = first.wrapping_sub(u64::from(actions) * 8);
}

/// For `context`: before the client stores a binding for the row being rebound, at
/// rcx + rax * 8 + its table. A row's binding goes to `shown`, and the client's store to
/// the scratch, as rcx is used after.
///
/// # Safety
/// From there, with `state` a `Store`.
pub unsafe fn store(context: &mut Context, state: *const c_void) {
    // SAFETY: the caller's word
    let store = unsafe { *state.cast::<Store>() };
    let (Some(index), Some(rows)) = (row(context.rax as u32, store.actions), ROWS.get()) else {
        return;
    };
    rows.shown[index].store(context.register(store.value), Ordering::Relaxed);
    let at = context.rcx.wrapping_add(u64::from(store.table));
    // a place in the scratch the store reaches with a whole rax
    let scratch = rows.scratch.as_ptr() as u64 + at % 8;
    context.rax = scratch.wrapping_sub(at) >> 3;
}

/// For `context`: as the controls menu opens.
///
/// # Safety
/// From there.
pub unsafe fn open(_: &mut Context, _: *const c_void) {
    if let Some(rows) = ROWS.get() {
        copy(
            rows.saved.iter().map(|s| s.load(Ordering::Relaxed)),
            rows.shown,
        );
    }
}

/// For `context`: where the client makes its default bindings.
///
/// # Safety
/// From there.
pub unsafe fn defaults(_: &mut Context, _: *const c_void) {
    if let Some(rows) = ROWS.get() {
        copy(rows.rows.iter().map(|r| r.default), rows.shown);
    }
}

/// For `context`: right after the client has saved its bindings, with al whether it did.
///
/// # Safety
/// From there.
pub unsafe fn saved(context: &mut Context, _: *const c_void) {
    let Some(rows) = ROWS.get() else {
        return;
    };
    if context.rax & 0xff != 0 && !rows.rows.is_empty() {
        copy(
            rows.shown.iter().map(|s| s.load(Ordering::Relaxed)),
            &rows.saved,
        );
        let done = save(rows);
        #[cfg(windows)]
        if done {
            crate::log::log!("controls: {} menu key(s) saved", rows.rows.len());
        } else {
            crate::log::log!("controls: could not save the menu keys");
        }
        #[cfg(not(windows))]
        let _ = done;
    }
}

/// For `context`: before the client compares the scroll a drag comes to, in eax.
///
/// # Safety
/// From there, with `state` a `Drag`.
pub unsafe fn drag(context: &mut Context, state: *const c_void) {
    // SAFETY: the caller's word
    let drag = unsafe { *state.cast::<Drag>() };
    let value = f32::from_bits(context.xmm[usize::from(drag.xmm)][0] as u32);
    context.rax = (context.rax & !0xffff_ffff) | u64::from(drag.scroll(value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_keys_by_their_glfw_codes_and_back() {
        assert_eq!(glfw(4), Some(u32::from(b'A')));
        assert_eq!(glfw(14), Some(u32::from(b'K')));
        assert_eq!(glfw(30), Some(u32::from(b'1')));
        assert_eq!(glfw(39), Some(u32::from(b'0')));
        assert_eq!(glfw(58), Some(290));
        assert_eq!(glfw(69), Some(301));
        assert_eq!(glfw(70), None);
        for code in (4..=39).chain(58..=69) {
            assert_eq!(scancode(glfw(code).unwrap()), Some(code));
        }
        // the client's own defaults: space, tab, escape, shift, control, up and down
        let keys = [
            (32, 44),
            (258, 43),
            (256, 41),
            (340, 225),
            (341, 224),
            (265, 82),
            (264, 81),
        ];
        for (glfw, sdl) in keys {
            assert_eq!(scancode(glfw), Some(sdl));
        }
        assert_eq!(
            (button(0), button(1), button(2)),
            (Some(1), Some(3), Some(2))
        );
    }

    #[test]
    fn reads_saved_keys() {
        let read = read_saved("gear\tstats\t4b\nbroken\nother\tmap\t100000001\n");
        assert_eq!(
            read,
            [
                ("gear".to_owned(), "stats".to_owned(), 0x4b),
                ("other".to_owned(), "map".to_owned(), MOUSE | 1),
            ]
        );
    }

    #[test]
    fn spreads_a_drag_over_every_row() {
        let drag = Drag::read(&[2, 22, 23]).unwrap();
        // the client's float for the top, the bottom and past it
        assert_eq!(drag.scroll(0.5), 0);
        assert_eq!(drag.scroll(22.5), 23);
        assert_eq!(drag.scroll(40.0), 23);
        assert_eq!(drag.scroll(11.5), 12);
        assert_eq!(Drag::read(&[16, 22, 23]), None);
        assert_eq!(Drag::read(&[1, 22, 21]), None);
    }

    #[test]
    fn reads_the_handlers_data() {
        let read = Label::read(&[0x84, 0, 0, 0, 30]).unwrap();
        assert_eq!((read.slot, read.actions), (0x84, 30));
        assert_eq!(Label::read(&[0x20, 0, 0, 0]), None);
        assert_eq!(actions(&[30]), Some(30));
        assert_eq!(actions(&[0]), None);
        let store = Store::read(&[30, 6, 0x20, 0x82, 0x0c, 0]).unwrap();
        assert_eq!((store.actions, store.value, store.table), (30, 6, 0xC8220));
        assert_eq!(Store::read(&[30, 16, 0, 0, 0, 0]), None);
    }
}
