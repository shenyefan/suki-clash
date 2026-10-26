use super::{
    ProfilesPage,
    model::{Document, DocumentKind, valid_remote_url},
};
use crate::ui;
use crate::ui::theme::{control, layout, space, text};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon, IndexPath, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Editor, EditorState, Input, InputState},
        select::{Select, SelectState},
        switch::Switch,
        v_flex,
    },
    *,
};

pub(super) struct DocumentEditor {
    owner: WeakEntity<ProfilesPage>,
    document: Document,
    content_only: bool,
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
}
impl DocumentEditor {
    pub(super) fn new(
        owner: WeakEntity<ProfilesPage>,
        document: Document,
        content_only: bool,
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
        let name = cx.new(|cx| InputState::new(window, cx).default_value(document.name.clone()));
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(document.url.clone())
                .placeholder("https://example.com/config.yaml")
        });
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(document.token.clone())
                .masked(true)
        });
        let agent = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(document.user_agent.clone())
                .placeholder("留空使用全局默认")
        });
        let age = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(document.age_key.clone())
                .masked(true)
        });
        let interval =
            cx.new(|cx| InputState::new(window, cx).default_value(document.interval.clone()));
        let timeout =
            cx.new(|cx| InputState::new(window, cx).default_value(document.timeout.clone()));
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
            owner,
            document,
            content_only,
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
            self.document.auto_update.to_string(),
        ]
    }
    pub(super) fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut document = self.document.clone();
        if self.content_only {
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
            if document.name.is_empty() {
                ui::show_error("填写配置名称".into(), window, cx);
                cx.notify();
                return false;
            }
            if !document.url.is_empty() && !valid_remote_url(&document.url) {
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
            if document.auto_update
                && document
                    .interval
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n > 0)
                    .is_none()
            {
                ui::show_error("自动更新间隔需为大于 0 的整数".into(), window, cx);
                cx.notify();
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
        if document.is_remote() && document.content.is_empty() {
            ui::request_command("下载远程配置", window, cx);
            return false;
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
        if self.content_only {
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
                        .when(self.document.kind != DocumentKind::JavaScript, |row| {
                            row.child(
                                Button::new("validate-document-yaml")
                                    .ghost()
                                    .with_size(control::STANDARD)
                                    .label("检查 YAML")
                                    .on_click(cx.listener(|this, _, w, cx| {
                                        match serde_yaml_ng::from_str::<serde_yaml_ng::Value>(
                                            this.content.read(cx).value().as_ref(),
                                        ) {
                                            Ok(_) => {
                                                w.push_notification("YAML 格式正确", cx);
                                            }
                                            Err(error) => ui::show_error(error.to_string(), w, cx),
                                        }
                                    })),
                            )
                        })
                        .child(
                            Button::new("copy-editor-file")
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
        let mut form = v_flex().gap(space::LG).pr(space::SM).child(ui::field_label(
            "名称",
            Input::new(&self.name)
                .with_size(control::STANDARD)
                .w_full()
                .aria_label("名称"),
        ));
        if self.document.is_remote() {
            form = form
                .child(ui::field_label(
                    "订阅地址",
                    Input::new(&self.url)
                        .with_size(control::STANDARD)
                        .w_full()
                        .aria_label("订阅地址"),
                ))
                .child(ui::field_label(
                    "访问令牌",
                    Input::new(&self.token)
                        .with_size(control::STANDARD)
                        .w_full()
                        .aria_label("访问令牌"),
                ))
                .child(ui::field_label(
                    "User-Agent",
                    Input::new(&self.agent)
                        .with_size(control::STANDARD)
                        .w_full()
                        .aria_label("User-Agent"),
                ))
                .when(self.document.kind == DocumentKind::Profile, |el| {
                    el.child(ui::field_label(
                        "Age 私钥",
                        Input::new(&self.age)
                            .with_size(control::STANDARD)
                            .w_full()
                            .aria_label("Age 私钥"),
                    ))
                })
                .child(
                    h_flex().justify_between().child("自动更新").child(
                        Switch::new("editor-auto-update")
                            .checked(self.document.auto_update)
                            .accessibility_label("自动更新")
                            .on_change(cx.listener(|this, value, _, cx| {
                                this.document.auto_update = *value;
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    h_flex()
                        .gap(space::LG)
                        .child(ui::field_label(
                            "更新间隔（分钟）",
                            Input::new(&self.interval)
                                .with_size(control::STANDARD)
                                .w_full()
                                .aria_label("更新间隔（分钟）"),
                        ))
                        .child(ui::field_label(
                            "下载超时（秒）",
                            Input::new(&self.timeout)
                                .with_size(control::STANDARD)
                                .w_full()
                                .aria_label("下载超时（秒）"),
                        )),
                );
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
                    h_flex()
                        .justify_between()
                        .child(
                            v_flex().gap(space::XS).child("启用此覆写").child(
                                ui::muted("应用覆写时包含此项目", cx).text_size(text::CAPTION),
                            ),
                        )
                        .child(
                            Switch::new("editor-enabled")
                                .checked(self.document.enabled)
                                .accessibility_label("启用此覆写")
                                .on_change(cx.listener(|this, value, _, cx| {
                                    this.document.enabled = *value;
                                    cx.notify();
                                })),
                        ),
                );
        }
        content = content.child(form);
        content
    }
}
