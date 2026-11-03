mod chain;
pub(crate) mod chain_config;
pub(crate) mod chain_sources;
mod model;
mod node_list;
pub(crate) mod preferences;
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
    search: model::Filter,
    group_filter: BTreeSet<String>,
    group_urls: BTreeMap<String, Entity<InputState>>,
    test_preferences: (Option<u64>, preferences::Preferences),
    url_editing: BTreeSet<String>,
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
    fn test_urls(&self, cx: &App) -> BTreeMap<String, String> {
        self.session
            .read(cx)
            .core()
            .proxy_preferences
            .profile(self.session.read(cx).active_profile_id())
            .test_urls
    }

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
        let test_preferences = (
            session.read(cx).active_profile_id(),
            session.read(cx).core().proxy_preferences.clone(),
        );
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
            cx.observe_in(&session, window, |this, session, window, cx| {
                let delay_timeout = crate::integration::read("settings", cx)
                    .get("DelayTestTimeout")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(5000);
                let preferences = (
                    session.read(cx).active_profile_id(),
                    session.read(cx).core().proxy_preferences.clone(),
                );
                if this.test_preferences != preferences {
                    let urls = preferences.1.profile(preferences.0).test_urls;
                    let previous_urls = this
                        .test_preferences
                        .1
                        .profile(this.test_preferences.0)
                        .test_urls;
                    for (group, input) in &this.group_urls {
                        if this.test_preferences.0 == preferences.0
                            && previous_urls.get(group) == urls.get(group)
                        {
                            continue;
                        }
                        input.update(cx, |state, cx| {
                            state.set_value(
                                urls.get(group).cloned().unwrap_or_default(),
                                window,
                                cx,
                            )
                        });
                    }
                    this.test_preferences = preferences;
                }
                let runtime = session.read(cx).runtime();
                if runtime.connected && session.read(cx).is_running() {
                    if this.observed.proxies != runtime.proxies
                        || this.observed.mode != runtime.mode
                        || this.delay_timeout != delay_timeout
                        || this.groups.is_empty()
                    {
                        let mut groups =
                            ProxyGroup::observed(&runtime.proxies, runtime.mode, delay_timeout);
                        for group in &mut groups {
                            if runtime
                                .proxies
                                .batches
                                .iter()
                                .any(|(id, _)| *id == group.id())
                                && let Some(previous) =
                                    this.groups.iter().find(|old| old.id() == group.id())
                            {
                                group.keep_latency_order(previous);
                            }
                        }
                        let old_order = this
                            .groups
                            .iter()
                            .map(|group| group.id())
                            .collect::<Vec<_>>();
                        groups.sort_by_key(|group| {
                            old_order
                                .iter()
                                .position(|id| id == &group.id())
                                .unwrap_or(usize::MAX)
                        });
                        let ids = groups.iter().map(ProxyGroup::id).collect::<BTreeSet<_>>();
                        this.expanded.retain(|id| ids.contains(id));
                        this.group_filter.retain(|id| ids.contains(id));
                        for group in &groups {
                            if !this.group_urls.contains_key(&group.id()) {
                                let input = this.url_input(&group.id(), window, cx);
                                this.group_urls.insert(group.id(), input);
                            }
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
                    this.search.compile(this.query.read(cx).value().as_ref());
                }
                if matches!(event, InputEvent::Blur)
                    && let Some(error) = &this.search.text.error
                {
                    crate::ui::show_error(error.clone(), window, cx);
                }
                this.refresh_list(window, cx);
                cx.notify();
            }),
        ];
        let mut page = Self {
            session,
            query,
            search: Default::default(),
            group_filter: BTreeSet::new(),
            group_urls: BTreeMap::new(),
            test_preferences,
            url_editing: Default::default(),
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
        for group in page.groups.iter().map(ProxyGroup::id).collect::<Vec<_>>() {
            let input = page.url_input(&group, window, cx);
            page.group_urls.insert(group, input);
        }
        page.refresh_list(window, cx);
        page
    }

    fn url_input(
        &mut self,
        group: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let value = self.test_urls(cx).get(group).cloned().unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("测速地址，留空使用默认")
                .default_value(value)
        });
        let group = group.to_owned();
        self._subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, input, event, window, cx| {
                if matches!(event, InputEvent::Blur) {
                    let url = input.read(cx).value().trim().to_owned();
                    let profile = this.session.read(cx).active_profile_id();
                    if this
                        .test_urls(cx)
                        .get(&group)
                        .map(String::as_str)
                        .unwrap_or("")
                        != url
                    {
                        crate::core::request(
                            crate::core::Command::ProxyPreferences {
                                profile,
                                change: preferences::Change::TestUrl {
                                    group: group.clone(),
                                    url,
                                },
                            },
                            window,
                            cx,
                        );
                    }
                }
            },
        ));
        input
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
