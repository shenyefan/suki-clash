use crate::ui::theme::control;
pub(crate) use actions::copy_environment;
mod actions;
mod core;
mod core_apply;
mod core_tools;
mod dns;
mod fields;
mod gist;
mod local_backup;
mod preferences;
mod resources;
mod runtime;
mod sysproxy;
mod update;
mod uwp;
use crate::preferences::schema;
mod view;
use crate::integration as store;

use crate::{session::Session, ui};
use gpui_kit::{
    assets::IconName,
    component::{
        IndexPath, Sizable, Theme, ThemeMode, WindowExt,
        button::{Button, ButtonVariants},
        input::{EditorState, Input, InputEvent, InputState, MaskPattern},
        select::SelectState,
    },
    *,
};
use std::collections::BTreeMap;

use schema::{FieldKind, FieldSpec, SectionSpec, validate};

enum FieldControl {
    Toggle(bool),
    Text(Entity<InputState>),
    Multiline(Entity<EditorState>),
    Select(Entity<SelectState<Vec<SharedString>>>),
    ReadOnly,
}
struct SettingField {
    spec: FieldSpec,
    control: FieldControl,
}

pub(crate) use crate::preferences::SettingsKind;
impl SettingsKind {
    pub(crate) fn icon(self) -> IconName {
        match self {
            Self::Sniffer => IconName::Radar,
            Self::Dns => IconName::Globe,
            Self::Core => IconName::Cpu,
            Self::Resources => IconName::FolderDown,
            Self::SysProxy => IconName::Earth,
            Self::Tun => IconName::Cable,
            Self::General => IconName::Settings2,
        }
    }
}

pub(crate) struct SettingsSaved {
    backdrop: String,
}
impl SettingsSaved {
    pub(crate) fn backdrop(&self) -> &str {
        &self.backdrop
    }
}
impl EventEmitter<SettingsSaved> for SettingsPage {}

pub(crate) struct SettingsPage {
    kind: SettingsKind,
    session: Entity<Session>,
    sections: Vec<SectionSpec>,
    fields: BTreeMap<&'static str, SettingField>,
    filter: Entity<InputState>,
    runtime_tab: usize,
    dirty: bool,
    synced_values: crate::storage::Values,
    saving: bool,
    pending_action: Option<Task<()>>,
    core_syncing: bool,
    core_pending: Option<(u64, BTreeMap<String, String>, AnyWindowHandle)>,
    core_versions: Vec<String>,
    resource_generation: Option<u64>,
    dns_raw: bool,
    dns_source: Entity<EditorState>,
    dns_document: serde_yaml_ng::Value,
    error: Option<String>,
    lists: BTreeMap<&'static str, gpui_kit::component::VirtualListScrollHandle>,
    panels: Vec<(u64, String, String)>,
    next_panel_id: u64,
    _subscriptions: Vec<Subscription>,
}
impl SettingsPage {
    pub(crate) fn new(
        kind: SettingsKind,
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sections = schema::sections(kind);
        let saved = store::read(kind.id(), cx);
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("搜索设置项"));
        let settings_window = window.window_handle();
        let mut subscriptions = vec![
            cx.observe(&session, move |this, session, cx| {
                let observed_core = session.read(cx).core();
                let observed_busy = session.read(cx).core_busy();
                if this.kind == SettingsKind::SysProxy
                    && !observed_busy
                    && let Some(enabled) = observed_core.system_proxy().enabled()
                    && let Some(field) = this.fields.get_mut("IsSystemProxyEnabled")
                {
                    field.control = FieldControl::Toggle(enabled);
                }
                let refresh_resources = this.kind == SettingsKind::Resources
                    && observed_core.phase() == crate::core::Phase::Ready
                    && !observed_busy
                    && this.resource_generation != Some(observed_core.generation());
                if let Some((id, _, _)) = &this.core_pending
                    && observed_core.request_id() == Some(*id)
                    && !observed_busy
                {
                    let Some((_, submitted, window)) = this.core_pending.take() else {
                        return;
                    };
                    this.saving = false;
                    this.error = observed_core.error().map(str::to_owned);
                    let owner = cx.weak_entity();
                    cx.defer(move |cx| {
                        let _ = window.update(cx, |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.restore_core_fields(
                                    &submitted,
                                    this.error.is_some(),
                                    window,
                                    cx,
                                );
                                if this.error.is_none()
                                    && submitted
                                        .get("CoreReleaseIndex")
                                        .is_some_and(|v| v == "指定版本")
                                {
                                    this.runtime_command("加载版本列表", window, cx);
                                }
                            });
                        });
                    });
                }
                if !observed_busy
                    && !this.saving
                    && !this.dirty
                    && this.error.is_none()
                    && store::read(this.kind.id(), cx) != this.synced_values
                {
                    let owner = cx.weak_entity();
                    cx.defer(move |cx| {
                        let _ = settings_window.update(cx, |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if !this.saving && !this.dirty && this.error.is_none() {
                                    this.sync_committed_settings(window, cx);
                                }
                            });
                        });
                    });
                }
                if refresh_resources {
                    let owner = cx.weak_entity();
                    cx.defer(move |cx| {
                        let _ = settings_window.update(cx, |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| this.refresh_resources(window, cx));
                        });
                    });
                }
                cx.notify();
            }),
            cx.subscribe(&filter, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    for handle in this.lists.values() {
                        handle.set_offset(Point::default());
                    }
                }
                cx.notify()
            }),
        ];
        subscriptions.push(cx.observe(&session, move |this, session, cx| {
            let versions = session.read(cx).core().versions();
            if this.kind == SettingsKind::Core && this.core_versions != versions {
                this.core_versions = versions.to_vec();
                let owner = cx.weak_entity();
                cx.defer(move |cx| {
                    let _ = settings_window.update(cx, |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| this.sync_core_versions(window, cx));
                    });
                });
            }
        }));
        let dns_source = cx.new(|cx| ui::editor::state("yaml", "", true, window, cx));
        subscriptions.push(cx.subscribe_in(
            &dns_source,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if this.core_syncing || !this.dns_raw {
                    return;
                }
                if matches!(event, InputEvent::Change) {
                    this.dirty = true;
                    cx.notify();
                }
                if matches!(event, InputEvent::Blur) {
                    this.commit_core(this.values(cx), window, cx);
                }
            },
        ));
        let dns_document = saved
            .get(if kind == SettingsKind::Sniffer {
                "SnifferYaml"
            } else {
                "DnsYaml"
            })
            .and_then(|source| serde_yaml_ng::from_str(source).ok())
            .unwrap_or_default();
        let mut fields = BTreeMap::new();
        for spec in sections.iter().flat_map(|s| s.fields.iter()) {
            let value = saved
                .get(spec.key)
                .map(String::as_str)
                .unwrap_or(spec.default);
            let control = match spec.kind {
                FieldKind::Toggle => FieldControl::Toggle(value == "true"),
                FieldKind::Text | FieldKind::Secret | FieldKind::Number => {
                    let numeric = spec.kind == FieldKind::Number && !spec.key.ends_with("Port");
                    let state = cx.new(|cx| {
                        let state = InputState::new(window, cx)
                            .default_value(value.to_owned())
                            .placeholder(
                                if kind == SettingsKind::Resources && spec.kind == FieldKind::Text {
                                    "默认地址"
                                } else {
                                    ""
                                },
                            )
                            .masked(spec.kind == FieldKind::Secret);
                        if numeric {
                            state.mask_pattern(MaskPattern::number(None))
                        } else {
                            state
                        }
                    });
                    let key = spec.key;
                    subscriptions.push(cx.subscribe_in(
                        &state,
                        window,
                        move |this, _, event: &InputEvent, window, cx| {
                            if this.core_syncing {
                                return;
                            }
                            if matches!(event, InputEvent::Blur) {
                                this.commit_core_field(key, window, cx);
                            }
                            if matches!(event, InputEvent::Change) {
                                this.dirty = true;
                                this.error = None;
                                cx.notify();
                            }
                        },
                    ));
                    FieldControl::Text(state)
                }
                FieldKind::Multiline => {
                    let state = cx.new(|cx| {
                        ui::editor::state(
                            if spec.key == "PacScript" {
                                "javascript"
                            } else {
                                "text"
                            },
                            value.to_owned(),
                            spec.key == "PacScript",
                            window,
                            cx,
                        )
                    });
                    let key = spec.key;
                    subscriptions.push(cx.subscribe_in(
                        &state,
                        window,
                        move |this, _, event: &InputEvent, window, cx| {
                            if this.core_syncing {
                                return;
                            }
                            if matches!(event, InputEvent::Blur) {
                                this.commit_core_field(key, window, cx);
                            }
                            if matches!(event, InputEvent::Change) {
                                this.dirty = true;
                                this.error = None;
                                cx.notify();
                            }
                        },
                    ));
                    FieldControl::Multiline(state)
                }
                FieldKind::Select => {
                    let items = if spec.key == "CoreSpecificVersion" && !value.is_empty() {
                        vec![SharedString::from(value.to_owned())]
                    } else {
                        spec.options
                            .iter()
                            .map(|v| SharedString::from(*v))
                            .collect::<Vec<_>>()
                    };
                    let index = items.iter().position(|v| v.as_ref() == value).unwrap_or(0);
                    let state = cx
                        .new(|cx| SelectState::new(items, Some(IndexPath::new(index)), window, cx));
                    let key = spec.key;
                    subscriptions.push(cx.subscribe_in(
                        &state,
                        window,
                        move |this,
                              state,
                              event: &gpui_kit::component::select::SelectEvent<
                            Vec<SharedString>,
                        >,
                              window,
                              cx| {
                            if matches!(event, gpui_kit::component::select::SelectEvent::Confirm(_))
                            {
                                if this.core_syncing {
                                    return;
                                }
                                this.dirty = true;
                                {
                                    this.commit_core_field(key, window, cx);
                                }
                                if key == "ThemeIndex" {
                                    let value = state.read(cx).selected_value().cloned();
                                    match value.as_deref() {
                                        Some("深色") => {
                                            Theme::change(ThemeMode::Dark, Some(window), cx)
                                        }
                                        Some("浅色") => {
                                            Theme::change(ThemeMode::Light, Some(window), cx)
                                        }
                                        _ => Theme::sync_system_appearance(Some(window), cx),
                                    }
                                    ui::configure_theme(cx);
                                }
                                if key == "NoticePosition" {
                                    ui::configure_theme(cx);
                                }
                                cx.notify();
                            }
                        },
                    ));
                    FieldControl::Select(state)
                }
                FieldKind::ReadOnly => FieldControl::ReadOnly,
            };
            fields.insert(
                spec.key,
                SettingField {
                    spec: spec.clone(),
                    control,
                },
            );
        }
        let panels = schema::panels_from_values(&saved);
        let next_panel_id = panels.iter().map(|panel| panel.0).max().unwrap_or(0) + 1;
        Self {
            lists: ["panels", "tunnels", "providers"]
                .into_iter()
                .map(|name| (name, gpui_kit::component::VirtualListScrollHandle::new()))
                .collect(),
            kind,
            session,
            sections,
            fields,
            filter,
            runtime_tab: 0,
            dirty: false,
            synced_values: saved.clone(),
            saving: false,
            pending_action: None,
            core_syncing: false,
            core_pending: None,
            core_versions: Vec::new(),
            resource_generation: None,
            dns_raw: false,
            dns_source,
            dns_document,
            error: None,
            panels,
            next_panel_id,
            _subscriptions: subscriptions,
        }
    }

    fn values(&self, cx: &App) -> BTreeMap<String, String> {
        self.fields
            .iter()
            .filter_map(|(key, field)| {
                if field.spec.kind == FieldKind::Secret && self.kind != SettingsKind::Core {
                    return None;
                }
                let value = match &field.control {
                    FieldControl::Toggle(value) => value.to_string(),
                    FieldControl::Text(state) => state.read(cx).value().to_string(),
                    FieldControl::Multiline(state) => state.read(cx).value().to_string(),
                    FieldControl::Select(state) => state
                        .read(cx)
                        .selected_value()
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    FieldControl::ReadOnly => return None,
                };
                Some(((*key).to_owned(), value))
            })
            .collect()
    }
    fn command_values(&self, cx: &App) -> BTreeMap<String, String> {
        let mut values = self.values(cx);
        for (key, field) in &self.fields {
            if field.spec.kind == FieldKind::Secret
                && let FieldControl::Text(state) = &field.control
            {
                values.insert((*key).into(), state.read(cx).value().to_string());
            }
        }
        values
    }
    fn open_panel(&self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !url.contains("%host") && !url.contains("%port") && !url.contains("%secret") {
            cx.open_url(url);
            return;
        }
        if !self.session.read(cx).is_running() {
            ui::show_error(
                "内核未运行，暂时无法获取 WebUI 连接信息。".into(),
                window,
                cx,
            );
            return;
        }
        let values = store::read("core", cx);
        if values.get("EnableExternalController").map(String::as_str) != Some("true") {
            ui::show_error("请先启用外部控制，再打开 WebUI".into(), window, cx);
            return;
        }
        match schema::expand_panel_url(
            url,
            values
                .get("ExternalController")
                .map(String::as_str)
                .unwrap_or(""),
            values.get("Secret").map(String::as_str).unwrap_or(""),
        ) {
            Ok(url) => cx.open_url(&url),
            Err(error) => ui::show_error(error.to_string(), window, cx),
        }
    }
    fn assign_values(
        &mut self,
        values: &BTreeMap<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (key, field) in &mut self.fields {
            let value = values
                .get(*key)
                .map(String::as_str)
                .unwrap_or(field.spec.default);
            match &mut field.control {
                FieldControl::Toggle(checked) => *checked = value == "true",
                FieldControl::Text(state) => {
                    state.update(cx, |s, cx| s.set_value(value.to_owned(), window, cx))
                }
                FieldControl::Multiline(state) => {
                    state.update(cx, |s, cx| s.set_value(value.to_owned(), window, cx))
                }
                FieldControl::Select(state) => {
                    let index = field
                        .spec
                        .options
                        .iter()
                        .position(|v| *v == value)
                        .unwrap_or(0);
                    state.update(cx, |s, cx| {
                        s.set_selected_index(Some(IndexPath::new(index)), window, cx)
                    });
                }
                FieldControl::ReadOnly => {}
            }
        }
        if self.kind == SettingsKind::Core {
            self.panels = schema::panels_from_values(values);
            self.next_panel_id = self.panels.iter().map(|panel| panel.0).max().unwrap_or(0) + 1;
        }
        self.dirty = true;
        self.error = None;
        cx.notify();
    }
    pub(crate) fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind == SettingsKind::General {
            crate::core::refresh_autostart(cx);
        }
        if self.kind == SettingsKind::Resources {
            self.refresh_resources(window, cx);
        }
        if self.saving {
            return;
        }
        self.error = None;
        self.dns_raw = false;
        self.sync_committed_settings(window, cx);
    }
    fn toggle(
        &mut self,
        key: &'static str,
        value: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let native_raw =
            self.dns_raw && matches!(self.kind, SettingsKind::Dns | SettingsKind::Sniffer);
        if native_raw && let Err(error) = self.read_dns_yaml(window, cx) {
            ui::show_error(error, window, cx);
            return;
        }
        if let Some(field) = self.fields.get_mut(key) {
            field.control = FieldControl::Toggle(value);
        }
        if native_raw && let Ok(source) = self.dns_to_yaml(cx) {
            self.core_syncing = true;
            self.dns_source
                .update(cx, |state, cx| state.set_value(source, window, cx));
            self.core_syncing = false;
        }
        if key == "AutoRoute"
            && !value
            && let Some(field) = self.fields.get_mut("AutoRedirect")
        {
            field.control = FieldControl::Toggle(false);
        }
        self.dirty = true;
        {
            self.commit_core_field(key, window, cx);
        }
        cx.notify();
    }
    fn field_action(
        &mut self,
        key: &'static str,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.local_action(action, window, cx) {
            return;
        }
        if self.kind == SettingsKind::Core && matches!(action, "复制地址" | "复制密钥") {
            let values = crate::integration::read("core", cx);
            if let Some(value) = values.get(key) {
                cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                crate::ui::notifications::push(window, "已复制", cx);
            }
            return;
        }
        if action == "网卡信息" {
            self.network_dialog(window, cx);
            return;
        }
        if action == "管理隧道" {
            self.edit_tunnel(None, window, cx);
            return;
        }
        if self.kind == SettingsKind::Core
            && matches!(
                action,
                "校验配置" | "清空 Fake-IP 缓存" | "更新 GeoData" | "刷新版本列表" | "应用所选通道"
            )
        {
            self.runtime_command(action, window, cx);
            return;
        }
        if matches!(action, "打开内核目录" | "打开配置目录" | "打开日志目录") {
            crate::integration::request(action, window, cx);
            return;
        }
        if action == "随机端口" {
            if let Some(SettingField {
                control: FieldControl::Text(state),
                ..
            }) = self.fields.get(key)
            {
                let port = 10000
                    + std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .subsec_nanos()
                        % 50000;
                state.update(cx, |state, cx| {
                    state.set_value(port.to_string(), window, cx)
                });
                self.dirty = true;
                self.commit_core_field(key, window, cx);
                cx.notify();
            }
        } else if action == "生成密钥" {
            let mut bytes = [0u8; 32];
            if let Err(error) = getrandom::fill(&mut bytes) {
                ui::show_error(format!("无法生成密钥：{error}"), window, cx);
                return;
            }
            let value: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            if let Some(SettingField {
                control: FieldControl::Text(state),
                ..
            }) = self.fields.get(key)
            {
                state.update(cx, |state, cx| state.set_value(value, window, cx));
                self.dirty = true;
                self.commit_core_field(key, window, cx);
                cx.notify();
            }
        } else if matches!(
            action,
            "重置程序" | "退出应用" | "复制环境变量" | "复制代理地址" | "复制 PAC 地址"
        ) {
            self.action(action, window, cx);
        } else {
            let values = self.command_values(cx);
            if let Some(error) = validate(&self.sections, &values) {
                ui::show_error(error, window, cx);
                return;
            }
            crate::integration::request_with(action, values, window, cx);
        }
    }
    fn action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.local_action(action, window, cx) {
            return;
        }
        match action {
            "退出应用" => crate::core::quit(cx),
            "重置程序" => {
                let owner = cx.weak_entity();
                window.open_alert_dialog(cx, move |dialog, window, cx| {
                    let owner = owner.clone();
                    ui::modal_alert(dialog, window, cx)
                        .title("重置程序？")
                        .description("删除全部设置、订阅、覆写、增强、节点选择和运行配置，随后退出程序。下次启动生成默认配置。保留内核、资源数据库、日志和备份。确认后等待 5 秒，期间可撤销。")
                        .footer(ui::dialog_footer("重置程序"))
                        .on_ok(move |_, window, cx| {
                            let _ = owner.update(cx, |this, cx| this.defer_destructive("重置程序", window, cx));
                            true
                        })
                });
            }
            "添加 WebUI" => self.add_panel(window, cx),
            _ => ui::request_command(action, window, cx),
        }
    }
    fn add_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_panel(None, window, cx);
    }
    fn edit_panel(&mut self, id: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let existing = self
            .panels
            .iter()
            .find(|panel| Some(panel.0) == id)
            .cloned();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("面板名称")
                .default_value(
                    existing
                        .as_ref()
                        .map(|panel| panel.1.clone())
                        .unwrap_or_default(),
                )
        });
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://")
                .default_value(
                    existing
                        .as_ref()
                        .map(|panel| panel.2.clone())
                        .unwrap_or_default(),
                )
        });
        let owner = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let name_save = name.clone();
            let url_save = url.clone();
            let name = name.clone();
            let url = url.clone();
            let owner = owner.clone();
            ui::modal(
                dialog,
                if id.is_some() {
                    "编辑 WebUI"
                } else {
                    "添加 WebUI"
                },
                crate::ui::theme::layout::DIALOG_WIDTH,
                window,
                cx,
            )
            .content(ui::modal_content(
                "webui-editor-body",
                crate::ui::theme::layout::DIALOG_COMPACT_HEIGHT,
                move |_, _| {
                    ui::form()
                        .child(ui::field_label(
                            "名称",
                            Input::new(&name)
                                .aria_label("面板名称")
                                .with_size(control::STANDARD)
                                .w_full(),
                        ))
                        .child(ui::field_label(
                            "地址",
                            Input::new(&url)
                                .aria_label("面板地址")
                                .with_size(control::STANDARD)
                                .w_full(),
                        ))
                        .into_any_element()
                },
            ))
            .footer(
                ui::modal_footer()
                    .child(
                        Button::new("cancel-panel")
                            .icon(gpui_kit::assets::IconName::X)
                            .with_size(control::STANDARD)
                            .label("取消")
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                    )
                    .child(
                        Button::new("save-panel")
                            .icon(gpui_kit::assets::IconName::Save)
                            .with_size(control::STANDARD)
                            .primary()
                            .label(if id.is_some() { "保存" } else { "添加" })
                            .on_click(move |_, window, cx| {
                                let name = name_save.read(cx).value().to_string();
                                let url = url_save.read(cx).value().to_string();
                                if name.trim().is_empty() || !schema::valid_web_url(&url) {
                                    ui::show_error(
                                        "填写面板名称和有效的 HTTP / HTTPS 地址".into(),
                                        window,
                                        cx,
                                    );
                                    return;
                                }
                                owner.update(cx, |this, cx| {
                                    if let Some(panel) =
                                        this.panels.iter_mut().find(|panel| Some(panel.0) == id)
                                    {
                                        panel.1 = name.trim().to_owned();
                                        panel.2 = url.trim().to_owned();
                                    } else {
                                        let id = this.next_panel_id;
                                        this.next_panel_id += 1;
                                        this.panels.push((
                                            id,
                                            name.trim().to_owned(),
                                            url.trim().to_owned(),
                                        ));
                                    }
                                    this.dirty = true;
                                    this.commit_panels(window, cx);
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            }),
                    ),
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Session, SettingsKind, SettingsPage};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, Role, TestAppContext, WindowOptions, component::WindowExt as _};
    #[gpui_kit::test]
    fn failed_core_reason_can_be_reopened_after_the_startup_toast_expires(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let service =
            crate::integration::Service::start(crate::storage::Repository::open(paths).unwrap())
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            crate::core::init_unavailable("虚拟网卡未就绪：adapter not ready".into(), cx);
            cx.set_reduce_motion(true);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let session = cx.new(Session::new);
                cx.new(|cx| SettingsPage::new(SettingsKind::Core, session, window, cx))
            })
            .unwrap()
        });
        for _ in 0..2 {
            handle
                .update(cx, |_, window, cx| {
                    window.render_frame(cx);
                    assert!(window.find("core-error-details").visible());
                    window.click("core-error-details", cx);
                    assert_eq!(crate::ui::notifications::current(window, cx).len(), 1);
                    assert!(!window.has_active_dialog(cx));
                })
                .unwrap();
            cx.executor()
                .advance_clock(std::time::Duration::from_secs(6));
            cx.run_until_parked();
        }
        crate::integration::finish_test_services(cx);
    }

    #[gpui_kit::test]
    fn resources_preserve_database_groups_switch_order_and_have_no_save_or_refresh(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::storage::Repository::open(crate::storage::AppPaths::isolated(root.path()))
                .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let session = cx.new(Session::new);
                cx.new(|cx| SettingsPage::new(SettingsKind::Resources, session, window, cx))
            })
            .unwrap()
        });
        cx.run_until_parked();
        handle
            .update(cx, |_, window, cx| {
                window.render_frame(cx);
                assert!(window.try_find("runtime-save").is_none());
                assert!(window.try_find("refresh-resources").is_none());
                assert_eq!(
                    window.find("setting-GeoAutoUpdate").role(),
                    Some(Role::Switch)
                );
                assert!(window.find("resource-update-GeoIpUrl").visible());
                assert!(!page.read(cx).values(cx)["GeoIpUrl"].is_empty());
            })
            .unwrap();
        // Enable the interval in the fixture without submitting a core command.
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| {
                    page.fields.get_mut("GeoAutoUpdate").unwrap().control =
                        super::FieldControl::Toggle(true);
                    cx.notify();
                });
                window.render_frame(cx);
                let state = match &page.read(cx).fields["GeoUpdateIntervalValue"].control {
                    super::FieldControl::Text(state) => state.clone(),
                    _ => unreachable!(),
                };
                let scope = ("input", state.entity_id());
                assert!(
                    window
                        .within(scope)
                        .try_find(("number-increment", state.entity_id()))
                        .is_none()
                );
                assert!(
                    window
                        .within(scope)
                        .try_find(("number-decrement", state.entity_id()))
                        .is_none()
                );
                window.click(scope, cx);
                window.press(
                    if cfg!(target_os = "macos") {
                        "cmd-a"
                    } else {
                        "ctrl-a"
                    },
                    cx,
                );
                window.input("25", cx);
                assert_eq!(state.read(cx).value(), "25");
            })
            .unwrap();
        cx.run_until_parked();
        handle
            .update(cx, |_, window, cx| {
                assert!(page.read(cx).dirty);
                assert!(page.read(cx).error.is_none());
                assert_eq!(page.read(cx).values(cx)["GeoUpdateIntervalValue"], "25");
                // Changing a draft does not submit until the existing blur handler runs.
                assert!(!page.read(cx).saving);
                page.update(cx, |page, cx| {
                    page.saving = true;
                    cx.notify();
                });
                window.render_frame(cx);
                let state = match &page.read(cx).fields["GeoUpdateIntervalValue"].control {
                    super::FieldControl::Text(state) => state.clone(),
                    _ => unreachable!(),
                };
                window.click(("input", state.entity_id()), cx);
                window.input("9", cx);
                assert_eq!(state.read(cx).value(), "25");
            })
            .unwrap();
        let (core_window, core_page) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let session = cx.new(Session::new);
                cx.new(|cx| {
                    let mut page = SettingsPage::new(SettingsKind::Core, session, window, cx);
                    page.runtime_tab = 1;
                    page
                })
            })
            .unwrap()
        });
        core_window
            .update(cx, |_, window, cx| {
                window.render_frame(cx);
                let state = match &core_page.read(cx).fields["MixedPort"].control {
                    super::FieldControl::Text(state) => state,
                    _ => unreachable!(),
                };
                assert!(
                    window
                        .within(("input", state.entity_id()))
                        .try_find(("number-increment", state.entity_id()))
                        .is_none()
                );
                let input_bounds = window.find(("input", state.entity_id())).bounds();
                let input = window.within(("input", state.entity_id()));
                let button = input.find(gpui_kit::SharedString::from("action-MixedPort-随机端口"));
                let bounds = button.bounds();
                assert!(bounds.size.width > gpui_kit::px(0.));
                assert!(bounds.left() >= input_bounds.left());
                assert!(bounds.right() <= input_bounds.right());
                assert!(bounds.top() >= input_bounds.top());
                assert!(bounds.bottom() <= input_bounds.bottom());
            })
            .unwrap();
        crate::integration::finish_test_services(cx);
    }
}

#[cfg(test)]
mod provider_layout_tests {
    use super::{SettingsKind, SettingsPage};
    use gpui_kit::{
        AppContext, Bounds, ScrollStrategy, TestAppContext, WindowBounds, WindowOptions, px, size,
        test::TestWindowExt,
    };
    #[gpui_kit::test]
    fn provider_pages_fill_available_height_and_keep_one_virtual_scroll_owner(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let service =
            crate::integration::Service::start(crate::storage::Repository::open(paths).unwrap())
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            cx.set_reduce_motion(true);
        });
        for (tab, kind) in [
            (1, crate::core::ProviderKind::Proxies),
            (2, crate::core::ProviderKind::Rules),
        ] {
            for count in [2, 200] {
                let entries = (0..count)
                    .map(|index| {
                        (
                            format!("item-{index:03}"),
                            serde_json::json!({"vehicleType":"HTTP","ruleCount":50}),
                        )
                    })
                    .collect::<serde_json::Map<_, _>>();
                let bytes = serde_json::to_vec(&serde_json::json!({"providers":entries})).unwrap();
                let core = crate::core::Snapshot::ready_with_providers(kind, &bytes).unwrap();
                let height = if count == 2 { 600. } else { 1000. };
                let (handle, page) = cx.update(|cx| {
                    gpui_kit::open_window(
                        WindowOptions {
                            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                                Default::default(),
                                size(px(1100.), px(height)),
                            ))),
                            ..Default::default()
                        },
                        cx,
                        |window, cx| {
                            let session = cx.new(|_| {
                                crate::session::Session::fixture(core, Default::default())
                            });
                            cx.new(|cx| {
                                let mut page =
                                    SettingsPage::new(SettingsKind::Resources, session, window, cx);
                                page.runtime_tab = tab;
                                page
                            })
                        },
                    )
                    .unwrap()
                });
                cx.run_until_parked();
                cx.update_window(handle, |_, window, cx| {
                    window.render_frame(cx);
                    assert!(window.try_find("resources-runtime-scroll").is_none());
                    let bounds = page.read(cx).lists["providers"].bounds();
                    assert!(
                        bounds.size.height > px(height * 0.6),
                        "viewport must fill page, including short lists: {bounds:?}"
                    );
                    let bottom = window.viewport_size().height - bounds.bottom();
                    assert!(
                        bottom < px(40.),
                        "list must reach workspace bottom: {bottom:?}"
                    );
                    if count == 200 {
                        assert!(
                            window
                                .try_find(format!("provider-update-{tab}-item-199"))
                                .is_none()
                        );
                        page.update(cx, |page, cx| {
                            page.lists["providers"].scroll_to_item(199, ScrollStrategy::Top);
                            cx.notify();
                        });
                        window.render_frame(cx);
                        window.render_frame(cx);
                        assert!(
                            window
                                .find(format!("provider-update-{tab}-item-199"))
                                .visible()
                        );
                    }
                })
                .unwrap();
            }
        }
        crate::integration::finish_test_services(cx);
    }
}

#[cfg(test)]
mod yaml_layout_tests {
    use super::{Session, SettingsKind, SettingsPage};
    use gpui_kit::test::{TestQueryExt, TestWindowExt};
    use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
    #[gpui_kit::test]
    fn whole_document_yaml_hides_tabs_and_search_and_fills_remaining_height(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::storage::Repository::open(crate::storage::AppPaths::isolated(root.path()))
                .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        for kind in [SettingsKind::Dns, SettingsKind::Sniffer] {
            let (window, page) = cx.update(|cx| {
                gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            Default::default(),
                            size(px(1100.), px(800.)),
                        ))),
                        ..Default::default()
                    },
                    cx,
                    |window, cx| {
                        let session = cx.new(Session::new);
                        cx.new(|cx| SettingsPage::new(kind, session, window, cx))
                    },
                )
                .unwrap()
            });
            cx.run_until_parked();
            window
                .update(cx, |_, window, cx| {
                    window.render_frame(cx);
                    assert!(window.elements().iter().any(|element| {
                        element
                            .path()
                            .contains(&gpui_kit::ElementId::from("runtime-tabs"))
                    }));
                    assert!(window.find("dns-edit-mode").visible());
                    assert!(window.try_find("runtime-reset").is_none());
                    window.click("dns-edit-mode", cx);
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |_, window, cx| {
                    window.render_frame(cx);
                    assert!(page.read(cx).dns_raw);
                    assert!(!window.elements().iter().any(|element| {
                        element
                            .path()
                            .contains(&gpui_kit::ElementId::from("runtime-tabs"))
                    }));
                    assert!(
                        window
                            .try_find(gpui_kit::SharedString::from(format!(
                                "{}-runtime-scroll",
                                kind.id()
                            )))
                            .is_none()
                    );
                    let viewport = window.find("settings-yaml-viewport").bounds();
                    assert!(viewport.size.height > px(500.), "{viewport:?}");
                    assert!(viewport.size.width > px(900.));
                    assert!(window.find("dns-edit-mode").visible());
                    assert!(window.try_find("runtime-reset").is_none());
                    window.click("dns-edit-mode", cx);
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |_, window, cx| {
                    window.render_frame(cx);
                    assert!(!page.read(cx).dns_raw);
                    assert!(window.elements().iter().any(|element| {
                        element
                            .path()
                            .contains(&gpui_kit::ElementId::from("runtime-tabs"))
                    }));
                })
                .unwrap();
        }
        crate::integration::finish_test_services(cx);
    }
}
