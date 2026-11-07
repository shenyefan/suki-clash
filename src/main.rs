#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod platform_access;
use crate::ui::theme::opacity;
mod app;
mod backup;
mod bootstrap;
mod configuration;
mod core;
mod desktop;
mod diagnostics;
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
mod system_proxy;
mod traffic;
mod tray;
mod ui;
mod updates;

use gpui_kit::{
    base::Root,
    component::{ActiveTheme, Theme, ThemeMode},
    *,
};

fn main() {
    velopack::VelopackApp::build()
        .set_auto_apply_on_startup(false)
        .run();
    let (instance, instance_error) = match integration::AppPaths::discover()
        .map_err(crate::logging::error_message)
        .and_then(|paths| {
            desktop::instance::Service::acquire(
                &paths,
                std::env::args().skip(1).find(|argument| {
                    argument.starts_with("clash:") || argument.starts_with("suki-clash:")
                }),
            )
        }) {
        Ok(desktop::instance::Disposition::Secondary) => return,
        Ok(desktop::instance::Disposition::Primary(service)) => (Some(service), None),
        Err(error) => (None, Some(error)),
    };
    let service = match integration::AppPaths::discover() {
        Ok(paths) if instance_error.is_some() => integration::Service::unavailable(
            Some(paths),
            instance_error.clone().unwrap_or_default(),
        ),
        Ok(paths) => match integration::Repository::open(paths.clone())
            .and_then(integration::Service::start)
        {
            Ok(service) => service,
            Err(error) => integration::Service::unavailable(Some(paths), error.to_string()),
        },
        Err(error) => integration::Service::unavailable(None, error.to_string()),
    };
    service.enable_logging();
    logging::install_panic_hook();
    log::info!(
        "Suki Clash {} 已启动，系统 {}，架构 {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let url_sender = instance.as_ref().map(|instance| instance.sender.clone());
    let application = application().with_assets(assets::AllAssets);
    application.on_open_urls(move |urls| {
        for uri in urls {
            if let Some(sender) = &url_sender {
                let _ = sender.try_send(desktop::instance::Activation { uri: Some(uri) });
            }
        }
    });
    application.on_reopen(desktop::reopen);
    application.run(move |cx| {
        #[cfg(target_os = "macos")]
        desktop::configure_icon();
        cx.set_app_identity("suki-clash", "Suki Clash");
        integration::init(service, instance_error, cx);
        init(cx);
        let font_error = ui::theme::load_fonts(cx).err();
        gpui_kit::component::set_locale("zh-CN");
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
        let captured = std::rc::Rc::new(std::cell::RefCell::new(None));
        let app_entity = captured.clone();
        let silent = integration::read("settings", cx)
            .get("SilentStart")
            .is_some_and(|value| value == "true");
        let mut options = app::window::options(cx);
        options.show = !silent;
        options.focus = !silent;
        let main_window = cx
            .open_window(options, |window, cx| {
                window.on_window_should_close(cx, |window, cx| {
                    desktop::close_main(window, cx);
                    false
                });
                let view = cx.new(|cx| app::SukiApp::new(window, cx));
                *app_entity.borrow_mut() = Some(view.clone());
                let transparent = cx.theme().background.opacity(opacity::TRANSPARENT);
                // Root normally paints an opaque component background. Override
                // only this window surface, keeping menus and dialogs opaque.
                cx.new(|cx| Root::new(view, window, cx).bg(transparent))
            })
            .expect("无法打开 Suki Clash 窗口");
        if let Some(error) = font_error {
            let message = format!("无法加载 MiSans 字体：{error}");
            log::error!(target: "ui", "{message}");
            let _ = main_window.update(cx, |_, window, cx| {
                ui::notifications::push(
                    window,
                    gpui_kit::component::notification::Notification::error(message),
                    cx,
                );
            });
        }
        let primary = instance.is_some();
        if let (Some(instance), Some(app)) = (instance, captured.borrow_mut().take()) {
            desktop::install(instance, main_window, app, cx);
        }
        cx.defer(move |cx| {
            if primary {
                tray::init(main_window.into(), cx);
            }
            if !silent || !tray::available(cx) {
                desktop::show_main(main_window.into(), cx);
            }
        });
        if !primary {
            return;
        }
        cx.spawn(async move |cx| {
            #[cfg(not(target_os = "macos"))]
            if let Err(error) = cx
                .background_spawn(async { desktop::scheme::register() })
                .await
            {
                cx.update(|cx| integration::announce_module("WARNING", "desktop", error, cx));
            }
            #[cfg(target_os = "macos")]
            if objc2_foundation::NSBundle::mainBundle()
                .bundleIdentifier()
                .is_some()
            {
                for scheme in ["clash", "suki-clash"] {
                    let registration = cx.update(|cx| cx.register_url_scheme(scheme));
                    if let Err(error) = registration.await {
                        cx.update(|cx| {
                            integration::announce_module(
                                "WARNING",
                                "desktop",
                                format!("无法注册订阅链接处理：{error}"),
                                cx,
                            )
                        });
                    }
                }
            }
        })
        .detach();
    });
}
