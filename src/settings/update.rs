//! The update dialog projects actor-owned results; no network work starts during rendering.
use crate::core::app_update::Command;
use crate::ui::theme::{control, layout};
use crate::{core, ui, updates::State};
use gpui_kit::{
    assets::IconName,
    component::{
        Sizable, WindowExt,
        button::{Button, ButtonVariants},
        link::Link,
        notification::Notification,
        text::TextView,
    },
    prelude::FluentBuilder as _,
    *,
};

pub(super) fn open(window: &mut Window, cx: &mut App) {
    let checking = ui::DownloadToast::show("检查更新", 0, None, window, cx);
    let on_error = checking.clone();
    if let Err(error) = core::execute(
        core::Command::AppUpdate(Command::Check),
        window,
        cx,
        move |result, window, cx| {
            ui::DownloadToast::dismiss(&checking, window, cx);
            if result.is_ok() {
                let state = core::controller(cx).read(cx).snapshot().app_update.clone();
                checked(state, window, cx);
            }
        },
    ) {
        ui::DownloadToast::dismiss(&on_error, window, cx);
        ui::show_error(error, window, cx);
    }
}

fn checked(state: State, window: &mut Window, cx: &mut App) {
    if let Some(url) = state.manual_url {
        ui::notifications::push(
            window,
            Notification::info("请前往发布页检查更新").action(move |_, _, _| {
                let url = url.clone();
                Button::new("app-update-release-page")
                    .icon(IconName::ExternalLink)
                    .outline()
                    .label("查看发布页")
                    .on_click(move |_, _, cx| cx.open_url(&url))
            }),
            cx,
        );
    } else if state.downloaded.is_some() {
        prompt_install(state, window, cx);
    } else if state.available.is_some() {
        show(state, window, cx);
    } else {
        ui::notifications::push(window, Notification::info("当前已是最新版本"), cx);
    }
}

fn show(state: State, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let expected = state.available.clone();
        ui::modal(
            dialog,
            format!(
                "发现新版本：{}",
                state.available.as_deref().unwrap_or_default()
            ),
            layout::EDITOR_DIALOG_WIDTH,
            window,
            cx,
        )
        .content(ui::modal_content(
            "app-update-content",
            layout::FORM_MAX_HEIGHT,
            {
                let state = state.clone();
                move |_, _| {
                    UpdateStatus {
                        state: state.clone(),
                    }
                    .into_any_element()
                }
            },
        ))
        .footer(
            ui::modal_footer()
                .when_some(state.comparison_url.clone(), |footer, url| {
                    footer.child(
                        Link::new("app-update-comparison")
                            .href(url)
                            .child("在 GitHub 查看完整变更"),
                    )
                })
                .child(div().flex_1())
                .child(
                    Button::new("app-update-close")
                        .icon(IconName::X)
                        .label("关闭")
                        .with_size(control::STANDARD)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .when_some(expected, |footer, expected| {
                    footer.child(
                        Button::new("app-update-primary")
                            .primary()
                            .label("下载更新")
                            .icon(IconName::Download)
                            .with_size(control::STANDARD)
                            .on_click(move |_, window, cx| {
                                match core::execute(
                                    core::Command::AppUpdate(Command::Download {
                                        expected: expected.clone(),
                                    }),
                                    window,
                                    cx,
                                    |result, window, cx| {
                                        if result.is_ok() {
                                            let state = core::controller(cx)
                                                .read(cx)
                                                .snapshot()
                                                .app_update
                                                .clone();
                                            prompt_install(state, window, cx);
                                        }
                                    },
                                ) {
                                    Ok(()) => window.close_dialog(cx),
                                    Err(error) => ui::show_error(error, window, cx),
                                }
                            }),
                    )
                }),
        )
    });
}

fn prompt_install(state: State, window: &mut Window, cx: &mut App) {
    let Some(expected) = state.downloaded else {
        return;
    };
    window.open_alert_dialog(cx, move |dialog, window, cx| {
        let expected = expected.clone();
        ui::modal_alert(dialog, window, cx)
            .title(format!("安装 {expected} 并重启？"))
            .description("更新已下载。")
            .footer(ui::dialog_footer("安装并重启"))
            .on_ok(move |_, window, cx| {
                let state = &core::controller(cx).read(cx).snapshot().app_update;
                if state.downloaded.as_deref() == Some(expected.as_str()) {
                    core::apply_app_update(expected.clone(), cx);
                } else {
                    ui::show_error("应用更新信息已变化，请重新检查".into(), window, cx);
                }
                true
            })
    });
}

#[derive(IntoElement)]
struct UpdateStatus {
    state: State,
}
impl RenderOnce for UpdateStatus {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        ui::form().child(
            TextView::markdown("app-update-changelog", self.state.changelog)
                .selectable(true)
                .scrollable(false),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{checked, show};
    use crate::updates::State;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Context, IntoElement, Render, TestAppContext, Window, WindowOptions, div,
    };

    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui_kit::test]
    fn no_update_uses_a_toast_and_keeps_an_existing_dialog(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::notifications::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            use gpui_kit::component::WindowExt;
            window.open_dialog(cx, |dialog, window, cx| {
                crate::ui::modal(
                    dialog,
                    "其他设置",
                    crate::ui::theme::layout::DIALOG_WIDTH,
                    window,
                    cx,
                )
            });
            checked(
                State {
                    checked: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            window.render_frame(cx);
            assert_eq!(window.find_all("dialog").len(), 1);
            assert!(window.find_all("app-update-primary").is_empty());
            assert_eq!(crate::ui::notifications::current(window, cx).len(), 1);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn update_details_fit_short_and_long_notes_with_actions_visible(cx: &mut TestAppContext) {
        use gpui_kit::{px, size};
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let handle = cx.open_window(size(px(960.), px(640.)), |window, cx| {
            let host = cx.new(|_| Host);
            gpui_kit::base::Root::new(host, window, cx)
        });
        let mut short_height = px(0.);
        for count in [2, 80] {
            cx.update_window(handle.into(), |_, window, cx| {
                use gpui_kit::component::WindowExt;
                show(
                    State {
                        available: Some("99.0.0".into()),
                        checked: true,
                        changelog: (0..count)
                            .map(|index| format!("- 修复第 {index} 项，保留完整变更链接\n"))
                            .collect(),
                        comparison_url: Some(
                            "https://github.com/owner/repo/compare/v1...v99".into(),
                        ),
                        ..Default::default()
                    },
                    window,
                    cx,
                );
                window.render_frame(cx);
                let surface = window.find("surface").bounds();
                let action = window.find("app-update-primary").bounds();
                assert!(action.bottom() <= window.viewport_size().height);
                assert!(surface.bottom() <= window.viewport_size().height);
                assert!(window.find("app-update-close").visible());
                if count == 2 {
                    short_height = surface.size.height;
                } else {
                    assert!(surface.size.height > short_height);
                }
                window.close_dialog(cx);
            })
            .unwrap();
        }
    }

    #[gpui_kit::test]
    fn downloaded_update_opens_one_restart_confirmation_and_can_be_cancelled(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            checked(
                State {
                    available: Some("0.2.0".into()),
                    downloaded: Some("0.2.0".into()),
                    checked: true,
                    ..Default::default()
                },
                window,
                cx,
            );
            window.render_frame(cx);
            assert_eq!(window.find_all("dialog").len(), 1);
            assert!(window.find("dialog-commit").visible());
            assert!(window.find_all("app-update-primary").is_empty());
            window.click("dialog-cancel", cx);
            window.render_frame(cx);
            assert!(window.find_all("dialog").is_empty());
        })
        .unwrap();
    }
}
