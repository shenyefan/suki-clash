/// Stable settings domains shared by storage, workflows and presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsKind {
    Sniffer,
    Dns,
    Core,
    Resources,
    SysProxy,
    Tun,
    General,
}
impl SettingsKind {
    pub(crate) const ALL: [Self; 7] = [
        Self::Sniffer,
        Self::Dns,
        Self::Core,
        Self::Resources,
        Self::SysProxy,
        Self::Tun,
        Self::General,
    ];
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Sniffer => "sniffer",
            Self::Dns => "dns",
            Self::Core => "core",
            Self::Resources => "resources",
            Self::SysProxy => "sysproxy",
            Self::Tun => "tun",
            Self::General => "settings",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Sniffer => "嗅探",
            Self::Dns => "DNS",
            Self::Core => "内核",
            Self::Resources => "外部资源",
            Self::SysProxy => "系统代理",
            Self::Tun => "虚拟网卡",
            Self::General => "设置",
        }
    }
}
