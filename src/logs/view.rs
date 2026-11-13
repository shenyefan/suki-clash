use super::{DismissLogDetails, LogsPage, model::LogEntry};
use crate::{
    ui,
    ui::theme::{control, layout, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, Sizable,
        button::{Button, ButtonVariants, DropdownButton},
        h_flex,
        input::Input,
        menu::DropdownMenu,
        scroll::ScrollableElement,
        scroll::{Scrollbar, ScrollbarMode},
        select::Select,
        tab::{Tab, TabBar},
        table::DataTable,
        tag::Tag,
        v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};

impl LogsPage {
    fn sort_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let delegate = self.table.read(cx).delegate();
        let (column, descending) = delegate.sort.unwrap_or((0, self.descending));
        const LABELS: [&str; 4] = ["时间", "级别", "来源", "内容"];
        let owner = cx.entity();
        DropdownButton::new("logs-sort")
            .outline()
            .with_size(control::STANDARD)
            .menu_accessibility_label("日志排序依据")
            .button(
                Button::new("logs-sort-direction")
                    .icon(if descending {
                        IconName::ArrowDown
                    } else {
                        IconName::ArrowUp
                    })
                    .label(LABELS[column])
                    .accessibility_label("切换日志排序方向")
                    .tooltip(if descending {
                        "当前降序，点击切换升序"
                    } else {
                        "当前升序，点击切换降序"
                    })
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.descending = !descending;
                        this.table.update(cx, |table, cx| {
                            table.delegate_mut().sort = Some((column, !descending));
                            table.refresh(cx);
                        });
                        this.refresh(w, cx);
                    })),
            )
            .dropdown_menu(move |mut menu, _, _| {
                for (ix, label) in LABELS.into_iter().enumerate() {
                    let owner = owner.clone();
                    menu = menu.item(ui::actions::menu(label).checked(ix == column).on_click(
                        move |_, w, cx| {
                            owner.update(cx, |this, cx| {
                                this.table.update(cx, |table, cx| {
                                    table.delegate_mut().sort = Some((ix, descending));
                                    table.refresh(cx);
                                });
                                this.refresh(w, cx);
                            });
                        },
                    ));
                }
                menu
            })
    }
    fn search_options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let options = [
            ("case", "区分大小写", self.search.case_sensitive),
            ("word", "全词匹配", self.search.whole_word),
            ("regex", "正则表达式", self.search.regular_expression),
        ];
        let owner = cx.entity();
        Button::new("logs-search-options")
            .ghost()
            .small()
            .flex_shrink_0()
            .icon(IconName::SlidersHorizontal)
            .accessibility_label("日志搜索选项")
            .tooltip("搜索选项")
            .dropdown_menu(move |mut menu, _, _| {
                for (key, label, enabled) in options {
                    let owner = owner.clone();
                    menu = menu.item(ui::actions::menu(label).checked(enabled).on_click(
                        move |_, w, cx| {
                            owner.update(cx, |this, cx| {
                                let option = match key {
                                    "case" => &mut this.search.case_sensitive,
                                    "word" => &mut this.search.whole_word,
                                    _ => &mut this.search.regular_expression,
                                };
                                *option = !*option;
                                this.refresh(w, cx);
                                if let Some(error) = &this.search.error {
                                    ui::show_error(error.clone(), w, cx);
                                }
                            });
                        },
                    ));
                }
                menu
            })
    }
}

impl Render for LogsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let delegate = self.table.read(cx).delegate();
        let count = delegate.entries.len();
        let detail = delegate
            .detail_id
            .and_then(|id| delegate.entries.iter().find(|entry| entry.id == id))
            .cloned();
        let counts = [0, 1].map(|source| {
            self.buffer
                .entries
                .iter()
                .filter(|entry| entry.source == source)
                .count()
        });
        let total = counts[self.source];
        let table = self.table.downgrade();
        let presence = ui::motion::presence("logs-details", detail.is_some(), window, cx);
        let detail = detail
            .or_else(|| self.detail_cache.clone())
            .filter(|_| presence.should_render());
        ui::workspace()
            .key_context("Logs")
            .on_action(
                cx.listener(|this, _: &DismissLogDetails, w, cx| this.dismiss_details(w, cx)),
            )
            .child(
                ui::workspace_header("日志", "", cx).child(
                    TabBar::new("logs-source")
                        .segmented()
                        .with_size(control::STANDARD)
                        .children([
                            Tab::new().label(format!("内核 {}", counts[0])),
                            Tab::new().label(format!("程序 {}", counts[1])),
                        ])
                        .selected_index(self.source)
                        .on_click(cx.listener(|this, ix, w, cx| {
                            this.source = *ix;
                            this.table.update(cx, |table, cx| {
                                table.delegate_mut().detail_id = None;
                                table.clear_selection(cx);
                            });
                            this.refresh(w, cx);
                        })),
                ),
            )
            .child(
                ui::workspace_toolbar()
                    .child(
                        div().flex_1().min_w(space::NONE).child(
                            Input::new(&self.query)
                                .with_size(control::STANDARD)
                                .w_full()
                                .cleanable(true)
                                .aria_label("搜索日志")
                                .prefix(Icon::new(IconName::Search).size(layout::ICON))
                                .suffix(self.search_options(cx))
                                .when(self.search.error.is_some(), |input| {
                                    input.border_color(cx.theme().danger)
                                }),
                        ),
                    )
                    .child(
                        div().w(layout::FILTER_WIDTH).flex_shrink_0().child(
                            Select::new(&self.level)
                                .with_size(control::STANDARD)
                                .w_full()
                                .accessibility_label("日志级别"),
                        ),
                    )
                    .child(self.sort_button(cx))
                    .child(
                        Button::new("logs-clear")
                            .outline()
                            .with_size(control::STANDARD)
                            .flex_shrink_0()
                            .icon(IconName::Trash)
                            .label("清空")
                            .accessibility_label("清空当前来源的日志")
                            .tooltip("清空日志")
                            .disabled(total == 0)
                            .on_click(cx.listener(|this, _, w, cx| {
                                let source = this.source;
                                let cleared = this.cleared[source];
                                let removed = this
                                    .buffer
                                    .entries
                                    .iter()
                                    .filter(|entry| entry.source == source)
                                    .cloned()
                                    .collect::<Vec<_>>();
                                this.cleared[this.source] = crate::integration::logs(cx)
                                    .since(0)
                                    .last()
                                    .map_or(0, |entry| entry.id);
                                this.buffer.clear(this.source);
                                this.refresh(w, cx);
                                let owner = cx.weak_entity();
                                ui::notifications::undo(w, "已清空日志", cx, move |w, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.cleared[source] =
                                                this.cleared[source].min(cleared);
                                            this.buffer.restore(removed.clone());
                                            this.refresh(w, cx);
                                            true
                                        })
                                        .unwrap_or(true)
                                });
                            })),
                    ),
            )
            .child(
                ui::workspace_body().child(
                    v_flex()
                        .size_full()
                        .min_h(space::NONE)
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h(space::NONE)
                                .child(
                                    canvas(
                                        move |bounds, _, cx| {
                                            if let Some(table) = table.upgrade()
                                                && table.read(cx).delegate().viewport_width
                                                    != bounds.size.width
                                            {
                                                cx.defer(move |cx| {
                                                    table.update(cx, |state, cx| {
                                                        state.delegate_mut().viewport_width =
                                                            bounds.size.width;
                                                        state.refresh(cx);
                                                        cx.notify();
                                                    })
                                                });
                                            }
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .size_full(),
                                )
                                .child(
                                    DataTable::new(&self.table)
                                        .with_size(control::STANDARD)
                                        .stripe(false)
                                        .bordered(false)
                                        .scrollbar_visible(true, false),
                                ),
                        )
                        .child(
                            div().h(Scrollbar::width()).w_full().flex_shrink_0().child(
                                Scrollbar::horizontal(
                                    &self.table.read(cx).horizontal_scroll_handle,
                                )
                                .id("logs-horizontal-scroll")
                                .mode(ScrollbarMode::Always)
                                .viewport_from_layout(),
                            ),
                        ),
                ),
            )
            .when_some(detail, |page, entry| {
                page.child(ui::motion::drawer(
                    presence,
                    self.log_details(entry, cx),
                    window,
                ))
            })
            .child(
                ui::workspace_footer()
                    .child(
                        ui::muted(format!("{count} / {total} 条日志"), cx).text_size(text::CAPTION),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("logs-latest")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowUp)
                            .accessibility_label("跳到最新")
                            .tooltip("跳到最新")
                            .disabled(count == 0)
                            .on_click(cx.listener(|this, _, w, cx| this.jump_to_latest(w, cx))),
                    ),
            )
    }
}

impl LogsPage {
    fn log_details(&self, entry: LogEntry, cx: &mut Context<Self>) -> Div {
        let value = entry.text();
        ui::bottom_details(cx)
            .child(
                h_flex()
                    .gap(space::MD)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(space::NONE)
                            .gap(space::XS)
                            .child(
                                h_flex()
                                    .gap(space::SM)
                                    .child(div().font_weight(text::HEADING).child(entry.time))
                                    .child(Tag::secondary().small().child(entry.level)),
                            )
                            .child(ui::muted(entry.origin.clone(), cx)),
                    )
                    .child(
                        ui::icon_button("copy-log-detail", IconName::Copy, "复制日志").on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(value.clone()))
                            },
                        ),
                    )
                    .child(
                        ui::icon_button(
                            "collapse-log-details",
                            IconName::ChevronDown,
                            "收起日志详情",
                        )
                        .on_click(cx.listener(|this, _, w, cx| this.dismiss_details(w, cx))),
                    ),
            )
            .child(
                div()
                    .id("log-detail-scroll")
                    .flex_1()
                    .min_h(space::NONE)
                    .overflow_y_scrollbar()
                    .child(div().w_full().text_size(text::BODY).child(entry.message)),
            )
    }
}
