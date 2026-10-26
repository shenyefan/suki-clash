use crate::settings::SettingsKind;
use gpui_kit::assets::IconName;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Page {
    Overview,
    Proxies,
    Profiles,
    Connections,
    Rules,
    Override,
    Sniffer,
    Dns,
    Core,
    Resources,
    Logs,
    SysProxy,
    Tun,
    Settings,
}
impl Page {
    pub(super) const ALL: [Self; 14] = [
        Self::Overview,
        Self::Proxies,
        Self::Profiles,
        Self::Connections,
        Self::Rules,
        Self::Override,
        Self::Sniffer,
        Self::Dns,
        Self::Core,
        Self::Resources,
        Self::Logs,
        Self::SysProxy,
        Self::Tun,
        Self::Settings,
    ];
    pub(super) fn settings_kind(self) -> Option<SettingsKind> {
        match self {
            Self::Sniffer => Some(SettingsKind::Sniffer),
            Self::Dns => Some(SettingsKind::Dns),
            Self::Core => Some(SettingsKind::Core),
            Self::Resources => Some(SettingsKind::Resources),
            Self::SysProxy => Some(SettingsKind::SysProxy),
            Self::Tun => Some(SettingsKind::Tun),
            Self::Settings => Some(SettingsKind::General),
            _ => None,
        }
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Proxies => "proxies",
            Self::Profiles => "profiles",
            Self::Connections => "connections",
            Self::Rules => "rules",
            Self::Override => "override",
            Self::Sniffer => SettingsKind::Sniffer.id(),
            Self::Dns => SettingsKind::Dns.id(),
            Self::Core => SettingsKind::Core.id(),
            Self::Resources => SettingsKind::Resources.id(),
            Self::Logs => "logs",
            Self::SysProxy => SettingsKind::SysProxy.id(),
            Self::Tun => SettingsKind::Tun.id(),
            Self::Settings => SettingsKind::General.id(),
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "仪表盘",
            Self::Proxies => "代理组",
            Self::Profiles => "订阅",
            Self::Connections => "连接",
            Self::Rules => "规则",
            Self::Override => "覆写",
            Self::Sniffer => SettingsKind::Sniffer.label(),
            Self::Dns => SettingsKind::Dns.label(),
            Self::Core => SettingsKind::Core.label(),
            Self::Resources => SettingsKind::Resources.label(),
            Self::Logs => "日志",
            Self::SysProxy => SettingsKind::SysProxy.label(),
            Self::Tun => SettingsKind::Tun.label(),
            Self::Settings => SettingsKind::General.label(),
        }
    }
    pub(super) fn icon(self) -> IconName {
        match self {
            Self::Overview => IconName::LayoutDashboard,
            Self::Proxies => IconName::Network,
            Self::Profiles => IconName::Layers,
            Self::Connections => IconName::Activity,
            Self::Rules => IconName::ListFilter,
            Self::Override => IconName::FileCode,
            Self::Sniffer => SettingsKind::Sniffer.icon(),
            Self::Dns => SettingsKind::Dns.icon(),
            Self::Core => SettingsKind::Core.icon(),
            Self::Resources => SettingsKind::Resources.icon(),
            Self::Logs => IconName::ScrollText,
            Self::SysProxy => SettingsKind::SysProxy.icon(),
            Self::Tun => SettingsKind::Tun.icon(),
            Self::Settings => SettingsKind::General.icon(),
        }
    }
}
