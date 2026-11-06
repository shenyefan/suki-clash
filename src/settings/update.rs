//! The update dialog projects actor-owned results; no network work starts during rendering.
use crate::core::app_update::Command;
use crate::ui::theme::{control, layout, space};
use crate::{core, ui, updates::State};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Sizable, WindowExt,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder as _,
    *,
};

pub(super) fn open(window: &mut Window, cx: &mut App) {
    if let Err(error) = core::execute(
        core::Command::AppUpdate(Command::Check),
        window,
        cx,
        |result, window, cx| {
            if result.is_ok() {
                let state = core::controller(cx).read(cx).snapshot().app_update.clone();
                if let Some(url) = &state.manual_url {
                    cx.open_url(url);
                } else if state.downloaded.is_some() {
                    prompt_install(state, window, cx);
                } else if state.available.is_some() {
                    show(state, window, cx);
                }
            }
        },
    ) {
        ui::show_error(error, window, cx);
    }
}

fn show(state: State, window: &mut Window, cx: &mut App) -> Entity<UpdateStatus> {
    let status = cx.new(|_| UpdateStatus { state, busy: false });
    let footer = cx.new(|cx| UpdateFooter::new(status.clone(), cx));
    let body = status.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        ui::modal(dialog, "应用更新", layout::DIALOG_WIDTH, window, cx)
            .content(ui::modal_content(
                "app-update-content",
                layout::FORM_MAX_HEIGHT,
                {
                    let body = body.clone();
                    move |_, _| body.clone().into_any_element()
                },
            ))
            .footer(footer.clone())
    });
    status
}

fn prompt_install(state: State, window: &mut Window, cx: &mut App) {
    let Some(expected) = state.downloaded else {
        return;
    };
    window.open_alert_dialog(cx, move |dialog, window, cx| {
        let expected = expected.clone();
        ui::modal_alert(dialog, window, cx)
            .title(format!("安装 {expected} 并重启？"))
            .description("更新已下载。安装前会关闭内核并清理系统代理，随后重新启动应用。")
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

struct UpdateStatus {
    state: State,
    busy: bool,
}
impl UpdateStatus {
    fn installable_version(&self) -> Option<&str> {
        self.state.downloaded.as_deref().filter(|version| {
            self.state
                .available
                .as_deref()
                .is_none_or(|available| available == *version)
        })
    }

    fn download(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(expected) = self.state.available.clone() else {
            return;
        };
        match core::execute(
            core::Command::AppUpdate(Command::Download { expected }),
            window,
            cx,
            move |result, window, cx| {
                if result.is_ok() {
                    let state = core::controller(cx).read(cx).snapshot().app_update.clone();
                    prompt_install(state, window, cx);
                }
            },
        ) {
            Ok(_) => {
                self.busy = true;
                window.close_dialog(cx);
            }
            Err(error) => ui::show_error(error, window, cx),
        }
        cx.notify();
    }

    fn confirm_install(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(_) = self.installable_version() else {
            return;
        };
        window.close_dialog(cx);
        prompt_install(self.state.clone(), window, cx);
    }
}

impl Render for UpdateStatus {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::form()
            .gap(space::MD)
            .child(ui::muted(
                format!("当前版本 {}", env!("CARGO_PKG_VERSION")),
                cx,
            ))
            .when_some(self.state.available.clone(), |view, version| {
                view.child(format!("新版本 {version}"))
            })
            .child(ui::muted(
                if self.busy {
                    "正在下载更新"
                } else if self.installable_version().is_some() {
                    "更新已下载，等待安装"
                } else if self.state.available.is_some() {
                    "发现新版本"
                } else {
                    "当前已是最新版本"
                },
                cx,
            ))
    }
}

struct UpdateFooter {
    status: Entity<UpdateStatus>,
    _subscription: Subscription,
}
impl UpdateFooter {
    fn new(status: Entity<UpdateStatus>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&status, |_, _, cx| cx.notify());
        Self {
            status,
            _subscription: subscription,
        }
    }
}
impl Render for UpdateFooter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status.read(cx);
        let busy = status.busy;
        let available = status.state.available.is_some();
        let installable = status.installable_version().is_some();
        let owner = self.status.clone();
        ui::modal_footer()
            .child(
                Button::new("app-update-close")
                    .label("关闭")
                    .with_size(control::STANDARD)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .when(installable || available, |footer| {
                footer.child(
                    Button::new("app-update-primary")
                        .primary()
                        .label(if installable {
                            "安装并重启"
                        } else {
                            "下载更新"
                        })
                        .icon(if installable {
                            IconName::RefreshCw
                        } else {
                            IconName::Download
                        })
                        .with_size(control::STANDARD)
                        .loading(busy)
                        .disabled(busy)
                        .on_click(move |_, window, cx| {
                            owner.update(cx, |this, cx| {
                                if installable {
                                    this.confirm_install(window, cx);
                                } else {
                                    this.download(window, cx);
                                }
                            });
                        }),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::show;
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
    fn downloaded_update_asks_before_install_and_cancel_keeps_package_ready(
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
            let status = show(
                State {
                    available: Some("0.2.0".into()),
                    downloaded: Some("0.2.0".into()),
                    checked: true,
                    manual_url: None,
                },
                window,
                cx,
            );
            window.render_frame(cx);
            window.click("app-update-primary", cx);
            window.render_frame(cx);
            assert!(window.find("dialog-commit").visible());
            assert!(window.find_all("app-update-primary").is_empty());
            window.click("dialog-cancel", cx);
            assert_eq!(status.read(cx).installable_version(), Some("0.2.0"));
            assert!(!status.read(cx).busy);
        })
        .unwrap();
    }
}
