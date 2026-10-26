use super::Overview;
use crate::ui;
use crate::ui::theme::{self, layout, opacity, space, stroke, text};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable, h_flex,
        scroll::ScrollableElement,
        tab::{Tab, TabBar},
        v_flex,
    },
    *,
};

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
                "—",
                "",
                cx,
            )))
            .child(
                v_flex()
                    .flex_1()
                    .min_h(space::NONE)
                    .px(space::XL)
                    .pb(space::LG)
                    .gap(space::SM)
                    .child(
                        Self::empty_plot(
                            if upload {
                                "upload-empty-plot"
                            } else {
                                "download-empty-plot"
                            },
                            "暂无速率记录",
                            cx,
                        )
                        .flex_1(),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .child(ui::muted("较早", cx).text_size(text::CAPTION))
                            .child(ui::muted("现在", cx).text_size(text::CAPTION)),
                    ),
            )
    }
    fn latency_card(&self, cx: &App) -> Div {
        ui::surface(cx)
            .flex_1()
            .min_w(space::NONE)
            .h(layout::OVERVIEW_TOP_HEIGHT)
            .p(space::XL)
            .justify_between()
            .child(Self::value("INTERNET 延迟", "—", "", cx))
            .child(h_flex().gap(space::LG).children(
                ["路由", "DNS", "代理"].map(|label| Self::context(label, "—".into(), cx)),
            ))
    }
    fn connections_card(&self, cx: &App) -> Div {
        ui::surface(cx)
            .h(layout::OVERVIEW_SUMMARY_HEIGHT)
            .p(space::XL)
            .justify_between()
            .child(Self::value("活动连接", "—", "", cx))
            .child(h_flex().gap(space::LG).children(
                ["进程", "代理连接", "直连"].map(|label| Self::context(label, "—".into(), cx)),
            ))
    }
    fn totals_card(&self, cx: &mut Context<Self>) -> Div {
        ui::surface(cx)
            .h(layout::OVERVIEW_SUMMARY_HEIGHT)
            .p(space::XL)
            .justify_between()
            .child(
                h_flex()
                    .items_start()
                    .justify_between()
                    .gap(space::SM)
                    .child(Self::value("总计", "—", "", cx))
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
                    .child(ui::muted("DIRECT  —", cx))
                    .child(ui::muted("代理  —", cx)),
            )
    }
    fn traffic_card(&self, cx: &mut Context<Self>) -> Div {
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
                    .child(ui::muted("流量", cx))
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
                    .child(Self::empty_plot(
                        "hourly-empty-plot",
                        "暂无小时流量记录",
                        cx,
                    )),
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
                    .items_center()
                    .justify_center()
                    .gap(space::SM)
                    .child(ui::muted(
                        ["暂无进程流量排行", "暂无域名流量排行", "暂无策略流量排行"]
                            [self.breakdown],
                        cx,
                    ))
                    .child(ui::muted("产生流量后显示统计结果", cx).text_size(text::CAPTION)),
            )
    }
}

impl Render for Overview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.session.read(cx);
        let profile = state.profile_name().unwrap_or("未选择配置").to_owned();
        let node = state.selected_node("节点选择").unwrap_or("—").to_owned();
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
                                    .child(Self::context("当前节点", node, cx))
                                    .child(Self::context("外部 IP", "—".into(), cx))
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
                                            .child(self.traffic_card(cx).w_full()),
                                    ),
                            ),
                    ),
            )
    }
}
