mod model;
mod table;
mod view;

use crate::{logging::Level, ui};
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

actions!(logs, [DismissLogDetails]);

pub(crate) struct LogsPage {
    buffer: LogBuffer,
    query: Entity<InputState>,
    level: Entity<SelectState<Vec<SharedString>>>,
    table: Entity<TableState<LogTable>>,
    search: LogSearch,
    source: usize,
    minimum: [Option<Level>; 2],
    descending: bool,
    detail_cache: Option<LogEntry>,
    _subscriptions: Vec<Subscription>,
    _reception: Task<()>,
    cleared: [u64; 2],
    visible: bool,
}
impl LogsPage {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let source = 0;
        let descending = true;
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索时间、级别或内容"));
        let levels: Vec<SharedString> = ["TRACE", "DEBUG", "INFO", "WARNING", "ERROR"]
            .into_iter()
            .map(Into::into)
            .collect();
        let selected = 2;
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
                |this, _, event: &SelectEvent<Vec<SharedString>>, w, cx| {
                    if let SelectEvent::Confirm(Some(value)) = event {
                        this.minimum[this.source] = Some(Level::parse(value));
                        this.refresh(w, cx);
                    }
                },
            ),
            cx.observe_window_visibility(window, |this, visibility, window, cx| {
                if visibility.is_visible() {
                    this.refresh(window, cx);
                }
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
            minimum: [None; 2],
            descending,
            detail_cache: None,
            _subscriptions: subscriptions,
            _reception: reception,
            cleared: [0; 2],
            visible: true,
        };
        page.refresh(window, cx);
        page
    }
    pub(crate) fn set_visible(
        &mut self,
        visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible {
            self.refresh(window, cx);
        }
    }
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible || !window.is_visible() {
            return;
        }
        self.search.compile(self.query.read(cx).value().as_ref());
        let minimum = self.minimum[self.source].unwrap_or_else(|| {
            let (page, key) = if self.source == 0 {
                ("core", "LogLevelIndex")
            } else {
                ("settings", "AppLogLevel")
            };
            let settings = crate::preferences::choices::encode(crate::integration::read(page, cx));
            settings
                .get(key)
                .map_or(Level::Info, |value| Level::parse(value))
                .min(Level::Error)
        });
        let selected: SharedString = minimum.label().into();
        if self.level.read(cx).selected_value() != Some(&selected) {
            self.level.update(cx, |level, cx| {
                level.set_selected_value(&selected, window, cx);
                cx.notify();
            });
        }
        let entries: Vec<_> = self
            .buffer
            .entries
            .iter()
            .filter(|entry| {
                entry.source == self.source
                    && Level::parse(&entry.level) >= minimum
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
        cx.notify();
    }
    // The runtime adapter delivers batches here.
    pub(crate) fn append(
        &mut self,
        entries: Vec<LogEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.buffer.append(entries);
        if !self.visible || !window.is_visible() {
            return;
        }
        let near_bottom = self
            .table
            .read(cx)
            .vertical_scroll_handle
            .is_scrolled_to_end()
            .unwrap_or(true);
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
    use crate::{
        integration::{AppPaths, Repository, Service},
        logging::{Level, Record, Source},
    };
    use gpui_kit::base::ScrollbarHandle as _;
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AppContext, Bounds, TestAppContext, WindowBounds, WindowOptions, WindowVisibility, px, size,
    };
    use std::time::Duration;

    #[gpui_kit::test]
    fn level_defaults_follow_each_source_and_manual_filters_include_higher_severities(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let repository = Repository::open(paths.clone()).unwrap();
        let mut core = crate::preferences::defaults(crate::preferences::SettingsKind::Core);
        core.insert("LogLevelIndex".into(), "debug".into());
        crate::configuration::Preferences::from_values(&core, None)
            .unwrap()
            .save(&paths)
            .unwrap();
        let mut general = crate::preferences::defaults(crate::preferences::SettingsKind::General);
        general.insert("AppLogLevel".into(), "warn".into());
        repository
            .save(
                &crate::storage::Namespace::parse("settings").unwrap(),
                &general,
            )
            .unwrap();
        let service = Service::start(repository).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
        });
        let settings_path = crate::preferences::app::path(&paths);
        let saved = std::fs::read(&settings_path).unwrap();
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| LogsPage::new(window, cx))
            })
            .unwrap()
        });
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| {
                    let entries = [0, 1].into_iter().flat_map(|source| {
                        [
                            Level::Trace,
                            Level::Debug,
                            Level::Info,
                            Level::Warning,
                            Level::Error,
                        ]
                        .into_iter()
                        .map(move |level| super::LogEntry {
                            id: 0,
                            time: "2026-11-13T12:00:00+08:00".into(),
                            level: level.label().into(),
                            source,
                            origin: "test".into(),
                            message: level.label().into(),
                        })
                    });
                    page.append(entries.collect(), window, cx);
                    assert_eq!(
                        page.level.read(cx).selected_value().unwrap().as_ref(),
                        "DEBUG"
                    );
                    assert_eq!(page.table.read(cx).delegate().entries.len(), 4);
                    page.source = 1;
                    page.refresh(window, cx);
                    assert_eq!(
                        page.level.read(cx).selected_value().unwrap().as_ref(),
                        "WARNING"
                    );
                    assert_eq!(page.table.read(cx).delegate().entries.len(), 2);
                });
            })
            .unwrap();
        for (level, expected) in [
            (Level::Trace, 5),
            (Level::Debug, 4),
            (Level::Info, 3),
            (Level::Warning, 2),
            (Level::Error, 1),
        ] {
            page.read_with(cx, |page, _| page.level.clone())
                .update(cx, |_, cx| {
                    cx.emit(super::SelectEvent::Confirm(Some(level.label().into())));
                });
            cx.run_until_parked();
            page.read_with(cx, |page, cx| {
                let entries = &page.table.read(cx).delegate().entries;
                assert_eq!(entries.len(), expected, "{level:?}");
                assert!(
                    entries
                        .iter()
                        .all(|entry| Level::parse(&entry.level) >= level)
                );
                assert_eq!(page.minimum, [None, Some(level)]);
            });
        }
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| {
                    page.source = 0;
                    page.refresh(window, cx);
                    assert_eq!(
                        page.level.read(cx).selected_value().unwrap().as_ref(),
                        "DEBUG"
                    );
                    assert_eq!(page.table.read(cx).delegate().entries.len(), 4);
                });
            })
            .unwrap();
        assert_eq!(std::fs::read(settings_path).unwrap(), saved);
        crate::integration::finish_test_services(cx);
    }

    #[gpui_kit::test]
    fn hidden_page_and_window_keep_collecting_without_refreshing_the_table(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        let service = Service::start(Repository::open(paths).unwrap()).unwrap();
        let feed = service.logs();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            cx.set_reduce_motion(true);
        });
        let (handle, page) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| LogsPage::new(window, cx))
            })
            .unwrap()
        });
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| page.set_visible(false, window, cx));
            })
            .unwrap();
        let mut record = Record::program(Level::Info, "TCP", "hidden page");
        record.source = Source::Core;
        feed.publish(record);
        cx.executor().advance_clock(Duration::from_millis(160));
        cx.run_until_parked();
        page.update(cx, |page, cx| {
            assert_eq!(page.buffer.entries.len(), 1);
            assert!(page.table.read(cx).delegate().entries.is_empty());
        });
        handle
            .update(cx, |_, window, cx| {
                page.update(cx, |page, cx| page.set_visible(true, window, cx));
            })
            .unwrap();
        page.update(cx, |page, cx| {
            assert_eq!(page.table.read(cx).delegate().entries.len(), 1)
        });
        cx.simulate_window_visibility_change(handle, WindowVisibility::Hidden);
        let mut record = Record::program(Level::Info, "TCP", "hidden window");
        record.source = Source::Core;
        feed.publish(record);
        cx.executor().advance_clock(Duration::from_millis(160));
        cx.run_until_parked();
        page.update(cx, |page, cx| {
            assert_eq!(page.buffer.entries.len(), 2);
            assert_eq!(page.table.read(cx).delegate().entries.len(), 1);
        });
        cx.simulate_window_visibility_change(handle, WindowVisibility::Visible);
        page.update(cx, |page, cx| {
            assert_eq!(page.table.read(cx).delegate().entries.len(), 2)
        });
        crate::integration::finish_test_services(cx);
    }

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
                |window, cx| cx.new(|cx| LogsPage::new(window, cx)),
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
