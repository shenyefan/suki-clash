use super::{FieldControl, SettingsPage, schema::FieldKind};
use crate::ui;
use crate::ui::theme::{control, layout, space, stroke, text};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Sizable,
        button::Button,
        h_flex,
        input::{Editor, Input},
        select::Select,
        switch::Switch,
        v_flex,
    },
    *,
};

impl SettingsPage {
    pub(super) fn field_row(&self, key: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let field = &self.fields[key];
        let spec = &field.spec;
        let policy = self.values(cx);
        let disabled = !super::schema::field_enabled(self.kind, key, &policy);
        let control = match &field.control {
            FieldControl::Toggle(value) => {
                let checked = *value;
                Switch::new(SharedString::from(format!("setting-{key}")))
                    .with_size(if self.kind.uses_grouped_layout() {
                        control::STANDARD
                    } else {
                        gpui_kit::component::Size::Small
                    })
                    .checked(checked)
                    .disabled(disabled)
                    .accessibility_label(spec.label)
                    .on_change(cx.listener(move |this, value, _, cx| this.toggle(key, *value, cx)))
                    .into_any_element()
            }
            FieldControl::Text(state) if key != "NewWebUiName" => div()
                .flex_1()
                .min_w(space::NONE)
                .child(
                    Input::new(state)
                        .w_full()
                        .with_size(control::STANDARD)
                        .disabled(disabled)
                        .aria_label(spec.label),
                )
                .into_any_element(),
            FieldControl::Multiline(_)
                if key == "BypassList"
                    && policy
                        .get("UseDefaultBypass")
                        .is_some_and(|value| value == "true") =>
            {
                ui::muted(super::schema::default_bypass(), cx)
                    .text_size(text::CAPTION)
                    .into_any_element()
            }
            FieldControl::Multiline(state) => Editor::new(state)
                .w_full()
                .h(layout::TEXTAREA_HEIGHT)
                .disabled(disabled)
                .aria_label(spec.label)
                .into_any_element(),
            FieldControl::Select(state) => Select::new(state)
                .w_full()
                .with_size(control::STANDARD)
                .disabled(disabled)
                .accessibility_label(spec.label)
                .into_any_element(),
            FieldControl::ReadOnly => {
                let value = match spec.label {
                    "运行状态" => {
                        if self.session.read(cx).is_running() {
                            "运行中"
                        } else {
                            "已停止"
                        }
                    }
                    "当前版本" => "—",
                    "实际代理状态" => "—",
                    "服务器地址" | "PAC 地址" => "—",
                    "应用版本" => env!("CARGO_PKG_VERSION"),
                    _ => "",
                };
                ui::muted(value, cx).into_any_element()
            }
            _ => div().into_any_element(),
        };
        let actions = spec
            .actions
            .iter()
            .map(|action| {
                let action = *action;
                let icon = if action.contains("随机") {
                    IconName::Shuffle
                } else if action.contains("打开") {
                    IconName::FolderOpen
                } else if action.contains("生成") {
                    IconName::KeyRound
                } else if action.contains("添加") {
                    IconName::Plus
                } else if action.contains("选择") {
                    IconName::Ellipsis
                } else if action.contains("创建备份") {
                    IconName::Archive
                } else {
                    IconName::RefreshCw
                };
                if spec.kind == FieldKind::ReadOnly && self.kind.uses_grouped_layout() {
                    return Button::new(SharedString::from(format!("action-{key}-{action}")))
                        .with_size(control::STANDARD)
                        .label(action)
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.field_action(key, action, w, cx)
                        }));
                }
                ui::icon_button(
                    SharedString::from(format!("action-{key}-{action}")),
                    icon,
                    action,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.field_action(key, action, window, cx)
                }))
            })
            .collect::<Vec<_>>();
        if spec.kind == FieldKind::Multiline {
            let row = v_flex()
                .w_full()
                .min_w(space::NONE)
                .gap(space::SM)
                .py(space::MD)
                .child(
                    v_flex()
                        .gap(space::XS)
                        .child(h_flex().justify_between().child(spec.label).when_some(
                            match &field.control {
                                FieldControl::Multiline(state) => Some(state.clone()),
                                _ => None,
                            },
                            |row, state| {
                                let title = SharedString::from(spec.label);
                                row.child(
                                    ui::icon_button(
                                        SharedString::from(format!("expand-{key}")),
                                        IconName::Maximize,
                                        "展开编辑…",
                                    )
                                    .on_click(
                                        move |_, w, cx| {
                                            ui::editor::expand(
                                                state.clone(),
                                                title.clone(),
                                                disabled,
                                                w,
                                                cx,
                                            )
                                        },
                                    ),
                                )
                            },
                        ))
                        .child(ui::muted(spec.description, cx).text_size(text::CAPTION)),
                )
                .child(
                    h_flex()
                        .w_full()
                        .min_w(space::NONE)
                        .items_start()
                        .gap(space::SM)
                        .child(div().flex_1().min_w(space::NONE).child(control))
                        .children(actions),
                );
            return if self.kind.uses_grouped_layout() {
                row.px(space::LG).into_any_element()
            } else {
                row.border_b(stroke::HAIRLINE)
                    .border_color(cx.theme().border)
                    .into_any_element()
            };
        }
        if self.kind.uses_grouped_layout() {
            let control = if matches!(
                field.control,
                FieldControl::Toggle(_) | FieldControl::ReadOnly
            ) {
                h_flex()
                    .gap(space::SM)
                    .justify_end()
                    .child(control)
                    .children(actions)
            } else {
                h_flex()
                    .gap(space::SM)
                    .justify_end()
                    .child(div().w(layout::LONG_FIELD_WIDTH).child(control))
                    .children(actions)
            };
            return ui::preference_row(spec.label, spec.description, control, cx)
                .into_any_element();
        }
        let label = div()
            .w(layout::LABEL_WIDTH)
            .flex_shrink_0()
            .child(spec.label);
        h_flex()
            .min_h(layout::RANKING_ROW_HEIGHT)
            .gap(space::MD)
            .py(space::XS)
            .border_b(stroke::HAIRLINE)
            .border_color(cx.theme().border)
            .child(label)
            .child(
                h_flex()
                    .w(layout::LONG_FIELD_WIDTH)
                    .flex_shrink_0()
                    .gap(space::XS)
                    .child(control)
                    .children(actions),
            )
            .child(
                ui::muted(spec.description, cx)
                    .text_size(text::CAPTION)
                    .flex_1()
                    .min_w(space::NONE),
            )
            .into_any_element()
    }
}
