use super::{
    ProfilesPage,
    model::{Document, DocumentKind, valid_remote_url},
};
use crate::ui;
use crate::ui::theme::{control, layout, space};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon, IndexPath, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Editor, EditorState, Input, InputState, MaskPattern},
        select::{Select, SelectState},
        switch::Switch,
        v_flex,
    },
    *,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EditorMode {
    Content,
    Information,
    Import,
}
pub(super) struct ImportInputs {
    pub(super) url: Entity<InputState>,
    pub(super) token: Entity<InputState>,
    pub(super) agent: Entity<InputState>,
    pub(super) age: Entity<InputState>,
}
pub(super) struct DocumentEditor {
    owner: WeakEntity<ProfilesPage>,
    document: Document,
    mode: EditorMode,
    scope: Entity<SelectState<Vec<SharedString>>>,
    scopes: Vec<(Option<u64>, String)>,
    name: Entity<InputState>,
    url: Entity<InputState>,
    token: Entity<InputState>,
    agent: Entity<InputState>,
    age: Entity<InputState>,
    interval: Entity<InputState>,
    timeout: Entity<InputState>,
    content: Entity<EditorState>,
    with_proxy: bool,
    self_proxy: bool,
    accept_invalid_certs: bool,
}
impl DocumentEditor {
    pub(super) fn new(
        owner: WeakEntity<ProfilesPage>,
        document: Document,
        mode: EditorMode,
        import_inputs: Option<ImportInputs>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut scopes = vec![(None, "所有订阅（全局）".to_owned())];
        scopes.extend(
            super::store::load(false, cx)
                .map(|(docs, _)| docs)
                .unwrap_or_default()
                .into_iter()
                .map(|doc| (Some(doc.id), doc.name)),
        );
        if let Some(id) = document.scope_profile
            && !scopes.iter().any(|scope| scope.0 == Some(id))
        {
            scopes.push((Some(id), document.scope_name.clone()));
        }
        let index = scopes
            .iter()
            .position(|scope| scope.0 == document.scope_profile)
            .unwrap_or(0);
        let scope = cx.new(|cx| {
            SelectState::new(
                scopes
                    .iter()
                    .map(|scope| SharedString::from(scope.1.clone()))
                    .collect(),
                Some(IndexPath::new(index)),
                window,
                cx,
            )
        });
        let importing = mode == EditorMode::Import;

        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(if importing {
                    String::new()
                } else {
                    document.name.clone()
                })
                .placeholder(if importing {
                    "自动获取"
                } else {
                    "配置名称"
                })
        });
        let url = import_inputs
            .as_ref()
            .map(|inputs| inputs.url.clone())
            .unwrap_or_else(|| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(document.url.clone())
                        .placeholder("https://example.com/config.yaml")
                })
            });
        let token = import_inputs
            .as_ref()
            .map(|inputs| inputs.token.clone())
            .unwrap_or_else(|| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(document.token.clone())
                        .placeholder("留空保留已保存凭据")
                        .masked(true)
                })
            });
        let agent = import_inputs
            .as_ref()
            .map(|inputs| inputs.agent.clone())
            .unwrap_or_else(|| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(document.user_agent.clone())
                        .placeholder("留空使用全局默认")
                })
            });
        let age = import_inputs
            .as_ref()
            .map(|inputs| inputs.age.clone())
            .unwrap_or_else(|| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(document.age_key.clone())
                        .placeholder("留空保留已保存凭据")
                        .masked(true)
                })
            });
        let interval = cx.new(|cx| {
            InputState::new(window, cx)
                .mask_pattern(MaskPattern::number(None))
                .default_value(document.interval.clone())
        });
        let timeout = cx.new(|cx| {
            InputState::new(window, cx)
                .mask_pattern(MaskPattern::number(None))
                .default_value(document.timeout.clone())
        });
        let content = cx.new(|cx| {
            ui::editor::state(
                if document.kind == DocumentKind::JavaScript {
                    "javascript"
                } else {
                    "yaml"
                },
                document.content.clone(),
                true,
                window,
                cx,
            )
        });
        Self {
            with_proxy: document.with_proxy,
            self_proxy: document.self_proxy,
            accept_invalid_certs: document.accept_invalid_certs,
            owner,
            document,
            mode,
            scope,
            scopes,
            name,
            url,
            token,
            agent,
            age,
            interval,
            timeout,
            content,
        }
    }
    pub(super) fn revision(&self, cx: &App) -> Vec<String> {
        vec![
            self.with_proxy.to_string(),
            self.self_proxy.to_string(),
            self.accept_invalid_certs.to_string(),
            self.name.read(cx).value().to_string(),
            self.url.read(cx).value().to_string(),
            self.token.read(cx).value().to_string(),
            self.agent.read(cx).value().to_string(),
            self.age.read(cx).value().to_string(),
            self.interval.read(cx).value().to_string(),
            self.timeout.read(cx).value().to_string(),
            self.content.read(cx).value().to_string(),
            self.scope
                .read(cx)
                .selected_value()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ]
    }
    pub(super) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).saving)
        {
            return false;
        }
        let mut document = self.document.clone();
        document.with_proxy = self.with_proxy;
        document.self_proxy = self.self_proxy;
        document.accept_invalid_certs = self.accept_invalid_certs;
        if self.mode == EditorMode::Content {
            document.content = self.content.read(cx).value().to_string();
        } else {
            let label = self.scope.read(cx).selected_value().cloned();
            if let Some(scope) = self
                .scopes
                .iter()
                .find(|scope| Some(scope.1.as_str()) == label.as_deref())
            {
                document.scope_profile = scope.0;
                document.scope_name = scope.1.clone();
            }
            document.name = self.name.read(cx).value().to_string().trim().to_owned();
            document.url = self.url.read(cx).value().to_string().trim().to_owned();
            if document.name.is_empty() && self.mode == EditorMode::Import {
                document.name = if document.kind == DocumentKind::Profile {
                    "远程订阅"
                } else {
                    "远程覆写"
                }
                .into();
            }
            if document.name.is_empty() {
                ui::show_error("填写配置名称".into(), window, cx);
                cx.notify();
                return false;
            }
            if (self.mode == EditorMode::Import || !document.url.is_empty())
                && !valid_remote_url(&document.url)
            {
                ui::show_error(
                    "地址需以 http:// 或 https:// 开头，并包含有效主机名".into(),
                    window,
                    cx,
                );
                cx.notify();
                return false;
            }
            document.interval = self.interval.read(cx).value().to_string();
            document.timeout = self.timeout.read(cx).value().to_string();
            if document
                .interval
                .parse::<u32>()
                .ok()
                .filter(|value| *value <= 525600)
                .is_none()
            {
                ui::show_error("更新间隔需为 0–525600 分钟".into(), window, cx);
                return false;
            }
            if document.is_remote()
                && document
                    .timeout
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0)
                    .is_none()
            {
                ui::show_error("下载超时需为大于 0 的整数".into(), window, cx);
                cx.notify();
                return false;
            }
            document.token = self.token.read(cx).value().to_string();
            document.user_agent = self.agent.read(cx).value().to_string();
            document.age_key = self.age.read(cx).value().to_string();
        }
        let editor = (cx.weak_entity(), self.revision(cx));
        match self.owner.update(cx, |owner, cx| {
            owner.save_document_with(document, Some(editor), window, cx)
        }) {
            Ok(Ok(())) => false,
            Ok(Err(error)) => {
                ui::show_error(error, window, cx);
                cx.notify();
                false
            }
            Err(_) => true,
        }
    }
}
impl Render for DocumentEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = v_flex().gap(space::LG);
        if self.mode == EditorMode::Content {
            return content
                .size_full()
                .min_h(space::NONE)
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap(space::SM)
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::FileCode).size(layout::ICON))
                        .child(format!(
                            "{}.{}",
                            self.document.name,
                            self.document.kind.extension()
                        ))
                        .child(div().flex_1())
                        .child(
                            Button::new("validate-document-yaml")
                                .icon(gpui_kit::assets::IconName::Check)
                                .ghost()
                                .with_size(control::STANDARD)
                                .label("检查语法")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    crate::core::request(
                                        crate::core::Command::CheckDocument {
                                            kind: this.document.kind,
                                            content: this.content.read(cx).value().to_string(),
                                        },
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            Button::new("copy-editor-file")
                                .icon(gpui_kit::assets::IconName::Copy)
                                .ghost()
                                .with_size(control::STANDARD)
                                .label("复制")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        this.content.read(cx).value().to_string(),
                                    ))
                                })),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h(space::NONE)
                        .font_family("monospace")
                        .child(
                            Editor::new(&self.content)
                                .w_full()
                                .h_full()
                                .aria_label("配置文件内容"),
                        ),
                );
        }
        let mut form = ui::form().child(ui::field_label(
            "名称",
            Input::new(&self.name)
                .id("document-name")
                .with_size(control::STANDARD)
                .w_full()
                .aria_label("名称"),
        ));
        let remote = self.mode == EditorMode::Import || self.document.is_remote();
        if remote {
            form = form
                .child(ui::field_label(
                    "订阅地址",
                    Input::new(&self.url)
                        .id("document-url")
                        .with_size(control::STANDARD)
                        .w_full()
                        .aria_label("订阅地址"),
                ))
                .child(ui::field_label(
                    "User-Agent",
                    Input::new(&self.agent)
                        .id("document-agent")
                        .with_size(control::STANDARD)
                        .w_full()
                        .aria_label("User-Agent"),
                ))
                .child(
                    ui::form_row()
                        .child(
                            ui::field_label(
                                "更新间隔（分钟）",
                                Input::new(&self.interval)
                                    .w_full()
                                    .with_size(control::STANDARD)
                                    .aria_label("更新间隔（分钟）"),
                            )
                            .flex_1(),
                        )
                        .child(
                            ui::field_label(
                                "超时（秒）",
                                Input::new(&self.timeout)
                                    .w_full()
                                    .with_size(control::STANDARD)
                                    .aria_label("下载超时（秒）"),
                            )
                            .flex_1(),
                        ),
                )
                .child(ui::field_label(
                    "访问令牌",
                    Input::new(&self.token)
                        .id("document-token")
                        .mask_toggle()
                        .with_size(control::STANDARD)
                        .w_full()
                        .aria_label("访问令牌"),
                ));
            if self.document.kind == DocumentKind::Profile {
                form = form.child(ui::field_label(
                    "Age 私钥",
                    Input::new(&self.age)
                        .id("document-age")
                        .mask_toggle()
                        .with_size(control::STANDARD)
                        .w_full()
                        .aria_label("Age 私钥"),
                ));
            }
        }
        if self.document.kind != DocumentKind::Profile {
            form = form
                .child(ui::field_label(
                    "作用范围",
                    Select::new(&self.scope)
                        .with_size(control::STANDARD)
                        .w_full()
                        .accessibility_label("覆写作用范围"),
                ))
                .child(
                    ui::form_row().justify_between().child("启用此覆写").child(
                        Switch::new("editor-enabled")
                            .with_size(control::STANDARD)
                            .checked(self.document.enabled)
                            .accessibility_label("启用此覆写")
                            .on_change(cx.listener(|this, value, _, cx| {
                                this.document.enabled = *value;
                                cx.notify();
                            })),
                    ),
                );
        }
        if remote {
            for (key, label, checked) in [
                ("with-proxy", "使用系统代理", self.with_proxy),
                ("self-proxy", "使用内核代理", self.self_proxy),
                ("invalid-certs", "忽略证书错误", self.accept_invalid_certs),
            ] {
                form = form.child(
                    ui::form_row().justify_between().child(label).child(
                        Switch::new(key)
                            .with_size(control::STANDARD)
                            .checked(checked)
                            .accessibility_label(label)
                            .on_change(cx.listener(move |this, checked, _, cx| {
                                match key {
                                    "with-proxy" => this.with_proxy = *checked,
                                    "self-proxy" => this.self_proxy = *checked,
                                    _ => this.accept_invalid_certs = *checked,
                                }
                                cx.notify();
                            })),
                    ),
                );
            }
        }
        content = content.flex_none().child(form);
        content
    }
}
