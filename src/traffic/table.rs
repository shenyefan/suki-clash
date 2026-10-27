use super::{
    TrafficKind,
    model::{self, Record},
};
use crate::ui;
use crate::ui::theme::{column, layout, space, text};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Sizable, h_flex,
        menu::PopupMenu,
        table::{Column, ColumnSort, TableDelegate, TableState},
    },
    *,
};

pub(super) fn rule_text(record: &Record) -> String {
    if record.cells[1] == "MATCH" {
        format!("MATCH,{}", record.cells[2])
    } else {
        format!(
            "{},{},{}",
            record.cells[1], record.cells[0], record.cells[2]
        )
    }
}

pub(super) const CONNECTION_COLUMNS: [(&str, f32); 12] = [
    ("主机", column::HOST),
    ("已下载", column::TRAFFIC),
    ("已上传", column::TRAFFIC),
    ("下载速度", column::TRAFFIC),
    ("上传速度", column::TRAFFIC),
    ("链路", column::CHAIN),
    ("规则", column::CONNECTION_RULE),
    ("进程", column::CONNECTION_PROCESS),
    ("连接时间", column::LAST_HIT),
    ("源地址", column::ADDRESS),
    ("目标地址", column::ADDRESS),
    ("类型", column::CONNECTION_TYPE),
];

pub(super) struct RecordsTable {
    pub(super) kind: TrafficKind,
    pub(super) live: bool,
    pub(super) records: Vec<Record>,
    pub(super) visible: Vec<usize>,
    pub(super) query: String,
    pub(super) search: crate::search::Search,
    pub(super) tab: usize,
    pub(super) filter: String,
    pub(super) sort: Option<(usize, bool)>,
    pub(super) detail_id: Option<u64>,
    pub(super) column_order: Vec<usize>,
    pub(super) hidden_columns: std::collections::BTreeSet<usize>,
}
impl RecordsTable {
    pub(super) fn new(kind: TrafficKind) -> Self {
        let mut table = Self {
            kind,
            live: false,
            records: Vec::new(),
            visible: vec![],
            query: String::new(),
            search: Default::default(),
            tab: 0,
            filter: String::new(),
            sort: None,
            detail_id: None,
            column_order: (0..CONNECTION_COLUMNS.len()).collect(),
            hidden_columns: Default::default(),
        };
        table.rebuild();
        table
    }
    pub(super) fn rebuild(&mut self) {
        self.search.compile(&self.query);
        self.visible = {
            self.records
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    let matches_tab = match self.kind {
                        TrafficKind::Connections => r.active == (self.tab == 0),
                        TrafficKind::Logs => r.source == self.tab,
                        TrafficKind::Rules => true,
                    };
                    let matches_filter = self.filter.is_empty()
                        || self.filter == "全部"
                        || self.filter == "全部类型"
                        || r.cells.iter().any(|v| v == &self.filter);
                    matches_tab && matches_filter && self.search.matches(&r.cells.join("  "))
                })
                .map(|(index, _)| index)
                .collect()
        };
        if let Some((column, descending)) = self.sort {
            if column == usize::MAX {
                if descending {
                    self.visible.reverse();
                }
            } else {
                self.visible =
                    model::sorted_indices(&self.records, self.visible.clone(), column, descending);
            }
        }
    }
    pub(super) fn count(&self, tab: usize) -> usize {
        self.records
            .iter()
            .filter(|r| match self.kind {
                TrafficKind::Connections => r.active == (tab == 0),
                TrafficKind::Logs => r.source == tab,
                TrafficKind::Rules => true,
            })
            .count()
    }
    pub(super) fn visible_columns(&self) -> Vec<usize> {
        self.column_order
            .iter()
            .copied()
            .filter(|index| !self.hidden_columns.contains(index))
            .collect()
    }

    fn data_column(&self, column: usize) -> usize {
        if self.kind == TrafficKind::Connections {
            self.visible_columns()[column]
        } else if self.kind == TrafficKind::Rules {
            column.checked_sub(1).unwrap_or(usize::MAX)
        } else {
            column
        }
    }

    #[cfg(test)]
    pub(super) fn close_connection(&mut self, id: u64) {
        if let Some(row) = self.records.iter_mut().find(|row| row.id == id) {
            row.active = false;
        }
        self.rebuild();
    }

    fn actions(&self, row: Record, _cx: &mut Context<TableState<Self>>) -> AnyElement {
        let id = row.id;
        let target = row.connection.clone();
        let title = match self.kind {
            TrafficKind::Connections => "连接详情",
            TrafficKind::Rules => "规则详情",
            TrafficKind::Logs => "日志详情",
        };
        let details = row.details.clone();
        let kind = self.kind;
        h_flex()
            .gap(space::XS)
            .child(
                ui::icon_button(("record-details", id), IconName::Info, "查看详情")
                    .small()
                    .on_click(move |_, window, cx| {
                        ui::detail_dialog(title, details.clone(), window, cx)
                    }),
            )
            .child(match kind {
                TrafficKind::Connections if row.active => {
                    ui::icon_button(("close-connection", id), IconName::X, "关闭此连接")
                        .small()
                        .disabled(!self.live)
                        .on_click(move |_, window, cx| {
                            super::close_connection(target.clone(), window, cx)
                        })
                        .into_any_element()
                }
                TrafficKind::Rules => {
                    let value = rule_text(&row);
                    ui::icon_button(("copy-rule", id), IconName::Copy, "复制规则")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(value.clone()))
                        })
                        .into_any_element()
                }
                _ => div().into_any_element(),
            })
            .into_any_element()
    }
    fn headers(&self) -> Vec<(&'static str, f32)> {
        match self.kind {
            TrafficKind::Connections => self
                .visible_columns()
                .into_iter()
                .map(|index| CONNECTION_COLUMNS[index])
                .collect(),
            TrafficKind::Rules => vec![
                ("顺序", column::RATE),
                ("匹配内容", column::CONTENT),
                ("类型", column::RULE_TYPE),
                ("策略", column::STRATEGY),
            ],
            TrafficKind::Logs => vec![
                ("时间", column::TIME),
                ("级别", column::RATE),
                ("来源", column::PROCESS),
                ("消息", column::MESSAGE),
            ],
        }
    }
}
impl TableDelegate for RecordsTable {
    fn columns_count(&self, _: &App) -> usize {
        self.headers().len()
    }
    fn rows_count(&self, _: &App) -> usize {
        self.visible.len()
    }
    fn column(&self, index: usize, cx: &App) -> Column {
        let headers = self.headers();
        let (name, width) = headers[index];
        let column = Column::new(format!("column-{}", self.data_column(index)), name)
            .width(cx.theme().font_size * width);
        column.sortable()
    }
    fn perform_sort(
        &mut self,
        index: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.sort = match sort {
            ColumnSort::Default => None,
            ColumnSort::Ascending => Some((self.data_column(index), false)),
            ColumnSort::Descending => Some((self.data_column(index), true)),
        };
        self.rebuild();
        cx.notify();
    }
    fn render_header(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let header = div().id("records-header");
        if self.kind != TrafficKind::Logs {
            header
        } else {
            header.relative().pr(layout::TABLE_ACTIONS_WIDTH).child(
                h_flex()
                    .absolute()
                    .right(space::NONE)
                    .top(space::NONE)
                    .h_full()
                    .w(layout::TABLE_ACTIONS_WIDTH)
                    .justify_center()
                    .text_size(text::CAPTION)
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.kind == TrafficKind::Rules {
                        "详情 / 复制"
                    } else {
                        "操作"
                    }),
            )
        }
    }

    fn render_tr(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let id = self
            .visible
            .get(index)
            .map(|index| self.records[*index].id)
            .unwrap_or(index as u64);
        let mut row = div().id(("record", id));
        if self.kind == TrafficKind::Logs {
            row = row.relative().pr(layout::TABLE_ACTIONS_WIDTH);
            if let Some(record) = self
                .visible
                .get(index)
                .map(|index| self.records[*index].clone())
            {
                row = row.child(
                    h_flex()
                        .absolute()
                        .right(space::NONE)
                        .top(space::NONE)
                        .h_full()
                        .w(layout::TABLE_ACTIONS_WIDTH)
                        .justify_center()
                        .child(self.actions(record, cx)),
                );
            }
        }
        row
    }
    fn render_td(
        &mut self,
        row_index: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let row = &self.records[self.visible[row_index]];
        if self.kind == TrafficKind::Rules && column == 0 {
            return div()
                .text_size(text::CAPTION)
                .text_color(cx.theme().muted_foreground)
                .child((self.visible[row_index] + 1).to_string())
                .into_any_element();
        }
        let column = self.data_column(column);
        let color = if !row.enabled {
            cx.theme().muted_foreground
        } else if self.kind == TrafficKind::Logs && column == 1 {
            match row.cells[column].as_str() {
                "ERROR" => cx.theme().danger,
                "WARN" => cx.theme().warning,
                _ => cx.theme().muted_foreground,
            }
        } else if column == 0 {
            cx.theme().foreground
        } else {
            cx.theme().muted_foreground
        };
        div()
            .text_size(text::BODY)
            .text_color(color)
            .truncate()
            .child(row.cells.get(column).cloned().unwrap_or_default())
            .into_any_element()
    }
    fn move_column(
        &mut self,
        from: usize,
        to: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        if self.kind == TrafficKind::Connections {
            let visible = self.visible_columns();
            let from = self
                .column_order
                .iter()
                .position(|column| *column == visible[from])
                .unwrap();
            let to = self
                .column_order
                .iter()
                .position(|column| *column == visible[to])
                .unwrap();
            let column = self.column_order.remove(from);
            self.column_order.insert(to, column);
        }
    }

    fn context_menu(
        &mut self,
        row: usize,
        mut menu: PopupMenu,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        if self.kind == TrafficKind::Rules {
            if let Some(record) = self.visible.get(row).map(|index| &self.records[*index]) {
                let value = rule_text(record);
                return menu.item(
                    crate::ui::actions::menu("复制规则")
                        .icon(IconName::Copy)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(value.clone()))
                        }),
                );
            }
            return menu;
        }
        if self.kind != TrafficKind::Connections {
            return menu;
        }
        let Some(record) = self
            .visible
            .get(row)
            .map(|index| self.records[*index].clone())
        else {
            return menu;
        };
        let id = record.id;
        let owner = cx.entity();
        menu = menu.item(
            crate::ui::actions::menu("查看连接详情")
                .icon(IconName::Info)
                .on_click(move |_, _, cx| {
                    owner.update(cx, |table, cx| {
                        table.delegate_mut().detail_id = Some(id);
                        cx.notify();
                    });
                }),
        );
        for (label, index) in [
            ("复制主机", 0),
            ("复制源地址", 9),
            ("复制目标地址", 10),
            ("复制进程", 7),
            ("复制链路", 5),
            ("复制规则", 6),
        ] {
            let value = record.cells[index].clone();
            menu = menu.item(crate::ui::actions::menu(label).on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(value.clone()))
            }));
        }
        let details = record.details.clone();
        menu = menu.item(
            crate::ui::actions::menu("复制全部详情").on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(details.clone()))
            }),
        );
        let target = record.connection.clone();
        menu.separator().item(
            crate::ui::actions::menu("关闭连接")
                .icon(IconName::X)
                .disabled(!record.active || !self.live)
                .on_click(move |_, window, cx| super::close_connection(target.clone(), window, cx)),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let (icon, title, desc) = match self.kind {
            TrafficKind::Connections => (
                IconName::Activity,
                if self.query.is_empty() {
                    "暂无连接"
                } else {
                    "无匹配连接"
                },
                "尝试调整搜索条件。",
            ),
            TrafficKind::Rules => (
                IconName::ListFilter,
                if self.query.is_empty() {
                    "暂无规则"
                } else {
                    "无匹配规则"
                },
                "尝试调整筛选条件。",
            ),
            TrafficKind::Logs => (
                IconName::ScrollText,
                if self.query.is_empty() {
                    "暂无日志"
                } else {
                    "无匹配日志"
                },
                "尝试调整搜索条件。",
            ),
        };
        ui::empty(icon, title, desc, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{RecordsTable, TrafficKind};

    fn fixture(kind: TrafficKind) -> RecordsTable {
        let mut table = RecordsTable::new(kind);
        table.records = crate::traffic::model::samples(kind);
        table.rebuild();
        table
    }
    #[test]
    fn rule_filter_and_sort_keep_original_match_order_and_copy_syntax() {
        let mut table = fixture(TrafficKind::Rules);
        table.filter = "DOMAIN-SUFFIX".into();
        table.sort = Some((0, true));
        table.rebuild();
        assert_eq!(table.visible, vec![6, 2, 0]);
        assert_eq!(
            super::rule_text(&table.records[table.visible[0]]),
            "DOMAIN-SUFFIX,telegram.org,节点选择"
        );
        let fallback = table
            .records
            .iter()
            .find(|row| row.cells[1] == "MATCH")
            .unwrap();
        assert_eq!(super::rule_text(fallback), "MATCH,节点选择");
        assert_eq!(table.data_column(0), usize::MAX);
        assert_eq!(table.data_column(3), 2);
    }

    #[test]
    fn hiding_and_reordering_columns_preserves_field_mapping() {
        let mut table = fixture(TrafficKind::Connections);
        table.column_order.swap(0, 7);
        table.hidden_columns.insert(1);
        table.hidden_columns.insert(2);
        assert_eq!(table.headers()[0].0, "进程");
        assert_eq!(table.data_column(0), 7);
        assert_eq!(table.headers()[1].0, "下载速度");
        let field = table.data_column(1);
        table.sort = Some((field, true));
        table.rebuild();
        assert_eq!(table.records[table.visible[0]].cells[0], "github.com");
        assert_eq!(
            table.records[table.visible[1]].cells[0],
            "rr1---sn.googlevideo.com"
        );
    }

    #[test]
    fn closing_a_connection_keeps_detail_identity_and_updates_both_tabs() {
        let mut table = fixture(TrafficKind::Connections);
        table.detail_id = Some(2);
        table.close_connection(2);
        assert_eq!(table.detail_id, Some(2));
        assert_eq!(table.count(0), 5);
        assert_eq!(table.count(1), 3);
        assert!(
            table
                .visible
                .iter()
                .all(|index| table.records[*index].id != 2)
        );
        table.tab = 1;
        table.rebuild();
        assert!(
            table
                .visible
                .iter()
                .any(|index| table.records[*index].id == 2)
        );
        assert!(table.records.iter().find(|row| row.id == 1).unwrap().active);
    }
}
