//! Win32 popup theme policy, matching GPUI Kit's native-menu platform mechanism.
use std::sync::OnceLock;
use windows_sys::Win32::{
    Foundation::HWND,
    System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
};

pub(super) fn theme(hwnd: HWND, dark: bool) {
    // HMENU has no documented dark palette API. Resolve the Windows theme
    // entry points, as GPUI Kit does for its native popup menus. Keep the
    // system module loaded while the process can display a tray menu.
    static MODULE: OnceLock<isize> = OnceLock::new();
    let module = *MODULE.get_or_init(|| unsafe {
        LoadLibraryExW(
            windows_sys::w!("uxtheme.dll"),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        ) as isize
    });
    if module == 0 {
        log::warn!("无法加载原生菜单主题，托盘菜单使用系统外观");
        return;
    }
    unsafe {
        let module = module as windows_sys::Win32::Foundation::HMODULE;
        if let Some(function) = GetProcAddress(module, 133usize as *const u8) {
            let allow: unsafe extern "system" fn(HWND, i32) -> i32 = std::mem::transmute(function);
            allow(hwnd, i32::from(dark));
        }
        if let Some(function) = GetProcAddress(module, 135usize as *const u8) {
            let prefer: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(function);
            // PreferredAppMode::ForceDark / ForceLight.
            prefer(if dark { 2 } else { 3 });
        }
        if let Some(function) = GetProcAddress(module, 136usize as *const u8) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(function);
            flush();
        }
    }
}
