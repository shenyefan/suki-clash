use super::{FieldControl, SettingsPage};
use crate::{
    ui,
    ui::theme::{control, layout, space},
};
use gpui_kit::{
    assets::IconName,
    component::{Disableable, Sizable, h_flex, select::Select, v_flex},
    prelude::FluentBuilder as _,
    *,
};

impl SettingsPage {
    pub(super) fn sync_core_versions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.core_versions.is_empty() {
            return;
        }
        let Some(super::SettingField {
            control: FieldControl::Select(state),
            ..
        }) = self.fields.get("CoreSpecificVersion")
        else {
            return;
        };
        let previous = state
            .read(cx)
            .selected_value()
            .cloned()
            .filter(|value| !value.is_empty());
        let mut items = self
            .core_versions
            .iter()
            .cloned()
            .map(SharedString::from)
            .collect::<Vec<_>>();
        if let Some(value) = &previous
            && !items.contains(value)
        {
            items.insert(0, value.clone());
        }
        let selected = previous.or_else(|| items.first().cloned());
        state.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            if let Some(selected) = selected {
                state.set_selected_value(&selected, window, cx);
            }
        });
        cx.notify();
    }
    pub(super) fn core_content(
        &self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut body = v_flex().gap(space::SECTION);
        let running = self.session.read(cx).is_running();
        let core = self.session.read(cx).core();
        let has_process = core.has_process();
        let busy = self.session.read(cx).core_busy();
        if self.runtime_tab == 0
            && (query.is_empty() || "mihomo smart 内核 状态 版本 发行 通道".contains(query))
        {
            let status = h_flex()
                .gap(space::SM)
                .child(ui::muted(core.label(), cx))
                .child(
                    ui::icon_button(
                        "core-power",
                        if has_process {
                            IconName::Square
                        } else {
                            IconName::Play
                        },
                        if has_process {
                            "停止内核"
                        } else {
                            "启动内核"
                        },
                    )
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.runtime_command(
                            if has_process {
                                "停止内核"
                            } else {
                                "启动内核"
                            },
                            w,
                            cx,
                        )
                    })),
                )
                .child(
                    ui::icon_button("core-restart", IconName::RefreshCw, "重启内核")
                        .disabled(!running || busy)
                        .on_click(
                            cx.listener(|this, _, w, cx| this.runtime_command("重启内核", w, cx)),
                        ),
                );
            let version = h_flex()
                .gap(space::LG)
                .child(ui::muted(core.version().unwrap_or("—"), cx))
                .child(
                    ui::icon_button("core-upgrade", IconName::RefreshCw, "检查内核更新")
                        .disabled(busy)
                        .on_click(
                            cx.listener(|this, _, w, cx| this.runtime_command("升级内核", w, cx)),
                        ),
                );
            let channel = h_flex()
                .gap(space::SM)
                .when_some(self.fields.get("CoreReleaseIndex"), |row, field| {
                    if let FieldControl::Select(state) = &field.control {
                        row.child(
                            div().w(layout::LONG_FIELD_WIDTH).flex_shrink_0().child(
                                Select::new(state)
                                    .with_size(control::STANDARD)
                                    .w_full()
                                    .accessibility_label("发行通道")
                                    .disabled(busy || self.saving),
                            ),
                        )
                    } else {
                        row
                    }
                })
                .child(
                    ui::icon_button("core-switch", IconName::Download, "下载所选内核并重启")
                        .disabled(busy)
                        .on_click(
                            cx.listener(|this, _, w, cx| this.runtime_command("切换内核", w, cx)),
                        ),
                );
            let mut rows = vec![
                ui::preference_row("运行状态", "", status, cx).into_any_element(),
                ui::preference_row("当前版本", "", version, cx).into_any_element(),
                ui::preference_row(
                    "发行通道",
                    "最新版 / 预览版 / Smart / 指定版本",
                    channel,
                    cx,
                )
                .into_any_element(),
            ];
            let specific=self.fields.get("CoreReleaseIndex").is_some_and(|field|matches!(&field.control,FieldControl::Select(state) if state.read(cx).selected_value().is_some_and(|value|value.as_ref()=="指定版本")));
            if specific {
                rows.push(self.field_row("CoreSpecificVersion", cx));
            }
            body = body.child(ui::preferences_group("Mihomo", rows, cx));
        }
        let titles: &[&str] = match self.runtime_tab {
            0 => &["基础参数"],
            1 => &["监听端口", "隧道"],
            2 => &["访问控制"],
            _ => &["内核维护", "外部控制", "WebUI", "日志维护"],
        };
        let mut found = false;
        for section in &self.sections {
            if section.title == "状态" {
                continue;
            }
            if query.is_empty() && !titles.contains(&section.title) {
                continue;
            }
            if section.title == "隧道" {
                if query.is_empty() || "tcp udp 隧道 转发".contains(query) {
                    found = true;
                    body = body.child(ui::preferences_group(
                        "隧道",
                        self.tunnel_rows(window, cx),
                        cx,
                    ));
                }
                continue;
            }
            if section.title == "WebUI" {
                if query.is_empty() || "webui 面板".contains(query) {
                    found = true;
                    let height =
                        crate::ui::theme::list::DETAIL_ROW_HEIGHT.to_pixels(window.rem_size());
                    let mut rows = Vec::new();
                    if !self.panels.is_empty() {
                        rows.push(ui::virtual_rows::panel(
                            cx.entity(),
                            "webui-rows",
                            vec![height; self.panels.len()],
                            &self.lists["panels"],
                            move |this, index, _, cx| {
                                let (id, name, url) = &this.panels[index];
                                let id = *id;
                                let open_url = url.clone();
                                let controls = h_flex()
                                    .gap(space::SM)
                                    .child(
                                        ui::icon_button(
                                            ("open-panel", id),
                                            IconName::ExternalLink,
                                            "打开面板",
                                        )
                                        .disabled(busy || this.saving)
                                        .on_click(
                                            cx.listener(move |this, _, w, cx| {
                                                this.open_panel(&open_url, w, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        ui::icon_button(
                                            ("edit-panel", id),
                                            IconName::Pencil,
                                            "编辑面板",
                                        )
                                        .disabled(busy || this.saving)
                                        .on_click(
                                            cx.listener(move |this, _, w, cx| {
                                                this.edit_panel(Some(id), w, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        ui::icon_button(
                                            ("remove-panel", id),
                                            IconName::Trash,
                                            "删除面板",
                                        )
                                        .disabled(busy || this.saving)
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                let removed = this
                                                    .panels
                                                    .iter()
                                                    .enumerate()
                                                    .find(|(_, panel)| panel.0 == id)
                                                    .map(|(index, panel)| (index, panel.clone()));
                                                this.panels.retain(|panel| panel.0 != id);
                                                this.commit_panels(window, cx);
                                                if let Some((index, panel)) = removed {
                                                    let owner = cx.weak_entity();
                                                    ui::notifications::undo(
                                                        window,
                                                        "已删除面板",
                                                        cx,
                                                        move |window, cx| {
                                                            owner
                                                                .update(cx, |this, cx| {
                                                                    if this.saving
                                                                        || this
                                                                            .session
                                                                            .read(cx)
                                                                            .core_busy()
                                                                    {
                                                                        ui::show_error(
                                                                        "正在应用配置，请稍后撤销"
                                                                            .into(),
                                                                        window,
                                                                        cx,
                                                                    );
                                                                        return false;
                                                                    }
                                                                    if !this.panels.iter().any(
                                                                        |item| item.0 == panel.0,
                                                                    ) {
                                                                        this.panels.insert(
                                                                            index.min(
                                                                                this.panels.len(),
                                                                            ),
                                                                            panel.clone(),
                                                                        );
                                                                        this.commit_panels(
                                                                            window, cx,
                                                                        );
                                                                    }
                                                                    true
                                                                })
                                                                .unwrap_or(true)
                                                        },
                                                    );
                                                }
                                                cx.notify();
                                            }),
                                        ),
                                    );
                                ui::data_preference_row(
                                    name.clone(),
                                    url.split('/').nth(2).unwrap_or(url).to_owned(),
                                    controls,
                                    cx,
                                )
                                .into_any_element()
                            },
                        ));
                    }
                    rows.push(
                        ui::preference_row(
                            "面板",
                            "使用外部控制器访问内核",
                            h_flex().gap(space::SM).child(
                                ui::icon_button("add-runtime-panel", IconName::Plus, "添加面板")
                                    .disabled(busy || self.saving)
                                    .on_click(cx.listener(|this, _, w, cx| this.add_panel(w, cx))),
                            ),
                            cx,
                        )
                        .into_any_element(),
                    );
                    body = body.child(ui::preferences_group("WebUI", rows, cx));
                }
                continue;
            }
            let keys: Vec<_> = section
                .fields
                .iter()
                .filter(|field| {
                    !matches!(
                        field.key,
                        "CoreAction0" | "CoreAction1" | "CoreSpecificVersion" | "CoreReleaseIndex"
                    ) && (query.is_empty()
                        || format!("{} {} {}", section.title, field.label, field.description)
                            .to_lowercase()
                            .contains(query))
                })
                .map(|field| field.key)
                .collect();
            if !keys.is_empty() {
                found = true;
                let rows = keys
                    .into_iter()
                    .map(|key| self.field_row(key, cx))
                    .collect();
                body = body.child(ui::preferences_group(
                    if section.title == "状态" {
                        "文件与目录"
                    } else {
                        section.title
                    },
                    rows,
                    cx,
                ));
            }
        }
        if !query.is_empty() && !found {
            return ui::empty(IconName::Search, "未找到设置项", "尝试其他关键词", cx);
        }
        body
    }
}
