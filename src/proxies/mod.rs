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
    list_scroll: component::VirtualListScrollHandle,
    locate_task: Option<Task<()>>,
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
        let groups = if session.read(cx).is_running() {
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
                if session.read(cx).is_running() {
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
                            (
                                this.observed.mode != runtime.mode
                                    && runtime.mode == Some(crate::session::ProxyMode::Global)
                                    && group.id() != "GLOBAL",
                                old_order
                                    .iter()
                                    .position(|id| id == &group.id())
                                    .unwrap_or(usize::MAX),
                            )
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
            list_scroll: component::VirtualListScrollHandle::new(),
            locate_task: None,
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
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("测速地址，留空使用默认")
                .default_value(value)
        })
    }

    /// The dialog owns draft editing; committing sends the one persisted test-URL change.
    fn commit_test_url(&mut self, group: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.group_urls.get(group) else {
            return;
        };
        let url = input.read(cx).value().trim().to_owned();
        if self
            .test_urls(cx)
            .get(group)
            .map(String::as_str)
            .unwrap_or("")
            == url
        {
            return;
        }
        let profile = self.session.read(cx).active_profile_id();
        crate::core::request(
            crate::core::Command::ProxyPreferences {
                profile,
                change: preferences::Change::TestUrl {
                    group: group.to_owned(),
                    url,
                },
            },
            window,
            cx,
        );
    }

    fn edit_test_url(&mut self, group: String, window: &mut Window, cx: &mut Context<Self>) {
        use crate::ui::{
            self,
            theme::{control, layout},
        };
        use gpui_kit::component::{
            Sizable as _, WindowExt,
            button::{Button, ButtonVariants},
            input::Input,
        };
        let Some(input) = self.group_urls.get(&group).cloned() else {
            return;
        };
        let saved = self.test_urls(cx).get(&group).cloned().unwrap_or_default();
        input.update(cx, |state, cx| state.set_value(saved, window, cx));
        let owner = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let (input, owner, group) = (input.clone(), owner.clone(), group.clone());
            let field = input.clone();
            ui::modal(dialog, "测速地址", layout::DIALOG_WIDTH, window, cx)
                .content(ui::modal_content(
                    "proxy-group-url-content",
                    layout::FORM_MAX_HEIGHT,
                    move |_, _| {
                        ui::form()
                            .child(ui::field_label(
                                "测速地址",
                                Input::new(&field).w_full().with_size(control::STANDARD),
                            ))
                            .into_any_element()
                    },
                ))
                .footer(
                    ui::modal_footer()
                        .child(
                            Button::new("cancel-group-url")
                                .icon(assets::IconName::X)
                                .with_size(control::STANDARD)
                                .outline()
                                .label("取消")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("save-group-url")
                                .icon(assets::IconName::Check)
                                .with_size(control::STANDARD)
                                .primary()
                                .label("保存")
                                .on_click(move |_, window, cx| {
                                    owner
                                        .update(cx, |this, cx| {
                                            this.commit_test_url(&group, window, cx)
                                        })
                                        .ok();
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
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
        self.locate_task = None;
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
        self.locate_task = None;
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

#[cfg(test)]
mod tests {
    use super::{ProxiesPage, model::ProxyGroup, runtime};
    use crate::session::Session;
    use gpui_kit::{AppContext, TestAppContext, WindowOptions};

    #[gpui_kit::test]
    fn entering_global_mode_prepends_exit_without_losing_group_order_or_filter(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let service =
            crate::integration::Service::start(crate::storage::Repository::open(paths).unwrap())
                .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
        });
        let (handle, session, page) = cx.update(|cx| {
            let session = cx.new(|_| Session::default());
            let state = session.clone();
            let (handle, page) =
                gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                    cx.new(|cx| ProxiesPage::new(state, window, cx))
                })
                .unwrap();
            (handle, session, page)
        });
        let proxies = runtime::Proxies::parse(
            br#"{"proxies":{"outer":{"name":"outer","type":"Selector","all":["inner"],"now":"inner"},"inner":{"name":"inner","type":"Selector","all":["DIRECT"],"now":"DIRECT"},"GLOBAL":{"name":"GLOBAL","type":"Selector","all":["outer"],"now":"outer"},"DIRECT":{"name":"DIRECT","type":"Direct"}}}"#,
            &["outer".into(), "inner".into()],
        )
        .unwrap();
        for mode in [
            crate::session::ProxyMode::Rule,
            crate::session::ProxyMode::Global,
        ] {
            cx.update(|cx| {
                session.update(cx, |session, cx| {
                    let snapshot = crate::core::Snapshot::ready_with_providers(
                        crate::core::ProviderKind::Proxies,
                        br#"{"providers":{}}"#,
                    )
                    .unwrap();
                    *session = Session::fixture(
                        snapshot,
                        crate::core::Runtime {
                            connected: true,
                            mode: Some(mode),
                            proxies: proxies.clone(),
                            ..Default::default()
                        },
                    );
                    cx.notify();
                });
            });
            cx.run_until_parked();
            cx.update_window(handle, |_, _, cx| {
                page.update(cx, |page, _| {
                    if mode == crate::session::ProxyMode::Rule {
                        page.groups.reverse();
                        page.group_filter.insert("inner".into());
                    } else {
                        assert_eq!(
                            page.groups.iter().map(ProxyGroup::id).collect::<Vec<_>>(),
                            ["GLOBAL", "inner", "outer"]
                        );
                        assert_eq!(page.group_filter, ["inner".to_owned()].into());
                        assert!(page.expanded.contains("GLOBAL"));
                    }
                });
            })
            .unwrap();
        }
        crate::integration::finish_test_services(cx);
    }
}
