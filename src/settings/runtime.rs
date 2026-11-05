use super::{SettingsKind, SettingsPage};
use crate::{
    integration, ui,
    ui::theme::{control, layout, space},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Icon, Sizable,
        button::Button,
        input::{Editor, Input},
        scroll::ScrollableElement,
        switch::Switch,
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder as _,
    *,
};

pub(super) enum DeferredAction {
    ResetProgram,
    FlushFakeIp,
    Service(crate::core::ServiceAction),
}
impl DeferredAction {
    fn label(&self) -> &'static str {
        match self {
            Self::ResetProgram => "重置程序",
            Self::FlushFakeIp => "清空 Fake-IP 缓存",
            Self::Service(_) => "卸载服务",
        }
    }
}
impl SettingsPage {
    pub(super) fn runtime_view(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if self.kind == SettingsKind::SysProxy {
            return self.system_proxy_view(window, cx);
        }
        let tabs: &[&str] = match self.kind {
            SettingsKind::Core => &["常规", "网络", "访问", "维护"],
            SettingsKind::Dns => &["常规", "服务器", "Fake-IP", "解析策略", "Hosts"],
            SettingsKind::Sniffer => &["常规", "协议", "过滤"],
            SettingsKind::Resources => &["数据库", "代理集合", "规则集合"],
            SettingsKind::Tun => &["常规", "路由与 DNS"],
            _ => &["常规", "外观", "托盘", "网络", "维护"],
        };
        let query = self.filter.read(cx).value().to_lowercase();
        let heading = if matches!(self.kind, SettingsKind::Dns | SettingsKind::Sniffer) {
            let key = crate::preferences::native::flag(self.kind);
            let checked = self
                .values(cx)
                .get(key)
                .is_some_and(|value| value == "true");
            ui::workspace_header_control(
                self.kind.label(),
                "",
                Switch::new(SharedString::from(format!("setting-{key}")))
                    .with_size(control::STANDARD)
                    .checked(checked)
                    .disabled(self.saving || self.session.read(cx).core_busy())
                    .accessibility_label(if self.kind == SettingsKind::Dns {
                        "使用全局 DNS 配置"
                    } else {
                        "使用全局嗅探配置"
                    })
                    .on_change(cx.listener(move |this, value, window, cx| {
                        this.toggle(key, *value, window, cx)
                    })),
                cx,
            )
        } else {
            ui::workspace_header(self.kind.label(), "", cx)
        };
        let header = heading.when(!self.dns_raw, |header| {
            header.child(
                TabBar::new("runtime-tabs")
                    .segmented()
                    .with_size(control::STANDARD)
                    .children(tabs.iter().map(|label| Tab::new().label(*label)))
                    .selected_index(self.runtime_tab)
                    .on_click(cx.listener(|this, ix, _, cx| {
                        this.runtime_tab = *ix;
                        cx.notify();
                    })),
            )
        });
        let toolbar = ui::workspace_toolbar()
            .child(
                div()
                    .flex_1()
                    .min_w(space::NONE)
                    .when(!self.dns_raw, |search| {
                        search.child(
                            Input::new(&self.filter)
                                .with_size(control::STANDARD)
                                .w_full()
                                .cleanable(true)
                                .prefix(Icon::new(IconName::Search).size(layout::ICON))
                                .aria_label("搜索当前页面"),
                        )
                    }),
            )
            .when(self.kind == SettingsKind::Resources, |toolbar| {
                toolbar.child(
                    Button::new("resources-update-all")
                        .outline()
                        .with_size(control::STANDARD)
                        .icon(IconName::RefreshCw)
                        .label("全部更新")
                        .disabled(
                            self.saving
                                || self.session.read(cx).core_busy()
                                || !self.session.read(cx).is_running(),
                        )
                        .on_click(cx.listener(|this, _, w, cx| this.update_resources(None, w, cx))),
                )
            });
        let body = if matches!(self.kind, SettingsKind::Dns | SettingsKind::Sniffer) && self.dns_raw
        {
            gpui_kit::component::v_flex()
                .size_full()
                .min_h(space::NONE)
                .min_w(space::NONE)
                .child(
                    Editor::new(&self.dns_source)
                        .w_full()
                        .h_full()
                        .disabled(
                            !self
                                .values(cx)
                                .get(crate::preferences::native::flag(self.kind))
                                .is_some_and(|value| value == "true")
                                || self.saving
                                || self.session.read(cx).core_busy(),
                        )
                        .aria_label(if self.kind == SettingsKind::Dns {
                            "DNS YAML 配置"
                        } else {
                            "嗅探 YAML 配置"
                        }),
                )
        } else {
            match self.kind {
                SettingsKind::Core => self.core_content(&query, window, cx),
                SettingsKind::Resources => self.resource_content(&query, window, cx),
                _ => self.preferences_content(&query, cx),
            }
        };
        ui::workspace()
            .child(header)
            .when(!self.dns_raw, |workspace| workspace.child(toolbar))
            .child(ui::workspace_body().child(if self.dns_raw {
                let viewport = body
                    .id("settings-yaml-viewport")
                    .size_full()
                    .min_h(space::NONE);
                #[cfg(test)]
                let viewport = {
                    use gpui_kit::test::TestSupportExt as _;
                    viewport.test_support()
                };
                viewport.into_any_element()
            } else if self.kind == SettingsKind::Resources && self.runtime_tab != 0 {
                body.py(space::SM)
                    .size_full()
                    .min_h(space::NONE)
                    .into_any_element()
            } else {
                div()
                    .id(SharedString::from(format!(
                        "{}-runtime-scroll",
                        self.kind.id()
                    )))
                    .size_full()
                    .overflow_y_scrollbar()
                    .child(body.py(space::SM))
                    .into_any_element()
            }))
            .when(
                matches!(self.kind, SettingsKind::Dns | SettingsKind::Sniffer),
                |workspace| {
                    workspace.child(ui::workspace_footer().child(div().flex_1()).when(
                        matches!(self.kind, SettingsKind::Dns | SettingsKind::Sniffer),
                        |footer| {
                            footer.child(
                                Button::new("dns-edit-mode")
                                    .icon(IconName::FileCode)
                                    .outline()
                                    .with_size(control::STANDARD)
                                    .label(if self.dns_raw {
                                        "表单编辑"
                                    } else {
                                        "YAML 编辑"
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.switch_dns_mode(!this.dns_raw, window, cx)
                                    })),
                            )
                        },
                    ))
                },
            )
    }
    pub(super) fn defer_destructive(
        &mut self,
        action: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let action = match action {
            "重置程序" => DeferredAction::ResetProgram,
            "清空 Fake-IP 缓存" => DeferredAction::FlushFakeIp,
            _ => {
                ui::show_error("操作不可用".into(), window, cx);
                return;
            }
        };
        self.defer_action(action, window, cx);
    }
    pub(super) fn defer_action(
        &mut self,
        action: DeferredAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_action.is_some() {
            ui::show_error("已有待执行操作，请先完成或撤销".into(), window, cx);
            return;
        }
        let owner = cx.weak_entity();
        let note = ui::notifications::undo(
            window,
            format!("将在 5 秒后{}", action.label()),
            cx,
            move |_, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.pending_action = None;
                    cx.notify();
                });
                true
            },
        );
        self.pending_action = Some(cx.spawn_in(window, async move |owner, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(5))
                .await;
            let _ = owner.update_in(cx, |this, window, cx| {
                if let Some(task) = this.pending_action.take() {
                    task.detach();
                }
                if let Some(note) = note {
                    note.update(cx, |note, cx| note.dismiss(window, cx));
                }
                match action {
                    DeferredAction::ResetProgram => crate::core::reset_program(window, cx),
                    DeferredAction::FlushFakeIp => {
                        crate::core::request(crate::core::Command::FlushFakeIp, window, cx)
                    }
                    DeferredAction::Service(action) => crate::core::request(
                        crate::core::Command::PlatformService(action),
                        window,
                        cx,
                    ),
                }
                cx.notify();
            });
        }));
    }
    pub(super) fn runtime_command(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let command = match action {
            "启动内核" => Some(crate::core::Command::Start),
            "停止内核" => Some(crate::core::Command::Stop),
            "重启内核" => Some(crate::core::Command::Restart),
            "校验配置" => Some(crate::core::Command::Validate),
            "清空 Fake-IP 缓存" => {
                self.defer_destructive("清空 Fake-IP 缓存", window, cx);
                return;
            }
            "更新 GeoData" => Some(crate::core::Command::UpdateGeo),
            "刷新版本列表" => Some(crate::core::Command::Versions { refresh: true }),
            "加载版本列表" => Some(crate::core::Command::Versions { refresh: false }),
            "升级内核" | "切换内核" | "应用所选通道" => {
                match crate::core::ReleaseSelection::from_values(&self.command_values(cx)) {
                    Ok(selection) => Some(if action == "升级内核" {
                        crate::core::Command::CheckUpdate(selection)
                    } else {
                        crate::core::Command::Install(selection)
                    }),
                    Err(error) => {
                        ui::show_error(error, window, cx);
                        return;
                    }
                }
            }
            _ => None,
        };
        if let Some(command) = command {
            crate::core::request(command, window, cx);
            return;
        }
        let values = self.command_values(cx);
        if let Some(error) = super::schema::validate(&self.sections, &values) {
            ui::show_error(error, window, cx);
            return;
        }
        integration::request_with(action, values, window, cx);
    }
}
