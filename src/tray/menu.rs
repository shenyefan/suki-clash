//! Read-only tray projection. Commands retain the runtime identity they target.
use super::{Command, model};
use crate::{core, integration, storage::Values};
use gpui_kit::{App, assets::IconName};
use std::hash::{Hash, Hasher};

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
            group.icon.hash(&mut hash);
            group.hidden.hash(&mut hash);
            group.now.hash(&mut hash);
            group.all.hash(&mut hash);
        }
    }
    runtime.proxies.provider_error.hash(&mut hash);
    runtime.icons.files.hash(&mut hash);
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
    snapshot.service_state().reason().is_some().hash(&mut hash);
    snapshot
        .profile()
        .map(|profile| (profile.id(), profile.name()))
        .hash(&mut hash);
    busy.hash(&mut hash);
    profiles.hash(&mut hash);
    hash.finish()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Root,
    Modes,
    Groups,
    Nodes(String),
    Profiles,
    More,
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    Page(Page),
    Command(Command),
}
#[derive(Clone)]
pub(super) struct Row {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub icon: Option<IconName>,
    pub image: Option<std::path::PathBuf>,
    pub checked: bool,
    pub enabled: bool,
    pub target: Target,
}
impl Row {
    fn action(
        id: impl Into<String>,
        label: impl Into<String>,
        icon: IconName,
        enabled: bool,
        command: Command,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            icon: Some(icon),
            image: None,
            checked: false,
            enabled,
            target: Target::Command(command),
        }
    }
    fn page(
        id: impl Into<String>,
        label: impl Into<String>,
        icon: IconName,
        enabled: bool,
        page: Page,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            icon: Some(icon),
            image: None,
            checked: false,
            enabled,
            target: Target::Page(page),
        }
    }
}

pub(super) fn rows(
    page: &Page,
    runtime: &core::Runtime,
    snapshot: &core::Snapshot,
    busy: bool,
    settings: &Values,
    cx: &App,
) -> Vec<Row> {
    let ready = snapshot.phase() == core::Phase::Ready && runtime.connected;
    let mut rows = Vec::new();
    let inline_modes = settings.get("TrayInlineModes").is_some_and(|v| v == "true");
    if *page == Page::Modes || (*page == Page::Root && inline_modes) {
        for mode in [
            crate::session::ProxyMode::Rule,
            crate::session::ProxyMode::Global,
            crate::session::ProxyMode::Direct,
        ] {
            let mut row = Row::action(
                format!("mode-{}", mode.label()),
                model::mode_label(Some(mode)),
                match mode {
                    crate::session::ProxyMode::Rule => IconName::ListFilter,
                    crate::session::ProxyMode::Global => IconName::Globe,
                    crate::session::ProxyMode::Direct => IconName::ArrowRight,
                },
                ready && !busy,
                Command::Mode(mode),
            );
            row.checked = runtime.mode == Some(mode) && runtime.connected;
            rows.push(row);
        }
    } else if *page == Page::Root {
        let mut row = Row::page(
            "modes",
            "出站模式",
            match runtime.mode {
                Some(crate::session::ProxyMode::Rule) => IconName::ListFilter,
                Some(crate::session::ProxyMode::Direct) => IconName::ArrowRight,
                _ => IconName::Globe,
            },
            !busy,
            Page::Modes,
        );
        row.detail = model::mode_label(runtime.connected.then_some(runtime.mode).flatten()).into();
        rows.push(row);
    }
    if *page == Page::Groups && ready {
        for name in model::groups(&runtime.proxies, runtime.mode) {
            let mut row = Row::page(
                format!("group-{name}"),
                &name,
                IconName::Network,
                !busy,
                Page::Nodes(name.clone()),
            );
            row.detail = runtime.proxies.selected(&name).unwrap_or("未选择").into();
            row.icon = None;
            row.image = runtime
                .proxies
                .entries
                .get(&name)
                .and_then(|group| runtime.icons.files.get(group.icon.trim()))
                .cloned();
            rows.push(row);
        }
    }
    if *page == Page::Root {
        let mut profiles = Row::page("profiles", "订阅", IconName::Layers, !busy, Page::Profiles);
        profiles.detail = snapshot
            .profile()
            .map(|p| p.name())
            .unwrap_or("未选择")
            .into();
        rows.push(profiles);
        rows.push(Row::page(
            "groups",
            "代理组",
            IconName::Network,
            !busy,
            Page::Groups,
        ));
    }
    match page {
        Page::Nodes(name) => {
            if let Some(group) = runtime.proxies.entries.get(name)
                && let Some(members) = runtime.proxies.members.get(name)
            {
                for member in members {
                    let key = runtime.proxies.measurement_key(name, member);
                    let mut row = Row::action(
                        format!("node-{name:?}-{:?}-{}", member.name, member.occurrence),
                        &member.name,
                        IconName::Globe,
                        ready
                            && !busy
                            && member.reason.is_none()
                            && matches!(group.kind.as_str(), "Selector" | "URLTest" | "Fallback"),
                        Command::Node {
                            group: name.clone(),
                            node: member.name.clone(),
                            generation: runtime.generation,
                            epoch: runtime.epoch,
                        },
                    );
                    row.icon = None;
                    row.checked = runtime.proxies.selected(name) == Some(member.name.as_str());
                    row.detail = match runtime.proxies.measurements.get(&(name.clone(), key)) {
                        Some(Ok(0))
                        | Some(Err(crate::proxies::runtime::MeasurementError::Timeout)) => {
                            "超时".into()
                        }
                        Some(Ok(ms)) => format!("{ms} ms"),
                        Some(Err(crate::proxies::runtime::MeasurementError::Failed(_))) => {
                            "失败".into()
                        }
                        _ => String::new(),
                    };
                    rows.push(row);
                }
            }
        }
        Page::Profiles => {
            if let Some((documents, _)) =
                crate::profiles::store::load_values(integration::read("profile-index", cx))
            {
                for doc in documents {
                    let mut row = Row::action(
                        format!("profile-{}", doc.id),
                        doc.name,
                        IconName::File,
                        !busy,
                        Command::Profile(doc.id),
                    );
                    row.checked = snapshot.profile().is_some_and(|p| p.id() == doc.id);
                    rows.push(row);
                }
            }
        }
        Page::More => {
            rows.push(Row::action(
                "close-connections",
                "关闭全部连接",
                IconName::X,
                ready && !busy,
                Command::CloseConnections {
                    generation: runtime.generation,
                    epoch: runtime.epoch,
                },
            ));
            rows.push(Row::action(
                "restart-core",
                "重启内核",
                IconName::RefreshCw,
                !busy,
                Command::RestartCore,
            ));
            rows.push(Row::action(
                "restart-program",
                "重启程序",
                IconName::RefreshCw,
                true,
                Command::RestartProgram,
            ));
        }
        Page::Root => {
            let system = snapshot.system_proxy().enabled();
            let mut proxy = Row::action(
                "system-proxy",
                "系统代理",
                IconName::Globe,
                !busy,
                Command::SystemProxy,
            );
            proxy.checked = system == Some(true);
            if system.is_none() {
                proxy.detail = "待确认".into();
            }
            rows.push(proxy);
            let mut tun = Row::action("tun", "虚拟网卡", IconName::Network, !busy, Command::Tun);
            tun.checked = snapshot.tun() == Some(true);
            if snapshot.tun().is_none() {
                tun.detail = "待确认".into();
            }
            rows.push(tun);
            rows.push(Row::action(
                "environment",
                "复制终端代理命令",
                IconName::Copy,
                true,
                Command::CopyEnvironment,
            ));
            rows.push(Row::page(
                "more",
                "更多操作",
                IconName::Settings,
                true,
                Page::More,
            ));
            rows.push(Row::action(
                "main",
                "打开主窗口",
                IconName::AppWindow,
                true,
                Command::ShowWindow,
            ));
            rows.push(Row::action(
                "quit",
                "退出",
                IconName::LogOut,
                true,
                Command::Quit,
            ));
        }
        _ => {}
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_revision_ignores_traffic_but_tracks_runtime_identity() {
        let mut runtime = core::Runtime::default();
        let settings = Values::new();
        let original = revision(&runtime, &settings, 1);
        runtime.statistics.upload_rate = Some(400);
        assert_eq!(revision(&runtime, &settings, 1), original);
        runtime.epoch = 2;
        assert_ne!(revision(&runtime, &settings, 1), original);
    }
}
