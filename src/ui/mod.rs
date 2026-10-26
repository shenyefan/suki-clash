use self::theme::{control, layout, opacity, space, stroke, text};
use crate::session::{ProxyMode, Session};
use gpui_kit::component::tab::{Tab, TabBar};
pub(crate) mod editor;
mod modal;
pub(crate) mod motion;
pub(crate) use modal::{modal, modal_alert, modal_close, modal_content, modal_panel, modal_scroll};
pub(crate) mod theme;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme, Icon, Sizable, WindowExt,
        button::{Button, ButtonVariants},
        h_flex, v_flex,
    },
    *,
};
pub(crate) use theme::{configure_theme, window_tint, workspace_tint};

/// All major regions share this inset, baseline and control density.
pub(crate) fn page_header(title: &str, icon: IconName, cx: &App) -> Div {
    h_flex()
        .h(layout::ROW_HEIGHT)
        .flex_shrink_0()
        .px(space::LG)
        .gap(space::SM)
        .border_b(stroke::HAIRLINE)
        .border_color(cx.theme().border)
        .child(
            Icon::new(icon)
                .size(layout::ICON)
                .text_color(cx.theme().muted_foreground),
        )
        .child(div().font_weight(text::HEADING).child(title.to_owned()))
        .child(div().flex_1())
}
pub(crate) fn section(title: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .h(layout::CONTROL_HEIGHT)
        .flex_shrink_0()
        .font_weight(text::EMPHASIS)
        .text_color(cx.theme().foreground)
        .child(title.into())
}
pub(crate) fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_size(text::BODY)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
pub(crate) fn icon_button(id: impl Into<ElementId>, icon: IconName, label: &str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon)
        .tooltip(label.to_owned())
        .accessibility_label(label.to_owned())
}

/// Quiet chrome command: native toggle semantics, color reflects enabled state.
pub(crate) fn status_toggle(
    id: &'static str,
    icon: IconName,
    label: &str,
    enabled: Option<bool>,
    cx: &App,
) -> Button {
    icon_button(
        id,
        icon,
        &match enabled {
            Some(true) => format!("关闭{label}"),
            Some(false) => format!("开启{label}"),
            None => format!("{label}：未获取实际状态"),
        },
    )
    .flex_shrink_0()
    .toggled(enabled == Some(true))
    .text_color(if enabled == Some(true) {
        cx.theme().primary
    } else {
        cx.theme().muted_foreground
    })
    .accessibility_label(format!(
        "{label}：{}",
        match enabled {
            Some(true) => "已开启",
            Some(false) => "已关闭",
            None => "未知",
        }
    ))
}

pub(crate) fn empty(icon: IconName, heading: &str, description: &str, cx: &App) -> Div {
    v_flex()
        .items_center()
        .justify_center()
        .gap(space::SM)
        .py(space::REGION)
        .size_full()
        .min_h(layout::EMPTY_STATE_HEIGHT)
        .px(space::LG)
        .child(
            Icon::new(icon)
                .size(layout::ICON_FEATURE)
                .text_color(cx.theme().muted_foreground),
        )
        .child(
            div()
                .text_center()
                .font_weight(text::EMPHASIS)
                .child(heading.to_owned()),
        )
        .child(muted(description.to_owned(), cx).text_center())
}
pub(crate) fn detail_rows(details: &str, cx: &App) -> Div {
    v_flex()
        .gap(space::XS)
        .children(details.lines().filter(|line| !line.is_empty()).map(|line| {
            if let Some((key, value)) = line.split_once('\t') {
                v_flex()
                    .gap(space::XS)
                    .py(space::SM)
                    .border_b(stroke::HAIRLINE)
                    .border_color(cx.theme().border)
                    .child(muted(key.to_owned(), cx).text_size(text::CAPTION))
                    .child(div().child(value.to_owned()))
                    .into_any_element()
            } else {
                div()
                    .py(space::SM)
                    .text_color(cx.theme().muted_foreground)
                    .child(line.to_owned())
                    .into_any_element()
            }
        }))
}
/// All validation failures share the native bottom-right notification stack.
pub(crate) fn show_error(message: String, window: &mut Window, cx: &mut App) {
    window.push_notification(
        gpui_kit::component::notification::Notification::error(message),
        cx,
    );
}

pub(crate) fn request_command(action: &str, window: &mut Window, cx: &mut App) {
    crate::integration::request(action, window, cx);
}
pub(crate) fn detail_dialog(
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) {
    let title = title.into();
    let body = body.into();
    window.open_dialog(cx, move |dialog, window, cx| {
        let body = body.clone();
        modal(dialog, title.clone(), layout::DIALOG_WIDTH, window, cx)
            .content(modal_content(
                "detail-scroll",
                crate::ui::theme::layout::DIALOG_FORM_HEIGHT,
                move |_, cx| {
                    if body.trim().is_empty() {
                        empty(IconName::ScrollText, "暂无记录", "有记录后会显示在这里", cx)
                            .into_any_element()
                    } else {
                        detail_rows(&body, cx).into_any_element()
                    }
                },
            ))
            .footer(modal_close("关闭"))
    })
}

pub(crate) fn field_label(label: &str, control: impl IntoElement) -> Div {
    v_flex()
        .gap(space::XS)
        .child(div().child(label.to_owned()))
        .child(control)
}
pub(crate) fn dialog_footer(commit: &str) -> impl IntoElement {
    use gpui_kit::component::dialog::{DialogAction, DialogFooter};
    DialogFooter::new()
        .child(
            Button::new("dialog-cancel")
                .with_size(control::STANDARD)
                .label("取消")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
        .child(
            div().w(layout::VALUE_WIDTH).flex_shrink_0().child(
                DialogAction::new().child(
                    Button::new("dialog-commit")
                        .with_size(control::STANDARD)
                        .primary()
                        .label(commit.to_owned()),
                ),
            ),
        )
}

/// Shared page frame: fixed heading/toolbar/footer, with the body owning scrolling.
pub(crate) fn workspace() -> Div {
    v_flex()
        .size_full()
        .min_w(space::NONE)
        .min_h(space::NONE)
        .px(space::SECTION)
        .pt(space::MD)
        .pb(space::LG)
        .gap(space::LG)
}

pub(crate) fn workspace_toolbar() -> Div {
    h_flex().w_full().flex_shrink_0().gap(space::SM)
}

pub(crate) fn workspace_body() -> Div {
    div().flex_1().min_w(space::NONE).min_h(space::NONE)
}

pub(crate) fn workspace_footer() -> Div {
    h_flex()
        .min_h(layout::FOOTER_HEIGHT)
        .flex_shrink_0()
        .gap(space::SM)
        .text_size(text::CAPTION)
}

/// Shared title geometry for dashboard, proxies, profiles and traffic.
pub(crate) fn workspace_header(title: &str, detail: &str, cx: &App) -> Div {
    h_flex()
        .min_h(layout::TITLE_HEIGHT)
        .gap(space::MD)
        .flex_shrink_0()
        .child(
            div()
                .text_size(text::TITLE)
                .font_weight(text::HEADING)
                .child(title.to_owned()),
        )
        .child(muted(detail.to_owned(), cx).text_size(text::CAPTION))
        .child(div().flex_1())
}

pub(crate) fn surface(cx: &App) -> Div {
    v_flex()
        .rounded(cx.theme().radius_lg)
        .bg(theme::dashboard_tint(cx).opacity(opacity::DASHBOARD))
}

pub(crate) fn mode_tabs(session: &Entity<Session>, id: &'static str, cx: &App) -> impl IntoElement {
    use gpui_kit::prelude::FluentBuilder as _;
    let current = session.read(cx).mode();
    TabBar::new(id)
        .segmented()
        .small()
        .children([
            Tab::new().label("规则"),
            Tab::new().label("全局"),
            Tab::new().label("直连"),
        ])
        .map(|tabs| match current {
            Some(mode) => tabs.selected_index(match mode {
                ProxyMode::Rule => 0,
                ProxyMode::Global => 1,
                ProxyMode::Direct => 2,
            }),
            None => tabs,
        })
        .on_click(move |_, window, cx| request_command("切换出站模式", window, cx))
}

/// Native preferences geometry shared by the core and external-resource pages.
pub(crate) fn preference_row(
    label: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    use gpui_kit::prelude::FluentBuilder as _;
    let description = description.into();
    h_flex()
        .min_h(theme::preferences::ROW_HEIGHT)
        .px(space::LG)
        .py(space::MD)
        .gap(space::LG)
        .child(
            v_flex()
                .flex_1()
                .min_w(space::NONE)
                .gap(space::XS)
                .child(label.into())
                .when(!description.is_empty(), |label| {
                    label.child(muted(description, cx).text_size(text::CAPTION))
                }),
        )
        .child(control)
}

pub(crate) fn preferences_group(
    title: impl Into<SharedString>,
    rows: Vec<AnyElement>,
    cx: &App,
) -> Div {
    let mut content = v_flex()
        .rounded(cx.theme().radius_lg)
        .bg(theme::preferences_tint(cx));
    for (ix, row) in rows.into_iter().enumerate() {
        if ix > 0 {
            content = content.child(
                div()
                    .mx(space::LG)
                    .h(stroke::HAIRLINE)
                    .bg(cx.theme().foreground.opacity(opacity::PREFERENCES_DIVIDER)),
            );
        }
        content = content.child(row);
    }
    v_flex()
        .gap(space::SM)
        .child(section(title, cx).px(space::LG))
        .child(content)
}

pub(crate) fn bottom_details(cx: &App) -> Div {
    v_flex()
        .h(layout::CONNECTION_DETAILS_HEIGHT)
        .flex_shrink_0()
        .min_w(space::NONE)
        .border_t(stroke::HAIRLINE)
        .border_color(cx.theme().border.opacity(opacity::PANEL))
        .pt(space::MD)
        .gap(space::MD)
}

pub(crate) const RULE_TYPES: &[&str] = &[
    "DOMAIN",
    "DOMAIN-SUFFIX",
    "DOMAIN-KEYWORD",
    "DOMAIN-REGEX",
    "GEOSITE",
    "GEOIP",
    "SRC-GEOIP",
    "IP-ASN",
    "SRC-IP-ASN",
    "IP-CIDR",
    "IP-CIDR6",
    "SRC-IP-CIDR",
    "IP-SUFFIX",
    "SRC-IP-SUFFIX",
    "SRC-PORT",
    "DST-PORT",
    "IN-PORT",
    "DSCP",
    "PROCESS-NAME",
    "PROCESS-PATH",
    "PROCESS-NAME-REGEX",
    "PROCESS-PATH-REGEX",
    "NETWORK",
    "UID",
    "IN-TYPE",
    "IN-USER",
    "IN-NAME",
    "SUB-RULE",
    "RULE-SET",
    "AND",
    "OR",
    "NOT",
    "MATCH",
];
