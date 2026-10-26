use super::{FieldControl, SettingsPage};
use crate::{
    ui,
    ui::theme::{control, layout, space},
};
use gpui_kit::{
    assets::IconName,
    component::{Disableable, Sizable, button::Button, h_flex, select::Select, v_flex},
    prelude::FluentBuilder as _,
    *,
};

impl SettingsPage {
    pub(super) fn core_content(&self, query: &str, cx: &mut Context<Self>) -> Div {
        let mut body = v_flex().gap(space::SECTION);
        let running = self.session.read(cx).is_running();
        if self.runtime_tab == 0 && query.is_empty() {
            let status = h_flex()
                .gap(space::SM)
                .child(ui::muted(if running { "运行中" } else { "已停止" }, cx))
                .child(
                    Button::new("core-power")
                        .with_size(control::STANDARD)
                        .label(if running { "停止" } else { "启动" })
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.runtime_command(
                                if running {
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
                    Button::new("core-restart")
                        .with_size(control::STANDARD)
                        .label("重启")
                        .disabled(!running)
                        .on_click(
                            cx.listener(|this, _, w, cx| this.runtime_command("重启内核", w, cx)),
                        ),
                );
            let version = h_flex().gap(space::LG).child(ui::muted("—", cx)).child(
                Button::new("core-upgrade")
                    .with_size(control::STANDARD)
                    .label("检查更新")
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
                                    .accessibility_label("发行通道"),
                            ),
                        )
                    } else {
                        row
                    }
                })
                .child(
                    Button::new("core-switch")
                        .with_size(control::STANDARD)
                        .label("切换并重启")
                        .on_click(
                            cx.listener(|this, _, w, cx| this.runtime_command("切换内核", w, cx)),
                        ),
                );
            body = body.child(ui::preferences_group(
                "Mihomo",
                vec![
                    ui::preference_row("运行状态", "", status, cx).into_any_element(),
                    ui::preference_row("当前版本", "", version, cx).into_any_element(),
                    ui::preference_row(
                        "发行通道",
                        "稳定版适合日常使用，预览版包含最新改动",
                        channel,
                        cx,
                    )
                    .into_any_element(),
                ],
                cx,
            ));
        }
        let titles: &[&str] = match self.runtime_tab {
            0 => &["基础参数"],
            1 => &["监听端口", "外部控制"],
            2 => &["访问控制"],
            _ => &["状态", "内核维护", "WebUI", "日志维护"],
        };
        let mut found = false;
        for section in &self.sections {
            if query.is_empty() && !titles.contains(&section.title) {
                continue;
            }
            if section.title == "WebUI" {
                if query.is_empty() || "webui 面板".contains(query) {
                    found = true;
                    let mut rows: Vec<AnyElement> = self
                        .panels
                        .iter()
                        .map(|(id, name, url)| {
                            let id = *id;
                            let open_url = url.clone();
                            let controls = h_flex()
                                .gap(space::SM)
                                .child(
                                    Button::new(("open-panel", id))
                                        .with_size(control::STANDARD)
                                        .label("打开")
                                        .on_click(cx.listener(move |this, _, w, cx| {
                                            this.open_panel(&open_url, w, cx)
                                        })),
                                )
                                .child(
                                    ui::icon_button(
                                        ("edit-panel", id),
                                        IconName::Pencil,
                                        "编辑面板…",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, w, cx| this.edit_panel(Some(id), w, cx),
                                    )),
                                )
                                .child(
                                    ui::icon_button(
                                        ("remove-panel", id),
                                        IconName::Trash,
                                        "删除面板",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.panels.retain(|panel| panel.0 != id);
                                            this.dirty = true;
                                            cx.notify();
                                        },
                                    )),
                                );
                            ui::preference_row(
                                name.clone(),
                                url.split('/').nth(2).unwrap_or(url).to_owned(),
                                controls,
                                cx,
                            )
                            .into_any_element()
                        })
                        .collect();
                    rows.push(
                        ui::preference_row(
                            "面板",
                            "使用外部控制器访问内核",
                            h_flex()
                                .gap(space::SM)
                                .child(
                                    Button::new("add-runtime-panel")
                                        .with_size(control::STANDARD)
                                        .label("添加…")
                                        .on_click(
                                            cx.listener(|this, _, w, cx| this.add_panel(w, cx)),
                                        ),
                                )
                                .child(
                                    Button::new("restore-runtime-panels")
                                        .with_size(control::STANDARD)
                                        .label("恢复默认")
                                        .on_click(cx.listener(|this, _, w, cx| {
                                            this.action("恢复默认 WebUI", w, cx)
                                        })),
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
