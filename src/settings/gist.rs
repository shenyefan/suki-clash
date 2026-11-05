//! Gist backup controls. Drafts belong to their dialogs; all disk/network work uses the actor.
use crate::core::gist::{Command, Entry, Settings, State};
use crate::ui::theme::{control, layout, space};
use crate::{core, profiles::CredentialEdits, ui};
use gpui_kit::{
    component::{
        Disableable, Sizable, VirtualListScrollHandle, WindowExt,
        button::Button,
        h_flex,
        input::{Input, InputEvent, InputState},
        switch::Switch,
        v_flex,
    },
    *,
};

pub(super) fn configure(window: &mut Window, cx: &mut App) {
    if let Err(error) = core::execute(
        core::Command::Gist(Command::Inspect),
        window,
        cx,
        |result, window, cx| {
            if result.is_err() {
                return;
            }
            let state = core::controller(cx).read(cx).snapshot().gist.clone();
            let editor = cx.new(|cx| Configuration::new(state, window, cx));
            window.open_dialog(cx, move |dialog, window, cx| {
                ui::modal(
                    dialog,
                    "Gist 备份配置",
                    layout::EDITOR_DIALOG_WIDTH,
                    window,
                    cx,
                )
                .content(ui::modal_content(
                    "gist-configuration",
                    layout::FORM_MAX_HEIGHT,
                    {
                        let editor = editor.clone();
                        move |_, _| editor.clone().into_any_element()
                    },
                ))
                .footer(
                    ui::modal_footer().child(
                        Button::new("gist-done")
                            .label("完成")
                            .with_size(control::STANDARD)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
            });
        },
    ) {
        ui::show_error(error, window, cx);
    }
}
struct Configuration {
    token: Entity<InputState>,
    key: Entity<InputState>,
    recipient: Entity<InputState>,
    encrypt: bool,
    expected: Settings,
    token_saved: bool,
    key_saved: bool,
    busy: bool,
    pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}
impl Configuration {
    fn new(state: State, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("留空保留已保存的 Token")
        });
        let key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("留空保留已保存的私钥")
        });
        let recipient = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(state.settings.recipient.clone())
                .placeholder("Age 接收方公钥")
        });
        let subscriptions = [&token, &key, &recipient]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(input, window, |this, _, event, window, cx| {
                    if matches!(event, InputEvent::Blur) {
                        this.commit(window, cx);
                    }
                })
            })
            .collect();
        Self {
            token,
            key,
            recipient,
            encrypt: state.settings.encrypt,
            expected: state.settings,
            token_saved: state.token_saved,
            key_saved: state.key_saved,
            busy: false,
            pending: None,
            _subscriptions: subscriptions,
        }
    }
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submit(None, window, cx);
    }
    fn submit(
        &mut self,
        edits: Option<CredentialEdits>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy || self.pending.is_some() {
            return;
        }
        let settings = Settings {
            encrypt: self.encrypt,
            recipient: self.recipient.read(cx).value().trim().into(),
        };
        let credentials = edits.unwrap_or_else(|| {
            CredentialEdits::from_inputs(
                self.token.read(cx).value().as_ref(),
                self.key.read(cx).value().as_ref(),
            )
        });
        if settings == self.expected && !credentials.changed() {
            return;
        }
        let owner = cx.weak_entity();
        match core::execute(
            core::Command::Gist(Command::Configure {
                expected: self.expected.clone(),
                settings: settings.clone(),
                credentials,
            }),
            window,
            cx,
            move |result, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.busy = false;
                    if result.is_ok() {
                        let state = core::controller(cx).read(cx).snapshot().gist.clone();
                        this.expected = settings;
                        this.token_saved = state.token_saved;
                        this.key_saved = state.key_saved;
                        this.token
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        this.key
                            .update(cx, |input, cx| input.set_value("", window, cx));
                    } else {
                        this.encrypt = this.expected.encrypt;
                    }
                    cx.notify();
                });
            },
        ) {
            Ok(_) => self.busy = true,
            Err(error) => {
                self.encrypt = self.expected.encrypt;
                ui::show_error(error, window, cx);
            }
        }
        cx.notify();
    }
    fn clear(&mut self, token: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.pending.is_some() {
            return;
        }
        let owner = cx.weak_entity();
        ui::notifications::undo(
            window,
            if token {
                "5 秒后清除 GitHub Token"
            } else {
                "5 秒后清除 Age 私钥"
            },
            cx,
            move |_, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.pending = None;
                    cx.notify();
                });
                true
            },
        );
        self.pending = Some(cx.spawn_in(window, async move |owner, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(5))
                .await;
            let _ = owner.update_in(cx, |this, window, cx| {
                this.pending = None;
                let mut edits = CredentialEdits::default();
                if token {
                    edits.token = crate::profiles::SecretEdit::Clear;
                } else {
                    edits.age = crate::profiles::SecretEdit::Clear;
                }
                this.submit(Some(edits), window, cx);
            });
        }));
        cx.notify();
    }
}
impl Render for Configuration {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        ui::form()
            .child(ui::field_label(
                if self.token_saved {
                    "GitHub Token · 已保存"
                } else {
                    "GitHub Token"
                },
                Input::new(&self.token)
                    .mask_toggle()
                    .disabled(self.busy || self.pending.is_some())
                    .w_full()
                    .aria_label("GitHub Token")
                    .suffix(
                        ui::icon_button(
                            "gist-token-clear",
                            gpui_kit::assets::IconName::Trash,
                            "清除令牌",
                        )
                        .disabled(self.busy || self.pending.is_some() || !self.token_saved)
                        .on_click(cx.listener(|this, _, window, cx| this.clear(true, window, cx))),
                    ),
            ))
            .child(ui::muted(
                "Token 需要 gist 权限；凭据保存在系统安全存储中",
                cx,
            ))
            .child(ui::field_label(
                "Age 接收方公钥",
                Input::new(&self.recipient)
                    .disabled(self.busy || self.pending.is_some())
                    .w_full()
                    .aria_label("Age 接收方公钥"),
            ))
            .child(ui::field_label(
                if self.key_saved {
                    "Age 私钥 · 已保存"
                } else {
                    "Age 私钥"
                },
                Input::new(&self.key)
                    .mask_toggle()
                    .disabled(self.busy || self.pending.is_some())
                    .w_full()
                    .aria_label("Age 私钥")
                    .suffix(
                        h_flex()
                            .gap(space::XS)
                            .child(
                                ui::icon_button(
                                    "gist-age-generate",
                                    gpui_kit::assets::IconName::KeyRound,
                                    "生成密钥",
                                )
                                .disabled(self.busy || self.pending.is_some())
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        use age::secrecy::ExposeSecret as _;
                                        let key = age::x25519::Identity::generate();
                                        this.recipient.update(cx, |input, cx| {
                                            input.set_value(key.to_public().to_string(), window, cx)
                                        });
                                        this.key.update(cx, |input, cx| {
                                            input.set_value(
                                                key.to_string().expose_secret().to_owned(),
                                                window,
                                                cx,
                                            )
                                        });
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                ui::icon_button(
                                    "gist-key-clear",
                                    gpui_kit::assets::IconName::Trash,
                                    "清除私钥",
                                )
                                .disabled(self.busy || self.pending.is_some() || !self.key_saved)
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.clear(false, window, cx)
                                    }),
                                ),
                            ),
                    ),
            ))
            .child(ui::muted(
                "未加密备份包含代理配置；请单独保管 Age 私钥。Token、私钥和本机同步设置不入备份",
                cx,
            ))
            .child(
                ui::form_row()
                    .items_center()
                    .justify_between()
                    .child("加密备份")
                    .child(
                        Switch::new("gist-encrypt")
                            .with_size(control::STANDARD)
                            .accessibility_label("加密备份")
                            .checked(self.encrypt)
                            .disabled(self.busy || self.pending.is_some())
                            .on_change(cx.listener(|this, enabled, window, cx| {
                                this.encrypt = *enabled;
                                this.commit(window, cx);
                            })),
                    ),
            )
    }
}

pub(super) fn history(window: &mut Window, cx: &mut App) {
    if let Err(error) = core::execute(
        core::Command::Gist(Command::List { page: 1 }),
        window,
        cx,
        |result, window, cx| {
            if result.is_err() {
                return;
            }
            let state = core::controller(cx).read(cx).snapshot().gist.clone();
            let history = cx.new(|_| History {
                state,
                busy: false,
                pending: None,
                scroll: VirtualListScrollHandle::new(),
            });
            window.open_dialog(cx, move |dialog, window, cx| {
                ui::modal(
                    dialog,
                    "Gist 备份历史",
                    layout::EDITOR_DIALOG_WIDTH,
                    window,
                    cx,
                )
                .content(ui::modal_content(
                    "gist-history",
                    layout::FORM_MAX_HEIGHT,
                    {
                        let history = history.clone();
                        move |_, _| history.clone().into_any_element()
                    },
                ))
                .footer(
                    ui::modal_footer().child(
                        Button::new("gist-history-done")
                            .label("完成")
                            .with_size(control::STANDARD)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
            });
        },
    ) {
        ui::show_error(error, window, cx);
    }
}
struct History {
    state: State,
    busy: bool,
    pending: Option<Task<()>>,
    scroll: VirtualListScrollHandle,
}
impl History {
    fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let owner = cx.weak_entity();
        match core::execute(
            core::Command::Gist(command),
            window,
            cx,
            move |result, _, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.busy = false;
                    // A committed restore/delete remains visible even if a later step failed.
                    let state = core::controller(cx).read(cx).snapshot().gist.clone();
                    this.state = state;
                    if result.is_ok()
                        && let Some(path) = &this.state.downloaded
                    {
                        cx.reveal_path(path);
                    }
                    cx.notify();
                });
            },
        ) {
            Ok(_) => self.busy = true,
            Err(error) => ui::show_error(error, window, cx),
        }
        cx.notify();
    }
    fn defer(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.pending.is_some() {
            return;
        }
        let message = if matches!(command, Command::Restore(_)) {
            "5 秒后恢复备份"
        } else {
            "5 秒后删除备份"
        };
        let owner = cx.weak_entity();
        ui::notifications::undo(window, message, cx, move |_, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.pending = None;
                cx.notify();
            });
            true
        });
        self.pending = Some(cx.spawn_in(window, async move |owner, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(5))
                .await;
            let _ = owner.update_in(cx, |this, window, cx| {
                this.pending = None;
                this.run(command, window, cx);
            });
        }));
        cx.notify();
    }
    fn row(&self, entry: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let download = entry.id.clone();
        let restore = entry.id.clone();
        let delete = entry.id.clone();
        let disabled = self.busy || self.pending.is_some();
        h_flex().w_full().gap(space::SM)
            .child(v_flex().flex_1().min_w(space::NONE).child(entry.created_at.clone()).child(ui::muted(entry.description.clone(), cx).truncate()))
            .child(ui::icon_button(SharedString::from(format!("gist-download-{}", entry.id)), gpui_kit::assets::IconName::Download, "下载备份").disabled(disabled).on_click(cx.listener(move |this, _, window, cx| this.run(Command::Download(download.clone()), window, cx))))
            .child(Button::new(SharedString::from(format!("gist-restore-{}", entry.id))).outline().label("恢复备份").disabled(disabled).on_click(cx.listener(move |_, _, window, cx| {
                let owner = cx.weak_entity(); let restore = restore.clone();
                window.open_alert_dialog(cx, move |dialog, window, cx| {
                    let owner = owner.clone(); let restore = restore.clone();
                    ui::modal_alert(dialog, window, cx).title("恢复此 Gist 备份？").description("替换当前配置、订阅与覆写；Gist凭据和本机同步设置保留；换源的订阅鉴权需重新填写。确认后等待 5 秒，期间可撤销。").footer(ui::dialog_footer("恢复备份")).on_ok(move |_, window, cx| { let _ = owner.update(cx, |this, cx| this.defer(Command::Restore(restore.clone()), window, cx)); true })
                });
            })))
            .child(ui::icon_button(SharedString::from(format!("gist-delete-{}", entry.id)), gpui_kit::assets::IconName::Trash, "删除备份").disabled(disabled).on_click(cx.listener(move |this, _, window, cx| this.defer(Command::Delete(delete.clone()), window, cx))))
            .into_any_element()
    }
}
impl Render for History {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.busy || self.pending.is_some();
        v_flex()
            .gap(space::MD)
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        Button::new("gist-history-refresh")
                            .outline()
                            .label("刷新")
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run(
                                    Command::List {
                                        page: this.state.page.max(1),
                                    },
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("gist-history-previous")
                            .outline()
                            .label("上一页")
                            .disabled(disabled || self.state.page <= 1)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run(
                                    Command::List {
                                        page: this.state.page - 1,
                                    },
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("gist-history-next")
                            .outline()
                            .label("下一页")
                            .disabled(disabled || !self.state.has_more)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run(
                                    Command::List {
                                        page: this.state.page + 1,
                                    },
                                    window,
                                    cx,
                                )
                            })),
                    ),
            )
            .child(if self.state.entries.is_empty() {
                ui::muted("暂无 Gist 备份", cx).into_any_element()
            } else {
                let height = crate::ui::theme::preferences::ROW_HEIGHT.to_pixels(window.rem_size());
                ui::virtual_rows::panel(
                    cx.entity(),
                    "gist-history-rows",
                    vec![height; self.state.entries.len()],
                    &self.scroll,
                    |this, ix, _, cx| this.row(&this.state.entries[ix].clone(), cx),
                )
                .into_any_element()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{Configuration, History};
    use crate::core::gist::{Entry, State};
    use gpui_kit::component::{VirtualListScrollHandle, WindowExt};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Context, IntoElement, ParentElement, Render, TestAppContext, Window,
        WindowOptions, div,
    };
    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    #[gpui_kit::test]
    fn configuration_generates_matching_draft_keys_and_clear_can_be_undone(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::notifications::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, editor) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    Configuration::new(
                        State {
                            token_saved: true,
                            ..Default::default()
                        },
                        window,
                        cx,
                    )
                })
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("gist-age-generate").visible());
            window.click("gist-age-generate", cx);
            let identity = editor
                .read(cx)
                .key
                .read(cx)
                .value()
                .parse::<age::x25519::Identity>()
                .unwrap();
            assert_eq!(
                identity.to_public().to_string(),
                editor.read(cx).recipient.read(cx).value().as_ref()
            );
            assert!(editor.read(cx).token.read(cx).value().is_empty());
            assert!(!editor.read(cx).busy); // Generating is a draft action, no credential write.
            editor.update(cx, |this, cx| this.clear(true, window, cx));
            assert!(editor.read(cx).pending.is_some());
            window.render_frame(cx);
            window.click("undo-action", cx);
            assert!(editor.read(cx).pending.is_none());
        })
        .unwrap();
        cx.background_executor
            .advance_clock(std::time::Duration::from_secs(6));
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            assert!(editor.read(cx).pending.is_none());
            assert!(!editor.read(cx).busy);
        })
        .unwrap();
    }
    #[gpui_kit::test]
    fn history_restore_uses_confirmation_and_delete_undo_cancels_before_request(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::notifications::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        let history = cx
            .update_window(handle, |_, window, cx| {
                let history = cx.new(|_| History {
                    state: State {
                        entries: vec![Entry {
                            id: "abc123".into(),
                            created_at: "2026-11-03T00:00:00Z".into(),
                            description: "检查备份".into(),
                        }],
                        page: 1,
                        ..Default::default()
                    },
                    busy: false,
                    pending: None,
                    scroll: VirtualListScrollHandle::new(),
                });
                let content = history.clone();
                window.open_dialog(cx, move |dialog, _, _| dialog.child(content.clone()));
                window.render_frame(cx);
                window.click("gist-restore-abc123", cx);
                window.render_frame(cx);
                assert!(window.within("dialog-1").find("dialog-commit").visible());
                window.click("dialog-cancel", cx);
                assert!(history.read(cx).pending.is_none());
                window.render_frame(cx);
                window.click("gist-delete-abc123", cx);
                assert!(history.read(cx).pending.is_some());
                window.render_frame(cx);
                window.click("undo-action", cx);
                assert!(history.read(cx).pending.is_none());
                history
            })
            .unwrap();
        cx.background_executor
            .advance_clock(std::time::Duration::from_secs(6));
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            assert!(!history.read(cx).busy);
            assert_eq!(history.read(cx).state.entries.len(), 1);
        })
        .unwrap();
    }
}
