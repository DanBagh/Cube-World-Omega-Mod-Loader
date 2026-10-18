//! The stats window: a window of the client's own kind, opened and closed with a key,
//! showing what the equipped gear adds up to.
//!
//! Four context hooks drive it, all on the client's main thread:
//! - `stats-window`, once a frame next to where the controls window places itself,
//!   builds, fills and places it the way the inventory places itself, tilting with the
//!   cursor. It also holds what the others share, so it is installed first.
//! - `stats-hotkey`, where the client handles the keys of its windows, notes whether it
//!   would take a window's key now. The key itself is read once a frame, as the client's
//!   own handler runs less often and can miss a short press.
//! - `stats-walk`, where the client closes its windows as the player starts to move,
//!   closes this one too.
//! - `stats-open`, wherever the client asks whether any window is open, makes it count,
//!   so the client frees the cursor and holds the camera while it is open.
//!
//! The plan names every function and offset of the client they use.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};

/// A row: the stat number the client's stat getter takes, its name, the short name its
/// tooltips use, and a colour. In the order the tooltips list them.
pub struct Row {
    pub stat: u8,
    pub name: &'static str,
    pub short: &'static str,
    pub colour: [f32; 3],
}

pub const ROWS: [Row; 7] = [
    Row {
        stat: 0,
        name: "ATTACK",
        short: "ATK",
        colour: [250.0, 120.0, 60.0],
    },
    Row {
        stat: 1,
        name: "ARMOR",
        short: "DEF",
        colour: [110.0, 170.0, 250.0],
    },
    Row {
        stat: 3,
        name: "HEALTH",
        short: "HP",
        colour: [220.0, 30.0, 40.0],
    },
    Row {
        stat: 2,
        name: "RESISTANCE",
        short: "RES",
        colour: [190.0, 110.0, 250.0],
    },
    Row {
        stat: 6,
        name: "REGENERATION",
        short: "REG",
        colour: [80.0, 220.0, 90.0],
    },
    Row {
        stat: 4,
        name: "CRITICAL",
        short: "CRT",
        colour: [250.0, 210.0, 50.0],
    },
    Row {
        stat: 5,
        name: "HASTE",
        short: "HST",
        colour: [70.0, 210.0, 230.0],
    },
];
/// The longest title the window takes.
pub const MAX_TITLE: usize = 24;
/// Most windows a plan names as closing this one.
pub const MAX_WINDOWS: usize = 16;
/// Margins inside the window, and the gaps between a row's name, short name and value.
const MARGIN: i32 = 6;
const GAP: i32 = 4;
/// Space between the title and the first row, and between rows.
const TITLE_GAP: i32 = 4;
const ROW_GAP: i32 = 2;
const VALUE_GAP: i32 = 12;
/// Space between the window and a tooltip, and the most items a tooltip lists.
const TIP_GAP: i32 = 10;
/// How far from the camera a tooltip is, nearer than the windows' 8 so it stays in front
/// of them, as the client's item tooltips do. It is shrunk to look the size it would at 8.
const TIP_DEPTH: f32 = 7.0;
const WINDOW_DEPTH: f32 = 8.0;
const MAX_TIP_ITEMS: usize = 10;
/// What a tooltip says when no gear adds to its stat, and the longest item name read.
const NO_GEAR: &[u8] = b"NO GEAR ADDS THIS";
const MAX_NAME: usize = 40;
/// The widest an item name is drawn in a tooltip, in voxels, before it is cut with dots
/// as the inventory cuts its names.
const MAX_NAME_WIDTH: i32 = 150;
const CUT: &[u8] = b"...";
/// What the widest value is measured as.
const WIDEST_VALUE: &[u8] = b"00000";
/// Text sits on the panel and is 3 voxels deep, as in the inventory. The grid is the
/// bar's, 5 deep.
const TEXT_Z: i32 = 2;
const TEXT_DEPTH: i32 = 3;
const DEPTH: i32 = 5;
const MAX_VOXELS: i32 = 0x20000;
/// What the client's widgets take beyond their base, at most.
const OWN_FIELDS: usize = 0x1000;
/// How long the window stays drawn after the client last handled its hotkeys, in ms. It
/// stops doing that in some menus, while typing and on the start screen.
const SEEN: u64 = 500;
const PANEL: [f32; 3] = [127.0, 127.0, 127.0];
const TITLE: [f32; 3] = [250.0, 180.0, 70.0];
const SHORT: [f32; 3] = [150.0, 150.0, 150.0];
const VALUE: [f32; 3] = [230.0, 230.0, 230.0];
/// The tilt toward the cursor, as the client's windows do it: degrees per screen height
/// across and down, and how fast it follows, per ms.
const TILT_ACROSS: f32 = 40.0;
const TILT_DOWN: f32 = 20.0;
const TILT_RATE: f64 = 0.005;
/// The left window slot, crafting's and the skills', as the client places it: turned
/// this far toward the middle, at most this UI scale, and to the left of the middle by a
/// base that grows as the tilt setting shrinks, plus a share of the window's width past
/// a point.
const SLOT_TURN: f32 = 50.0;
const SLOT_MAX_UI: f32 = 0.85;
const SLOT_BASE: f32 = 3.0;
const SLOT_BASE_TILT: f32 = 1.25;
const SLOT_WIDE: f32 = 135.0;
const SLOT_WIDE_SHARE: f32 = 0.016;
/// Widths the inventory has, in voxels: its outer edge is mirrored on the left.
const INVENTORY_WIDTHS: std::ops::Range<i32> = 32..1024;
/// Where a client font keeps its size and its glyph advances.
const FONT_HEADER: usize = 0x40000;
const FONT_ADVANCE: usize = 0x40010;
/// Taller glyphs than this mean the font is not what the plan expects.
const MAX_GLYPH: u32 = 64;
/// The client's sounds for a window opening and closing.
const OPEN_SOUND: u16 = 0x63;
const CLOSE_SOUND: u16 = 0x64;
/// How many windows the hotkey closes, at most, before opening this one.
const CLOSE_AT_MOST: usize = 8;

static OPEN: AtomicBool = AtomicBool::new(false);
/// Whether the client took the keys of its windows when it last handled them.
static HOTKEYS: AtomicBool = AtomicBool::new(false);
/// The SDL scancode of the key, once `stats-hotkey` is installed.
static KEY: AtomicU32 = AtomicU32::new(u32::MAX);
/// When the hotkey hook last ran, in ms since boot.
static SEEN_AT: AtomicU64 = AtomicU64::new(0);

/// Reads the little-endian fields of a handler's data in turn.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (&head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take::<1>().map(|[b]| b)
    }
    fn u16(&mut self) -> Option<u16> {
        self.take().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }
    fn bytes(&mut self, length: usize) -> Option<Vec<u8>> {
        let bytes = self.0.get(..length)?.to_vec();
        self.0 = &self.0[length..];
        Some(bytes)
    }
}

/// What `stats-window` is told about the client, as RVAs and offsets.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The global holding the game object.
    pub game: u32,
    /// The UI scale in the game object, what the inventory multiplies it by, and the
    /// window tilt setting.
    pub ui_scale: u32,
    pub scale: f32,
    pub tilt: u32,
    /// `move_2d(widget, pos, rotation, scale, valign)`: draws a widget for this frame.
    pub place: u32,
    /// The cursor: an object in the game whose method `slot` gives its position, and the
    /// globals with the screen's size.
    pub mouse: u32,
    pub mouse_slot: u8,
    pub width: u32,
    pub height: u32,
    /// `check(game)`: true when the gear shown is at `equip_alt` instead of `equip`.
    pub equip_check: u32,
    pub equip_alt: u32,
    pub equip: u32,
    pub slots: u8,
    pub item_size: u8,
    /// `stat(item, number) -> f32`.
    pub stat: u32,
    /// `bar(widget, width, height, name, colour)`: the plainest widget constructor.
    pub bar: u32,
    /// Widget layout: its base size, the grid headers of its two layers, and a mask.
    pub base: u32,
    pub grid: u32,
    pub background_grid: u32,
    pub mask: u32,
    /// `background(widget, x, y, z, w, h)` and `commit_background(widget)`.
    pub background: u32,
    pub commit_background: u32,
    /// `panel(widget, x, y, z, w, h, colour)`, `glyph(font, char, grid, x, y, z, depth,
    /// colour)`, the text and title fonts and `commit(widget)`.
    pub panel: u32,
    pub glyph: u32,
    pub font: u32,
    pub title_font: u32,
    pub commit: u32,
    /// `sound(game, id, volume, pitch)`, and `close_top(game) -> bool`, which closes the
    /// client's top window.
    pub sound: u32,
    pub close_top: u32,
    /// The system menu's open flag, and those of the windows that count as open.
    pub system: u32,
    pub windows: Vec<u32>,
    /// The inventory's and crafting's open flags: those windows may stay open with this
    /// one.
    pub keep: Vec<u32>,
    /// `hide(widget)`: takes a widget off the screen until it is placed again.
    pub hide: u32,
    /// `local(widget, out)`: where the cursor is on a widget, in voxels from its bottom
    /// left.
    pub local: u32,
    /// The item names: a table of an English and a German C string per type, and how
    /// many types it has.
    pub names: u32,
    pub name_count: u32,
    /// Where an item keeps its rarity, and the colour of rarities 1 to 4.
    pub rarity: u32,
    pub rarity_colours: [[f32; 3]; 4],
    /// `notice(game)`: notes whether the player is moving as a window opens, which lets
    /// them go on moving with it open.
    pub notice: u32,
    /// The windows of the slot this one takes, crafting and skills: each one's open flag,
    /// where it is in the game object, and its `close(window)`.
    pub slot: Vec<[u32; 3]>,
    pub title: Vec<u8>,
}

impl Layout {
    pub fn read(data: &[u8]) -> Option<Layout> {
        let mut r = Reader(data);
        let layout = Layout {
            game: r.u32()?,
            ui_scale: r.u32()?,
            scale: r.f32()?,
            tilt: r.u32()?,
            place: r.u32()?,
            mouse: r.u32()?,
            mouse_slot: r.u8()?,
            width: r.u32()?,
            height: r.u32()?,
            equip_check: r.u32()?,
            equip_alt: r.u32()?,
            equip: r.u32()?,
            slots: r.u8()?,
            item_size: r.u8()?,
            stat: r.u32()?,
            bar: r.u32()?,
            base: r.u32()?,
            grid: r.u32()?,
            background_grid: r.u32()?,
            mask: r.u32()?,
            background: r.u32()?,
            commit_background: r.u32()?,
            panel: r.u32()?,
            glyph: r.u32()?,
            font: r.u32()?,
            title_font: r.u32()?,
            commit: r.u32()?,
            sound: r.u32()?,
            close_top: r.u32()?,
            system: r.u32()?,
            hide: r.u32()?,
            local: r.u32()?,
            names: r.u32()?,
            name_count: r.u32()?,
            rarity: r.u32()?,
            notice: r.u32()?,
            rarity_colours: {
                let mut colours = [[0f32; 3]; 4];
                for channel in 0..3 {
                    for colour in &mut colours {
                        colour[channel] = r.f32()?;
                    }
                }
                colours
            },
            windows: {
                let count = usize::from(r.u8()?);
                (0..count).map(|_| r.u32()).collect::<Option<_>>()?
            },
            keep: {
                let count = usize::from(r.u8()?);
                (0..count).map(|_| r.u32()).collect::<Option<_>>()?
            },
            slot: {
                let count = usize::from(r.u8()?);
                (0..count)
                    .map(|_| Some([r.u32()?, r.u32()?, r.u32()?]))
                    .collect::<Option<_>>()?
            },
            title: {
                let length = usize::from(r.u8()?);
                r.bytes(length)?
            },
        };
        let fits = r.0.is_empty()
            && layout.title.len() <= MAX_TITLE
            && layout.windows.len() <= MAX_WINDOWS
            && layout.keep.iter().all(|k| layout.windows.contains(k))
            && layout.slot.iter().all(|s| layout.windows.contains(&s[0]))
            && layout.name_count < 0x1000
            && layout.rarity + 4 <= u32::from(layout.item_size)
            && layout.slots <= 16
            && layout.item_size >= 8
            && layout.mouse_slot.is_multiple_of(8)
            && layout.grid < layout.mask
            && layout.mask < layout.background_grid
            && layout.background_grid < layout.base;
        fits.then_some(layout)
    }
}

/// What `stats-hotkey` is told: where the hotkey handler keeps whether keys work right
/// now, and the SDL scancode of the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    pub gate: u8,
    pub scancode: u16,
}

impl Keys {
    pub fn read(data: &[u8]) -> Option<Keys> {
        let mut r = Reader(data);
        let keys = Keys {
            gate: r.u8()?,
            scancode: r.u16()?,
        };
        (r.0.is_empty() && keys.scancode < 512).then_some(keys)
    }
}

/// How a place where the client asks whether a window is open answers: as a value, or by
/// comparing to 1 or to 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Value,
    EqualsOne,
    NotZero,
}

impl Answer {
    pub fn read(data: &[u8]) -> Option<Answer> {
        match data {
            [0] => Some(Answer::Value),
            [1] => Some(Answer::EqualsOne),
            [2] => Some(Answer::NotZero),
            _ => None,
        }
    }
}

/// A stat as the client's tooltips write it: one decimal below 10, none from there, and
/// no trailing zeros.
pub fn number(value: f32) -> String {
    if !value.is_finite() || value.abs() < 1e-5 {
        return "0".into();
    }
    let text = format!("{:.*}", usize::from(value.abs() < 10.0), value);
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// `text` as it fits in `room`, cut and ended with dots if it is wider.
pub fn fit(text: &[u8], room: i32, width: impl Fn(&[u8]) -> i32) -> Vec<u8> {
    if width(text) <= room {
        return text.to_vec();
    }
    let mut kept = text.len();
    while kept > 0 && width(&[&text[..kept], CUT].concat()) > room {
        kept -= 1;
    }
    let kept = text[..kept].trim_ascii_end();
    [kept, CUT].concat()
}

/// How far a smoothed value moves toward its target in `elapsed` ms, as the client's
/// windows follow the cursor.
pub fn follow(value: f32, target: f32, elapsed: f64) -> f32 {
    let share = 1.0 - (-TILT_RATE * elapsed).exp();
    value + (target - value) * share as f32
}

#[cfg(windows)]
pub use game::{Hotkey, Window, hotkey, opened, walked, window};

#[cfg(windows)]
mod game {
    use super::*;
    use crate::context::Context;
    use crate::log::log;
    use std::ptr;
    use std::time::Instant;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc,
    };
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;

    type Place = unsafe extern "win64" fn(*mut u8, *const [f32; 3], *const [f32; 3], f32, i32);
    type Check = unsafe extern "win64" fn(*mut u8) -> u8;
    type Stat = unsafe extern "win64" fn(*const u8, u8) -> f32;
    type Bar = unsafe extern "win64" fn(*mut u8, i32, i32, *const u8, *const [f32; 3]) -> *mut u8;
    type Background = unsafe extern "win64" fn(*mut u8, i32, i32, i32, i32, i32);
    type Commit = unsafe extern "win64" fn(*mut u8);
    type Hide = unsafe extern "win64" fn(*mut u8);
    /// A client function that takes one object and returns nothing worth reading.
    type Call = unsafe extern "win64" fn(*mut u8);
    type Local = unsafe extern "win64" fn(*mut u8, *mut [f32; 2]);
    type Panel = unsafe extern "win64" fn(*mut u8, i32, i32, i32, i32, i32, *const [f32; 3]);
    type Glyph =
        unsafe extern "win64" fn(*const u8, u32, *mut u8, i32, i32, i32, i32, *const [f32; 3]);
    type Sound = unsafe extern "win64" fn(*mut u8, u16, f32, f32);
    type Mouse = unsafe extern "win64" fn(*mut u8, *mut [f32; 2]);
    type Keyboard = unsafe extern "C" fn(*mut i32) -> *const u8;

    /// The window, once `stats-window` is installed, for the other hooks.
    static WINDOW: AtomicPtr<Window> = AtomicPtr::new(ptr::null_mut());

    fn now() -> u64 {
        // SAFETY: no arguments
        unsafe { GetTickCount64() }
    }

    /// A loaded client font: where it is, and how tall its glyphs are.
    #[derive(Clone, Copy)]
    struct Font(*const u8, i32);

    impl Font {
        /// # Safety
        /// `at` must be one of the client's fonts.
        unsafe fn at(at: usize) -> Option<Font> {
            let font = at as *const u8;
            // SAFETY: the header is four u32s, all 0 until the font is loaded
            let header = unsafe { font.add(FONT_HEADER).cast::<[u32; 4]>().read_unaligned() };
            (!header.contains(&0) && header[3] <= MAX_GLYPH).then_some(Font(font, header[3] as i32))
        }

        fn advance(self, c: u8) -> i32 {
            // SAFETY: 256 advances follow the header
            unsafe {
                self.0
                    .add(FONT_ADVANCE + 4 * usize::from(c))
                    .cast::<i32>()
                    .read_unaligned()
            }
        }

        fn width(self, text: &[u8]) -> i32 {
            text.iter().map(|&c| self.advance(c)).sum()
        }
    }

    /// Where the window's parts go, in voxels from its top left.
    #[derive(Clone, Copy)]
    struct Frame {
        width: i32,
        height: i32,
        first_row: i32,
        row: i32,
        short_x: i32,
    }

    impl Frame {
        /// The row the cursor is on, from where it is in voxels from the bottom left.
        fn hovered(self, at: [f32; 2]) -> Option<usize> {
            let (x, from_top) = (at[0], self.height as f32 - at[1]);
            if !(0.0..self.width as f32).contains(&x) || from_top < self.first_row as f32 {
                return None;
            }
            let row = ((from_top - self.first_row as f32) / self.row as f32) as usize;
            (row < ROWS.len()).then_some(row)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn finds_the_row_under_the_cursor() {
            let frame = Frame {
                width: 100,
                height: 100,
                first_row: 20,
                row: 10,
                short_x: 50,
            };
            // y counts from the bottom
            assert_eq!(frame.hovered([50.0, 79.0]), Some(0));
            assert_eq!(frame.hovered([50.0, 61.0]), Some(1));
            assert_eq!(frame.hovered([50.0, 85.0]), None);
            assert_eq!(frame.hovered([150.0, 79.0]), None);
            assert_eq!(frame.hovered([50.0, 5.0]), None);
        }
    }

    /// A widget of the client's kind, made by its bar constructor, and its size now.
    struct Canvas {
        widget: Cell<*mut u8>,
        size: Cell<(i32, i32)>,
        /// Whether it was placed since it was last hidden.
        placed: Cell<bool>,
    }

    impl Canvas {
        fn new() -> Canvas {
            Canvas {
                widget: Cell::new(ptr::null_mut()),
                size: Cell::new((0, 0)),
                placed: Cell::new(false),
            }
        }
    }

    /// What a tooltip shows: the row, and each item adding to its stat, its name, how much
    /// as bits and its rarity's colour.
    #[derive(Clone, PartialEq)]
    struct Tip {
        row: usize,
        items: Vec<(Vec<u8>, u32, [u32; 3])>,
    }

    pub struct Window {
        layout: Layout,
        base: usize,
        keyboard: Keyboard,
        main: Canvas,
        frame: Cell<Option<Frame>>,
        /// The stats last drawn, as bits.
        shown: Cell<Option<[u32; 7]>>,
        tooltip: Canvas,
        tip: RefCell<Option<Tip>>,
        /// The tilt, across and down, and when it was last moved.
        tilt: Cell<(f32, f32)>,
        tilted: Cell<Option<Instant>>,
        /// Whether the key was down last frame.
        held: Cell<bool>,
    }

    impl Window {
        /// None when SDL is not loaded.
        pub fn new(layout: Layout, base: usize) -> Option<&'static Window> {
            // SAFETY: names NUL-terminated, and SDL_GetKeyboardState has that type
            let keyboard = unsafe {
                let sdl = GetModuleHandleA(c"SDL2.dll".as_ptr().cast());
                if sdl.is_null() {
                    return None;
                }
                let found = GetProcAddress(sdl, c"SDL_GetKeyboardState".as_ptr().cast())?;
                std::mem::transmute::<unsafe extern "system" fn() -> isize, Keyboard>(found)
            };
            let window: &'static Window = Box::leak(Box::new(Window {
                layout,
                base,
                keyboard,
                main: Canvas::new(),
                frame: Cell::new(None),
                shown: Cell::new(None),
                tooltip: Canvas::new(),
                tip: RefCell::new(None),
                tilt: Cell::new((0.0, 0.0)),
                tilted: Cell::new(None),
                held: Cell::new(false),
            }));
            WINDOW.store(ptr::from_ref(window).cast_mut(), Ordering::Release);
            Some(window)
        }

        fn at(&self, rva: u32) -> usize {
            self.base + rva as usize
        }

        /// # Safety
        /// `rva` must be the start of a function of type `F` in the client.
        unsafe fn function<F: Copy>(&self, rva: u32) -> F {
            let address = self.at(rva);
            // SAFETY: the caller's word, and F is a function pointer type
            unsafe { std::mem::transmute_copy(&address) }
        }

        /// The game object, once there is one.
        fn game(&self) -> Option<*mut u8> {
            // SAFETY: the plan's global for it
            let game = unsafe { (self.at(self.layout.game) as *const *mut u8).read() };
            (!game.is_null()).then_some(game)
        }

        /// # Safety
        /// `game` must be the game object.
        unsafe fn flag(&self, game: *mut u8, offset: u32) -> bool {
            // SAFETY: the caller's word, and the plan's offset of an open flag in it
            unsafe { game.add(offset as usize).read() & 1 != 0 }
        }

        /// Whether a window is open that this one gives way to: any but the inventory.
        /// The system menu leaves it open, as it does crafting and the skills.
        ///
        /// # Safety
        /// As for `flag`.
        unsafe fn other_open(&self, game: *mut u8) -> bool {
            let l = &self.layout;
            // SAFETY: the caller's word
            unsafe {
                l.windows
                    .iter()
                    .any(|&w| !l.keep.contains(&w) && self.flag(game, w))
            }
        }

        /// # Safety
        /// On the client's main thread.
        unsafe fn sound(&self, game: *mut u8, id: u16) {
            // SAFETY: the client's sound player, as its windows call it
            unsafe { self.function::<Sound>(self.layout.sound)(game, id, 1.0, 1.0) }
        }

        /// Closes the window, with the client's sound if `sound`.
        ///
        /// # Safety
        /// On the client's main thread.
        unsafe fn close(&self, game: *mut u8, why: &str, sound: bool) {
            if OPEN.swap(false, Ordering::Relaxed) {
                log!("stats window closed ({why})");
                if sound {
                    // SAFETY: the caller's word
                    unsafe { self.sound(game, CLOSE_SOUND) };
                }
            }
        }

        /// The text and title fonts, once the client has loaded them.
        fn fonts(&self) -> Option<(Font, Font)> {
            // SAFETY: the plan's fonts
            unsafe {
                Some((
                    Font::at(self.at(self.layout.font))?,
                    Font::at(self.at(self.layout.title_font))?,
                ))
            }
        }

        /// Where the parts go, for these fonts.
        fn measure(text: Font, title: Font, heading: &[u8]) -> Option<Frame> {
            let name = ROWS.iter().map(|r| text.width(r.name.as_bytes())).max()?;
            let short = ROWS.iter().map(|r| text.width(r.short.as_bytes())).max()?;
            let rows = name + GAP + short + VALUE_GAP + text.width(WIDEST_VALUE);
            let width = 2 * MARGIN + rows.max(title.width(heading));
            let first_row = MARGIN + title.1 + TITLE_GAP;
            let mut row = text.1 + ROW_GAP;
            // tighter rows if the grid would not hold the window otherwise
            loop {
                let height = first_row + ROWS.len() as i32 * row + MARGIN;
                if width * height * DEPTH <= MAX_VOXELS {
                    break Some(Frame {
                        width,
                        height,
                        first_row,
                        row,
                        short_x: MARGIN + name + GAP,
                    });
                }
                if row <= text.1 {
                    break None;
                }
                row -= 1;
            }
        }

        /// Gives a canvas this size, making its widget the first time the way the client
        /// makes its own, and draws its background frame.
        ///
        /// # Safety
        /// On the client's main thread, with `width * height * DEPTH` within the grid.
        unsafe fn shape(&self, canvas: &Canvas, width: i32, height: i32) -> Option<*mut u8> {
            let l = &self.layout;
            // SAFETY: fresh zeroed pages for the client's own constructor, then its own
            // layout of a widget's two layers
            unsafe {
                if canvas.widget.get().is_null() {
                    let widget: *mut u8 = VirtualAlloc(
                        ptr::null(),
                        l.base as usize + OWN_FIELDS,
                        MEM_COMMIT | MEM_RESERVE,
                        PAGE_READWRITE,
                    )
                    .cast();
                    if widget.is_null() {
                        return None;
                    }
                    let name = [0u8; 0x40];
                    let colour = [0f32; 3];
                    // a bar adds 5 to the height it is given, and is 5 deep
                    self.function::<Bar>(l.bar)(widget, width, height - 5, name.as_ptr(), &colour);
                    canvas.widget.set(widget);
                } else if canvas.size.get() != (width, height) {
                    let widget = canvas.widget.get();
                    for header in [l.grid, l.background_grid] {
                        let header = widget.add(header as usize).cast::<i32>();
                        header.add(1).write_unaligned(width);
                        header.add(2).write_unaligned(height);
                    }
                    // the background layer is laid out as the front one is
                    let background = widget.add(l.background_grid as usize);
                    ptr::write_bytes(background.add(16), 0, (l.mask - l.grid - 16) as usize);
                    let mask = l.mask + (l.background_grid - l.grid);
                    ptr::write_bytes(widget.add(mask as usize), 0, 0x4000);
                } else {
                    return Some(canvas.widget.get());
                }
                let widget = canvas.widget.get();
                self.function::<Background>(l.background)(widget, 0, 0, 0, width, height);
                self.function::<Commit>(l.commit_background)(widget);
                canvas.size.set((width, height));
                Some(widget)
            }
        }

        /// Clears a canvas's front layer and draws its panel, ready for text.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped.
        unsafe fn panel(&self, canvas: &Canvas) {
            let l = &self.layout;
            let (width, height) = canvas.size.get();
            let widget = canvas.widget.get();
            // SAFETY: the caller's word, and the grid's own size
            unsafe {
                let voxels = width * height * DEPTH;
                ptr::write_bytes(widget.add(l.grid as usize + 16), 0, voxels as usize * 3);
                ptr::write_bytes(widget.add(l.mask as usize), 0, 0x4000);
                self.function::<Panel>(l.panel)(widget, 0, 0, 0, width, height, &PANEL);
            }
        }

        /// Writes text on a canvas's front layer from `x`, `y` voxels from its top left.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped and the font loaded.
        unsafe fn write(
            &self,
            canvas: &Canvas,
            font: Font,
            text: &[u8],
            at: (i32, i32),
            colour: &[f32; 3],
        ) {
            let grid = canvas.widget.get().wrapping_add(self.layout.grid as usize);
            // SAFETY: the caller's word
            unsafe {
                let glyph = self.function::<Glyph>(self.layout.glyph);
                let mut x = at.0;
                for &c in text {
                    glyph(font.0, c.into(), grid, x, at.1, TEXT_Z, TEXT_DEPTH, colour);
                    x += font.advance(c);
                }
            }
        }

        /// Hands a canvas's front layer to the client to build.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped.
        unsafe fn commit(&self, canvas: &Canvas) {
            // SAFETY: the caller's word
            unsafe { self.function::<Commit>(self.layout.commit)(canvas.widget.get()) }
        }

        /// Draws a canvas for this frame.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped.
        unsafe fn place(&self, canvas: &Canvas, at: [f32; 3], rotation: &[f32; 3], scale: f32) {
            // SAFETY: the caller's word
            unsafe {
                self.function::<Place>(self.layout.place)(
                    canvas.widget.get(),
                    &at,
                    rotation,
                    scale,
                    1,
                )
            };
            canvas.placed.set(true);
        }

        /// Takes a canvas off the screen, if it is on it. The client keeps drawing a widget
        /// where it was last placed until it is hidden.
        ///
        /// # Safety
        /// On the client's main thread.
        unsafe fn hide(&self, canvas: &Canvas) {
            if canvas.placed.replace(false) {
                // SAFETY: the widget was made and placed, and the caller's word
                unsafe { self.function::<Hide>(self.layout.hide)(canvas.widget.get()) };
            }
        }

        /// The gear: each equipped item.
        ///
        /// # Safety
        /// `game` must be the game object.
        unsafe fn gear(&self, game: *mut u8) -> Vec<*const u8> {
            let l = &self.layout;
            // SAFETY: the client's own check of which gear to show, as its tooltips use it
            unsafe {
                let alternative = self.function::<Check>(l.equip_check)(game) != 0;
                let gear = game.add(if alternative { l.equip_alt } else { l.equip } as usize);
                (0..usize::from(l.slots))
                    .map(|slot| gear.add(slot * usize::from(l.item_size)).cast_const())
                    .filter(|item| item.cast::<u32>().read_unaligned() != 0)
                    .collect()
            }
        }

        /// What an item adds to a row's stat.
        ///
        /// # Safety
        /// `item` must be one of `gear`.
        unsafe fn stat(&self, item: *const u8, row: &Row) -> f32 {
            // SAFETY: the client's own stat getter
            unsafe { self.function::<Stat>(self.layout.stat)(item, row.stat) }
        }

        /// An item's name, as the client's names table has it in English.
        ///
        /// # Safety
        /// `item` must be one of `gear`.
        unsafe fn name(&self, item: *const u8) -> Vec<u8> {
            let l = &self.layout;
            // SAFETY: the plan's table, a C string per type
            unsafe {
                let kind = item.cast::<u32>().read_unaligned();
                let name = if kind < l.name_count {
                    (self.at(l.names) as *const *const u8)
                        .add(2 * kind as usize)
                        .read()
                } else {
                    ptr::null()
                };
                let image = self.base..self.base + 0x1000_0000;
                if name.is_null() || !image.contains(&(name as usize)) {
                    return format!("ITEM {kind}").into_bytes();
                }
                (0..MAX_NAME)
                    .map(|i| name.add(i).read())
                    .take_while(|&c| c != 0)
                    .map(|c| {
                        if (b' '..=b'~').contains(&c) {
                            c.to_ascii_uppercase()
                        } else {
                            b'?'
                        }
                    })
                    .collect()
            }
        }

        /// The colour of an item's name: its rarity's, as the client's tooltips draw it.
        ///
        /// # Safety
        /// `item` must be one of `gear`.
        unsafe fn colour(&self, item: *const u8) -> [f32; 3] {
            let l = &self.layout;
            // SAFETY: the caller's word, and the rarity is within the item
            let rarity = unsafe { item.add(l.rarity as usize).cast::<u32>().read_unaligned() };
            match (rarity / 5).checked_sub(1) {
                Some(index @ 0..4) => l.rarity_colours[index as usize],
                _ => VALUE,
            }
        }

        /// The rotation the window is drawn at: toward the cursor, as far as the client's
        /// window tilt setting has it.
        ///
        /// # Safety
        /// `game` must be the game object, on the client's main thread.
        unsafe fn rotation(&self, game: *mut u8) -> [f32; 3] {
            let l = &self.layout;
            // SAFETY: the cursor object and the screen size, as the client's windows read
            // them
            unsafe {
                let mouse = game.add(l.mouse as usize).cast::<*mut u8>().read();
                let width = (self.at(l.width) as *const u32).read() as f32;
                let height = (self.at(l.height) as *const u32).read() as f32;
                let setting = game.add(l.tilt as usize).cast::<f32>().read_unaligned();
                let (mut across, mut down) = self.tilt.get();
                if !mouse.is_null() && height > 0.0 {
                    let method = mouse
                        .cast::<*const usize>()
                        .read()
                        .add(usize::from(l.mouse_slot) / 8)
                        .read();
                    let mut at = [0f32; 2];
                    std::mem::transmute::<usize, Mouse>(method)(mouse, &mut at);
                    let now = Instant::now();
                    let elapsed = self
                        .tilted
                        .replace(Some(now))
                        .map_or(0.0, |then| now.duration_since(then).as_secs_f64() * 1000.0);
                    across = follow(
                        across,
                        (at[0] - width / 2.0) * TILT_ACROSS / height + SLOT_TURN,
                        elapsed,
                    );
                    down = follow(down, (at[1] - height / 2.0) * TILT_DOWN / height, elapsed);
                    self.tilt.set((across, down));
                }
                [setting * down, 0.0, setting * across]
            }
        }

        /// Draws the stats.
        ///
        /// # Safety
        /// On the client's main thread, with the window shaped to `frame`.
        unsafe fn draw(&self, frame: Frame, stats: &[f32; 7]) {
            let Some((text, title)) = self.fonts() else {
                return;
            };
            let c = &self.main;
            // SAFETY: the caller's word
            unsafe {
                self.panel(c);
                self.write(c, title, &self.layout.title, (MARGIN, MARGIN), &TITLE);
                let mut y = frame.first_row;
                for (row, value) in ROWS.iter().zip(stats) {
                    let value = number(*value);
                    self.write(c, text, row.name.as_bytes(), (MARGIN, y), &row.colour);
                    self.write(c, text, row.short.as_bytes(), (frame.short_x, y), &SHORT);
                    let x = frame.width - MARGIN - text.width(value.as_bytes());
                    self.write(c, text, value.as_bytes(), (x, y), &VALUE);
                    y += frame.row;
                }
                self.commit(c);
            }
        }

        /// The tooltip for a row: each item that adds to its stat, the most first.
        ///
        /// # Safety
        /// `game` must be the game object.
        unsafe fn tip(&self, game: *mut u8, row: usize) -> Tip {
            // SAFETY: the caller's word
            unsafe {
                let mut items: Vec<(Vec<u8>, f32, [f32; 3])> = self
                    .gear(game)
                    .into_iter()
                    .map(|item| {
                        (
                            self.name(item),
                            self.stat(item, &ROWS[row]),
                            self.colour(item),
                        )
                    })
                    .filter(|(_, value, _)| value.is_finite() && value.abs() >= 1e-5)
                    .collect();
                items.sort_by(|a, b| b.1.total_cmp(&a.1));
                items.truncate(MAX_TIP_ITEMS);
                Tip {
                    row,
                    items: items
                        .into_iter()
                        .map(|(n, v, c)| (n, v.to_bits(), c.map(f32::to_bits)))
                        .collect(),
                }
            }
        }

        /// Shapes and draws the tooltip. Its size, or None if it does not fit a widget.
        ///
        /// # Safety
        /// On the client's main thread.
        unsafe fn draw_tip(&self, tip: &Tip) -> Option<(i32, i32)> {
            let (text, title) = self.fonts()?;
            let row = &ROWS[tip.row];
            let lines: Vec<(Vec<u8>, String, [f32; 3])> = tip
                .items
                .iter()
                .map(|(name, value, colour)| {
                    let value = f32::from_bits(*value);
                    let sign = if value > 0.0 { "+" } else { "" };
                    let number = format!("{sign}{}", number(value));
                    let name = fit(name, MAX_NAME_WIDTH, |s| text.width(s));
                    (name, number, colour.map(f32::from_bits))
                })
                .collect();
            let names = lines.iter().map(|(n, ..)| text.width(n)).max().unwrap_or(0);
            let values = lines
                .iter()
                .map(|(_, v, _)| text.width(v.as_bytes()))
                .max()
                .unwrap_or(0);
            let empty = text.width(NO_GEAR);
            let heading = title.width(row.name.as_bytes());
            let width = 2 * MARGIN + heading.max(empty).max(names + VALUE_GAP + values);
            let rows = lines.len().max(1) as i32;
            let height = MARGIN + title.1 + TITLE_GAP + rows * (text.1 + ROW_GAP) + MARGIN;
            if width * height * DEPTH > MAX_VOXELS {
                return None;
            }
            let c = &self.tooltip;
            // SAFETY: the caller's word, and the size fits the grid
            unsafe {
                self.shape(c, width, height)?;
                self.panel(c);
                self.write(c, title, row.name.as_bytes(), (MARGIN, MARGIN), &row.colour);
                let mut y = MARGIN + title.1 + TITLE_GAP;
                if lines.is_empty() {
                    self.write(c, text, NO_GEAR, (MARGIN, y), &SHORT);
                }
                for (name, value, colour) in &lines {
                    self.write(c, text, name, (MARGIN, y), colour);
                    let x = width - MARGIN - text.width(value.as_bytes());
                    self.write(c, text, value.as_bytes(), (x, y), &row.colour);
                    y += text.1 + ROW_GAP;
                }
                self.commit(c);
            }
            Some((width, height))
        }

        /// Takes the window and its tooltip off the screen.
        ///
        /// # Safety
        /// On the client's main thread.
        unsafe fn hide_all(&self) {
            self.tilted.set(None);
            // SAFETY: the caller's word
            unsafe {
                self.hide(&self.tooltip);
                self.hide(&self.main);
            }
        }

        /// Once a frame.
        ///
        /// # Safety
        /// From the client's frame, on its main thread.
        pub unsafe fn frame(&self) {
            let Some(game) = self.game() else {
                return;
            };
            let l = &self.layout;
            // SAFETY: the plan's layout of this client, on its main thread
            unsafe {
                if self.pressed() {
                    self.press(game);
                }
                if OPEN.load(Ordering::Relaxed) && self.other_open(game) {
                    self.close(game, "another window opened", false);
                }
                // closed, or hidden in menus, while typing and on the start screen
                if !OPEN.load(Ordering::Relaxed)
                    || now().saturating_sub(SEEN_AT.load(Ordering::Relaxed)) > SEEN
                {
                    self.hide_all();
                    return;
                }
                let frame = match self.frame.get() {
                    Some(frame) => frame,
                    None => {
                        let Some((text, title)) = self.fonts() else {
                            return;
                        };
                        let Some(frame) = Self::measure(text, title, &l.title) else {
                            log!("stats window: the fonts make it too big for a widget");
                            return;
                        };
                        if self.shape(&self.main, frame.width, frame.height).is_none() {
                            return;
                        }
                        log!(
                            "stats window made, {} by {} voxels",
                            frame.width,
                            frame.height
                        );
                        self.frame.set(Some(frame));
                        frame
                    }
                };
                let stats = self
                    .gear(game)
                    .into_iter()
                    .fold([0f32; 7], |mut sums, item| {
                        for (sum, row) in sums.iter_mut().zip(&ROWS) {
                            *sum += self.stat(item, row);
                        }
                        sums
                    });
                let bits = stats.map(f32::to_bits);
                if self.shown.get() != Some(bits) {
                    self.draw(frame, &stats);
                    self.shown.set(Some(bits));
                }
                // in the left slot, as the client places its side windows, and with its
                // outer edge as far from the screen's as the inventory's is on the right
                let rotation = self.rotation(game);
                let ui = game
                    .add(l.ui_scale as usize)
                    .cast::<f32>()
                    .read_unaligned()
                    .min(SLOT_MAX_UI);
                let setting = game.add(l.tilt as usize).cast::<f32>().read_unaligned();
                // the inventory's open flag is its first own field, after its base
                let inventory = l
                    .keep
                    .first()
                    .and_then(|&flag| flag.checked_sub(l.base))
                    .map(|widget| {
                        let header = (widget + l.grid) as usize;
                        game.add(header + 4).cast::<i32>().read_unaligned()
                    })
                    .filter(|width| INVENTORY_WIDTHS.contains(width))
                    .unwrap_or(frame.width);
                let wide = (inventory as f32 - SLOT_WIDE).max(0.0) * SLOT_WIDE_SHARE;
                let half = |width: i32| width as f32 * SLOT_WIDE_SHARE;
                let edge = wide + SLOT_BASE + (1.0 - setting) * SLOT_BASE_TILT + half(inventory);
                let left = (edge - half(frame.width)) * ui;
                let scale = ui * l.scale;
                self.place(&self.main, [-left, 0.0, WINDOW_DEPTH], &rotation, scale);

                // the tooltip of the row under the cursor, to the window's right
                let mut at = [0f32; 2];
                self.function::<Local>(l.local)(self.main.widget.get(), &mut at);
                let Some(row) = frame.hovered(at) else {
                    self.hide(&self.tooltip);
                    return;
                };
                let tip = self.tip(game, row);
                if self.tip.borrow().as_ref() != Some(&tip) {
                    if self.draw_tip(&tip).is_none() {
                        self.hide(&self.tooltip);
                        return;
                    }
                    *self.tip.borrow_mut() = Some(tip);
                }
                let (width, _) = self.tooltip.size.get();
                let across = (frame.width + width) as f32 / 2.0 + TIP_GAP as f32;
                let near = TIP_DEPTH / WINDOW_DEPTH;
                self.place(
                    &self.tooltip,
                    [(across * scale - left) * near, 0.0, TIP_DEPTH],
                    &rotation,
                    scale * near,
                );
            }
        }

        /// Whether the key went down since the last frame.
        fn pressed(&self) -> bool {
            let key = KEY.load(Ordering::Relaxed);
            let mut count = 0;
            // SAFETY: SDL's key states, as many as it says
            let down = unsafe {
                let states = (self.keyboard)(&mut count);
                !states.is_null()
                    && (key as i64) < i64::from(count)
                    && states.add(key as usize).read() != 0
            };
            let was = self.held.replace(down);
            if down != was {
                log!("stats key {}", if down { "down" } else { "up" });
            }
            down && !was
        }

        /// The key went down: closes the window, or opens it where the client would open
        /// one of its own.
        ///
        /// # Safety
        /// On the client's main thread, with `game` the game object.
        unsafe fn press(&self, game: *mut u8) {
            let open = OPEN.load(Ordering::Relaxed);
            let hotkeys = HOTKEYS.load(Ordering::Relaxed)
                && now().saturating_sub(SEEN_AT.load(Ordering::Relaxed)) <= SEEN;
            // SAFETY: the caller's word
            unsafe {
                let system = self.flag(game, self.layout.system);
                log!(
                    "stats key: hotkeys {}, system menu {}, open {open}, other window {}",
                    if hotkeys { "on" } else { "off" },
                    if system { "open" } else { "closed" },
                    self.other_open(game)
                );
                if open {
                    self.close(game, "key", true);
                    return;
                }
                if !hotkeys || system {
                    return;
                }
                // crafting and the skills give way, as they do to each other, and any other
                // window as to the client's own
                for &[flag, window, close] in &self.layout.slot {
                    if self.flag(game, flag) {
                        self.function::<Call>(close)(game.add(window as usize));
                    }
                }
                let close_top = self.function::<Check>(self.layout.close_top);
                for _ in 0..CLOSE_AT_MOST {
                    if !self.other_open(game) || close_top(game) == 0 {
                        break;
                    }
                }
                if self.other_open(game) {
                    log!("stats window not opened: another window stays open");
                    return;
                }
                OPEN.store(true, Ordering::Relaxed);
                log!("stats window opened");
                self.function::<Call>(self.layout.notice)(game);
                self.sound(game, OPEN_SOUND);
            }
        }
    }

    fn installed() -> Option<&'static Window> {
        // SAFETY: set once to a leaked Window, never freed
        unsafe { WINDOW.load(Ordering::Acquire).as_ref() }
    }

    pub struct Hotkey {
        gate: u8,
    }

    impl Hotkey {
        pub fn new(keys: Keys) -> Hotkey {
            KEY.store(keys.scancode.into(), Ordering::Relaxed);
            Hotkey { gate: keys.gate }
        }
    }

    /// For `context`: the handlers, each on its hook's state.
    ///
    /// # Safety
    /// `state` must be the `Window` this handler's hook was made with.
    pub unsafe fn window(_: &mut Context, state: *const c_void) {
        // SAFETY: the caller's word
        unsafe { (*state.cast::<Window>()).frame() }
    }

    /// # Safety
    /// `state` must be a `Hotkey`.
    pub unsafe fn hotkey(context: &mut Context, state: *const c_void) {
        // SAFETY: the caller's word, and the hotkey handler keeps the byte on its stack
        unsafe {
            let gate = usize::from((*state.cast::<Hotkey>()).gate);
            let works = ((context.rsp() as usize + gate) as *const u8).read() == 1;
            HOTKEYS.store(works, Ordering::Relaxed);
            SEEN_AT.store(now(), Ordering::Relaxed);
        }
    }

    /// Where the client closes its windows as the player starts to move, unless they were
    /// moving when the windows opened.
    ///
    /// # Safety
    /// From there, on the client's main thread.
    pub unsafe fn walked(_: &mut Context, _: *const c_void) {
        let Some(window) = installed() else {
            return;
        };
        if let Some(game) = window.game() {
            // SAFETY: the caller's word
            unsafe { window.close(game, "walked", true) };
        }
    }

    /// Right where the client has read whether the system menu is open, the last of its
    /// tests of whether any window is: the window counts as one.
    ///
    /// # Safety
    /// From there, with `state` an `Answer`.
    pub unsafe fn opened(context: &mut Context, state: *const c_void) {
        const ZERO_FLAG: u64 = 1 << 6;
        if installed().is_none() || !OPEN.load(Ordering::Relaxed) {
            return;
        }
        // SAFETY: the caller's word
        match unsafe { *state.cast::<Answer>() } {
            Answer::Value => context.rax = 1,
            Answer::EqualsOne => context.rflags |= ZERO_FLAG,
            Answer::NotZero => context.rflags &= !ZERO_FLAG,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_numbers_as_the_tooltips_do() {
        assert_eq!(number(0.0), "0");
        assert_eq!(number(3.25), "3.2");
        assert_eq!(number(3.0), "3");
        assert_eq!(number(9.96), "10");
        assert_eq!(number(12.6), "13");
        assert_eq!(number(-0.5), "-0.5");
        assert_eq!(number(f32::NAN), "0");
    }

    #[test]
    fn cuts_long_names_with_dots() {
        let width = |s: &[u8]| 10 * s.len() as i32;
        assert_eq!(fit(b"GOLD RING", 200, width), b"GOLD RING");
        assert_eq!(fit(b"COTTON SHOULDER ARMOR", 100, width), b"COTTON...");
        assert_eq!(fit(b"ABCDEFGHIJ", 30, width), b"...");
    }

    #[test]
    fn follows_the_cursor_at_the_clients_rate() {
        assert_eq!(follow(0.0, 10.0, 0.0), 0.0);
        let after = follow(0.0, 10.0, 100.0);
        assert!((after - 10.0 * (1.0 - (-0.5f64).exp()) as f32).abs() < 1e-4);
        assert!((follow(0.0, 10.0, 1e6) - 10.0).abs() < 1e-4);
    }

    #[test]
    fn reads_the_small_hooks() {
        let mut data = vec![0x3E];
        data.extend(14u16.to_le_bytes());
        assert_eq!(
            Keys::read(&data),
            Some(Keys {
                gate: 0x3E,
                scancode: 14
            })
        );
        assert_eq!(Keys::read(&data[1..]), None);
    }
}
