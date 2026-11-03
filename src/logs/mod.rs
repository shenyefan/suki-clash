mod model;
mod table;
mod view;

use crate::{session::Session, ui};
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
    _reception: Task<()>,
    cleared: [u64; 2],
}
impl LogsPage {
    pub(crate) fn new(
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let source = 0;
        let descending = true;
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索时间、级别或内容"));
        let levels: Vec<SharedString> = ["全部级别", "TRACE", "DEBUG", "INFO", "WARNING", "ERROR"]
            .into_iter()
            .map(Into::into)
            .collect();
        let selected = 0;
        let level =
            cx.new(|cx| SelectState::new(levels, Some(IndexPath::new(selected)), window, cx));
        cx.bind_keys([KeyBinding::new("escape", DismissLogDetails, Some("Logs"))]);
        let table = cx.new(|cx| {
            TableState::new(
                LogTable {
                    descending: true,
                    ..Default::default()
                },
                window,
                cx,
            )
        });
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
                if matches!(event, InputEvent::Blur)
                    && let Some(error) = &this.search.error
                {
                    ui::show_error(error.clone(), w, cx);
                }
            }),
            cx.subscribe_in(
                &level,
                window,
                |this, _, _: &SelectEvent<Vec<SharedString>>, w, cx| {
                    this.refresh(w, cx);
                },
            ),
            cx.observe_in(&session, window, |this, _, w, cx| {
                this.refresh(w, cx);
            }),
        ];
        let feed = crate::integration::logs(cx);
        let reception = cx.spawn_in(window, async move |owner, cx| {
            let mut cursor = 0;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(150))
                    .await;
                let records = feed.since(cursor);
                if records.is_empty() {
                    continue;
                }
                if owner
                    .update_in(cx, |this, window, cx| {
                        cursor = records.last().map_or(cursor, |record| record.id);
                        for record in &records {
                            if record.origin == "logging"
                                && record.level == crate::logging::Level::Error
                            {
                                ui::show_error(record.message.clone(), window, cx);
                            }
                        }
                        let entries = records
                            .into_iter()
                            .filter_map(|record| {
                                let source =
                                    usize::from(record.source == crate::logging::Source::Program);
                                (record.id > this.cleared[source]).then(|| LogEntry {
                                    id: record.id,
                                    time: record.time,
                                    level: record.level.label().into(),
                                    source,
                                    origin: record.origin,
                                    message: record.message,
                                })
                            })
                            .collect();
                        this.append(entries, window, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut page = Self {
            buffer: LogBuffer::default(),
            query,
            level,
            table,
            search: LogSearch::default(),
            source,
            descending,
            detail_cache: None,
            _subscriptions: subscriptions,
            _reception: reception,
            cleared: [0; 2],
        };
        page.refresh(window, cx);
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
                    && self.search.matches(&entry.text())
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
            ui::retain_table_selection(table, selected, cx);
            cx.notify();
        });
        let _ = window;
        cx.notify();
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
            .max_by_key(|(_, entry)| entry.time_key())
            .map(|(ix, _)| ix)
        else {
            return;
        };
        self.table
            .update(cx, |table, cx| table.scroll_to_row(index, cx));
        let _ = window;
    }
}

#[cfg(test)]
mod tests {
    use super::LogsPage;
    use crate::session::Session;
    use crate::{
        integration::{AppPaths, Repository, Service},
        logging::{Level, Record, Source},
    };
    use gpui_kit::base::ScrollbarHandle as _;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, px, size};
    use std::time::Duration;

    #[gpui_kit::test]
    fn live_tabs_descending_details_and_view_state_stay_separate_from_settings(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let service = Service::start(Repository::open(paths.clone()).unwrap()).unwrap();
        let feed = service.logs();
        let mut core = Record::program(Level::Info, "TCP", "core fixture");
        core.source = Source::Core;
        feed.publish(core);
        feed.publish(Record::program(
            Level::Warning,
            "storage",
            "program fixture",
        ));
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            cx.set_reduce_motion(true);
        });
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(1400.), px(800.)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let session = cx.new(|_| Session::default());
                    cx.new(|cx| LogsPage::new(session, window, cx))
                },
            )
            .unwrap()
        });
        cx.executor().advance_clock(Duration::from_millis(160));
        cx.run_until_parked();
        cx.update_window(handle, |_, w, cx| {
            w.render_frame(cx);
            page.update(cx, |page, cx| {
                assert!(page.descending);
                assert_eq!(page.table.read(cx).delegate().entries.len(), 1);
                assert_eq!(page.table.read(cx).delegate().entries[0].origin, "TCP");
                page.source = 1;
                page.refresh(w, cx);
                assert_eq!(page.table.read(cx).delegate().entries[0].origin, "storage");
            });
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, w, cx| {
            w.render_frame(cx);
            w.click(("log-row", 2u64), cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, w, cx| {
            w.render_frame(cx);
            let input = w.find(("input", page.read(cx).query.entity_id())).bounds();
            let options = w.find("logs-search-options").bounds();
            assert!(options.left() >= input.left() && options.right() <= input.right());
            assert!(options.top() >= input.top() && options.bottom() <= input.bottom());
            let width = page.read(cx).table.read(cx).delegate().viewport_width;
            assert!(width > px(1000.));
            let table = page.read(cx).table.read(cx);
            let columns = (0..4)
                .map(|ix| table.col_width(ix).unwrap())
                .fold(px(0.), |sum, width| sum + width);
            assert!((columns - width).abs() < px(1.));
            assert!((w.find("detail-drawer").bounds().size.width - width).abs() < px(1.));
            page.update(cx, |page, cx| {
                page.descending = false;
                page.search.case_sensitive = true;
                page.refresh(w, cx);
            });
        })
        .unwrap();
        // Live identity restoration must not undo a user's scroll away from selection.
        cx.update_window(handle, |_, w, cx| {
            page.update(cx, |page, cx| {
                page.descending = true;
                let records = (10..310)
                    .map(|id| super::LogEntry {
                        id,
                        time: chrono::DateTime::from_timestamp(id as i64, 0)
                            .unwrap()
                            .to_rfc3339(),
                        level: "INFO".into(),
                        source: 1,
                        origin: "test".into(),
                        message: format!("entry {id}"),
                    })
                    .collect();
                page.append(records, w, cx);
                page.table
                    .update(cx, |table, cx| table.set_selected_row(0, cx));
            });
            w.render_frame(cx);
            let table = page.read(cx).table.clone();
            table.update(cx, |table, cx| table.scroll_to_row(100, cx));
            w.render_frame(cx);
            let table = page.read(cx).table.clone();
            let before = table.read(cx).vertical_scroll_handle.offset();
            assert!(before.y < px(-100.));
            let selected = table.read(cx).delegate().entries[0].id;
            page.update(cx, |page, cx| {
                page.append(
                    vec![super::LogEntry {
                        id: 400,
                        time: chrono::DateTime::from_timestamp(400, 0)
                            .unwrap()
                            .to_rfc3339(),
                        level: "INFO".into(),
                        source: 1,
                        origin: "test".into(),
                        message: "new entry".into(),
                    }],
                    w,
                    cx,
                )
            });
            w.render_frame(cx);
            let state = table.read(cx);
            assert_eq!(
                state.delegate().entries[state.selected_row().unwrap()].id,
                selected
            );
            assert_eq!(state.vertical_scroll_handle.offset(), before);
        })
        .unwrap();
        assert!(!paths.settings().join("logs.json").exists());
        crate::integration::finish_test_services(cx);
    }
}
