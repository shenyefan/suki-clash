mod details;
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
                TrafficKind::Connections => "搜索主机、进程或代理链…",
                TrafficKind::Rules => "筛选规则、策略或类型…",
                TrafficKind::Logs => "搜索日志…",
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
            cx.observe(&session, move |this, _, cx| {
                this.table.update(cx, |table, cx| {
                    table.delegate_mut().rebuild();
                    table.clear_selection(cx);
                    cx.notify();
                });
                cx.notify();
            }),
            cx.subscribe(&query, |this, state, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let query = state.read(cx).value().to_lowercase();
                    this.table.update(cx, |table, cx| {
                        table.delegate_mut().query = query;
                        table.delegate_mut().rebuild();
                        table.clear_selection(cx);
                        cx.notify();
                    });
                    cx.notify();
                }
            }),
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
        Self {
            kind,
            table,
            query,
            filter,
            sort_index: 0,
            paused: false,
            reverse: false,
            detail_cache: None,
            _subscriptions: subscriptions,
        }
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
            crate::ui::request_command("关闭全部连接", window, cx);
            return;
        }
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if kind == TrafficKind::Logs {
                let tab = delegate.tab;
                delegate.records.retain(|r| r.source != tab);
            }
            delegate.rebuild();
            table.clear_selection(cx);
            cx.notify();
        });
        cx.notify();
    }
}
