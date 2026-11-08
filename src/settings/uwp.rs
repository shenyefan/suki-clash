use crate::ui::theme::{layout, preferences, space};
use crate::{core, ui};
use gpui_kit::{
    component::{
        Disableable, Sizable, VirtualListScrollHandle, WindowExt,
        button::{Button, ButtonVariants},
        checkbox::Checkbox,
        input::{Input, InputEvent, InputState},
        v_flex,
    },
    *,
};
use std::collections::BTreeSet;
use suki_service::windows_network::{Request, UwpApp};
pub(super) fn open(window: &mut Window, cx: &mut App) {
    if let Err(error) = core::execute(
        core::Command::WindowsNetwork(Request::List),
        window,
        cx,
        |result, window, cx| {
            if result.is_err() {
                return;
            }
            let apps = core::controller(cx).read(cx).snapshot().uwp_apps().to_vec();
            Editor::show(apps, window, cx);
        },
    ) {
        ui::show_error(error, window, cx)
    }
}
struct Editor {
    apps: Vec<UwpApp>,
    selected: BTreeSet<String>,
    expected: Vec<String>,
    query: Entity<InputState>,
    visible: Vec<usize>,
    scroll: VirtualListScrollHandle,
    busy: bool,
    open: bool,
    _query: Subscription,
}
impl Editor {
    fn show(apps: Vec<UwpApp>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let editor = cx.new(|cx| Editor::new(apps, window, cx));
        let shown = editor.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            let save = editor.clone();
            let closed = editor.downgrade();
            ui::modal(
                dialog,
                "UWP 应用代理",
                layout::EDITOR_DIALOG_WIDTH,
                window,
                cx,
            )
            .on_close(move |_, _, cx| {
                let _ = closed.update(cx, |this, _| this.open = false);
            })
            .content(ui::modal_content("uwp-content", layout::FORM_MAX_HEIGHT, {
                let editor = editor.clone();
                move |_, _| editor.clone().into_any_element()
            }))
            .footer(
                ui::modal_footer()
                    .child(
                        Button::new("uwp-cancel")
                            .icon(gpui_kit::assets::IconName::X)
                            .label("取消")
                            .with_size(ui::theme::control::STANDARD)
                            .on_click({
                                let editor = editor.downgrade();
                                move |_, window, cx| {
                                    let _ = editor.update(cx, |this, _| this.open = false);
                                    window.close_dialog(cx);
                                }
                            }),
                    )
                    .child(
                        Button::new("uwp-save")
                            .icon(gpui_kit::assets::IconName::Check)
                            .label("保存")
                            .primary()
                            .with_size(ui::theme::control::STANDARD)
                            .on_click(move |_, window, cx| {
                                save.update(cx, |this, cx| this.save(window, cx))
                            }),
                    ),
            )
        });
        shown
    }
    fn saved(&mut self, succeeded: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = false;
        if !succeeded {
            self.selected = self.expected.iter().cloned().collect();
        }
        if succeeded && self.open {
            self.open = false;
            window.close_dialog(cx);
        }
        cx.notify();
    }
    fn new(apps: Vec<UwpApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let selected = apps
            .iter()
            .filter(|app| app.exempt)
            .map(|app| app.sid.clone())
            .collect::<BTreeSet<_>>();
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索应用"));
        let subscription = cx.subscribe(&query, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                let query = this.query.read(cx).value().to_lowercase();
                this.visible = this
                    .apps
                    .iter()
                    .enumerate()
                    .filter(|(_, app)| {
                        format!("{} {}", app.name, app.package)
                            .to_lowercase()
                            .contains(&query)
                    })
                    .map(|(ix, _)| ix)
                    .collect();
                cx.notify()
            }
        });
        Self {
            visible: (0..apps.len()).collect(),
            apps,
            expected: selected.iter().cloned().collect(),
            selected,
            query,
            scroll: VirtualListScrollHandle::new(),
            busy: false,
            open: true,
            _query: subscription,
        }
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let command = core::Command::WindowsNetwork(Request::SetLoopback {
            expected: self.expected.clone(),
            selected: self.selected.iter().cloned().collect(),
        });
        let owner = cx.weak_entity();
        match core::execute(command, window, cx, move |result, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.saved(result.is_ok(), window, cx);
            });
        }) {
            Ok(_) => {
                self.busy = true;
                cx.notify()
            }
            Err(error) => {
                self.saved(false, window, cx);
                ui::show_error(error, window, cx);
            }
        }
    }
}
impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let height = preferences::ROW_HEIGHT.to_pixels(window.rem_size());
        let all = !self.visible.is_empty()
            && self
                .visible
                .iter()
                .all(|ix| self.selected.contains(&self.apps[*ix].sid));
        v_flex()
            .gap(space::MD)
            .child(
                Input::new(&self.query)
                    .w_full()
                    .disabled(self.busy)
                    .aria_label("搜索 UWP 应用"),
            )
            .child(
                gpui_kit::component::h_flex()
                    .justify_between()
                    .child(
                        Checkbox::new("uwp-all")
                            .label("全选")
                            .checked(all)
                            .disabled(self.busy || self.visible.is_empty())
                            .on_change(cx.listener(|this, checked, _, cx| {
                                for ix in &this.visible {
                                    let sid = &this.apps[*ix].sid;
                                    if *checked {
                                        this.selected.insert(sid.clone());
                                    } else {
                                        this.selected.remove(sid);
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(ui::muted(format!("已选择 {} 个", self.selected.len()), cx)),
            )
            .child(
                div()
                    .h(layout::DIALOG_FORM_HEIGHT)
                    .w_full()
                    .child(ui::virtual_rows::rows(
                        cx.entity(),
                        "uwp-rows",
                        vec![height; self.visible.len()],
                        &self.scroll,
                        |this, ix, _, cx| {
                            let app = &this.apps[this.visible[ix]];
                            let sid = app.sid.clone();
                            div()
                                .px(space::SM)
                                .size_full()
                                .flex()
                                .items_center()
                                .child(
                                    Checkbox::new(SharedString::from(sid.clone()))
                                        .label(app.name.clone())
                                        .checked(this.selected.contains(&sid))
                                        .disabled(this.busy)
                                        .on_change(cx.listener(move |this, checked, _, cx| {
                                            if *checked {
                                                this.selected.insert(sid.clone());
                                            } else {
                                                this.selected.remove(&sid);
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    ui::muted(app.package.clone(), cx)
                                        .text_size(ui::theme::text::CAPTION)
                                        .truncate(),
                                )
                                .gap(space::SM)
                                .into_any_element()
                        },
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::Editor;
    use gpui_kit::{
        AppContext as _, Context, IntoElement, ParentElement, Render, TestAppContext, Window,
        WindowOptions,
        component::{WindowExt, button::Button},
        div,
        test::TestWindowExt as _,
    };
    struct Host;
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    #[gpui_kit::test]
    fn late_uwp_success_cannot_close_a_newer_dialog(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        let old = cx
            .update_window(handle, |_, window, cx| {
                let editor = Editor::show(Vec::new(), window, cx);
                editor.update(cx, |this, _| this.busy = true);
                window.render_frame(cx);
                window.click("uwp-cancel", cx);
                assert!(!editor.read(cx).open);
                assert!(!window.has_active_dialog(cx));
                window.open_dialog(cx, |dialog, _, _| {
                    dialog.child(Button::new("newer-dialog").label("新弹窗"))
                });
                editor.update(cx, |this, cx| this.saved(true, window, cx));
                window.render_frame(cx);
                assert!(window.has_active_dialog(cx));
                assert!(window.find("newer-dialog").visible());
                assert!(!editor.read(cx).busy);
                editor
            })
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            // The retained old entity still cannot affect a later retry completion.
            old.update(cx, |this, cx| this.saved(true, window, cx));
            assert!(window.has_active_dialog(cx));
        })
        .unwrap();
    }
    #[gpui_kit::test]
    fn normal_uwp_success_closes_its_editor_and_dismissal_marks_it_closed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| Host)).unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            let editor = Editor::show(Vec::new(), window, cx);
            editor.update(cx, |this, cx| {
                this.busy = true;
                this.expected = vec!["confirmed-sid".into()];
                this.selected.insert("rejected-sid".into());
                this.query.update(cx, |state, cx| {
                    state.set_value("retained query", window, cx)
                });
                this.saved(false, window, cx);
                assert_eq!(this.selected, this.expected.iter().cloned().collect());
                assert_eq!(this.query.read(cx).value().as_ref(), "retained query");
            });
            assert!(window.has_active_dialog(cx));
            assert!(editor.read(cx).open);
            assert!(!editor.read(cx).busy);
            editor.update(cx, |this, cx| {
                this.busy = true;
                this.saved(true, window, cx);
            });
            assert!(!window.has_active_dialog(cx));
            assert!(!editor.read(cx).open);
            let dismissed = Editor::show(Vec::new(), window, cx);
            window.render_frame(cx);
            window.press("escape", cx);
            assert!(!dismissed.read(cx).open);
        })
        .unwrap();
    }
}
