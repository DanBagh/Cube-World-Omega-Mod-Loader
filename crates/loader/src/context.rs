//! Context hooks: a handler of this DLL, written in Rust, runs at a place in the client
//! with all of the client's registers at hand, and can change them.
//!
//! The client jumps to a small piece of code made for the hook. It runs the instructions
//! the jump replaced, before or after the handler as the plan says, and goes into `entry`
//! with the hook's record. `entry` saves every register, the flags and xmm0 to xmm15 as a
//! `Context`, calls the handler through `dispatch`, puts them all back and returns to the
//! hook's code, which jumps back into the client.
//!
//! Handlers are named here, and only these exist. A plan cannot ask for anything else.

use std::ffi::c_void;

use crate::plan::MAX_HOOK;
use crate::{controls, ui};

/// The room the code of one hook takes: the replaced instructions and 40 bytes.
pub const SPACE: usize = (MAX_HOOK + 40).next_multiple_of(16);

/// The client's registers while a handler runs, as `entry` saves them. The client's stack
/// keeps no alignment at a hook, so neither does this.
#[repr(C)]
pub struct Context {
    pub xmm: [[u64; 2]; 16],
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rflags: u64,
    pub rax: u64,
    /// Where the hook's code goes on. Not a register.
    resume: u64,
}

impl Context {
    /// A general register by its number, 0 for rax to 15 for r15.
    pub fn register(&self, number: u8) -> u64 {
        match number {
            0 => self.rax,
            1 => self.rcx,
            2 => self.rdx,
            3 => self.rbx,
            4 => self.rsp(),
            5 => self.rbp,
            6 => self.rsi,
            7 => self.rdi,
            8 => self.r8,
            9 => self.r9,
            10 => self.r10,
            11 => self.r11,
            12 => self.r12,
            13 => self.r13,
            14 => self.r14,
            _ => self.r15,
        }
    }

    /// The client's rsp at the hook: right above what `entry` and the hook's code pushed.
    pub fn rsp(&self) -> u64 {
        (self as *const Context as u64) + size_of::<Context>() as u64
    }
}

/// One installed hook, as its code hands it to `entry`.
#[repr(C)]
pub struct Hook {
    /// Where the hook's code goes on after the handler. First, for `entry`.
    pub resume: usize,
    run: unsafe fn(&mut Context, *const c_void),
    state: *const c_void,
}

/// What one pickup-sound hook knows: where the event record is, and the sound per item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pickup {
    /// The number of the register holding the event record.
    pub record: u8,
    /// Where the record keeps the item type, and right after it the subtype.
    pub item: u8,
    /// Item type, subtype (`u32::MAX` for any) and sound id, in the order to try them.
    pub sounds: Vec<(u32, u32, u16)>,
}

impl Pickup {
    /// Reads the handler's data: the register, the item offset, then entries of type,
    /// subtype and sound id, little-endian.
    pub fn read(data: &[u8]) -> Option<Pickup> {
        let (&[record, item], rest) = data.split_first_chunk::<2>()?;
        let (entries, left) = rest.as_chunks::<10>();
        (record < 16 && left.is_empty()).then(|| Pickup {
            record,
            item,
            sounds: entries
                .iter()
                .map(|entry| {
                    let word =
                        |at: usize| u32::from_le_bytes(entry[at..at + 4].try_into().unwrap());
                    (word(0), word(4), u16::from_le_bytes([entry[8], entry[9]]))
                })
                .collect(),
        })
    }

    /// The sound for an item, if the table has one.
    pub fn sound(&self, kind: u32, subtype: u32) -> Option<u16> {
        self.sounds
            .iter()
            .find(|&&(t, s, _)| t == kind && (s == u32::MAX || s == subtype))
            .map(|&(_, _, id)| id)
    }
}

/// Right after the client stored its one pickup sound in the queue entry at rcx + rax:
/// puts the item's own sound there instead.
///
/// # Safety
/// Only as the `pickup-sound` hook, with `state` its `Pickup`.
unsafe fn pickup(context: &mut Context, state: *const c_void) {
    // SAFETY: `state` is the hook's leaked table
    let table = unsafe { &*state.cast::<Pickup>() };
    let item = (context.register(table.record) as *const u8).wrapping_add(table.item.into());
    // SAFETY: at this place the register holds the event record, with the item in it
    let (kind, subtype) = unsafe {
        (
            item.cast::<u32>().read_unaligned(),
            item.add(4).cast::<u32>().read_unaligned(),
        )
    };
    if let Some(id) = table.sound(kind, subtype) {
        let entry = context.rcx.wrapping_add(context.rax) as *mut u16;
        // SAFETY: the client has just stored the stock sound id there
        unsafe { entry.write_unaligned(id) };
    }
}

/// Whether this DLL has a handler of that name, and it can read the data.
pub fn valid(handler: &str, data: &[u8]) -> bool {
    match handler {
        "pickup-sound" => Pickup::read(data).is_some(),
        "ui-frame" => ui::Layout::read(data).is_some(),
        "ui-hotkey" => ui::gate(data).is_some(),
        "ui-walk" => data.is_empty(),
        "ui-open" => ui::Answer::read(data).is_some(),
        "ui-controls-label" => controls::Label::read(data).is_some(),
        "ui-controls-key" => controls::actions(data).is_some(),
        "ui-controls-store" => controls::Store::read(data).is_some(),
        "ui-controls-open" | "ui-controls-defaults" | "ui-controls-save" => data.is_empty(),
        "ui-controls-drag" => controls::Drag::read(data).is_some(),
        _ => false,
    }
}

/// The record of a hook for `handler` in the client at `base`, if its data reads. It lives
/// as long as the process, and `resume` is to be filled in.
pub fn hook(handler: &str, data: &[u8], base: usize) -> Option<*mut Hook> {
    fn leak<T>(state: T) -> *const c_void {
        Box::into_raw(Box::new(state)).cast()
    }
    #[cfg(not(windows))]
    let _ = base;
    let (run, state): (unsafe fn(&mut Context, *const c_void), *const c_void) = match handler {
        "pickup-sound" => (pickup, leak(Pickup::read(data)?)),
        #[cfg(windows)]
        "ui-frame" => (
            ui::frame,
            leak(ui::Hook::new(ui::Layout::read(data)?, base)),
        ),
        #[cfg(windows)]
        "ui-hotkey" => (ui::hotkey, leak(ui::gate(data)?)),
        #[cfg(windows)]
        "ui-walk" => (ui::walked, std::ptr::null()),
        #[cfg(windows)]
        "ui-open" => (ui::opened, leak(ui::Answer::read(data)?)),
        "ui-controls-label" => (controls::label, leak(controls::Label::read(data)?)),
        "ui-controls-key" => (controls::key, leak(controls::actions(data)?)),
        "ui-controls-store" => (controls::store, leak(controls::Store::read(data)?)),
        "ui-controls-open" => (controls::open, std::ptr::null()),
        "ui-controls-defaults" => (controls::defaults, std::ptr::null()),
        "ui-controls-save" => (controls::saved, std::ptr::null()),
        "ui-controls-drag" => (controls::drag, leak(controls::Drag::read(data)?)),
        _ => return None,
    };
    Some(Box::into_raw(Box::new(Hook {
        resume: 0,
        run,
        state,
    })))
}

/// The code a hook's jump goes to. It runs the `replaced` instructions, before the
/// handler with `after` so the handler sees what they did, enters `entry` with `hook` in
/// rax and two stack slots, one of them for the way on, then jumps to `back`. Returns the
/// code and where in it the way on starts.
pub fn code(replaced: &[u8], after: bool, hook: u64, entry: u64, back: u64) -> (Vec<u8>, usize) {
    let mut code = Vec::new();
    if after {
        code.extend(replaced);
    }
    // push rax / push rax / mov rax,<hook> / jmp [rip+0] <entry>
    code.extend([0x50, 0x50, 0x48, 0xB8]);
    code.extend(hook.to_le_bytes());
    code.extend([0xFF, 0x25, 0, 0, 0, 0]);
    code.extend(entry.to_le_bytes());
    let resume = code.len();
    if !after {
        code.extend(replaced);
    }
    // jmp [rip+0] <back>
    code.extend([0xFF, 0x25, 0, 0, 0, 0]);
    code.extend(back.to_le_bytes());
    (code, resume)
}

/// Runs a hook's handler. `entry` calls it with the saved registers.
#[cfg(windows)]
unsafe extern "win64" fn dispatch(context: *mut Context, hook: *const Hook) {
    // SAFETY: `entry` hands over the context it built and the record from the hook's code
    unsafe { ((*hook).run)(&mut *context, (*hook).state) }
}

/// Saves the client's registers as a `Context`, calls `dispatch`, restores them and
/// jumps to the hook's way on. rax holds the hook's record, the slot above the saved rax
/// is for the way on. No `ret` to there, which a shadow stack would refuse.
///
/// # Safety
/// Only for a hook's code to jump to, never to call.
#[cfg(windows)]
#[unsafe(naked)]
pub unsafe extern "win64" fn entry() {
    core::arch::naked_asm!(
        "pushfq",
        "push rcx",
        "push rdx",
        "push rbx",
        "push rbp",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // the way on, into the slot above the saved rax
        "mov rcx, [rax]",
        "mov [rsp + 16*8], rcx",
        "sub rsp, 0x100",
        "movups [rsp + 0x00], xmm0",
        "movups [rsp + 0x10], xmm1",
        "movups [rsp + 0x20], xmm2",
        "movups [rsp + 0x30], xmm3",
        "movups [rsp + 0x40], xmm4",
        "movups [rsp + 0x50], xmm5",
        "movups [rsp + 0x60], xmm6",
        "movups [rsp + 0x70], xmm7",
        "movups [rsp + 0x80], xmm8",
        "movups [rsp + 0x90], xmm9",
        "movups [rsp + 0xA0], xmm10",
        "movups [rsp + 0xB0], xmm11",
        "movups [rsp + 0xC0], xmm12",
        "movups [rsp + 0xD0], xmm13",
        "movups [rsp + 0xE0], xmm14",
        "movups [rsp + 0xF0], xmm15",
        "mov rcx, rsp",
        "mov rdx, rax",
        // the client's stack may be at any alignment here
        "mov rbx, rsp",
        "and rsp, -16",
        "sub rsp, 0x20",
        "call {dispatch}",
        "mov rsp, rbx",
        "movups xmm0, [rsp + 0x00]",
        "movups xmm1, [rsp + 0x10]",
        "movups xmm2, [rsp + 0x20]",
        "movups xmm3, [rsp + 0x30]",
        "movups xmm4, [rsp + 0x40]",
        "movups xmm5, [rsp + 0x50]",
        "movups xmm6, [rsp + 0x60]",
        "movups xmm7, [rsp + 0x70]",
        "movups xmm8, [rsp + 0x80]",
        "movups xmm9, [rsp + 0x90]",
        "movups xmm10, [rsp + 0xA0]",
        "movups xmm11, [rsp + 0xB0]",
        "movups xmm12, [rsp + 0xC0]",
        "movups xmm13, [rsp + 0xD0]",
        "movups xmm14, [rsp + 0xE0]",
        "movups xmm15, [rsp + 0xF0]",
        "add rsp, 0x100",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rbp",
        "pop rbx",
        "pop rdx",
        "pop rcx",
        "popfq",
        "pop rax",
        "lea rsp, [rsp + 8]",
        "jmp qword ptr [rsp - 8]",
        dispatch = sym dispatch,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn reads_the_table_and_picks_the_first_entry_that_fits() {
        let mut data = vec![15, 0x10];
        for (kind, subtype, id) in [
            (95u32, 40u32, 0x8001u16),
            (43, u32::MAX, 0x8000),
            (95, u32::MAX, 0x6B),
        ] {
            data.extend(kind.to_le_bytes());
            data.extend(subtype.to_le_bytes());
            data.extend(id.to_le_bytes());
        }
        let table = Pickup::read(&data).unwrap();
        assert_eq!(
            (table.record, table.item, table.sounds.len()),
            (15, 0x10, 3)
        );
        assert_eq!(table.sound(43, 7), Some(0x8000));
        assert_eq!(table.sound(95, 40), Some(0x8001));
        assert_eq!(table.sound(95, 41), Some(0x6B));
        assert_eq!(table.sound(1, 0), None);
        // a register that does not exist, and an entry cut short
        assert_eq!(Pickup::read(&[16, 0x10]), None);
        assert_eq!(Pickup::read(&data[..data.len() - 1]), None);
        assert!(valid("pickup-sound", &data) && !valid("pickup-sound", &[16, 0x10]));
        assert!(!valid("anything else", &data));
        assert!(hook("anything else", &data, 0).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn a_handler_sees_and_changes_the_registers_where_asked() {
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc,
        };

        unsafe fn add_one(context: &mut Context, seen: *const c_void) {
            // SAFETY: the test's own u64
            unsafe { *seen.cast::<u64>().cast_mut() = context.rax };
            context.rax += 1;
        }
        // the handler before `mov eax,7` sees 55h and its change is lost, after it sees 7
        for (after, saw, returned) in [(true, 7, 8), (false, 0x55, 7)] {
            let mut seen = 0u64;
            // SAFETY: fresh pages, filled in before they run
            unsafe {
                let page: *mut u8 = VirtualAlloc(
                    ptr::null(),
                    0x1000,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_EXECUTE_READWRITE,
                )
                .cast();
                assert!(!page.is_null());
                let hook = Box::into_raw(Box::new(Hook {
                    resume: 0,
                    run: add_one,
                    state: (&raw mut seen).cast(),
                }));
                let thunk = page.add(0x40);
                let (code, resume) = code(
                    &[0xB8, 7, 0, 0, 0],
                    after,
                    hook as u64,
                    entry as *const () as u64,
                    page as u64 + 10,
                );
                (*hook).resume = thunk as usize + resume;
                ptr::copy_nonoverlapping(code.as_ptr(), thunk, code.len());
                // mov eax,55h / jmp thunk, where mov eax,7 was / ret
                let jump = (0x40 - 10i32).to_le_bytes();
                let client = [
                    0xB8, 0x55, 0, 0, 0, 0xE9, jump[0], jump[1], jump[2], jump[3], 0xC3,
                ];
                ptr::copy_nonoverlapping(client.as_ptr(), page, client.len());
                let run: extern "win64" fn() -> u64 = std::mem::transmute(page);
                assert_eq!((run(), seen), (returned, saw));
            }
        }
    }

    #[test]
    fn the_context_is_what_entry_pushes() {
        // 16 xmm registers, 14 general ones, the flags, rax and the way on
        assert_eq!(size_of::<Context>(), 0x100 + 17 * 8);
    }

    #[test]
    fn the_hooks_code_runs_the_replaced_instructions_where_asked() {
        let replaced = [0x66, 0xC7, 0x04, 0x01, 0x3D, 0x00];
        let (hook, entry, back) = (0x1111_2222_3333_4444, 0x5555_6666_7777_8888, 0x9999);
        let (after, resume) = code(&replaced, true, hook, entry, back);
        assert_eq!(after[..6], replaced);
        assert_eq!(after[6..10], [0x50, 0x50, 0x48, 0xB8]);
        assert_eq!(after[10..18], hook.to_le_bytes());
        assert_eq!(after[18..24], [0xFF, 0x25, 0, 0, 0, 0]);
        assert_eq!(after[24..32], entry.to_le_bytes());
        // the way on is straight back
        assert_eq!(resume, 32);
        assert_eq!(after[32..38], [0xFF, 0x25, 0, 0, 0, 0]);
        assert_eq!(after[38..], back.to_le_bytes());

        let (before, resume) = code(&replaced, false, hook, entry, back);
        assert_eq!(resume, 26);
        assert_eq!(before[26..32], replaced);
        assert_eq!(before[32..38], [0xFF, 0x25, 0, 0, 0, 0]);
    }
}
