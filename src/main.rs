use crate::ui::theme::opacity;
mod app;
mod integration;
mod logs;
mod overview;
mod profiles;
mod proxies;
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
    let service = match integration::AppPaths::discover()
        .and_then(|paths| {
            integration::Repository::open(
                paths,
                Some(
                    &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("target/suki-preview"),
                ),
            )
        })
        .and_then(integration::Service::start)
    {
        Ok(repository) => repository,
        Err(error) => {
            eprintln!("无法初始化应用存储：{error}");
            return;
        }
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
            cx.on_action(|_: &app::Quit, cx| cx.quit());
            let quit_shortcut = if cx.platform_info().is_mac() {
                "cmd-q"
            } else {
                "ctrl-q"
            };
            cx.bind_keys([KeyBinding::new(quit_shortcut, app::Quit, None)]);
            cx.open_window(app::window::options(cx), |window, cx| {
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
