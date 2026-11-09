use super::{
    ProxiesPage,
    model::{GroupKind, Node, ProxyGroup},
};
use crate::ui::{
    self,
    theme::{control, layout, list, proxy_card, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, Sizable,
        button::{Button, ButtonVariants},
        h_flex, v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};

#[derive(Clone)]
struct GroupDrag(String);
impl Render for GroupDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(self.0.clone())
    }
}
#[derive(Clone)]
struct GroupInfo {
    id: String,
    name: String,
    kind: GroupKind,
    default_node: String,
    count: usize,
}
impl From<&ProxyGroup> for GroupInfo {
    fn from(group: &ProxyGroup) -> Self {
        Self {
            id: group.id(),
            name: group.name(),
            kind: group.kind(),
            default_node: group.default_node(),
            count: group.len(),
        }
    }
}

impl ProxiesPage {
    pub(super) fn group_content(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let query = self.query.read(cx).value().trim().to_lowercase();
        let groups: Vec<_> = self
            .groups
            .iter()
            .filter_map(|group| {
                if !self.group_filter.is_empty() && !self.group_filter.contains(&group.id()) {
                    return None;
                }
                let nodes = group.visible_filtered(
                    &self.search,
                    self.only_available,
                    self.group_sorts
                        .get(&group.id())
                        .copied()
                        .unwrap_or(self.sort),
                    self.reverse,
                );
                (!nodes.is_empty()).then(|| {
                    let mut info = GroupInfo::from(group);
                    info.count = nodes.len();
                    (info, nodes)
                })
            })
            .collect();
        let direct = self.session.read(cx).mode() == Some(crate::session::ProxyMode::Direct);
        if direct || groups.is_empty() {
            let (icon, heading, description) = if direct {
                (
                    IconName::Route,
                    "当前为直连模式",
                    "切换到规则或全局模式后查看代理组。",
                )
            } else if self.groups.is_empty() {
                (
                    IconName::Network,
                    "暂无代理组",
                    "启用配置并启动内核后，代理组和节点会显示在这里。",
                )
            } else {
                (
                    IconName::Search,
                    "没有匹配的节点",
                    "尝试其他关键词，或关闭“仅显示可用节点”。",
                )
            };
            let empty = ui::empty(icon, heading, description, cx).id("proxy-groups-empty");
            #[cfg(test)]
            let empty = {
                use gpui_kit::test::TestSupportExt;
                empty.test_support()
            };
            let mut content = empty;
            if !direct && !self.groups.is_empty() {
                content = content.child(
                    Button::new("clear-proxy-filters")
                        .icon(IconName::X)
                        .outline()
                        .with_size(control::STANDARD)
                        .label("清除筛选")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.only_available = false;
                            this.group_filter.clear();
                            this.query.update(cx, |query, cx| {
                                query.set_value("", window, cx);
                                query.focus_handle(cx).focus(window, cx);
                            });
                            this.refresh_list(window, cx);
                            cx.notify();
                        })),
                );
            }
            return content.into_any_element();
        }
        let height = if self.compact {
            proxy_card::COMPACT_HEIGHT
        } else {
            proxy_card::HEIGHT
        };
        let node_height = height.to_pixels(window.rem_size());
        let gap = proxy_card::GAP.to_pixels(window.rem_size());
        let mut rows = Vec::new();
        let mut heights = Vec::new();
        for (group, nodes) in groups {
            let open = self.is_expanded(&group.id, !query.is_empty());
            let motion = cx.theme().motion_tokens();
            let fraction = base::motion::transition(
                (
                    ElementId::from("proxy-group-body"),
                    SharedString::from(group.id.clone()),
                ),
                if open { 1. } else { 0. },
                base::motion::Transition::new(motion.duration_fast)
                    .easing(motion.easing_move.clone()),
                window,
                cx,
            );
            heights.push(node_height + gap);
            rows.push((group.clone(), None));
            if fraction > 0. {
                for chunk in nodes.chunks(self.columns.max(1)) {
                    heights.push((node_height + gap) * fraction);
                    rows.push((group.clone(), Some(chunk.to_vec())));
                }
            }
        }
        let searching = !query.is_empty();
        let offset = -self.list_scroll.offset().y;
        let mut origin = px(0.);
        let mut pinned = None;
        let mut next_header = None;
        for ((group, nodes), height) in rows.iter().zip(&heights) {
            if nodes.is_none() {
                if origin <= offset {
                    pinned = Some(group.clone());
                } else if pinned.is_some() {
                    next_header = Some(origin);
                    break;
                }
            }
            origin += *height;
        }
        let sticky = pinned.map(|group| {
            let y = next_header
                .map(|next| (next - offset - node_height - gap).min(px(0.)))
                .unwrap_or(px(0.));
            div()
                .id("proxy-sticky-header")
                .absolute()
                .top(y)
                .left(list::INSET_X)
                .right(list::INSET_X)
                .h(height)
                .rounded(cx.theme().radius)
                .bg(cx.theme().muted)
                .occlude()
                .child(self.group_header(group, searching, true, window, cx))
        });
        let rows = std::rc::Rc::new(rows);
        div()
            .id("proxy-sticky-list")
            .relative()
            .overflow_hidden()
            .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
            .size_full()
            .min_w(space::NONE)
            .py(list::INSET_Y)
            .child(ui::virtual_rows::rows(
                cx.entity(),
                "proxy-virtual-rows",
                heights,
                &self.list_scroll,
                sticky.map(IntoElement::into_any_element),
                move |this, index, window, cx| {
                    let (group, nodes) = &rows[index];
                    if let Some(nodes) = nodes {
                        div()
                            .h(height + proxy_card::GAP)
                            .pb(proxy_card::GAP)
                            .px(list::INSET_X + proxy_card::GROUP_INSET_X)
                            .grid()
                            .grid_cols(this.columns as u16)
                            .gap(proxy_card::GAP)
                            .children(nodes.iter().cloned().map(|node| {
                                this.node_card(
                                    group.clone(),
                                    node,
                                    this.is_expanded(&group.id, searching),
                                    cx,
                                )
                            }))
                            .into_any_element()
                    } else {
                        div()
                            .h(height + proxy_card::GAP)
                            .pb(proxy_card::GAP)
                            .px(list::INSET_X)
                            .child(this.group_header(group.clone(), searching, false, window, cx))
                            .into_any_element()
                    }
                },
            ))
            .into_any_element()
    }
    fn group_header(
        &self,
        group: GroupInfo,
        searching: bool,
        sticky: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view_id = if sticky {
            format!("sticky-{}", group.id)
        } else {
            group.id.clone()
        };
        let open = self.is_expanded(&group.id, searching);
        let motion = cx.theme().motion_tokens();
        let angle = base::motion::transition(
            (
                ElementId::from("proxy-disclosure"),
                SharedString::from(group.id.clone()),
            ),
            if open {
                std::f32::consts::FRAC_PI_2
            } else {
                0.
            },
            base::motion::Transition::new(motion.duration_fast).easing(motion.easing_move.clone()),
            window,
            cx,
        );
        let selected = self
            .session
            .read(cx)
            .selected_node(&group.name)
            .unwrap_or(&group.default_node);
        let runtime = self.session.read(cx).runtime();
        let fixed_node = runtime
            .proxies
            .entries
            .get(&group.name)
            .and_then(|proxy| proxy.fixed.as_deref())
            .filter(|name| !name.is_empty());
        let fixed = fixed_node.is_some();
        let height = if self.compact {
            proxy_card::COMPACT_HEIGHT
        } else {
            proxy_card::HEIGHT
        };
        let runtime = self.session.read(cx).runtime();
        let generation = runtime.generation;
        let epoch = runtime.epoch;
        let urls = self.test_urls(cx);
        let toggle_id = group.id.clone();
        let test_group = group.name.clone();
        let locate_group = group.id.clone();
        let sort_group = group.id.clone();
        let sort = self
            .group_sorts
            .get(&group.id)
            .copied()
            .unwrap_or(self.sort);
        let testing = runtime
            .proxies
            .batches
            .iter()
            .any(|(name, _)| name == &group.name);
        let reset_group = group.name.clone();
        let url_group = group.name.clone();
        let actions = h_flex()
            .id(SharedString::from(format!(
                "proxy-group-actions-{}",
                view_id
            )))
            .gap(space::XS)
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                ui::icon_button(
                    SharedString::from(format!("proxy-group-locate-{}", view_id)),
                    IconName::LocateFixed,
                    "定位当前节点",
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.locate_group(&locate_group, window, cx)
                })),
            )
            .child(
                ui::icon_button(
                    SharedString::from(format!("proxy-group-test-{}", view_id)),
                    IconName::Gauge,
                    "测试代理组延迟",
                )
                .loading(testing)
                .on_click(move |_, window, cx| {
                    crate::core::test_delay(
                        super::runtime::Test {
                            urls: urls.clone(),
                            group: Some(test_group.clone()),
                            node: None,
                            generation,
                            epoch,
                        },
                        window,
                        cx,
                    )
                }),
            )
            .child(
                ui::icon_button(
                    SharedString::from(format!("proxy-group-sort-{}", view_id)),
                    match sort {
                        super::model::SortOrder::Default => IconName::ArrowDownWideNarrow,
                        super::model::SortOrder::Latency => IconName::Clock,
                        super::model::SortOrder::Name => IconName::ArrowDownAZ,
                    },
                    sort.label(),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.group_sorts.insert(
                        sort_group.clone(),
                        match sort {
                            super::model::SortOrder::Default => super::model::SortOrder::Latency,
                            super::model::SortOrder::Latency => super::model::SortOrder::Name,
                            super::model::SortOrder::Name => super::model::SortOrder::Default,
                        },
                    );
                    cx.notify();
                })),
            )
            .child(
                ui::icon_button(
                    SharedString::from(format!("group-url-{}", view_id)),
                    IconName::Link,
                    "编辑测速地址",
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_test_url(url_group.clone(), window, cx)
                })),
            )
            .when(fixed, |row| {
                row.child(
                    ui::icon_button(
                        SharedString::from(format!("proxy-group-unfix-{}", view_id)),
                        IconName::PinOff,
                        "恢复自动选择",
                    )
                    .on_click(move |_, window, cx| {
                        crate::core::request(
                            crate::core::Command::Proxy(super::runtime::Action::Unfix {
                                group: reset_group.clone(),
                                generation,
                                epoch,
                            }),
                            window,
                            cx,
                        )
                    }),
                )
            });
        let drag_target = group.id.clone();
        let header = h_flex()
            .bg(cx.theme().muted)
            .rounded(cx.theme().radius)
            .id(SharedString::from(format!("group-drag-{}", view_id)))
            .on_drag(GroupDrag(group.id.clone()), |drag, _, _, cx| {
                cx.new(|_| drag.clone())
            })
            .on_drop(cx.listener(move |this, drag: &GroupDrag, window, cx| {
                if let (Some(from), Some(to)) = (
                    this.groups.iter().position(|group| group.id() == drag.0),
                    this.groups
                        .iter()
                        .position(|group| group.id() == drag_target),
                ) {
                    let group = this.groups.remove(from);
                    this.groups.insert(to, group);
                    this.refresh_list(window, cx);
                    cx.notify();
                }
            }))
            .gap(space::SM)
            .child(
                Button::new(SharedString::from(format!("proxy-group-{}", view_id)))
                    .ghost()
                    .with_size(control::STANDARD)
                    .w_full()
                    .min_w(space::NONE)
                    .px(list::PADDING_X)
                    .h(height)
                    .accessibility_label(format!(
                        "{}{}，{}个节点，当前节点{}",
                        if open { "收起" } else { "展开" },
                        group.name,
                        group.count,
                        selected
                    ))
                    .child(
                        h_flex()
                            .w_full()
                            .gap(space::MD)
                            .child(
                                Icon::new(IconName::ChevronRight)
                                    .rotate(radians(angle))
                                    .size(layout::ICON),
                            )
                            .child(self.group_icon(&group, cx))
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w(space::NONE)
                                    .gap(space::SM)
                                    .child(
                                        div()
                                            .truncate()
                                            .font_weight(text::EMPHASIS)
                                            .child(group.name.clone()),
                                    )
                                    .child(ui::muted(group.kind.label(), cx))
                                    .child(
                                        ui::muted(
                                            format!(
                                                "{}{}",
                                                selected,
                                                fixed_node
                                                    .map(|node| format!(" · 固定：{node}"))
                                                    .unwrap_or_default()
                                            ),
                                            cx,
                                        )
                                        .truncate(),
                                    ),
                            )
                            .child(ui::muted(
                                format!(
                                    "{} 个节点",
                                    self.groups
                                        .iter()
                                        .find(|entry| entry.id() == group.id)
                                        .map(|entry| entry
                                            .visible_nodes("", false, self.sort, false)
                                            .len())
                                        .unwrap_or(0)
                                ),
                                cx,
                            ))
                            .child(actions),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_group(toggle_id.clone(), searching, window, cx)
                    })),
            );
        header.into_any_element()
    }
    fn locate_group(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty();
        let opening = if searching {
            self.search_expanded.insert(id.to_owned())
        } else {
            self.expanded.insert(id.to_owned())
        };
        self.locate_task = None;
        if opening && !cx.reduce_motion() {
            // VirtualList consumes its pending scroll in the next layout. Wait
            // for the existing expansion transition to give rows their final sizes.
            let timer = cx
                .background_executor()
                .timer(cx.theme().motion_tokens().duration_fast);
            let id = id.to_owned();
            self.locate_task = Some(cx.spawn_in(window, async move |owner, cx| {
                timer.await;
                let _ = owner.update_in(cx, |this, window, cx| {
                    this.locate_task = None;
                    this.center_selected_group(&id, window, cx);
                });
            }));
            self.refresh_list(window, cx);
        } else {
            self.center_selected_group(id, window, cx);
        }
    }

    fn center_selected_group(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let searching = !self.query.read(cx).value().trim().is_empty();
        let mut row = 0;
        for group in &self.groups {
            if !self.group_filter.is_empty() && !self.group_filter.contains(&group.id()) {
                continue;
            }
            let nodes = group.visible_filtered(
                &self.search,
                self.only_available,
                self.group_sorts
                    .get(&group.id())
                    .copied()
                    .unwrap_or(self.sort),
                self.reverse,
            );
            if nodes.is_empty() {
                continue;
            }
            if group.id() == id {
                let selected = self
                    .session
                    .read(cx)
                    .selected_node(&group.name())
                    .unwrap_or("")
                    .to_owned();
                if let Some(index) = nodes.iter().position(|node| node.name() == selected) {
                    row += 1 + index / self.columns.max(1);
                    self.list_scroll.scroll_to_item(row, ScrollStrategy::Center);
                } else {
                    self.list_scroll
                        .scroll_to_item(row, ScrollStrategy::Nearest);
                }
                break;
            }
            row += 1;
            if self.is_expanded(&group.id(), searching) {
                row += nodes.len().div_ceil(self.columns.max(1));
            }
        }
        self.refresh_list(window, cx);
    }
    fn group_icon(&self, group: &GroupInfo, cx: &App) -> AnyElement {
        let runtime = self.session.read(cx).runtime();
        if let Some(path) = runtime
            .proxies
            .entries
            .get(&group.name)
            .and_then(|proxy| runtime.icons.files.get(proxy.icon.trim()))
        {
            return img(path.clone())
                .size(layout::ICON_LG)
                .object_fit(ObjectFit::Contain)
                .into_any_element();
        }
        Icon::new(match group.kind {
            GroupKind::Select | GroupKind::Fallback => IconName::Route,
            GroupKind::UrlTest => IconName::Zap,
            GroupKind::Media => IconName::TvMinimalPlay,
            GroupKind::LoadBalance => IconName::Network,
            GroupKind::Relay => IconName::Link,
        })
        .size(layout::ICON_LG)
        .into_any_element()
    }
    fn node_card(&self, group: GroupInfo, node: Node, open: bool, cx: &App) -> AnyElement {
        let active = self
            .session
            .read(cx)
            .selected_node(&group.name)
            .unwrap_or(&group.default_node)
            == node.name();
        let focus = self.card_focus[&(group.id.clone(), node.id())].clone();
        let runtime = self.session.read(cx).runtime();
        let generation = runtime.generation;
        let epoch = runtime.epoch;
        let urls = self.test_urls(cx);
        let select_group = group.name.clone();
        let select_node = node.name();
        let test_node = node.id();
        Button::new(SharedString::from(format!(
            "node-card-{}-{}",
            group.id,
            node.id()
        )))
        .ghost()
        .disabled(
            !node.resolved() || matches!(group.kind, GroupKind::LoadBalance | GroupKind::Relay),
        )
        .with_size(control::STANDARD)
        .track_focus(&focus)
        .tab_stop(open)
        .accessibility_label(format!(
            "{}：{}{}",
            group.name,
            node.display_name(),
            if active { "，当前使用" } else { "" }
        ))
        .tooltip(format!("{}\n{}", node.display_name(), node.details()))
        .size_full()
        .line_height(relative(text::LINE_HEIGHT))
        .min_w(space::NONE)
        .px(proxy_card::PADDING)
        .justify_center()
        .gap(space::XS)
        .rounded(cx.theme().radius)
        .map(|card| ui::selectable_card(card, active, cx))
        .bg(if active {
            cx.theme().muted.blend(ui::theme::selection_tint(cx))
        } else {
            cx.theme().muted
        })
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            crate::core::request(
                crate::core::Command::Proxy(super::runtime::Action::Select {
                    group: select_group.clone(),
                    node: select_node.clone(),
                    generation,
                    epoch,
                }),
                window,
                cx,
            );
        })
        .child(
            v_flex()
                .w_full()
                .min_w(space::NONE)
                .gap(space::XS)
                .child(
                    h_flex().gap(space::SM).child(
                        div()
                            .flex_1()
                            .min_w(space::NONE)
                            .truncate()
                            .font_weight(text::EMPHASIS)
                            .child(node.display_name()),
                    ),
                )
                .child(
                    h_flex()
                        .gap(space::SM)
                        .child(
                            ui::muted(
                                if node.region().is_empty() {
                                    node.details()
                                } else {
                                    format!("{} · {}", node.region(), node.details())
                                },
                                cx,
                            )
                            .text_size(text::CAPTION)
                            .truncate()
                            .flex_1()
                            .min_w(space::NONE),
                        )
                        .child(
                            Button::new(SharedString::from(format!(
                                "node-delay-{}-{}",
                                group.id,
                                node.id()
                            )))
                            .ghost()
                            .small()
                            .disabled(!node.testable())
                            .loading(node.testing())
                            .when_else(
                                node.delay().is_none()
                                    && (!node.testable()
                                        || !node.tested() && node.failure().is_none()),
                                |button| button.icon(IconName::Gauge),
                                |button| {
                                    button.label(
                                        node.delay().map(|delay| delay.to_string()).unwrap_or_else(
                                            || {
                                                if node.failure().is_some() {
                                                    "失败"
                                                } else {
                                                    "超时"
                                                }
                                                .into()
                                            },
                                        ),
                                    )
                                },
                            )
                            .text_color(if node.testing() {
                                cx.theme().muted_foreground
                            } else if let Some(delay) = node.delay() {
                                if delay >= 400 {
                                    cx.theme().warning
                                } else if delay >= 250 {
                                    cx.theme().primary
                                } else {
                                    cx.theme().success
                                }
                            } else if node.tested() && node.testable() {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            })
                            .accessibility_label(format!("测试{}的{}延迟", group.name, node.name()))
                            .tooltip(
                                if node.timed_out() {
                                    "测速超时"
                                } else {
                                    node.failure().unwrap_or("测试此节点延迟")
                                }
                                .to_owned(),
                            )
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                crate::core::test_delay(
                                    super::runtime::Test {
                                        urls: urls.clone(),
                                        group: Some(group.name.clone()),
                                        node: Some(test_node.clone()),
                                        generation,
                                        epoch,
                                    },
                                    window,
                                    cx,
                                );
                            }),
                        ),
                ),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod motion_tests {
    use crate::{proxies::ProxiesPage, session::Session};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
    use std::time::Duration;
    #[gpui_kit::test]
    fn locate_centers_the_selected_node_in_normal_and_compact_layouts(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let service =
            crate::integration::Service::start(crate::storage::Repository::open(paths).unwrap())
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
            crate::integration::init(service, None, cx);
        });
        let (handle, page) = cx.update(|cx| {
            let names = (0..90).map(|i| format!("node-{i}")).collect::<Vec<_>>();
            let mut entries = serde_json::Map::new();
            for name in &names {
                entries.insert(name.clone(), serde_json::json!({"name": name, "type": "Shadowsocks"}));
            }
            entries.insert("group".into(), serde_json::json!({"name": "group", "type": "Selector", "all": names, "now": "node-40"}));
            let proxies = crate::proxies::runtime::Proxies::parse(
                &serde_json::to_vec(&serde_json::json!({"proxies": entries})).unwrap(), &["group".into()]
            ).unwrap();
            let session = cx.new(|_| {
                Session::fixture(
                    crate::core::Snapshot::ready_with_providers(
                        crate::core::ProviderKind::Proxies,
                        br#"{"providers":{}}"#,
                    )
                    .unwrap(),
                    crate::core::Runtime {
                        connected: true,
                        mode: Some(crate::session::ProxyMode::Rule),
                        proxies,
                        ..Default::default()
                    },
                )
            });
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1000.), px(640.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| ProxiesPage::new(session, window, cx)),
            )
            .unwrap()
        });
        for compact in [false, true] {
            cx.update_window(handle, |_, window, cx| {
                page.update(cx, |page, cx| {
                    page.compact = compact;
                    page.expanded.insert("group".into());
                    page.list_scroll.set_offset(Default::default());
                    cx.notify();
                });
                window.render_frame(cx);
                window.click("proxy-group-locate-group", cx);
                window.render_frame(cx);
                window.render_frame(cx);
                let target = page.read(cx).groups[0]
                    .visible_nodes("", false, crate::proxies::model::SortOrder::Default, false)
                    .into_iter()
                    .find(|node| node.name() == "node-40")
                    .unwrap();
                let card = window.find(format!("node-card-group-{}", target.id()));
                assert!(card.visible());
                let viewport = page.read(cx).list_scroll.bounds();
                assert!(
                    (card.bounds().center().y - viewport.center().y).abs()
                        < card.bounds().size.height / 2.,
                    "selected node was not centered"
                );
            })
            .unwrap();
        }
        cx.update(|cx| cx.set_reduce_motion(false));
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| {
                page.expanded.clear();
                page.list_scroll.set_offset(Default::default());
                cx.notify();
            });
            window.render_frame(cx);
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("proxy-group-locate-group", cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let target = page.read(cx).groups[0]
                .visible_nodes("", false, crate::proxies::model::SortOrder::Default, false)
                .into_iter()
                .find(|node| node.name() == "node-40")
                .unwrap();
            let card = window.find(format!("node-card-group-{}", target.id()));
            let viewport = page.read(cx).list_scroll.bounds();
            assert!(
                (card.bounds().center().y - viewport.center().y).abs()
                    < card.bounds().size.height / 2.
            );
        })
        .unwrap();
        cx.simulate_window_resize(handle, size(px(2600.), px(640.)));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(
                page.read(cx).columns > 3,
                "wide windows must not cap node columns at three"
            )
        });
        crate::integration::finish_test_services(cx);
    }
    #[gpui_kit::test]
    fn unresolved_member_card_is_disabled_while_direct_remains_selectable(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let service =
            crate::integration::Service::start(crate::storage::Repository::open(paths).unwrap())
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
        });
        let (handle, session) = cx.update(|cx| {
            let session = cx.new(|_| Session::default());
            let state = session.clone();
            let (handle, _) = gpui_kit::open_window(WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(Default::default(), size(px(1100.), px(900.))))),
                ..Default::default()
            }, cx, |window, cx| {
                cx.new(|cx| {
                    let mut page = ProxiesPage::new(state, window, cx);
                    let proxies = crate::proxies::runtime::Proxies::parse(br#"{"proxies":{"g":{"name":"g","type":"Selector","all":["DIRECT","missing"]},"DIRECT":{"name":"DIRECT","type":"Direct"}}}"#, &[]).unwrap();
                    page.groups = crate::proxies::model::ProxyGroup::observed(&proxies, None, 5000);
                    page.card_focus = page.groups.iter().flat_map(|group| group.visible_nodes("", false, crate::proxies::model::SortOrder::Default, false).into_iter().map(|node| ((group.id(), node.id()), cx.focus_handle())).collect::<Vec<_>>()).collect();
                    page.expanded.insert("g".into());
                    page.refresh_list(window, cx);
                    page
                })
            }).unwrap();
            (handle, session)
        });
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("node-card-g-missing:0").disabled(), Some(true));
            assert_ne!(
                window.find("node-card-g-c:6:DIRECT:0").disabled(),
                Some(true)
            );
            assert_eq!(
                window.find("node-delay-g-c:6:DIRECT:0").disabled(),
                Some(true)
            );
            window.click("node-card-g-missing:0", cx);
            assert_eq!(session.read(cx).core().request_id(), None);
        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }
    #[gpui_kit::test]
    fn changed_groups_prune_stale_filters_and_clear_action_restores_nodes(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let service =
            crate::integration::Service::start(crate::storage::Repository::open(paths).unwrap())
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
        });
        let (handle, session, page) = cx.update(|cx| {
            let session = cx.new(|_| Session::default());
            let state = session.clone();
            let (handle, page) = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1100.), px(900.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| ProxiesPage::new(state, window, cx)),
            )
            .unwrap();
            (handle, session, page)
        });
        cx.update_window(handle, |_, _, cx| {
            page.update(cx, |page, _| {
                page.group_filter.insert("removed".into());
                page.group_filter.insert("current".into());
            })
        })
        .unwrap();
        cx.update(|cx| session.update(cx, |session, cx| {
            let proxies = crate::proxies::runtime::Proxies::parse(br#"{"proxies":{"current":{"name":"current","type":"Selector","all":["DIRECT"]},"DIRECT":{"name":"DIRECT","type":"Direct"}}}"#, &[]).unwrap();
            let snapshot = crate::core::Snapshot::ready_with_providers(crate::core::ProviderKind::Proxies, br#"{"providers":{}}"#).unwrap();
            *session = Session::fixture(snapshot, crate::core::Runtime { connected: true, proxies, ..Default::default() });
            cx.notify();
        }));
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            page.update(cx, |page, cx| {
                assert_eq!(page.group_filter, ["current".to_owned()].into());
                page.group_filter.insert("hidden".into());
                page.group_filter.remove("current");
                page.only_available = true;
                page.query
                    .update(cx, |input, cx| input.set_value("absent", window, cx));
                page.refresh_list(window, cx);
                cx.notify();
            })
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("clear-proxy-filters", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            page.update(cx, |page, cx| {
                assert!(page.group_filter.is_empty());
                assert!(!page.only_available);
                assert!(page.query.read(cx).value().is_empty());
            })
        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }
    #[gpui_kit::test]
    fn group_body_and_following_header_move_continuously_on_reversal(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let service =
            crate::integration::Service::start(crate::storage::Repository::open(paths).unwrap())
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            cx.set_reduce_motion(false);
        });
        let (handle, _) = cx.update(|cx| {
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
                    let session = cx.new(|_| Session::default());
                    cx.new(|cx| {
                        let mut page = ProxiesPage::new(session, window, cx);
                        page.groups = crate::proxies::model::sample_groups();
                        page.card_focus = page
                            .groups
                            .iter()
                            .flat_map(|group| {
                                group
                                    .visible_nodes(
                                        "",
                                        false,
                                        crate::proxies::model::SortOrder::Default,
                                        false,
                                    )
                                    .into_iter()
                                    .map(|node| ((group.id(), node.id()), cx.focus_handle()))
                                    .collect::<Vec<_>>()
                            })
                            .collect();
                        page.expanded.insert("proxy".into());
                        page.refresh_list(window, cx);
                        page
                    })
                },
            )
            .unwrap()
        });
        cx.run_until_parked();
        let initial = cx
            .update_window(handle, |_, w, cx| {
                w.render_frame(cx);
                assert!(w.find("node-card-proxy-hk01").visible());
                assert!(w.find("node-card-proxy-hk02").visible());
                w.hover("node-card-proxy-hk01", cx);
                assert!(w.find("node-card-proxy-hk01").visible());
                assert!(w.find("proxy-group-test-proxy").visible());
                assert!(w.find("proxy-group-locate-proxy").visible());
                assert!(w.find("proxy-group-sort-proxy").visible());
                let initial = w.find("proxy-group-auto").bounds().origin.y;
                w.click("proxy-group-proxy", cx);
                let start = w.find("proxy-group-auto").bounds().origin.y;
                assert!((start - initial).abs() < px(1.));
                initial
            })
            .unwrap();
        cx.executor().advance_clock(Duration::from_millis(70));
        let middle = cx
            .update_window(handle, |_, w, cx| {
                w.render_frame(cx);
                let middle = w.find("proxy-group-auto").bounds().origin.y;
                assert!(middle < initial);
                w.click("proxy-group-proxy", cx);
                assert!((w.find("proxy-group-auto").bounds().origin.y - middle).abs() < px(1.));
                middle
            })
            .unwrap();
        cx.executor().advance_clock(Duration::from_millis(220));
        cx.update_window(handle, |_, w, cx| {
            w.render_frame(cx);
            let end = w.find("proxy-group-auto").bounds().origin.y;
            assert!(end > middle);
            assert!((end - initial).abs() < px(1.));
        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }
}
