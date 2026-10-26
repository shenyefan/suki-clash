use crate::ui::theme::{control, space};
mod actions;
mod core;
mod dns;
mod fields;
mod preferences;
mod resources;
mod runtime;
mod schema;
mod view;
use crate::integration as store;

use crate::{session::Session, ui};
use gpui_kit::{
    assets::IconName,
    component::{
        IndexPath, Sizable, Theme, ThemeMode, WindowExt,
        button::{Button, ButtonVariants},
        dialog::DialogButtonProps,
        h_flex,
        input::{EditorState, Input, InputEvent, InputState},
        select::SelectState,
        v_flex,
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

/// Settings destinations are owned by the settings feature, independent of
/// the application's sidebar routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsKind {
    Sniffer,
    Dns,
    Core,
    Resources,
    SysProxy,
    Tun,
    General,
}
impl SettingsKind {
    pub(crate) const ALL: [Self; 7] = [
        Self::Sniffer,
        Self::Dns,
        Self::Core,
        Self::Resources,
        Self::SysProxy,
        Self::Tun,
        Self::General,
    ];
    pub(super) fn uses_grouped_layout(self) -> bool {
        true
    }
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Sniffer => "sniffer",
            Self::Dns => "dns",
            Self::Core => "core",
            Self::Resources => "resources",
            Self::SysProxy => "sysproxy",
            Self::Tun => "tun",
            Self::General => "settings",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Sniffer => "嗅探",
            Self::Dns => "DNS",
            Self::Core => "内核",
            Self::Resources => "外部资源",
            Self::SysProxy => "系统代理",
            Self::Tun => "虚拟网卡",
            Self::General => "设置",
        }
    }
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
    active_section: usize,
    runtime_tab: usize,
    dirty: bool,
    saving: bool,
    resource_pending: std::collections::BTreeSet<String>,
    dns_raw: bool,
    dns_source: Entity<EditorState>,
    dns_document: serde_yaml_ng::Value,
    error: Option<String>,
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
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("搜索设置项…"));
        let mut subscriptions = vec![
            cx.observe(&session, |_, _, cx| cx.notify()),
            cx.subscribe(&filter, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        let dns_source = cx.new(|cx| ui::editor::state("yaml", "", true, window, cx));
        subscriptions.push(
            cx.subscribe(&dns_source, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) && this.dns_raw {
                    this.dirty = true;
                    cx.notify();
                }
            }),
        );
        let dns_document = saved
            .get("DnsYaml")
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
                    let state = cx.new(|cx| {
                        InputState::new(window, cx)
                            .default_value(value.to_owned())
                            .masked(spec.kind == FieldKind::Secret)
                    });
                    subscriptions.push(cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.dirty = true;
                            this.error = None;
                            cx.notify();
                        }
                    }));
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
                    subscriptions.push(cx.subscribe(&state, |this, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.dirty = true;
                            this.error = None;
                            cx.notify();
                        }
                    }));
                    FieldControl::Multiline(state)
                }
                FieldKind::Select => {
                    let index = spec.options.iter().position(|v| *v == value).unwrap_or(0);
                    let state = cx.new(|cx| {
                        SelectState::new(
                            spec.options
                                .iter()
                                .map(|v| SharedString::from(*v))
                                .collect::<Vec<_>>(),
                            Some(IndexPath::new(index)),
                            window,
                            cx,
                        )
                    });
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
                                this.dirty = true;
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
            kind,
            session,
            sections,
            fields,
            filter,
            active_section: usize::MAX,
            runtime_tab: 0,
            dirty: false,
            saving: false,
            resource_pending: Default::default(),
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
                if field.spec.kind == FieldKind::Secret {
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
        if self.kind == SettingsKind::SysProxy {
            if values
                .get("UseDefaultBypass")
                .is_some_and(|value| value == "true")
            {
                values.insert("BypassList".into(), schema::default_bypass().into());
            }
            let host = values.get("SystemProxyHost").cloned().unwrap_or_default();
            let mixed = store::read("core", cx)
                .get("MixedPort")
                .cloned()
                .unwrap_or_else(|| "7890".into());
            if let Some(script) = values.get_mut("PacScript") {
                *script = script
                    .replace("%proxy_host%", &host)
                    .replace("%mixed-port%", &mixed);
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
        let values = self.command_values(cx);
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
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_inner(false, window, cx);
    }
    fn save_inner(&mut self, backup: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        if self.kind == SettingsKind::Dns
            && self.dns_raw
            && let Err(error) = self.read_dns_yaml(window, cx)
        {
            ui::show_error(error, window, cx);
            return;
        }
        let mut values = self.values(cx);
        if self.kind == SettingsKind::Dns {
            match self.dns_to_yaml(cx) {
                Ok(source) => {
                    values.insert("DnsYaml".into(), source);
                }
                Err(error) => {
                    ui::show_error(error, window, cx);
                    return;
                }
            }
        }
        if self.kind == SettingsKind::Core {
            schema::store_panels(&self.panels, &mut values);
        }
        if let Some(error) = validate(&self.sections, &values) {
            ui::show_error(error.clone(), window, cx);
            self.error = Some(error);
            cx.notify();
            return;
        }
        let owner = cx.weak_entity();
        let submitted = values.clone();
        let submitted_dns_source = self.dns_source.read(cx).value().to_string();
        self.saving = true;
        cx.notify();
        store::save(
            self.kind.id(),
            values,
            Vec::new(),
            window,
            cx,
            move |result, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.saving = false;
                    match result {
                        Ok(()) => {
                            let mut current = this.values(cx);
                            if this.kind == SettingsKind::Dns {
                                current.insert(
                                    "DnsYaml".into(),
                                    this.dns_to_yaml(cx).unwrap_or_default(),
                                );
                            }
                            if this.kind == SettingsKind::Core {
                                schema::store_panels(&this.panels, &mut current);
                            }
                            this.dirty = current != submitted
                                || (this.kind == SettingsKind::Dns
                                    && this.dns_raw
                                    && this.dns_source.read(cx).value().as_ref()
                                        != submitted_dns_source);
                            this.error = None;
                            if this.kind == SettingsKind::General {
                                ui::configure_theme(cx);
                                cx.emit(SettingsSaved {
                                    backdrop: submitted
                                        .get("BackdropIndex")
                                        .cloned()
                                        .unwrap_or_default(),
                                });
                            }
                            window.push_notification(
                                if crate::storage::contains_session_credentials(&submitted) {
                                    "已保存设置；密钥和密码仅在本次运行保留"
                                } else {
                                    "已保存设置"
                                },
                                cx,
                            );
                            if backup
                                && let Ok(namespace) =
                                    crate::storage::Namespace::parse(this.kind.id())
                            {
                                store::execute(
                                    store::Command::Backup(namespace),
                                    window,
                                    cx,
                                    |result, window, cx| match result {
                                        Ok(store::Outcome::Backup(path)) => window
                                            .push_notification(
                                                format!("本页设置已备份：{}", path.display()),
                                                cx,
                                            ),
                                        Err(error) => ui::show_error(error.to_string(), window, cx),
                                        _ => {}
                                    },
                                );
                            }
                        }
                        Err(error) => {
                            let message = format!("无法保存设置：{error}");
                            ui::show_error(message.clone(), window, cx);
                            this.error = Some(message);
                        }
                    }
                    cx.notify();
                });
            },
        );
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
    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        window.open_alert_dialog(cx, move |dialog, window, cx| {
            ui::modal_alert(dialog, window, cx)
                .title("恢复本页默认设置？")
                .description("恢复当前页面的默认设置，保存后生效。")
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("恢复默认")
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok({
                    let entity = entity.clone();
                    move |_, window, cx| {
                        entity.update(cx, |this, cx| {
                            this.assign_values(&BTreeMap::new(), window, cx)
                        });
                        true
                    }
                })
        });
    }
    fn toggle(&mut self, key: &'static str, value: bool, cx: &mut Context<Self>) {
        if let Some(field) = self.fields.get_mut(key) {
            field.control = FieldControl::Toggle(value);
        }
        if key == "AutoRoute"
            && !value
            && let Some(field) = self.fields.get_mut("AutoRedirect")
        {
            field.control = FieldControl::Toggle(false);
        }
        self.dirty = true;
        cx.notify();
    }
    fn field_action(
        &mut self,
        key: &'static str,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
                cx.notify();
            }
        } else if matches!(
            action,
            "创建备份"
                | "恢复最新备份"
                | "重置应用"
                | "退出应用"
                | "复制版本号"
                | "复制环境变量"
                | "复制代理地址"
                | "复制 PAC 地址"
        ) {
            self.action(action, window, cx);
        } else if self.kind.uses_grouped_layout() {
            let values = self.command_values(cx);
            if matches!(
                action,
                "测试 WebDAV 连接" | "上传备份" | "获取远程备份…" | "远程备份历史…"
            ) && ["WebDavUrl", "WebDavUsername", "WebDavPassword"]
                .iter()
                .any(|key| values.get(*key).is_none_or(|value| value.trim().is_empty()))
            {
                ui::show_error("填写 WebDAV 地址、用户名和密码后再操作".into(), window, cx);
                return;
            }
            if let Some(error) = validate(&self.sections, &values) {
                ui::show_error(error, window, cx);
                return;
            }
            crate::integration::request_with(action, values, window, cx);
        } else {
            self.action(action, window, cx);
        }
    }
    fn action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.local_action(action, window, cx) {
            return;
        }
        match action {
            "退出应用" => cx.quit(),
            "创建备份" => self.save_inner(true, window, cx),
            "恢复最新备份" => {
                let owner = cx.weak_entity();
                if let Ok(namespace) = crate::storage::Namespace::parse(self.kind.id()) {
                    store::execute(
                        store::Command::Restore(namespace),
                        window,
                        cx,
                        move |result, window, cx| {
                            let _ = owner.update(cx, |this, cx| match result {
                                Ok(store::Outcome::Restored(values)) => {
                                    this.assign_values(&values, window, cx)
                                }
                                Err(error) => ui::show_error(error.to_string(), window, cx),
                                _ => {}
                            });
                        },
                    );
                }
            }
            "重置应用" => {
                let values = self.command_values(cx);
                window.open_alert_dialog(cx, move |dialog, window, cx| {
                    let values = values.clone();
                    ui::modal_alert(dialog, window, cx)
                        .title("重置应用？")
                        .description("应用设置与配置将由重置服务处理，请先创建备份。")
                        .button_props(
                            DialogButtonProps::default()
                                .ok_text("重置")
                                .cancel_text("取消")
                                .show_cancel(true),
                        )
                        .on_ok(move |_, window, cx| {
                            crate::integration::request_with(
                                "重置应用",
                                values.clone(),
                                window,
                                cx,
                            );
                            true
                        })
                });
            }
            "UWP 应用代理" | "选择应用…" => Self::uwp_dialog(window, cx),
            "添加 WebUI" => self.add_panel(window, cx),
            "恢复默认 WebUI" => {
                self.panels = schema::default_panels();
                self.next_panel_id = 4;
                self.dirty = true;
                cx.notify();
            }
            _ => ui::request_command(action, window, cx),
        }
    }
    fn uwp_dialog(window: &mut Window, cx: &mut App) {
        ui::request_command("枚举 UWP 回环应用", window, cx);
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
                .placeholder("https://…")
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
                    v_flex()
                        .gap(space::LG)
                        .child(ui::field_label(
                            "名称",
                            Input::new(&name).with_size(control::STANDARD).w_full(),
                        ))
                        .child(ui::field_label(
                            "地址",
                            Input::new(&url).with_size(control::STANDARD).w_full(),
                        ))
                        .into_any_element()
                },
            ))
            .footer(
                h_flex()
                    .justify_end()
                    .gap(space::SM)
                    .child(
                        Button::new("cancel-panel")
                            .with_size(control::STANDARD)
                            .label("取消")
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                    )
                    .child(
                        Button::new("save-panel")
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
                                    cx.notify();
                                });
                                window.close_dialog(cx);
                            }),
                    ),
            )
        });
    }
}
