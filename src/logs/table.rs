use super::model::LogEntry;
use crate::{
    ui,
    ui::theme::{column, opacity, text},
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme,
        menu::{PopupMenu, PopupMenuItem},
        table::{Column, ColumnSort, TableDelegate, TableState},
    },
    *,
};
use regex::Regex;

#[derive(Default)]
pub(super) struct LogTable {
    pub(super) entries: Vec<LogEntry>,
    pub(super) matcher: Option<Regex>,
    pub(super) detail_id: Option<u64>,
    pub(super) sort: Option<(usize, bool)>,
    pub(super) descending: bool,
}
impl LogTable {
    pub(super) fn sort_entries(&mut self) {
        let (column, descending) = self.sort.unwrap_or((0, self.descending));
        self.entries.sort_by(|a, b| {
            let order = match column {
                1 => a.level.cmp(&b.level).then(a.id.cmp(&b.id)),
                2 => a.source.cmp(&b.source).then(a.id.cmp(&b.id)),
                3 => a.message.cmp(&b.message).then(a.id.cmp(&b.id)),
                _ => a.id.cmp(&b.id),
            };
            if descending { order.reverse() } else { order }
        });
    }
    pub(super) fn selected_index(&self) -> Option<usize> {
        self.detail_id
            .and_then(|id| self.entries.iter().position(|entry| entry.id == id))
    }
}
fn highlighted(value: String, matcher: Option<&Regex>, cx: &App) -> StyledText {
    let ranges: Vec<_> = matcher
        .into_iter()
        .flat_map(|matcher| matcher.find_iter(&value))
        .filter(|found| !found.is_empty())
        .map(|found| {
            (
                found.range(),
                HighlightStyle {
                    background_color: Some(cx.theme().primary.opacity(opacity::SELECTED)),
                    font_weight: Some(text::HEADING),
                    ..Default::default()
                },
            )
        })
        .collect();
    StyledText::new(value).with_highlights(ranges)
}
impl TableDelegate for LogTable {
    fn columns_count(&self, _: &App) -> usize {
        4
    }
    fn rows_count(&self, _: &App) -> usize {
        self.entries.len()
    }
    fn column(&self, ix: usize, cx: &App) -> Column {
        let (key, label, width) = [
            ("time", "时间", column::TIME),
            ("level", "级别", column::LOG_LEVEL),
            ("source", "来源", column::PROCESS),
            ("message", "内容", column::MESSAGE),
        ][ix];
        Column::new(key, label)
            .width(cx.theme().font_size * width)
            .sortable()
    }
    fn render_tr(
        &mut self,
        ix: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div()
            .id(("log-row", self.entries.get(ix).map_or(0, |entry| entry.id)))
            .aria_label(self.entries.get(ix).map(LogEntry::text).unwrap_or_default())
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let entry = &self.entries[row];
        let value = match column {
            0 => entry.time.clone(),
            1 => entry.level.clone(),
            2 => if entry.source == 0 {
                "内核"
            } else {
                "程序"
            }
            .into(),
            _ => entry.message.clone(),
        };
        let color = match (column, entry.level.as_str()) {
            (1, "ERROR") => cx.theme().danger,
            (1, "WARNING" | "WARN") => cx.theme().warning,
            (3, _) => cx.theme().foreground,
            _ => cx.theme().muted_foreground,
        };
        div()
            .text_size(text::BODY)
            .text_color(color)
            .truncate()
            .child(highlighted(value, self.matcher.as_ref(), cx))
    }
    fn perform_sort(
        &mut self,
        column: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.sort = match sort {
            ColumnSort::Default => None,
            ColumnSort::Ascending => Some((column, false)),
            ColumnSort::Descending => Some((column, true)),
        };
        self.sort_entries();
        let selected = self.selected_index();
        let table = cx.entity();
        cx.defer(move |cx| {
            table.update(cx, |table, cx| {
                if let Some(ix) = selected {
                    table.set_selected_row(ix, cx);
                } else {
                    table.clear_selection(cx);
                }
                cx.notify();
            })
        });
    }
    fn context_menu(
        &mut self,
        ix: usize,
        menu: PopupMenu,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let Some(entry) = self.entries.get(ix) else {
            return menu;
        };
        let message = entry.message.clone();
        let text = entry.text();
        menu.item(PopupMenuItem::new("复制内容").on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(message.clone()))
        }))
        .item(
            PopupMenuItem::new("复制完整日志").on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
            }),
        )
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        ui::empty(IconName::ScrollText, "暂无日志", "调整级别或搜索条件", cx)
    }
}

#[cfg(test)]
mod tests {
    use super::{LogEntry, LogTable};
    #[test]
    fn sorting_keeps_detail_attached_to_the_same_entry() {
        let mut table = LogTable {
            entries: [1, 2, 3]
                .into_iter()
                .map(|id| LogEntry {
                    id,
                    time: "18:42:00".into(),
                    level: "INFO".into(),
                    source: 0,
                    message: format!("entry {id}"),
                })
                .collect(),
            detail_id: Some(2),
            descending: true,
            ..Default::default()
        };
        table.sort_entries();
        assert_eq!(table.entries[0].id, 3);
        assert_eq!(table.entries[table.selected_index().unwrap()].id, 2);
        table.sort = Some((3, false));
        table.sort_entries();
        assert_eq!(table.entries[0].id, 1);
        assert_eq!(table.entries[table.selected_index().unwrap()].id, 2);
        table.entries.retain(|entry| entry.id != 2);
        assert_eq!(table.selected_index(), None);
    }
}
