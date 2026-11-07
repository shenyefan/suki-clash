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

/// Prefer the side with room, then clamp to the usable display (including negative origins).
/// Content never affects this size; only the display work area can constrain it.
pub(super) fn bounds(
    anchor: Bounds<Pixels>,
    work: Bounds<Pixels>,
    desired: Size<Pixels>,
    gap: Pixels,
) -> Bounds<Pixels> {
    let panel_size = size(
        desired.width.min(work.size.width),
        desired.height.min(work.size.height),
    );
    let below = anchor.bottom() + gap;
    let above = anchor.top() - gap - panel_size.height;
    let y = if below + panel_size.height <= work.bottom() {
        below
    } else {
        above
    };
    let x = anchor.right() - panel_size.width;
    Bounds::new(
        point(
            x.clamp(work.left(), work.right() - panel_size.width),
            y.clamp(work.top(), work.bottom() - panel_size.height),
        ),
        panel_size,
    )
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

#[cfg(test)]
mod tests {
    use super::bounds;
    use gpui_kit::{Bounds, point, px, size};
    #[test]
    fn edges_dpi_and_content_do_not_change_popup_size_or_leave_work_area() {
        for work in [
            Bounds::new(point(px(0.), px(24.)), size(px(1920.), px(1016.))),
            Bounds::new(point(px(-1280.), px(-200.)), size(px(1280.), px(960.))),
        ] {
            for scale in [1., 1.25, 1.5, 2.] {
                let desired = size(px(375.), px(540.));
                for origin in [work.origin, work.bottom_right(), work.center()] {
                    let anchor = Bounds::new(origin, size(px(24. / scale), px(24. / scale)));
                    let result = bounds(anchor, work, desired, px(4.));
                    assert_eq!(result.size, desired);
                    assert!(result.left() >= work.left() && result.top() >= work.top());
                    assert!(result.right() <= work.right() && result.bottom() <= work.bottom());
                }
            }
        }
    }
    #[test]
    fn tiny_display_constrains_once_and_prefers_below_or_above_icon() {
        let work = Bounds::new(point(px(0.), px(24.)), size(px(300.), px(400.)));
        let anchor = Bounds::new(point(px(280.), px(0.)), size(px(20.), px(24.)));
        assert_eq!(
            bounds(anchor, work, size(px(375.), px(540.)), px(4.)).size,
            work.size
        );
        let work = Bounds::new(point(px(0.), px(24.)), size(px(1920.), px(1032.)));
        let bottom = Bounds::new(point(px(1800.), px(1056.)), size(px(24.), px(24.)));
        assert_eq!(
            bounds(bottom, work, size(px(375.), px(540.)), px(4.)).bottom(),
            bottom.top() - px(4.)
        );
    }
}
