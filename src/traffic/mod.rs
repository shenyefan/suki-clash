mod connections;
mod details;
pub(crate) mod rules;
pub(crate) use connections::{Action as ConnectionAction, Target as ConnectionTarget};
mod model;
mod table;
mod view;

use crate::session::Session;
use gpui_kit::{
    component::{
        IndexPath,
        input::{InputEvent, InputState},
        select::{SelectEvent, SelectState},
        table::{TableEvent, TableState},
    },
    *,
};
use table::RecordsTable;

gpui_kit::actions!(connections, [DismissConnectionDetails]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrafficKind {
    Connections,
    Rules,
    Logs,
}
pub(crate) struct TrafficPage {
    kind: TrafficKind,
    session: Entity<Session>,
    connection_records: connections::Tracker,
    rule_snapshot: std::sync::Arc<rules::Snapshot>,
    table: Entity<TableState<RecordsTable>>,
    query: Entity<InputState>,
    filter: Entity<SelectState<Vec<SharedString>>>,
    sort_index: usize,
    paused: bool,
    reverse: bool,
    detail_cache: Option<model::Record>,
    _subscriptions: Vec<Subscription>,
}
impl TrafficPage {
    pub(crate) fn new(
        kind: TrafficKind,
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.bind_keys([KeyBinding::new(
            "escape",
            DismissConnectionDetails,
            Some("Connections"),
        )]);
        let table = cx.new(|cx| TableState::new(RecordsTable::new(kind), window, cx));
        let query = cx.new(|cx| {
            InputState::new(window, cx).placeholder(match kind {
                TrafficKind::Connections => "搜索主机、进程或代理链",
                TrafficKind::Rules => "筛选规则、策略或类型",
                TrafficKind::Logs => "搜索日志",
            })
        });
        let filters: Vec<SharedString> = if kind == TrafficKind::Logs {
            ["全部", "DEBUG", "INFO", "WARN", "ERROR"]
                .iter()
                .map(|s| (*s).into())
                .collect()
        } else {
            std::iter::once("全部类型")
                .chain(crate::ui::RULE_TYPES.iter().copied())
                .map(Into::into)
                .collect()
        };
        let filter = cx.new(|cx| SelectState::new(filters, Some(IndexPath::new(0)), window, cx));
        let observed_table = table.clone();
        let subscriptions = vec![
            cx.observe(&session, move |this, session, cx| {
                let runtime = session.read(cx).runtime();
                if this.kind == TrafficKind::Connections {
                    let live = runtime.connections_live && session.read(cx).is_running();
                    if this.table.read(cx).delegate().live != live {
                        this.table.update(cx, |table, cx| {
                            table.delegate_mut().live = live;
                            cx.notify();
                        });
                    }
                    let changed = if session.read(cx).core().phase() == crate::core::Phase::Stopped
                    {
                        this.connection_records.stopped()
                    } else {
                        this.connection_records.update(&runtime)
                    };
                    if changed && !this.paused {
                        this.sync_connection_rows(cx);
                    }
                }
                if this.kind == TrafficKind::Rules {
                    let rules = session.read(cx).core().rules().clone();
                    if !std::sync::Arc::ptr_eq(&rules, &this.rule_snapshot) {
                        let same_scope = (rules.generation, rules.epoch)
                            == (this.rule_snapshot.generation, this.rule_snapshot.epoch);
                        this.rule_snapshot = rules.clone();
                        this.table.update(cx, |table, cx| {
                            let selected = table
                                .selected_row()
                                .and_then(|row| table.delegate().visible.get(row))
                                .map(|index| table.delegate().records[*index].id);
                            table.delegate_mut().records = rules.rows();
                            table.delegate_mut().rebuild();
                            if !same_scope {
                                table.delegate_mut().detail_id = None;
                                table.clear_selection(cx);
                            } else if let Some(id) = selected {
                                let row =
                                    table.delegate().visible.iter().position(|index| {
                                        table.delegate().records[*index].id == id
                                    });
                                crate::ui::retain_table_selection(table, row, cx);
                            }
                            cx.notify();
                        });
                    }
                }
                cx.notify();
            }),
            cx.subscribe_in(
                &query,
                window,
                |this, state, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let query = state.read(cx).value().to_string();
                        this.table.update(cx, |table, cx| {
                            table.delegate_mut().query = query;
                            table.delegate_mut().rebuild();
                            table.clear_selection(cx);
                            cx.notify();
                        });
                        cx.notify();
                    }
                    if matches!(event, InputEvent::Blur)
                        && let Some(error) = this.table.read(cx).delegate().search.error.clone()
                    {
                        crate::ui::show_error(error, window, cx);
                    }
                },
            ),
            cx.subscribe(
                &filter,
                |this, _, event: &SelectEvent<Vec<SharedString>>, cx| {
                    let SelectEvent::Confirm(value) = event;
                    let value = value.clone().unwrap_or_default().to_string();
                    this.table.update(cx, |table, cx| {
                        table.delegate_mut().filter = value;
                        table.delegate_mut().rebuild();
                        table.clear_selection(cx);
                        cx.notify();
                    });
                    cx.notify();
                },
            ),
            cx.subscribe_in(
                &observed_table,
                window,
                |this, state, event: &TableEvent, window, cx| {
                    if this.kind != TrafficKind::Logs
                        && let TableEvent::SelectRow(index) = event
                    {
                        state.update(cx, |table, cx| {
                            table.delegate_mut().detail_id = table
                                .delegate()
                                .visible
                                .get(*index)
                                .map(|index| table.delegate().records[*index].id);
                            cx.notify();
                        });
                        state.focus_handle(cx).focus(window, cx);
                        cx.notify();
                    }
                },
            ),
            cx.observe(&observed_table, |this, table, cx| {
                let delegate = table.read(cx).delegate();
                if let Some(record) = delegate
                    .detail_id
                    .and_then(|id| delegate.records.iter().find(|record| record.id == id))
                {
                    this.detail_cache = Some(record.clone());
                }
                cx.notify();
            }),
        ];
        let mut page = Self {
            kind,
            session,
            connection_records: Default::default(),
            rule_snapshot: Default::default(),
            table,
            query,
            filter,
            sort_index: 0,
            paused: false,
            reverse: false,
            detail_cache: None,
            _subscriptions: subscriptions,
        };
        if page.kind == TrafficKind::Connections
            && page
                .connection_records
                .update(&page.session.read(cx).runtime())
        {
            page.sync_connection_rows(cx);
        }
        if page.kind == TrafficKind::Rules {
            let rules = page.session.read(cx).core().rules().clone();
            page.rule_snapshot = rules.clone();
            page.table.update(cx, |table, cx| {
                table.delegate_mut().records = rules.rows();
                table.delegate_mut().rebuild();
                cx.notify();
            });
        }
        page
    }

    fn sync_connection_rows(&mut self, cx: &mut Context<Self>) {
        let records = self.connection_records.rows();
        let live =
            self.session.read(cx).runtime().connections_live && self.session.read(cx).is_running();
        self.table.update(cx, |table, cx| {
            let selected = table
                .selected_row()
                .and_then(|row| table.delegate().visible.get(row))
                .map(|index| table.delegate().records[*index].id);
            table.delegate_mut().live = live;
            table.delegate_mut().records = records;
            table.delegate_mut().rebuild();
            if let Some(id) = selected {
                let new_row = table
                    .delegate()
                    .visible
                    .iter()
                    .position(|index| table.delegate().records[*index].id == id);
                crate::ui::retain_table_selection(table, new_row, cx);
            }
            cx.notify();
        });
    }
    fn apply_sort(&mut self, index: usize, cx: &mut Context<Self>) {
        self.sort_index = index;
        let reverse = self.reverse;
        self.table.update(cx, |table, cx| {
            table.delegate_mut().sort = if table.delegate().kind == TrafficKind::Rules {
                Some((
                    match index {
                        0 => usize::MAX,
                        1 => 0,
                        2 => 1,
                        _ => 2,
                    },
                    reverse,
                ))
            } else {
                match index {
                    0 => Some((usize::MAX, reverse)),
                    1 => Some((0, reverse)),
                    2 => Some((7, reverse)),
                    3 => Some((5, reverse)),
                    4 => Some((4, reverse)),
                    5 => Some((3, reverse)),
                    _ => None,
                }
            };
            table.delegate_mut().rebuild();
            table.clear_selection(cx);
            cx.notify();
        });
        cx.notify();
    }
    fn switch_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        self.table.update(cx, |table, cx| {
            table.delegate_mut().detail_id = None;
            table.delegate_mut().tab = index;
            table.delegate_mut().rebuild();
            table.clear_selection(cx);
            cx.notify();
        });
        cx.notify();
    }
    fn clear_or_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = self.kind;
        if kind == TrafficKind::Connections {
            let runtime = self.session.read(cx).runtime();
            crate::core::request(
                crate::core::Command::Connections(ConnectionAction::CloseAll {
                    generation: runtime.generation,
                    epoch: runtime.epoch,
                }),
                window,
                cx,
            );
            return;
        }
        let removed = self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            let tab = delegate.tab;
            let removed = delegate
                .records
                .iter()
                .filter(|record| record.source == tab)
                .cloned()
                .collect::<Vec<_>>();
            delegate.records.retain(|record| record.source != tab);
            delegate.rebuild();
            table.clear_selection(cx);
            cx.notify();
            removed
        });
        let table = self.table.downgrade();
        crate::ui::notifications::undo(window, "已清空日志", cx, move |_, cx| {
            table
                .update(cx, |table, cx| {
                    let delegate = table.delegate_mut();
                    for record in &removed {
                        if !delegate.records.iter().any(|entry| entry.id == record.id) {
                            delegate.records.push(record.clone());
                        }
                    }
                    delegate.records.sort_by_key(|record| record.id);
                    delegate.rebuild();
                    cx.notify();
                    true
                })
                .unwrap_or(true)
        });
        cx.notify();
    }
}

pub(super) fn close_connection(
    target: Option<ConnectionTarget>,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(target) = target {
        crate::core::request(
            crate::core::Command::Connections(ConnectionAction::Close(target)),
            window,
            cx,
        );
    } else {
        crate::ui::show_error("连接身份不可用".into(), window, cx);
    }
}
