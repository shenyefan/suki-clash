use super::{FieldControl, SettingsKind, SettingsPage};
use crate::ui;
use crate::ui::theme::{control, layout, space, stroke, text};
use gpui_kit::component::Selectable;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        input::Input,
        scroll::ScrollableElement,
        v_flex,
    },
    *,
};

impl SettingsPage {
    fn categories(&self, cx: &mut Context<Self>) -> Div {
        let mut rows = v_flex()
            .w(layout::LABEL_WIDTH)
            .flex_shrink_0()
            .p(space::SM)
            .gap(space::XS)
            .bg(cx.theme().muted.opacity(crate::ui::theme::opacity::PANEL))
            .border_r(stroke::HAIRLINE)
            .border_color(cx.theme().border)
            .child(
                Button::new("settings-all")
                    .ghost()
                    .with_size(control::STANDARD)
                    .w_full()
                    .h(layout::CONTROL_HEIGHT)
                    .justify_start()
                    .label("全部设置")
                    .selected(self.active_section == usize::MAX)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.active_section = usize::MAX;
                        cx.notify();
                    })),
            );
        for (index, section) in self.sections.iter().enumerate() {
            rows = rows.child(
                Button::new(SharedString::from(format!(
                    "settings-category-{}-{index}",
                    self.kind.id()
                )))
                .ghost()
                .with_size(control::STANDARD)
                .w_full()
                .h(layout::CONTROL_HEIGHT)
                .justify_start()
                .label(section.title)
                .selected(self.active_section == index)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active_section = index;
                    cx.notify();
                })),
            );
        }
        rows
    }
    fn providers(&self, cx: &App) -> Div {
        v_flex()
            .gap(space::XS)
            .pt(space::MD)
            .child(
                h_flex()
                    .h(layout::CONTROL_HEIGHT)
                    .justify_between()
                    .child(ui::section("规则集合", cx))
                    .child(
                        ui::icon_button(
                            "refresh-providers",
                            IconName::RefreshCw,
                            "全部更新规则集合",
                        )
                        .on_click(|_, w, cx| ui::request_command("更新规则集合", w, cx)),
                    ),
            )
            .child(ui::empty(
                IconName::FolderDown,
                "暂无规则集合",
                "加载配置后显示",
                cx,
            ))
    }
    pub(super) fn panels(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap(space::XS)
            .children(self.panels.iter().map(|(id, name, url)| {
                let id = *id;
                let url = url.clone();
                h_flex()
                    .h(layout::RANKING_ROW_HEIGHT)
                    .gap(space::SM)
                    .child(div().w(layout::FILTER_WIDTH).child(name.clone()))
                    .child(
                        ui::muted(url.clone(), cx)
                            .flex_1()
                            .min_w(space::NONE)
                            .truncate(),
                    )
                    .child(
                        ui::icon_button(("open-panel", id), IconName::ExternalLink, "打开面板")
                            .on_click(
                                cx.listener(move |this, _, w, cx| this.open_panel(&url, w, cx)),
                            ),
                    )
                    .child(
                        ui::icon_button(("edit-panel", id), IconName::Pencil, "编辑面板…")
                            .on_click(
                                cx.listener(move |this, _, w, cx| this.edit_panel(Some(id), w, cx)),
                            ),
                    )
                    .child(
                        ui::icon_button(("remove-panel", id), IconName::Trash, "删除面板")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.panels.retain(|panel| panel.0 != id);
                                this.dirty = true;
                                cx.notify();
                            })),
                    )
            }))
    }
}
impl Render for SettingsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.kind.uses_grouped_layout() {
            return self.runtime_view(cx).into_any_element();
        }
        let query = self.filter.read(cx).value().to_lowercase();
        let pac = self
            .fields
            .get("SystemProxyModeIndex")
            .and_then(|field| {
                if let FieldControl::Select(state) = &field.control {
                    state.read(cx).selected_value().map(|value| value == "PAC")
                } else {
                    None
                }
            })
            .unwrap_or(false);
        let sections = self
            .sections
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                self.active_section == usize::MAX
                    || !query.is_empty()
                    || *index == self.active_section
            })
            .map(|(_, section)| {
                (
                    section.title,
                    section
                        .fields
                        .iter()
                        .filter(|field| {
                            let conditional = match (self.kind, field.key) {
                                (SettingsKind::SysProxy, "PacScript") => pac,
                                (SettingsKind::SysProxy, "BypassList") => !pac,
                                _ => true,
                            };
                            conditional
                                && (query.is_empty()
                                    || field.label.to_lowercase().contains(&query)
                                    || field.description.to_lowercase().contains(&query))
                        })
                        .map(|field| field.key)
                        .collect::<Vec<_>>(),
                )
            })
            .filter(|(_, keys)| !keys.is_empty())
            .collect::<Vec<_>>();
        let no_results = sections.is_empty();
        let count = sections.iter().map(|(_, keys)| keys.len()).sum::<usize>();
        let running = self.session.read(cx).is_running();
        let icon = self.kind.icon();
        let header = ui::page_header(self.kind.label(), icon, cx)
            .child(
                div().w(layout::FIELD_WIDTH).child(
                    Input::new(&self.filter)
                        .w_full()
                        .with_size(control::STANDARD)
                        .prefix(Icon::new(IconName::Search))
                        .cleanable(true),
                ),
            )
            .when(self.kind == SettingsKind::Core, |row| {
                row.child(
                    ui::icon_button("validate-core-config", IconName::FileCheck, "校验配置")
                        .on_click(|_, w, cx| ui::request_command("校验配置", w, cx)),
                )
                .child(
                    ui::icon_button("restart-core", IconName::RefreshCw, "重启内核")
                        .on_click(|_, w, cx| ui::request_command("重启内核", w, cx)),
                )
                .child(
                    ui::icon_button(
                        "toggle-core",
                        IconName::Power,
                        if running {
                            "停止内核"
                        } else {
                            "启动内核"
                        },
                    )
                    .on_click(move |_, window, cx| {
                        ui::request_command(
                            if running {
                                "停止内核"
                            } else {
                                "启动内核"
                            },
                            window,
                            cx,
                        )
                    }),
                )
            })
            .child(
                ui::icon_button("reset-settings", IconName::RotateCcw, "恢复本页默认值…")
                    .on_click(cx.listener(|this, _, w, cx| this.reset(w, cx))),
            )
            .child(
                Button::new("save-settings")
                    .outline()
                    .with_size(control::STANDARD)
                    .icon(IconName::Save)
                    .label(if self.dirty { "保存" } else { "已保存" })
                    .disabled(!self.dirty || self.saving)
                    .on_click(cx.listener(|this, _, w, cx| this.save(w, cx))),
            );
        let mut body = v_flex().px(space::LG).pb(space::LG);
        for (title, keys) in sections {
            body = body
                .child(
                    h_flex()
                        .h(layout::NAV_ITEM_HEIGHT)
                        .pt(space::SM)
                        .child(ui::section(title, cx)),
                )
                .children(keys.into_iter().map(|key| self.field_row(key, cx)));
            if self.kind == SettingsKind::Core && title == "WebUI" {
                body = body.child(self.panels(cx));
            }
        }
        if no_results {
            body = body.child(ui::empty(
                IconName::Search,
                "未找到设置项",
                "调整关键词",
                cx,
            ));
        }
        if self.kind == SettingsKind::Resources {
            body = body.child(self.providers(cx));
        }
        v_flex()
            .size_full()
            .min_h(space::NONE)
            .child(header)
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h(space::NONE)
                    .child(self.categories(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(space::NONE)
                            .min_h(space::NONE)
                            .child(
                                div()
                                    .id(SharedString::from(format!("{}-scroll", self.kind.id())))
                                    .flex_1()
                                    .min_h(space::NONE)
                                    .overflow_y_scrollbar()
                                    .child(body),
                            )
                            .child(
                                h_flex()
                                    .h(layout::TABLE_HEADER_HEIGHT)
                                    .px(space::LG)
                                    .border_t(stroke::HAIRLINE)
                                    .border_color(cx.theme().border)
                                    .justify_between()
                                    .text_size(text::CAPTION)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{count} 项设置"))
                                    .child("保存在本地"),
                            ),
                    ),
            )
            .into_any_element()
    }
}
