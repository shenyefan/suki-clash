//! Tray screen coordinates are converted by the platform that owns them.
use gpui_kit::*;

pub(super) fn anchor(event: &tray_icon::TrayIconEvent, cx: &App) -> Result<Bounds<Pixels>, String> {
    let tray_icon::TrayIconEvent::Click { position, rect, .. } = event else {
        return Err("托盘事件缺少位置".into());
    };
    #[cfg(target_os = "macos")]
    let scale = {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSEvent, NSScreen};
        let main = MainThreadMarker::new().ok_or("托盘定位需要主线程")?;
        let pointer = NSEvent::mouseLocation();
        let screens = NSScreen::screens(main);
        screens
            .iter()
            .find(|screen| {
                let frame = screen.frame();
                pointer.x >= frame.origin.x
                    && pointer.x < frame.origin.x + frame.size.width
                    && pointer.y >= frame.origin.y
                    && pointer.y < frame.origin.y + frame.size.height
            })
            .ok_or("无法定位托盘所在显示器")?
            .backingScaleFactor() as f32
    };
    #[cfg(windows)]
    let scale = {
        use windows_sys::Win32::{
            Foundation::POINT,
            Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint},
            UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        };
        let monitor = unsafe {
            MonitorFromPoint(
                POINT {
                    x: position.x as i32,
                    y: position.y as i32,
                },
                MONITOR_DEFAULTTONEAREST,
            )
        };
        let (mut x, mut y) = (0, 0);
        let result = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
        if result < 0 {
            return Err(format!("无法读取托盘显示器缩放：{result:#x}"));
        }
        x as f32 / 96.
    };
    // SNI coordinates are the desktop host's screen-coordinate hint, not the
    // main window's physical scale. In particular, never divide by main-window DPI.
    #[cfg(target_os = "linux")]
    let scale = 1.;
    let _ = cx;
    if rect.size.width > 0 && rect.size.height > 0 {
        Ok(Bounds::new(
            point(
                px(rect.position.x as f32 / scale),
                px(rect.position.y as f32 / scale),
            ),
            size(
                px(rect.size.width as f32 / scale),
                px(rect.size.height as f32 / scale),
            ),
        ))
    } else {
        Ok(Bounds::new(
            point(px(position.x as f32 / scale), px(position.y as f32 / scale)),
            size(px(0.), px(0.)),
        ))
    }
}

pub(super) fn options(anchor: Bounds<Pixels>, cx: &App) -> Result<WindowOptions, String> {
    let display = cx
        .displays()
        .into_iter()
        .find(|d| d.bounds().contains(&anchor.center()))
        .or_else(|| cx.primary_display())
        .ok_or("没有可用显示器")?;
    let bounds = display.visible_bounds();
    #[allow(unused_mut)]
    let mut kind = WindowKind::PopUp;
    #[cfg(target_os = "linux")]
    if cx.compositor_name() != "X11" {
        use gpui_kit::layer_shell::{Anchor, KeyboardInteractivity, LayerShellOptions};
        let origin = bounds.origin - display.bounds().origin;
        kind = WindowKind::LayerShell(LayerShellOptions {
            namespace: "suki-clash-tray".into(),
            anchor: Anchor::TOP | Anchor::LEFT,
            margin: Some((origin.y, px(0.), px(0.), origin.x)),
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            ..Default::default()
        });
    }
    Ok(WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        kind,
        display_id: Some(display.id()),
        focus: true,
        show: true,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    })
}
