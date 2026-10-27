//! Application modal frame. The body owns one bounded scroll region;
//! the native title, close affordance and footer remain outside that region.
use super::theme::{layout, space};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable, WindowExt,
        button::Button,
        dialog::{AlertDialog, Dialog, DialogContent, DialogEntrance, DialogFooter},
        scroll::ScrollableElement,
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
    max_height: Rems,
    build: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
) -> impl Fn(DialogContent, &mut Window, &mut App) -> DialogContent {
    move |content, window, cx| {
        let available = (window.viewport_size().height
            - layout::DIALOG_RESERVED_HEIGHT.to_pixels(window.rem_size()))
        .max(space::NONE.to_pixels(window.rem_size()));
        let height = available.min(max_height.to_pixels(window.rem_size()));
        let body = build(window, cx);
        content
            .flex_initial()
            .flex_shrink_0()
            .min_h(space::NONE)
            .max_h(height)
            .child(
                div()
                    .w_full()
                    .min_h(space::NONE)
                    .max_h(height)
                    .child(body)
                    .overflow_y_scrollbar()
                    .id(id),
            )
    }
}

pub(crate) fn modal_close(label: &str) -> DialogFooter {
    DialogFooter::new().child(
        Button::new("modal-close")
            .icon(gpui_kit::assets::IconName::X)
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
    fn dialog_body_fits_short_content_and_scrolls_long_content_without_a_gutter(
        cx: &mut TestAppContext,
    ) {
        use gpui_kit::component::v_flex;
        use gpui_kit::test::TestSupportExt as _;
        use gpui_kit::{
            Bounds, ElementId, InteractiveElement, ScrollDelta, SharedString, Styled, WindowBounds,
            point, px, size,
        };
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(960.), px(640.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(|_| Host),
            )
            .unwrap()
        });
        let mut short_height = None;
        for count in [2usize, 30] {
            cx.update_window(handle, |_, window, cx| {
                window.open_dialog(cx, move |dialog, window, cx| {
                    super::modal(
                        dialog,
                        "通用表单",
                        crate::ui::theme::layout::DIALOG_WIDTH,
                        window,
                        cx,
                    )
                    .content(super::modal_content(
                        "fit-scroll",
                        crate::ui::theme::layout::DIALOG_FORM_HEIGHT,
                        move |_, _| {
                            v_flex()
                                .w_full()
                                .gap(crate::ui::theme::space::SM)
                                .children((0..count).map(|ix| {
                                    Button::new(SharedString::from(format!("modal-fit-row-{ix}")))
                                        .label("设置项")
                                        .w_full()
                                }))
                                .into_any_element()
                        },
                    ))
                    .footer(
                        gpui_kit::div()
                            .id(ElementId::from("modal-fit-footer"))
                            .w_full()
                            .test_support()
                            .child(Button::new("modal-fit-submit").label("保存")),
                    )
                });
                window.render_frame(cx);
            })
            .unwrap();
            cx.run_until_parked();
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let surface = window.find("surface").bounds();
                let first = window.find("modal-fit-row-0").bounds();
                let footer = window.find("modal-fit-footer").bounds();
                assert!(
                    (first.origin.x - footer.origin.x).abs() <= px(1.),
                    "left edges differ: {first:?} {footer:?}"
                );
                assert!(
                    (first.right() - footer.right()).abs() <= px(1.),
                    "right edges differ: {first:?} {footer:?}"
                );
                assert!(
                    ((first.origin.x - surface.origin.x) - (surface.right() - first.right())).abs()
                        <= px(1.),
                    "dialog side insets differ: {surface:?} {first:?}"
                );
                if count == 2 {
                    short_height = Some(surface.size.height);
                    let last = window.find("modal-fit-row-1").bounds();
                    assert!(
                        footer.origin.y - last.bottom() < px(40.),
                        "short form has forced blank height: {last:?} {footer:?}"
                    );
                } else {
                    assert!(surface.size.height > short_height.unwrap());
                    assert!(surface.size.height < window.viewport_size().height);
                    window.scroll(
                        "modal-fit-row-0",
                        ScrollDelta::Pixels(point(px(0.), px(-200.))),
                        cx,
                    );
                    assert!(
                        window.find("modal-fit-row-0").bounds().origin.y < first.origin.y,
                        "long body did not scroll"
                    );
                    assert_eq!(
                        window.find("modal-fit-footer").bounds(),
                        footer,
                        "footer moved with content"
                    );
                }
                window.close_dialog(cx);
                window.render_frame(cx);
            })
            .unwrap();
            cx.run_until_parked();
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
