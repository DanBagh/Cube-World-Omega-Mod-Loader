//! Menus for mods: menus of the client's own kind that mods declare and their scripts
//! draw. This holds no menu of its own, only what any menu needs: making and placing
//! the client's widgets, its fonts and panels, the slots its menus share, its keys, the
//! cursor, and the client's way of closing menus as the player moves.
//!
//! Context hooks drive it, all on the client's main thread:
//! - `ui-frame`, once a frame next to where the controls menu places itself: runs the
//!   scripts, opens and closes menus, has their scripts draw them, and places them the
//!   way the client places its own. It holds what the others share, so it is installed
//!   first.
//! - `ui-hotkey`, where the client handles the keys of its menus, notes whether it would
//!   take a menu's key now. The keys themselves are read once a frame, as the client's
//!   own handler runs less often and can miss a short press.
//! - `ui-walk`, where the client closes its menus as the player starts to move, closes
//!   these too.
//! - `ui-open`, wherever the client asks whether any menu is open, makes them count, so
//!   the client frees the cursor and holds the camera while one is open.
//!
//! Slots, as the client has them: `left` is crafting's and the skills', and stays open
//! with the inventory and under the system menu. `center` is the controls menu's, and
//! gives way to any other menu.
//!
//! The plan names every function and offset of the client used.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The grid of a widget: 5 voxels deep, and at most this many voxels.
pub const DEPTH: i32 = 5;
pub const MAX_VOXELS: i32 = 0x20000;
/// Text sits on the panel and is 3 voxels deep, as in the inventory.
pub const TEXT_Z: i32 = 2;
pub const TEXT_DEPTH: i32 = 3;
/// What text is cut with when it does not fit.
pub const CUT: &[u8] = b"...";
/// The client's colors: its panels, its titles, its text and its quieter text.
pub const PANEL: [f32; 3] = [127.0, 127.0, 127.0];
pub const TITLE: [f32; 3] = [250.0, 180.0, 70.0];
pub const TEXT: [f32; 3] = [230.0, 230.0, 230.0];
pub const MUTED: [f32; 3] = [150.0, 150.0, 150.0];
/// The tilt toward the cursor, as the client's menus do it: degrees per screen height
/// across and down, and how fast it follows, per ms.
const TILT_ACROSS: f32 = 40.0;
const TILT_DOWN: f32 = 20.0;
const TILT_RATE: f64 = 0.005;
/// Item stats by the names scripts use, as the client's stat getter numbers them.
pub const STATS: [(&str, u8); 7] = [
    ("attack", 0),
    ("armor", 1),
    ("resistance", 2),
    ("health", 3),
    ("critical", 4),
    ("haste", 5),
    ("regeneration", 6),
];
/// Most menus a plan names as open-able, and the longest item name read.
pub const MAX_MENUS: usize = 16;
const MAX_NAME: usize = 40;

/// Whether the client took the keys of its menus when it last handled them, and when
/// that was, in ms since boot.
static HOTKEYS: AtomicBool = AtomicBool::new(false);
static SEEN_AT: AtomicU64 = AtomicU64::new(0);
/// Whether any mod's menu is open, for the client's tests, and whether the player has
/// started to move since the last frame.
static ANY_OPEN: AtomicBool = AtomicBool::new(false);
static WALKED: AtomicBool = AtomicBool::new(false);

/// Where a menu goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Left,
    Center,
}

impl Slot {
    pub fn named(name: &str) -> Option<Slot> {
        match name {
            "left" => Some(Slot::Left),
            "center" => Some(Slot::Center),
            _ => None,
        }
    }
}

/// A menu a mod declares: whose, its id, its slot, the SDL scancode of its key, and what
/// the controls menu calls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub owner: String,
    pub id: String,
    pub slot: Slot,
    pub key: Option<u16>,
    pub label: String,
}

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
    fn u32(&mut self) -> Option<u32> {
        self.take().map(u32::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }
    fn words(&mut self) -> Option<Vec<u32>> {
        let count = usize::from(self.u8()?);
        (0..count).map(|_| self.u32()).collect()
    }
}

/// What `ui-frame` is told about the client, as RVAs and offsets.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The global holding the game object.
    pub game: u32,
    /// The UI scale in the game object, what the inventory multiplies it by, and the
    /// menu tilt setting.
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
    /// `bar(widget, width, height, name, color)`: the plainest widget constructor.
    pub bar: u32,
    /// Widget layout: its base size, the grid headers of its two layers, and a mask.
    pub base: u32,
    pub grid: u32,
    pub background_grid: u32,
    pub mask: u32,
    /// `background(widget, x, y, z, w, h)` and `commit_background(widget)`.
    pub background: u32,
    pub commit_background: u32,
    /// `panel(widget, x, y, z, w, h, color)`, `glyph(font, char, grid, x, y, z, depth,
    /// color)`, the text and title fonts and `commit(widget)`.
    pub panel: u32,
    pub glyph: u32,
    pub font: u32,
    pub title_font: u32,
    pub commit: u32,
    /// `sound(game, id, volume, pitch)`, and `close_top(game) -> bool`, which closes the
    /// client's top menu.
    pub sound: u32,
    pub close_top: u32,
    /// The system menu's open flag.
    pub system: u32,
    /// `hide(widget)`: takes a widget off the screen until it is placed again.
    pub hide: u32,
    /// `local(widget, out)`: where the cursor is on a widget, in voxels from its bottom
    /// left.
    pub local: u32,
    /// The item names: a table of an English and a German C string per type, and how
    /// many types it has.
    pub names: u32,
    pub name_count: u32,
    /// Where an item keeps its rarity, and the color of rarities 1 to 4.
    pub rarity: u32,
    /// `notice(game)`: notes whether the player is moving as a menu opens, which lets
    /// them go on moving with it open.
    pub notice: u32,
    pub rarity_colors: [[f32; 3]; 4],
    /// The open flags of the client's menus.
    pub menus: Vec<u32>,
    /// The inventory's: it stays open beside the left slot.
    pub keep: Vec<u32>,
    /// The menus of the left slot, crafting and skills: each one's open flag, where it
    /// is in the game object, and its `close(menu)`.
    pub slot: Vec<[u32; 3]>,
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
            rarity_colors: {
                let mut colors = [[0f32; 3]; 4];
                for channel in 0..3 {
                    for color in &mut colors {
                        color[channel] = r.f32()?;
                    }
                }
                colors
            },
            menus: r.words()?,
            keep: r.words()?,
            slot: {
                let count = usize::from(r.u8()?);
                (0..count)
                    .map(|_| Some([r.u32()?, r.u32()?, r.u32()?]))
                    .collect::<Option<_>>()?
            },
        };
        let fits = r.0.is_empty()
            && layout.menus.len() <= MAX_MENUS
            && layout.keep.iter().all(|k| layout.menus.contains(k))
            && layout.slot.iter().all(|s| layout.menus.contains(&s[0]))
            && layout.name_count < 0x1000
            && layout.rarity + 4 <= u32::from(layout.item_size)
            && layout.slots <= 16
            && (8..=64).contains(&layout.item_size)
            && layout.mouse_slot.is_multiple_of(8)
            && layout.grid < layout.mask
            && layout.mask < layout.background_grid
            && layout.background_grid < layout.base;
        fits.then_some(layout)
    }
}

/// Where the hotkey handler keeps whether keys work right now.
pub fn gate(data: &[u8]) -> Option<u8> {
    match data {
        [gate] => Some(*gate),
        _ => None,
    }
}

/// How a place where the client asks whether a menu is open answers: as a value, or by
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
pub fn number(value: f64) -> String {
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
/// menus follow the cursor.
pub fn follow(value: f32, target: f32, elapsed: f64) -> f32 {
    let share = 1.0 - (-TILT_RATE * elapsed).exp();
    value + (target - value) * share as f32
}

/// Where a voxel of a widget's grid is kept, from voxels from its top left and a depth.
pub fn voxel_index(size: (i32, i32), x: i32, y: i32, z: i32) -> Option<usize> {
    let (width, height) = size;
    let inside = (0..width).contains(&x) && (0..height).contains(&y) && (0..DEPTH).contains(&z);
    // the grid counts rows from the bottom
    inside.then(|| (x + width * (height - 1 - y) + width * height * z) as usize)
}

/// Where a popup goes beside its menu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Right, left, above or below.
    pub side: Side,
    /// Voxels between the menu and the popup, and how far it is moved along the side,
    /// right or down.
    pub gap: i32,
    pub along: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
    Above,
    Below,
}

impl Side {
    pub fn named(name: &str) -> Option<Side> {
        match name {
            "right" => Some(Side::Right),
            "left" => Some(Side::Left),
            "above" => Some(Side::Above),
            "below" => Some(Side::Below),
            _ => None,
        }
    }
}

/// From a menu's center to a popup's, in voxels across and down the screen.
pub fn beside(menu: (i32, i32), popup: (i32, i32), anchor: Anchor) -> (f32, f32) {
    let half = |a: i32, b: i32| (a + b) as f32 / 2.0;
    let gap = anchor.gap as f32;
    let along = anchor.along as f32;
    match anchor.side {
        Side::Right => (half(menu.0, popup.0) + gap, along),
        Side::Left => (-half(menu.0, popup.0) - gap, along),
        Side::Above => (along, -half(menu.1, popup.1) - gap),
        Side::Below => (along, half(menu.1, popup.1) + gap),
    }
}

#[cfg(windows)]
pub use game::*;

#[cfg(windows)]
mod game {
    use super::*;
    use crate::context::Context;
    use crate::log::log;
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::ptr;
    use std::rc::Rc;
    use std::sync::OnceLock;
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
    /// A client function that takes one object and returns nothing worth reading.
    type Call = unsafe extern "win64" fn(*mut u8);
    type Local = unsafe extern "win64" fn(*mut u8, *mut [f32; 2]);
    type Panel = unsafe extern "win64" fn(*mut u8, i32, i32, i32, i32, i32, *const [f32; 3]);
    type Glyph =
        unsafe extern "win64" fn(*const u8, u32, *mut u8, i32, i32, i32, i32, *const [f32; 3]);
    type Sound = unsafe extern "win64" fn(*mut u8, u16, f32, f32);
    type Mouse = unsafe extern "win64" fn(*mut u8, *mut [f32; 2]);
    type Keyboard = unsafe extern "C" fn(*mut i32) -> *const u8;
    type Buttons = unsafe extern "C" fn(*mut i32, *mut i32) -> u32;

    /// How long menus stay drawn after the client last handled its hotkeys, in ms. It
    /// stops doing that in some menus, while typing and on the start screen.
    const SEEN: u64 = 500;
    /// The client's sounds for a menu opening and closing.
    const OPEN_SOUND: u16 = 0x63;
    const CLOSE_SOUND: u16 = 0x64;
    /// How many menus a menu closes, at most, as it opens.
    const CLOSE_AT_MOST: usize = 8;
    /// What the client's widgets take beyond their base, at most.
    const OWN_FIELDS: usize = 0x1000;
    /// Where a client font keeps its size and its glyph advances.
    const FONT_HEADER: usize = 0x40000;
    const FONT_ADVANCE: usize = 0x40010;
    /// Taller glyphs than this mean the font is not what the plan expects.
    const MAX_GLYPH: u32 = 64;
    /// How far from the camera menus are, and popups nearer, in front of them as the
    /// client's item tooltips are. A popup is shrunk to look the size it would at 8.
    const MENU_DEPTH: f32 = 8.0;
    const POPUP_DEPTH: f32 = 7.0;
    /// The left slot, as the client places it: turned this far toward the middle, at most
    /// this UI scale, and to the left of the middle by a base that grows as the tilt
    /// setting shrinks, plus a share of the menu's width past a point.
    const SLOT_TURN: f32 = 50.0;
    const SLOT_MAX_UI: f32 = 0.85;
    const SLOT_BASE: f32 = 3.0;
    const SLOT_BASE_TILT: f32 = 1.25;
    const SLOT_WIDE: f32 = 135.0;
    const SLOT_WIDE_SHARE: f32 = 0.016;
    /// Widths the inventory has, in voxels: the left slot mirrors its outer edge.
    const INVENTORY_WIDTHS: std::ops::Range<i32> = 32..1024;
    /// The size a menu has until its script gives it one.
    const FIRST_SIZE: (i32, i32) = (100, 100);

    fn now() -> u64 {
        // SAFETY: no arguments
        unsafe { GetTickCount64() }
    }

    fn seen() -> bool {
        now().saturating_sub(SEEN_AT.load(Ordering::Relaxed)) <= SEEN
    }

    /// The menus the plan declares, for the engine to make on its first frame.
    static DECLARED: OnceLock<Vec<Declared>> = OnceLock::new();

    pub fn declare(menus: Vec<Declared>) {
        let _ = DECLARED.set(menus);
    }

    /// A loaded client font: where it is, and how tall its glyphs are.
    #[derive(Clone, Copy)]
    pub struct Font(*const u8, pub i32);

    impl Font {
        /// # Safety
        /// `at` must be one of the client's fonts.
        unsafe fn at(at: usize) -> Option<Font> {
            let font = at as *const u8;
            // SAFETY: the header is four u32s, all 0 until the font is loaded
            let header = unsafe { font.add(FONT_HEADER).cast::<[u32; 4]>().read_unaligned() };
            (!header.contains(&0) && header[3] <= MAX_GLYPH).then_some(Font(font, header[3] as i32))
        }

        pub fn advance(self, c: u8) -> i32 {
            // SAFETY: 256 advances follow the header
            unsafe {
                self.0
                    .add(FONT_ADVANCE + 4 * usize::from(c))
                    .cast::<i32>()
                    .read_unaligned()
            }
        }

        pub fn width(self, text: &[u8]) -> i32 {
            text.iter().map(|&c| self.advance(c)).sum()
        }
    }

    /// The client: its functions and offsets, as the plan names them.
    pub struct Game {
        layout: Layout,
        base: usize,
        keyboard: Keyboard,
        buttons: Buttons,
    }

    impl Game {
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
        pub fn object(&self) -> Option<*mut u8> {
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

        /// Whether one of the client's menus is open that a menu in `slot` gives way
        /// to.
        ///
        /// # Safety
        /// As for `flag`.
        unsafe fn in_the_way(&self, game: *mut u8, slot: Slot) -> bool {
            let l = &self.layout;
            // SAFETY: the caller's word
            unsafe {
                l.menus.iter().any(|&w| match slot {
                    Slot::Left => !l.keep.contains(&w) && self.flag(game, w),
                    Slot::Center => self.flag(game, w),
                }) || (slot == Slot::Center && self.flag(game, l.system))
            }
        }

        /// # Safety
        /// On the client's main thread.
        unsafe fn sound(&self, game: *mut u8, id: u16) {
            // SAFETY: the client's sound player, as its menus call it
            unsafe { self.function::<Sound>(self.layout.sound)(game, id, 1.0, 1.0) }
        }

        /// The client's text and title fonts, once it has loaded them.
        pub fn font(&self, title: bool) -> Option<Font> {
            let l = &self.layout;
            // SAFETY: the plan's fonts
            unsafe { Font::at(self.at(if title { l.title_font } else { l.font })) }
        }

        /// Gives a canvas this size, making its widget the first time the way the client
        /// makes its own, and draws its background frame.
        ///
        /// # Safety
        /// On the client's main thread, with the size checked to fit the grid.
        unsafe fn shape(&self, canvas: &Canvas, size: (i32, i32)) -> bool {
            let l = &self.layout;
            let (width, height) = size;
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
                        return false;
                    }
                    let name = [0u8; 0x40];
                    let color = [0f32; 3];
                    // a bar adds 5 to the height it is given, and is 5 deep
                    self.function::<Bar>(l.bar)(widget, width, height - 5, name.as_ptr(), &color);
                    canvas.widget.set(widget);
                } else if canvas.size.get() != size {
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
                    return true;
                }
                let widget = canvas.widget.get();
                self.function::<Background>(l.background)(widget, 0, 0, 0, width, height);
                self.function::<Call>(l.commit_background)(widget);
                canvas.size.set(size);
                true
            }
        }

        /// Clears a canvas's front layer.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped.
        unsafe fn clear(&self, canvas: &Canvas) {
            let l = &self.layout;
            let (width, height) = canvas.size.get();
            // SAFETY: the grid's own size
            unsafe {
                let widget = canvas.widget.get();
                let voxels = (width * height * DEPTH) as usize;
                ptr::write_bytes(widget.add(l.grid as usize + 16), 0, voxels * 3);
                ptr::write_bytes(widget.add(l.mask as usize), 0, 0x4000);
            }
        }

        /// The client's menu panel over all of a canvas.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped.
        pub unsafe fn panel(&self, canvas: &Canvas, color: &[f32; 3]) {
            let (width, height) = canvas.size.get();
            // SAFETY: the caller's word
            unsafe {
                self.function::<Panel>(self.layout.panel)(
                    canvas.widget.get(),
                    0,
                    0,
                    0,
                    width,
                    height,
                    color,
                )
            }
        }

        /// Writes text from `x`, `y` voxels from a canvas's top left, from depth `z` and
        /// `depth` deep. Its width.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped.
        pub unsafe fn write(
            &self,
            canvas: &Canvas,
            font: Font,
            text: &[u8],
            at: (i32, i32, i32, i32),
            color: &[f32; 3],
        ) -> i32 {
            let (mut x, y, z, depth) = at;
            let (width, height) = canvas.size.get();
            let grid = canvas.widget.get().wrapping_add(self.layout.grid as usize);
            // the glyph function draws what falls inside the grid and no more, but keep
            // whole glyphs in the canvas's rows and layers
            if y < 0 || y + font.1 > height || z < 0 || z + depth > DEPTH || depth < 1 {
                return font.width(text);
            }
            // SAFETY: the caller's word
            unsafe {
                let glyph = self.function::<Glyph>(self.layout.glyph);
                for &c in text {
                    let advance = font.advance(c);
                    if x >= 0 && x + advance <= width {
                        glyph(font.0, c.into(), grid, x, y, z, depth, color);
                    }
                    x += advance;
                }
            }
            x - at.0
        }

        /// Sets voxels of a canvas's front layer: the color's bytes, at least 2 a channel
        /// as the client keeps them, 0 to clear.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped.
        pub unsafe fn fill(
            &self,
            canvas: &Canvas,
            at: (i32, i32, i32),
            size: (i32, i32, i32),
            color: Option<[u8; 3]>,
        ) {
            let l = &self.layout;
            let shape = canvas.size.get();
            let widget = canvas.widget.get();
            let rgb = color.map_or([0; 3], |c| c.map(|channel| channel.max(2)));
            for z in at.2..at.2 + size.2 {
                for y in at.1..at.1 + size.1 {
                    for x in at.0..at.0 + size.0 {
                        let Some(index) = voxel_index(shape, x, y, z) else {
                            continue;
                        };
                        // SAFETY: the index is inside the grid the canvas is shaped to
                        unsafe {
                            let voxel = widget.add(l.grid as usize + 16 + index * 3);
                            ptr::copy_nonoverlapping(rgb.as_ptr(), voxel, 3);
                            // as the client's glyphs leave the mask
                            let word = widget.add(l.mask as usize + index / 64 * 8).cast::<u64>();
                            word.write_unaligned(word.read_unaligned() & !(1 << (index % 64)));
                        }
                    }
                }
            }
        }

        /// Hands a canvas's front layer to the client to build.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas shaped.
        unsafe fn commit(&self, canvas: &Canvas) {
            // SAFETY: the caller's word
            unsafe { self.function::<Call>(self.layout.commit)(canvas.widget.get()) }
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
                unsafe { self.function::<Call>(self.layout.hide)(canvas.widget.get()) };
            }
        }

        /// Where the cursor is on a placed canvas, in voxels from its top left, if it is
        /// on it.
        ///
        /// # Safety
        /// On the client's main thread, with the canvas placed this frame.
        unsafe fn cursor_on(&self, canvas: &Canvas) -> Option<(f32, f32)> {
            let mut at = [0f32; 2];
            // SAFETY: the caller's word
            unsafe { self.function::<Local>(self.layout.local)(canvas.widget.get(), &mut at) };
            let (width, height) = canvas.size.get();
            let (x, y) = (at[0], height as f32 - at[1]);
            ((0.0..width as f32).contains(&x) && (0.0..height as f32).contains(&y))
                .then_some((x, y))
        }

        /// The equipped items, each as its bytes.
        pub fn gear(&self) -> Vec<Vec<u8>> {
            let l = &self.layout;
            let Some(game) = self.object() else {
                return Vec::new();
            };
            // SAFETY: the client's own check of which gear to show, as its tooltips use it
            unsafe {
                let alternative = self.function::<Check>(l.equip_check)(game) != 0;
                let gear = game.add(if alternative { l.equip_alt } else { l.equip } as usize);
                (0..usize::from(l.slots))
                    .map(|slot| {
                        std::slice::from_raw_parts(
                            gear.add(slot * usize::from(l.item_size)),
                            usize::from(l.item_size),
                        )
                        .to_vec()
                    })
                    .filter(|item| item[..4] != [0; 4])
                    .collect()
            }
        }

        /// An item's stat, by the client's stat getter.
        pub fn stat(&self, item: &[u8], stat: u8) -> f32 {
            if item.len() != usize::from(self.layout.item_size) {
                return 0.0;
            }
            // SAFETY: the client's getter reads the item and nothing else
            unsafe { self.function::<Stat>(self.layout.stat)(item.as_ptr(), stat) }
        }

        /// An item's name, as the client's names table has it in English.
        pub fn name(&self, item: &[u8]) -> String {
            let l = &self.layout;
            let kind = u32::from_le_bytes(item[..4].try_into().unwrap_or_default());
            // SAFETY: the plan's table, a C string per type
            unsafe {
                let name = if kind < l.name_count {
                    (self.at(l.names) as *const *const u8)
                        .add(2 * kind as usize)
                        .read()
                } else {
                    ptr::null()
                };
                let image = self.base..self.base + 0x1000_0000;
                if name.is_null() || !image.contains(&(name as usize)) {
                    return format!("ITEM {kind}");
                }
                (0..MAX_NAME)
                    .map(|i| name.add(i).read())
                    .take_while(|&c| c != 0)
                    .map(|c| {
                        if (b' '..=b'~').contains(&c) {
                            c as char
                        } else {
                            '?'
                        }
                    })
                    .collect()
            }
        }

        /// An item's rarity, and its color as the client's tooltips draw its name, if the
        /// rarity has one.
        pub fn rarity(&self, item: &[u8]) -> (u32, Option<[f32; 3]>) {
            let at = self.layout.rarity as usize;
            let rarity = item
                .get(at..at + 4)
                .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap_or_default()));
            let color = match (rarity / 5).checked_sub(1) {
                Some(index @ 0..4) => Some(self.layout.rarity_colors[index as usize]),
                _ => None,
            };
            (rarity, color)
        }

        /// The cursor on the screen, and the screen's size.
        ///
        /// # Safety
        /// `game` must be the game object, on the client's main thread.
        unsafe fn screen_cursor(&self, game: *mut u8) -> Option<([f32; 2], f32, f32)> {
            let l = &self.layout;
            // SAFETY: the cursor object and the screen size, as the client's menus read
            // them
            unsafe {
                let mouse = game.add(l.mouse as usize).cast::<*mut u8>().read();
                let width = (self.at(l.width) as *const u32).read() as f32;
                let height = (self.at(l.height) as *const u32).read() as f32;
                if mouse.is_null() || height <= 0.0 {
                    return None;
                }
                let method = mouse
                    .cast::<*const usize>()
                    .read()
                    .add(usize::from(l.mouse_slot) / 8)
                    .read();
                let mut at = [0f32; 2];
                std::mem::transmute::<usize, Mouse>(method)(mouse, &mut at);
                Some((at, width, height))
            }
        }
    }

    /// A widget of the client's kind, made by its bar constructor, and its size now.
    pub struct Canvas {
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

        pub fn size(&self) -> (i32, i32) {
            self.size.get()
        }
    }

    /// Whether a size fits a widget's grid.
    pub fn fits(size: (i32, i32)) -> bool {
        size.0 >= 8 && size.1 >= 8 && size.0 * size.1 * DEPTH <= MAX_VOXELS
    }

    /// A popup of a menu: a canvas of its own beside it.
    pub struct Popup {
        pub id: String,
        canvas: Canvas,
        pub wanted: Cell<(i32, i32)>,
        pub dirty: Cell<bool>,
        pub shown: Cell<Option<Anchor>>,
    }

    /// A declared menu, and what its script and the engine know of it.
    pub struct Menu {
        pub owner: String,
        pub id: String,
        slot: Slot,
        canvas: Canvas,
        open: Cell<bool>,
        /// What its script asked for: to open or to close it.
        pub request: Cell<Option<bool>>,
        pub wanted: Cell<(i32, i32)>,
        pub dirty: Cell<bool>,
        /// Where the cursor is on it, in voxels from its top left.
        pub cursor: Cell<Option<(f32, f32)>>,
        pub popups: RefCell<Vec<Rc<Popup>>>,
        held: Cell<bool>,
        tilt: Cell<(f32, f32)>,
        tilted: Cell<Option<Instant>>,
    }

    impl Popup {
        pub fn canvas(&self) -> &Canvas {
            &self.canvas
        }
    }

    impl Menu {
        pub fn is_open(&self) -> bool {
            self.open.get()
        }

        pub fn canvas(&self) -> &Canvas {
            &self.canvas
        }

        /// A popup of the menu, made the first time it is asked for.
        pub fn popup(&self, id: &str) -> Rc<Popup> {
            let mut popups = self.popups.borrow_mut();
            if let Some(popup) = popups.iter().find(|p| p.id == id) {
                return popup.clone();
            }
            let popup = Rc::new(Popup {
                id: id.to_owned(),
                canvas: Canvas::new(),
                wanted: Cell::new(FIRST_SIZE),
                dirty: Cell::new(true),
                shown: Cell::new(None),
            });
            popups.push(popup.clone());
            popup
        }
    }

    /// What the engine has scripts do: they answer through `script`.
    pub enum Event<'a> {
        Open,
        Close,
        Frame(f64),
        Draw(&'a Rc<Menu>, Option<&'a Rc<Popup>>),
    }

    /// The menus, and the client they are drawn with.
    pub struct Engine {
        pub game: Rc<Game>,
        pub menus: Vec<Rc<Menu>>,
        last: Cell<Option<Instant>>,
    }

    thread_local! {
        static ENGINE: RefCell<Option<Rc<Engine>>> = const { RefCell::new(None) };
    }

    /// The engine, once the first frame has made it.
    pub fn engine() -> Option<Rc<Engine>> {
        ENGINE.with(|engine| engine.borrow().clone())
    }

    impl Engine {
        fn new(game: Rc<Game>) -> Engine {
            let menus = DECLARED
                .get()
                .into_iter()
                .flatten()
                .map(|declared| {
                    Rc::new(Menu {
                        owner: declared.owner.clone(),
                        id: declared.id.clone(),
                        slot: declared.slot,
                        canvas: Canvas::new(),
                        open: Cell::new(false),
                        request: Cell::new(None),
                        wanted: Cell::new(FIRST_SIZE),
                        dirty: Cell::new(true),
                        cursor: Cell::new(None),
                        popups: RefCell::new(Vec::new()),
                        held: Cell::new(false),
                        tilt: Cell::new((0.0, 0.0)),
                        tilted: Cell::new(None),
                    })
                })
                .collect();
            Engine {
                game,
                menus,
                last: Cell::new(None),
            }
        }

        pub fn menu(&self, owner: &str, id: &str) -> Option<Rc<Menu>> {
            self.menus
                .iter()
                .find(|w| w.owner == owner && w.id == id)
                .cloned()
        }

        /// Whether a menu's key or button, as the controls menu has it, went down since
        /// the last frame.
        fn pressed(&self, menu: &Menu) -> bool {
            let Some(binding) = crate::controls::binding(&menu.owner, &menu.id) else {
                return false;
            };
            let code = binding as u32;
            // SAFETY: SDL's key states, as many as it says, and its button mask
            let down = unsafe {
                if binding & crate::controls::MOUSE != 0 {
                    let buttons = (self.game.buttons)(std::ptr::null_mut(), std::ptr::null_mut());
                    crate::controls::button(code).is_some_and(|b| buttons & (1 << (b - 1)) != 0)
                } else {
                    let mut count = 0;
                    let states = (self.game.keyboard)(&mut count);
                    crate::controls::scancode(code).is_some_and(|key| {
                        !states.is_null()
                            && i32::from(key) < count
                            && states.add(usize::from(key)).read() != 0
                    })
                }
            };
            let held = menu.held.replace(down);
            down && !held
        }

        /// Closes a menu, with the client's sound if `sound`.
        ///
        /// # Safety
        /// On the client's main thread, with `game` the game object.
        unsafe fn close(&self, game: *mut u8, menu: &Menu, why: &str, sound: bool) {
            if !menu.open.replace(false) {
                return;
            }
            log!("{}: menu {} closed ({why})", menu.owner, menu.id);
            if sound {
                // SAFETY: the caller's word
                unsafe { self.game.sound(game, CLOSE_SOUND) };
            }
            crate::script::menu_event(menu, Event::Close);
        }

        /// Opens a menu where its slot lets it, closing what it takes the place of.
        ///
        /// # Safety
        /// As for `close`.
        unsafe fn open(&self, game: *mut u8, menu: &Rc<Menu>) {
            let g = &self.game;
            let l = &g.layout;
            // SAFETY: the caller's word
            unsafe {
                if g.flag(game, l.system) {
                    return;
                }
                for other in &self.menus {
                    let shares = other.slot == menu.slot
                        || other.slot == Slot::Center
                        || menu.slot == Slot::Center;
                    if !Rc::ptr_eq(other, menu) && shares {
                        self.close(game, other, "replaced", false);
                    }
                }
                // the client's own menus give way, as they do to each other
                if menu.slot == Slot::Left {
                    for &[flag, at, close] in &l.slot {
                        if g.flag(game, flag) {
                            g.function::<Call>(close)(game.add(at as usize));
                        }
                    }
                }
                let close_top = g.function::<Check>(l.close_top);
                for _ in 0..CLOSE_AT_MOST {
                    if !g.in_the_way(game, menu.slot) || close_top(game) == 0 {
                        break;
                    }
                }
                if g.in_the_way(game, menu.slot) {
                    log!(
                        "{}: menu {} not opened: another menu stays open",
                        menu.owner,
                        menu.id
                    );
                    return;
                }
                menu.open.set(true);
                menu.dirty.set(true);
                log!("{}: menu {} opened", menu.owner, menu.id);
                g.function::<Call>(l.notice)(game);
                g.sound(game, OPEN_SOUND);
            }
            crate::script::menu_event(menu, Event::Open);
        }

        /// Where a menu goes, its rotation and its scale.
        ///
        /// # Safety
        /// `game` must be the game object, on the client's main thread.
        unsafe fn placing(&self, game: *mut u8, menu: &Menu) -> ([f32; 3], [f32; 3], f32) {
            let g = &self.game;
            let l = &g.layout;
            let (width, _) = menu.canvas.size();
            // SAFETY: the plan's offsets of the client's settings
            unsafe {
                let ui = game
                    .add(l.ui_scale as usize)
                    .cast::<f32>()
                    .read_unaligned()
                    .min(SLOT_MAX_UI);
                let setting = game.add(l.tilt as usize).cast::<f32>().read_unaligned();
                let turn = if menu.slot == Slot::Left {
                    SLOT_TURN
                } else {
                    0.0
                };
                let (mut across, mut down) = menu.tilt.get();
                if let Some((at, screen_width, screen_height)) = g.screen_cursor(game) {
                    let now = Instant::now();
                    let elapsed = menu
                        .tilted
                        .replace(Some(now))
                        .map_or(0.0, |then| now.duration_since(then).as_secs_f64() * 1000.0);
                    across = follow(
                        across,
                        (at[0] - screen_width / 2.0) * TILT_ACROSS / screen_height + turn,
                        elapsed,
                    );
                    down = follow(
                        down,
                        (at[1] - screen_height / 2.0) * TILT_DOWN / screen_height,
                        elapsed,
                    );
                    menu.tilt.set((across, down));
                }
                let rotation = [setting * down, 0.0, setting * across];
                let left = match menu.slot {
                    Slot::Center => 0.0,
                    Slot::Left => {
                        // its outer edge as far from the screen's as the inventory's is on
                        // the right. The inventory's open flag is its first own field
                        let inventory = l
                            .keep
                            .first()
                            .and_then(|&flag| flag.checked_sub(l.base))
                            .map(|widget| {
                                game.add((widget + l.grid) as usize + 4)
                                    .cast::<i32>()
                                    .read_unaligned()
                            })
                            .filter(|w| INVENTORY_WIDTHS.contains(w))
                            .unwrap_or(width);
                        let wide = (inventory as f32 - SLOT_WIDE).max(0.0) * SLOT_WIDE_SHARE;
                        let half = |w: i32| w as f32 * SLOT_WIDE_SHARE;
                        let edge =
                            wide + SLOT_BASE + (1.0 - setting) * SLOT_BASE_TILT + half(inventory);
                        (edge - half(width)) * ui
                    }
                };
                ([-left, 0.0, MENU_DEPTH], rotation, ui * l.scale)
            }
        }

        /// Shapes a canvas to the size wanted, and has the script draw it if it has to.
        ///
        /// # Safety
        /// On the client's main thread.
        unsafe fn redraw(&self, menu: &Rc<Menu>, popup: Option<&Rc<Popup>>) -> bool {
            let (canvas, wanted, dirty) = match popup {
                Some(p) => (&p.canvas, &p.wanted, &p.dirty),
                None => (&menu.canvas, &menu.wanted, &menu.dirty),
            };
            let size = wanted.get();
            if !fits(size) {
                return false;
            }
            // SAFETY: the caller's word, and the size fits
            unsafe {
                let new = canvas.size.get() != size;
                if !self.game.shape(canvas, size) {
                    return false;
                }
                if new || dirty.replace(false) {
                    self.game.clear(canvas);
                    crate::script::menu_event(menu, Event::Draw(menu, popup));
                    self.game.commit(canvas);
                }
            }
            true
        }

        /// Takes a menu and its popups off the screen.
        ///
        /// # Safety
        /// On the client's main thread.
        unsafe fn hide(&self, menu: &Menu) {
            menu.tilted.set(None);
            menu.cursor.set(None);
            // SAFETY: the caller's word
            unsafe {
                for popup in menu.popups.borrow().iter() {
                    self.game.hide(&popup.canvas);
                }
                self.game.hide(&menu.canvas);
            }
        }

        /// Once a frame.
        ///
        /// # Safety
        /// From the client's frame, on its main thread.
        unsafe fn frame(&self) {
            let Some(game) = self.game.object() else {
                return;
            };
            let now = Instant::now();
            let seconds = self
                .last
                .replace(Some(now))
                .map_or(0.0, |then| now.duration_since(then).as_secs_f64());
            // SAFETY: the plan's layout of this client, on its main thread
            unsafe {
                let walked = WALKED.swap(false, Ordering::Relaxed);
                for menu in &self.menus {
                    if walked {
                        self.close(game, menu, "walked", true);
                    }
                    let pressed = self.pressed(menu);
                    let request = menu.request.take();
                    if pressed && menu.open.get() || request == Some(false) {
                        self.close(game, menu, if pressed { "key" } else { "script" }, true);
                    } else if (pressed && HOTKEYS.load(Ordering::Relaxed) && seen())
                        || request == Some(true)
                    {
                        self.open(game, menu);
                    }
                    if menu.open.get() && self.game.in_the_way(game, menu.slot) {
                        self.close(game, menu, "another menu opened", false);
                    }
                }
                ANY_OPEN.store(self.menus.iter().any(|w| w.open.get()), Ordering::Relaxed);

                crate::script::frame(seconds);
                for menu in &self.menus {
                    if menu.open.get() {
                        crate::script::menu_event(menu, Event::Frame(seconds));
                    }
                }

                for menu in &self.menus {
                    // closed, or hidden in menus, while typing and on the start screen
                    if !menu.open.get() || !seen() || !self.redraw(menu, None) {
                        self.hide(menu);
                        continue;
                    }
                    let (at, rotation, scale) = self.placing(game, menu);
                    self.game.place(&menu.canvas, at, &rotation, scale);
                    menu.cursor.set(self.game.cursor_on(&menu.canvas));
                    let popups = menu.popups.borrow().clone();
                    for popup in &popups {
                        let Some(anchor) = popup.shown.get() else {
                            self.game.hide(&popup.canvas);
                            continue;
                        };
                        if !self.redraw(menu, Some(popup)) {
                            self.game.hide(&popup.canvas);
                            continue;
                        }
                        let (across, down) =
                            beside(menu.canvas.size(), popup.canvas.size(), anchor);
                        let near = POPUP_DEPTH / MENU_DEPTH;
                        let at = [
                            (at[0] + across * scale) * near,
                            (at[1] + down * scale) * near,
                            POPUP_DEPTH,
                        ];
                        self.game.place(&popup.canvas, at, &rotation, scale * near);
                    }
                }
            }
        }
    }

    /// The `ui-frame` hook's state: the client, until the first frame makes the engine.
    pub struct Hook {
        layout: Layout,
        base: usize,
    }

    impl Hook {
        pub fn new(layout: Layout, base: usize) -> Hook {
            Hook { layout, base }
        }
    }

    /// For `context`: once a frame. Makes the engine the first time, then the scripts.
    ///
    /// # Safety
    /// `state` must be a `Hook`, on the client's main thread.
    pub unsafe fn frame(_: &mut Context, state: *const c_void) {
        let engine = ENGINE.with(|engine| {
            let mut engine = engine.borrow_mut();
            if engine.is_none() {
                // SAFETY: the caller's word
                let hook = unsafe { &*state.cast::<Hook>() };
                // SAFETY: names NUL-terminated
                let sdl = |name: &std::ffi::CStr| unsafe {
                    let sdl = GetModuleHandleA(c"SDL2.dll".as_ptr().cast());
                    (!sdl.is_null())
                        .then(|| GetProcAddress(sdl, name.as_ptr().cast()))
                        .flatten()
                };
                let found = sdl(c"SDL_GetKeyboardState").zip(sdl(c"SDL_GetMouseState"));
                let Some((keyboard, buttons)) = found else {
                    log!("ui: SDL is not loaded, no menus");
                    return None;
                };
                type Found = unsafe extern "system" fn() -> isize;
                // SAFETY: SDL's functions have these types
                let (keyboard, buttons) = unsafe {
                    (
                        std::mem::transmute::<Found, Keyboard>(keyboard),
                        std::mem::transmute::<Found, Buttons>(buttons),
                    )
                };
                let game = Rc::new(Game {
                    layout: hook.layout.clone(),
                    base: hook.base,
                    keyboard,
                    buttons,
                });
                *engine = Some(Rc::new(Engine::new(game)));
                return engine.clone().map(|made| (made, true));
            }
            engine.clone().map(|made| (made, false))
        });
        let engine = engine.map(|(engine, made)| {
            // with the engine in place, as scripts ask it for their menus
            if made {
                crate::script::start();
            }
            engine
        });
        if let Some(engine) = engine {
            // SAFETY: the caller's word
            unsafe { engine.frame() };
        }
    }

    /// Where the client handles the keys of its menus: notes whether it would take
    /// one now.
    ///
    /// # Safety
    /// From there, with `state` the gate's offset as a `u8`.
    pub unsafe fn hotkey(context: &mut Context, state: *const c_void) {
        // SAFETY: the caller's word, and the hotkey handler keeps the byte on its stack
        unsafe {
            let gate = usize::from(*state.cast::<u8>());
            let works = ((context.rsp() as usize + gate) as *const u8).read() == 1;
            HOTKEYS.store(works, Ordering::Relaxed);
            SEEN_AT.store(now(), Ordering::Relaxed);
        }
    }

    /// Where the client closes its menus as the player starts to move, unless they were
    /// moving when the menus opened.
    ///
    /// # Safety
    /// From there.
    pub unsafe fn walked(_: &mut Context, _: *const c_void) {
        WALKED.store(true, Ordering::Relaxed);
    }

    /// Right where the client has read whether the system menu is open, the last of its
    /// tests of whether any menu is: a mod's menu counts as one.
    ///
    /// # Safety
    /// From there, with `state` an `Answer`.
    pub unsafe fn opened(context: &mut Context, state: *const c_void) {
        const ZERO_FLAG: u64 = 1 << 6;
        if !ANY_OPEN.load(Ordering::Relaxed) {
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
        assert_eq!(number(f64::NAN), "0");
    }

    #[test]
    fn cuts_long_text_with_dots() {
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
    fn finds_voxels_from_the_top_left() {
        // 4 wide, 3 tall: the top left voxel is in the grid's top row
        assert_eq!(voxel_index((4, 3), 0, 0, 0), Some(8));
        assert_eq!(voxel_index((4, 3), 3, 2, 0), Some(3));
        assert_eq!(voxel_index((4, 3), 0, 2, 1), Some(12));
        assert_eq!(voxel_index((4, 3), 4, 0, 0), None);
        assert_eq!(voxel_index((4, 3), 0, 0, DEPTH), None);
    }

    #[test]
    fn puts_popups_beside_their_menu() {
        let anchor = |side, gap, along| Anchor { side, gap, along };
        assert_eq!(
            beside((100, 80), (60, 40), anchor(Side::Right, 10, 0)),
            (90.0, 0.0)
        );
        assert_eq!(
            beside((100, 80), (60, 40), anchor(Side::Left, 10, 5)),
            (-90.0, 5.0)
        );
        assert_eq!(
            beside((100, 80), (60, 40), anchor(Side::Below, 4, 0)),
            (0.0, 64.0)
        );
        assert_eq!(
            beside((100, 80), (60, 40), anchor(Side::Above, 4, 0)),
            (0.0, -64.0)
        );
    }

    #[test]
    fn reads_the_small_hooks() {
        assert_eq!(gate(&[0x3E]), Some(0x3E));
        assert_eq!(gate(&[]), None);
        assert_eq!(Answer::read(&[1]), Some(Answer::EqualsOne));
        assert_eq!(Slot::named("left"), Some(Slot::Left));
        assert_eq!(Slot::named("right"), None);
    }
}
