use super::Overview;
use crate::ui;
use crate::ui::theme::{self, layout, opacity, space, stroke, text};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable,
        chart::LineChart,
        h_flex,
        scroll::ScrollableElement,
        tab::{Tab, TabBar},
        v_flex,
    },
    *,
};

fn bytes(value: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut number = value as f64;
    let mut unit = 0;
    while number >= 1024. && unit < units.len() - 1 {
        number /= 1024.;
        unit += 1;
    }
    format!("{number:.1} {}", units[unit])
}
impl Overview {
    fn empty_plot(id: &'static str, message: &'static str, cx: &App) -> impl IntoElement + Styled {
        let plot = div().id(id);
        #[cfg(test)]
        let plot = {
            use gpui_kit::test::TestSupportExt;
            plot.test_support()
        };
        plot.relative()
            .size_full()
            .min_h(space::NONE)
            .child(v_flex().absolute().size_full().justify_between().children(
                (0..layout::EMPTY_CHART_GRID_LINES).map(|_| {
                    div()
                        .w_full()
                        .border_b(stroke::HAIRLINE)
                        .border_color(cx.theme().foreground.opacity(opacity::PREFERENCES_DIVIDER))
                }),
            ))
            .child(
                h_flex()
                    .relative()
                    .size_full()
                    .justify_center()
                    .child(ui::muted(message, cx).text_size(text::CAPTION)),
            )
    }

    fn value(label: &str, value: &str, unit: &str, cx: &App) -> Div {
        v_flex()
            .gap(space::SM)
            .child(ui::muted(label.to_owned(), cx))
            .child(
                h_flex()
                    .items_baseline()
                    .gap(space::XS)
                    .child(
                        div()
                            .text_size(text::METRIC)
                            .font_weight(text::EMPHASIS)
                            .child(value.to_owned()),
                    )
                    .child(div().text_size(text::BODY).child(unit.to_owned())),
            )
    }

    fn context(label: &str, value: String, cx: &App) -> Div {
        v_flex()
            .flex_1()
            .min_w(space::NONE)
            .gap(space::SM)
            .child(ui::muted(label.to_owned(), cx).text_size(text::CAPTION))
            .child(div().truncate().font_weight(text::EMPHASIS).child(value))
    }

    fn rate_card(&self, upload: bool, cx: &App) -> Div {
        let runtime = self.session.read(cx).runtime();
        let statistics = &runtime.statistics;
        let rate = if upload {
            statistics.upload_rate
        } else {
            statistics.download_rate
        };
        let value = rate.map(bytes).unwrap_or_else(|| "—".into());
        let plot = if statistics.rates.is_empty() {
            Self::empty_plot(
                if upload {
                    "upload-empty-plot"
                } else {
                    "download-empty-plot"
                },
                "暂无速率记录",
                cx,
            )
            .into_any_element()
        } else {
            let chart = LineChart::new(statistics.rates.clone())
                .x(|point: &super::statistics::Point| point.time.clone())
                .y(move |point: &super::statistics::Point| {
                    if upload {
                        point.upload as f64
                    } else {
                        point.download as f64
                    }
                })
                .stroke(if upload {
                    theme::upload_color(cx)
                } else {
                    theme::download_color(cx)
                })
                .x_axis(false)
                .y_axis(false)
                .grid(true);
            let frame = div()
                .id(if upload {
                    "upload-rate-plot"
                } else {
                    "download-rate-plot"
                })
                .size_full();
            #[cfg(test)]
            let frame = {
                use gpui_kit::test::TestSupportExt;
                frame.test_support()
            };
            frame.child(chart).into_any_element()
        };
        ui::surface(cx)
            .flex_1()
            .min_w(space::NONE)
            .h(layout::OVERVIEW_TOP_HEIGHT)
            .overflow_hidden()
            .text_color(if upload {
                theme::upload_color(cx)
            } else {
                theme::download_color(cx)
            })
            .child(div().p(space::XL).child(Self::value(
                if upload { "上传" } else { "下载" },
                &value,
                if rate.is_some() { "/s" } else { "" },
                cx,
            )))
            .child(
                v_flex()
                    .flex_1()
                    .min_h(space::NONE)
                    .px(space::XL)
                    .pb(space::LG)
                    .gap(space::SM)
                    .child(div().size_full().child(plot).flex_1())
                    .child(
                        h_flex()
                            .justify_between()
                            .child(ui::muted("较早", cx).text_size(text::CAPTION))
                            .child(ui::muted("现在", cx).text_size(text::CAPTION)),
                    ),
            )
    }
    fn latency_card(&self, cx: &App) -> Div {
        let runtime = self.session.read(cx).runtime();
        let latency = runtime
            .latency
            .map(|value| value.to_string())
            .unwrap_or_else(|| "—".into());
        ui::surface(cx)
            .flex_1()
            .min_w(space::NONE)
            .h(layout::OVERVIEW_TOP_HEIGHT)
            .p(space::XL)
            .justify_between()
            .child(Self::value(
                "出口请求耗时",
                &latency,
                if runtime.latency.is_some() { "ms" } else { "" },
                cx,
            ))
            .child(
                h_flex().gap(space::LG).children(
                    [
                        (
                            "路由",
                            runtime
                                .mode
                                .map(|mode| mode.label().to_owned())
                                .unwrap_or_else(|| "—".into()),
                        ),
                        (
                            "DNS",
                            runtime.dns_mode.clone().unwrap_or_else(|| "—".into()),
                        ),
                        (
                            "代理",
                            if runtime.connected {
                                runtime
                                    .proxy_port
                                    .map(|port| format!("127.0.0.1:{port}"))
                                    .unwrap_or_else(|| "未启用".into())
                            } else {
                                "未连接".into()
                            },
                        ),
                    ]
                    .map(|(label, value)| Self::context(label, value, cx)),
                ),
            )
    }
    fn connections_card(&self, cx: &App) -> Div {
        let runtime = self.session.read(cx).runtime();
        let statistics = &runtime.statistics;
        let count = statistics
            .connections
            .map(|value| value.to_string())
            .unwrap_or_else(|| "—".into());
        ui::surface(cx)
            .h(layout::OVERVIEW_SUMMARY_HEIGHT)
            .p(space::XL)
            .justify_between()
            .child(Self::value("活动连接", &count, "", cx))
            .child(
                h_flex().gap(space::LG).children(
                    [
                        ("进程", statistics.processes),
                        ("代理连接", statistics.proxy_connections),
                        ("直连", statistics.direct_connections),
                    ]
                    .map(|(label, value)| {
                        Self::context(
                            label,
                            if statistics.connections.is_some() {
                                value.to_string()
                            } else {
                                "—".into()
                            },
                            cx,
                        )
                    }),
                ),
            )
    }
    fn totals_card(&self, cx: &mut Context<Self>) -> Div {
        let runtime = self.session.read(cx).runtime();
        let totals = if self.period == 0 {
            &runtime.statistics.today
        } else {
            &runtime.statistics.month
        };
        ui::surface(cx)
            .h(layout::OVERVIEW_SUMMARY_HEIGHT)
            .p(space::XL)
            .justify_between()
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap(space::SM)
                    .child(Self::value("已采集流量", &bytes(totals.bytes()), "", cx))
                    .child(
                        TabBar::new("overview-period")
                            .segmented()
                            .small()
                            .children([Tab::new().label("今日"), Tab::new().label("本月")])
                            .selected_index(self.period)
                            .on_click(cx.listener(|this, index, _, cx| {
                                this.period = *index;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(ui::muted(format!("DIRECT  {}", bytes(totals.direct)), cx))
                    .child(ui::muted(format!("代理  {}", bytes(totals.proxy)), cx)),
            )
    }
    fn traffic_card(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let runtime = self.session.read(cx).runtime();
        let statistics = &runtime.statistics;
        let ranks = if self.proxy_only {
            &statistics.proxy_rankings
        } else {
            &statistics.rankings
        };
        let ranks = ranks.get(self.breakdown).cloned().unwrap_or_default();
        let plot = if statistics.hours.iter().all(|hour| hour.1 == 0) {
            Self::empty_plot("hourly-empty-plot", "暂无小时流量记录", cx).into_any_element()
        } else {
            let proxy = self.proxy_only;
            LineChart::new(statistics.hours.clone())
                .x(|point: &(String, u64, u64)| point.0.clone())
                .y(move |point: &(String, u64, u64)| {
                    if proxy {
                        point.2 as f64
                    } else {
                        point.1 as f64
                    }
                })
                .stroke(theme::download_color(cx))
                .x_axis(false)
                .y_axis(false)
                .grid(true)
                .into_any_element()
        };
        let ranking = v_flex().id("traffic-ranking-empty");
        #[cfg(test)]
        let ranking = {
            use gpui_kit::test::TestSupportExt;
            ranking.test_support()
        };
        ui::surface(cx)
            .flex_1()
            .min_w(space::NONE)
            .p(space::XL)
            .gap(space::SM)
            .child(
                h_flex()
                    .justify_between()
                    .gap(space::SM)
                    .child(ui::muted("今日采集流量 · 分类为采样统计", cx))
                    .child(
                        TabBar::new("traffic-scope")
                            .segmented()
                            .small()
                            .children([Tab::new().label("全部"), Tab::new().label("仅代理")])
                            .selected_index(usize::from(self.proxy_only))
                            .on_click(cx.listener(|this, index, _, cx| {
                                this.proxy_only = *index == 1;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .h(layout::OVERVIEW_HISTORY_HEIGHT)
                    .flex_shrink_0()
                    .child(plot),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(ui::muted("00:00", cx).text_size(text::CAPTION))
                    .child(ui::muted("12:00", cx).text_size(text::CAPTION))
                    .child(ui::muted("24:00", cx).text_size(text::CAPTION)),
            )
            .child(
                TabBar::new("traffic-breakdown")
                    .segmented()
                    .small()
                    .w_full()
                    .children([
                        Tab::new().label("进程").flex_1(),
                        Tab::new().label("域名").flex_1(),
                        Tab::new().label("策略").flex_1(),
                    ])
                    .selected_index(self.breakdown)
                    .on_click(cx.listener(|this, index, _, cx| {
                        this.breakdown = *index;
                        cx.notify();
                    })),
            )
            .child(
                ranking
                    .min_h(layout::OVERVIEW_RANKING_HEIGHT)
                    .flex_1()
                    .gap(space::SM)
                    .when(ranks.is_empty(), |view| {
                        view.items_center().justify_center().child(ui::muted(
                            ["暂无进程流量排行", "暂无域名流量排行", "暂无策略流量排行"]
                                [self.breakdown],
                            cx,
                        ))
                    })
                    .when(!ranks.is_empty(), |view| {
                        let height = layout::CONTROL_HEIGHT.to_pixels(window.rem_size());
                        let count = ranks.len();
                        let ranks = std::rc::Rc::new(ranks);
                        view.child(div().h(layout::OVERVIEW_RANKING_HEIGHT).w_full().child(
                            ui::virtual_rows::rows(
                                cx.entity(),
                                "traffic-ranking-rows",
                                vec![height; count],
                                &self.ranking_scroll,
                                move |_, index, _, cx| {
                                    let (name, amount) = &ranks[index];
                                    h_flex()
                                        .h_full()
                                        .gap(space::SM)
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(space::NONE)
                                                .truncate()
                                                .child(name.clone()),
                                        )
                                        .child(ui::muted(bytes(*amount), cx))
                                        .into_any_element()
                                },
                            ),
                        ))
                    })
                    .child(
                        ui::muted(
                            format!(
                                "未分类 {} · 采集中断 {} 次",
                                bytes(statistics.today.unclassified),
                                statistics.gaps
                            ),
                            cx,
                        )
                        .text_size(text::CAPTION),
                    ),
            )
    }
}

impl Render for Overview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.session.read(cx);
        let profile = state.profile_name().unwrap_or("未选择配置").to_owned();
        let runtime = state.runtime();
        let node = runtime
            .mode
            .and_then(|mode| runtime.proxies.current_node(mode))
            .unwrap_or("—")
            .to_owned();
        let quick_test = runtime.mode.and_then(|mode| {
            runtime
                .proxies
                .current_test(mode, runtime.generation, runtime.epoch)
        });
        ui::workspace()
            .child(ui::workspace_header("仪表盘", "", cx))
            .child(
                ui::workspace_body()
                    .id("overview-scroll")
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            .gap(space::LG)
                            .child(
                                h_flex()
                                    .gap(space::SECTION)
                                    .pb(space::SM)
                                    .child(Self::context("配置", profile, cx))
                                    .child(
                                        h_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .gap(space::SM)
                                            .child(Self::context("策略节点", node, cx))
                                            .child({
                                                use gpui_kit::component::button::{
                                                    Button, ButtonVariants,
                                                };
                                                Button::new("current-node-test")
                                                    .ghost()
                                                    .small()
                                                    .icon(gpui_kit::assets::IconName::Gauge)
                                                    .accessibility_label("测试当前节点延迟")
                                                    .tooltip("测试当前节点延迟")
                                                    .disabled(quick_test.is_none())
                                                    .on_click(move |_, window, cx| {
                                                        if let Some(test) = quick_test.clone() {
                                                            crate::core::test_delay(
                                                                test, window, cx,
                                                            );
                                                        }
                                                    })
                                            }),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .gap(space::SM)
                                            .child(
                                                h_flex()
                                                    .gap(space::XS)
                                                    .child(
                                                        gpui_kit::component::switch::Switch::new(
                                                            "ip-auto-refresh",
                                                        )
                                                        .small()
                                                        .checked(self.ip_auto_refresh)
                                                        .accessibility_label("自动刷新出口 IP")
                                                        .on_change(cx.listener(
                                                            |this, enabled, _, cx| {
                                                                this.ip_auto_refresh = *enabled;
                                                                crate::core::ip_auto_refresh(
                                                                    *enabled, cx,
                                                                );
                                                                cx.notify();
                                                            },
                                                        )),
                                                    )
                                                    .child(
                                                        ui::muted("外部 IP", cx)
                                                            .text_size(text::CAPTION),
                                                    )
                                                    .child(div().flex_1())
                                                    .child(
                                                        ui::icon_button(
                                                            "ip-visibility",
                                                            if self.show_ip {
                                                                gpui_kit::assets::IconName::EyeOff
                                                            } else {
                                                                gpui_kit::assets::IconName::Eye
                                                            },
                                                            if self.show_ip {
                                                                "隐藏 IP"
                                                            } else {
                                                                "显示 IP"
                                                            },
                                                        )
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.show_ip = !this.show_ip;
                                                            cx.notify();
                                                        })),
                                                    )
                                                    .child(
                                                        ui::icon_button(
                                                            "ip-refresh",
                                                            gpui_kit::assets::IconName::RefreshCw,
                                                            "刷新出口 IP",
                                                        )
                                                        .loading(runtime.ip_loading)
                                                        .disabled(!state.is_running())
                                                        .on_click(|_, window, cx| {
                                                            crate::core::refresh_ip(window, cx)
                                                        }),
                                                    ),
                                            )
                                            .child(if self.show_ip {
                                                runtime
                                                    .external_ip
                                                    .clone()
                                                    .unwrap_or_else(|| "—".into())
                                            } else {
                                                "••••••••".into()
                                            })
                                            .when_some(runtime.ip_info.as_ref(), |view, info| {
                                                view.child(ui::muted(
                                                    format!("{} {}", info.flag(), info.country),
                                                    cx,
                                                ))
                                                .child(ui::muted(info.location(), cx))
                                                .child(ui::muted(
                                                    format!(
                                                        "{} {}",
                                                        info.asn
                                                            .map(|asn| format!("AS{asn}"))
                                                            .unwrap_or_default(),
                                                        info.asn_organization
                                                    ),
                                                    cx,
                                                ))
                                                .child(ui::muted(info.organization.clone(), cx))
                                                .when_some(
                                                    info.latitude_microdegrees
                                                        .zip(info.longitude_microdegrees),
                                                    |view, (latitude, longitude)| {
                                                        view.child(ui::muted(
                                                            format!(
                                                                "{:.2}, {:.2}",
                                                                longitude as f64 / 1_000_000.,
                                                                latitude as f64 / 1_000_000.
                                                            ),
                                                            cx,
                                                        ))
                                                    },
                                                )
                                            }),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .gap(space::SM)
                                            .items_start()
                                            .child(
                                                ui::muted("出站模式", cx).text_size(text::CAPTION),
                                            )
                                            .child(ui::mode_tabs(
                                                &self.session,
                                                "overview-mode",
                                                cx,
                                            )),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_stretch()
                                    .gap(space::LG)
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .child(self.latency_card(cx).w_full()),
                                    )
                                    .child(
                                        h_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .gap(space::LG)
                                            .child(self.rate_card(true, cx))
                                            .child(self.rate_card(false, cx)),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_stretch()
                                    .gap(space::LG)
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .gap(space::LG)
                                            .child(self.connections_card(cx))
                                            .child(self.totals_card(cx)),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .child(self.traffic_card(window, cx).w_full()),
                                    ),
                            ),
                    ),
            )
    }
}
