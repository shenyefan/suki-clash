//! Desktop integration owns OS launch behavior and instance lifetime.
pub(crate) mod autostart;
pub(crate) mod instance;
mod launch;
#[cfg(target_os = "linux")]
use launch::desktop_command;
pub(crate) use launch::executable;
#[cfg(not(target_os = "macos"))]
pub(crate) mod scheme;

pub(crate) async fn startup_script(paths: &crate::storage::AppPaths) -> Result<(), String> {
    let repository =
        crate::storage::Repository::open(paths.clone()).map_err(crate::logging::error_message)?;
    let values = crate::preferences::values(&repository, crate::preferences::SettingsKind::General)
        .map_err(crate::logging::error_message)?;
    let Some(path) = values
        .get("StartupScript")
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(());
    };
    let path = std::path::Path::new(path);
    if !path.is_absolute() || !path.is_file() {
        return Err("启动脚本需为存在的绝对文件路径".into());
    }
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or_default();
    let mut command = match extension {
        "sh" => {
            let mut c = tokio::process::Command::new("bash");
            c.arg(path);
            c
        }
        "ps1" => {
            let mut c =
                tokio::process::Command::new(if cfg!(windows) { "powershell" } else { "pwsh" });
            c.args(["-NoProfile", "-File"]).arg(path);
            c
        }
        "bat" if cfg!(windows) => {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C"]).arg(path);
            c
        }
        _ => return Err("启动脚本仅支持 .sh、.ps1 和 Windows .bat".into()),
    };
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command
        .current_dir(path.parent().ok_or("启动脚本目录无效")?)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("无法执行启动脚本：{error}"))?;
    match tokio::time::timeout(std::time::Duration::from_secs(60), child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!("启动脚本未完成（{status}）")),
        Ok(Err(error)) => Err(format!("启动脚本执行失败：{error}")),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err("启动脚本执行超时，已终止".into())
        }
    }
}

use gpui_kit::*;
struct Instance {
    service: instance::Service,
    main: WindowHandle<base::Root>,
    app: Entity<crate::app::SukiApp>,
    _task: Task<()>,
    _quit: Subscription,
}
impl Global for Instance {}
pub(crate) fn shutdown(cx: &mut App) -> Option<std::thread::JoinHandle<()>> {
    cx.try_global::<Instance>()?;
    cx.global_mut::<Instance>().service.shutdown()
}
/// Reinstall the instance endpoint when spawning the replacement application failed.
pub(crate) fn recover(service: instance::Service, cx: &mut App) {
    let instance = cx.global::<Instance>();
    let main = instance.main;
    let app = instance.app.clone();
    install(service, main, app, cx);
}
/// Native close and the Linux titlebar button share the same cleanup policy.
pub(crate) fn close_main(window: &mut Window, cx: &mut App) {
    if !crate::tray::close_to_tray(cx) {
        crate::core::quit(cx);
        return;
    }
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(RawWindowHandle::AppKit(handle)) =
            HasWindowHandle::window_handle(window).map(|handle| handle.as_raw())
        {
            // Hide this window, not NSApplication: unhiding the application for
            // a tray panel would otherwise restore every application window.
            let view = unsafe { &*handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
            if let Some(native) = view.window() {
                native.orderOut(None);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match HasWindowHandle::window_handle(window).map(|handle| handle.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                    handle.hwnd.get() as _,
                    windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE,
                );
            },
            _ => crate::ui::show_error("无法取得主窗口句柄".into(), window, cx),
        }
    }
    // GPUI exposes minimization on both X11 and Wayland, but no hide/show API.
    #[cfg(target_os = "linux")]
    window.minimize_window();
    #[cfg(target_os = "macos")]
    let _ = window;
}

/// One foreground entry for Dock/Finder reopen, tray and single-instance activation.
pub(crate) fn show_main(main: AnyWindowHandle, cx: &mut App) {
    let _ = main.update(cx, |_, window, cx| {
        #[cfg(target_os = "windows")]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(RawWindowHandle::Win32(handle)) =
                HasWindowHandle::window_handle(window).map(|handle| handle.as_raw())
            {
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(
                        handle.hwnd.get() as _,
                        windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW,
                    );
                }
            }
        }
        window.activate_window();
        cx.activate(true);
    });
}
#[cfg(target_os = "macos")]
pub(crate) fn configure_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    if let Some(main_thread) = MainThreadMarker::new()
        && let Some(icon) = NSImage::initWithData(
            NSImage::alloc(),
            &NSData::with_bytes(include_bytes!("../../assets/logo.png")),
        )
    {
        unsafe {
            NSApplication::sharedApplication(main_thread).setApplicationIconImage(Some(&icon));
        }
    }
}

pub(crate) fn reopen(cx: &mut App) {
    if let Some(instance) = cx.try_global::<Instance>() {
        show_main(instance.main.into(), cx);
    }
}
pub(crate) fn install(
    service: instance::Service,
    main: WindowHandle<base::Root>,
    app: Entity<crate::app::SukiApp>,
    cx: &mut App,
) {
    let receiver = service.events.clone();
    let application = app.clone();
    let task = cx.spawn(async move |cx| {
        while let Ok(activation) = receiver.recv().await {
            let app = app.clone();
            cx.update(|cx| show_main(main.into(), cx));
            let _ = main.update(cx, |_, window, cx| {
                if let Some(uri) = activation.uri {
                    app.update(cx, |app, cx| app.import_uri(&uri, window, cx));
                }
            });
        }
    });
    let quit = cx.on_app_quit(|cx| {
        let worker = cx.global_mut::<Instance>().service.shutdown();
        cx.background_spawn(async move {
            if let Some(worker) = worker {
                let _ = worker.join();
            }
        })
    });
    cx.set_global(Instance {
        service,
        main,
        app: application,
        _task: task,
        _quit: quit,
    });
}
