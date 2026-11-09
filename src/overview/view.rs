use super::Overview;
use crate::ui;
use crate::ui::theme::{self, layout, opacity, space, stroke, text};
use gpui_kit::{
    base::plot::AxisLabelPlacement,
    component::{
        ActiveTheme, Disableable, Sizable,
        chart::{BarChart, LineChart},
        h_flex,
        progress::Progress,
        scroll::ScrollableElement,
        tab::{Tab, TabBar},
        v_flex,
    },
    *,
};

fn bytes(value: u64) -> String {
    let (number, unit) = split_bytes(value);
    format!("{number} {unit}")
}

fn split_bytes(value: u64) -> (String, &'static str) {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut number = value as f64;
    let mut unit = 0;
    while number >= 1024. && unit < units.len() - 1 {
        number /= 1024.;
        unit += 1;
    }
    (format!("{number:.1}"), units[unit])
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
                    .child(
                        div()
                            .text_size(text::METRIC)
                            .font_weight(text::EMPHASIS)
                            .child(unit.to_owned()),
                    ),
            )
    }

    fn context(label: &str, value: String, label_size: Rems, cx: &App) -> Div {
        v_flex()
            .flex_1()
            .min_w(space::NONE)
            .gap(space::SM)
            .child(ui::muted(label.to_owned(), cx).text_size(label_size))
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
        let (value, unit) = rate
            .map(|rate| {
                let (number, unit) = split_bytes(rate);
                (number, format!("{unit}/s"))
            })
            .unwrap_or_else(|| ("—".into(), String::new()));
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
                &unit,
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
    fn latency_card(
        &self,
        node_latency: String,
        test: Option<crate::proxies::runtime::Test>,
        cx: &App,
    ) -> Div {
        let runtime = self.session.read(cx).runtime();
        let internet = runtime
            .latency
            .map(|ms| (ms.to_string(), "ms"))
            .unwrap_or_else(|| ("—".into(), ""));
        ui::surface(cx)
            .flex_1()
            .min_w(space::NONE)
            .h(layout::OVERVIEW_TOP_HEIGHT)
            .p(space::XL)
            .justify_between()
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .child(Self::value("网络延迟", &internet.0, internet.1, cx))
                    .child({
                        use gpui_kit::component::button::{Button, ButtonVariants};
                        Button::new("current-node-test")
                            .ghost()
                            .small()
                            .icon(assets::IconName::Gauge)
                            .accessibility_label("测试节点延迟")
                            .tooltip("测试节点延迟")
                            .disabled(test.is_none())
                            .on_click(move |_, window, cx| {
                                if let Some(test) = test.clone() {
                                    crate::core::test_delay(test, window, cx);
                                }
                            })
                    }),
            )
            .child(
                h_flex()
                    .gap(space::SECTION)
                    .child(Self::context("节点", node_latency, text::CAPTION, cx))
                    .children(self.connection_contexts(cx)),
            )
    }

    fn connection_contexts(&self, cx: &App) -> [Div; 2] {
        let runtime = self.session.read(cx).runtime();
        let count = runtime
            .statistics
            .connections
            .map(|count| count.to_string())
            .unwrap_or_else(|| "—".into());
        let memory = runtime
            .memory
            .as_ref()
            .map(|memory| bytes(memory.inuse))
            .unwrap_or_else(|| "—".into());
        [
            Self::context("活动连接", count, text::CAPTION, cx),
            Self::context("内核内存", memory, text::CAPTION, cx),
        ]
    }
    fn totals(&self, cx: &mut Context<Self>) -> Div {
        let runtime = self.session.read(cx).runtime();
        let available = runtime.history.available;
        let period = if self.month {
            runtime.history.month
        } else {
            runtime.history.today
        };
        let amount = period.bytes(self.proxy_only);
        let (up, down) = (amount.upload, amount.download);
        let total = up.saturating_add(down);
        let share = if total == 0 {
            0.5
        } else {
            up as f32 / total as f32
        };
        let show = |value: u64| {
            if available {
                bytes(value)
            } else {
                "—".into()
            }
        };
        let (total_value, total_unit) = if available {
            let (number, unit) = split_bytes(total);
            (number, unit.to_owned())
        } else {
            ("—".into(), String::new())
        };
        v_flex()
            .w_full()
            .h_full()
            .justify_between()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap(space::SM)
                    .child(ui::muted(
                        if self.proxy_only {
                            "累计流量 · 仅代理"
                        } else {
                            "累计流量"
                        },
                        cx,
                    ))
                    .child(
                        TabBar::new("traffic-period")
                            .segmented()
                            .small()
                            .children([Tab::new().label("今日"), Tab::new().label("本月")])
                            .selected_index(usize::from(self.month))
                            .on_click(cx.listener(|this, index, _, cx| {
                                this.month = *index == 1;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .items_baseline()
                    .gap(space::XS)
                    .text_size(text::METRIC)
                    .font_weight(text::EMPHASIS)
                    .child(total_value)
                    .child(total_unit),
            )
            .child(
                v_flex()
                    .gap(space::SM)
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                v_flex()
                                    .child(ui::muted("上传", cx).text_size(text::CAPTION))
                                    .child(
                                        div()
                                            .font_weight(text::EMPHASIS)
                                            .text_color(theme::upload_color(cx))
                                            .child(show(up)),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .items_end()
                                    .child(ui::muted("下载", cx).text_size(text::CAPTION))
                                    .child(
                                        div()
                                            .font_weight(text::EMPHASIS)
                                            .text_color(theme::download_color(cx))
                                            .child(show(down)),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .h(layout::OVERVIEW_TOTAL_BAR_HEIGHT)
                            .gap(space::XS)
                            .child(div().h_full().w(relative(share)).rounded_full().bg(
                                if available && total > 0 {
                                    theme::upload_color(cx)
                                } else {
                                    cx.theme().muted
                                },
                            ))
                            .child(div().h_full().flex_1().rounded_full().bg(
                                if available && total > 0 {
                                    theme::download_color(cx)
                                } else {
                                    cx.theme().muted
                                },
                            )),
                    ),
            )
    }
    fn history_card(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let runtime = self.session.read(cx).runtime();
        let proxy_only = self.proxy_only;
        let points: Vec<_> = runtime.history.hours.iter().copied().enumerate().collect();
        let color = theme::download_color(cx);
        let plot = BarChart::new(points)
            .id("hourly-traffic-chart")
            .appear(false)
            .band(|(hour, _): &(usize, super::history::Hour)| format!("{hour:02}:00"))
            .value(move |(_, hour): &(usize, super::history::Hour)| {
                hour.bytes(proxy_only).total() as f64
            })
            .fill(move |_, _, _, _| color)
            .tick_margin(6)
            .value_axis(true)
            .value_axis_label_placement(AxisLabelPlacement::Inside)
            .value_tick_count(3)
            .value_tick_format(|value| bytes(value.max(0.) as u64))
            .grid(true)
            .grid_dashed(true)
            .max_band_width(layout::OVERVIEW_HOURLY_BAR_WIDTH.to_pixels(window.rem_size()))
            .min_length(
                layout::OVERVIEW_HOURLY_BAR_WIDTH
                    .to_pixels(window.rem_size())
                    .as_f32(),
            )
            .corner_radii(layout::OVERVIEW_HOURLY_BAR_RADIUS.to_pixels(window.rem_size()))
            .tooltip_title(|(hour, _)| format!("{hour:02}:00–{:02}:00", hour + 1).into())
            .tooltip_value(move |(_, hour), _| {
                if hour.samples == 0 {
                    "未记录".into()
                } else {
                    format!(
                        "上传 {} · 下载 {}",
                        bytes(hour.bytes(proxy_only).upload),
                        bytes(hour.bytes(proxy_only).download)
                    )
                    .into()
                }
            })
            .name(if proxy_only { "仅代理" } else { "全部" });
        let frame = div()
            .id("hourly-traffic-plot")
            .flex_1()
            .min_h(space::NONE)
            .w_full();
        #[cfg(test)]
        let frame = {
            use gpui_kit::test::TestSupportExt;
            frame.test_support()
        };
        ui::surface(cx)
            .w_full()
            .h(layout::OVERVIEW_TOP_HEIGHT)
            .p(space::XL)
            .gap(space::LG)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap(space::SM)
                    .child(ui::muted("流量", cx))
                    .child(
                        TabBar::new("traffic-scope")
                            .segmented()
                            .small()
                            .children([Tab::new().label("全部"), Tab::new().label("仅代理")])
                            .selected_index(usize::from(proxy_only))
                            .on_click(cx.listener(|this, index, _, cx| {
                                this.proxy_only = *index == 1;
                                cx.notify();
                            })),
                    ),
            )
            .child(frame.child(plot))
    }
    fn rankings_card(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let runtime = self.session.read(cx).runtime();
        let ranks = runtime
            .statistics
            .rankings
            .get(self.breakdown)
            .cloned()
            .unwrap_or_default();
        let height = layout::OVERVIEW_RANKING_ROW_HEIGHT.to_pixels(window.rem_size());
        let count = ranks.len();
        let ranks = std::rc::Rc::new(ranks);
        ui::surface(cx)
            .w_full()
            .h(layout::OVERVIEW_DETAILS_HEIGHT)
            .p(space::XL)
            .gap(space::LG)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(ui::muted("流量排行", cx))
                    .child(
                        TabBar::new("traffic-breakdown")
                            .segmented()
                            .small()
                            .children([
                                Tab::new().label("域名"),
                                Tab::new().label("进程"),
                                Tab::new().label("代理"),
                            ])
                            .selected_index(self.breakdown)
                            .on_click(cx.listener(|this, index, _, cx| {
                                this.breakdown = *index;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h(space::NONE)
                    .w_full()
                    .justify_center()
                    .child(if count == 0 {
                        ui::empty_inline(
                            assets::IconName::ChartNoAxesColumn,
                            "暂无流量记录",
                            "",
                            cx,
                        )
                        .into_any_element()
                    } else {
                        ui::virtual_rows::rows(
                            cx.entity(),
                            "traffic-ranking-rows",
                            vec![height; count],
                            &self.ranking_scroll,
                            move |_, index, _, cx| {
                                let (name, amount) = &ranks[index];
                                v_flex()
                                    .h_full()
                                    .justify_center()
                                    .gap(space::XS)
                                    .child(
                                        h_flex()
                                            .gap(space::SM)
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(space::NONE)
                                                    .truncate()
                                                    .child(name.clone()),
                                            )
                                            .child(ui::muted(bytes(*amount), cx)),
                                    )
                                    .child(
                                        Progress::new(SharedString::from(format!(
                                            "ranking-{name}"
                                        )))
                                        .small()
                                        .value(if ranks.first().is_some_and(|entry| entry.1 > 0) {
                                            (*amount as f64 / ranks[0].1 as f64 * 100.) as f32
                                        } else {
                                            0.
                                        })
                                        .color(cx.theme().muted_foreground)
                                        .accessibility_label(format!("{name}流量占比")),
                                    )
                                    .into_any_element()
                            },
                        )
                        .into_any_element()
                    }),
            )
    }
    fn ip_details(&self, cx: &App) -> AnyElement {
        let runtime = self.session.read(cx).runtime();
        let card = ui::surface(cx).id("overview-ip-details");
        #[cfg(test)]
        let card = {
            use gpui_kit::test::TestSupportExt;
            card.test_support()
        };
        let mut card = card
            .w_full()
            .h(layout::OVERVIEW_DETAILS_HEIGHT)
            .p(space::XL)
            .gap(space::LG)
            .child("IP 信息");
        if let Some(info) = &runtime.ip_info {
            let network = format!(
                "{} {}",
                info.asn.map(|asn| format!("AS{asn}")).unwrap_or_default(),
                info.asn_organization
            );
            for (key, label, value) in [
                (
                    "overview-ip-country",
                    "国家地区",
                    format!("{} {}", info.flag(), info.country),
                ),
                ("overview-ip-city", "城市", info.location()),
                ("overview-ip-network", "网络组织", network),
                ("overview-ip-isp", "运营商", info.organization.clone()),
                (
                    "overview-ip-timezone",
                    "时区",
                    if info.timezone.is_empty() {
                        "—".into()
                    } else {
                        info.timezone.clone()
                    },
                ),
                (
                    "overview-ip-coordinates",
                    "经纬度",
                    info.longitude_microdegrees
                        .zip(info.latitude_microdegrees)
                        .map(|(longitude, latitude)| {
                            format!(
                                "{:.4}, {:.4}",
                                f64::from(longitude) / 1_000_000.,
                                f64::from(latitude) / 1_000_000.
                            )
                        })
                        .unwrap_or_else(|| "—".into()),
                ),
            ] {
                if !value.trim().is_empty() {
                    let row = h_flex()
                        .id(key)
                        .gap(space::MD)
                        .child(ui::muted(label, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w(space::NONE)
                                .truncate()
                                .text_right()
                                .child(value.trim().to_owned()),
                        );
                    #[cfg(test)]
                    let row = {
                        use gpui_kit::test::TestSupportExt;
                        row.test_support()
                    };
                    card = card.child(row);
                }
            }
        } else {
            card = card.child(ui::muted("暂无 IP 信息", cx));
        }
        card.into_any_element()
    }
}

impl Render for Overview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.session.read(cx);
        let profile = state.profile_name().unwrap_or("未选择配置").to_owned();
        let runtime = state.runtime();
        let profile_id = state.active_profile_id();
        let preferences = state.core().proxy_preferences.profile(profile_id);
        let group = runtime
            .mode
            .and_then(|mode| preferences.group(&runtime.proxies, mode))
            .map(str::to_owned);
        let node = if runtime.mode == Some(crate::session::ProxyMode::Direct) {
            "DIRECT"
        } else {
            group
                .as_deref()
                .and_then(|group| runtime.proxies.current_node_in(group))
                .unwrap_or("—")
        }
        .to_owned();
        let quick_test = group.as_deref().and_then(|group| {
            let node = runtime.proxies.selected(group)?;
            runtime
                .proxies
                .testable(group, node)
                .then(|| crate::proxies::runtime::Test {
                    group: Some(group.to_owned()),
                    node: Some(node.to_owned()),
                    urls: preferences.test_urls.clone(),
                    generation: runtime.generation,
                    epoch: runtime.epoch,
                })
        });
        let (latency, latency_unit) = quick_test
            .as_ref()
            .and_then(|test| {
                let group = test.group.as_ref()?;
                let node = test.node.as_ref()?;
                let member = runtime
                    .proxies
                    .members
                    .get(group)?
                    .iter()
                    .find(|member| &member.name == node)?;
                let key = (
                    group.clone(),
                    runtime.proxies.measurement_key(group, member),
                );
                if runtime.proxies.testing.contains(&key) {
                    return Some(("测量中".into(), ""));
                }
                let result = runtime
                    .proxies
                    .measurements
                    .get(&key)
                    .cloned()
                    .or_else(|| {
                        runtime
                            .proxies
                            .member_proxy(member)?
                            .history
                            .last()
                            .map(|entry| Ok(entry.delay))
                    })?;
                let timeout = crate::integration::read("settings", cx)
                    .get("DelayTestTimeout")
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(5000);
                Some(match result {
                    Ok(delay) if delay > 0 && delay < timeout => (delay.to_string(), "ms"),
                    Ok(_) | Err(crate::proxies::runtime::MeasurementError::Timeout) => {
                        ("超时".into(), "")
                    }
                    Err(_) => ("失败".into(), ""),
                })
            })
            .unwrap_or_else(|| ("—".into(), ""));
        let ip_header = v_flex()
            .flex_1()
            .min_w(space::NONE)
            .gap(space::SM)
            .child(
                h_flex()
                    .gap(space::XS)
                    .child(ui::muted("外部 IP", cx).text_size(text::BODY)),
            )
            .child(
                div()
                    .truncate()
                    .font_weight(text::EMPHASIS)
                    .child(runtime.external_ip.clone().unwrap_or_else(|| "—".into())),
            );
        let controls = h_flex()
            .w_full()
            .px(space::XL)
            .items_start()
            .gap(space::LG)
            .child(Self::context("配置", profile, text::BODY, cx))
            .child(Self::context("节点", node, text::BODY, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(space::NONE)
                    .gap(space::SM)
                    .child(ui::muted("出站模式", cx).text_size(text::BODY))
                    .child(h_flex().child(ui::mode_tabs(&self.session, "overview-mode", cx))),
            )
            .child(ip_header);
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
                            .child(controls)
                            .child(
                                h_flex()
                                    .items_stretch()
                                    .gap(space::LG)
                                    .child(
                                        v_flex().flex_1().min_w(space::NONE).child(
                                            self.latency_card(
                                                format!(
                                                    "{latency}{}",
                                                    if latency_unit.is_empty() {
                                                        ""
                                                    } else {
                                                        " ms"
                                                    }
                                                ),
                                                quick_test.clone(),
                                                cx,
                                            )
                                            .w_full(),
                                        ),
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
                                            .child(
                                                ui::surface(cx)
                                                    .w_full()
                                                    .h(layout::OVERVIEW_TOP_HEIGHT)
                                                    .p(space::XL)
                                                    .child(self.totals(cx)),
                                            )
                                            .child(self.ip_details(cx)),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(space::NONE)
                                            .gap(space::LG)
                                            .child(self.history_card(_window, cx))
                                            .child(self.rankings_card(_window, cx)),
                                    ),
                            ),
                    ),
            )
    }
}
