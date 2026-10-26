mod model;
mod table;
mod view;

use crate::{integration as storage, session::Session, ui};
use gpui_kit::{
    component::{
        IndexPath,
        input::{InputEvent, InputState},
        select::{SelectEvent, SelectState},
        table::{TableEvent, TableState},
    },
    *,
};
use model::{LogBuffer, LogEntry, LogSearch};
use std::collections::BTreeMap;
use table::LogTable;

gpui_kit::actions!(logs, [DismissLogDetails]);

pub(crate) struct LogsPage {
    buffer: LogBuffer,
    query: Entity<InputState>,
    level: Entity<SelectState<Vec<SharedString>>>,
    table: Entity<TableState<LogTable>>,
    search: LogSearch,
    source: usize,
    descending: bool,
    detail_cache: Option<LogEntry>,
    _subscriptions: Vec<Subscription>,
}
impl LogsPage {
    pub(crate) fn new(
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = storage::read("logs", cx);
        let source = usize::from(saved.get("source").is_some_and(|value| value == "1"));
        let descending = saved.get("descending").is_some_and(|value| value == "true");
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索时间、级别或内容…"));
        let levels: Vec<SharedString> = ["全部级别", "DEBUG", "INFO", "WARNING", "ERROR"]
            .into_iter()
            .map(Into::into)
            .collect();
        let selected = levels
            .iter()
            .position(|value| Some(value.as_ref()) == saved.get("level").map(String::as_str))
            .unwrap_or(0);
        let level =
            cx.new(|cx| SelectState::new(levels, Some(IndexPath::new(selected)), window, cx));
        cx.bind_keys([KeyBinding::new("escape", DismissLogDetails, Some("Logs"))]);
        let table = cx.new(|cx| TableState::new(LogTable::default(), window, cx));
        let observed_table = table.clone();
        let subscriptions = vec![
            cx.subscribe(
                &observed_table,
                |_, state, event: &TableEvent, cx| match event {
                    TableEvent::SelectRow(ix) => {
                        state.update(cx, |table, cx| {
                            table.delegate_mut().detail_id =
                                table.delegate().entries.get(*ix).map(|entry| entry.id);
                            cx.notify();
                        });
                        cx.notify();
                    }
                    TableEvent::ClearSelection => {
                        state.update(cx, |table, cx| {
                            table.delegate_mut().detail_id = None;
                            cx.notify();
                        });
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.observe(&observed_table, |this, table, cx| {
                let delegate = table.read(cx).delegate();
                if let Some(record) = delegate
                    .detail_id
                    .and_then(|id| delegate.entries.iter().find(|record| record.id == id))
                {
                    this.detail_cache = Some(record.clone());
                }
                cx.notify();
            }),
            cx.subscribe_in(&query, window, |this, _, event: &InputEvent, w, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh(w, cx);
                }
            }),
            cx.subscribe_in(
                &level,
                window,
                |this, _, _: &SelectEvent<Vec<SharedString>>, w, cx| {
                    this.refresh(w, cx);
                    this.persist(w, cx);
                },
            ),
            cx.subscribe_in(
                &crate::integration::notices(cx),
                window,
                |this, _, notice: &crate::integration::Notice, window, cx| {
                    this.append(
                        vec![LogEntry {
                            id: 0,
                            time: notice.time().to_owned(),
                            level: notice.level().to_owned(),
                            source: 1,
                            message: notice.message().to_owned(),
                        }],
                        window,
                        cx,
                    );
                },
            ),
            cx.observe_in(&session, window, |this, _, w, cx| {
                this.refresh(w, cx);
            }),
        ];
        let buffer = LogBuffer::default();
        let mut page = Self {
            buffer,
            query,
            level,
            table,
            search: LogSearch {
                case_sensitive: saved.get("case").is_some_and(|value| value == "true"),
                whole_word: saved.get("word").is_some_and(|value| value == "true"),
                regular_expression: saved.get("regex").is_some_and(|value| value == "true"),
                ..Default::default()
            },
            source,
            descending,
            detail_cache: None,
            _subscriptions: subscriptions,
        };
        page.append(
            vec![LogEntry {
                id: 0,
                time: chrono::Local::now().format("%H:%M:%S").to_string(),
                level: "INFO".into(),
                source: 1,
                message: "应用配置存储已就绪".into(),
            }],
            window,
            cx,
        );
        page
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.compile(self.query.read(cx).value().as_ref());
        let level = self
            .level
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default();
        let entries: Vec<_> = self
            .buffer
            .entries
            .iter()
            .filter(|entry| {
                entry.source == self.source
                    && (level == "全部级别" || entry.level == level.as_ref())
                    && self.search.matches(entry)
            })
            .cloned()
            .collect();
        let matcher = self.search.matcher.clone();
        let descending = self.descending;
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            delegate.entries = entries;
            delegate.matcher = matcher;
            delegate.descending = descending;
            delegate.sort_entries();
            let selected = delegate.selected_index();
            if selected.is_none() {
                delegate.detail_id = None;
            }
            if let Some(ix) = selected {
                table.set_selected_row(ix, cx);
            } else {
                table.clear_selection(cx);
            }
            cx.notify();
        });
        let _ = window;
        cx.notify();
    }
    fn persist(&self, window: &mut Window, cx: &mut Context<Self>) {
        let values = BTreeMap::from([
            ("source".into(), self.source.to_string()),
            ("descending".into(), self.descending.to_string()),
            (
                "level".into(),
                self.level
                    .read(cx)
                    .selected_value()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            ),
            ("case".into(), self.search.case_sensitive.to_string()),
            ("word".into(), self.search.whole_word.to_string()),
            ("regex".into(), self.search.regular_expression.to_string()),
        ]);
        storage::save(
            "logs",
            values,
            Vec::new(),
            window,
            cx,
            |result, window, cx| {
                if let Err(error) = result {
                    ui::show_error(format!("无法保存日志偏好：{error}"), window, cx);
                }
            },
        );
    }

    // The runtime adapter can deliver batches here; paused reception never fabricates new rows.
    pub(crate) fn append(
        &mut self,
        entries: Vec<LogEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let near_bottom = self
            .table
            .read(cx)
            .vertical_scroll_handle
            .is_scrolled_to_end()
            .unwrap_or(true);
        self.buffer.append(entries);
        self.refresh(window, cx);
        if !self.descending && near_bottom {
            self.jump_to_latest(window, cx);
        }
    }
    fn dismiss_details(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().detail_id = None;
            table.clear_selection(cx);
            cx.notify();
        });
        self.table.focus_handle(cx).focus(window, cx);
    }
    fn jump_to_latest(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self
            .table
            .read(cx)
            .delegate()
            .entries
            .iter()
            .enumerate()
            .max_by_key(|(_, entry)| entry.id)
            .map(|(ix, _)| ix)
        else {
            return;
        };
        self.table
            .update(cx, |table, cx| table.scroll_to_row(index, cx));
        let _ = window;
    }
}
