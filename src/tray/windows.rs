//! Theme native popup windows as well as their owner; Win32 retains menu tracking and scrolling.
use std::{cell::Cell, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::HWND,
    System::{
        LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        Threading::GetCurrentThreadId,
    },
    UI::{
        Controls::SetWindowTheme,
        WindowsAndMessaging::{
            CWPRETSTRUCT, CallNextHookEx, GetClassNameW, HHOOK, SetWindowsHookExW,
            UnhookWindowsHookEx, WH_CALLWNDPROCRET, WM_CREATE, WM_SHOWWINDOW,
        },
    },
};

thread_local! { static DARK: Cell<bool> = const { Cell::new(false) }; }

pub(super) struct Appearance {
    hook: HHOOK,
}
impl Appearance {
    pub(super) fn new() -> Self {
        // Thread-local hook observes only this application's UI thread, including nested submenus.
        let hook = unsafe {
            SetWindowsHookExW(
                WH_CALLWNDPROCRET,
                Some(popup_created),
                std::ptr::null_mut(),
                GetCurrentThreadId(),
            )
        };
        if hook.is_null() {
            log::warn!(
                "无法同步原生弹出菜单主题：{}",
                std::io::Error::last_os_error()
            );
        }
        Self { hook }
    }
    pub(super) fn theme(&self, hwnd: HWND, dark: bool) {
        DARK.set(dark);
        if let Some(api) = api() {
            unsafe {
                if let Some(prefer) = api.prefer {
                    prefer(if dark { 2 } else { 3 });
                }
                if let Some(allow) = api.allow {
                    allow(hwnd, dark);
                }
                if let Some(flush) = api.flush {
                    flush();
                }
            }
        }
    }
}
impl Drop for Appearance {
    fn drop(&mut self) {
        if !self.hook.is_null() {
            unsafe {
                UnhookWindowsHookEx(self.hook);
            }
        }
    }
}

unsafe extern "system" fn popup_created(code: i32, wparam: usize, lparam: isize) -> isize {
    if code >= 0 {
        let message = unsafe { &*(lparam as *const CWPRETSTRUCT) };
        // WM_SHOWWINDOW also covers popup HWNDs reused by the native menu tracking loop.
        if message.message == WM_CREATE || (message.message == WM_SHOWWINDOW && message.wParam != 0)
        {
            let mut class = [0u16; 16];
            let length =
                unsafe { GetClassNameW(message.hwnd, class.as_mut_ptr(), class.len() as i32) };
            if length == 6 && class[..6] == (*b"#32768").map(u16::from) {
                let dark = DARK.get();
                if let Some(allow) = api().and_then(|api| api.allow) {
                    unsafe {
                        allow(message.hwnd, dark);
                    }
                }
                // The popup's own MENU theme includes the overflow arrows and their background.
                // Setting only the hidden tray owner leaves this independent HWND's theme cached.
                let sub_app = if dark {
                    windows_sys::w!("DarkMode")
                } else {
                    std::ptr::null()
                };
                let result =
                    unsafe { SetWindowTheme(message.hwnd, sub_app, windows_sys::w!("Menu")) };
                if result < 0 {
                    log::warn!("原生弹出菜单主题更新失败：0x{:08X}", result as u32);
                }
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

struct Api {
    allow: Option<unsafe extern "system" fn(HWND, bool) -> bool>,
    prefer: Option<unsafe extern "system" fn(i32) -> i32>,
    flush: Option<unsafe extern "system" fn()>,
}
fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        // HMENU's dark palette policy is not a documented API. Retain the loaded system module
        // for the function pointers' lifetime, matching the native GPUI Kit menu mechanism.
        let module = LoadLibraryExW(
            windows_sys::w!("uxtheme.dll"),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        if module.is_null() {
            log::warn!("无法加载原生菜单主题：{}", std::io::Error::last_os_error());
            return None;
        }
        Some(Api {
            allow: GetProcAddress(module, 133usize as *const u8).map(|function| {
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(HWND, bool) -> bool,
                >(function)
            }),
            prefer: GetProcAddress(module, 135usize as *const u8).map(|function| {
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(i32) -> i32,
                >(function)
            }),
            flush: GetProcAddress(module, 136usize as *const u8).map(|function| {
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(),
                >(function)
            }),
        })
    })
    .as_ref()
}
