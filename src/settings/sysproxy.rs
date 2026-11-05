//! One mode switch selects the applicable editor; fields retain their own drafts.
use super::{FieldControl, SettingsPage};
use crate::{
    ui,
    ui::theme::{control, layout, space},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Sizable,
        button::ButtonVariants,
        h_flex,
        input::Editor,
        scroll::ScrollableElement,
        switch::Switch,
        tab::{Tab, TabBar},
        v_flex,
    },
    *,
};
impl SettingsPage {
    pub(super) fn system_proxy_view(&self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let values = self.values(cx);
        let pac = values
            .get("SystemProxyModeIndex")
            .is_some_and(|v| v == "PAC");
        let busy = self.saving || self.session.read(cx).core_busy();
        let enabled = self.session.read(cx).system_proxy();
        let header = ui::workspace_header_control(
            "系统代理",
            "",
            Switch::new("system-proxy-enabled")
                .with_size(control::STANDARD)
                .checked(enabled == Some(true))
                .disabled(busy || enabled.is_none())
                .tooltip(if enabled.is_none() {
                    "系统代理状态未知"
                } else {
                    "启用系统代理"
                })
                .accessibility_label("启用系统代理")
                .on_change(cx.listener(|this, value, window, cx| {
                    this.toggle("IsSystemProxyEnabled", *value, window, cx)
                })),
            cx,
        )
        .child(
            TabBar::new("system-proxy-mode")
                .segmented()
                .with_size(control::STANDARD)
                .children([
                    Tab::new().label("手动").disabled(busy),
                    Tab::new().label("PAC").disabled(busy),
                ])
                .selected_index(usize::from(pac))
                .on_click(cx.listener(|this, index, window, cx| {
                    let mode = SharedString::from(if *index == 0 { "手动" } else { "PAC" });
                    if let FieldControl::Select(state) =
                        &this.fields["SystemProxyModeIndex"].control
                    {
                        state.update(cx, |state, cx| state.set_selected_value(&mode, window, cx));
                    }
                    this.commit_core_field("SystemProxyModeIndex", window, cx);
                    cx.notify();
                })),
        );
        let mut rows = vec![
            self.field_row("SystemProxyHost", cx),
            self.field_row("EnableProxyGuard", cx),
        ];
        if values.get("EnableProxyGuard").is_some_and(|v| v == "true") {
            rows.push(self.field_row("ProxyGuardDuration", cx));
        }
        if !pac {
            rows.push(self.field_row("UseDefaultBypass", cx));
            rows.push(self.field_row("EnableBypassCheck", cx));
        }
        let key = if pac { "PacScript" } else { "BypassList" };
        let field = &self.fields[key];
        let mut editor = v_flex().w_full().min_w(space::NONE).gap(space::SM);
        editor = editor.child(
            h_flex().justify_between().child(field.spec.label).child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        ui::icon_button(
                            "system-proxy-reset-editor",
                            IconName::RotateCcw,
                            "恢复默认",
                        )
                        .ghost()
                        .disabled(busy)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.restore_default_text(key, window, cx);
                            },
                        )),
                    )
                    .child(
                        ui::icon_button(
                            "system-proxy-expand-editor",
                            IconName::Maximize,
                            "展开编辑",
                        )
                        .disabled(busy)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if let FieldControl::Multiline(state) = &this.fields[key].control {
                                    ui::editor::expand(
                                        state.clone(),
                                        SharedString::from(this.fields[key].spec.label),
                                        false,
                                        window,
                                        cx,
                                    );
                                }
                            },
                        )),
                    ),
            ),
        );
        if let FieldControl::Multiline(state) = &field.control {
            editor = editor.child(
                Editor::new(state)
                    .w_full()
                    .h(layout::SETTINGS_EDITOR_HEIGHT)
                    .disabled(busy)
                    .aria_label(field.spec.label),
            );
        }
        editor = editor.child(ui::muted(field.spec.description, cx));
        ui::workspace().child(header).child(
            ui::workspace_body().child(
                div()
                    .id("system-proxy-scroll")
                    .size_full()
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            .gap(space::SECTION)
                            .py(space::SM)
                            .child(ui::preferences_group("常规", rows, cx))
                            .child(editor),
                    ),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FieldControl, SettingsKind, SettingsPage};
    use gpui_kit::{
        AppContext, Focusable, TestAppContext, WindowOptions,
        base::Root,
        component::WindowExt,
        test::{TestAppContextExt, TestQueryExt, TestWindowExt},
    };
    use std::{cell::RefCell, rc::Rc, time::Duration};
    #[gpui_kit::test]
    async fn modes_are_at_top_right_and_both_editors_share_the_expanded_draft(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::storage::Repository::open(crate::storage::AppPaths::isolated(root.path()))
                .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            cx.set_reduce_motion(true);
        });
        let retained = Rc::new(RefCell::new(None));
        let created = retained.clone();
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                let session = cx.new(crate::session::Session::new);
                let page =
                    cx.new(|cx| SettingsPage::new(SettingsKind::SysProxy, session, window, cx));
                *created.borrow_mut() = Some(page.clone());
                cx.new(|cx| Root::new(page, window, cx))
            })
            .unwrap()
        });
        let page = retained.borrow_mut().take().unwrap();
        cx.run_until_parked();
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| {
                    page.toggle("IsSystemProxyEnabled", true, window, cx);
                });
                window.render_frame(cx);
                // An unavailable executor rejects the request. The draft must
                // never masquerade as a confirmed OS state.
                assert!(page.read(cx).error.is_some());
                assert_eq!(window.find("system-proxy-enabled").checked(), Some(false));
                assert_eq!(window.find("system-proxy-enabled").disabled(), Some(true));
                assert!(
                    window.find("system-proxy-enabled").bounds().origin.x
                        < window.viewport_size().width / 2.
                );
            })
            .unwrap();
        for (index, label, key) in [(0usize, "手动", "BypassList"), (1, "PAC", "PacScript")] {
            cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
                !page.read(cx).saving && !page.read(cx).session.read(cx).core_busy()
            })
            .await;
            handle
                .update(cx, |_, window, cx| {
                    window.render_frame(cx);
                    window.within("system-proxy-mode").click(index, cx);
                    // Rejected segmented choices revert, just like the shared Select path.
                    assert_eq!(page.read(cx).values(cx)["SystemProxyModeIndex"], "手动");
                    // Select each presentation locally to inspect its retained editor binding.
                    if let FieldControl::Select(state) =
                        &page.read(cx).fields["SystemProxyModeIndex"].control
                    {
                        state.clone().update(cx, |state, cx| {
                            state.set_selected_value(
                                &gpui_kit::SharedString::from(label),
                                window,
                                cx,
                            )
                        });
                    }
                    let mode = window.within("system-proxy-mode").find(index);
                    assert!(mode.bounds().origin.x > window.viewport_size().width / 2.);
                    assert!(window.try_find("runtime-tabs").is_none());
                    assert!(window.try_find("runtime-save").is_none());
                })
                .unwrap();
            cx.wait_for(handle, Duration::from_secs(5), |_, cx| {
                !page.read(cx).saving
            })
            .await;
            handle
                .update(cx, |_, window, cx| {
                    window.render_frame(cx);
                    assert!(
                        !window
                            .find_all_by_label(page.read(cx).fields[key].spec.label)
                            .is_empty()
                    );
                    window.click("system-proxy-expand-editor", cx);
                    assert!(window.has_active_dialog(cx));
                    let state = match &page.read(cx).fields[key].control {
                        FieldControl::Multiline(state) => state.clone(),
                        _ => unreachable!(),
                    };
                    // Both presentations bind the retained editor entity, including focus.
                    state.read(cx).focus_handle(cx).focus(window, cx);
                    window.render_frame(cx);
                    let before = state.read(cx).value().to_string();
                    window.input(" ", cx);
                    assert_ne!(state.read(cx).value().as_ref(), before);
                    window.close_dialog(cx);
                    window.render_frame(cx);
                    assert!(!window.has_active_dialog(cx));
                    assert_eq!(
                        page.read(cx).values(cx)[key],
                        state.read(cx).value().as_ref()
                    );
                })
                .unwrap();
        }
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| {
                    if let FieldControl::Multiline(state) = &page.fields["BypassList"].control {
                        state.update(cx, |state, cx| state.set_value("undo.example", window, cx));
                    }
                    page.restore_default_text("BypassList", window, cx);
                });
            })
            .unwrap();
        cx.run_until_parked();
        handle
            .update(cx, |_, window, cx| {
                window.render_frame(cx);
                window.click("undo-action", cx);
                assert_eq!(page.read(cx).values(cx)["BypassList"], "undo.example");
            })
            .unwrap();
        cx.run_until_parked();
        cx.background_executor.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| {
                    page.defer_destructive("重置程序", window, cx)
                });
            })
            .unwrap();
        cx.run_until_parked();
        handle
            .update(cx, |_, window, cx| {
                assert!(page.read(cx).pending_action.is_some());
                window.render_frame(cx);
                window.click("undo-action", cx);
                assert!(page.read(cx).pending_action.is_none());
            })
            .unwrap();
        cx.background_executor
            .advance_clock(Duration::from_secs(11));
        cx.run_until_parked();
        assert!(root.path().join("config/settings/settings.json").is_file());
        crate::integration::finish_test_services(cx);
    }
}
