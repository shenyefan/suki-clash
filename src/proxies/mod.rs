mod chain;
pub(crate) mod chain_config;
mod model;
mod node_list;
pub(crate) mod runtime;
mod view;

use crate::session::Session;
use gpui_kit::{
    component::input::{InputEvent, InputState},
    *,
};
use model::{ProxyGroup, SortOrder};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct ProxiesPage {
    session: Entity<Session>,
    query: Entity<InputState>,
    groups: Vec<ProxyGroup>,
    observed: std::sync::Arc<crate::core::Runtime>,
    expanded: BTreeSet<String>,
    search_expanded: BTreeSet<String>,
    sort: SortOrder,
    group_sorts: BTreeMap<String, SortOrder>,
    delay_timeout: u32,
    reverse: bool,
    only_available: bool,
    compact: bool,
    columns: usize,
    list_scroll: gpui_kit::component::VirtualListScrollHandle,
    card_focus: BTreeMap<(String, String), FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl ProxiesPage {
    pub(crate) fn new(
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索代理组或节点"));
        let delay_timeout = crate::integration::read("settings", cx)
            .get("DelayTestTimeout")
            .and_then(|value| value.parse().ok())
            .unwrap_or(5000);
        let runtime = session.read(cx).runtime();
        let groups = if runtime.connected && session.read(cx).is_running() {
            ProxyGroup::observed(&runtime.proxies, runtime.mode, delay_timeout)
        } else {
            Vec::new()
        };
        let expanded = groups.iter().map(ProxyGroup::id).collect();
        let card_focus = groups
            .iter()
            .flat_map(|group| {
                group
                    .visible_nodes("", false, SortOrder::Default, false)
                    .into_iter()
                    .map(|node| ((group.id(), node.id()), cx.focus_handle()))
                    .collect::<Vec<_>>()
            })
            .collect();
        let subscriptions = vec![
            cx.observe(&session, |this, session, cx| {
                let delay_timeout = crate::integration::read("settings", cx)
                    .get("DelayTestTimeout")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(5000);
                let runtime = session.read(cx).runtime();
                if runtime.connected && session.read(cx).is_running() {
                    if this.observed.proxies != runtime.proxies
                        || this.observed.mode != runtime.mode
                        || this.delay_timeout != delay_timeout
                        || this.groups.is_empty()
                    {
                        let groups =
                            ProxyGroup::observed(&runtime.proxies, runtime.mode, delay_timeout);
                        let ids = groups.iter().map(ProxyGroup::id).collect::<BTreeSet<_>>();
                        this.expanded.retain(|id| ids.contains(id));
                        for group in &groups {
                            if !this.groups.iter().any(|old| old.id() == group.id()) {
                                this.expanded.insert(group.id());
                                if !this.query.read(cx).value().trim().is_empty() {
                                    this.search_expanded.insert(group.id());
                                }
                            }
                            for node in group.visible_nodes("", false, SortOrder::Default, false) {
                                this.card_focus
                                    .entry((group.id(), node.id()))
                                    .or_insert_with(|| cx.focus_handle());
                            }
                        }
                        let valid = groups
                            .iter()
                            .flat_map(|group| {
                                group
                                    .visible_nodes("", false, SortOrder::Default, false)
                                    .into_iter()
                                    .map(|node| (group.id(), node.id()))
                                    .collect::<Vec<_>>()
                            })
                            .collect::<BTreeSet<_>>();
                        this.card_focus.retain(|key, _| valid.contains(key));
                        this.groups = groups;
                    }
                } else if !session.read(cx).is_running() {
                    this.groups.clear();
                    this.card_focus.clear();
                }
                this.delay_timeout = delay_timeout;
                this.observed = runtime;
                cx.notify();
            }),
            cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    // Search reveals matches without losing the user's normal
                    // expansion state when the query is cleared.
                    this.list_scroll.set_offset(Point::default());
                    this.search_expanded = this.groups.iter().map(ProxyGroup::id).collect();
                }
                this.refresh_list(window, cx);
                cx.notify();
            }),
        ];
        let mut page = Self {
            session,
            query,
            groups,
            observed: runtime,
            expanded,
            search_expanded: BTreeSet::new(),
            sort: SortOrder::Default,
            group_sorts: Default::default(),
            delay_timeout,
            reverse: false,
            only_available: false,
            compact: false,
            columns: 2,
            list_scroll: gpui_kit::component::VirtualListScrollHandle::new(),
            card_focus,
            _subscriptions: subscriptions,
        };
        page.refresh_list(window, cx);
        page
    }

    fn is_expanded(&self, id: &str, searching: bool) -> bool {
        if searching {
            self.search_expanded.contains(id)
        } else {
            self.expanded.contains(id)
        }
    }

    fn toggle_group(
        &mut self,
        id: String,
        searching: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let expanded = if searching {
            &mut self.search_expanded
        } else {
            &mut self.expanded
        };
        if !expanded.remove(&id) {
            expanded.insert(id);
        }
        self.refresh_list(window, cx);
        cx.notify();
    }

    fn expand_all(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let expanded = if self.query.read(cx).value().trim().is_empty() {
            &mut self.expanded
        } else {
            &mut self.search_expanded
        };
        *expanded = if open {
            self.groups.iter().map(ProxyGroup::id).collect()
        } else {
            BTreeSet::new()
        };
        self.refresh_list(window, cx);
        cx.notify();
    }

    fn refresh_list(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }
}
