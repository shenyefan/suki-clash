//! Application modal frame. The body owns one bounded scroll region;
//! the native title, close affordance and footer remain outside that region.
use super::theme::{layout, space};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        ActiveTheme, Sizable, WindowExt,
        button::Button,
        dialog::{AlertDialog, Dialog, DialogContent, DialogEntrance, DialogFooter},
        h_flex,
        scroll::{Scrollbar, ScrollbarMode},
    },
    *,
};

pub(crate) fn modal(
    dialog: Dialog,
    title: impl Into<SharedString>,
    width: Rems,
    window: &Window,
    cx: &App,
) -> Dialog {
    dialog
        .entrance(DialogEntrance::FadeSlide)
        .title(title.into())
        .width(width.to_pixels(window.rem_size()))
        .bg(cx.theme().popover)
        .rounded(cx.theme().radius_lg)
        .p(space::SECTION)
        .gap(space::LG)
        .overlay_closable(false)
}

pub(crate) fn modal_content(
    id: &'static str,
    preferred_height: Rems,
    build: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
) -> impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent {
    move |content, window, cx| {
        let available = (window.viewport_size().height
            - layout::DIALOG_RESERVED_HEIGHT.to_pixels(window.rem_size()))
        .max(space::NONE.to_pixels(window.rem_size()));
        let height = available.min(preferred_height.to_pixels(window.rem_size()));
        let body = build(window, cx);
        let handle = window
            .use_keyed_state((ElementId::from("modal-scroll"), id), cx, |_, _| {
                ScrollHandle::new()
            })
            .read(cx)
            .clone();
        content
            .flex_initial()
            .flex_shrink_0()
            .h(height)
            .min_h(space::NONE)
            .child(
                div()
                    .w_full()
                    .h(height)
                    .child(modal_scroll(id, &handle, body)),
            )
    }
}

pub(crate) fn modal_close(label: &str) -> DialogFooter {
    DialogFooter::new().child(
        Button::new("modal-close")
            .with_size(super::theme::control::STANDARD)
            .label(label.to_owned())
            .on_click(|_, window, cx| window.close_dialog(cx)),
    )
}

pub(crate) fn modal_alert(dialog: AlertDialog, window: &Window, cx: &App) -> AlertDialog {
    dialog
        .entrance(DialogEntrance::FadeSlide)
        .width(layout::DIALOG_WIDTH.to_pixels(window.rem_size()))
        .bg(cx.theme().popover)
        .rounded(cx.theme().radius_lg)
        .p(space::SECTION)
        .gap(space::LG)
}

/// Bounded layout for dialogs whose view fixes its own toolbar and scrolls only its body.
pub(crate) fn modal_panel(
    preferred_height: Rems,
    build: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
) -> impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent {
    move |content, window, cx| {
        let available = (window.viewport_size().height
            - layout::DIALOG_RESERVED_HEIGHT.to_pixels(window.rem_size()))
        .max(Pixels::ZERO);
        let height = available.min(preferred_height.to_pixels(window.rem_size()));
        let body = build(window, cx);
        content
            .flex_initial()
            .flex_shrink_0()
            .h(height)
            .min_h(space::NONE)
            .child(div().w_full().h(height).min_h(space::NONE).child(body))
    }
}

/// The scrollbar has a fixed lane outside the scroll target, so it never moves
/// with content or steals space from inputs and row actions.
pub(crate) fn modal_scroll(id: &'static str, handle: &ScrollHandle, body: AnyElement) -> Div {
    #[cfg(test)]
    use gpui_kit::test::TestSupportExt as _;
    h_flex()
        .size_full()
        .min_h(space::NONE)
        .items_stretch()
        .child(
            div()
                .id(id)
                .flex_1()
                .min_w(space::NONE)
                .h_full()
                .overflow_y_scroll()
                .map(|region| {
                    #[cfg(test)]
                    let region = region.test_support();
                    region
                })
                .track_scroll(handle)
                .child(body),
        )
        .child(
            div().w(Scrollbar::width()).h_full().flex_shrink_0().child(
                Scrollbar::vertical(handle)
                    .id((ElementId::from(id), "bar"))
                    .mode(ScrollbarMode::Always)
                    .viewport_from_layout(),
            ),
        )
}

#[cfg(test)]
mod motion_tests {
    use gpui_kit::{
        AppContext, Context, IntoElement, ParentElement, Render, TestAppContext, Window,
        WindowOptions, div,
    };
    use gpui_kit::{
        component::{WindowExt, button::Button},
        test::TestWindowExt,
    };
    use std::time::Duration;
    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    #[gpui_kit::test]
    fn dialog_stays_mounted_during_exit_then_restores_focus(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(false);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        let focus = cx.update(|cx| cx.focus_handle());
        cx.update_window(handle, |_, window, cx| {
            window.focus(&focus, cx);
            window.open_dialog(cx, |dialog, window, cx| {
                super::modal(
                    dialog,
                    "退场验证",
                    crate::ui::theme::layout::DIALOG_WIDTH,
                    window,
                    cx,
                )
                .child(Button::new("test-dialog-content").label("内容"))
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(220));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("surface").bounds();
            window.click("close", cx);
            assert!(
                !window.has_active_dialog(cx),
                "an exiting dialog must no longer be active"
            );
            assert_eq!(
                window.find("surface").bounds(),
                bounds,
                "the first exit frame must not jump"
            );
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(70));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(!window.has_active_dialog(cx));
            assert!(!window.find_all("test-dialog-content").is_empty());
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(220));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(!window.has_active_dialog(cx));
            assert!(window.find_all("test-dialog-content").is_empty());
            assert!(focus.is_focused(window));
        })
        .unwrap();
    }
}
