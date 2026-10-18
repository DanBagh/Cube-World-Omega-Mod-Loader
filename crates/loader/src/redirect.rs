//! Serving replacement files: the client's file-open imports are pointed at functions
//! here, which swap the path for the ones the plan redirects and pass everything else on.
//!
//! The game's own files stay as they are on disk.

use std::ffi::{CStr, c_void};
use std::ptr;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar, WideCharToMultiByte};
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows_sys::Win32::System::Memory::{PAGE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::log::log;

/// Longest path looked at, in UTF-16 units with the NUL.
const MAX_PATH: usize = 1024;
/// Redirects written to the log. The game opens some files over and over.
const MAX_LOGGED: u32 = 64;
const CRT: &CStr = c"api-ms-win-crt-stdio-l1-1-0.dll";

struct Redirects {
    /// Folder of the client DLL, normalized, with its trailing slash. Opens by absolute
    /// path under it match too.
    root: Vec<u16>,
    /// Normalized game-relative path, and the NUL-terminated file to open instead.
    entries: Vec<(Vec<u16>, Vec<u16>)>,
}

static REDIRECTS: OnceLock<Redirects> = OnceLock::new();
static LOGGED: AtomicU32 = AtomicU32::new(0);

type Handle = *mut c_void;
type CreateFileW =
    unsafe extern "system" fn(*const u16, u32, u32, *const c_void, u32, u32, Handle) -> Handle;
type CreateFileA =
    unsafe extern "system" fn(*const u8, u32, u32, *const c_void, u32, u32, Handle) -> Handle;
type WfopenS = unsafe extern "C" fn(*mut *mut c_void, *const u16, *const u16) -> i32;
type FopenS = unsafe extern "C" fn(*mut *mut c_void, *const u8, *const u8) -> i32;
type FiopenW = unsafe extern "C" fn(*const u16, i32, i32) -> *mut c_void;
type FiopenA = unsafe extern "C" fn(*const u8, i32, i32) -> *mut c_void;

/// What an import pointed at before it was hooked.
struct Real<F>(AtomicPtr<c_void>, std::marker::PhantomData<F>);

impl<F: Copy> Real<F> {
    const fn new() -> Self {
        Real(AtomicPtr::new(ptr::null_mut()), std::marker::PhantomData)
    }

    fn get(&self) -> Option<F> {
        let function = self.0.load(Ordering::Relaxed);
        // SAFETY: F is a function pointer type, as wide as the pointer stored for it
        (!function.is_null()).then(|| unsafe { std::mem::transmute_copy(&function) })
    }
}

static CREATE_FILE_W: Real<CreateFileW> = Real::new();
static CREATE_FILE_A: Real<CreateFileA> = Real::new();
static WFOPEN_S: Real<WfopenS> = Real::new();
static FOPEN_S: Real<FopenS> = Real::new();
static FIOPEN_W: Real<FiopenW> = Real::new();
static FIOPEN_A: Real<FiopenA> = Real::new();

/// Lowercases ASCII, turns backslashes, and drops a leading `\\?\` and `./`. Returns how
/// much of `out` was filled, or None for a path too long to be one of the game's.
fn normalize(path: impl Iterator<Item = u16>, out: &mut [u16; MAX_PATH]) -> Option<usize> {
    let mut length = 0;
    for unit in path {
        *out.get_mut(length)? = match unit {
            0x5C => 0x2F,
            0x41..=0x5A => unit + 32,
            _ => unit,
        };
        length += 1;
    }
    let mut start = if out[..length].starts_with(&[0x2F, 0x2F, 0x3F, 0x2F]) {
        4
    } else {
        0
    };
    while out[start..length].starts_with(&[0x2E, 0x2F]) {
        start += 2;
    }
    out.copy_within(start..length, 0);
    Some(length - start)
}

/// The file to open in place of `path`, NUL-terminated.
///
/// # Safety
/// `path` must be null or a NUL-terminated UTF-16 string.
unsafe fn lookup_w(path: *const u16) -> Option<*const u16> {
    let redirects = REDIRECTS.get()?;
    if path.is_null() {
        return None;
    }
    let mut buffer = [0u16; MAX_PATH];
    let units = (0..).map_while(|at| Some(unsafe { *path.add(at) }).filter(|&unit| unit != 0));
    let length = normalize(units, &mut buffer)?;
    let mut wanted = &buffer[..length];
    if wanted.len() > redirects.root.len() && wanted.starts_with(&redirects.root) {
        wanted = &wanted[redirects.root.len()..];
    }
    let (key, target) = redirects.entries.iter().find(|(key, _)| key == wanted)?;
    if LOGGED.fetch_add(1, Ordering::Relaxed) < MAX_LOGGED {
        let text = |units: &[u16]| String::from_utf16_lossy(units);
        log!(
            "redirect {} -> {}",
            text(key),
            text(&target[..target.len() - 1])
        );
    }
    Some(target.as_ptr())
}

/// # Safety
/// `path` must be null or a NUL-terminated string in the ANSI code page.
unsafe fn lookup_a(path: *const u8) -> Option<*const u16> {
    REDIRECTS.get()?;
    if path.is_null() {
        return None;
    }
    let mut wide = [0u16; MAX_PATH];
    // SAFETY: the source is NUL-terminated and the buffer as long as the length passed
    let converted =
        unsafe { MultiByteToWideChar(CP_ACP, 0, path, -1, wide.as_mut_ptr(), MAX_PATH as i32) };
    if converted == 0 {
        return None;
    }
    unsafe { lookup_w(wide.as_ptr()) }
}

/// `target` in the ANSI code page, for a narrow call that has no wide sibling to pass it to.
///
/// # Safety
/// `target` must be a NUL-terminated UTF-16 string.
unsafe fn to_ansi(target: *const u16, out: &mut [u8; MAX_PATH]) -> bool {
    let (null, size) = (ptr::null(), MAX_PATH as i32);
    unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            target,
            -1,
            out.as_mut_ptr(),
            size,
            null,
            ptr::null_mut(),
        ) > 0
    }
}

// A hook only runs once its import was swapped, which stores the original first. The
// early returns for a missing original are what the real functions give on failure.

const INVALID_HANDLE: Handle = usize::MAX as Handle;
const EINVAL: i32 = 22;

unsafe extern "system" fn create_file_w(
    name: *const u16,
    access: u32,
    share: u32,
    security: *const c_void,
    disposition: u32,
    flags: u32,
    template: Handle,
) -> Handle {
    let Some(real) = CREATE_FILE_W.get() else {
        return INVALID_HANDLE;
    };
    let name = unsafe { lookup_w(name) }.unwrap_or(name);
    unsafe { real(name, access, share, security, disposition, flags, template) }
}

unsafe extern "system" fn create_file_a(
    name: *const u8,
    access: u32,
    share: u32,
    security: *const c_void,
    disposition: u32,
    flags: u32,
    template: Handle,
) -> Handle {
    let Some(real) = CREATE_FILE_A.get() else {
        return INVALID_HANDLE;
    };
    let Some(target) = (unsafe { lookup_a(name) }) else {
        return unsafe { real(name, access, share, security, disposition, flags, template) };
    };
    // through the wide call, so a mod path outside the ANSI code page survives
    if let Some(wide) = CREATE_FILE_W.get() {
        return unsafe {
            wide(
                target,
                access,
                share,
                security,
                disposition,
                flags,
                template,
            )
        };
    }
    let mut ansi = [0u8; MAX_PATH];
    let name = if unsafe { to_ansi(target, &mut ansi) } {
        ansi.as_ptr()
    } else {
        name
    };
    unsafe { real(name, access, share, security, disposition, flags, template) }
}

unsafe extern "C" fn wfopen_s(file: *mut *mut c_void, name: *const u16, mode: *const u16) -> i32 {
    let Some(real) = WFOPEN_S.get() else {
        return EINVAL;
    };
    let name = unsafe { lookup_w(name) }.unwrap_or(name);
    unsafe { real(file, name, mode) }
}

unsafe extern "C" fn fopen_s(file: *mut *mut c_void, name: *const u8, mode: *const u8) -> i32 {
    let Some(real) = FOPEN_S.get() else {
        return EINVAL;
    };
    let Some(target) = (unsafe { lookup_a(name) }) else {
        return unsafe { real(file, name, mode) };
    };
    if let Some(wide) = WFOPEN_S.get().filter(|_| !mode.is_null()) {
        let mut wide_mode = [0u16; 16];
        for (at, unit) in wide_mode.iter_mut().take(15).enumerate() {
            let byte = unsafe { *mode.add(at) };
            if byte == 0 {
                break;
            }
            *unit = u16::from(byte);
        }
        return unsafe { wide(file, target, wide_mode.as_ptr()) };
    }
    let mut ansi = [0u8; MAX_PATH];
    let name = if unsafe { to_ansi(target, &mut ansi) } {
        ansi.as_ptr()
    } else {
        name
    };
    unsafe { real(file, name, mode) }
}

unsafe extern "C" fn fiopen_w(name: *const u16, mode: i32, protection: i32) -> *mut c_void {
    let Some(real) = FIOPEN_W.get() else {
        return ptr::null_mut();
    };
    let name = unsafe { lookup_w(name) }.unwrap_or(name);
    unsafe { real(name, mode, protection) }
}

unsafe extern "C" fn fiopen_a(name: *const u8, mode: i32, protection: i32) -> *mut c_void {
    let Some(real) = FIOPEN_A.get() else {
        return ptr::null_mut();
    };
    let Some(target) = (unsafe { lookup_a(name) }) else {
        return unsafe { real(name, mode, protection) };
    };
    if let Some(wide) = FIOPEN_W.get() {
        return unsafe { wide(target, mode, protection) };
    }
    let mut ansi = [0u8; MAX_PATH];
    let name = if unsafe { to_ansi(target, &mut ansi) } {
        ansi.as_ptr()
    } else {
        name
    };
    unsafe { real(name, mode, protection) }
}

/// Points the import of `function` from `dll` at `hook`, keeping what it pointed at.
///
/// # Safety
/// `base` must be a module the OS loader mapped, and `hook` a function with the import's
/// signature.
unsafe fn hook_import<F>(
    base: *mut u8,
    dll: &CStr,
    function: &CStr,
    hook: *const c_void,
    real: &Real<F>,
) -> bool {
    let u32_at = |at: usize| unsafe { ptr::read_unaligned(base.add(at).cast::<u32>()) } as usize;
    let name_at = |rva: usize| unsafe { CStr::from_ptr(base.add(rva).cast()) };
    let optional = u32_at(0x3C) + 24;
    // the import directory is the second entry of the data directories
    let mut descriptor = u32_at(optional + 112 + 8);
    if descriptor == 0 {
        return false;
    }
    // { names, time stamp, forwarder chain, DLL name, slots }, ended by a zero name
    while u32_at(descriptor + 12) != 0 {
        let (names, slots) = (u32_at(descriptor), u32_at(descriptor + 16));
        let ours = name_at(u32_at(descriptor + 12))
            .to_bytes()
            .eq_ignore_ascii_case(dll.to_bytes());
        descriptor += 20;
        if !ours || names == 0 {
            continue;
        }
        for index in 0.. {
            let entry = unsafe { ptr::read_unaligned(base.add(names + 8 * index).cast::<u64>()) };
            if entry == 0 {
                break;
            }
            // by ordinal, or by a hint and a name
            if entry >> 63 != 0 || name_at(entry as usize + 2) != function {
                continue;
            }
            let slot = unsafe { base.add(slots + 8 * index) };
            let mut old = 0;
            unsafe {
                real.0
                    .store(ptr::read_unaligned(slot.cast()), Ordering::Relaxed);
                if VirtualProtect(slot.cast(), 8, PAGE_READWRITE, &mut old) == 0 {
                    return false;
                }
                ptr::write_unaligned(slot.cast(), hook);
                VirtualProtect(slot.cast(), 8, old, &mut old);
                FlushInstructionCache(GetCurrentProcess(), slot.cast(), 8);
            }
            return true;
        }
    }
    false
}

/// Hooks the file-open imports of the client at `client` to serve `redirects`.
pub fn install(client: *mut u8, redirects: &[(String, String)]) {
    let mut path = [0u16; MAX_PATH];
    // SAFETY: the buffer is as long as the length passed
    let length =
        unsafe { GetModuleFileNameW(client.cast(), path.as_mut_ptr(), MAX_PATH as u32) } as usize;
    let folder = path[..length]
        .iter()
        .rposition(|&unit| unit == 0x5C)
        .map_or(0, |slash| slash + 1);
    let mut buffer = [0u16; MAX_PATH];
    let root = normalize(path[..folder].iter().copied(), &mut buffer).unwrap_or(0);
    let wide = |text: &str, end: &[u16]| text.encode_utf16().chain(end.iter().copied()).collect();
    let installed = REDIRECTS.set(Redirects {
        root: buffer[..root].to_vec(),
        entries: redirects
            .iter()
            .map(|(key, target)| (wide(key, &[]), wide(target, &[0])))
            .collect(),
    });
    if installed.is_err() {
        return;
    }
    let kernel = c"KERNEL32.dll";
    let cpp = c"MSVCP140.dll";
    // SAFETY: `client` is the loaded client DLL, and each hook has its import's signature.
    // The wide ones go first, so the narrow hooks can pass redirected opens to them
    let hooked = unsafe {
        [
            hook_import(
                client,
                kernel,
                c"CreateFileW",
                create_file_w as _,
                &CREATE_FILE_W,
            ),
            hook_import(
                client,
                kernel,
                c"CreateFileA",
                create_file_a as _,
                &CREATE_FILE_A,
            ),
            hook_import(client, CRT, c"_wfopen_s", wfopen_s as _, &WFOPEN_S),
            hook_import(client, CRT, c"fopen_s", fopen_s as _, &FOPEN_S),
            hook_import(
                client,
                cpp,
                c"?_Fiopen@std@@YAPEAU_iobuf@@PEB_WHH@Z",
                fiopen_w as _,
                &FIOPEN_W,
            ),
            hook_import(
                client,
                cpp,
                c"?_Fiopen@std@@YAPEAU_iobuf@@PEBDHH@Z",
                fiopen_a as _,
                &FIOPEN_A,
            ),
        ]
    };
    log!(
        "file redirection: {} override(s), {}/6 import hooks installed",
        redirects.len(),
        hooked.iter().filter(|&&done| done).count()
    );
}
