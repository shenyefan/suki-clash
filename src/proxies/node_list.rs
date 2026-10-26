use super::{
    ProxiesPage,
    model::{GroupKind, Node, ProxyGroup},
};
use crate::ui::{
    self,
    theme::{control, layout, list, opacity, proxy_card, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon, Sizable,
        button::{Button, ButtonVariants},
        collapsible::Collapsible,
        h_flex,
        menu::{DropdownMenu, PopupMenuItem},
        v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};

#[derive(Clone, Copy)]
struct GroupInfo {
    id: &'static str,
    name: &'static str,
    kind: GroupKind,
    default_node: &'static str,
}
impl From<&ProxyGroup> for GroupInfo {
    fn from(group: &ProxyGroup) -> Self {
        Self {
            id: group.id(),
            name: group.name(),
            kind: group.kind(),
            default_node: group.default_node(),
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
                let nodes =
                    group.visible_nodes(&query, self.only_available, self.sort, self.reverse);
                (!nodes.is_empty()).then_some((GroupInfo::from(group), nodes))
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
                        .outline()
                        .with_size(control::STANDARD)
                        .label("清除筛选")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.only_available = false;
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
        v_flex()
            .py(list::INSET_Y)
            .gap(list::GROUP_GAP)
            .px(list::INSET_X)
            .children(groups.into_iter().map(|(group, nodes)| {
                let open = self.is_expanded(group.id, !query.is_empty());
                let searching = !query.is_empty();
                let motion = cx.theme().motion_tokens();
                let angle = gpui_kit::base::motion::transition(
                    (ElementId::from("proxy-disclosure"), group.id),
                    if open {
                        std::f32::consts::FRAC_PI_2
                    } else {
                        0.
                    },
                    gpui_kit::base::motion::Transition::new(motion.duration_fast)
                        .easing(motion.easing_move.clone()),
                    window,
                    cx,
                );
                let selected = self
                    .session
                    .read(cx)
                    .selected_node(group.name)
                    .unwrap_or(group.default_node);
                let fixed = self.session.read(cx).is_fixed(group.name);
                let height = if self.compact {
                    proxy_card::COMPACT_HEIGHT
                } else {
                    proxy_card::HEIGHT
                };
                let header = h_flex()
                    .gap(space::SM)
                    .bg(cx.theme().muted.opacity(opacity::GROUP_HEADER))
                    .rounded(cx.theme().radius)
                    .child(
                        Button::new(SharedString::from(format!("proxy-group-{}", group.id)))
                            .ghost()
                            .with_size(control::STANDARD)
                            .flex_1()
                            .min_w(space::NONE)
                            .px(list::PADDING_X)
                            .h(height)
                            .accessibility_label(format!(
                                "{}{}，{}个节点，当前节点{}",
                                if open { "收起" } else { "展开" },
                                group.name,
                                nodes.len(),
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
                                    .child(
                                        Icon::new(match group.kind {
                                            GroupKind::Select => IconName::Route,
                                            GroupKind::UrlTest => IconName::Zap,
                                            GroupKind::Media => IconName::TvMinimalPlay,
                                        })
                                        .size(layout::ICON_LG),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .child(
                                                h_flex()
                                                    .gap(space::SM)
                                                    .child(
                                                        div()
                                                            .font_weight(text::EMPHASIS)
                                                            .child(group.name),
                                                    )
                                                    .child(ui::muted(group.kind.label(), cx)),
                                            )
                                            .child(
                                                ui::muted(
                                                    format!(
                                                        "{}{}",
                                                        selected,
                                                        if fixed { " · 手动固定" } else { "" }
                                                    ),
                                                    cx,
                                                )
                                                .truncate(),
                                            ),
                                    )
                                    .child(ui::muted(format!("{} 个节点", nodes.len()), cx)),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_group(group.id, searching, window, cx)
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("proxy-group-menu-{}", group.id)))
                            .ghost()
                            .small()
                            .icon(IconName::Ellipsis)
                            .accessibility_label(format!("{}操作", group.name))
                            .dropdown_menu(move |menu, _, _| {
                                menu.item(
                                    PopupMenuItem::new("测试此代理组延迟")
                                        .icon(IconName::Zap)
                                        .on_click(|_, window, cx| {
                                            ui::request_command("测试此代理组延迟", window, cx)
                                        }),
                                )
                                .when(fixed, |menu| {
                                    menu.separator().item(
                                        PopupMenuItem::new("恢复自动选择")
                                            .icon(IconName::PinOff)
                                            .on_click(move |_, window, cx| {
                                                ui::request_command("恢复自动选择", window, cx)
                                            }),
                                    )
                                })
                            }),
                    );
                Collapsible::new()
                    .motion_id((ElementId::from("proxy-group-body"), group.id))
                    .open(open)
                    .child(header)
                    .content(
                        div()
                            .pt(space::SM)
                            .grid()
                            .grid_cols(self.columns as u16)
                            .gap(proxy_card::GAP)
                            .children(nodes.into_iter().map(|node| {
                                div()
                                    .h(height)
                                    .min_w(space::NONE)
                                    .child(self.node_card(group, node, open, cx))
                            })),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }
    fn node_card(&self, group: GroupInfo, node: Node, open: bool, cx: &App) -> AnyElement {
        let active = self
            .session
            .read(cx)
            .selected_node(group.name)
            .unwrap_or(group.default_node)
            == node.name();
        let focus = self.card_focus[&(group.id, node.id())].clone();
        let keyboard_focus = focus.clone();
        v_flex()
            .id(SharedString::from(format!(
                "node-card-{}-{}",
                group.id,
                node.id()
            )))
            .track_focus(&focus)
            .role(Role::Button)
            .tab_stop(open)
            .aria_label(format!(
                "{}：{}{}",
                group.name,
                node.name(),
                if active { "，当前使用" } else { "" }
            ))
            .size_full()
            .line_height(relative(text::LINE_HEIGHT))
            .min_w(space::NONE)
            .px(proxy_card::PADDING)
            .justify_center()
            .gap(space::XS)
            .rounded(cx.theme().radius)
            .bg(if active {
                ui::theme::selection_tint(cx)
            } else {
                cx.theme().muted.opacity(opacity::PANEL)
            })
            .hover(move |card| {
                if active {
                    card
                } else {
                    card.bg(cx.theme().list_hover)
                }
            })
            .border_1()
            .border_color(cx.theme().ring.opacity(opacity::TRANSPARENT))
            .focus_visible(|card| card.border_color(cx.theme().ring))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                ui::request_command("切换代理节点", window, cx);
            })
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if keyboard_focus.is_focused(window)
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    cx.stop_propagation();
                    ui::request_command("切换代理节点", window, cx);
                }
            })
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        div()
                            .flex_1()
                            .min_w(space::NONE)
                            .truncate()
                            .font_weight(text::EMPHASIS)
                            .child(node.name()),
                    )
                    .child(div().w(layout::ICON).when(active, |slot| {
                        slot.child(Icon::new(IconName::Check).size(layout::ICON))
                    })),
            )
            .child(
                h_flex()
                    .gap(space::SM)
                    .child(
                        ui::muted(format!("{} · {}", node.region(), node.protocol()), cx)
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
                        .label(
                            node.delay()
                                .map(|delay| format!("{delay} ms"))
                                .unwrap_or_else(|| "超时".into()),
                        )
                        .text_color(if node.delay().is_some() {
                            cx.theme().muted_foreground
                        } else {
                            cx.theme().warning
                        })
                        .accessibility_label(format!("测试{}的{}延迟", group.name, node.name()))
                        .tooltip("测试此节点延迟")
                        .on_click(|_, window, cx| {
                            cx.stop_propagation();
                            ui::request_command("测试节点延迟", window, cx);
                        }),
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
    fn group_body_and_following_header_move_continuously_on_reversal(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
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
                        page.expanded.insert("proxy");
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
    }
}
