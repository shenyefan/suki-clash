//! Stable persisted identifiers, kept separate from translated presentation labels.
use crate::storage::Values;
fn options(key: &str) -> &'static [(&'static str, &'static str)] {
    match key {
        "SystemProxyModeIndex" => &[("手动", "manual"), ("PAC", "pac")],
        "StackIndex" => &[
            ("System", "system"),
            ("gVisor", "gvisor"),
            ("Mixed", "mixed"),
            ("Mips", "mips"),
        ],
        "StartPage" => &[
            ("仪表盘", "dashboard"),
            ("代理组", "proxies"),
            ("订阅", "profiles"),
            ("连接", "connections"),
            ("规则", "rules"),
            ("日志", "logs"),
            ("设置", "settings"),
        ],
        "TrayEvent" => &[
            ("显示主窗口", "show_window"),
            ("显示托盘菜单", "show_menu"),
            ("切换系统代理", "toggle_system_proxy"),
            ("切换虚拟网卡", "toggle_tun"),
            ("禁用", "disabled"),
        ],
        "NoticePosition" => &[
            ("顶部居中", "top_center"),
            ("右下角", "bottom_right"),
            ("右上角", "top_right"),
            ("左下角", "bottom_left"),
            ("左上角", "top_left"),
        ],
        "AutoLogClean" => &[
            ("不清理", "never"),
            ("保留 1 天", "1d"),
            ("保留 7 天", "7d"),
            ("保留 30 天", "30d"),
            ("保留 90 天", "90d"),
        ],
        "FakeIpFilterModeIndex" => &[
            ("黑名单（blacklist）", "blacklist"),
            ("白名单（whitelist）", "whitelist"),
            ("规则（rule）", "rule"),
        ],
        "CoreReleaseIndex" => &[
            ("最新版", "latest"),
            ("预览版", "preview"),
            ("Smart", "smart"),
            ("指定版本", "specific"),
        ],
        "LogLevelIndex" => &[
            ("静默（silent）", "silent"),
            ("错误（error）", "error"),
            ("警告（warning）", "warning"),
            ("信息（info）", "info"),
            ("调试（debug）", "debug"),
        ],
        "FindProcessIndex" => &[
            ("关闭（off）", "off"),
            ("严格（strict）", "strict"),
            ("始终（always）", "always"),
        ],
        "ThemeIndex" => &[("跟随系统", "system"), ("浅色", "light"), ("深色", "dark")],
        "BackdropIndex" => &[
            ("原生磨砂", "native"),
            ("纯色", "solid"),
            ("Acrylic", "acrylic"),
        ],
        "AppLogLevel" => &[
            ("Trace", "trace"),
            ("Debug", "debug"),
            ("Info", "info"),
            ("Warn", "warn"),
            ("Error", "error"),
            ("Silent", "silent"),
        ],
        "EnvTypeIndex" => &[
            ("PowerShell", "powershell"),
            ("CMD", "cmd"),
            ("Bash", "bash"),
            ("Fish", "fish"),
            ("Nushell", "nushell"),
        ],
        _ => &[],
    }
}
pub(crate) fn encode(mut values: Values) -> Values {
    for (key, value) in &mut values {
        if let Some((_, id)) = options(key).iter().find(|(label, _)| *label == value) {
            *value = (*id).into();
        }
    }
    values
}
pub(crate) fn decode(mut values: Values) -> Values {
    for (key, value) in &mut values {
        if let Some((label, _)) = options(key).iter().find(|(_, id)| *id == value) {
            *value = (*label).into();
        }
    }
    values
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_ids_roundtrip_without_changing_user_text() {
        let original = Values::from([
            ("ThemeIndex".into(), "深色".into()),
            ("CoreReleaseIndex".into(), "Smart".into()),
            ("WebUiName".into(), "我的面板".into()),
        ]);
        let encoded = encode(original.clone());
        assert_eq!(encoded["ThemeIndex"], "dark");
        assert_eq!(encoded["CoreReleaseIndex"], "smart");
        assert_eq!(encoded["WebUiName"], "我的面板");
        assert_eq!(decode(encoded), original);
    }
}
