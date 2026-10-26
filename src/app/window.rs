use crate::ui::theme::window;
use gpui_kit::{component::TitleBar, *};

/// Native backdrop selection stays at the window boundary. GPUI's macOS blur
/// uses NSVisualEffectView; Windows Mica is supplied by the system compositor.
pub(crate) fn options(cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(
        None,
        size(window::DEFAULT_WIDTH, window::DEFAULT_HEIGHT),
        cx,
    );
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(window::MIN_WIDTH, window::MIN_HEIGHT)),
        window_background: background_appearance(cx),
        ..TitleBar::window_options()
    }
}

pub(super) fn set_background(value: &str, window: &mut Window) {
    window.set_background_appearance(if value == "纯色" {
        WindowBackgroundAppearance::Opaque
    } else {
        native_background()
    });
}

fn background_appearance(cx: &App) -> WindowBackgroundAppearance {
    if crate::integration::read("settings", cx)
        .get("BackdropIndex")
        .is_some_and(|value| value == "纯色")
    {
        WindowBackgroundAppearance::Opaque
    } else {
        native_background()
    }
}

fn native_background() -> WindowBackgroundAppearance {
    #[cfg(target_os = "windows")]
    {
        WindowBackgroundAppearance::MicaBackdrop
    }
    #[cfg(target_os = "macos")]
    {
        WindowBackgroundAppearance::Blurred
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        WindowBackgroundAppearance::Opaque
    }
}
