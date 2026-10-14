//! Getting in front of the client DLL when the game loads it after this one.
//!
//! The OS tells of every DLL it maps, but before the DLL's imports are bound, which is too
//! early to hook them. So the notice is only used to swap the client's entry point in the
//! OS loader's record of it. The loader calls that once the imports are bound and before
//! any of the client's own code, which is the moment the plan is for.

use std::ffi::{CString, c_void};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::{ptr, slice};

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

use crate::log::log;

const DLL_PROCESS_ATTACH: u32 = 1;
/// LDR_DLL_NOTIFICATION_REASON_LOADED
const LOADED: u32 = 1;

type Entry = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> i32;
type Notice = unsafe extern "system" fn(u32, *const Mapped, *mut c_void);
type Register = unsafe extern "system" fn(u32, Notice, *mut c_void, *mut *mut c_void) -> i32;

#[repr(C)]
struct Text {
    /// In bytes.
    length: u16,
    capacity: u16,
    units: *const u16,
}

/// LDR_DLL_LOADED_NOTIFICATION_DATA
#[repr(C)]
struct Mapped {
    flags: u32,
    path: *const Text,
    name: *const Text,
    base: *mut c_void,
    size: u32,
}

/// The DLLs waited for, and what to do once one of them is ready.
static WANTED: OnceLock<(Vec<String>, fn())> = OnceLock::new();
/// Where the OS loader keeps the client's entry point, while this module's is in it.
static SLOT: AtomicPtr<*mut c_void> = AtomicPtr::new(ptr::null_mut());
static ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static DONE: AtomicBool = AtomicBool::new(false);

/// Whether `dll` is loaded in this process.
pub fn loaded(dll: &str) -> bool {
    CString::new(dll).is_ok_and(|name| {
        // SAFETY: the name is NUL-terminated
        !unsafe { GetModuleHandleA(name.as_ptr().cast()) }.is_null()
    })
}

/// Where the OS loader keeps the entry point of the module mapped at `base`.
unsafe fn entry_slot(base: *mut c_void) -> Option<*mut *mut c_void> {
    // SAFETY: gs:[60h] is the PEB, whose Ldr at 18h lists the modules in load order at
    // 10h. Each LDR_DATA_TABLE_ENTRY starts with its links in that list, has DllBase at
    // 30h and EntryPoint at 38h. The OS loader's lock is held by the caller
    unsafe {
        let peb: *const u8;
        core::arch::asm!("mov {}, gs:[0x60]", out(reg) peb, options(nostack, readonly, pure));
        let head = (*peb.add(0x18).cast::<*const u8>()).add(0x10);
        let mut link = *head.cast::<*const u8>();
        while !link.is_null() && link != head {
            if *link.add(0x30).cast::<*mut c_void>() == base {
                return Some(link.add(0x38) as *mut *mut c_void);
            }
            link = *link.cast::<*const u8>();
        }
    }
    None
}

/// Stands in for the client's entry point the first time the OS loader calls it.
unsafe extern "system" fn entry(module: *mut c_void, reason: u32, reserved: *mut c_void) -> i32 {
    let original = ORIGINAL.load(Ordering::Relaxed);
    let slot = SLOT.load(Ordering::Relaxed);
    if !slot.is_null() {
        // SAFETY: the record outlives the module, and the loader's lock is held
        unsafe { *slot = original };
    }
    if reason == DLL_PROCESS_ATTACH
        && !DONE.swap(true, Ordering::Relaxed)
        && let Some((_, ready)) = WANTED.get()
    {
        // a panic must not unwind into the OS loader
        let _ = std::panic::catch_unwind(ready);
    }
    if original.is_null() {
        return 1;
    }
    // SAFETY: it is the entry point the OS loader was about to call with these arguments
    unsafe { std::mem::transmute::<*mut c_void, Entry>(original)(module, reason, reserved) }
}

unsafe extern "system" fn mapped(reason: u32, data: *const Mapped, _context: *mut c_void) {
    if reason != LOADED || data.is_null() || !SLOT.load(Ordering::Relaxed).is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        // SAFETY: the OS loader passes a filled record that lives for the call
        let (name, base) = unsafe {
            let name = &*(*data).name;
            let units = slice::from_raw_parts(name.units, usize::from(name.length) / 2);
            (String::from_utf16_lossy(units), (*data).base)
        };
        let Some((wanted, ready)) = WANTED.get() else {
            return;
        };
        if !wanted.iter().any(|dll| dll.eq_ignore_ascii_case(&name)) {
            return;
        }
        // SAFETY: the OS loader calls this with its lock held
        match unsafe { entry_slot(base) } {
            Some(slot) => {
                // SAFETY: as above, and the slot is in a record the loader keeps
                unsafe {
                    ORIGINAL.store(*slot, Ordering::Relaxed);
                    SLOT.store(slot, Ordering::Relaxed);
                    *slot = entry as *mut c_void;
                }
            }
            None => {
                // still patched, but file replacements are lost when the imports are bound
                log!("{name} is not in the list of loaded modules yet; applying early");
                if !DONE.swap(true, Ordering::Relaxed) {
                    ready();
                }
            }
        }
    });
}

/// Calls `ready` when one of the `dlls` is loaded: imports bound, none of its code run yet.
/// False if the OS has no way to be told.
pub fn wait(dlls: Vec<String>, ready: fn()) -> bool {
    if WANTED.set((dlls, ready)).is_err() {
        return false;
    }
    // SAFETY: both names are NUL-terminated, and ntdll is in every process
    let register = unsafe {
        let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast());
        GetProcAddress(ntdll, c"LdrRegisterDllNotification".as_ptr().cast())
    };
    let Some(register) = register else {
        return false;
    };
    let mut cookie = ptr::null_mut();
    // SAFETY: the function has this signature since Windows Vista, and `mapped` stays
    // loaded as long as this DLL does
    let status = unsafe {
        let register: Register = std::mem::transmute(register);
        register(0, mapped, ptr::null_mut(), &mut cookie)
    };
    status >= 0
}
