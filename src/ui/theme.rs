//! Single theme entry point: edit spacing and geometry below, then the DARK /
//! LIGHT palettes. Views read semantic colors from cx.theme() and use short
//! names such as space::SM, text::BODY and layout::SIDEBAR_WIDTH.
use gpui_kit::{
    component::{ActiveTheme, Theme},
    *,
};

// 1. 共用尺度：间距、文字与几何尺寸。
pub(crate) mod space {
    use super::*;
    pub(crate) const NONE: Rems = rems(0.);
    pub(crate) const XS: Rems = rems(0.25);
    pub(crate) const SM: Rems = rems(0.5);
    pub(crate) const MD: Rems = rems(0.75);
    pub(crate) const LG: Rems = rems(1.);
    pub(crate) const XL: Rems = rems(1.25);
    pub(crate) const SECTION: Rems = rems(1.5);
    pub(crate) const REGION: Rems = rems(2.5);
}

pub(crate) mod text {
    use super::*;
    pub(crate) const BASE: Pixels = px(15.);
    pub(crate) const LINE_HEIGHT: f32 = 1.25;
    pub(crate) const CAPTION: Rems = rems(0.75);
    pub(crate) const BODY: Rems = rems(0.875);
    pub(crate) const TITLE: Rems = rems(1.25);
    pub(crate) const METRIC: Rems = rems(1.875);
    pub(crate) const EMPHASIS: FontWeight = FontWeight::MEDIUM;
    pub(crate) const HEADING: FontWeight = FontWeight::SEMIBOLD;
    pub(crate) const MAC_FONT: &str = ".AppleSystemUIFont";
    pub(crate) const WINDOWS_FONT: &str = "Segoe UI";
}

/// Product geometry. Names include their role and axis so call sites are clear.
pub(crate) mod layout {
    use super::*;
    pub(crate) const ICON_SM: Rems = rems(0.75);
    pub(crate) const ICON: Rems = rems(1.);
    pub(crate) const ICON_LG: Rems = rems(1.25);
    pub(crate) const ICON_FEATURE: Rems = rems(1.5);
    pub(crate) const PROGRESS_HEIGHT: Rems = rems(0.25);
    pub(crate) const FOOTER_HEIGHT: Rems = rems(1.5);
    pub(crate) const DETAIL_MIN_WIDTH: Rems = rems(18.);
    pub(crate) const CONNECTION_DETAILS_HEIGHT: Rems = rems(15.);
    pub(crate) const CONTROL_HEIGHT: Rems = rems(2.);
    pub(crate) const NAV_ITEM_HEIGHT: Rems = rems(2.5);
    pub(crate) const SIDEBAR_GROUP_HEADER_HEIGHT: Rems = rems(2.);
    pub(crate) const TITLE_HEIGHT: Rems = rems(3.5);
    pub(crate) const BRAND_LOGO_SIZE: Rems = rems(1.75);
    pub(crate) const BRAND_HEIGHT: Rems = rems(4.);
    pub(crate) const TEXTAREA_HEIGHT: Rems = rems(5.);
    pub(crate) const SETTINGS_EDITOR_HEIGHT: Rems = rems(16.);
    pub(crate) const OVERVIEW_TRAFFIC_HEIGHT: Rems = rems(29.);
    pub(crate) const OVERVIEW_RANKING_ROW_HEIGHT: Rems = rems(3.);
    pub(crate) const OVERVIEW_TOTAL_BAR_HEIGHT: Rems = rems(0.75);
    pub(crate) const OVERVIEW_TOP_HEIGHT: Rems = rems(14.);
    pub(crate) const EMPTY_STATE_HEIGHT: Rems = rems(18.);
    pub(crate) const EMPTY_INLINE_HEIGHT: Rems = rems(6.);
    pub(crate) const EMPTY_CHART_GRID_LINES: usize = 3;
    pub(crate) const QR_SIZE: Rems = rems(16.);
    pub(crate) const EDITOR_HEIGHT: Rems = rems(24.);
    pub(crate) const DIALOG_COMPACT_HEIGHT: Rems = rems(14.);
    pub(crate) const DIALOG_FORM_HEIGHT: Rems = rems(22.);
    pub(crate) const DIALOG_RESERVED_HEIGHT: Rems = rems(12.);
    pub(crate) const FORM_MAX_HEIGHT: Rems = rems(28.);
    pub(crate) const METRIC_WIDTH: Rems = rems(4.);
    pub(crate) const DETAIL_LABEL_WIDTH: Rems = rems(5.);
    pub(crate) const FILTER_WIDTH: Rems = rems(10.);
    pub(crate) const SIDEBAR_WIDTH: Rems = rems(13.5);
    pub(crate) const LONG_FIELD_WIDTH: Rems = rems(14.);
    pub(crate) const SEARCH_WIDTH: Rems = rems(20.);
    pub(crate) const DIALOG_WIDTH: Rems = rems(32.);
    pub(crate) const EDITOR_DIALOG_WIDTH: Rems = rems(48.);
    pub(crate) const SIDEBAR_COLLAPSED_WIDTH: Rems = METRIC_WIDTH;
}

/// Compact subscription rows keep title, source and traffic visually grouped.
/// Rem values preserve those proportions when the application font is scaled.
pub(crate) mod preferences {
    use super::*;
    pub(crate) const ROW_HEIGHT: Rems = rems(3.);
    pub(crate) const URL_WIDTH: Rems = rems(22.);
}

pub(crate) mod profile {
    use super::*;
    pub(crate) const MIN_WIDTH: Rems = rems(260. / 15.);
    pub(crate) const MIN_HEIGHT: Rems = rems(6.);
    pub(crate) const HEADER_HEIGHT: Rems = rems(1.5);
    pub(crate) const SOURCE_HEIGHT: Rems = rems(1.25);
    pub(crate) const ROW_HEIGHT: Rems = rems(26. / 15.);
    pub(crate) const PADDING_X: Rems = rems(16. / 15.);
    pub(crate) const PADDING_Y: Rems = rems(8. / 15.);
    pub(crate) const GAP: Rems = rems(8. / 15.);
    pub(crate) const NAME_SIZE: Rems = text::BODY;
    pub(crate) const META_SIZE: Rems = text::CAPTION;
}

/// Node cards share the subscription width rhythm and adapt to available space.
pub(crate) mod proxy_card {
    use super::*;
    pub(crate) const MIN_WIDTH: Rems = profile::MIN_WIDTH;
    pub(crate) const GAP: Rems = space::SM;
    pub(crate) const HEIGHT: Rems = rems(3.75);
    pub(crate) const GROUP_INSET_X: Rems = space::SM;
    pub(crate) const COMPACT_HEIGHT: Rems = rems(3.25);
    pub(crate) const PADDING: Rems = space::MD;
}

/// List spacing policy, separate from the shared spacing scale.
pub(crate) mod list {
    pub(crate) const DETAIL_ROW_HEIGHT: Rems = rems(4.);
    pub(crate) const MAX_VISIBLE_ROWS: usize = 8;
    use super::*;
    pub(crate) const INSET_X: Rems = space::SM;
    pub(crate) const INSET_Y: Rems = space::XS;
    pub(crate) const PADDING_X: Rems = space::MD;
}

/// Standard controls use one native size so text, icons and hit areas agree.
/// Inline row actions and title-bar tools deliberately retain native Small.
pub(crate) mod control {
    use gpui_kit::component::Size;
    pub(crate) const STANDARD: Size = Size::Medium;
}

pub(crate) mod window {
    use super::*;
    // Platform window bounds are physical logical pixels, independent of UI zoom.
    pub(crate) const DEFAULT_WIDTH: Pixels = px(1220.);
    pub(crate) const DEFAULT_HEIGHT: Pixels = px(820.);
    pub(crate) const MIN_WIDTH: Pixels = px(960.);
    pub(crate) const MIN_HEIGHT: Pixels = px(640.);
}

pub(crate) mod radius {
    use super::*;
    pub(crate) const CONTROL: Pixels = px(10.);
    pub(crate) const SURFACE: Pixels = px(18.);
}

pub(crate) mod stroke {
    use super::*;
    pub(crate) const NONE: Pixels = px(0.);
    pub(crate) const HAIRLINE: Pixels = px(1.);
    pub(crate) const INSERTION: Pixels = px(2.);
}

pub(crate) mod opacity {
    pub(crate) const TRANSPARENT: f32 = 0.;
    pub(crate) const PREFERENCES_DARK: f32 = 0.72;
    pub(crate) const PREFERENCES_LIGHT: f32 = 0.72;
    pub(crate) const PREFERENCES_DIVIDER: f32 = 0.06;
    pub(crate) const SELECTED: f32 = 0.12;
    pub(crate) const ACTIVE_DARK: f32 = 0.55;
    pub(crate) const ACTIVE_LIGHT: f32 = 0.18;
    pub(crate) const PANEL: f32 = 0.3;
    pub(crate) const DASHBOARD: f32 = 0.9;
    pub(crate) const CONTROL_LIGHT: f32 = 0.65;
    pub(crate) const BUTTON_HOVER: f32 = 0.9;
    pub(crate) const BUTTON_PRESSED: f32 = 0.8;
    const CHROME_DARK: f32 = 0.74;
    const CHROME_LIGHT: f32 = 0.72;
    const WORKSPACE_DARK: f32 = 0.18;
    const WORKSPACE_LIGHT: f32 = 0.88;
    const ACRYLIC_CHROME_DARK: f32 = 0.73;
    const ACRYLIC_CHROME_LIGHT: f32 = 0.68;
    const ACRYLIC_WORKSPACE_DARK: f32 = 0.12;
    const ACRYLIC_WORKSPACE_LIGHT: f32 = 0.30;

    pub(super) fn chrome(dark: bool) -> f32 {
        if cfg!(target_os = "windows") {
            acrylic_chrome(dark)
        } else if dark {
            CHROME_DARK
        } else {
            CHROME_LIGHT
        }
    }
    pub(super) fn workspace(dark: bool) -> f32 {
        if cfg!(target_os = "windows") {
            acrylic_workspace(dark)
        } else if dark {
            WORKSPACE_DARK
        } else {
            WORKSPACE_LIGHT
        }
    }

    pub(super) fn acrylic_chrome(dark: bool) -> f32 {
        if dark {
            ACRYLIC_CHROME_DARK
        } else {
            ACRYLIC_CHROME_LIGHT
        }
    }

    pub(super) fn acrylic_workspace(dark: bool) -> f32 {
        if dark {
            ACRYLIC_WORKSPACE_DARK
        } else {
            ACRYLIC_WORKSPACE_LIGHT
        }
    }
}

pub(crate) mod column {
    pub(crate) const HOST: f32 = 12.;
    pub(crate) const TRAFFIC: f32 = 6.;
    pub(crate) const ADDRESS: f32 = 11.;
    pub(crate) const CHAIN: f32 = 18.;
    pub(crate) const CONNECTION_RULE: f32 = 14.;
    pub(crate) const CONNECTION_PROCESS: f32 = 12.;
    pub(crate) const CONNECTION_TYPE: f32 = 8.;
    pub(crate) const CONTENT: f32 = 13.;
    pub(crate) const PROCESS: f32 = 6.;
    pub(crate) const RATE: f32 = 5.;
    pub(crate) const RULE_TYPE: f32 = 11.;
    pub(crate) const STRATEGY: f32 = 7.5;
    pub(crate) const LAST_HIT: f32 = 7.;
    pub(crate) const MESSAGE: f32 = 25.;
    pub(crate) const LOG_LEVEL: f32 = 7.;
    pub(crate) const LOG_TIME: f32 = 10.5;
}

// 2. 深浅调色板：同一个语义在两种外观中有独立取值。
#[derive(Clone, Copy)]
struct Palette {
    background: u32,
    foreground: u32,
    chrome: u32,
    workspace: u32,
    muted: u32,
    preferences: u32,
    muted_foreground: u32,
    border: u32,
    primary: u32,
    primary_foreground: u32,
    accent: u32,
    selected: u32,
    input: u32,
    dashboard: u32,
    upload: u32,
    download: u32,
}
const DARK: Palette = Palette {
    background: 0x14161b,
    foreground: 0xf0f2f6,
    chrome: 0x0d1015,
    workspace: 0x191c22,
    muted: 0x20242c,
    preferences: 0x282b32,
    muted_foreground: 0xd2d8e3,
    border: 0x444b58,
    primary: 0xa7c4ff,
    primary_foreground: 0x152139,
    accent: 0x293346,
    selected: 0x44669c,
    input: 0x4e5768,
    dashboard: 0x101217,
    upload: 0x938dff,
    download: 0x39cff0,
};
const LIGHT: Palette = Palette {
    background: 0xffffff,
    foreground: 0x252932,
    chrome: 0xffffff,
    workspace: 0xffffff,
    muted: 0xffffff,
    preferences: 0xffffff,
    muted_foreground: 0x29333f,
    border: 0xb7c0cd,
    primary: 0x315fa5,
    primary_foreground: 0xffffff,
    accent: 0xd4e0f3,
    selected: 0x315fa5,
    input: 0xa5b0c1,
    dashboard: 0xffffff,
    upload: 0x6655cc,
    download: 0x087f9d,
};
fn palette(dark: bool) -> Palette {
    if dark { DARK } else { LIGHT }
}

fn translucent(cx: &App) -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
        && crate::integration::read("settings", cx)
            .get("BackdropIndex")
            .is_none_or(|value| value != "纯色")
}

pub(crate) fn window_tint(cx: &App) -> Hsla {
    let color = cx.theme().sidebar;
    if translucent(cx) {
        color.opacity(opacity::chrome(cx.theme().is_dark()))
    } else {
        color
    }
}
pub(crate) fn workspace_tint(cx: &App) -> Hsla {
    if translucent(cx) {
        let p = palette(cx.theme().is_dark());
        Hsla::from(rgb(p.workspace)).opacity(opacity::workspace(cx.theme().is_dark()))
    } else {
        cx.theme().background
    }
}

/// One selection surface; never apply another opacity to this resolved color.
pub(crate) fn selection_tint(cx: &App) -> Hsla {
    cx.theme().list_active
}

pub(crate) fn dashboard_tint(cx: &App) -> Hsla {
    rgb(palette(cx.theme().is_dark()).dashboard).into()
}
pub(crate) fn upload_color(cx: &App) -> Hsla {
    rgb(palette(cx.theme().is_dark()).upload).into()
}
pub(crate) fn download_color(cx: &App) -> Hsla {
    rgb(palette(cx.theme().is_dark()).download).into()
}

// 3. 唯一初始化入口，将产品主题映射到 GPUI Kit。
/// Apply the product theme and synchronize the native window appearance.
pub(crate) fn configure_theme(cx: &mut App) {
    cx.set_window_appearance(Some(if cx.theme().is_dark() {
        WindowAppearance::Dark
    } else {
        WindowAppearance::Light
    }));
    let preferences = crate::integration::read("settings", cx);
    let translucent = translucent(cx);
    Theme::update(cx, |theme| {
        apply(theme, translucent);
        theme.notification.placement = match preferences.get("NoticePosition").map(String::as_str) {
            Some("右上角") => Anchor::TopRight,
            Some("左上角") => Anchor::TopLeft,
            Some("左下角") => Anchor::BottomLeft,
            Some("右下角") => Anchor::BottomRight,
            _ => Anchor::TopCenter,
        };
    });
}

/// Resolve each theme completely so switching appearance cannot retain colors
/// or interactive states from the previous palette.
fn apply(theme: &mut Theme, translucent: bool) {
    let mut p = palette(theme.is_dark());
    if !translucent && !theme.is_dark() {
        p.chrome = 0xf0f2f6;
        p.muted = 0xf5f6f9;
    }
    theme.font_family = if cfg!(target_os = "macos") {
        text::MAC_FONT
    } else {
        text::WINDOWS_FONT
    }
    .into();
    theme.font_size = text::BASE;
    theme.notification.placement = Anchor::TopCenter;
    theme.radius = radius::CONTROL;
    theme.radius_lg = radius::SURFACE;
    theme.colors.background = if !translucent {
        rgb(p.background).into()
    } else if theme.is_dark() {
        Hsla::from(rgb(p.foreground)).opacity(opacity::SELECTED)
    } else {
        Hsla::from(rgb(p.background)).opacity(opacity::CONTROL_LIGHT)
    };
    theme.colors.foreground = rgb(p.foreground).into();
    theme.colors.sidebar = rgb(p.chrome).into();
    theme.colors.sidebar_foreground = rgb(p.muted_foreground).into();
    let selected = Hsla::from(rgb(p.selected)).opacity(if theme.is_dark() {
        opacity::ACTIVE_DARK
    } else {
        opacity::ACTIVE_LIGHT
    });
    theme.colors.sidebar_accent = selected;
    theme.colors.sidebar_accent_foreground = theme.colors.foreground;
    theme.colors.muted = rgb(p.muted).into();
    theme.colors.muted_foreground = rgb(p.muted_foreground).into();
    theme.colors.border = rgb(p.border).into();
    theme.colors.primary = rgb(p.primary).into();
    theme.colors.primary_foreground = rgb(p.primary_foreground).into();
    theme.colors.input = rgb(p.input).into();
    theme.colors.accent = Hsla::from(rgb(p.accent)).opacity(opacity::PANEL);
    theme.colors.accent_foreground = theme.colors.foreground;
    theme.colors.list_active = selected;
    theme.colors.list_active_border = theme.colors.primary;
    theme.colors.list_hover = theme.colors.foreground.opacity(opacity::SELECTED);
    theme.colors.list_head = theme.colors.muted.opacity(opacity::PANEL);
    theme.colors.list_even = theme.colors.background.opacity(opacity::TRANSPARENT);
    theme.colors.popover = rgb(p.background).into();
    theme.colors.popover_foreground = theme.colors.foreground;
    theme.colors.ring = theme.colors.primary;
    theme.colors.selection = theme.colors.primary.opacity(opacity::PANEL);
    theme.colors.title_bar = theme.colors.sidebar;
    theme.colors.title_bar_border = theme.colors.border.opacity(opacity::TRANSPARENT);
    theme.colors.sidebar_border = theme.colors.title_bar_border;
    theme.colors.button_foreground = theme.colors.foreground;
    theme.colors.button_hover = theme.colors.list_hover;
    theme.colors.button_active = selected;
    theme.colors.button_primary = theme.colors.primary;
    theme.colors.button_primary_foreground = theme.colors.primary_foreground;
    theme.colors.button_primary_hover = theme.colors.primary.opacity(opacity::BUTTON_HOVER);
    theme.colors.button_primary_active = theme.colors.primary.opacity(opacity::BUTTON_PRESSED);
    theme.colors.table = theme.colors.background.opacity(opacity::TRANSPARENT);
    theme.colors.table_head = theme.colors.muted.opacity(opacity::PANEL);
    theme.colors.table_foot = theme.colors.table_head;
    theme.colors.table_head_foreground = theme.colors.muted_foreground;
    theme.colors.table_foot_foreground = theme.colors.muted_foreground;
    theme.colors.table_even = theme.colors.table;
    theme.colors.table_hover = theme.colors.list_hover;
    theme.colors.table_active = theme.colors.list_active;
    theme.colors.table_active_border = theme.colors.primary;
    theme.colors.tab = theme.colors.background;
    theme.colors.tab_bar = theme.colors.background;
    theme.colors.tab_bar_segmented = theme.colors.muted.opacity(opacity::PANEL);
    theme.colors.tab_active = selected;
    theme.colors.tab_foreground = theme.colors.muted_foreground;
    theme.colors.tab_active_foreground = theme.colors.foreground;
    theme.colors.secondary = theme.colors.muted.opacity(opacity::PANEL);
    theme.colors.secondary_foreground = theme.colors.foreground;
    theme.colors.secondary_hover = theme.colors.accent;
    theme.colors.secondary_active = selected;
}

pub(crate) fn preferences_tint(cx: &App) -> Hsla {
    let dark = cx.theme().is_dark();
    let color: Hsla = if translucent(cx) {
        rgb(palette(dark).preferences).into()
    } else {
        cx.theme().muted
    };
    color.opacity(if dark {
        opacity::PREFERENCES_DARK
    } else {
        opacity::PREFERENCES_LIGHT
    })
}

#[cfg(test)]
mod tests {
    use super::{opacity, palette};
    fn channels(hex: u32) -> [f32; 3] {
        [
            ((hex >> 16) & 255) as f32,
            ((hex >> 8) & 255) as f32,
            (hex & 255) as f32,
        ]
        .map(|v| v / 255.)
    }
    fn blend(top: [f32; 3], a: f32, bottom: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|i| top[i] * a + bottom[i] * (1. - a))
    }
    fn luminance(c: [f32; 3]) -> f32 {
        let c = c.map(|v| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
    }
    fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    #[gpui_kit::test]
    fn solid_surfaces_are_opaque_distinct_and_restore_after_backdrop_switches(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
        cx.update(gpui_kit::init);
        for dark in [true, false] {
            for translucent in [false, true, false] {
                cx.update(|cx| {
                    Theme::change(
                        if dark {
                            ThemeMode::Dark
                        } else {
                            ThemeMode::Light
                        },
                        None,
                        cx,
                    );
                    Theme::update(cx, |theme| super::apply(theme, translucent));
                    let theme = cx.theme();
                    if translucent {
                        assert!(theme.background.a < 1.);
                    } else {
                        assert_eq!(theme.background.a, 1.);
                        assert_eq!(theme.sidebar.a, 1.);
                        assert_ne!(
                            theme.sidebar, theme.background,
                            "sidebar merged into workspace"
                        );
                        assert_eq!(
                            theme.background,
                            gpui_kit::Hsla::from(gpui_kit::rgb(palette(dark).background))
                        );
                        for surface in [theme.sidebar, theme.background, theme.muted] {
                            let color = surface.to_rgb();
                            assert!(
                                contrast(
                                    channels(palette(dark).foreground),
                                    [color.r, color.g, color.b]
                                ) >= 4.5
                            );
                        }
                    }
                });
            }
        }
    }
    #[test]
    fn text_remains_readable_over_light_and_dark_backdrops_in_both_themes() {
        for dark in [true, false] {
            let p = palette(dark);
            // Exercise Windows Acrylic contrast on every development platform.
            for (chrome_opacity, workspace_opacity) in [
                (opacity::chrome(dark), opacity::workspace(dark)),
                (
                    opacity::acrylic_chrome(dark),
                    opacity::acrylic_workspace(dark),
                ),
            ] {
                for backdrop in [0x000000, 0xffffff] {
                    let chrome = blend(channels(p.chrome), chrome_opacity, channels(backdrop));
                    let workspace = blend(channels(p.workspace), workspace_opacity, chrome);
                    for surface in [
                        chrome,
                        workspace,
                        channels(p.background),
                        blend(channels(p.dashboard), opacity::DASHBOARD, chrome),
                        blend(
                            channels(if dark { p.foreground } else { p.background }),
                            if dark {
                                opacity::SELECTED
                            } else {
                                opacity::CONTROL_LIGHT
                            },
                            workspace,
                        ),
                        blend(channels(p.accent), opacity::PANEL, workspace),
                        blend(
                            channels(p.selected),
                            if dark {
                                opacity::ACTIVE_DARK
                            } else {
                                opacity::ACTIVE_LIGHT
                            },
                            chrome,
                        ),
                        blend(
                            channels(p.selected),
                            if dark {
                                opacity::ACTIVE_DARK
                            } else {
                                opacity::ACTIVE_LIGHT
                            },
                            workspace,
                        ),
                    ] {
                        for text in [p.foreground, p.muted_foreground] {
                            assert!(
                                contrast(channels(text), surface) >= 4.5,
                                "theme dark={dark}, text={text:x}, contrast={}",
                                contrast(channels(text), surface)
                            );
                        }
                    }
                }
            }
        }
    }
}
