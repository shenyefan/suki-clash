use super::{DismissConnectionDetails, TrafficKind, TrafficPage};
use crate::{
    ui,
    ui::theme::{control, layout, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Icon, Sizable,
        button::{Button, ButtonVariants, DropdownButton},
        h_flex,
        input::Input,
        menu::DropdownMenu,
        scroll::{Scrollbar, ScrollbarMode},
        select::Select,
        tab::{Tab, TabBar},
        table::DataTable,
        v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};

impl TrafficPage {
    fn search_options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let search = &self.table.read(cx).delegate().search;
        let options = [
            ("case", "区分大小写", search.case_sensitive),
            ("word", "全词匹配", search.whole_word),
            ("regex", "正则表达式", search.regular_expression),
        ];
        let table = self.table.clone();
        Button::new("records-search-options")
            .ghost()
            .small()
            .icon(IconName::SlidersHorizontal)
            .accessibility_label("搜索选项")
            .dropdown_menu(move |mut menu, _, _| {
                for (key, label, checked) in options {
                    let table = table.clone();
                    menu = menu.item(crate::ui::actions::menu(label).checked(checked).on_click(
                        move |_, window, cx| {
                            let error = table.update(cx, |table, cx| {
                                let search = &mut table.delegate_mut().search;
                                let value = match key {
                                    "case" => &mut search.case_sensitive,
                                    "word" => &mut search.whole_word,
                                    _ => &mut search.regular_expression,
                                };
                                *value = !*value;
                                table.delegate_mut().rebuild();
                                let error = table.delegate().search.error.clone();
                                table.clear_selection(cx);
                                cx.notify();
                                error
                            });
                            if let Some(error) = error {
                                ui::show_error(error, window, cx);
                            }
                        },
                    ));
                }
                menu
            })
    }
    fn sort_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rules = self.kind == TrafficKind::Rules;
        let (sort, reverse) = if rules {
            self.table
                .read(cx)
                .delegate()
                .sort
                .map(|(column, descending)| {
                    (
                        match column {
                            usize::MAX => 0,
                            0 => 1,
                            1 => 2,
                            _ => 3,
                        },
                        descending,
                    )
                })
                .unwrap_or((0, false))
        } else {
            (self.sort_index, self.reverse)
        };
        let owner = cx.entity();
        let labels: &[&str] = if rules {
            &["配置顺序", "匹配内容", "类型", "策略"]
        } else {
            &[
                "配置顺序",
                "目标主机",
                "进程",
                "代理链",
                "上传速度",
                "下载速度",
            ]
        };
        DropdownButton::new("connection-sort")
            .ghost()
            .with_size(control::STANDARD)
            .menu_accessibility_label(if rules {
                "规则排序依据"
            } else {
                "连接排序依据"
            })
            .button(
                Button::new("connection-sort-direction")
                    .icon(if reverse {
                        IconName::ArrowDown
                    } else {
                        IconName::ArrowUp
                    })
                    .label(labels[sort])
                    .tooltip(if reverse {
                        "当前降序，点击切换升序"
                    } else {
                        "当前升序，点击切换降序"
                    })
                    .accessibility_label(if rules {
                        "切换规则排序方向"
                    } else {
                        "切换连接排序方向"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reverse = !reverse;
                        this.apply_sort(sort, cx);
                    })),
            )
            .dropdown_menu(move |mut menu, _, _| {
                for (index, label) in labels.iter().copied().enumerate() {
                    let owner = owner.clone();
                    menu = menu.item(
                        crate::ui::actions::menu(label)
                            .checked(index == sort)
                            .on_click(move |_, _, cx| {
                                owner.update(cx, |this, cx| {
                                    this.reverse = reverse;
                                    this.apply_sort(index, cx)
                                })
                            }),
                    );
                }
                menu
            })
    }
}

impl Render for TrafficPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        let delegate = self.table.read(cx).delegate();
        let detail = delegate
            .detail_id
            .and_then(|id| delegate.records.iter().find(|record| record.id == id))
            .cloned();
        let horizontal_scroll = self.table.read(cx).horizontal_scroll_handle.clone();
        let tab = delegate.tab;
        let count = delegate.visible.len();
        let count0 = delegate.count(0);
        let count1 = delegate.count(1);
        let (up, down) =
            delegate
                .records
                .iter()
                .filter(|r| r.active)
                .fold((0., 0.), |(up, down), r| {
                    (
                        up + r.numbers.get(4).unwrap_or(&0.),
                        down + r.numbers.get(3).unwrap_or(&0.),
                    )
                });
        let title = match kind {
            TrafficKind::Connections => "连接",
            TrafficKind::Rules => "规则",
            TrafficKind::Logs => "日志",
        };
        let labels = match kind {
            TrafficKind::Connections => vec![format!("活跃 {count0}"), format!("已关闭 {count1}")],
            TrafficKind::Logs => vec!["程序".into(), "内核".into()],
            TrafficKind::Rules => vec![],
        };
        let speed = if kind == TrafficKind::Connections
            && !self.session.read(cx).runtime().connections_live
        {
            "实时数据待确认".into()
        } else if kind == TrafficKind::Connections {
            format!("↑ {up:.0} KB/s    ↓ {:.2} MB/s", down / 1024.)
        } else if kind == TrafficKind::Rules {
            let core = self.session.read(cx).core();
            if !self.session.read(cx).is_running() {
                "内核未运行".into()
            } else if (core.rules().generation, core.rules().epoch)
                != (core.generation(), core.config_epoch())
            {
                "规则待刷新".into()
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        let header = ui::workspace_header(title, &speed, cx)
            .when(kind == TrafficKind::Connections, |row| {
                row.child(
                    TabBar::new("connection-status")
                        .segmented()
                        .with_size(control::STANDARD)
                        .children([
                            Tab::new().label(format!("活跃 {count0}")),
                            Tab::new().label(format!("已关闭 {count1}")),
                        ])
                        .selected_index(tab)
                        .on_click(cx.listener(|this, index, _, cx| this.switch_tab(*index, cx))),
                )
            })
            .when(kind == TrafficKind::Rules, |row| {
                row.child(
                    Button::new("refresh-rules")
                        .outline()
                        .with_size(control::STANDARD)
                        .icon(IconName::RefreshCw)
                        .label("刷新规则")
                        .disabled(
                            self.session.read(cx).core_busy()
                                || !self.session.read(cx).is_running(),
                        )
                        .on_click(|_, w, cx| {
                            crate::core::request(crate::core::Command::Rules, w, cx)
                        }),
                )
            })
            .when(kind == TrafficKind::Logs, |row| {
                row.child(
                    ui::icon_button("copy-logs", IconName::Copy, "复制日志").on_click(cx.listener(
                        |this, _, _, cx| {
                            let delegate = this.table.read(cx).delegate();
                            let value = delegate
                                .visible
                                .iter()
                                .map(|i| delegate.records[*i].cells.join("  "))
                                .collect::<Vec<_>>()
                                .join("\n");
                            cx.write_to_clipboard(ClipboardItem::new_string(value));
                        },
                    )),
                )
                .child(
                    ui::icon_button(
                        "pause-logs",
                        if self.paused {
                            IconName::Play
                        } else {
                            IconName::Pause
                        },
                        if self.paused {
                            "继续刷新"
                        } else {
                            "暂停刷新"
                        },
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.paused = !this.paused;
                        if !this.paused && this.kind == TrafficKind::Connections {
                            this.sync_connection_rows(cx);
                        }
                        cx.notify();
                    })),
                )
            })
            .when(kind == TrafficKind::Logs, |row| {
                row.child(
                    Button::new("clear-records")
                        .ghost()
                        .with_size(control::STANDARD)
                        .icon(IconName::Trash)
                        .label("清空日志")
                        .disabled(count == 0)
                        .on_click(cx.listener(|this, _, w, cx| this.clear_or_close(w, cx))),
                )
            });
        let presence = ui::motion::presence("traffic-details", detail.is_some(), window, cx);
        let detail = detail
            .or_else(|| self.detail_cache.clone())
            .filter(|_| presence.should_render());
        ui::workspace()
            .when(kind != TrafficKind::Logs, |page| {
                page.key_context("Connections")
            })
            .on_action(
                cx.listener(|this, _: &DismissConnectionDetails, window, cx| {
                    this.table.update(cx, |table, cx| {
                        table.delegate_mut().detail_id = None;
                        table.clear_selection(cx);
                        cx.notify();
                    });
                    this.table.focus_handle(cx).focus(window, cx);
                }),
            )
            .child(header)
            .when(kind == TrafficKind::Logs, |col| {
                col.child(
                    h_flex().justify_between().gap(space::MD).child(
                        TabBar::new("traffic-tabs")
                            .segmented()
                            .with_size(control::STANDARD)
                            .children(labels.into_iter().map(|label| Tab::new().label(label)))
                            .selected_index(tab)
                            .on_click(
                                cx.listener(|this, index, _, cx| this.switch_tab(*index, cx)),
                            ),
                    ),
                )
            })
            .child(
                ui::workspace_toolbar()
                    .child(
                        div().flex_1().min_w(space::NONE).child(
                            Input::new(&self.query)
                                .with_size(control::STANDARD)
                                .w_full()
                                .prefix(Icon::new(IconName::Search).size(layout::ICON))
                                .suffix(self.search_options(cx))
                                .cleanable(true)
                                .aria_label(if kind == TrafficKind::Rules {
                                    "搜索规则、类型或策略"
                                } else {
                                    "搜索记录"
                                }),
                        ),
                    )
                    .when(kind != TrafficKind::Connections, |row| {
                        row.child(
                            div().w(layout::FILTER_WIDTH).flex_shrink_0().child(
                                Select::new(&self.filter)
                                    .with_size(control::STANDARD)
                                    .w_full()
                                    .accessibility_label(if kind == TrafficKind::Logs {
                                        "日志级别"
                                    } else {
                                        "规则类型"
                                    }),
                            ),
                        )
                    })
                    .when(kind == TrafficKind::Connections, |row| {
                        row.child(self.sort_button(cx))
                            .child(self.column_button(cx))
                            .when(tab == 1, |row| {
                                row.child(
                                    Button::new("clear-closed-connections")
                                        .outline()
                                        .with_size(control::STANDARD)
                                        .icon(IconName::Trash)
                                        .label("清空已关闭")
                                        .disabled(count1 == 0)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.connection_records.clear_closed();
                                            this.table.update(cx, |table, cx| {
                                                table
                                                    .delegate_mut()
                                                    .records
                                                    .retain(|record| record.active);
                                                table.delegate_mut().detail_id = None;
                                                table.delegate_mut().rebuild();
                                                table.clear_selection(cx);
                                                cx.notify();
                                            });
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(
                                Button::new("clear-records")
                                    .outline()
                                    .with_size(control::STANDARD)
                                    .icon(IconName::Unplug)
                                    .label("关闭全部")
                                    .disabled(
                                        count0 == 0
                                            || tab == 1
                                            || !self.session.read(cx).runtime().connections_live,
                                    )
                                    .on_click(
                                        cx.listener(|this, _, w, cx| this.clear_or_close(w, cx)),
                                    ),
                            )
                    })
                    .when(kind == TrafficKind::Rules, |row| {
                        row.child(self.sort_button(cx)).child(
                            Button::new("copy-visible-rules")
                                .ghost()
                                .with_size(control::STANDARD)
                                .icon(IconName::Copy)
                                .label("复制")
                                .disabled(count == 0)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let delegate = this.table.read(cx).delegate();
                                    let value = delegate
                                        .visible
                                        .iter()
                                        .map(|index| {
                                            super::table::rule_text(&delegate.records[*index])
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    cx.write_to_clipboard(ClipboardItem::new_string(value));
                                })),
                        )
                    }),
            )
            .child(
                ui::workspace_body().child(
                    v_flex()
                        .size_full()
                        .min_h(space::NONE)
                        .child(
                            div().flex_1().min_h(space::NONE).child(
                                DataTable::new(&self.table)
                                    .with_size(control::STANDARD)
                                    .stripe(false)
                                    .scrollbar_visible(true, false)
                                    .bordered(false),
                            ),
                        )
                        .when(kind != TrafficKind::Logs, |body| {
                            body.child(
                                div().h(Scrollbar::width()).w_full().flex_shrink_0().child(
                                    Scrollbar::horizontal(&horizontal_scroll)
                                        .id("connections-horizontal-scroll")
                                        .mode(ScrollbarMode::Always)
                                        .viewport_from_layout(),
                                ),
                            )
                        }),
                ),
            )
            .when_some(
                detail.filter(|_| kind != TrafficKind::Logs),
                |page, record| {
                    if kind == TrafficKind::Rules {
                        page.child(ui::motion::drawer(
                            presence,
                            self.rule_details(record, cx),
                            window,
                        ))
                    } else {
                        page.child(ui::motion::drawer(
                            presence,
                            self.connection_details(record, cx),
                            window,
                        ))
                    }
                },
            )
            .child(
                ui::workspace_footer()
                    .child(
                        ui::muted(
                            if kind == TrafficKind::Rules {
                                format!("{count} / {count0} 条规则")
                            } else {
                                format!("{count} 条记录")
                            },
                            cx,
                        )
                        .text_size(text::CAPTION),
                    )
                    .child(div().flex_1())
                    .when(kind == TrafficKind::Rules, |row| {
                        row.child(
                            ui::muted("按配置顺序匹配，首条命中后停止", cx)
                                .text_size(text::CAPTION),
                        )
                    })
                    .when(kind == TrafficKind::Logs && self.paused, |row| {
                        row.child(ui::muted("刷新已暂停", cx).text_size(text::CAPTION))
                    }),
            )
    }
}

#[cfg(test)]
mod motion_tests {
    use super::{TrafficKind, TrafficPage};
    use crate::session::Session;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
    use std::time::Duration;
    #[gpui_kit::test]
    fn details_clip_from_zero_and_keep_content_until_exit_finishes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.set_reduce_motion(false);
        });
        let (handle, _) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1100.), px(800.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let session = cx.new(|_| Session::default());
                    cx.new(|cx| {
                        let page = TrafficPage::new(TrafficKind::Connections, session, window, cx);
                        page.table.update(cx, |table, cx| {
                            table.delegate_mut().records =
                                crate::traffic::model::samples(TrafficKind::Connections);
                            table.delegate_mut().rebuild();
                            cx.notify();
                        });
                        page
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(handle, |_, w, cx| {
            w.render_frame(cx);
            w.click(("record", 1usize), cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, w, cx| {
            w.render_frame(cx);
            assert_eq!(w.find("detail-drawer").bounds().size.height, px(0.));
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(70));
        let partial = cx
            .update_window(handle, |_, w, cx| {
                w.render_frame(cx);
                w.find("detail-drawer").bounds().size.height
            })
            .unwrap();
        assert!(partial > px(0.));
        cx.executor().advance_clock(Duration::from_millis(220));
        let full = cx
            .update_window(handle, |_, w, cx| {
                w.render_frame(cx);
                let full = w.find("detail-drawer").bounds().size.height;
                assert!(full > partial);
                w.click("collapse-connection-details", cx);
                assert_eq!(w.find("detail-drawer").bounds().size.height, full);
                full
            })
            .unwrap();
        cx.executor().advance_clock(Duration::from_millis(70));
        cx.update_window(handle, |_, w, cx| {
            w.render_frame(cx);
            let closing = w.find("detail-drawer").bounds().size.height;
            assert!(closing > px(0.) && closing < full);
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(220));
        cx.update_window(handle, |_, w, cx| {
            w.render_frame(cx);
            assert!(w.find_all("detail-drawer").is_empty());
        })
        .unwrap();
    }
}
