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
        ActiveTheme, Icon, Selectable, Sizable, TitleBar,
        button::{Button, ButtonVariants},
        h_flex,
        scroll::ScrollableElement,
        v_flex,
    },
    *,
};

gpui_kit::actions!(
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
}

impl SukiApp {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.set_window_title("Suki Clash");
        let session = cx.new(|_| Session::default());
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
                self::window::set_background(event.backdrop(), window);
                cx.notify();
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
        }
    }

    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            cx.notify();
        }
    }

    fn page_weights(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<(Page, f32)> {
        let motion = cx.theme().motion_tokens();
        let policy = gpui_kit::base::motion::Transition::new(motion.duration_normal)
            .easing(motion.easing_move.clone());
        let mut layers: Vec<_> = Page::ALL
            .into_iter()
            .map(|page| {
                let weight = gpui_kit::base::motion::transition(
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
            .px(space::MD)
            .rounded(cx.theme().radius)
            .selected(self.page == page)
            .accessibility_label(page.label())
            .when(collapsed, |button| {
                button.px(space::NONE).tooltip(page.label())
            })
            .child(
                h_flex()
                    .w_full()
                    .gap(space::MD)
                    .when(collapsed, |row| row.justify_center().gap(space::NONE))
                    .child(Icon::new(page.icon()).size(layout::ICON).flex_shrink_0())
                    .when(!collapsed, |row| row.child(div().child(page.label()))),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.navigate(page, cx)))
    }

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = if self.collapsed {
            layout::SIDEBAR_COLLAPSED_WIDTH
        } else {
            layout::SIDEBAR_WIDTH
        };
        let motion = cx.theme().motion_tokens();
        let width = gpui_kit::base::motion::transition(
            "sidebar-width",
            width.to_pixels(window.rem_size()),
            gpui_kit::base::motion::Transition::new(motion.duration_normal)
                .easing(motion.easing_move.clone()),
            window,
            cx,
        );
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
                    .gap(space::XS)
                    .when(!self.collapsed, |column| {
                        column.child(
                            div()
                                .px(space::MD)
                                .pt(space::XS)
                                .pb(space::XS)
                                .text_size(text::CAPTION)
                                .text_color(cx.theme().muted_foreground)
                                .child(label),
                        )
                    })
                    .children(pages.into_iter().map(|page| self.nav_item(page, cx))),
            );
        }
        v_flex()
            .w(width)
            .overflow_hidden()
            .flex_shrink_0()
            .h_full()
            .child(
                h_flex()
                    .h(layout::BRAND_HEIGHT)
                    .flex_shrink_0()
                    .px(space::LG)
                    .gap(space::SM)
                    .when(self.collapsed, |row| {
                        row.px(space::NONE).gap(space::NONE).justify_center()
                    })
                    .child(
                        img(self.logo.clone())
                            .size(layout::BRAND_LOGO_SIZE)
                            .flex_shrink_0()
                            .object_fit(ObjectFit::Contain),
                    )
                    .when(!self.collapsed, |row| {
                        row.child(div().font_weight(text::HEADING).child("Suki Clash"))
                            .child(div().flex_1())
                    }),
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
                    .px(space::MD)
                    .pb(space::MD)
                    .gap(space::SM)
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
                    .border(stroke::NONE)
                    .bg(cx.theme().background.opacity(opacity::TRANSPARENT))
                    .child(
                        h_flex()
                            .w_full()
                            .gap(space::SM)
                            .pr(space::SM)
                            .child(
                                ui::icon_button(
                                    "toggle-sidebar",
                                    IconName::PanelLeft,
                                    "切换侧栏 · ⌘B",
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.collapsed = !this.collapsed;
                                        cx.notify();
                                    },
                                )),
                            )
                            .when(self.page != Page::Overview, |bar| {
                                bar.child(ui::muted("Suki Clash", cx).text_size(text::CAPTION))
                            })
                            .child(div().flex_1())
                            .child(
                                ui::icon_button(
                                    "core-status",
                                    IconName::Cpu,
                                    if running {
                                        "内核运行中 · 打开内核设置"
                                    } else {
                                        "内核已停止 · 打开内核设置"
                                    },
                                )
                                .text_color(if running {
                                    cx.theme().primary
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .accessibility_label(if running {
                                    "内核：运行中"
                                } else {
                                    "内核：已停止"
                                })
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.navigate(Page::Core, cx)),
                                ),
                            )
                            .child(
                                ui::status_toggle(
                                    "system-proxy",
                                    IconName::Globe,
                                    "系统代理",
                                    system_proxy,
                                    cx,
                                )
                                .on_click(move |_, window, cx| {
                                    ui::request_command("切换系统代理", window, cx)
                                }),
                            )
                            .child(
                                ui::status_toggle("tun", IconName::Network, "虚拟网卡", tun, cx)
                                    .on_click(move |_, window, cx| {
                                        ui::request_command("切换虚拟网卡", window, cx)
                                    }),
                            ),
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
    fn empty_workspace_renders_all_routes_without_preview_fallback(cx: &mut TestAppContext) {
        let root = tempfile::tempdir().unwrap();
        let service = crate::integration::Service::start(
            crate::integration::Repository::open(
                crate::integration::AppPaths::isolated(root.path()),
                None,
            )
            .unwrap(),
        )
        .unwrap();
        cx.update(|cx| {
            crate::integration::init(service, cx);
            gpui_kit::init(cx);
            cx.set_reduce_motion(true);
        });
        for dimensions in [size(px(1220.), px(820.)), size(px(960.), px(640.))] {
            let owner = std::rc::Rc::new(std::cell::RefCell::new(None));
            let captured = owner.clone();
            let window = cx.open_window(dimensions, move |window, cx| {
                let view = cx.new(|cx| SukiApp::new(window, cx));
                *captured.borrow_mut() = Some(view.clone());
                gpui_kit::base::Root::new(view, window, cx)
            });
            cx.run_until_parked();
            let app = owner.borrow().as_ref().unwrap().clone();
            for page in Page::ALL {
                app.update(cx, |app, cx| app.navigate(page, cx));
                cx.run_until_parked();
                cx.update_window(window.into(), |_, window, cx| {
                    window.render_frame(cx);
                    if page == Page::Overview {
                        for id in [
                            "upload-empty-plot",
                            "download-empty-plot",
                            "hourly-empty-plot",
                            "traffic-ranking-empty",
                        ] {
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
    }
}
