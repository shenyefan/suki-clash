//! Application-owned native status icon and GPUI popup.
#[cfg(target_os = "linux")]
mod linux;
mod menu;
mod model;
mod placement;
mod platform;
mod popup;
use crate::{core, integration, ui};
use gpui_kit::*;

struct Handle {
    _tray: Entity<Tray>,
}
impl Global for Handle {}
struct Tray {
    platform: platform::Platform,
    main: AnyWindowHandle,
    popup: Option<(AnyWindowHandle, WeakEntity<popup::Panel>)>,
    menu_attempt: Option<u64>,
    _events: Option<Task<()>>,
    _observations: Vec<Subscription>,
}

pub(crate) fn init(main: AnyWindowHandle, cx: &mut App) {
    popup::init(cx);
    let (sender, receiver) = async_channel::bounded(32);
    let platform = match platform::Platform::new(cx, sender) {
        Ok(platform) => platform,
        Err(error) => {
            integration::announce_module("ERROR", "tray", format!("无法创建托盘：{error}"), cx);
            let _ = main.update(cx, |_, window, cx| {
                ui::show_error(format!("无法创建托盘：{error}"), window, cx)
            });
            return;
        }
    };
    let owner = cx.new(|_| Tray {
        platform,
        main,
        popup: None,
        menu_attempt: None,
        _events: None,
        _observations: vec![],
    });
    cx.set_global(Handle {
        _tray: owner.clone(),
    });
    owner.update(cx, |this, cx| {
        this.sync(cx);
        let controller = core::controller(cx);
        this._observations
            .push(cx.observe(&controller, |this, _, cx| this.sync(cx)));
        this._events = Some(cx.spawn(async move |owner, cx| {
            while let Ok(event) = receiver.recv().await {
                let _ = owner.update(cx, |this, cx| match event {
                    platform::Event::Icon(
                        event @ tray_icon::TrayIconEvent::Click {
                            button_state: tray_icon::MouseButtonState::Up,
                            ..
                        },
                    ) => this.click(event, cx),
                    #[cfg(target_os = "linux")]
                    platform::Event::Error(error) => this.error(error, cx),
                    _ => {}
                });
            }
        }));
    });
}

pub(crate) fn close_to_tray(cx: &App) -> bool {
    cx.try_global::<Handle>().is_some()
        && integration::read("settings", cx)
            .get("CloseToTray")
            .is_some_and(|value| value == "true")
}
pub(crate) fn available(cx: &App) -> bool {
    cx.try_global::<Handle>().is_some()
}

fn show_main(main: AnyWindowHandle, cx: &mut App) {
    crate::desktop::show_main(main, cx);
}

impl Tray {
    fn sync(&mut self, cx: &mut Context<Self>) {
        let controller = core::controller(cx);
        let state = controller.read(cx);
        let runtime = state.runtime();
        let general = integration::read("settings", cx);
        let mut errors = Vec::new();
        let system = state.snapshot().system_proxy().enabled() == Some(true);
        let tun = state.snapshot().tun() == Some(true);
        let profile = state
            .snapshot()
            .profile()
            .map(|profile| profile.name())
            .unwrap_or("未选择");
        let tooltip = format!(
            "Suki Clash {}\n系统代理：{}\n虚拟网卡：{}\n出站模式：{}\n订阅：{profile}",
            env!("CARGO_PKG_VERSION"),
            match state.snapshot().system_proxy().enabled() {
                Some(true) => "开启",
                Some(false) => "关闭",
                None => "待确认",
            },
            match state.snapshot().tun() {
                Some(true) => "开启",
                Some(false) => "关闭",
                None => "待确认",
            },
            model::mode_label(runtime.connected.then_some(runtime.mode).flatten())
        );
        let context = menu::context_revision(
            state.snapshot(),
            state.is_busy(),
            &integration::read("profile-index", cx),
        );
        let key = menu::revision(&runtime, &general, context);
        if self.menu_attempt != Some(key) {
            self.menu_attempt = Some(key);
            if let Some((handle, panel)) = &self.popup {
                let panel = panel.clone();
                let _ = handle.update(cx, |_, window, cx| {
                    let _ = panel.update(cx, |panel, cx| panel.sync(window, cx));
                });
            }
        }
        let speed = if cfg!(target_os = "macos")
            && general
                .get("EnableTraySpeed")
                .is_some_and(|value| value == "true")
        {
            Some(if runtime.traffic_live {
                format!(
                    "↑ {}  ↓ {}",
                    runtime
                        .statistics
                        .upload_rate
                        .map(|v| format!("{}/s", ui::file_size(v)))
                        .unwrap_or_else(|| "—".into()),
                    runtime
                        .statistics
                        .download_rate
                        .map(|v| format!("{}/s", ui::file_size(v)))
                        .unwrap_or_else(|| "—".into())
                )
            } else {
                "↑ —  ↓ —".into()
            })
        } else {
            None
        };
        if let Err(error) = self
            .platform
            .update(system, tun, &tooltip, speed.as_deref())
        {
            errors.push(error);
        }
        if !errors.is_empty() {
            let error = errors.join("；");
            integration::announce_module("ERROR", "tray", error.clone(), cx);
            let _ = self
                .main
                .update(cx, |_, window, cx| ui::show_error(error, window, cx));
        }
    }
    fn error(&self, error: String, cx: &mut Context<Self>) {
        integration::announce_module("ERROR", "tray", error.clone(), cx);
        let _ = self
            .main
            .update(cx, |_, window, cx| ui::show_error(error, window, cx));
    }
    fn click(&mut self, event: tray_icon::TrayIconEvent, cx: &mut Context<Self>) {
        let tray_icon::TrayIconEvent::Click { button, .. } = &event else {
            return;
        };
        if *button == tray_icon::MouseButton::Middle {
            return;
        }
        let behavior = integration::read("settings", cx)
            .get("TrayEvent")
            .cloned()
            .unwrap_or_else(|| "显示托盘菜单".into());
        let command = if *button == tray_icon::MouseButton::Left {
            match behavior.as_str() {
                "显示主窗口" => Some(Command::ShowWindow),
                "切换系统代理" => Some(Command::SystemProxy),
                "切换虚拟网卡" => Some(Command::Tun),
                _ => None,
            }
        } else {
            None
        };
        self.platform.retry();
        if let Some(command) = command {
            if let Some((handle, _)) = self.popup.take() {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
            let main = self.main;
            cx.defer(move |cx| dispatch(&command, main, cx));
            return;
        }
        if let Some((handle, _)) = self.popup.take()
            && handle
                .update(cx, |_, window, _| window.remove_window())
                .is_ok()
        {
            return;
        }
        let options = placement::anchor(&event, cx)
            .and_then(|anchor| placement::options(anchor, cx).map(|options| (options, anchor)));
        let main = self.main;
        match options.and_then(|(options, anchor)| {
            let mut panel = None;
            let handle = cx
                .open_window(options, |window, cx| {
                    window.set_window_title("Suki Clash · 托盘");
                    let view = cx.new(|cx| popup::Panel::new(main, anchor, window, cx));
                    panel = Some(view.downgrade());
                    use gpui_kit::component::ActiveTheme;
                    let transparent = cx
                        .theme()
                        .popover
                        .opacity(crate::ui::theme::opacity::TRANSPARENT);
                    cx.new(|cx| gpui_kit::base::Root::new(view, window, cx).bg(transparent))
                })
                .map_err(crate::logging::error_message)?;
            Ok((handle.into(), panel))
        }) {
            Ok((handle, Some(panel))) => self.popup = Some((handle, panel)),
            Ok(_) => {}
            Err(error) => {
                self.error(format!("无法打开托盘菜单：{error}"), cx);
            }
        }
    }
}

/// Dispatch to the long-lived main window so asynchronous feedback survives panel dismissal.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, Action)]
#[action(namespace=tray, no_json)]
pub(super) enum Command {
    Mode(crate::proxies::runtime::ProxyMode),
    Profile(u64),
    Node {
        group: String,
        node: String,
        generation: u64,
        epoch: u64,
    },
    SystemProxy,
    Tun,
    CloseConnections {
        generation: u64,
        epoch: u64,
    },
    CopyEnvironment,
    RestartCore,
    RestartProgram,
    ShowWindow,
    Quit,
}

pub(super) fn dispatch(command: &Command, main: AnyWindowHandle, cx: &mut App) {
    if matches!(command, Command::Quit) {
        core::quit(cx);
        return;
    }
    if matches!(command, Command::RestartProgram) {
        core::restart_program(cx);
        return;
    }
    if matches!(command, Command::ShowWindow) {
        show_main(main, cx);
        return;
    }
    let command = command.clone();
    let _ = main.update(cx, move |_, window, cx| {
        use crate::proxies::runtime::Action;
        match command {
            Command::Mode(mode) => {
                core::request(core::Command::Proxy(Action::Mode(mode)), window, cx)
            }
            Command::Profile(id) => core::request(core::Command::Activate(id), window, cx),
            Command::Node {
                group,
                node,
                generation,
                epoch,
            } => core::request(
                core::Command::Proxy(Action::Select {
                    group,
                    node,
                    generation,
                    epoch,
                }),
                window,
                cx,
            ),
            Command::Tun => ui::request_command("切换虚拟网卡", window, cx),
            Command::SystemProxy => ui::request_command("切换系统代理", window, cx),
            Command::CloseConnections { generation, epoch } => core::request(
                core::Command::Connections(crate::traffic::ConnectionAction::CloseAll {
                    generation,
                    epoch,
                }),
                window,
                cx,
            ),
            Command::RestartCore => core::request(core::Command::Restart, window, cx),
            Command::CopyEnvironment => crate::settings::copy_environment(window, cx),
            _ => {}
        }
    });
}
