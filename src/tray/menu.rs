//! OS-native status menu. The OS owns tracking, focus, material and overflow arrows.
use super::{Command, model};
use crate::{core, integration, storage::Values};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
};
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

pub(super) fn revision(runtime: &core::Runtime, settings: &Values, revision: u64) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    revision.hash(&mut hash);
    settings.hash(&mut hash);
    runtime.connected.hash(&mut hash);
    runtime.generation.hash(&mut hash);
    runtime.epoch.hash(&mut hash);
    runtime.mode.map(|m| m.label()).hash(&mut hash);
    for name in &runtime.proxies.order {
        name.hash(&mut hash);
        if let Some(group) = runtime.proxies.entries.get(name) {
            group.kind.hash(&mut hash);
            group.hidden.hash(&mut hash);
            group.now.hash(&mut hash);
            group.all.hash(&mut hash);
        }
    }
    runtime.proxies.provider_error.hash(&mut hash);
    for (group, members) in &runtime.proxies.members {
        group.hash(&mut hash);
        for member in members {
            member.name.hash(&mut hash);
            member.occurrence.hash(&mut hash);
            member.record_id.hash(&mut hash);
            member.reason.hash(&mut hash);
        }
    }
    // Delay updates refresh labels without rebuilding on each traffic sample.
    for (key, value) in &runtime.proxies.measurements {
        key.hash(&mut hash);
        value.hash(&mut hash);
    }
    hash.finish()
}
/// Menu identity tracks visible facts, not unrelated lifecycle publication counters.
pub(super) fn context_revision(snapshot: &core::Snapshot, busy: bool, profiles: &Values) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (snapshot.phase() == core::Phase::Ready).hash(&mut hash);
    snapshot.system_proxy().enabled().hash(&mut hash);
    snapshot.tun().hash(&mut hash);
    snapshot
        .profile()
        .map(|profile| (profile.id(), profile.name()))
        .hash(&mut hash);
    busy.hash(&mut hash);
    profiles.hash(&mut hash);
    hash.finish()
}

struct Builder {
    commands: BTreeMap<String, Command>,
    revision: u64,
    icons: BTreeMap<String, tray_icon::menu::Icon>,
    #[cfg(target_os = "macos")]
    title_width: f64,
}
impl Builder {
    fn item(
        &mut self,
        label: impl AsRef<str>,
        enabled: bool,
        checked: Option<bool>,
        command: Command,
    ) -> Box<dyn IsMenuItem> {
        // IDs encode no names and cannot collide for names containing underscores.
        let id = format!("suki-tray-{}-{}", self.revision, self.commands.len());
        self.commands.insert(id.clone(), command);
        match checked {
            Some(checked) => Box::new(CheckMenuItem::with_id(id, label, enabled, checked, None)),
            None => Box::new(MenuItem::with_id(id, label, enabled, None)),
        }
    }
    fn nodes(
        &mut self,
        name: &str,
        runtime: &core::Runtime,
        enabled: bool,
    ) -> Result<Submenu, String> {
        let group = &runtime.proxies.entries[name];
        let selected = runtime.proxies.selected(name).unwrap_or("未选择");
        let menu = Submenu::new(format!("{name} — {selected}"), enabled);
        #[cfg(target_os = "macos")]
        super::macos::title(&menu, name, selected, self.title_width);
        if let Some(icon) = self.icons.get(name) {
            menu.set_icon(Some(icon.clone()));
        }
        if let Some(members) = runtime.proxies.members.get(name) {
            for member in members {
                let key = runtime.proxies.measurement_key(name, member);
                let label = match runtime.proxies.measurements.get(&(name.into(), key)) {
                    Some(Ok(0)) | Some(Err(crate::proxies::runtime::MeasurementError::Timeout)) => {
                        format!("{} · 超时", member.name)
                    }
                    Some(Ok(ms)) => format!("{} · {ms} ms", member.name),
                    Some(Err(crate::proxies::runtime::MeasurementError::Failed(_))) => {
                        format!("{} · 失败", member.name)
                    }
                    _ => member.name.clone(),
                };
                let item = self.item(
                    label,
                    enabled
                        && member.reason.is_none()
                        && matches!(group.kind.as_str(), "Selector" | "URLTest" | "Fallback"),
                    Some(selected == member.name),
                    Command::Node {
                        group: name.into(),
                        node: member.name.clone(),
                        generation: runtime.generation,
                        epoch: runtime.epoch,
                    },
                );
                menu.append(item.as_ref())
                    .map_err(|error| error.to_string())?;
            }
        }
        Ok(menu)
    }
}
pub(super) fn build(
    runtime: &core::Runtime,
    snapshot: &core::Snapshot,
    busy: bool,
    settings: &Values,
    cx: &gpui_kit::App,
    icons: BTreeMap<String, tray_icon::menu::Icon>,
    revision: u64,
) -> Result<(Menu, BTreeMap<String, Command>), String> {
    let mut builder = Builder {
        commands: Default::default(),
        revision,
        icons,
        #[cfg(target_os = "macos")]
        title_width: super::macos::width(model::groups(&runtime.proxies, runtime.mode).iter().map(
            |name| {
                (
                    name.as_str(),
                    runtime.proxies.selected(name).unwrap_or("未选择"),
                )
            },
        )),
    };
    let menu = Menu::new();
    let ready = snapshot.phase() == core::Phase::Ready && runtime.connected;
    let modes = Submenu::new(
        format!(
            "出站模式 — {}",
            model::mode_label(runtime.connected.then_some(runtime.mode).flatten())
        ),
        ready && !busy,
    );
    let inline = settings.get("TrayInlineModes").is_some_and(|v| v == "true");
    for mode in [
        crate::session::ProxyMode::Rule,
        crate::session::ProxyMode::Global,
        crate::session::ProxyMode::Direct,
    ] {
        let item = builder.item(
            model::mode_label(Some(mode)),
            ready && !busy,
            Some(runtime.mode == Some(mode)),
            Command::Mode(mode),
        );
        if inline {
            menu.append(item.as_ref())
        } else {
            modes.append(item.as_ref())
        }
        .map_err(|error| error.to_string())?;
    }
    if !inline {
        menu.append(&modes).map_err(|error| error.to_string())?;
    }
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|error| error.to_string())?;
    let groups = Submenu::new("代理组", ready && !busy);
    let layout = settings
        .get("TrayProxyGroups")
        .map(String::as_str)
        .unwrap_or("内联");
    if ready && layout != "隐藏" {
        for name in model::groups(&runtime.proxies, runtime.mode) {
            let submenu = builder.nodes(&name, runtime, !busy)?;
            if layout == "默认" {
                groups.append(&submenu)
            } else {
                menu.append(&submenu)
            }
            .map_err(|error| error.to_string())?;
        }
        if layout == "默认" {
            menu.append(&groups).map_err(|error| error.to_string())?;
        }
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|error| error.to_string())?;
    }
    let enabled = snapshot.system_proxy().enabled();
    for (label, available, checked, command) in [
        (
            if enabled.is_none() {
                "系统代理（待确认）"
            } else {
                "系统代理"
            },
            !busy && (ready || enabled == Some(true)),
            Some(enabled == Some(true)),
            Command::SystemProxy,
        ),
        (
            "虚拟网卡",
            ready && !busy,
            Some(snapshot.tun() == Some(true)),
            Command::Tun,
        ),
        ("复制终端代理命令", true, None, Command::CopyEnvironment),
    ] {
        menu.append(builder.item(label, available, checked, command).as_ref())
            .map_err(|error| error.to_string())?;
    }
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|error| error.to_string())?;
    let profile = snapshot.profile();
    let profiles = Submenu::new(
        format!("订阅 — {}", profile.map(|p| p.name()).unwrap_or("未选择")),
        !busy,
    );
    if let Some((documents, _)) =
        crate::profiles::store::load_values(integration::read("profile-index", cx))
    {
        for doc in documents {
            profiles
                .append(
                    builder
                        .item(
                            doc.name,
                            !busy,
                            Some(profile.is_some_and(|p| p.id() == doc.id)),
                            Command::Profile(doc.id),
                        )
                        .as_ref(),
                )
                .map_err(|error| error.to_string())?;
        }
    }
    menu.append(&profiles).map_err(|error| error.to_string())?;
    let directories = Submenu::new("打开目录与日志", true);
    for (label, command) in [
        ("配置目录", Command::OpenDirectory("config".into())),
        ("内核目录", Command::OpenDirectory("core".into())),
        ("日志目录", Command::OpenDirectory("logs".into())),
        ("程序日志", Command::OpenLog(false)),
        ("内核日志", Command::OpenLog(true)),
    ] {
        directories
            .append(builder.item(label, true, None, command).as_ref())
            .map_err(|error| error.to_string())?;
    }
    menu.append(&directories)
        .map_err(|error| error.to_string())?;
    let more = Submenu::new("更多操作", true);
    for (label, available, command) in [
        (
            "关闭全部连接",
            ready && !busy,
            Command::CloseConnections {
                generation: runtime.generation,
                epoch: runtime.epoch,
            },
        ),
        ("重启内核", !busy, Command::RestartCore),
        ("重启程序", true, Command::RestartProgram),
    ] {
        more.append(builder.item(label, available, None, command).as_ref())
            .map_err(|error| error.to_string())?;
    }
    menu.append(&more).map_err(|error| error.to_string())?;
    menu.append(
        builder
            .item("打开主窗口", true, None, Command::ShowWindow)
            .as_ref(),
    )
    .map_err(|error| error.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|error| error.to_string())?;
    menu.append(builder.item("退出", true, None, Command::Quit).as_ref())
        .map_err(|error| error.to_string())?;
    Ok((menu, builder.commands))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_revision_ignores_traffic_but_tracks_selection_and_configuration() {
        let mut runtime = core::Runtime::default();
        let settings = Values::new();
        let original = revision(&runtime, &settings, 1);
        runtime.statistics.upload_rate = Some(400);
        assert_eq!(revision(&runtime, &settings, 1), original);
        runtime.epoch = 2;
        assert_ne!(revision(&runtime, &settings, 1), original);
        assert_ne!(
            revision(&runtime, &settings, 2),
            revision(&runtime, &settings, 1)
        );
    }
    #[test]
    fn menu_context_tracks_available_actions_and_profile_records() {
        let snapshot = core::Snapshot::default();
        let profiles = Values::new();
        let idle = context_revision(&snapshot, false, &profiles);
        assert_ne!(idle, context_revision(&snapshot, true, &profiles));
        let mut profiles = profiles;
        profiles.insert("4/name".into(), "renamed".into());
        assert_ne!(idle, context_revision(&snapshot, false, &profiles));
    }
    #[test]
    fn native_item_identity_keeps_underscores_and_snapshot_scope() {
        let mut builder = Builder {
            commands: Default::default(),
            revision: 42,
            icons: Default::default(),
            #[cfg(target_os = "macos")]
            title_width: 0.,
        };
        let command = Command::Node {
            group: "a_b".into(),
            node: "c_d".into(),
            generation: 3,
            epoch: 4,
        };
        let first = builder.item("节点", true, Some(true), command.clone());
        let second = builder.item("打开主窗口", true, None, Command::ShowWindow);
        assert_ne!(first.id(), second.id());
        assert_eq!(builder.commands.get(first.id().as_ref()), Some(&command));
    }
}
