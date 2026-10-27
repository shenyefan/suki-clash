use super::{
    ProxiesPage,
    chain_config::{Change, Config},
};
use crate::{
    ui,
    ui::theme::{control, layout, space},
};
use gpui_kit::{
    component::{
        Disableable, IndexPath, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Editor, EditorState},
        select::{Select, SelectState},
        switch::Switch,
    },
    *,
};
impl ProxiesPage {
    pub(super) fn edit_chain(&self, window: &mut Window, cx: &mut Context<Self>) {
        let runtime = self.session.read(cx).runtime();
        let nodes = runtime
            .proxies
            .entries
            .values()
            .filter(|proxy| {
                proxy.all.is_none()
                    && !matches!(
                        proxy.name.as_str(),
                        "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS" | "COMPATIBLE"
                    )
            })
            .map(|proxy| SharedString::from(proxy.name.clone()))
            .collect();
        let groups = runtime
            .proxies
            .order
            .iter()
            .filter(|name| {
                runtime
                    .proxies
                    .entries
                    .get(*name)
                    .is_some_and(|proxy| proxy.kind == "Selector")
            })
            .cloned()
            .collect();
        let editor = cx.new(|cx| ChainEditor::new(self.session.clone(), nodes, groups, window, cx));
        let footer = cx.new(|cx| ChainFooter::new(editor.clone(), cx));
        window.open_dialog(cx, move |dialog, window, cx| {
            let body = editor.clone();
            ui::modal(dialog, "链式代理", layout::EDITOR_DIALOG_WIDTH, window, cx)
                .content(ui::modal_content(
                    "proxy-chain-body",
                    layout::FORM_MAX_HEIGHT,
                    move |_, _| body.clone().into_any_element(),
                ))
                .footer(footer.clone())
        });
    }
}
struct ChainEditor {
    session: Entity<crate::session::Session>,
    expected: Config,
    enabled: bool,
    generation: u64,
    epoch: u64,
    pending: bool,
    entry: Entity<SelectState<Vec<SharedString>>>,
    exit: Entity<SelectState<Vec<SharedString>>>,
    group: Entity<SelectState<Vec<SharedString>>>,
    source: Entity<EditorState>,
    _subscriptions: Vec<Subscription>,
}
impl ChainEditor {
    fn new(
        session: Entity<crate::session::Session>,
        mut nodes: Vec<SharedString>,
        mut groups: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let expected = session.read(cx).core().chain().clone();
        let runtime = session.read(cx).runtime();
        for name in &expected.nodes {
            if !nodes.iter().any(|node| node.as_ref() == name) {
                nodes.push(name.clone().into());
            }
        }
        if !expected.group.is_empty() && !groups.contains(&expected.group) {
            groups.push(expected.group.clone());
        }
        let entry_ix = expected
            .nodes
            .first()
            .and_then(|name| nodes.iter().position(|node| node.as_ref() == name))
            .or_else(|| (!nodes.is_empty()).then_some(0));
        let exit_ix = expected
            .nodes
            .last()
            .and_then(|name| nodes.iter().position(|node| node.as_ref() == name))
            .or_else(|| (nodes.len() > 1).then_some(1));
        let target = if session.read(cx).mode() == Some(crate::session::ProxyMode::Global) {
            "GLOBAL"
        } else {
            expected.group.as_str()
        };
        let group_ix = groups
            .iter()
            .position(|name| name == target)
            .or_else(|| (!groups.is_empty()).then_some(0));
        let entry =
            cx.new(|cx| SelectState::new(nodes.clone(), entry_ix.map(IndexPath::new), window, cx));
        let exit = cx.new(|cx| SelectState::new(nodes, exit_ix.map(IndexPath::new), window, cx));
        let group = cx.new(|cx| {
            SelectState::new(
                groups
                    .into_iter()
                    .map(SharedString::from)
                    .collect::<Vec<_>>(),
                group_ix.map(IndexPath::new),
                window,
                cx,
            )
        });
        let source =
            cx.new(|cx| ui::editor::state("yaml", expected.custom.clone(), true, window, cx));
        let subscriptions = vec![
            cx.observe(&entry, |_, _, cx| cx.notify()),
            cx.observe(&exit, |_, _, cx| cx.notify()),
            cx.observe(&group, |_, _, cx| cx.notify()),
            cx.observe(&session, |_, _, cx| cx.notify()),
        ];
        Self {
            session,
            enabled: expected.enabled,
            expected,
            generation: runtime.generation,
            epoch: runtime.epoch,
            pending: false,
            entry,
            exit,
            group,
            source,
            _subscriptions: subscriptions,
        }
    }
    fn save(&mut self, disconnect: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        let node = |state: &Entity<SelectState<Vec<SharedString>>>| {
            state
                .read(cx)
                .selected_value()
                .map(ToString::to_string)
                .unwrap_or_default()
        };
        let desired = if disconnect {
            Config {
                enabled: false,
                ..self.expected.clone()
            }
        } else {
            Config {
                enabled: self.enabled,
                group: if self.session.read(cx).mode() == Some(crate::session::ProxyMode::Global) {
                    "GLOBAL".into()
                } else {
                    node(&self.group)
                },
                nodes: [node(&self.entry), node(&self.exit)]
                    .into_iter()
                    .filter(|name| !name.is_empty())
                    .collect(),
                custom: self.source.read(cx).value().to_string(),
            }
        };
        if let Err(error) = desired.validate() {
            ui::show_error(error, window, cx);
            return;
        }
        let owner = cx.weak_entity();
        let result = crate::core::execute(
            crate::core::Command::Chain(Change {
                expected: self.expected.clone(),
                desired,
                generation: self.generation,
                epoch: self.epoch,
            }),
            window,
            cx,
            move |result, window, cx| {
                if let Some(owner) = owner.upgrade() {
                    owner.update(cx, |editor, cx| {
                        editor.pending = false;
                        cx.notify();
                    });
                }
                if result.is_ok() {
                    window.close_dialog(cx);
                }
            },
        );
        match result {
            Ok(()) => {
                self.pending = true;
                cx.notify();
            }
            Err(error) => ui::show_error(error, window, cx),
        }
    }
}
impl Render for ChainEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.pending || self.session.read(cx).core_busy();
        ui::form()
            .child(
                ui::form_row()
                    .justify_between()
                    .child("启用链式代理")
                    .child(
                        Switch::new("chain-enabled")
                            .with_size(control::STANDARD)
                            .checked(self.enabled)
                            .disabled(busy)
                            .accessibility_label("启用链式代理")
                            .on_change(cx.listener(|this, value, _, cx| {
                                this.enabled = *value;
                                cx.notify();
                            })),
                    ),
            )
            .child(ui::field_label(
                "目标代理组",
                Select::new(&self.group)
                    .w_full()
                    .with_size(control::STANDARD)
                    .disabled(busy)
                    .accessibility_label("目标代理组"),
            ))
            .child(
                ui::form_row()
                    .child(
                        div().flex_1().min_w(space::NONE).child(ui::field_label(
                            "入口节点",
                            Select::new(&self.entry)
                                .w_full()
                                .with_size(control::STANDARD)
                                .disabled(busy)
                                .accessibility_label("入口节点"),
                        )),
                    )
                    .child(
                        div().flex_1().min_w(space::NONE).child(ui::field_label(
                            "出口节点",
                            Select::new(&self.exit)
                                .w_full()
                                .with_size(control::STANDARD)
                                .disabled(busy)
                                .accessibility_label("出口节点"),
                        )),
                    ),
            )
            .child(ui::muted("本机 → 入口 → 出口 → 目标", cx))
            .child(ui::field_label(
                "自定义链式配置",
                Editor::new(&self.source)
                    .w_full()
                    .h(layout::DIALOG_COMPACT_HEIGHT)
                    .disabled(busy)
                    .aria_label("链式代理 YAML"),
            ))
    }
}
struct ChainFooter {
    editor: Entity<ChainEditor>,
    _subscription: Subscription,
}
impl ChainFooter {
    fn new(editor: Entity<ChainEditor>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&editor, |_, _, cx| cx.notify());
        Self {
            editor,
            _subscription: subscription,
        }
    }
}
impl Render for ChainFooter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::prelude::FluentBuilder as _;
        let editor = self.editor.read(cx);
        let pending = editor.pending;
        let busy = pending || editor.session.read(cx).core_busy();
        let connected = editor.expected.enabled;
        let disconnect = self.editor.clone();
        let save = self.editor.clone();
        h_flex()
            .w_full()
            .gap(space::SM)
            .when(connected, |row| {
                row.child(
                    Button::new("disconnect-chain")
                        .icon(gpui_kit::assets::IconName::Unplug)
                        .outline()
                        .with_size(control::STANDARD)
                        .label("断开")
                        .disabled(busy)
                        .on_click(move |_, window, cx| {
                            disconnect.update(cx, |editor, cx| editor.save(true, window, cx))
                        }),
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("cancel-chain")
                    .icon(gpui_kit::assets::IconName::X)
                    .with_size(control::STANDARD)
                    .label("取消")
                    .disabled(pending)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("save-chain")
                    .icon(gpui_kit::assets::IconName::Save)
                    .primary()
                    .with_size(control::STANDARD)
                    .label("保存并应用")
                    .loading(pending)
                    .disabled(busy)
                    .on_click(move |_, window, cx| {
                        save.update(cx, |editor, cx| editor.save(false, window, cx))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use crate::{proxies::ProxiesPage, session::Session};
    use gpui_kit::{
        AppContext, Bounds, Role, TestAppContext, WindowBounds, WindowOptions, px, size,
        test::{TestQueryExt, TestWindowExt},
    };
    #[gpui_kit::test]
    fn chain_dialog_uses_switch_large_custom_editor_and_apply_action(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let service =
            crate::integration::Service::start(crate::storage::Repository::open(paths).unwrap())
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, cx);
            cx.set_reduce_motion(true);
        });
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1100.), px(900.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let session = cx.new(Session::new);
                    cx.new(|cx| ProxiesPage::new(session, window, cx))
                },
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| page.edit_chain(window, cx));
            window.render_frame(cx);
            assert_eq!(window.find("chain-enabled").role(), Some(Role::Switch));
            assert!(window.find("save-chain").visible());
            assert!(window.find_by_label("链式代理 YAML").bounds().size.height >= px(190.));
            assert!(window.try_find("dialog-commit").is_none());
        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }
}
