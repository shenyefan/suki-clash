use crate::ui::theme::opacity;
mod app;
mod bootstrap;
mod configuration;
mod core;
mod failure;
mod integration;
mod logging;
mod logs;
mod network;
mod overview;
mod preferences;
mod profiles;
mod proxies;
mod search;
mod session;
mod settings;
mod storage;
mod traffic;
mod ui;

use gpui_kit::{
    base::Root,
    component::{ActiveTheme, Theme, ThemeMode},
    *,
};

fn main() {
    let service = match integration::AppPaths::discover() {
        Ok(paths) => match integration::Repository::open(paths.clone())
            .and_then(integration::Service::start)
        {
            Ok(service) => service,
            Err(error) => integration::Service::unavailable(Some(paths), error.to_string()),
        },
        Err(error) => integration::Service::unavailable(None, error.to_string()),
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            integration::init(service, cx);
            gpui_kit::init(cx);
            match integration::read("settings", cx)
                .get("ThemeIndex")
                .map(String::as_str)
            {
                Some("浅色") => Theme::change(ThemeMode::Light, None, cx),
                Some("跟随系统") => Theme::sync_system_appearance(None, cx),
                _ => Theme::change(ThemeMode::Dark, None, cx),
            }
            ui::configure_theme(cx);
            cx.on_action(|_: &app::Quit, cx| core::quit(cx));
            let quit_shortcut = if cx.platform_info().is_mac() {
                "cmd-q"
            } else {
                "ctrl-q"
            };
            cx.bind_keys([KeyBinding::new(quit_shortcut, app::Quit, None)]);
            cx.open_window(app::window::options(cx), |window, cx| {
                window.on_window_should_close(cx, |_, cx| {
                    core::quit(cx);
                    false
                });
                let view = cx.new(|cx| app::SukiApp::new(window, cx));
                let transparent = cx.theme().background.opacity(opacity::TRANSPARENT);
                // Root normally paints an opaque component background. Override
                // only this window surface, keeping menus and dialogs opaque.
                cx.new(|cx| Root::new(view, window, cx).bg(transparent))
            })
            .expect("无法打开 Suki Clash 窗口");
            cx.activate(true);
        });
}
