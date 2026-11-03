use super::{
    ProxiesPage,
    chain_config::{Change, Config},
};
use crate::{
    ui,
    ui::theme::{control, layout, list, space, stroke, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, IndexPath, Sizable, VirtualListScrollHandle, WindowExt,
        button::{Button, ButtonVariants},
        h_flex,
        input::{Editor, EditorState},
        list::ListItem,
        select::{Select, SelectState},
        switch::Switch,
        v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};
use serde_yaml_ng::Value;
use std::collections::BTreeMap;

impl ProxiesPage {
    pub(super) fn edit_chain(&self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.session.clone();
        if let Err(error) = crate::core::execute(
            crate::core::Command::ChainCandidates,
            window,
            cx,
            move |result, window, cx| {
                if result.is_err() {
                    return;
                }
                let yaml = crate::core::controller(cx)
                    .read(cx)
                    .snapshot()
                    .runtime_yaml()
                    .unwrap_or_default()
                    .to_owned();
                match serde_yaml_ng::from_str::<Value>(&yaml) {
                    Ok(config) => open(session, config, window, cx),
                    Err(_) => ui::show_error("运行配置不是有效 YAML 对象".into(), window, cx),
                }
            },
        ) {
            ui::show_error(error, window, cx);
        }
    }
}
fn open(
    session: Entity<crate::session::Session>,
    config: Value,
    window: &mut Window,
    cx: &mut App,
) {
    let editor = cx.new(|cx| ChainEditor::new(session, config, window, cx));
    let footer = cx.new(|cx| ChainFooter::new(editor.clone(), cx));
    window.open_dialog(cx, move |dialog, window, cx| {
        let body = editor.clone();
        ui::modal(dialog, "链式代理", layout::EDITOR_DIALOG_WIDTH, window, cx)
            .content(ui::modal_panel(layout::FORM_MAX_HEIGHT, move |_, _| {
                body.clone().into_any_element()
            }))
            .footer(footer.clone())
    });
}
#[derive(Clone)]
struct ChainDrag(String);
impl Render for ChainDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(space::MD)
            .py(space::SM)
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .child(self.0.clone())
    }
}
struct ChainEditor {
    session: Entity<crate::session::Session>,
    expected: Config,
    enabled: bool,
    generation: u64,
    epoch: u64,
    pending: bool,
    raw: bool,
    entry: Entity<SelectState<Vec<SharedString>>>,
    group: Entity<SelectState<Vec<SharedString>>>,
    nodes: Vec<String>,
    definitions: BTreeMap<String, Value>,
    sources: BTreeMap<String, super::chain_sources::Source>,
    source: Entity<EditorState>,
    list_scroll: VirtualListScrollHandle,
    drop_target: Option<(String, bool)>,
    _subscriptions: Vec<Subscription>,
}
impl ChainEditor {
    fn new(
        session: Entity<crate::session::Session>,
        config: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let expected = session.read(cx).core().chain().clone();
        let generation = session.read(cx).core().generation();
        let epoch = session.read(cx).core().config_epoch();
        let mut definitions = BTreeMap::new();
        for proxy in config
            .get("proxies")
            .and_then(Value::as_sequence)
            .into_iter()
            .flatten()
            .cloned()
            .chain(expected.custom_nodes().unwrap_or_default())
        {
            if let Some(name) = proxy.get("name").and_then(Value::as_str) {
                if expected.sources.contains_key(name) {
                    definitions.entry(name.to_owned()).or_insert(proxy);
                } else {
                    definitions.insert(name.to_owned(), proxy);
                }
            }
        }
        let candidates = definitions
            .keys()
            .cloned()
            .map(SharedString::from)
            .collect::<Vec<_>>();
        let entry = cx.new(|cx| SelectState::new(candidates, None, window, cx));
        let mut groups = config
            .get("proxy-groups")
            .and_then(Value::as_sequence)
            .into_iter()
            .flatten()
            .filter(|group| {
                matches!(
                    group.get("type").and_then(Value::as_str),
                    Some("select" | "url-test")
                )
            })
            .filter_map(|group| {
                group
                    .get("name")
                    .and_then(Value::as_str)
                    .map(SharedString::from)
            })
            .collect::<Vec<_>>();
        if session.read(cx).mode() == Some(crate::session::ProxyMode::Global) {
            groups = vec!["GLOBAL".into()];
        }
        if !expected.group.is_empty()
            && !groups.iter().any(|group| group.as_ref() == expected.group)
        {
            groups.push(expected.group.clone().into());
        }
        let index = groups
            .iter()
            .position(|group| group.as_ref() == expected.group)
            .or_else(|| (!groups.is_empty()).then_some(0));
        let group = cx.new(|cx| SelectState::new(groups, index.map(IndexPath::new), window, cx));
        let source = cx.new(|cx| ui::editor::state("yaml", "proxies: []\n", true, window, cx));
        let subscriptions = vec![
            cx.observe(&entry, |_, _, cx| cx.notify()),
            cx.observe(&group, |_, _, cx| cx.notify()),
            cx.observe(&session, |_, _, cx| cx.notify()),
        ];
        let mut sources = expected.sources.clone();
        sources.extend(session.read(cx).core().chain_sources.clone());
        let imported = if expected.nodes.is_empty() {
            let result = group
                .read(cx)
                .selected_value()
                .and_then(|group| session.read(cx).selected_node(group.as_ref()))
                .map(|exit| Config::existing(&config, exit));
            match result {
                Some(Ok(nodes)) => nodes,
                Some(Err(error)) => {
                    ui::show_error(error, window, cx);
                    Vec::new()
                }
                None => Vec::new(),
            }
        } else {
            expected.nodes.clone()
        };
        Self {
            session,
            enabled: expected.enabled,
            nodes: imported,
            expected,
            generation,
            epoch,
            pending: false,
            raw: false,
            entry,
            group,
            definitions,
            sources,
            source,
            list_scroll: VirtualListScrollHandle::new(),
            drop_target: None,
            _subscriptions: subscriptions,
        }
    }
    fn yaml(&self) -> Result<String, String> {
        let mut proxies = Vec::new();
        for (index, name) in self.nodes.iter().enumerate() {
            let mut proxy = self
                .definitions
                .get(name)
                .cloned()
                .ok_or_else(|| format!("链式节点尚未解析：{name}"))?;
            let map = proxy.as_mapping_mut().ok_or("链式节点不是对象")?;
            map.remove("dialer-proxy");
            if index > 0 {
                map.insert("dialer-proxy".into(), self.nodes[index - 1].clone().into());
            }
            proxies.push(proxy);
        }
        serde_yaml_ng::to_string(&BTreeMap::from([("proxies", proxies)]))
            .map_err(|_| "无法编码代理链".into())
    }
    fn accept_yaml(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<(), String> {
        let draft = Config {
            custom: self.source.read(cx).value().to_string(),
            ..Config::default()
        };
        let proxies = draft.custom_nodes()?;
        let nodes = proxies
            .iter()
            .filter_map(|proxy| proxy.get("name").and_then(Value::as_str).map(str::to_owned))
            .collect::<Vec<_>>();
        Config {
            nodes: nodes.clone(),
            ..draft
        }
        .validate()?;
        for proxy in proxies {
            if let Some(name) = proxy.get("name").and_then(Value::as_str) {
                // Editing a materialized provider node turns it into a custom
                // definition; a YAML roundtrip alone retains its source binding.
                if self.sources.contains_key(name) {
                    let mut candidate = proxy.clone();
                    candidate
                        .as_mapping_mut()
                        .map(|map| map.remove("dialer-proxy"));
                    let mut original = self.definitions.get(name).cloned();
                    if let Some(original) = original.as_mut().and_then(Value::as_mapping_mut) {
                        original.remove("dialer-proxy");
                    }
                    if original.as_ref() != Some(&candidate) {
                        self.sources.remove(name);
                    }
                }
                self.definitions.insert(name.to_owned(), proxy);
            }
        }
        self.nodes = nodes;
        let candidates = self
            .definitions
            .keys()
            .cloned()
            .map(SharedString::from)
            .collect();
        self.entry
            .update(cx, |state, cx| state.set_items(candidates, window, cx));
        Ok(())
    }
    fn mode(&mut self, raw: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.raw == raw {
            return;
        }
        let result = if raw {
            self.yaml().map(|yaml| {
                self.source
                    .update(cx, |source, cx| source.set_value(yaml, window, cx))
            })
        } else {
            self.accept_yaml(window, cx)
        };
        match result {
            Ok(()) => {
                self.raw = raw;
                cx.notify();
            }
            Err(error) => ui::show_error(error, window, cx),
        }
    }
    fn save(&mut self, disconnect: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending {
            return;
        }
        if !disconnect
            && self.raw
            && let Err(error) = self.accept_yaml(window, cx)
        {
            ui::show_error(error, window, cx);
            return;
        }
        let desired = if disconnect {
            Config::default()
        } else {
            let custom = match self.yaml() {
                Ok(yaml) => yaml,
                Err(error) => {
                    ui::show_error(error, window, cx);
                    return;
                }
            };
            Config {
                enabled: self.enabled,
                group: self
                    .group
                    .read(cx)
                    .selected_value()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                nodes: self.nodes.clone(),
                custom,
                sources: self
                    .sources
                    .iter()
                    .filter(|(name, _)| self.nodes.contains(name))
                    .map(|(name, source)| (name.clone(), source.clone()))
                    .collect(),
            }
        };
        if let Err(error) = desired.validate() {
            ui::show_error(error, window, cx);
            return;
        }
        let owner = cx.weak_entity();
        match crate::core::execute(
            crate::core::Command::Chain(Change {
                expected: self.expected.clone(),
                desired: desired.clone(),
                generation: self.generation,
                epoch: self.epoch,
            }),
            window,
            cx,
            move |result, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.pending = false;
                    let snapshot = this.session.read(cx).core();
                    let committed = snapshot.operation_committed() && snapshot.chain() == &desired;
                    if result.is_ok() || committed {
                        this.expected = desired;
                        this.enabled = this.expected.enabled;
                        let snapshot = this.session.read(cx).core();
                        this.generation = snapshot.generation();
                        this.epoch = snapshot.config_epoch();
                        if disconnect {
                            this.nodes.clear();
                            this.source.update(cx, |source, cx| {
                                source.set_value("proxies: []\n", window, cx)
                            });
                        }
                    }
                    cx.notify();
                });
            },
        ) {
            Ok(()) => {
                self.pending = true;
                cx.notify();
            }
            Err(error) => ui::show_error(error, window, cx),
        }
    }
    fn remove(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(position) = self.nodes.iter().position(|node| node == name) else {
            return;
        };
        let node = self.nodes.remove(position);
        let owner = cx.weak_entity();
        cx.notify();
        ui::notifications::undo(window, "链式节点已移除", cx, move |window, cx| {
            owner
                .update(cx, |this, cx| {
                    if this.pending {
                        ui::show_error("正在应用代理链，请稍后撤销".into(), window, cx);
                        return false;
                    }
                    if !this.nodes.contains(&node) {
                        this.nodes
                            .insert(position.min(this.nodes.len()), node.clone());
                        cx.notify();
                    }
                    true
                })
                .unwrap_or(true)
        });
    }
    fn row(&self, index: usize, name: &str, cx: &mut Context<Self>) -> AnyElement {
        let role = match (index, index + 1 == self.nodes.len()) {
            (0, true) => "入口／出口",
            (0, false) => "入口",
            (_, true) => "出口",
            _ => "中继",
        };
        let name = name.to_owned();
        let remove = name.clone();
        let target = name.clone();
        let hover = name.clone();
        let draggable = !self.pending && !self.session.read(cx).core_busy();
        let kind = self
            .definitions
            .get(&name)
            .and_then(|proxy| proxy.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("未解析")
            .to_owned();
        let insertion = name.clone();
        let row =
            ListItem::new(SharedString::from(format!("chain-node-{name}")))
                .w_full()
                .h_full()
                .rounded(cx.theme().radius)
                .px(space::LG)
                .py(space::MD)
                .accessibility_label(format!("{role}节点 {name}，拖动排序"))
                .when(draggable, |row| {
                    row.on_drag(ChainDrag(name.clone()), |drag, _, _, cx| {
                        cx.new(|_| drag.clone())
                    })
                })
                .can_drop(move |value, _, _| {
                    draggable
                        && value
                            .downcast_ref::<ChainDrag>()
                            .is_some_and(|drag| drag.0 != target)
                })
                .on_drag_move(
                    cx.listener(move |this, event: &DragMoveEvent<ChainDrag>, _, cx| {
                        if event.bounds.contains(&event.event.position) {
                            let target = Some((
                                hover.clone(),
                                event.event.position.y >= event.bounds.center().y,
                            ));
                            if this.drop_target != target {
                                this.drop_target = target;
                                cx.notify();
                            }
                        }
                    }),
                )
                .drag_over::<ChainDrag>(|style, _, _, cx| style.bg(cx.theme().drop_target))
                .on_drop(cx.listener({
                    let destination = name.clone();
                    move |this, drag: &ChainDrag, _, cx| {
                        if this.pending || this.session.read(cx).core_busy() {
                            return;
                        }
                        let after = this
                            .drop_target
                            .take()
                            .is_some_and(|(target, after)| target == destination && after);
                        if drag.0 == destination {
                            return;
                        }
                        let Some(from) = this.nodes.iter().position(|node| node == &drag.0) else {
                            return;
                        };
                        let node = this.nodes.remove(from);
                        if let Some(to) = this.nodes.iter().position(|node| node == &destination) {
                            this.nodes.insert(to + usize::from(after), node);
                        }
                        cx.notify();
                    }
                }))
                .child(
                    h_flex()
                        .w_full()
                        .min_w(space::NONE)
                        .gap(space::MD)
                        .child(Icon::new(IconName::GripVertical).size(layout::ICON))
                        .child(ui::muted(role, cx).text_size(text::CAPTION))
                        .child(div().flex_1().min_w(space::NONE).truncate().child(name))
                        .child(ui::muted(kind, cx).text_size(text::CAPTION))
                        .child(
                            ui::icon_button(
                                SharedString::from(format!("chain-remove-{remove}")),
                                IconName::Trash,
                                "删除链式节点",
                            )
                            .disabled(!draggable)
                            .on_click(cx.listener(
                                move |this, _, window, cx| this.remove(&remove, window, cx),
                            )),
                        ),
                );
        div()
            .relative()
            .size_full()
            .child(row)
            .when(
                cx.has_active_drag()
                    && self
                        .drop_target
                        .as_ref()
                        .is_some_and(|(target, _)| target == &insertion),
                |view| {
                    let after = self.drop_target.as_ref().is_some_and(|(_, after)| *after);
                    view.child(
                        div()
                            .absolute()
                            .left(space::NONE)
                            .right(space::NONE)
                            .h(stroke::INSERTION)
                            .bg(cx.theme().primary)
                            .map(|line| {
                                if after {
                                    line.bottom(space::NONE)
                                } else {
                                    line.top(space::NONE)
                                }
                            }),
                    )
                },
            )
            .into_any_element()
    }
}

impl Render for ChainEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.pending || self.session.read(cx).core_busy();
        if self.raw {
            return Editor::new(&self.source)
                .w_full()
                .h_full()
                .disabled(busy)
                .aria_label("链式代理 YAML")
                .into_any_element();
        }
        let nodes = std::rc::Rc::new(self.nodes.clone());
        v_flex()
            .size_full()
            .min_h(space::NONE)
            .gap(space::LG)
            .child(
                ui::form()
                    .child(
                        ui::form_row()
                            .items_center()
                            .justify_between()
                            .child("启用链式代理")
                            .child(
                                Switch::new("chain-enabled")
                                    .checked(self.enabled)
                                    .disabled(busy)
                                    .accessibility_label("启用链式代理")
                                    .on_change(cx.listener(|this, enabled, _, cx| {
                                        this.enabled = *enabled;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(ui::field_label(
                        "目标代理组",
                        Select::new(&self.group)
                            .w_full()
                            .placeholder("请选择目标代理组")
                            .with_size(control::STANDARD)
                            .disabled(busy)
                            .accessibility_label("目标代理组"),
                    )),
            )
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_weight(text::EMPHASIS)
                            .child("代理链"),
                    )
                    .child(
                        div().flex_1().min_w(space::NONE).child(
                            Select::new(&self.entry)
                                .id("chain-add-select")
                                .w_full()
                                .placeholder("请选择节点")
                                .with_size(control::STANDARD)
                                .disabled(busy)
                                .accessibility_label("添加链式节点"),
                        ),
                    )
                    .child(
                        ui::icon_button("chain-add-node", IconName::Plus, "添加链式节点")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(node) = this
                                    .entry
                                    .read(cx)
                                    .selected_value()
                                    .map(ToString::to_string)
                                    && !this.nodes.contains(&node)
                                {
                                    this.nodes.push(node);
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        Button::new("chain-clear")
                            .ghost()
                            .label("清空")
                            .disabled(busy || self.nodes.is_empty())
                            .on_click(cx.listener(|this, _, window, cx| {
                                let nodes = std::mem::take(&mut this.nodes);
                                let owner = cx.weak_entity();
                                cx.notify();
                                ui::notifications::undo(
                                    window,
                                    "代理链已清空",
                                    cx,
                                    move |window, cx| {
                                        owner
                                            .update(cx, |this, cx| {
                                                if this.pending || !this.nodes.is_empty() {
                                                    ui::show_error(
                                                        "代理链已更改，无法撤销此次清空".into(),
                                                        window,
                                                        cx,
                                                    );
                                                    return false;
                                                }
                                                this.nodes = nodes.clone();
                                                cx.notify();
                                                true
                                            })
                                            .unwrap_or(true)
                                    },
                                );
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(space::NONE)
                    .w_full()
                    .when(self.nodes.is_empty(), |view| {
                        view.child(ui::muted("尚未添加节点", cx))
                    })
                    .when(!self.nodes.is_empty(), |view| {
                        view.child(ui::virtual_rows::rows(
                            cx.entity(),
                            "chain-node-list",
                            vec![list::DETAIL_ROW_HEIGHT.to_pixels(window.rem_size()); nodes.len()],
                            &self.list_scroll,
                            move |this, index, _, cx| this.row(index, &nodes[index], cx),
                        ))
                    }),
            )
            .into_any_element()
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
        let editor = self.editor.read(cx);
        let busy = editor.pending || editor.session.read(cx).core_busy();
        let mode = self.editor.clone();
        let disconnect = self.editor.clone();
        let save = self.editor.clone();
        ui::modal_footer()
            .w_full()
            .when(editor.expected.enabled, |row| {
                row.child(
                    Button::new("disconnect-chain")
                        .outline()
                        .icon(IconName::Unplug)
                        .label("断开")
                        .disabled(busy)
                        .on_click(move |_, window, cx| {
                            disconnect.update(cx, |this, cx| this.save(true, window, cx))
                        }),
                )
            })
            .child(
                Button::new("chain-editor-mode")
                    .icon(IconName::FileCode)
                    .outline()
                    .with_size(control::STANDARD)
                    .disabled(busy)
                    .label(if editor.raw {
                        "表单编辑"
                    } else {
                        "YAML 编辑"
                    })
                    .on_click(move |_, window, cx| {
                        mode.update(cx, |this, cx| this.mode(!this.raw, window, cx));
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("cancel-chain")
                    .icon(IconName::X)
                    .label("取消")
                    .disabled(editor.pending)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("save-chain")
                    .primary()
                    .icon(IconName::Save)
                    .label("保存并应用")
                    .loading(editor.pending)
                    .disabled(busy)
                    .on_click(move |_, window, cx| {
                        save.update(cx, |this, cx| this.save(false, window, cx))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::ChainEditor;
    use gpui_kit::{
        AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size,
        test::{TestQueryExt, TestWindowExt},
    };
    use serde_yaml_ng::Value;
    fn configuration() -> Value {
        serde_yaml_ng::from_str("proxies:\n- {name: a, type: socks5, server: 127.0.0.1, port: 1080, password: keep}\n- {name: b, type: http, server: 127.0.0.1, port: 1081}\n- {name: c, type: trojan, server: example.test, port: 443}\nproxy-groups: [{name: g, type: select, proxies: [a,b,c]}]").unwrap()
    }
    #[gpui_kit::test]
    fn visual_chain_yaml_share_order_definitions_and_deleted_nodes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        let (handle, editor) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1000.), px(650.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let session = cx.new(|_| crate::session::Session::default());
                    cx.new(|cx| {
                        let mut editor = ChainEditor::new(session, configuration(), window, cx);
                        editor.nodes = vec!["a".into(), "b".into(), "c".into()];
                        editor
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(handle,|_,window,cx| {
            window.render_frame(cx);
            let row=window.find("chain-node-a").bounds();let delete=window.find("chain-remove-a").bounds();
            assert!(delete.right()>row.left()+row.size.width*0.9);
            let select=window.find("chain-add-select").bounds();let add=window.find("chain-add-node").bounds();
            let gap=crate::ui::theme::space::SM.to_pixels(window.rem_size());
            assert!((select.right()+gap-add.left()).abs()<=px(1.));
            assert!(select.size.width>row.size.width*0.6);
            assert!(window.try_find("chain-up-a").is_none());assert!(window.try_find("chain-down-a").is_none());
            assert!(window.try_find_by_label("入口节点 a，拖动排序").is_some());
            assert!(window.try_find_by_label("中继节点 b，拖动排序").is_some());
            assert!(window.try_find_by_label("出口节点 c，拖动排序").is_some());
            let last=window.find("chain-node-c").bounds();
            window.drag(gpui_kit::point(row.left()+px(10.),row.center().y),gpui_kit::point(last.left()+px(10.),last.bottom()-px(5.)),cx);
            assert_eq!(editor.read(cx).nodes,["b","c","a"]);
            window.render_frame(cx);
            assert!(window.try_find_by_label("入口节点 b，拖动排序").is_some());
            assert!(window.try_find_by_label("中继节点 c，拖动排序").is_some());
            assert!(window.try_find_by_label("出口节点 a，拖动排序").is_some());

            editor.update(cx,|this,cx| {
                this.nodes=vec!["a".into(),"b".into(),"c".into()];
                this.nodes.swap(0,2);
                this.mode(true,window,cx);
                let yaml=this.source.read(cx).value().to_string();let root:Value=serde_yaml_ng::from_str(&yaml).unwrap();
                assert_eq!(root["proxies"][0]["name"].as_str(),Some("c"));
                assert_eq!(root["proxies"][2]["password"].as_str(),Some("keep"));
                assert_eq!(root["proxies"][2]["dialer-proxy"].as_str(),Some("b"));
                this.source.update(cx,|source,cx|source.set_value("proxies: [{name: new, type: socks5, server: example.test, port: 1080}, {name: a, type: http, server: 127.0.0.1, port: 7890}]",window,cx));
                this.mode(false,window,cx);
                assert_eq!(this.nodes,["new","a"]);assert_eq!(this.definitions["a"]["type"].as_str(),Some("http"));
                this.nodes.remove(0);
                let yaml=this.yaml().unwrap();assert!(!yaml.contains("name: new"));assert!(!yaml.contains("dialer-proxy"));
                this.raw=true;this.source.update(cx,|source,cx|source.set_value("proxies: [",window,cx));
                assert!(this.accept_yaml(window,cx).is_err());assert_eq!(this.nodes,["a"]);
            });
        }).unwrap();
    }
}
