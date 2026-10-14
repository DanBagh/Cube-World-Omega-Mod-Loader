//! The exports of the real dbghelp.dll, passed straight through.
//!
//! Each is a jump through a table filled from the system's own dbghelp.dll, so arguments
//! and return values go by untouched whatever the export's signature is. The game asks for
//! one of them. All are here because anything else in the process that names dbghelp.dll
//! is given this DLL too.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows_sys::Win32::Foundation::{ERROR_PROC_NOT_FOUND, SetLastError};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW};
use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

/// The system DLL this one is installed under the name of.
const REAL: &str = "dbghelp.dll";

extern "system" fn missing() -> isize {
    // SAFETY: only sets this thread's error code
    unsafe { SetLastError(ERROR_PROC_NOT_FOUND) };
    0
}

macro_rules! forward {
    ($($name:ident)*) => {
        const NAMES: &[&str] = &[$(concat!(stringify!($name), "\0")),*];

        static TABLE: [AtomicPtr<c_void>; NAMES.len()] =
            [const { AtomicPtr::new(ptr::null_mut()) }; NAMES.len()];

        /// An export's place in the table.
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        enum Slot {
            $($name),*
        }

        $(
            #[unsafe(naked)]
            #[unsafe(no_mangle)]
            #[allow(non_snake_case)]
            pub unsafe extern "system" fn $name() {
                core::arch::naked_asm!(
                    "jmp qword ptr [rip + {table} + {offset}]",
                    table = sym TABLE,
                    offset = const Slot::$name as usize * 8,
                )
            }
        )*
    };
}

// every export dbghelp.dll has a name for, as of Windows 11 25H2. One an older Windows
// lacks fails with ERROR_PROC_NOT_FOUND when called
forward! {
    SymAllocDiaString SymFreeDiaString SymGetDiaSession SymGetDiaSource SymGetLineFromAddrEx
    SymGetLineFromNameEx SymGetLineNextEx SymGetLinePrevEx SymGetOmapBlockBase
    SymRegisterGetSourcePathPartCallback SymRegisterSourceFileUrlListCallback SymSetDiaSession
    SymSetServiceManager _EFN_DumpImage DbgHelpCreateUserDump DbgHelpCreateUserDumpW EnumDirTree
    EnumDirTreeW EnumerateLoadedModules EnumerateLoadedModules64 EnumerateLoadedModulesEx
    EnumerateLoadedModulesExW EnumerateLoadedModulesW64 ExtensionApiVersion FindDebugInfoFile
    FindDebugInfoFileEx FindDebugInfoFileExW FindExecutableImage FindExecutableImageEx
    FindExecutableImageExW FindFileInPath FindFileInSearchPath GetSymLoadError
    GetTimestampForLoadedLibrary ImageDirectoryEntryToData ImageDirectoryEntryToDataEx
    ImageNtHeader ImageRvaToSection ImageRvaToVa ImagehlpApiVersion ImagehlpApiVersionEx
    MakeSureDirectoryPathExists MiniDumpReadDumpStream MiniDumpWriteDump
    RangeMapAddPeImageSections RangeMapCreate RangeMapFree RangeMapRead RangeMapRemove
    RangeMapWrite RemoveInvalidModuleList ReportSymbolLoadSummary SearchTreeForFile
    SearchTreeForFileW SetCheckUserInterruptShared SetSymLoadError StackWalk StackWalk2
    StackWalk64 StackWalkEx SymAddSourceStream SymAddSourceStreamA SymAddSourceStreamW
    SymAddSymbol SymAddSymbolW SymAddrIncludeInlineTrace SymCleanup SymCompareInlineTrace
    SymDeleteSymbol SymDeleteSymbolW SymEnumLines SymEnumLinesW SymEnumProcesses
    SymEnumSourceFileTokens SymEnumSourceFiles SymEnumSourceFilesW SymEnumSourceLines
    SymEnumSourceLinesW SymEnumSym SymEnumSymbols SymEnumSymbolsEx SymEnumSymbolsExW
    SymEnumSymbolsForAddr SymEnumSymbolsForAddrW SymEnumSymbolsW SymEnumTypes SymEnumTypesByName
    SymEnumTypesByNameW SymEnumTypesW SymEnumerateModules SymEnumerateModules64
    SymEnumerateModulesW64 SymEnumerateSymbols SymEnumerateSymbols64 SymEnumerateSymbolsW
    SymEnumerateSymbolsW64 SymFindDebugInfoFile SymFindDebugInfoFileW SymFindExecutableImage
    SymFindExecutableImageW SymFindFileInPath SymFindFileInPathW SymFromAddr SymFromAddrW
    SymFromIndex SymFromIndexW SymFromInlineContext SymFromInlineContextW SymFromName
    SymFromNameW SymFromToken SymFromTokenW SymFunctionTableAccess SymFunctionTableAccess64
    SymFunctionTableAccess64AccessRoutines SymGetExtendedOption SymGetFileLineOffsets64
    SymGetHomeDirectory SymGetHomeDirectoryW SymGetLineFromAddr SymGetLineFromAddr64
    SymGetLineFromAddrW64 SymGetLineFromInlineContext SymGetLineFromInlineContextW
    SymGetLineFromName SymGetLineFromName64 SymGetLineFromNameW64 SymGetLineNext
    SymGetLineNext64 SymGetLineNextW64 SymGetLinePrev SymGetLinePrev64 SymGetLinePrevW64
    SymGetModuleBase SymGetModuleBase64 SymGetModuleInfo SymGetModuleInfo64 SymGetModuleInfoW
    SymGetModuleInfoW64 SymGetOmaps SymGetOptions SymGetParentWindow SymGetScope SymGetScopeW
    SymGetSearchPath SymGetSearchPathW SymGetSourceFile SymGetSourceFileChecksum
    SymGetSourceFileChecksumW SymGetSourceFileFromToken SymGetSourceFileFromTokenByTokenName
    SymGetSourceFileFromTokenByTokenNameW SymGetSourceFileFromTokenW SymGetSourceFileToken
    SymGetSourceFileTokenByTokenName SymGetSourceFileTokenByTokenNameW SymGetSourceFileTokenW
    SymGetSourceFileW SymGetSourceVarFromToken SymGetSourceVarFromTokenW SymGetSymFromAddr
    SymGetSymFromAddr64 SymGetSymFromName SymGetSymFromName64 SymGetSymNext SymGetSymNext64
    SymGetSymPrev SymGetSymPrev64 SymGetSymbolFile SymGetSymbolFileW SymGetTypeFromName
    SymGetTypeFromNameW SymGetTypeInfo SymGetTypeInfoEx SymGetUnwindInfo SymInitialize
    SymInitializeW SymLoadModule SymLoadModule64 SymLoadModuleEx SymLoadModuleExW
    SymMatchFileName SymMatchFileNameW SymMatchString SymMatchStringA SymMatchStringW SymNext
    SymNextW SymPrev SymPrevW SymQueryInlineTrace SymRefreshModuleList SymRegisterCallback
    SymRegisterCallback64 SymRegisterCallbackW64 SymRegisterFunctionEntryCallback
    SymRegisterFunctionEntryCallback64 SymSearch SymSearchW SymSetContext SymSetExtendedOption
    SymSetHomeDirectory SymSetHomeDirectoryW SymSetOptions SymSetParentWindow
    SymSetScopeFromAddr SymSetScopeFromIndex SymSetScopeFromInlineContext SymSetSearchPath
    SymSetSearchPathW SymSrvDeltaName SymSrvDeltaNameW SymSrvGetFileIndexInfo
    SymSrvGetFileIndexInfoW SymSrvGetFileIndexString SymSrvGetFileIndexStringW
    SymSrvGetFileIndexes SymSrvGetFileIndexesW SymSrvGetSupplement SymSrvGetSupplementW
    SymSrvIsStore SymSrvIsStoreW SymSrvStoreFile SymSrvStoreFileW SymSrvStoreSupplement
    SymSrvStoreSupplementW SymUnDName SymUnDName64 SymUnloadModule SymUnloadModule64
    UnDecorateSymbolName UnDecorateSymbolNameW WinDbgExtensionDllInit block chksym dbghelp dh
    fptr homedir inlinedbg itoldyouso lmi lminfo lmsourcesinfo omap optdbgdump optdbgdumpaddr
    srcfiles stack_force_ebp stackdbg sym vc7fpo
}

/// Fills the table. The real DLL is loaded by its full path: by name, the search would
/// find this one again.
pub fn load() {
    let mut path = [0u16; 300];
    // SAFETY: the buffer is longer than the length passed
    let length = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), 260) } as usize;
    let mut real = ptr::null_mut();
    if (1..260).contains(&length) {
        path[length] = 0x5C;
        for (at, unit) in REAL.encode_utf16().enumerate() {
            path[length + 1 + at] = unit;
        }
        // SAFETY: the path is NUL-terminated, the rest of the buffer being zero
        real = unsafe { LoadLibraryExW(path.as_ptr(), ptr::null_mut(), 0) };
    }
    for (slot, name) in TABLE.iter().zip(NAMES) {
        let found = if real.is_null() {
            None
        } else {
            // SAFETY: `real` is a loaded module and the name is NUL-terminated
            unsafe { GetProcAddress(real, name.as_ptr()) }
        };
        let target = found.map_or(missing as *mut c_void, |function| function as *mut c_void);
        slot.store(target, Ordering::Relaxed);
    }
}
