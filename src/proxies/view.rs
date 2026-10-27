use super::{ProxiesPage, model::SortOrder};
use crate::ui::theme::{control, layout, proxy_card, space, text};
use crate::{session::ProxyMode, ui};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, Selectable, Sizable,
        button::{Button, ButtonVariants, DropdownButton},
        input::Input,
        menu::DropdownMenu,
    },
    *,
};

impl ProxiesPage {
    fn sort_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity();
        let sort = self.sort;
        let reverse = self.reverse;
        DropdownButton::new("proxy-sort")
            .ghost()
            .with_size(control::STANDARD)
            .menu_accessibility_label("节点排序依据")
            .button(
                Button::new("proxy-sort-direction")
                    .icon(if reverse {
                        IconName::ArrowDown
                    } else {
                        IconName::ArrowUp
                    })
                    .label(sort.label())
                    .tooltip(if reverse {
                        "当前降序，点击切换升序"
                    } else {
                        "当前升序，点击切换降序"
                    })
                    .accessibility_label("切换节点排序方向")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.reverse = !this.reverse;
                        this.refresh_list(window, cx);
                        cx.notify();
                    })),
            )
            .dropdown_menu(move |mut menu, _, _| {
                for order in SortOrder::ALL {
                    let owner = owner.clone();
                    menu = menu.item(
                        crate::ui::actions::menu(order.label())
                            .checked(order == sort)
                            .on_click(move |_, window, cx| {
                                owner.update(cx, |this, cx| {
                                    this.sort = order;
                                    this.group_sorts.clear();
                                    this.refresh_list(window, cx);
                                    cx.notify();
                                })
                            }),
                    );
                }
                menu
            })
    }

    fn display_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity();
        let compact = self.compact;
        let available = self.only_available;
        Button::new("proxy-display")
            .ghost()
            .with_size(control::STANDARD)
            .icon(IconName::SlidersHorizontal)
            .label("显示")
            .selected(available || compact)
            .child(Icon::new(IconName::ChevronDown).size(layout::ICON_SM))
            .dropdown_menu(move |menu, _, _| {
                let filter = owner.clone();
                let density = owner.clone();
                let expand = owner.clone();
                let collapse = owner.clone();
                menu.item(
                    crate::ui::actions::menu("仅显示可用节点")
                        .checked(available)
                        .on_click(move |_, window, cx| {
                            filter.update(cx, |this, cx| {
                                this.only_available = !this.only_available;
                                this.refresh_list(window, cx);
                                cx.notify();
                            })
                        }),
                )
                .item(
                    crate::ui::actions::menu("紧凑列表")
                        .checked(compact)
                        .on_click(move |_, window, cx| {
                            density.update(cx, |this, cx| {
                                this.compact = !this.compact;
                                this.refresh_list(window, cx);
                                cx.notify();
                            })
                        }),
                )
                .separator()
                .item(
                    crate::ui::actions::menu("展开全部分组")
                        .icon(IconName::ChevronsUpDown)
                        .on_click(move |_, window, cx| {
                            expand.update(cx, |this, cx| this.expand_all(true, window, cx))
                        }),
                )
                .item(
                    crate::ui::actions::menu("收起全部分组")
                        .icon(IconName::ChevronsDownUp)
                        .on_click(move |_, window, cx| {
                            collapse.update(cx, |this, cx| this.expand_all(false, window, cx))
                        }),
                )
                .separator()
                .item(
                    crate::ui::actions::menu("刷新代理组")
                        .icon(IconName::RefreshCw)
                        .on_click(|_, window, cx| {
                            crate::core::request(
                                crate::core::Command::Proxy(super::runtime::Action::Refresh),
                                window,
                                cx,
                            )
                        }),
                )
            })
    }

    fn toolbar(&self, disabled: bool, cx: &mut Context<Self>) -> Div {
        let runtime = self.session.read(cx).runtime();
        let generation = runtime.generation;
        let epoch = runtime.epoch;
        ui::workspace_toolbar()
            .child(
                Input::new(&self.query)
                    .aria_label("搜索代理组或节点")
                    .with_size(control::STANDARD)
                    .w(layout::SEARCH_WIDTH)
                    .flex_1()
                    .min_w(space::NONE)
                    .prefix(Icon::new(IconName::Search).size(layout::ICON))
                    .cleanable(true),
            )
            .child(self.sort_button(cx))
            .child(self.display_button(cx))
            .child(
                Button::new("proxy-chain")
                    .ghost()
                    .with_size(control::STANDARD)
                    .icon(IconName::Link)
                    .label("链式代理")
                    .on_click(cx.listener(|this, _, w, cx| this.edit_chain(w, cx))),
            )
            .child(
                Button::new("test-all-groups")
                    .outline()
                    .with_size(control::STANDARD)
                    .icon(IconName::Gauge)
                    .label("测试延迟")
                    .disabled(disabled)
                    .on_click(move |_, window, cx| {
                        crate::core::test_delay(
                            super::runtime::Test {
                                group: None,
                                node: None,
                                generation,
                                epoch,
                            },
                            window,
                            cx,
                        )
                    }),
            )
    }
}

impl Render for ProxiesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let direct = self.session.read(cx).mode() == Some(ProxyMode::Direct);
        let profile = self
            .session
            .read(cx)
            .profile_name()
            .unwrap_or("未选择配置")
            .to_owned();
        let query = self.query.read(cx).value().trim().to_lowercase();
        let group_count = self
            .groups
            .iter()
            .filter(|group| {
                !group
                    .visible_nodes(&query, self.only_available, self.sort, self.reverse)
                    .is_empty()
            })
            .count();
        let content = self.group_content(window, cx);
        let owner = cx.weak_entity();
        let current_columns = self.columns;
        ui::workspace()
            .child(
                ui::workspace_header("代理组", &profile, cx).child(ui::mode_tabs(
                    &self.session,
                    "proxies-mode",
                    cx,
                )),
            )
            .child(self.toolbar(self.groups.is_empty() || direct, cx))
            .child(
                ui::workspace_body()
                    .on_children_prepainted(move |bounds, window, cx| {
                        let Some(bounds) = bounds.first() else {
                            return;
                        };
                        let gap = proxy_card::GAP.to_pixels(window.rem_size());
                        let min = proxy_card::MIN_WIDTH.to_pixels(window.rem_size());
                        let inset = crate::ui::theme::list::INSET_X.to_pixels(window.rem_size());
                        let columns = (((bounds.size.width - inset * 2. + gap) / (min + gap))
                            .floor() as usize)
                            .clamp(proxy_card::MIN_COLUMNS, proxy_card::MAX_COLUMNS);
                        if columns != current_columns {
                            let owner = owner.clone();
                            window.defer(cx, move |window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if this.columns != columns {
                                        this.columns = columns;
                                        this.refresh_list(window, cx);
                                        cx.notify();
                                    }
                                });
                            });
                        }
                    })
                    .child(div().id("proxy-group-scroll").size_full().child(content)),
            )
            .child(
                ui::workspace_footer()
                    .text_size(text::CAPTION)
                    .text_color(cx.theme().muted_foreground)
                    .child(if !direct {
                        format!("{group_count} 个代理组")
                    } else {
                        "0 个代理组".into()
                    })
                    .when(self.only_available, |row| row.child("· 仅显示可用节点"))
                    .child(div().flex_1()),
            )
    }
}
