mod chain;
mod model;
mod node_list;
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
    expanded: BTreeSet<&'static str>,
    search_expanded: BTreeSet<&'static str>,
    sort: SortOrder,
    reverse: bool,
    only_available: bool,
    compact: bool,
    columns: usize,
    card_focus: BTreeMap<(&'static str, &'static str), FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl ProxiesPage {
    pub(crate) fn new(
        session: Entity<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索代理组或节点…"));
        let groups: Vec<ProxyGroup> = Vec::new();
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
            cx.observe(&session, |_, _, cx| {
                cx.notify();
            }),
            cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    // Search reveals matches without losing the user's normal
                    // expansion state when the query is cleared.
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
            expanded: BTreeSet::new(),
            search_expanded: BTreeSet::new(),
            sort: SortOrder::Default,
            reverse: false,
            only_available: false,
            compact: false,
            columns: 2,
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
        id: &'static str,
        searching: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let expanded = if searching {
            &mut self.search_expanded
        } else {
            &mut self.expanded
        };
        if !expanded.remove(id) {
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
