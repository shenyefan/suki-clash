use super::{TrafficPage, model::Record, table::CONNECTION_COLUMNS};
use crate::ui::{
    self,
    theme::{control, layout, space, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        Disableable, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        menu::DropdownMenu,
        scroll::ScrollableElement,
        tag::Tag,
        v_flex,
    },
    *,
};

impl TrafficPage {
    pub(super) fn column_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hidden = self.table.read(cx).delegate().hidden_columns.clone();
        let table = self.table.clone();
        Button::new("connection-columns")
            .ghost()
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
                let table = table.clone();
                menu.separator()
                    .item(
                        crate::ui::actions::menu("恢复默认列").on_click(move |_, _, cx| {
                            table.update(cx, |table, cx| {
                                table.delegate_mut().hidden_columns.clear();
                                table.delegate_mut().column_order =
                                    (0..CONNECTION_COLUMNS.len()).collect();
                                table.refresh(cx);
                                cx.notify();
                            });
                        }),
                    )
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
                            .ghost()
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
        let target = record.connection.clone();
        let fields = |indices: &[usize]| {
            v_flex()
                .gap(space::MD)
                .children(indices.iter().map(|index| {
                    v_flex()
                        .min_w(space::NONE)
                        .gap(space::XS)
                        .child(ui::muted(CONNECTION_COLUMNS[*index].0, cx).text_size(text::CAPTION))
                        .child(div().truncate().child(record.cells[*index].clone()))
                }))
        };
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
                    .id("connection-detail-scroll")
                    .flex_1()
                    .min_h(space::NONE)
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .grid()
                            .grid_cols(layout::CONNECTION_DETAILS_COLUMNS)
                            .gap(space::SECTION)
                            .child(fields(&[11, 5, 6]))
                            .child(fields(&[3, 4]))
                            .child(fields(&[1, 2]))
                            .child(fields(&[9, 10, 8])),
                    ),
            )
    }
}
