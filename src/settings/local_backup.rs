//! Retained local backup history; native pickers only choose paths, the actor owns files.
use crate::ui::theme::{control, layout, space};
use crate::{
    backup::local::Entry,
    core::{
        self,
        local_backup::{Command, State},
    },
    ui,
};
use gpui_kit::{
    component::{
        Disableable, Sizable, VirtualListScrollHandle, WindowExt, button::Button, h_flex, v_flex,
    },
    *,
};

pub(super) fn open(window: &mut Window, cx: &mut App) {
    if let Err(error) = core::execute(
        core::Command::LocalBackup(Command::List),
        window,
        cx,
        |result, window, cx| {
            if result.is_err() {
                return;
            }
            let state = core::controller(cx)
                .read(cx)
                .snapshot()
                .local_backup
                .clone();
            let history = cx.new(|_| History {
                state,
                busy: false,
                pending: None,
                picker: None,
                scroll: VirtualListScrollHandle::new(),
            });
            window.open_dialog(cx, move |dialog, window, cx| {
                ui::modal(dialog, "本地备份", layout::EDITOR_DIALOG_WIDTH, window, cx)
                    .content(ui::modal_content(
                        "local-backup-history",
                        layout::FORM_MAX_HEIGHT,
                        {
                            let history = history.clone();
                            move |_, _| history.clone().into_any_element()
                        },
                    ))
                    .footer(
                        ui::modal_footer().child(
                            Button::new("local-backup-done")
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
    picker: Option<Task<()>>,
    scroll: VirtualListScrollHandle,
}
impl History {
    fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let owner = cx.weak_entity();
        match core::execute(
            core::Command::LocalBackup(command),
            window,
            cx,
            move |_, _, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.busy = false;
                    this.state = core::controller(cx)
                        .read(cx)
                        .snapshot()
                        .local_backup
                        .clone();
                    if let Some(path) = &this.state.exported {
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
    fn import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            return;
        }
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("导入 Suki Clash 备份".into()),
        });
        self.picker = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = picker.await;
            let _ = owner.update_in(cx, |this, window, cx| {
                this.picker = None;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            this.run(Command::Import(path), window, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => ui::show_error(error.to_string(), window, cx),
                    Err(error) => ui::show_error(error.to_string(), window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn export(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            return;
        }
        let picker = cx.prompt_for_new_path(&std::env::temp_dir(), Some(entry.id.name()));
        self.picker = Some(cx.spawn_in(window, async move |owner, cx| {
            let result = picker.await;
            let _ = owner.update_in(cx, |this, window, cx| {
                this.picker = None;
                match result {
                    Ok(Ok(Some(target))) => this.run(
                        Command::Export {
                            id: entry.id,
                            target,
                        },
                        window,
                        cx,
                    ),
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => ui::show_error(error.to_string(), window, cx),
                    Err(error) => ui::show_error(error.to_string(), window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn row(&self, entry: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let export = entry.clone();
        let restore = entry.id.clone();
        let delete = entry.id.clone();
        let disabled = self.busy || self.pending.is_some() || self.picker.is_some();
        let date: chrono::DateTime<chrono::Local> = entry.modified.into();
        h_flex().w_full().gap(space::SM)
            .child(v_flex().flex_1().min_w(space::NONE).child(date.format("%Y-%m-%d %H:%M:%S %:z").to_string()).child(ui::muted(entry.id.name(),cx).truncate()))
            .child(Button::new(SharedString::from(format!("export-{}",entry.id.name()))).label("导出").disabled(disabled).on_click(cx.listener(move|this,_,window,cx|this.export(export.clone(),window,cx))))
            .child(Button::new(SharedString::from(format!("restore-{}",entry.id.name()))).label("恢复").disabled(disabled).on_click(cx.listener(move|_,_,window,cx|{
                let owner=cx.weak_entity();let restore=restore.clone();
                window.open_alert_dialog(cx,move|dialog,window,cx|{
                    let owner=owner.clone();let restore=restore.clone();
                    ui::modal_alert(dialog,window,cx).title("恢复此本地备份？").description("替换当前配置、订阅与覆写；本机同步设置保留。确认后等待 5 秒，期间可撤销。").footer(ui::dialog_footer("恢复备份")).on_ok(move|_,window,cx|{let _=owner.update(cx,|this,cx|this.defer(Command::Restore(restore.clone()),window,cx));true})
                });
            })))
            .child(ui::icon_button(SharedString::from(format!("delete-{}",entry.id.name())),gpui_kit::assets::IconName::Trash,"删除备份").disabled(disabled).on_click(cx.listener(move|this,_,window,cx|this.defer(Command::Delete(delete.clone()),window,cx))))
            .into_any_element()
    }
}
impl Render for History {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.busy || self.pending.is_some() || self.picker.is_some();
        v_flex()
            .w_full()
            .gap(space::SM)
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(space::SM)
                    .child(
                        Button::new("local-import")
                            .label("导入备份")
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| this.import(window, cx))),
                    )
                    .child(
                        Button::new("local-create")
                            .label("创建备份")
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run(Command::Create, window, cx)
                            })),
                    ),
            )
            .child(if self.state.entries.is_empty() {
                ui::empty_inline(
                    gpui_kit::assets::IconName::Archive,
                    "暂无本地备份",
                    "创建备份或导入已有备份",
                    cx,
                )
                .into_any_element()
            } else {
                ui::virtual_rows::panel(
                    cx.entity(),
                    "local-backup-rows",
                    vec![
                        crate::ui::theme::preferences::ROW_HEIGHT.to_pixels(window.rem_size());
                        self.state.entries.len()
                    ],
                    &self.scroll,
                    |this, index, _, cx| this.row(&this.state.entries[index].clone(), cx),
                )
                .into_any_element()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, History, State};
    use gpui_kit::component::VirtualListScrollHandle;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, TestAppContext, WindowOptions};

    #[gpui_kit::test]
    fn delete_undo_cancels_before_local_file_request(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        paths.create().unwrap();
        let id = crate::backup::local::store(&paths, "isolated backup").unwrap();
        let entries = crate::backup::local::list(&paths).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::ui::notifications::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, history) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                cx.new(|_| History {
                    state: State {
                        entries,
                        ..Default::default()
                    },
                    busy: false,
                    pending: None,
                    picker: None,
                    scroll: VirtualListScrollHandle::new(),
                })
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            history.update(cx, |this, cx| this.defer(Command::Delete(id), window, cx));
            assert!(history.read(cx).pending.is_some());
            window.render_frame(cx);
            window.click("undo-action", cx);
            assert!(history.read(cx).pending.is_none());
        })
        .unwrap();
        cx.background_executor
            .advance_clock(std::time::Duration::from_secs(6));
        cx.run_until_parked();
        assert_eq!(crate::backup::local::list(&paths).unwrap().len(), 1);
        cx.update(|cx| assert!(!history.read(cx).busy));
    }
}
