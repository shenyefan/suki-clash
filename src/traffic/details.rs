use super::{TrafficPage, model::Record, table::CONNECTION_COLUMNS};
use crate::ui::{
    self,
    theme::{control, layout, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Sizable, button::Button, h_flex, menu::DropdownMenu,
        scroll::ScrollableElement, tag::Tag, v_flex,
    },
    *,
};

impl TrafficPage {
    pub(super) fn column_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hidden = self.table.read(cx).delegate().hidden_columns.clone();
        let table = self.table.clone();
        Button::new("connection-columns")
            .outline()
            .with_size(control::STANDARD)
            .icon(IconName::SlidersHorizontal)
            .label("列设置")
            .dropdown_menu(move |mut menu, _, _| {
                for (index, (label, _)) in CONNECTION_COLUMNS.iter().enumerate() {
                    let table = table.clone();
                    let visible = !hidden.contains(&index);
                    menu = menu.item(
                        crate::ui::actions::menu(*label)
                            .checked(visible)
                            .disabled(visible && hidden.len() + 1 == CONNECTION_COLUMNS.len())
                            .on_click(move |_, _, cx| {
                                table.update(cx, |table, cx| {
                                    let delegate = table.delegate_mut();
                                    if !delegate.hidden_columns.remove(&index) {
                                        delegate.hidden_columns.insert(index);
                                    }
                                    table.refresh(cx);
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            })
    }

    pub(super) fn rule_details(&self, record: Record, cx: &mut Context<Self>) -> Div {
        let value = super::table::rule_text(&record);
        ui::bottom_details(cx)
            .child(
                h_flex()
                    .gap(space::MD)
                    .child(div().flex_1().font_weight(text::HEADING).child("规则详情"))
                    .child(
                        Button::new("copy-selected-rule")
                            .outline()
                            .with_size(control::STANDARD)
                            .icon(IconName::Copy)
                            .label("复制规则")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(value.clone()))
                            }),
                    )
                    .child(
                        Button::new("collapse-rule-details")
                            .outline()
                            .with_size(control::STANDARD)
                            .icon(IconName::ChevronDown)
                            .label("收起")
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.table.update(cx, |table, cx| {
                                    table.delegate_mut().detail_id = None;
                                    table.clear_selection(cx);
                                    cx.notify();
                                });
                                this.table.focus_handle(cx).focus(w, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("rule-detail-scroll")
                    .flex_1()
                    .min_h(space::NONE)
                    .overflow_y_scrollbar()
                    .child(
                        v_flex().gap(space::LG).children(
                            [("类型", 1), ("匹配内容", 0), ("出站策略", 2)]
                                .into_iter()
                                .map(|(label, index)| {
                                    v_flex()
                                        .gap(space::XS)
                                        .child(ui::muted(label, cx).text_size(text::CAPTION))
                                        .child(record.cells[index].clone())
                                }),
                        ),
                    ),
            )
    }

    pub(super) fn connection_details(&self, record: Record, cx: &mut Context<Self>) -> Div {
        let owner = cx.weak_entity();
        let columns = self.detail_columns;
        let target = record.connection.clone();
        let extra = |label: &str| {
            record
                .details
                .lines()
                .filter_map(|line| line.split_once('\t'))
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.to_owned())
                .unwrap_or_default()
        };
        let section = |title: &'static str, fields: Vec<(&'static str, String)>| {
            v_flex()
                .min_w(space::NONE)
                .gap(space::MD)
                .child(div().font_weight(text::EMPHASIS).child(title))
                .children(fields.into_iter().map(|(label, value)| {
                    h_flex()
                        .items_start()
                        .gap(space::MD)
                        .min_w(space::NONE)
                        .child(
                            ui::muted(label, cx)
                                .w(layout::DETAIL_LABEL_WIDTH)
                                .flex_shrink_0()
                                .text_size(text::CAPTION),
                        )
                        .child(div().flex_1().min_w(space::NONE).whitespace_normal().child(
                            if value.is_empty() {
                                "—".into()
                            } else {
                                value
                            },
                        ))
                }))
        };
        let sections = [
            section(
                "连接",
                vec![
                    ("主机", record.cells[0].clone()),
                    ("协议", record.cells[11].clone()),
                    ("源地址", record.cells[9].clone()),
                    ("目标地址", record.cells[10].clone()),
                    ("远端目标", extra("远端目标")),
                ],
            ),
            section(
                "流量",
                vec![
                    ("下载速率", record.cells[3].clone()),
                    ("上传速率", record.cells[4].clone()),
                    ("下载总量", record.cells[1].clone()),
                    ("上传总量", record.cells[2].clone()),
                ],
            ),
            section(
                "路由",
                vec![
                    ("规则", record.cells[6].clone()),
                    ("代理链", record.cells[5].clone()),
                ],
            ),
            section(
                "进程与时间",
                vec![
                    ("进程", record.cells[7].clone()),
                    ("进程路径", extra("进程路径")),
                    ("开始时间", {
                        let full = extra("完整开始时间");
                        if full.is_empty() {
                            record.cells[8].clone()
                        } else {
                            full
                        }
                    }),
                    ("连接 ID", extra("连接 ID")),
                ],
            ),
        ];
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
                                    .child(
                                        div()
                                            .min_w(space::NONE)
                                            .font_weight(text::HEADING)
                                            .truncate()
                                            .child(record.cells[7].clone()),
                                    )
                                    .child(Tag::secondary().small().child(if record.active {
                                        "活跃"
                                    } else {
                                        "已关闭"
                                    })),
                            )
                            .child(ui::muted(record.cells[0].clone(), cx).truncate()),
                    )
                    .child(
                        Button::new("close-selected-connection")
                            .outline()
                            .with_size(control::STANDARD)
                            .icon(IconName::X)
                            .label("关闭连接")
                            .disabled(
                                !record.active || !self.session.read(cx).runtime().connections_live,
                            )
                            .on_click(move |_, window, cx| {
                                super::close_connection(target.clone(), window, cx)
                            }),
                    )
                    .child(
                        Button::new("collapse-connection-details")
                            .outline()
                            .with_size(control::STANDARD)
                            .icon(IconName::ChevronDown)
                            .label("收起")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.table.update(cx, |table, cx| {
                                    table.delegate_mut().detail_id = None;
                                    table.clear_selection(cx);
                                    cx.notify();
                                });
                                this.table.focus_handle(cx).focus(window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .on_children_prepainted(move |bounds, window, cx| {
                        let Some(bounds) = bounds.first() else { return };
                        let next = ui::adaptive_columns(
                            bounds.size.width,
                            layout::DETAIL_MIN_WIDTH.to_pixels(window.rem_size()),
                            space::SECTION.to_pixels(window.rem_size()),
                        );
                        if next != columns {
                            let owner = owner.clone();
                            window.defer(cx, move |_, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if this.detail_columns != next {
                                        this.detail_columns = next;
                                        cx.notify();
                                    }
                                });
                            });
                        }
                    })
                    .id("connection-detail-scroll")
                    .flex_1()
                    .min_h(space::NONE)
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .grid()
                            .grid_cols(columns.min(sections.len()) as u16)
                            .gap(space::SECTION)
                            .children(sections),
                    ),
            )
    }
}
