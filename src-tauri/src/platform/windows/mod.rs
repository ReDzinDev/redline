pub mod clipboard;
pub mod focus;
pub mod input;
pub mod placement;
pub mod uia;

/// Hardens the process DLL search order so implicit/dynamic loads of Windows system DLLs
/// resolve strictly from `%SystemRoot%\System32` and never from CWD or application directories.
pub fn harden_dll_search_order() {
    use windows::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories, SetDllDirectoryW,
    };
    use windows::core::w;

    // SAFETY: Process-wide kernel32 DLL search-order hardening with static literal path.
    unsafe {
        let _ = SetDllDirectoryW(w!(""));
        let _ = SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32);
    }
}
