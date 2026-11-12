use super::{FieldControl, SettingsPage, schema::FieldKind};
use crate::ui;
use crate::ui::theme::{control, layout, space, text};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Sizable,
        button::{Button, ButtonVariants},
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
            || (key == "IsTunEnabled"
                && self.session.read(cx).core().phase() == crate::core::Phase::Failed)
            || self.saving
            || self.session.read(cx).core_busy();
        let control = match &field.control {
            FieldControl::Toggle(value) => {
                let checked = if key == "AutoRun" {
                    self.session
                        .read(cx)
                        .core()
                        .autostart
                        .as_ref()
                        .and_then(|result| result.as_ref().ok())
                        .copied()
                        .unwrap_or(false)
                } else if key == "IsTunEnabled" {
                    self.session.read(cx).tun() == Some(true)
                } else {
                    *value
                };
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
                        .when(spec.actions.contains(&"生成密钥"), |input| {
                            input.suffix(
                                ui::icon_button(
                                    SharedString::from(format!("action-{key}-生成密钥")),
                                    IconName::KeyRound,
                                    "生成密钥",
                                )
                                .disabled(disabled)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.field_action(key, "生成密钥", window, cx)
                                    },
                                )),
                            )
                        })
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
                    "后台服务" => self.session.read(cx).core().service_state().label(),
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
            .filter(|action| !matches!(**action, "随机端口" | "生成密钥"))
            .map(|action| {
                let action = *action;
                let busy = self.saving || self.session.read(cx).core_busy();
                let icon = match action {
                    "配置 Gist 备份" => Some(IconName::Settings2),
                    "创建 Gist 备份" | "创建本地备份" => Some(IconName::Archive),
                    "Gist 备份历史" | "本地备份历史" => Some(IconName::Clock),
                    "复制环境变量" | "复制地址" | "复制密钥" => Some(IconName::Copy),
                    "网卡信息" => Some(IconName::Network),
                    "随机端口" => Some(IconName::Shuffle),
                    "打开配置目录" | "打开内核目录" | "打开日志目录" | "打开备份目录" => {
                        Some(IconName::FolderOpen)
                    }
                    "生成密钥" => Some(IconName::KeyRound),
                    "安装服务" => Some(IconName::Download),
                    "修复服务" => Some(IconName::Wrench),
                    "卸载服务" => Some(IconName::Trash),
                    "应用所选通道" => Some(IconName::Check),
                    "校验配置" => Some(IconName::FileCheck),
                    "清空 Fake-IP 缓存" => Some(IconName::Trash),
                    "管理隧道" => Some(IconName::Network),
                    "退出应用" => Some(IconName::LogOut),
                    "选择应用" => Some(IconName::AppWindow),
                    "重建防火墙规则" => Some(IconName::Shield),
                    "重置程序" => Some(IconName::RotateCcw),
                    "添加 WebUI" => Some(IconName::Plus),
                    "导出诊断信息" => Some(IconName::FileOutput),
                    "查看运行配置" => Some(IconName::FileCode),
                    "检查应用更新" | "刷新版本列表" | "更新 GeoIP" | "更新 GeoSite"
                    | "更新 MMDB" | "更新 ASN" => Some(IconName::RefreshCw),
                    _ => None,
                };
                let label = match action {
                    "配置 Gist 备份" => "Gist 配置",
                    "复制环境变量" => "复制",
                    "刷新版本列表" => "刷新版本",
                    "应用所选通道" => "应用通道",
                    "重建防火墙规则" => "重建规则",
                    "清空 Fake-IP 缓存" => "清空缓存",
                    "创建 Gist 备份" | "创建本地备份" => "创建备份",
                    "本地备份历史" | "Gist 备份历史" => "备份历史",
                    "打开配置目录" => "配置目录",
                    "打开内核目录" => "内核目录",
                    "打开日志目录" => "日志目录",
                    "打开备份目录" => "备份目录",
                    "检查应用更新" => "检查更新",
                    "导出诊断信息" => "导出诊断",
                    "查看运行配置" => "运行配置",
                    _ => action,
                };
                Button::new(SharedString::from(format!("action-{key}-{action}")))
                    .outline()
                    .with_size(control::STANDARD)
                    .when(
                        spec.kind == FieldKind::ReadOnly || icon.is_none(),
                        |button| button.label(label),
                    )
                    .when(
                        spec.kind != FieldKind::ReadOnly && icon.is_some(),
                        |button| button.ghost().accessibility_label(label).tooltip(label),
                    )
                    .when_some(icon, |button, icon| button.icon(icon))
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
                                                .disabled(disabled)
                                                .on_click(move |_, w, cx| {
                                                    ui::editor::expand(
                                                        state.clone(),
                                                        title.clone(),
                                                        false,
                                                        None,
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
        if key == "TunService" {
            let state = self.session.read(cx).core().service_state();
            let ready = matches!(state, crate::core::ServiceState::Ready);
            let checking = matches!(state, crate::core::ServiceState::Checking);
            let actions: &[&str] =
                if ready || matches!(state, crate::core::ServiceState::Rejected(_)) {
                    &["修复服务", "卸载服务"]
                } else {
                    &["安装服务", "修复服务"]
                };
            let buttons = h_flex()
                .gap(space::SM)
                .children(actions.iter().map(|action| {
                    let action = *action;
                    Button::new(SharedString::from(format!("service-{action}")))
                        .outline()
                        .label(action)
                        .icon(match action {
                            "安装服务" => IconName::Download,
                            "修复服务" => IconName::Wrench,
                            _ => IconName::Trash,
                        })
                        .with_size(control::STANDARD)
                        .disabled(disabled || checking)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.field_action(key, action, window, cx)
                        }))
                }));
            return ui::preference_row(spec.label, spec.description, buttons, cx)
                .into_any_element();
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
