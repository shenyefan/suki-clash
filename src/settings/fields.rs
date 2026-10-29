use super::{FieldControl, SettingsPage, schema::FieldKind};
use crate::ui;
use crate::ui::theme::{control, layout, space, text};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Sizable,
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
        let disabled = !super::schema::field_enabled(self.kind, key, &policy)
            || self.saving
            || self.session.read(cx).core_busy();
        let control = match &field.control {
            FieldControl::Toggle(value) => {
                let checked = *value;
                Switch::new(SharedString::from(format!("setting-{key}")))
                    .with_size(control::STANDARD)
                    .checked(checked)
                    .disabled(disabled)
                    .accessibility_label(spec.label)
                    .on_change(cx.listener(move |this, value, window, cx| {
                        this.toggle(key, *value, window, cx)
                    }))
                    .into_any_element()
            }
            FieldControl::Text(state) if key != "NewWebUiName" => div()
                .flex_1()
                .min_w(space::NONE)
                .child(
                    Input::new(state)
                        .w_full()
                        .with_size(control::STANDARD)
                        .when(spec.kind == FieldKind::Secret, |input| input.mask_toggle())
                        .disabled(disabled)
                        .aria_label(spec.label)
                        .when(spec.actions.contains(&"随机端口"), |input| {
                            input.suffix(
                                ui::icon_button(
                                    SharedString::from(format!("action-{key}-随机端口")),
                                    IconName::Shuffle,
                                    "随机端口",
                                )
                                .disabled(disabled)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.field_action(key, "随机端口", window, cx);
                                    },
                                )),
                            )
                        }),
                )
                .into_any_element(),
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
                    "运行状态" => self.session.read(cx).core().label(),
                    "当前版本" => "—",
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
            .filter(|action| **action != "随机端口")
            .map(|action| {
                let action = *action;
                let busy = self.saving || self.session.read(cx).core_busy();
                let icon = if action.contains("复制") {
                    IconName::Copy
                } else if action == "网卡信息" {
                    IconName::Network
                } else if action.contains("随机") {
                    IconName::Shuffle
                } else if action.contains("打开") {
                    IconName::FolderOpen
                } else if action.contains("生成") {
                    IconName::KeyRound
                } else if action.contains("添加") {
                    IconName::Plus
                } else if action.contains("选择") {
                    IconName::FolderOpen
                } else if action.contains("创建备份") {
                    IconName::Archive
                } else {
                    IconName::RefreshCw
                };
                if spec.kind == FieldKind::ReadOnly
                    && !(self.kind == super::SettingsKind::Core
                        && matches!(action, "打开内核目录" | "打开配置目录" | "校验配置"))
                {
                    return Button::new(SharedString::from(format!("action-{key}-{action}")))
                        .with_size(control::STANDARD)
                        .label(action)
                        .icon(icon)
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, w, cx| {
                            this.field_action(key, action, w, cx)
                        }));
                }
                ui::icon_button(
                    SharedString::from(format!("action-{key}-{action}")),
                    icon,
                    action,
                )
                .disabled(busy || disabled)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.field_action(key, action, window, cx)
                }))
            })
            .collect::<Vec<_>>();
        if spec.kind == FieldKind::Multiline {
            let row =
                v_flex()
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
                                        h_flex()
                                            .gap(space::SM)
                                            .child(
                                                ui::icon_button(
                                                    SharedString::from(format!("reset-{key}")),
                                                    IconName::RotateCcw,
                                                    "恢复默认",
                                                )
                                                .disabled(disabled)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.restore_default_text(key, window, cx);
                                                    },
                                                )),
                                            )
                                            .child(
                                                ui::icon_button(
                                                    SharedString::from(format!("expand-{key}")),
                                                    IconName::Maximize,
                                                    "展开编辑",
                                                )
                                                .on_click(move |_, w, cx| {
                                                    ui::editor::expand(
                                                        state.clone(),
                                                        title.clone(),
                                                        disabled,
                                                        w,
                                                        cx,
                                                    )
                                                }),
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
            return row.px(space::LG).into_any_element();
        }
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
        ui::preference_row(spec.label, spec.description, control, cx).into_any_element()
    }
}
