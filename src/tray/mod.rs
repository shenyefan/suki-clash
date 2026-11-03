//! Application-owned native status icon and OS menu.
#[cfg(target_os = "macos")]
mod macos;
mod menu;
mod model;
mod platform;
use crate::{core, integration, ui};
use gpui_kit::*;

struct Handle(Entity<Tray>);
impl Global for Handle {}
struct Tray {
    platform: platform::Platform,
    main: AnyWindowHandle,
    commands: std::collections::BTreeMap<String, Command>,
    menu_attempt: Option<u64>,
    _events: Option<Task<()>>,
    _observations: Vec<Subscription>,
    icon_task: Option<Task<()>>,
    group_task: Option<Task<()>>,
    group_key: Option<GroupIconsKey>,
    group_icons: std::collections::BTreeMap<String, tray_icon::menu::Icon>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupIconsKey {
    generation: u64,
    epoch: u64,
    groups: std::collections::BTreeMap<String, (String, std::path::PathBuf)>,
}
impl GroupIconsKey {
    fn new(runtime: &core::Runtime) -> Self {
        Self {
            generation: runtime.generation,
            epoch: runtime.epoch,
            groups: runtime
                .proxies
                .entries
                .iter()
                .filter(|(_, proxy)| proxy.all.is_some())
                .filter_map(|(name, proxy)| {
                    let source = proxy.icon.trim();
                    runtime
                        .icons
                        .files
                        .get(source)
                        .map(|path| (name.clone(), (source.to_owned(), path.clone())))
                })
                .collect(),
        }
    }
}

pub(crate) fn init(main: AnyWindowHandle, cx: &mut App) {
    let platform = match platform::Platform::new() {
        Ok(platform) => platform,
        Err(error) => {
            integration::announce_module("ERROR", "tray", format!("无法创建托盘：{error}"), cx);
            let _ = main.update(cx, |_, window, cx| {
                ui::show_error(format!("无法创建托盘：{error}"), window, cx)
            });
            return;
        }
    };
    let (sender, receiver) = async_channel::bounded(32);
    platform::Platform::events(sender);
    let (menu_sender, menu_receiver) = async_channel::bounded(32);
    tray_icon::menu::MenuEvent::set_event_handler(Some(
        move |event: tray_icon::menu::MenuEvent| {
            let _ = menu_sender.try_send(event.id.0);
        },
    ));
    let owner = cx.new(|_| Tray {
        platform,
        main,
        commands: Default::default(),
        menu_attempt: None,
        _events: None,
        _observations: vec![],
        icon_task: None,
        group_task: None,
        group_key: None,
        group_icons: Default::default(),
    });
    cx.set_global(Handle(owner.clone()));
    owner.update(cx, |this, cx| {
        this.sync(cx);
        this.load_icons(cx);
        let controller = core::controller(cx);
        this._observations
            .push(cx.observe(&controller, |this, _, cx| this.sync(cx)));
        this._events = Some(cx.spawn(async move |owner, cx| {
            loop {
                let event = futures_util::future::select(
                    Box::pin(receiver.recv()),
                    Box::pin(menu_receiver.recv()),
                )
                .await;
                match event {
                    futures_util::future::Either::Left((
                        Ok(tray_icon::TrayIconEvent::Click {
                            button,
                            button_state: tray_icon::MouseButtonState::Up,
                            ..
                        }),
                        _,
                    )) => {
                        let _ = owner.update(cx, |this, cx| this.click(button, cx));
                    }
                    futures_util::future::Either::Right((Ok(id), _)) => {
                        let action = owner.update(cx, |this, _| {
                            this.commands
                                .get(&id)
                                .cloned()
                                .map(|command| (this.main, command))
                        });
                        if let Ok(Some((main, command))) = action {
                            cx.update(|cx| dispatch(&command, main, cx));
                        }
                    }
                    futures_util::future::Either::Left((Err(_), _))
                    | futures_util::future::Either::Right((Err(_), _)) => break,
                    _ => {}
                }
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
    fn load_icons(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = integration::paths(cx) else {
            return;
        };
        self.icon_task = Some(cx.spawn(async move |owner, cx| {
            let icons = cx
                .background_spawn(async move {
                    let mut icons = std::collections::BTreeMap::new();
                    for kind in ["common", "system", "tun"] {
                        let path = paths.settings().join("tray").join(format!("{kind}.image"));
                        match std::fs::read(&path) {
                            Ok(bytes) => {
                                icons.insert(kind.into(), decode_icon(&bytes)?);
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(error) => {
                                return Err(format!(
                                    "无法读取托盘图标 {}：{error}",
                                    path.display()
                                ));
                            }
                        }
                    }
                    Ok::<_, String>(icons)
                })
                .await;
            let _ = owner.update(cx, |this, cx| {
                if let Some(task) = this.icon_task.take() {
                    task.detach();
                }
                match icons {
                    Ok(icons) => {
                        this.platform.custom(icons);
                        this.sync(cx);
                    }
                    Err(error) => {
                        let _ = this
                            .main
                            .update(cx, |_, window, cx| ui::show_error(error, window, cx));
                    }
                }
            });
        }));
    }
    fn sync(&mut self, cx: &mut Context<Self>) {
        let controller = core::controller(cx);
        let state = controller.read(cx);
        let runtime = state.runtime();
        let key = GroupIconsKey::new(&runtime);
        if self.group_key.as_ref() != Some(&key) {
            self.group_key = Some(key.clone());
            // A new binding cannot reuse icons attached to old group names.
            self.group_task = None;
            self.group_icons.clear();
            self.menu_attempt = None;
            let groups = key.groups.clone();
            self.group_task = Some(cx.spawn(async move |owner, cx| {
                let result = cx
                    .background_spawn(async move {
                        groups
                            .into_iter()
                            .filter_map(|(name, (_, path))| {
                                let bytes = std::fs::read(path).ok()?;
                                let rgba = match image::load_from_memory(&bytes) {
                                    Ok(image) => image
                                        .resize_exact(18, 18, image::imageops::FilterType::Lanczos3)
                                        .into_rgba8()
                                        .into_raw(),
                                    Err(_) => {
                                        let tree = resvg::usvg::Tree::from_data(
                                            &bytes,
                                            &Default::default(),
                                        )
                                        .ok()?;
                                        let mut pixmap = resvg::tiny_skia::Pixmap::new(18, 18)?;
                                        let size = tree.size();
                                        let scale = (18.0 / size.width()).min(18.0 / size.height());
                                        let transform =
                                            resvg::tiny_skia::Transform::from_translate(
                                                (18.0 - size.width() * scale) / 2.0,
                                                (18.0 - size.height() * scale) / 2.0,
                                            )
                                            .pre_scale(scale, scale);
                                        resvg::render(&tree, transform, &mut pixmap.as_mut());
                                        pixmap
                                            .pixels()
                                            .iter()
                                            .flat_map(|pixel| {
                                                let pixel = pixel.demultiply();
                                                [
                                                    pixel.red(),
                                                    pixel.green(),
                                                    pixel.blue(),
                                                    pixel.alpha(),
                                                ]
                                            })
                                            .collect()
                                    }
                                };
                                tray_icon::menu::Icon::from_rgba(rgba, 18, 18)
                                    .ok()
                                    .map(|icon| (name, icon))
                            })
                            .collect()
                    })
                    .await;
                let _ = owner.update(cx, |this, cx| {
                    if this.group_key.as_ref() != Some(&key) {
                        return;
                    }
                    if let Some(task) = this.group_task.take() {
                        task.detach();
                    }
                    this.group_icons = result;
                    this.menu_attempt = None;
                    this.sync(cx);
                });
            }));
        }
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
            match menu::build(
                &runtime,
                state.snapshot(),
                state.is_busy(),
                &general,
                cx,
                self.group_icons.clone(),
                key,
            ) {
                Ok((menu, commands)) => {
                    self.platform.menu(
                        menu,
                        general
                            .get("TrayEvent")
                            .is_none_or(|value| value == "显示托盘菜单"),
                    );
                    self.commands = commands;
                }
                Err(error) => errors.push(format!("托盘菜单：{error}")),
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
        if let Err(error) = self.platform.update(
            general.get("TrayIcon").is_none_or(|value| value != "彩色"),
            system,
            tun,
            &tooltip,
            speed.as_deref(),
        ) {
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
    fn click(&mut self, button: tray_icon::MouseButton, cx: &mut Context<Self>) {
        // A deliberate tray interaction may retry a paused native operation once.
        self.platform.retry();
        self.menu_attempt = None;
        self.sync(cx);
        if button != tray_icon::MouseButton::Left {
            return;
        }
        let behavior = integration::read("settings", cx)
            .get("TrayEvent")
            .cloned()
            .unwrap_or_else(|| "显示托盘菜单".into());
        match behavior.as_str() {
            "显示主窗口" => {
                let main = self.main;
                cx.defer(move |cx| show_main(main, cx));
            }
            "切换系统代理" => {
                let main = self.main;
                cx.defer(move |cx| dispatch(&Command::SystemProxy, main, cx));
            }
            "切换虚拟网卡" => {
                let main = self.main;
                cx.defer(move |cx| dispatch(&Command::Tun, main, cx));
            }
            _ => {}
        }
    }
}

fn decode_icon(bytes: &[u8]) -> Result<tray_icon::Icon, String> {
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("托盘图标超过 2 MiB".into());
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "无法识别托盘图标")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| "托盘图标无效，请选择 PNG、ICO 或其他支持的图片")?
        .resize(32, 32, image::imageops::FilterType::Lanczos3)
        .into_rgba8();
    tray_icon::Icon::from_rgba(image.into_raw(), 32, 32).map_err(|error| error.to_string())
}

pub(crate) fn choose_icon(kind: &'static str, window: &mut Window, cx: &mut App) {
    let Some(handle) = cx.try_global::<Handle>() else {
        ui::show_error("托盘不可用".into(), window, cx);
        return;
    };
    let owner = handle.0.clone();
    if owner.read(cx).icon_task.is_some() {
        ui::show_error("正在处理托盘图标，请稍后重试".into(), window, cx);
        return;
    }
    let Some(paths) = integration::paths(cx) else {
        return;
    };
    let picker = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("选择托盘图标".into()),
    });
    let origin = window.window_handle();
    owner.update(cx, |this, cx| {
        this.icon_task = Some(cx.spawn(async move |owner, cx| {
            let result = async {
                let selected = match picker
                    .await
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())?
                {
                    Some(mut paths) => paths.pop(),
                    None => None,
                };
                let Some(selected) = selected else {
                    return Ok(None);
                };
                cx.background_spawn(async move {
                    let bytes = std::fs::read(&selected).map_err(|error| error.to_string())?;
                    let icon = decode_icon(&bytes)?;
                    let directory = paths.settings().join("tray");
                    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
                    crate::storage::atomic_write(&directory.join(format!("{kind}.image")), &bytes)
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>(Some(icon))
                })
                .await
            }
            .await;
            let _ = owner.update(cx, |this, cx| {
                if let Some(task) = this.icon_task.take() {
                    task.detach();
                }
                match result {
                    Ok(Some(_)) => this.load_icons(cx),
                    Ok(None) => {}
                    Err(error) => {
                        let _ =
                            origin.update(cx, |_, window, cx| ui::show_error(error, window, cx));
                    }
                }
            });
        }));
    });
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
    OpenDirectory(String),
    OpenLog(bool),
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
            Command::Tun => core::request(core::Command::ToggleTun, window, cx),
            Command::SystemProxy => core::request(core::Command::ToggleSystemProxy, window, cx),
            Command::CloseConnections { generation, epoch } => core::request(
                core::Command::Connections(crate::traffic::ConnectionAction::CloseAll {
                    generation,
                    epoch,
                }),
                window,
                cx,
            ),
            Command::RestartCore => core::request(core::Command::Restart, window, cx),
            Command::OpenDirectory(kind) => {
                let kind = match kind.as_str() {
                    "core" => integration::DirectoryKind::Core,
                    "logs" => integration::DirectoryKind::Logs,
                    _ => integration::DirectoryKind::Config,
                };
                integration::execute(
                    integration::Command::OpenDirectory(kind),
                    window,
                    cx,
                    |result, window, cx| match result {
                        Ok(integration::Outcome::Directory(path)) => cx.open_with_system(&path),
                        Err(error) => ui::show_error(error.to_string(), window, cx),
                        _ => {}
                    },
                );
            }
            Command::OpenLog(core) => open_log(core, window, cx),
            Command::CopyEnvironment => crate::settings::copy_environment(window, cx),
            _ => {}
        }
    });
}

fn open_log(core: bool, window: &mut Window, cx: &mut App) {
    let Some(paths) = integration::paths(cx) else {
        return;
    };
    let directory = paths.directory(integration::DirectoryKind::Logs);
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let found = cx
            .background_spawn(async move {
                let prefix = if core { "core-" } else { "program-" };
                std::fs::read_dir(directory)
                    .map_err(|error| error.to_string())?
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
                    .max_by_key(|entry| entry.metadata().and_then(|m| m.modified()).ok())
                    .map(|entry| entry.path())
                    .ok_or_else(|| "暂无日志文件".to_string())
            })
            .await;
        let _ = handle.update(cx, |_, window, cx| match found {
            Ok(path) => cx.open_with_system(&path),
            Err(error) => ui::show_error(error, window, cx),
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::GroupIconsKey;
    use crate::core;
    #[test]
    fn icon_binding_identity_tracks_names_sources_and_runtime_scope() {
        let mut runtime = core::Runtime {
            generation: 1,
            epoch: 1,
            ..Default::default()
        };
        runtime.proxies = crate::proxies::runtime::Proxies::parse(br#"{"proxies":{"a":{"name":"a","type":"Selector","all":[],"icon":"https://example.test/x"},"b":{"name":"b","type":"Selector","all":[],"icon":"https://example.test/y"}}}"#, &[]).unwrap();
        runtime
            .icons
            .files
            .insert("https://example.test/x".into(), "/tmp/x".into());
        runtime
            .icons
            .files
            .insert("https://example.test/y".into(), "/tmp/y".into());
        let initial = GroupIconsKey::new(&runtime);
        runtime.proxies.entries.get_mut("a").unwrap().icon = "https://example.test/y".into();
        runtime.proxies.entries.get_mut("b").unwrap().icon = "https://example.test/x".into();
        assert_ne!(initial, GroupIconsKey::new(&runtime));
        let swapped = GroupIconsKey::new(&runtime);
        let proxy = runtime.proxies.entries.remove("a").unwrap();
        runtime.proxies.entries.insert("renamed".into(), proxy);
        assert_ne!(swapped, GroupIconsKey::new(&runtime));
        let renamed = GroupIconsKey::new(&runtime);
        runtime.epoch += 1;
        assert_ne!(renamed, GroupIconsKey::new(&runtime));
        let revised = GroupIconsKey::new(&runtime);
        runtime.generation += 1;
        assert_ne!(revised, GroupIconsKey::new(&runtime));
    }
}
