use super::{SettingsKind, SettingsPage};
use crate::{
    integration, ui,
    ui::theme::{control, layout, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Icon, Sizable,
        button::{Button, ButtonVariants},
        input::{Editor, Input},
        scroll::ScrollableElement,
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder as _,
    *,
};

impl SettingsPage {
    pub(super) fn runtime_view(&self, cx: &mut Context<Self>) -> Div {
        let tabs: &[&str] = match self.kind {
            SettingsKind::Core => &["常规", "网络", "访问", "维护"],
            SettingsKind::Dns => &["常规", "服务器", "Fake-IP", "解析策略", "Hosts"],
            SettingsKind::Sniffer => &["常规", "协议", "过滤"],
            SettingsKind::Resources => &["数据库", "代理集合", "规则集合"],
            SettingsKind::SysProxy => &["常规", "绕过", "PAC"],
            SettingsKind::Tun => &["常规", "路由与 DNS", "高级"],
            _ => &["常规", "外观", "托盘", "网络", "同步", "维护"],
        };
        let query = self.filter.read(cx).value().to_lowercase();
        let header = ui::workspace_header(self.kind.label(), "", cx).child(
            TabBar::new("runtime-tabs")
                .segmented()
                .with_size(control::STANDARD)
                .children(tabs.iter().map(|label| Tab::new().label(*label)))
                .selected_index(self.runtime_tab)
                .on_click(cx.listener(|this, ix, _, cx| {
                    this.runtime_tab = *ix;
                    cx.notify();
                })),
        );
        let toolbar =
            ui::workspace_toolbar()
                .child(
                    div().flex_1().min_w(space::NONE).child(
                        Input::new(&self.filter)
                            .with_size(control::STANDARD)
                            .w_full()
                            .cleanable(true)
                            .prefix(Icon::new(IconName::Search).size(layout::ICON))
                            .aria_label("搜索当前页面"),
                    ),
                )
                .when(self.kind == SettingsKind::Dns, |toolbar| {
                    toolbar.child(
                        Button::new("dns-edit-mode")
                            .outline()
                            .with_size(control::STANDARD)
                            .label(if self.dns_raw {
                                "表单编辑"
                            } else {
                                "YAML 编辑"
                            })
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.switch_dns_mode(!this.dns_raw, w, cx)
                            })),
                    )
                })
                .when(self.kind == SettingsKind::Resources, |toolbar| {
                    toolbar.child(
                        Button::new("resources-update-all")
                            .outline()
                            .with_size(control::STANDARD)
                            .icon(IconName::RefreshCw)
                            .label("全部更新")
                            .disabled(self.runtime_tab != 0)
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.runtime_command(
                                    ["更新 GeoData", "更新全部代理集合", "更新全部规则集合"]
                                        [this.runtime_tab],
                                    w,
                                    cx,
                                )
                            })),
                    )
                });
        let body = if self.kind == SettingsKind::Dns && self.dns_raw {
            gpui_kit::component::v_flex()
                .gap(space::SM)
                .child(
                    ui::muted("编辑 DNS 与 Hosts；切回表单前会检查 YAML 格式。", cx)
                        .text_size(text::CAPTION),
                )
                .child(
                    Editor::new(&self.dns_source)
                        .w_full()
                        .h(layout::EDITOR_HEIGHT)
                        .aria_label("DNS YAML 配置"),
                )
        } else {
            match self.kind {
                SettingsKind::Core => self.core_content(&query, cx),
                SettingsKind::Resources => self.resource_content(&query, cx),
                _ => self.preferences_content(&query, cx),
            }
        };
        ui::workspace()
            .child(header)
            .child(toolbar)
            .child(
                ui::workspace_body().child(
                    div()
                        .id(SharedString::from(format!(
                            "{}-runtime-scroll",
                            self.kind.id()
                        )))
                        .size_full()
                        .overflow_y_scrollbar()
                        .child(body.py(space::SM)),
                ),
            )
            .child(
                ui::workspace_footer()
                    .child(
                        ui::muted(
                            if self.dirty {
                                "有未保存的更改"
                            } else {
                                ""
                            },
                            cx,
                        )
                        .text_size(text::CAPTION),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("runtime-reset")
                            .ghost()
                            .with_size(control::STANDARD)
                            .label("恢复默认…")
                            .on_click(cx.listener(|this, _, w, cx| this.reset(w, cx))),
                    )
                    .child(
                        Button::new("runtime-save")
                            .outline()
                            .with_size(control::STANDARD)
                            .icon(IconName::Save)
                            .label(if self.saving {
                                "保存中"
                            } else {
                                "保存设置"
                            })
                            .disabled(!self.dirty || self.saving)
                            .on_click(cx.listener(|this, _, w, cx| this.save(w, cx))),
                    ),
            )
    }
    pub(super) fn runtime_command(
        &mut self,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let values = self.command_values(cx);
        if let Some(error) = super::schema::validate(&self.sections, &values) {
            ui::show_error(error, window, cx);
            return;
        }
        integration::request_with(action, values, window, cx);
    }
}
