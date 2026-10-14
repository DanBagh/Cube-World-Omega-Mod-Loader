//! Applying the plan's patch groups to the loaded client.
//!
//! A group is verified as a whole against the bytes the plan expects and applied only if
//! all of it matches, so a plan made for another build does nothing instead of doing harm.

use std::ffi::{CString, c_void};
use std::ptr;

use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_FREE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ,
    PAGE_EXECUTE_READWRITE, PAGE_READWRITE, VirtualAlloc, VirtualProtect, VirtualQuery,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::log::log;
use crate::plan::{MAX_HOOK, Module, Op, Plan};

/// How far a rel32 reaches, less a margin.
const REACH: u64 = 0x7FF0_0000;
const GRANULARITY: u64 = 0x1_0000;
const PAGE: usize = 0x1000;

/// What applying a plan left behind.
#[derive(Default)]
pub struct Done {
    /// The client DLL that matched the plan.
    pub client: Option<*mut u8>,
    /// The data area: where it is and how long.
    pub data: Option<(usize, u32)>,
}

struct Client {
    base: *mut u8,
    size_of_image: u32,
    /// The plan's blob, placed within rel32 reach of the whole module.
    blob: *mut u8,
    blob_size: u32,
}

/// Replaces bytes of code or read-only data.
///
/// # Safety
/// `at` must point at `bytes.len()` bytes of committed memory in this process.
unsafe fn write(at: *mut u8, bytes: &[u8]) -> bool {
    let mut old = 0;
    unsafe {
        if VirtualProtect(at.cast(), bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len());
        VirtualProtect(at.cast(), bytes.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), at.cast(), bytes.len());
    }
    true
}

/// Free address space within rel32 reach of every byte of the module, committed read/write.
fn alloc_near(base: *mut u8, image_size: u32, size: usize) -> Option<*mut u8> {
    let round = |address: u64| (address + GRANULARITY - 1) & !(GRANULARITY - 1);
    let low = base as u64 + u64::from(image_size);
    let high = (base as u64 + REACH).checked_sub(size as u64)?;
    let mut address = round(low);
    while address < high {
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: only asks about the address, and `info` is as long as the length passed
        let got = unsafe { VirtualQuery(address as *const c_void, &mut info, size_of_val(&info)) };
        if got == 0 {
            break;
        }
        let end = info.BaseAddress as u64 + info.RegionSize as u64;
        if info.State == MEM_FREE && address + size as u64 <= end {
            // SAFETY: asks for fresh pages at an address that is free
            let pages = unsafe {
                VirtualAlloc(
                    address as *const c_void,
                    size,
                    MEM_RESERVE | MEM_COMMIT,
                    PAGE_READWRITE,
                )
            };
            if !pages.is_null() {
                return Some(pages.cast());
            }
        }
        address = round(end.max(address + 1));
    }
    None
}

impl Client {
    /// The loaded DLL the module of a plan is for, if it is that build.
    fn find(module: &Module) -> Option<Client> {
        let name = CString::new(module.dll.as_str()).ok()?;
        // SAFETY: the name is NUL-terminated
        let base: *mut u8 = unsafe { GetModuleHandleA(name.as_ptr().cast()) }.cast();
        if base.is_null() {
            return None;
        }
        // SAFETY: `base` is a module the OS loader mapped, so its headers are readable
        let (timestamp, size_of_image) = unsafe {
            let nt = base.add(ptr::read_unaligned(base.add(0x3C).cast::<u32>()) as usize);
            (
                ptr::read_unaligned(nt.add(8).cast::<u32>()),
                ptr::read_unaligned(nt.add(24 + 56).cast::<u32>()),
            )
        };
        if (timestamp, size_of_image) != (module.timestamp, module.size_of_image) {
            log!(
                "module {} is loaded but is a different build than the plan (timestamp {timestamp:08X}, size {size_of_image:08X})",
                module.dll
            );
            return None;
        }
        log!("module {} at {base:p} matches the plan", module.dll);
        Some(Client {
            base,
            size_of_image,
            blob: ptr::null_mut(),
            blob_size: 0,
        })
    }

    /// Places the blob, and the data area on pages of its own after it.
    fn place(&mut self, blob: &[u8], data_size: u32) -> Option<Option<(usize, u32)>> {
        if blob.is_empty() {
            return Some(None);
        }
        let blob_pages = blob.len().next_multiple_of(PAGE);
        let pages = alloc_near(
            self.base,
            self.size_of_image,
            blob_pages + data_size as usize,
        )?;
        let mut old = 0;
        // SAFETY: the pages were just committed for this, and are at least as long as the blob
        unsafe {
            ptr::copy_nonoverlapping(blob.as_ptr(), pages, blob.len());
            // new strings and position-independent trampolines
            VirtualProtect(pages.cast(), blob.len(), PAGE_EXECUTE_READ, &mut old);
        }
        self.blob = pages;
        self.blob_size = blob.len() as u32;
        Some((data_size > 0).then(|| (pages as usize + blob_pages, data_size)))
    }

    fn holds(&self, rva: u32, len: usize) -> bool {
        u64::from(rva) + len as u64 <= u64::from(self.size_of_image)
    }

    /// The bytes of the module at `rva`.
    ///
    /// # Safety
    /// `holds(rva, len)` must be true.
    unsafe fn bytes(&self, rva: u32, len: usize) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.base.add(rva as usize), len) }
    }

    fn verify(&self, op: &Op) -> bool {
        // SAFETY: each read is of a range `holds` has just allowed
        unsafe {
            match op {
                Op::Rel32 {
                    site,
                    old_target,
                    blob_offset,
                } => {
                    self.holds(*site, 4) && *blob_offset < self.blob_size && {
                        let now = u32::from_le_bytes(self.bytes(*site, 4).try_into().unwrap());
                        site.wrapping_add(4).wrapping_add(now) == *old_target
                    }
                }
                Op::Ptr64 {
                    site,
                    old_target,
                    blob_offset,
                } => {
                    self.holds(*site, 8) && *blob_offset < self.blob_size && {
                        let now = u64::from_le_bytes(self.bytes(*site, 8).try_into().unwrap());
                        now == self.base as u64 + u64::from(*old_target)
                    }
                }
                Op::Bytes { rva, expect, .. } => {
                    self.holds(*rva, expect.len()) && self.bytes(*rva, expect.len()) == expect
                }
                Op::Hook {
                    rva,
                    expect,
                    blob_offset,
                } => {
                    (5..=MAX_HOOK).contains(&expect.len())
                        && *blob_offset < self.blob_size
                        && self.holds(*rva, expect.len())
                        && self.bytes(*rva, expect.len()) == expect
                }
                Op::BlobRel32 {
                    blob_offset,
                    target,
                } => {
                    u64::from(*blob_offset) + 4 <= u64::from(self.blob_size)
                        && *target < self.size_of_image
                }
            }
        }
    }

    fn apply(&self, op: &Op) -> bool {
        let module = |rva: u32| self.base as i64 + i64::from(rva);
        let blob = |offset: u32| self.blob as i64 + i64::from(offset);
        let rel32 = |to: i64, from: i64| i32::try_from(to - from).ok().map(i32::to_le_bytes);
        // SAFETY: `verify` has allowed every range written here
        unsafe {
            match op {
                Op::Rel32 {
                    site, blob_offset, ..
                } => rel32(blob(*blob_offset), module(*site) + 4)
                    .is_some_and(|bytes| write(module(*site) as *mut u8, &bytes)),
                Op::Ptr64 {
                    site, blob_offset, ..
                } => write(
                    module(*site) as *mut u8,
                    &(blob(*blob_offset) as u64).to_le_bytes(),
                ),
                Op::Bytes { rva, new, .. } => write(module(*rva) as *mut u8, new),
                Op::Hook {
                    rva,
                    expect,
                    blob_offset,
                } => rel32(blob(*blob_offset), module(*rva) + 5).is_some_and(|bytes| {
                    // jmp rel32 into the blob, then NOPs over the rest of what it replaces
                    let mut code = [0x90u8; MAX_HOOK];
                    code[0] = 0xE9;
                    code[1..5].copy_from_slice(&bytes);
                    write(module(*rva) as *mut u8, &code[..expect.len()])
                }),
                Op::BlobRel32 {
                    blob_offset,
                    target,
                } => rel32(module(*target), blob(*blob_offset) + 4)
                    .is_some_and(|bytes| write(blob(*blob_offset) as *mut u8, &bytes)),
            }
        }
    }
}

/// Applies the plan to whichever of its client DLLs are loaded.
pub fn run(plan: &Plan) -> Done {
    let mut done = Done::default();
    let (mut matched, mut applied, mut skipped) = (0, 0, 0);
    for module in &plan.modules {
        let Some(mut client) = Client::find(module) else {
            continue;
        };
        matched += 1;
        done.client = Some(client.base);
        let Some(blob) = &module.blob else {
            log!("plan: bad blob, module skipped");
            continue;
        };
        match client.place(blob, module.data_size) {
            Some(data) => done.data = data.or(done.data),
            None => {
                log!(
                    "could not allocate {} bytes near the module, module skipped",
                    blob.len()
                );
                continue;
            }
        }
        for group in &module.groups {
            let fits = group.ops.as_ref().is_some_and(|ops| {
                !ops.is_empty()
                    && ops.iter().all(|op| client.verify(op))
                    && ops.iter().all(|op| client.apply(op))
            });
            if fits {
                applied += 1;
            } else {
                skipped += 1;
                log!(
                    "skipped {} (game bytes differ from what the plan expects)",
                    group.name
                );
            }
        }
    }
    if matched == 0 {
        log!("no client module matching the plan is loaded; nothing was patched");
    } else {
        log!("patch groups applied: {applied}, skipped: {skipped}");
    }
    done
}
