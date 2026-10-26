use crate::ui::theme::{control, layout, space};
mod editor;
mod enhance;
mod model;
mod organize;
mod overrides;
mod store;
mod view;

use crate::{session::Session, ui};
use editor::DocumentEditor;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        Sizable, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Input, InputState},
        v_flex,
    },
    *,
};
use model::{Document, DocumentKind, valid_remote_url};

pub(crate) struct ProfilesPage {
    overrides: bool,
    batch: bool,
    selected: std::collections::BTreeSet<u64>,
    session: Entity<Session>,
    documents: Vec<Document>,
    next_id: u64,
    desired_profile: Option<u64>,
    saving: bool,
    url: Entity<InputState>,
    columns: u16,
    card_focus: std::collections::BTreeMap<u64, FocusHandle>,
    import_token: Entity<InputState>,
    import_agent: Entity<InputState>,
    import_age: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
impl ProfilesPage {
    pub(crate) fn new(
        overrides: bool,
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url = cx.new(|cx| {
            InputState::new(window, cx).placeholder(if overrides {
                "远程覆写地址…"
            } else {
                "粘贴订阅链接…"
            })
        });
        let import_token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Authorization 标头内容")
                .masked(true)
        });
        let import_agent = cx.new(|cx| InputState::new(window, cx).placeholder("留空使用全局默认"));
        let import_age = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("AGE-SECRET-KEY-…")
                .masked(true)
        });
        let subscriptions = vec![cx.observe(&session, |_, _, cx| cx.notify())];
        let stored = store::load(overrides, cx);
        let (documents, active) = stored.unwrap_or_default();
        let next_id = documents.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        let desired_profile = active.filter(|id| documents.iter().any(|doc| doc.id == *id));
        let card_focus = documents
            .iter()
            .map(|doc| (doc.id, cx.focus_handle()))
            .collect();
        Self {
            card_focus,
            overrides,
            batch: false,
            selected: Default::default(),
            session,
            columns: 2,
            documents,
            next_id,
            desired_profile,
            saving: false,
            url,
            import_token,
            import_agent,
            import_age,
            _subscriptions: subscriptions,
        }
    }
    fn open_editor(
        &mut self,
        document: Document,
        content_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = if content_only {
            format!("编辑文件 · {}", document.name)
        } else {
            format!(
                "编辑{}",
                if self.overrides {
                    "覆写信息"
                } else {
                    "订阅"
                }
            )
        };
        let owner = cx.weak_entity();
        let editor = cx.new(|cx| DocumentEditor::new(owner, document, content_only, window, cx));
        let width = if content_only {
            layout::EDITOR_DIALOG_WIDTH
        } else {
            layout::DIALOG_WIDTH
        };
        window.open_dialog(cx, move |dialog, window, cx| {
            let commit = editor.clone();
            let body = editor.clone();
            ui::modal(dialog, title.clone(), width, window, cx)
                .map(|dialog| {
                    if content_only {
                        dialog.content(ui::modal_panel(layout::FORM_MAX_HEIGHT, move |_, _| {
                            body.clone().into_any_element()
                        }))
                    } else {
                        dialog.content(ui::modal_content(
                            "document-editor-body",
                            layout::FORM_MAX_HEIGHT,
                            move |_, _| body.clone().into_any_element(),
                        ))
                    }
                })
                .footer(ui::dialog_footer("保存"))
                .on_ok(move |_, window, cx| {
                    commit.update(cx, |editor, cx| editor.commit(window, cx))
                })
        });
    }
    fn create(&mut self, kind: DocumentKind, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_id;
        self.next_id += 1;
        self.open_editor(Document::new(id, kind), true, window, cx);
    }
    fn import_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.url.read(cx).value().to_string().trim().to_owned();
        if !valid_remote_url(&url) {
            ui::show_error("填写有效的 HTTP 或 HTTPS 地址".into(), window, cx);
            cx.notify();
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let kind = if self.overrides {
            if url.split('?').next().is_some_and(|p| p.ends_with(".js")) {
                DocumentKind::JavaScript
            } else {
                DocumentKind::Yaml
            }
        } else {
            DocumentKind::Profile
        };
        let mut document = Document::new(id, kind);
        document.name = if self.overrides {
            "远程覆写"
        } else {
            "远程订阅"
        }
        .into();
        document.url = url;
        document.content = String::new();
        document.token = self.import_token.read(cx).value().to_string();
        document.user_agent = self.import_agent.read(cx).value().to_string();
        document.age_key = self.import_age.read(cx).value().to_string();
        self.open_editor(document, false, window, cx);
    }
    fn import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.import_token.clone();
        let agent = self.import_agent.clone();
        let age = self.import_age.clone();
        let overrides = self.overrides;
        let url = self.url.clone();
        let owner = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let owner = owner.clone();
            let (url, token, agent, age) = (url.clone(), token.clone(), agent.clone(), age.clone());
            ui::modal(
                dialog,
                if overrides {
                    "导入远程覆写"
                } else {
                    "导入订阅"
                },
                layout::DIALOG_WIDTH,
                window,
                cx,
            )
            .content(ui::modal_content(
                "import-document-body",
                crate::ui::theme::layout::DIALOG_FORM_HEIGHT,
                move |_, _| {
                    v_flex()
                        .gap(space::LG)
                        .child(ui::field_label(
                            "订阅地址",
                            Input::new(&url)
                                .with_size(control::STANDARD)
                                .w_full()
                                .aria_label("订阅地址"),
                        ))
                        .child(ui::field_label(
                            "访问令牌",
                            Input::new(&token)
                                .with_size(control::STANDARD)
                                .w_full()
                                .aria_label("导入访问令牌"),
                        ))
                        .child(ui::field_label(
                            "User-Agent",
                            Input::new(&agent)
                                .with_size(control::STANDARD)
                                .w_full()
                                .aria_label("导入 User-Agent"),
                        ))
                        .when(!overrides, |el| {
                            el.child(ui::field_label(
                                "Age 私钥",
                                Input::new(&age)
                                    .with_size(control::STANDARD)
                                    .w_full()
                                    .aria_label("导入 Age 私钥"),
                            ))
                        })
                        .into_any_element()
                },
            ))
            .footer(
                h_flex().justify_end().child(
                    Button::new("done-advanced")
                        .with_size(control::STANDARD)
                        .label("继续")
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            let owner = owner.clone();
                            window.defer(cx, move |window, cx| {
                                owner.update(cx, |this, cx| this.import_remote(window, cx))
                            });
                        }),
                ),
            )
        });
    }
    fn import_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("导入 YAML 或 JavaScript 配置".into()),
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = match receiver.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => Some(
                        cx.background_spawn(async move { store::read_document(path) })
                            .await,
                    ),
                    None => None,
                },
                Ok(Ok(None)) => None,
                Ok(Err(error)) => Some(Err(error.to_string())),
                Err(error) => Some(Err(error.to_string())),
            };
            if let Some(result) = result {
                let _ = cx.update(|window, cx| {
                    let _ = owner.update(cx, |this, cx| match result {
                        Ok((name, content, js)) => {
                            if js && !this.overrides {
                                ui::show_error(
                                    "订阅仅支持 YAML 配置，JavaScript 请在覆写页导入".into(),
                                    window,
                                    cx,
                                );
                                cx.notify();
                                return;
                            }
                            let kind = if !this.overrides {
                                DocumentKind::Profile
                            } else if js {
                                DocumentKind::JavaScript
                            } else {
                                DocumentKind::Yaml
                            };
                            let id = this.next_id;
                            this.next_id += 1;
                            let mut document = Document::new(id, kind);
                            document.name = name;
                            document.content = content;
                            if let Err(error) = this.save_document(document, window, cx) {
                                ui::show_error(error, window, cx);
                                cx.notify();
                            }
                        }
                        Err(error) => {
                            ui::show_error(error, window, cx);
                            cx.notify();
                        }
                    });
                });
            }
        })
        .detach();
    }
    fn persist_documents(
        &mut self,
        documents: Vec<Document>,
        active: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
        complete: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Result<(), String> {
        if self.saving {
            return Err("正在保存配置，请稍后重试".into());
        }
        let writes = documents
            .iter()
            .filter(|doc| !doc.is_remote() || !doc.content.is_empty())
            .map(|doc| crate::integration::DocumentWrite {
                path: store::document_path(self.overrides, doc, cx),
                content: doc.content.clone(),
            })
            .collect();
        let values = store::values(&documents, active);
        let owner = cx.weak_entity();
        self.saving = true;
        cx.notify();
        crate::integration::save(
            store::key(self.overrides),
            values,
            writes,
            window,
            cx,
            move |result, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.saving = false;
                    match result {
                        Ok(()) => {
                            this.documents = documents;
                            this.desired_profile = active;
                            for doc in &this.documents {
                                this.card_focus
                                    .entry(doc.id)
                                    .or_insert_with(|| cx.focus_handle());
                            }
                            complete(this, window, cx);
                        }
                        Err(error) => ui::show_error(error, window, cx),
                    }
                    cx.notify();
                });
            },
        );
        Ok(())
    }
    fn save_document(
        &mut self,
        document: Document,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.save_document_with(document, None, window, cx)
    }
    fn save_document_with(
        &mut self,
        document: Document,
        editor: Option<(WeakEntity<DocumentEditor>, Vec<String>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut documents = self.documents.clone();
        if let Some(existing) = documents.iter_mut().find(|d| d.id == document.id) {
            *existing = document;
        } else {
            documents.push(document);
        }
        let active = self.desired_profile;
        let submit_focus = window.focused(cx);
        self.persist_documents(documents, active, window, cx, move |_, window, cx| {
            if submit_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
                && editor.is_some_and(|(owner, revision)| {
                    owner
                        .upgrade()
                        .is_some_and(|editor| editor.read(cx).revision(cx) == revision)
                })
            {
                window.close_dialog(cx);
            }
        })
    }
    fn document_action(
        &mut self,
        id: u64,
        action: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.documents.iter().find(|d| d.id == id).cloned() else {
            return;
        };
        match action {
            "分享二维码…" => Self::share_qr(&document, window, cx),
            "复制链接" => cx.write_to_clipboard(ClipboardItem::new_string(document.url)),
            "编辑规则…" => enhance::open(document, 0, window, cx),
            "编辑代理…" => enhance::open(document, 1, window, cx),
            "编辑代理组…" => enhance::open(document, 2, window, cx),
            "编辑信息…" => self.open_editor(document, false, window, cx),
            "编辑文件…" => self.open_editor(document, true, window, cx),
            "更新" => ui::request_command(&format!("更新订阅:{id}"), window, cx),
            "执行日志…" => {
                ui::detail_dialog(format!("执行日志 · {}", document.name), "", window, cx)
            }
            "打开文件" => {
                let path = store::document_path(self.overrides, &document, cx);
                crate::integration::execute(
                    crate::integration::Command::OpenDocument(path),
                    window,
                    cx,
                    |result, window, cx| match result {
                        Ok(crate::integration::Outcome::Document(path)) => {
                            cx.open_with_system(&path)
                        }
                        Err(error) => ui::show_error(error.to_string(), window, cx),
                        _ => {}
                    },
                );
            }
            "删除" => {
                if self.session.read(cx).active_profile_id() == Some(id) {
                    ui::request_command("删除正在使用的订阅", window, cx);
                    return;
                }
                let candidate = self
                    .documents
                    .iter()
                    .filter(|d| d.id != id)
                    .cloned()
                    .collect::<Vec<_>>();
                let active = self.desired_profile.filter(|active| *active != id);
                let result =
                    self.persist_documents(candidate, active, window, cx, move |_, window, cx| {
                        cx.notify();
                        let owner = cx.entity();
                        window.push_notification(
                            gpui_kit::component::notification::Notification::new()
                                .message(format!("已移除“{}”", document.name))
                                .action(move |_, _, _| {
                                    let owner = owner.clone();
                                    let document = document.clone();
                                    Button::new("undo-delete")
                                        .with_size(control::STANDARD)
                                        .ghost()
                                        .label("撤销")
                                        .on_click(move |_, window, cx| {
                                            owner.update(cx, |this, cx| {
                                                if !this.documents.iter().any(|d| d.id == id)
                                                    && let Err(error) = this.save_document(
                                                        document.clone(),
                                                        window,
                                                        cx,
                                                    )
                                                {
                                                    ui::show_error(error, window, cx);
                                                    cx.notify();
                                                }
                                            })
                                        })
                                }),
                            cx,
                        );
                    });
                if let Err(error) = result {
                    ui::show_error(error, window, cx);
                }
            }
            _ => {}
        }
    }
    fn select_profile(&mut self, _id: u64, window: &mut Window, cx: &mut Context<Self>) {
        ui::request_command("激活订阅", window, cx);
    }
}
