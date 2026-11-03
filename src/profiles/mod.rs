use crate::ui::theme::{control, layout};
mod credentials;
mod editor;
mod enhance;
pub(crate) mod model;
pub(crate) mod override_repository;
mod proxy_uri;
pub(crate) mod remote;
pub(crate) mod repository;
pub(crate) use credentials::SecretEdit;
pub(crate) use credentials::{CredentialEdits, CredentialStore};
mod organize;
mod overrides;
pub(crate) mod store;
mod view;

use crate::{session::Session, ui};
use editor::{DocumentEditor, EditorMode, ImportInputs};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        Sizable, WindowExt,
        button::{Button, ButtonVariants},
        input::InputState,
    },
    *,
};
use model::{Document, DocumentKind, valid_remote_url};

type ProfileCompletion =
    Box<dyn FnOnce(&mut ProfilesPage, &mut Window, &mut Context<ProfilesPage>)>;
pub(crate) struct ProfilesPage {
    overrides: bool,
    batch: bool,
    selected: std::collections::BTreeSet<u64>,
    session: Entity<Session>,
    documents: Vec<Document>,
    next_id: u64,
    desired_profile: Option<u64>,
    core_revision: u64,
    saving: bool,
    rejected: bool,
    override_drop: Option<(u64, bool)>,
    import_task: Option<Task<()>>,
    url: Entity<InputState>,
    columns: u16,
    list_scroll: gpui_kit::component::VirtualListScrollHandle,
    card_focus: std::collections::BTreeMap<u64, FocusHandle>,
    import_token: Entity<InputState>,
    import_agent: Entity<InputState>,
    import_age: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    pending_profile: Option<(u64, ProfileCompletion)>,
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
                "远程覆写地址"
            } else {
                "粘贴订阅链接"
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
                .placeholder("AGE-SECRET-KEY-")
                .masked(true)
        });
        let subscriptions = vec![
            cx.observe_in(&session, window, |this, session, window, cx| {
                {
                    let revision = session.read(cx).core().revision();
                    if this.core_revision == revision {
                        return;
                    }
                    this.core_revision = revision;
                    let core = session.read(cx).core().clone();
                    if !session.read(cx).core_busy() {
                        let completed = this.pending_profile.as_ref().is_some_and(|(id, _)| {
                            core.request_id().is_some_and(|request| request >= *id)
                        });
                        let committed = core.operation_committed()
                            && this
                                .pending_profile
                                .as_ref()
                                .is_some_and(|(id, _)| core.request_id() == Some(*id));
                        if completed {
                            this.saving = false;
                            this.rejected = core.error().is_some() && !committed;
                        }
                        if !this.rejected
                            && let Some(values) = core.settings(store::key(this.overrides))
                            && let Some((documents, _)) = store::load_values(values.clone())
                        {
                            this.documents = documents;
                            this.desired_profile =
                                values.get("active").and_then(|value| value.parse().ok());
                            this.next_id = this.next_id.max(
                                this.documents
                                    .iter()
                                    .map(|doc| doc.id)
                                    .max()
                                    .unwrap_or(0)
                                    .saturating_add(1),
                            );
                            for document in &this.documents {
                                this.card_focus
                                    .entry(document.id)
                                    .or_insert_with(|| cx.focus_handle());
                            }
                            this.selected
                                .retain(|id| this.documents.iter().any(|doc| doc.id == *id));
                        }
                        if completed
                            && let Some((_, complete)) = this.pending_profile.take()
                            && (core.error().is_none() || committed)
                        {
                            complete(this, window, cx);
                        }
                    }
                }
                cx.notify();
            }),
        ];
        let stored = store::load(overrides, cx);
        let (documents, active) = stored.unwrap_or_default();
        let next_id = (documents.iter().map(|d| d.id).max().unwrap_or(0) + 1)
            .max(chrono::Utc::now().timestamp_millis().max(1) as u64);
        let desired_profile = active.filter(|id| documents.iter().any(|doc| doc.id == *id));
        let card_focus = documents
            .iter()
            .map(|doc| (doc.id, cx.focus_handle()))
            .collect();
        Self {
            list_scroll: gpui_kit::component::VirtualListScrollHandle::new(),
            card_focus,
            overrides,
            batch: false,
            selected: Default::default(),
            session,
            columns: 2,
            documents,
            next_id,
            desired_profile,
            core_revision: 0,
            saving: false,
            rejected: false,
            override_drop: None,
            import_task: None,
            url,
            import_token,
            import_agent,
            import_age,
            _subscriptions: subscriptions,
            pending_profile: None,
        }
    }
    pub(crate) fn reopen(&mut self, cx: &mut Context<Self>) {
        if !self.rejected || self.saving {
            return;
        }
        if let Some((documents, active)) = store::load(self.overrides, cx) {
            self.documents = documents;
            self.desired_profile = active;
        }
        self.rejected = false;
        cx.notify();
    }
    fn open_editor(
        &mut self,
        document: Document,
        content_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_editor(
            document,
            if content_only {
                EditorMode::Content
            } else {
                EditorMode::Information
            },
            window,
            cx,
        );
    }
    fn show_editor(
        &mut self,
        document: Document,
        mode: EditorMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let content_only = mode == EditorMode::Content;
        let title = if mode == EditorMode::Import {
            if self.overrides {
                "导入远程覆写".into()
            } else {
                "导入订阅".into()
            }
        } else if content_only {
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
        let import_inputs = (mode == EditorMode::Import).then(|| ImportInputs {
            url: self.url.clone(),
            token: self.import_token.clone(),
            agent: self.import_agent.clone(),
            age: self.import_age.clone(),
        });
        let editor = cx.new(|cx| {
            DocumentEditor::new(owner.clone(), document, mode, import_inputs, window, cx)
        });
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
                .footer(ui::dialog_footer_pending(
                    if mode == EditorMode::Import {
                        "导入"
                    } else {
                        "保存"
                    },
                    owner.upgrade().is_some_and(|owner| owner.read(cx).saving),
                ))
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
    fn remote_document(&mut self, cx: &App) -> Document {
        let url = self.url.read(cx).value().trim().to_owned();
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let kind = if self.overrides {
            if url
                .split('?')
                .next()
                .is_some_and(|path| path.ends_with(".js"))
            {
                DocumentKind::JavaScript
            } else {
                DocumentKind::Yaml
            }
        } else {
            DocumentKind::Profile
        };
        let mut document = Document::new(id, kind);
        document.name.clear();
        document.interval.clear();
        document.url = url;
        document.content.clear();
        document.self_proxy = crate::integration::read("settings", cx)
            .get("ProfileUseProxy")
            .is_some_and(|value| value == "true");
        if let Some(timeout) = crate::integration::read("settings", cx).get("SubscriptionTimeout") {
            document.timeout = timeout.clone();
        }
        document.token = self.import_token.read(cx).value().to_string();
        document.user_agent = self.import_agent.read(cx).value().to_string();
        document.age_key = self.import_age.read(cx).value().to_string();
        document.credentials = CredentialEdits::from_inputs(&document.token, &document.age_key);
        document
    }
    fn import_remote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.session.read(cx).core_busy() {
            ui::show_error("已有配置操作正在执行".into(), window, cx);
            return;
        }
        let url = self.url.read(cx).value().trim().to_owned();
        if url.is_empty() {
            self.import_dialog(window, cx);
            return;
        }
        if !valid_remote_url(&url) {
            ui::show_error("填写有效的 HTTP 或 HTTPS 地址".into(), window, cx);
            return;
        }
        let document = self.remote_document(cx);
        if let Err(error) = self.save_document(document, window, cx) {
            ui::show_error(error, window, cx);
        }
    }
    pub(crate) fn import_uri(&mut self, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.session.read(cx).core_busy() {
            ui::show_error("已有配置操作正在执行".into(), window, cx);
            return;
        }
        match crate::desktop::instance::parse_uri(uri) {
            Ok(import) => {
                self.url
                    .update(cx, |state, cx| state.set_value(import.url, window, cx));
                let mut document = self.remote_document(cx);
                if let Some(name) = import.name {
                    document.name = name;
                }
                if let Err(error) = self.save_document(document, window, cx) {
                    ui::show_error(error, window, cx);
                }
            }
            Err(error) => ui::show_error(error, window, cx),
        }
    }
    fn import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let document = self.remote_document(cx);
        self.show_editor(document, EditorMode::Import, window, cx);
    }
    fn import_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.import_task.is_some() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("导入 YAML 或 JavaScript 配置".into()),
        });
        self.import_task = Some(cx.spawn_in(window, async move |owner, cx| {
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
            let _ = owner.update(cx, |this, cx| {
                if let Some(task) = this.import_task.take() {
                    task.detach();
                }
                cx.notify();
            });
        }));
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
        let expected = crate::integration::read(store::key(self.overrides), cx);
        let command = if self.overrides {
            crate::core::Command::Overrides(crate::core::OverrideChange::Save {
                expected,
                documents,
            })
        } else {
            crate::core::Command::Profiles(crate::core::ProfileCommand::Save {
                expected,
                documents,
            })
        };
        let id = crate::core::submit(command, window, cx)?;
        self.saving = true;
        self.pending_profile = Some((id, Box::new(complete)));
        cx.notify();
        let _ = active;
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
        let imported =
            document.is_remote() && !self.documents.iter().any(|value| value.id == document.id);
        let submitted_url = document.url.clone();
        let mut documents = self.documents.clone();
        if let Some(existing) = documents.iter_mut().find(|d| d.id == document.id) {
            *existing = document;
        } else {
            documents.push(document);
        }
        let active = self.desired_profile;
        let submit_focus = window.focused(cx);
        self.persist_documents(documents, active, window, cx, move |this, window, cx| {
            let close = submit_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
                && editor.is_some_and(|(owner, revision)| {
                    owner
                        .upgrade()
                        .is_some_and(|editor| editor.read(cx).revision(cx) == revision)
                });
            if imported && this.url.read(cx).value().trim() == submitted_url {
                this.url
                    .update(cx, |state, cx| state.set_value("", window, cx));
                this.import_token
                    .update(cx, |state, cx| state.set_value("", window, cx));
                this.import_age
                    .update(cx, |state, cx| state.set_value("", window, cx));
            }
            if close {
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
            "分享二维码" => Self::share_qr(&document, window, cx),
            "复制链接" => cx.write_to_clipboard(ClipboardItem::new_string(document.url)),
            "订阅增强" => {
                enhance::open(document, 0, window, cx);
            }
            "编辑信息" => self.open_editor(document, false, window, cx),
            "编辑文件" => self.open_editor(document, true, window, cx),
            "更新" => self.update_profiles(vec![id], window, cx),
            "执行日志" => {
                let title = format!("执行日志 · {}", document.name);
                if let Err(error) = crate::core::execute(
                    crate::core::Command::ScriptLogs(id),
                    window,
                    cx,
                    move |result, window, cx| {
                        if result.is_ok() {
                            let text = crate::core::controller(cx)
                                .read(cx)
                                .snapshot()
                                .script_log(id)
                                .unwrap_or("暂无执行记录")
                                .to_owned();
                            ui::detail_dialog(title, text, window, cx);
                        }
                    },
                ) {
                    ui::show_error(error, window, cx);
                }
            }
            "打开文件" => {
                let path = match store::document_path(self.overrides, &document, cx) {
                    Ok(path) => path,
                    Err(error) => {
                        ui::show_error(error, window, cx);
                        return;
                    }
                };
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
                        crate::ui::notifications::push(
                            window,
                            gpui_kit::component::notification::Notification::new()
                                .message(format!("已移除“{}”", document.name))
                                .action(move |_, _, _| {
                                    let owner = owner.clone();
                                    Button::new("undo-delete")
                                        .icon(gpui_kit::assets::IconName::RotateCcw)
                                        .with_size(control::STANDARD)
                                        .ghost()
                                        .label("撤销")
                                        .on_click(move |_, window, cx| {
                                            owner.update(cx, |this, cx| {
                                                this.restore_deleted(vec![id], window, cx);
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
    fn restore_deleted(&mut self, ids: Vec<u64>, window: &mut Window, cx: &mut Context<Self>) {
        match crate::core::submit(
            if self.overrides {
                crate::core::Command::Overrides(crate::core::OverrideChange::Restore { ids })
            } else {
                crate::core::Command::Profiles(crate::core::ProfileCommand::Restore { ids })
            },
            window,
            cx,
        ) {
            Ok(id) => {
                self.saving = true;
                self.pending_profile = Some((id, Box::new(|_, _, _| {})));
                cx.notify();
            }
            Err(error) => ui::show_error(error, window, cx),
        }
    }
    fn update_profiles(&mut self, ids: Vec<u64>, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.session.read(cx).core_busy() {
            ui::show_error("已有配置操作正在执行".into(), window, cx);
            return;
        }
        if ids.is_empty() {
            return;
        }
        match crate::core::submit(
            if self.overrides {
                crate::core::Command::Overrides(crate::core::OverrideChange::Update {
                    ids,
                    automatic: false,
                })
            } else {
                crate::core::Command::Profiles(crate::core::ProfileCommand::Update {
                    ids,
                    automatic: false,
                })
            },
            window,
            cx,
        ) {
            Ok(id) => {
                self.saving = true;
                self.pending_profile = Some((id, Box::new(|_, _, _| {})));
                cx.notify();
            }
            Err(error) => ui::show_error(error, window, cx),
        }
    }
    fn select_profile(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.session.read(cx).core_busy() {
            return;
        }
        crate::core::request(crate::core::Command::Activate(id), window, cx);
    }
}
