use crate::ui::theme::{layout, opacity, space, stroke, text};
use gpui_kit::prelude::FluentBuilder as _;
pub(crate) mod window;
use crate::{
    logs::LogsPage,
    overview::Overview,
    profiles::ProfilesPage,
    proxies::ProxiesPage,
    session::Session,
    settings::{SettingsKind, SettingsPage, SettingsSaved},
    traffic::{TrafficKind, TrafficPage},
    ui,
};
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Disableable, Icon, Selectable, Sizable, TitleBar,
        button::{Button, ButtonVariants},
        h_flex,
        scroll::ScrollableElement,
        v_flex,
    },
    *,
};

actions!(
    suki,
    [
        Quit,
        Dashboard,
        Proxies,
        Profiles,
        Connections,
        Settings,
        ToggleSidebar
    ]
);

mod navigation;
use navigation::Page;

pub(crate) struct SukiApp {
    page: Page,
    logo: std::sync::Arc<Image>,
    collapsed: bool,
    focus: FocusHandle,
    session: Entity<Session>,
    overview: Entity<Overview>,
    proxies: Entity<ProxiesPage>,
    profiles: Entity<ProfilesPage>,
    overrides: Entity<ProfilesPage>,
    connections: Entity<TrafficPage>,
    rules: Entity<TrafficPage>,
    logs: Entity<LogsPage>,
    settings: Vec<(SettingsKind, Entity<SettingsPage>)>,
    _session_subscription: Subscription,
    _settings_subscription: Subscription,
    _tun_service_subscription: Subscription,
}

impl SukiApp {
    pub(crate) fn import_uri(&mut self, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Page::Profiles, cx);
        self.profiles
            .update(cx, |profiles, cx| profiles.import_uri(uri, window, cx));
    }
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("Suki Clash");
        crate::core::attach_window(window, cx);
        let session = cx.new(Session::new);
        let overview = cx.new(|cx| Overview::new(session.clone(), cx));
        let proxies = cx.new(|cx| ProxiesPage::new(session.clone(), window, cx));
        let profiles = cx.new(|cx| ProfilesPage::new(false, session.clone(), window, cx));
        let overrides = cx.new(|cx| ProfilesPage::new(true, session.clone(), window, cx));
        let connections =
            cx.new(|cx| TrafficPage::new(TrafficKind::Connections, session.clone(), window, cx));
        let rules = cx.new(|cx| TrafficPage::new(TrafficKind::Rules, session.clone(), window, cx));
        let logs = cx.new(|cx| LogsPage::new(session.clone(), window, cx));
        let settings: Vec<(SettingsKind, Entity<SettingsPage>)> = SettingsKind::ALL
            .into_iter()
            .map(|kind| {
                (
                    kind,
                    cx.new(|cx| SettingsPage::new(kind, session.clone(), window, cx)),
                )
            })
            .collect();
        let general = settings
            .iter()
            .find(|(kind, _)| *kind == SettingsKind::General)
            .unwrap()
            .1
            .clone();
        let settings_subscription = cx.subscribe_in(
            &general,
            window,
            |_, _, event: &SettingsSaved, window, cx| {
                window::set_background(event.backdrop(), window);
                ui::configure_theme(cx);
                cx.notify();
            },
        );
        let tun_service_subscription = cx.subscribe_in(
            &crate::core::controller(cx),
            window,
            |this, _, _: &crate::core::TunServiceRequired, window, cx| {
                this.navigate(Page::Tun, cx);
                let main = window.window_handle();
                cx.defer(move |cx| crate::desktop::show_main(main, cx));
            },
        );
        let saved = crate::integration::read("settings", cx);
        let subscription = cx.observe(&session, |_, _, cx| cx.notify());
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        // Static bindings require explicit modifiers in GPUI 0.3.10.
        let [
            dashboard,
            proxies_shortcut,
            profiles_shortcut,
            connections_shortcut,
            settings_shortcut,
            sidebar,
        ] = if cx.platform_info().is_mac() {
            ["cmd-1", "cmd-2", "cmd-3", "cmd-4", "cmd-,", "cmd-b"]
        } else {
            ["ctrl-1", "ctrl-2", "ctrl-3", "ctrl-4", "ctrl-,", "ctrl-b"]
        };
        cx.bind_keys([
            KeyBinding::new(dashboard, Dashboard, Some("Suki")),
            KeyBinding::new(proxies_shortcut, Proxies, Some("Suki")),
            KeyBinding::new(profiles_shortcut, Profiles, Some("Suki")),
            KeyBinding::new(connections_shortcut, Connections, Some("Suki")),
            KeyBinding::new(settings_shortcut, Settings, Some("Suki")),
            KeyBinding::new(sidebar, ToggleSidebar, Some("Suki")),
        ]);
        Self {
            logo: std::sync::Arc::new(Image::from_bytes(
                ImageFormat::Png,
                include_bytes!("../../assets/logo.png").to_vec(),
            )),
            page: match saved.get("StartPage").map(String::as_str) {
                Some("代理组") => Page::Proxies,
                Some("订阅") => Page::Profiles,
                Some("连接") => Page::Connections,
                Some("规则") => Page::Rules,
                Some("日志") => Page::Logs,
                Some("设置") => Page::Settings,
                _ => Page::Overview,
            },
            collapsed: false,
            focus,
            session,
            overview,
            proxies,
            profiles,
            overrides,
            connections,
            rules,
            logs,
            settings,
            _session_subscription: subscription,
            _settings_subscription: settings_subscription,
            _tun_service_subscription: tun_service_subscription,
        }
    }

    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if page == Page::Overview
            && let Some(window) = cx.active_window()
        {
            cx.defer(move |cx| {
                let _ = window.update(cx, |_, window, cx| {
                    if crate::core::controller(cx).read(cx).snapshot().phase()
                        == crate::core::Phase::Ready
                    {
                        crate::core::refresh_overview(window, cx);
                    }
                });
            });
        }
        if self.page != page {
            if page == Page::Override {
                self.overrides.update(cx, |page, cx| page.reopen(cx));
            }
            if let Some(kind) = page.settings_kind()
                && let Some((_, settings)) = self.settings.iter().find(|(key, _)| *key == kind)
                && let Some(window) = cx.active_window()
            {
                let settings = settings.clone();
                cx.defer(move |cx| {
                    let _ = window.update(cx, |_, window, cx| {
                        settings.update(cx, |page, cx| page.reopen(window, cx))
                    });
                });
            }
            self.page = page;
            cx.notify();
        }
    }

    fn page_weights(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<(Page, f32)> {
        let motion = cx.theme().motion_tokens();
        let policy = base::motion::Transition::new(motion.duration_normal)
            .easing(motion.easing_move.clone());
        let mut layers: Vec<_> = Page::ALL
            .into_iter()
            .map(|page| {
                let weight = base::motion::transition(
                    (ElementId::from("page-handoff"), page.id()),
                    if page == self.page { 1.0 } else { 0.0 },
                    policy.clone(),
                    window,
                    cx,
                );
                (page, weight)
            })
            .filter(|(page, weight)| *weight > 0. || *page == self.page)
            .collect();
        let total: f32 = layers.iter().map(|(_, weight)| *weight).sum();
        if total > 0. {
            for (_, weight) in &mut layers {
                *weight /= total;
            }
        }
        layers
    }

    fn nav_item(&self, page: Page, cx: &mut Context<Self>) -> Button {
        let collapsed = self.collapsed;
        Button::new(SharedString::from(format!("nav-{}", page.id())))
            .ghost()
            .small()
            .w_full()
            .h(layout::NAV_ITEM_HEIGHT)
            .flex_shrink_0()
            .px(space::MD)
            .overflow_hidden()
            .rounded(cx.theme().radius)
            .selected(self.page == page)
            .accessibility_label(page.label())
            .when(collapsed, |button| button.tooltip(page.label()))
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap(space::MD)
                    .child(Icon::new(page.icon()).size(layout::ICON).flex_shrink_0())
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(page.label()),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
    }

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let motion = cx.theme().motion_tokens();
        let expanded = base::motion::transition(
            "sidebar-expanded",
            if self.collapsed { 0.0_f32 } else { 1.0 },
            base::motion::Transition::new(motion.duration_normal)
                .easing(motion.easing_move.clone()),
            window,
            cx,
        );
        let collapsed_width = layout::SIDEBAR_COLLAPSED_WIDTH.to_pixels(window.rem_size());
        let expanded_width = layout::SIDEBAR_WIDTH.to_pixels(window.rem_size());
        let width = collapsed_width + (expanded_width - collapsed_width) * expanded;
        let header_height = layout::SIDEBAR_GROUP_HEADER_HEIGHT.to_pixels(window.rem_size());
        let brand_inset =
            (collapsed_width - layout::BRAND_LOGO_SIZE.to_pixels(window.rem_size())) / 2.;
        let brand_inset =
            brand_inset + (space::LG.to_pixels(window.rem_size()) - brand_inset) * expanded;
        let mut navigation = v_flex().gap(space::LG).px(space::MD).pb(space::MD);
        for (label, pages) in [
            (
                "工作区",
                vec![
                    Page::Overview,
                    Page::Proxies,
                    Page::Profiles,
                    Page::Connections,
                ],
            ),
            (
                "流量配置",
                vec![Page::Rules, Page::Override, Page::Dns, Page::Sniffer],
            ),
            ("运行", vec![Page::Core, Page::Resources, Page::Logs]),
        ] {
            navigation = navigation.child(
                v_flex()
                    .child(
                        div()
                            .h(header_height * expanded)
                            .flex_shrink_0()
                            .overflow_hidden()
                            .child(
                                div()
                                    .h(header_height)
                                    .px(space::MD)
                                    .pt(space::XS)
                                    .pb(space::XS)
                                    .text_size(text::CAPTION)
                                    .text_color(cx.theme().muted_foreground)
                                    .whitespace_nowrap()
                                    .child(label),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap(space::XS)
                            .children(pages.into_iter().map(|page| self.nav_item(page, cx))),
                    ),
            );
        }
        v_flex()
            .id("sidebar")
            .w(width)
            .overflow_hidden()
            .flex_shrink_0()
            .h_full()
            .child(
                h_flex()
                    .h(layout::BRAND_HEIGHT)
                    .flex_shrink_0()
                    .px(brand_inset)
                    .gap(space::SM)
                    .overflow_hidden()
                    .child(
                        Button::new("toggle-sidebar-logo")
                            .ghost()
                            .p(space::NONE)
                            .size(layout::BRAND_LOGO_SIZE)
                            .flex_shrink_0()
                            .accessibility_label(if self.collapsed {
                                "展开侧栏"
                            } else {
                                "收起侧栏"
                            })
                            .tooltip(if cx.platform_info().is_mac() {
                                "切换侧栏 · ⌘B"
                            } else {
                                "切换侧栏 · Ctrl+B"
                            })
                            .child(
                                img(self.logo.clone())
                                    .size(layout::BRAND_LOGO_SIZE)
                                    .object_fit(ObjectFit::Contain),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.collapsed = !this.collapsed;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .font_weight(text::HEADING)
                            .child("Suki Clash"),
                    ),
            )
            .child(
                div()
                    .id("main-navigation-scroll")
                    .flex_1()
                    .min_h(space::NONE)
                    .overflow_y_scrollbar()
                    .child(navigation),
            )
            .child(
                v_flex()
                    .flex_shrink_0()
                    .px(space::MD)
                    .pb(space::MD)
                    .gap(space::XS)
                    .child(self.nav_item(Page::SysProxy, cx))
                    .child(self.nav_item(Page::Tun, cx))
                    .child(self.nav_item(Page::Settings, cx)),
            )
    }

    fn content(&self, route: Page) -> AnyElement {
        match route {
            Page::Overview => self.overview.clone().into_any_element(),
            Page::Proxies => self.proxies.clone().into_any_element(),
            Page::Profiles => self.profiles.clone().into_any_element(),
            Page::Override => self.overrides.clone().into_any_element(),
            Page::Connections => self.connections.clone().into_any_element(),
            Page::Rules => self.rules.clone().into_any_element(),
            Page::Logs => self.logs.clone().into_any_element(),
            page => self
                .settings
                .iter()
                .find(|(key, _)| Some(*key) == page.settings_kind())
                .expect("every settings route is registered")
                .1
                .clone()
                .into_any_element(),
        }
    }
}

impl Render for SukiApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let system_proxy = self.session.read(cx).system_proxy();
        let tun = self.session.read(cx).tun();
        let running = self.session.read(cx).is_running();
        let core_label = self.session.read(cx).core().label();
        v_flex()
            .id("suki-window")
            .key_context("Suki")
            .track_focus(&self.focus)
            .size_full()
            .text_size(text::BODY)
            .bg(ui::window_tint(cx))
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &Dashboard, _, cx| this.navigate(Page::Overview, cx)))
            .on_action(cx.listener(|this, _: &Proxies, _, cx| this.navigate(Page::Proxies, cx)))
            .on_action(cx.listener(|this, _: &Profiles, _, cx| this.navigate(Page::Profiles, cx)))
            .on_action(
                cx.listener(|this, _: &Connections, _, cx| this.navigate(Page::Connections, cx)),
            )
            .on_action(cx.listener(|this, _: &Settings, _, cx| this.navigate(Page::Settings, cx)))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.collapsed = !this.collapsed;
                cx.notify();
            }))
            .child(
                TitleBar::new()
                    .on_close_window(|_, window, cx| crate::desktop::close_main(window, cx))
                    .border(stroke::NONE)
                    .bg(cx.theme().background.opacity(opacity::TRANSPARENT))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(space::SM)
                            .pr(space::SM)
                            .when(self.collapsed, |bar| {
                                let title = ui::muted("Suki Clash", cx)
                                    .id("window-title")
                                    .text_size(text::BODY);
                                #[cfg(test)]
                                let title = {
                                    use gpui_kit::test::TestSupportExt;
                                    title.test_support()
                                };
                                bar.child(title)
                            })
                            .child(div().flex_1())
                            .child(window::titlebar_control(
                                ui::icon_button(
                                    "core-status",
                                    IconName::Cpu,
                                    &format!("内核{core_label} · 打开内核设置"),
                                )
                                .text_color(if running {
                                    cx.theme().primary
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .accessibility_label(format!("内核：{core_label}"))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.navigate(Page::Core, cx)),
                                ),
                            ))
                            .child(window::titlebar_control(
                                ui::status_toggle(
                                    "system-proxy",
                                    IconName::Globe,
                                    "系统代理",
                                    system_proxy,
                                    cx,
                                )
                                .disabled(self.session.read(cx).core_busy())
                                .on_click(move |_, window, cx| {
                                    ui::request_command("切换系统代理", window, cx)
                                }),
                            ))
                            .child(window::titlebar_control(
                                ui::status_toggle("tun", IconName::Network, "虚拟网卡", tun, cx)
                                    .disabled(self.session.read(cx).core_busy())
                                    .on_click(move |_, window, cx| {
                                        ui::request_command("切换虚拟网卡", window, cx)
                                    }),
                            )),
                    ),
            )
            .child(
                h_flex()
                    .items_stretch()
                    .flex_1()
                    .min_h(space::NONE)
                    .w_full()
                    .child(self.sidebar(window, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(space::NONE)
                            .min_h(space::NONE)
                            .mr(space::SM)
                            .mb(space::SM)
                            .rounded(cx.theme().radius_lg)
                            .when(self.page != Page::Overview, |pane| {
                                pane.bg(ui::workspace_tint(cx))
                            })
                            .overflow_hidden()
                            .child({
                                let layers = self.page_weights(window, cx);
                                div().id("page-transition").size_full().relative().children(
                                    layers.into_iter().map(|(page, weight)| {
                                        div()
                                            .id((ElementId::from("page-layer"), page.id()))
                                            .absolute()
                                            .size_full()
                                            .opacity(weight)
                                            .child(self.content(page))
                                    }),
                                )
                            }),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Page, SukiApp};
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{AppContext as _, TestAppContext, px, size};

    #[gpui_kit::test]
    fn startup_failure_keeps_navigation_and_settings_visible(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let paths = crate::storage::AppPaths::isolated(root.path());
        let repository = crate::storage::Repository::open(paths.clone()).unwrap();
        let broken = paths.settings().join("settings.json");
        std::fs::write(&broken, b"broken settings").unwrap();
        let service = crate::integration::Service::start(repository).unwrap();
        assert!(service.startup_error().is_some());
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, None, cx);
            cx.set_reduce_motion(true);
        });
        let handle = cx.open_window(size(px(1220.), px(820.)), |window, cx| {
            let app = cx.new(|cx| SukiApp::new(window, cx));
            gpui_kit::base::Root::new(app, window, cx)
        });
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("nav-overview").bounds().size.width > px(0.));
            window.click("nav-settings", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("nav-settings").bounds().size.width > px(0.));
            window.click("nav-logs", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find("logs-clear").bounds().size.width > px(0.));
        })
        .unwrap();
        assert_eq!(std::fs::read(broken).unwrap(), b"broken settings");
        crate::integration::finish_test_services(cx);
    }
    #[gpui_kit::test]
    fn unavailable_executor_does_not_repeat_startup_error_from_background_pages(
        cx: &mut TestAppContext,
    ) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::integration::Repository::open(crate::integration::AppPaths::isolated(
                root.path(),
            ))
            .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::integration::init(service, Some("内核执行端不可用".into()), cx);
            cx.set_reduce_motion(true);
        });
        let handle = cx.open_window(size(px(1220.), px(820.)), |window, cx| {
            let app = cx.new(|cx| SukiApp::new(window, cx));
            gpui_kit::base::Root::new(app, window, cx)
        });
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            assert_eq!(crate::ui::notifications::current(window, cx).len(), 1);
            window.click("nav-resources", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle.into(), |_, window, cx| {
            assert_eq!(crate::ui::notifications::current(window, cx).len(), 1);
        })
        .unwrap();
        crate::integration::finish_test_services(cx);
    }

    #[gpui_kit::test]
    fn sidebar_geometry_moves_together_and_reverses_without_jumps(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::integration::Repository::open(crate::integration::AppPaths::isolated(
                root.path(),
            ))
            .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(false);
        });
        let owner = std::rc::Rc::new(std::cell::RefCell::new(None));
        let captured = owner.clone();
        let handle = cx.open_window(size(px(1220.), px(820.)), move |window, cx| {
            let view = cx.new(|cx| SukiApp::new(window, cx));
            *captured.borrow_mut() = Some(view.clone());
            gpui_kit::base::Root::new(view, window, cx)
        });
        cx.run_until_parked();
        let app = owner.borrow().as_ref().unwrap().clone();
        let geometry = |cx: &mut TestAppContext| {
            cx.update_window(handle.into(), |_, window, cx| {
                window.render_frame(cx);
                ["nav-overview", "nav-rules", "nav-settings"].map(|id| window.find(id).bounds())
            })
            .unwrap()
        };
        let initial = geometry(cx);
        app.update(cx, |app, cx| {
            app.collapsed = true;
            cx.notify();
        });
        let start = geometry(cx);
        assert_eq!(
            start, initial,
            "collapse must not immediately rearrange navigation"
        );
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(70));
        let midway = geometry(cx);
        assert!(midway[0].size.width < initial[0].size.width);
        assert!(midway[0].top() < initial[0].top());
        assert!(midway[1].top() < initial[1].top());
        assert_eq!(
            midway[2].top(),
            initial[2].top(),
            "footer must remain anchored"
        );

        app.update(cx, |app, cx| {
            app.collapsed = false;
            cx.notify();
        });
        let reversed = geometry(cx);
        for (before, after) in midway.into_iter().zip(reversed) {
            assert!((before.size.width - after.size.width).abs() < px(0.01));
            assert!((before.top() - after.top()).abs() < px(0.01));
        }
        cx.update(|cx| cx.set_reduce_motion(true));
        assert_eq!(geometry(cx), initial);
        app.update(cx, |app, cx| {
            app.collapsed = true;
            cx.notify();
        });
        let collapsed = geometry(cx);
        assert!(collapsed[0].size.width < midway[0].size.width);
        assert!(collapsed[0].top() < midway[0].top());
        cx.update_window(handle.into(), |_, window, cx| window.click("nav-rules", cx))
            .unwrap();
        cx.run_until_parked();
        cx.update(|cx| assert_eq!(app.read(cx).page, Page::Rules));
        crate::integration::finish_test_services(cx);
    }

    #[gpui_kit::test]
    fn empty_workspace_renders_all_routes_without_preview_fallback(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::integration::Repository::open(crate::integration::AppPaths::isolated(
                root.path(),
            ))
            .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, None, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        for dimensions in [
            size(px(1220.), px(820.)),
            size(px(960.), px(640.)),
            size(px(2400.), px(1080.)),
        ] {
            let owner = std::rc::Rc::new(std::cell::RefCell::new(None));
            let captured = owner.clone();
            let window = cx.open_window(dimensions, move |window, cx| {
                let view = cx.new(|cx| SukiApp::new(window, cx));
                *captured.borrow_mut() = Some(view.clone());
                gpui_kit::base::Root::new(view, window, cx)
            });
            cx.run_until_parked();
            let app = owner.borrow().as_ref().unwrap().clone();
            cx.update_window(window.into(), |_, window, cx| {
                window.click("toggle-sidebar-logo", cx);
            })
            .unwrap();
            cx.run_until_parked();
            cx.update(|cx| assert!(app.read(cx).collapsed));
            cx.update_window(window.into(), |_, window, cx| {
                window.render_frame(cx);
                assert!(window.find("window-title").visible());
                assert!(window.find_all("toggle-sidebar").is_empty());
            })
            .unwrap();
            cx.update_window(window.into(), |_, window, cx| {
                window.press(
                    if cx.platform_info().is_mac() {
                        "cmd-b"
                    } else {
                        "ctrl-b"
                    },
                    cx,
                );
            })
            .unwrap();
            cx.run_until_parked();
            cx.update(|cx| assert!(!app.read(cx).collapsed));
            cx.update_window(window.into(), |_, window, cx| {
                window.click("core-status", cx);
            })
            .unwrap();
            cx.run_until_parked();
            cx.update(|cx| assert_eq!(app.read(cx).page, Page::Core));
            for page in Page::ALL {
                app.update(cx, |app, cx| app.navigate(page, cx));
                cx.run_until_parked();
                cx.update_window(window.into(), |_, window, cx| {
                    window.render_frame(cx);
                    assert!(
                        window.find_all("window-title").is_empty(),
                        "title visible while sidebar is expanded"
                    );
                    if page == Page::Overview {
                        for id in ["upload-empty-plot", "download-empty-plot"] {
                            let bounds = window.find(id).bounds();
                            assert!(
                                bounds.size.width > px(0.) && bounds.size.height > px(0.),
                                "empty region collapsed: {id}"
                            );
                            assert!(
                                bounds.right() <= window.viewport_size().width,
                                "empty region overflowed: {id}"
                            );
                        }
                    } else if page == Page::Proxies {
                        let bounds = window.find("proxy-groups-empty").bounds();
                        assert!(bounds.size.width > px(0.) && bounds.size.height >= px(270.));
                    }
                })
                .unwrap();
            }
        }
        crate::integration::finish_test_services(cx);
    }
}
