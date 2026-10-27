use crate::ui::theme::window;
use gpui_kit::{component::TitleBar, *};

/// Native backdrop selection stays at the window boundary. GPUI's macOS blur
/// uses NSVisualEffectView; Windows blur uses the compositor's Acrylic effect.
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
    window.set_background_appearance(resolve_background(value));
}

fn resolve_background(value: &str) -> WindowBackgroundAppearance {
    if value == "纯色" {
        WindowBackgroundAppearance::Opaque
    } else {
        native_background()
    }
}

fn background_appearance(cx: &App) -> WindowBackgroundAppearance {
    resolve_background(
        crate::integration::read("settings", cx)
            .get("BackdropIndex")
            .map(String::as_str)
            .unwrap_or_default(),
    )
}

fn native_background() -> WindowBackgroundAppearance {
    // gpui-pre-windows 0.3.10 maps Blurred to ACCENT_ENABLE_ACRYLICBLURBEHIND.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        WindowBackgroundAppearance::Blurred
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        WindowBackgroundAppearance::Opaque
    }
}

/// Exclude controls from TitleBar's native Drag hitbox. Stopping event bubbling
/// alone is too late on Windows: WM_NCHITTEST runs before GPUI click handlers.
pub(super) fn titlebar_control(control: impl IntoElement) -> Div {
    div().occlude().child(control)
}

#[cfg(test)]
mod tests {
    use super::{native_background, resolve_background, titlebar_control};
    use gpui_kit::{
        AppContext as _, Context, FocusHandle, InteractiveElement, IntoElement, MouseButton,
        ParentElement, Render, Styled, TestAppContext, Window, WindowBackgroundAppearance,
        component::TitleBar,
        div, px, size,
        test::{TestSupportExt, TestWindowExt},
    };
    use std::{cell::Cell, rc::Rc};

    struct TitlebarFixture {
        focus: FocusHandle,
        activations: Rc<Cell<usize>>,
        drag_presses: Rc<Cell<usize>>,
    }

    impl Render for TitlebarFixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicked = self.activations.clone();
            let dragged = self.drag_presses.clone();
            div().track_focus(&self.focus).child(
                TitleBar::new().child(
                    div()
                        .id("drag-region")
                        .flex()
                        .size_full()
                        .on_mouse_down(MouseButton::Left, move |_, _, _| {
                            dragged.set(dragged.get() + 1);
                        })
                        .child(titlebar_control(
                            crate::ui::icon_button(
                                "titlebar-button",
                                gpui_kit::assets::IconName::PanelLeft,
                                "切换侧栏",
                            )
                            .on_click(move |_, _, _| clicked.set(clicked.get() + 1)),
                        ))
                        .child(div().id("drag-space").test_support().flex_1().h_full()),
                ),
            )
        }
    }

    #[test]
    fn saved_backgrounds_resolve_consistently_at_startup_and_after_save() {
        assert_eq!(
            resolve_background("纯色"),
            WindowBackgroundAppearance::Opaque
        );
        for value in ["", "Acrylic", "原生磨砂"] {
            assert_eq!(resolve_background(value), native_background());
        }
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        assert_eq!(native_background(), WindowBackgroundAppearance::Blurred);
    }

    #[gpui_kit::test]
    fn titlebar_controls_receive_clicks_without_hitting_the_drag_region(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let activations = Rc::new(Cell::new(0));
        let drag_presses = Rc::new(Cell::new(0));
        let clicked = activations.clone();
        let dragged = drag_presses.clone();
        let handle = cx.open_window(size(px(640.), px(480.)), move |window, cx| {
            let fixture = cx.new(|cx| {
                let focus = cx.focus_handle();
                focus.focus(window, cx);
                TitlebarFixture {
                    focus,
                    activations: clicked,
                    drag_presses: dragged,
                }
            });
            gpui_kit::base::Root::new(fixture, window, cx)
        });
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            window.click("titlebar-button", cx);
            assert_eq!(activations.get(), 1);
            assert_eq!(
                drag_presses.get(),
                0,
                "button entered the window drag region"
            );
        })
        .unwrap();
        // Tab and button activation dispatch deferred actions through Root.
        for key in ["tab", "space", "enter"] {
            cx.update_window(handle.into(), |_, window, cx| window.press(key, cx))
                .unwrap();
            cx.run_until_parked();
        }
        cx.update_window(handle.into(), |_, window, cx| {
            assert_eq!(activations.get(), 3, "keyboard activation stopped working");

            window.click("drag-space", cx);
            assert_eq!(
                drag_presses.get(),
                1,
                "empty title bar stopped accepting drags"
            );
        })
        .unwrap();
    }
}
