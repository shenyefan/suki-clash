// Settings contracts; runtime interactions reference the local Clash Verge Rev source.
use super::SettingsKind;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum FieldKind {
    Toggle,
    Text,
    Secret,
    Multiline,
    Number,
    Select,
    ReadOnly,
}
#[derive(Clone)]
pub(crate) struct FieldSpec {
    pub(crate) key: &'static str,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) kind: FieldKind,
    pub(crate) default: &'static str,
    pub(crate) options: &'static [&'static str],
    pub(crate) actions: &'static [&'static str],
}
pub(crate) struct SectionSpec {
    pub(crate) title: &'static str,
    pub(crate) fields: Vec<FieldSpec>,
}

pub(crate) fn default_panels() -> Vec<(u64, String, String)> {
    [
        (1, "MetaCubeXD", "https://metacubex.github.io/metacubexd/#/setup?http=true&hostname=%host&port=%port&secret=%secret"),
        (2, "YACD", "https://yacd.metacubex.one/?hostname=%host&port=%port&secret=%secret"),
        (3, "Zashboard", "https://board.zash.run.place/#/setup?http=true&hostname=%host&port=%port&secret=%secret"),
    ]
    .into_iter()
    .map(|(id, name, url)| (id, name.into(), url.into()))
    .collect()
}

pub(crate) fn panels_from_values(values: &BTreeMap<String, String>) -> Vec<(u64, String, String)> {
    if !values.contains_key("WebUiConfigured") {
        return default_panels();
    }
    values
        .iter()
        .filter_map(|(key, name)| {
            let id = key
                .strip_prefix("WebUi.")?
                .strip_suffix(".name")?
                .parse::<u64>()
                .ok()?;
            let url = values.get(&format!("WebUi.{id}.url"))?;
            Some((id, name.clone(), url.clone()))
        })
        .collect()
}

pub(crate) fn store_panels(
    panels: &[(u64, String, String)],
    values: &mut BTreeMap<String, String>,
) {
    values.insert("WebUiConfigured".into(), "true".into());
    for (id, name, url) in panels {
        values.insert(format!("WebUi.{id}.name"), name.clone());
        values.insert(format!("WebUi.{id}.url"), url.clone());
    }
}

pub(crate) fn valid_web_url(value: &str) -> bool {
    value
        .trim()
        .split_once("://")
        .is_some_and(|(scheme, rest)| {
            matches!(scheme, "http" | "https")
                && !rest.is_empty()
                && !rest.starts_with('/')
                && !rest.chars().any(char::is_whitespace)
        })
}

pub(crate) fn expand_panel_url(url: &str, address: &str, secret: &str) -> Result<String, String> {
    if !valid_web_url(url) {
        return Err("面板地址需为 HTTP / HTTPS 地址".into());
    }
    let (host, port) = address.rsplit_once(':').ok_or("控制器地址需包含端口")?;
    if port.parse::<u16>().ok().filter(|port| *port > 0).is_none() {
        return Err("控制器端口无效".into());
    }
    let host = if matches!(host, "0.0.0.0" | "::" | "[::]") {
        "127.0.0.1"
    } else {
        host
    };
    let encoded: String = secret
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    Ok(url
        .replace("%host", host)
        .replace("%port", port)
        .replace("%secret", &encoded))
}

pub(crate) fn sections(kind: SettingsKind) -> Vec<SectionSpec> {
    match kind {
        SettingsKind::SysProxy => proxy_sections(),
        SettingsKind::Tun => tun_sections(),
        SettingsKind::General => {
            let mut sections = original_sections(kind);
            sections.extend(general_extensions());
            sections
        }
        _ => original_sections(kind),
    }
}

fn proxy_sections() -> Vec<SectionSpec> {
    vec![SectionSpec {
        title: "系统代理",
        fields: vec![
            FieldSpec {
                key: "IsSystemProxyEnabled",
                label: "启用系统代理",
                description: "",
                kind: FieldKind::Toggle,
                default: "false",
                options: &[],
                actions: &[],
            },
            FieldSpec {
                key: "SystemProxyHost",
                label: "代理主机",
                description: "",
                kind: FieldKind::Text,
                default: "127.0.0.1",
                options: &[],
                actions: &[],
            },
            FieldSpec {
                key: "SystemProxyModeIndex",
                label: "代理模式",
                description: "",
                kind: FieldKind::Select,
                default: "手动",
                options: &["手动", "PAC"],
                actions: &[],
            },
            FieldSpec {
                key: "EnableProxyGuard",
                label: "代理守卫",
                description: "",
                kind: FieldKind::Toggle,
                default: "false",
                options: &[],
                actions: &[],
            },
            FieldSpec {
                key: "ProxyGuardDuration",
                label: "检查间隔（秒）",
                description: "",
                kind: FieldKind::Number,
                default: "10",
                options: &[],
                actions: &[],
            },
            FieldSpec {
                key: "BypassList",
                label: "绕过地址",
                description: "每行一个域名、IP 或 CIDR",
                kind: FieldKind::Multiline,
                default: default_bypass(),
                options: &[],
                actions: &[],
            },
            FieldSpec {
                key: "EnableBypassCheck",
                label: "绕过校验",
                description: "",
                kind: FieldKind::Toggle,
                default: "true",
                options: &[],
                actions: &[],
            },
            FieldSpec {
                key: "UseDefaultBypass",
                label: "使用默认绕过",
                description: "",
                kind: FieldKind::Toggle,
                default: "true",
                options: &[],
                actions: &[],
            },
            FieldSpec {
                key: "PacScript",
                label: "PAC 脚本",
                description: "支持 %proxy_host% 与 %mixed-port% 占位符",
                kind: FieldKind::Multiline,
                default: "function FindProxyForURL(url, host) {\n  return \"PROXY %proxy_host%:%mixed-port%; SOCKS5 %proxy_host%:%mixed-port%; DIRECT;\";\n}",
                options: &[],
                actions: &[],
            },
        ],
    }]
}
fn tun_sections() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            title: "虚拟网卡",
            fields: vec![
                FieldSpec {
                    key: "IsTunEnabled",
                    label: "启用虚拟网卡",
                    description: "与标题栏开关保持同步",
                    kind: FieldKind::Toggle,
                    default: "false",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "TunService",
                    label: "后台服务",
                    description: "虚拟网卡需要系统网络权限",
                    kind: FieldKind::ReadOnly,
                    default: "",
                    options: &[],
                    actions: &["安装服务", "修复服务", "卸载服务"],
                },
                FieldSpec {
                    key: "StackIndex",
                    label: "协议栈",
                    description: "",
                    kind: FieldKind::Select,
                    default: "Mips",
                    options: &["System", "gVisor", "Mixed", "Mips"],
                    actions: &[],
                },
                FieldSpec {
                    key: "DeviceName",
                    label: "设备名称",
                    description: "留空使用系统默认名称",
                    kind: FieldKind::Text,
                    default: if cfg!(target_os = "macos") {
                        "utun1024"
                    } else {
                        "Mihomo"
                    },
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "Mtu",
                    label: "MTU",
                    description: "允许范围 576–65535",
                    kind: FieldKind::Number,
                    default: "1500",
                    options: &[],
                    actions: &[],
                },
            ],
        },
        SectionSpec {
            title: "路由",
            fields: vec![
                FieldSpec {
                    key: "AutoRoute",
                    label: "自动路由",
                    description: "",
                    kind: FieldKind::Toggle,
                    default: "true",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "AutoRedirect",
                    label: "自动重定向",
                    description: "仅 Linux 可用，需要启用自动路由",
                    kind: FieldKind::Toggle,
                    default: "false",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "StrictRoute",
                    label: "严格路由",
                    description: "",
                    kind: FieldKind::Toggle,
                    default: "false",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "AutoDetectInterface",
                    label: "自动选择接口",
                    description: "",
                    kind: FieldKind::Toggle,
                    default: "true",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "RouteExcludeAddress",
                    label: "路由排除地址",
                    description: "每行一个 IPv4 / IPv6 地址或 CIDR",
                    kind: FieldKind::Multiline,
                    default: "",
                    options: &[],
                    actions: &[],
                },
            ],
        },
        SectionSpec {
            title: "DNS",
            fields: vec![FieldSpec {
                key: "DnsHijack",
                label: "DNS 劫持",
                description: "每行一个，例如 any:53、tcp://any:53",
                kind: FieldKind::Multiline,
                default: "any:53",
                options: &[],
                actions: &[],
            }],
        },
        SectionSpec {
            title: "服务与权限",
            fields: vec![FieldSpec {
                key: "TunAction1",
                label: "防火墙规则",
                description: "仅适用于 Windows",
                kind: FieldKind::ReadOnly,
                default: "",
                options: &[],
                actions: &["重建防火墙规则"],
            }],
        },
    ]
}

fn general_extensions() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            title: "启动与行为",
            fields: vec![
                FieldSpec {
                    key: "StartPage",
                    label: "启动页面",
                    description: "",
                    kind: FieldKind::Select,
                    default: "仪表盘",
                    options: &["仪表盘", "代理组", "订阅", "连接", "规则", "日志", "设置"],
                    actions: &[],
                },
                FieldSpec {
                    key: "TrayEvent",
                    label: "托盘点击行为",
                    description: "",
                    kind: FieldKind::Select,
                    default: "显示托盘菜单",
                    options: &[
                        "显示主窗口",
                        "显示托盘菜单",
                        "切换系统代理",
                        "切换虚拟网卡",
                        "禁用",
                    ],
                    actions: &[],
                },
            ],
        },
        SectionSpec {
            title: "通知",
            fields: vec![FieldSpec {
                key: "NoticePosition",
                label: "通知位置",
                description: "",
                kind: FieldKind::Select,
                default: "顶部居中",
                options: &["顶部居中", "右下角", "右上角", "左下角", "左上角"],
                actions: &[],
            }],
        },
        SectionSpec {
            title: "托盘",
            fields: vec![
                FieldSpec {
                    key: "EnableTraySpeed",
                    label: "显示托盘网速",
                    description: "",
                    kind: FieldKind::Toggle,
                    default: "false",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "TrayProxyGroups",
                    label: "托盘代理组布局",
                    description: "",
                    kind: FieldKind::Select,
                    default: "内联",
                    options: &["默认", "内联", "隐藏"],
                    actions: &[],
                },
                FieldSpec {
                    key: "TrayInlineModes",
                    label: "托盘内联显示出站模式",
                    description: "",
                    kind: FieldKind::Toggle,
                    default: "false",
                    options: &[],
                    actions: &[],
                },
            ],
        },
        SectionSpec {
            title: "测速与日志",
            fields: vec![
                FieldSpec {
                    key: "EnableAutoDelay",
                    label: "自动检测延迟",
                    description: "",
                    kind: FieldKind::Toggle,
                    default: "false",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "AutoDelayInterval",
                    label: "检测间隔（分钟）",
                    description: "",
                    kind: FieldKind::Number,
                    default: "5",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "AppLogLevel",
                    label: "应用日志级别",
                    description: "",
                    kind: FieldKind::Select,
                    default: "Info",
                    options: &["Trace", "Debug", "Info", "Warn", "Error", "Silent"],
                    actions: &[],
                },
                FieldSpec {
                    key: "AppLogMaxSize",
                    label: "单个日志大小（KB）",
                    description: "",
                    kind: FieldKind::Number,
                    default: "128",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "AppLogMaxCount",
                    label: "日志文件数量",
                    description: "",
                    kind: FieldKind::Number,
                    default: "8",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "AutoLogClean",
                    label: "日志清理",
                    description: "",
                    kind: FieldKind::Select,
                    default: "不清理",
                    options: &[
                        "不清理",
                        "保留 1 天",
                        "保留 7 天",
                        "保留 30 天",
                        "保留 90 天",
                    ],
                    actions: &[],
                },
            ],
        },
        SectionSpec {
            title: "工具与更新",
            fields: vec![
                FieldSpec {
                    key: "RuntimeConfig",
                    label: "运行配置",
                    description: "",
                    kind: FieldKind::ReadOnly,
                    default: "",
                    options: &[],
                    actions: &["查看运行配置"],
                },
                FieldSpec {
                    key: "ConfigDirectories",
                    label: "目录",
                    description: "",
                    kind: FieldKind::ReadOnly,
                    default: "",
                    options: &[],
                    actions: &["打开配置目录", "打开内核目录", "打开日志目录"],
                },
                FieldSpec {
                    key: "CheckUpdates",
                    label: "应用更新",
                    description: "",
                    kind: FieldKind::ReadOnly,
                    default: "",
                    options: &[],
                    actions: &["检查应用更新"],
                },
                FieldSpec {
                    key: "DeveloperTools",
                    label: "诊断",
                    description: "",
                    kind: FieldKind::ReadOnly,
                    default: "",
                    options: &[],
                    actions: &["导出诊断信息"],
                },
                FieldSpec {
                    key: "AppVersion",
                    label: "应用版本",
                    description: "",
                    kind: FieldKind::ReadOnly,
                    default: "",
                    options: &[],
                    actions: &[],
                },
            ],
        },
    ]
}

fn original_sections(kind: SettingsKind) -> Vec<SectionSpec> {
    match kind {
        SettingsKind::Sniffer => vec![
            SectionSpec {
                title: "基础设置",
                fields: vec![
                    FieldSpec {
                        key: "SnifferOverrideEnabled",
                        label: "覆写嗅探配置",
                        description: "关闭时使用订阅中的嗅探配置",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "SnifferEnabled",
                        label: "启用嗅探",
                        description: "仅在“覆写嗅探配置”开启时生效",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "OverrideDestination",
                        label: "覆盖目标地址",
                        description: "使用嗅探结果覆盖连接目标",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "ForceDnsMapping",
                        label: "强制 DNS 映射",
                        description: "对 redir-host 类型的连接强制执行嗅探",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "ParsePureIp",
                        label: "嗅探纯 IP 连接",
                        description: "对仅含 IP 地址的连接也尝试识别域名",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "协议端口",
                fields: vec![
                    FieldSpec {
                        key: "HttpPorts",
                        label: "HTTP 端口",
                        description: "逗号分隔端口或范围，例如 80,8080-8880；留空不嗅探此协议",
                        kind: FieldKind::Text,
                        default: "80",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "TlsPorts",
                        label: "TLS 端口",
                        description: "逗号分隔端口或范围，例如 80,8080-8880；留空不嗅探此协议",
                        kind: FieldKind::Text,
                        default: "443",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "QuicPorts",
                        label: "QUIC 端口",
                        description: "逗号分隔端口或范围，例如 80,8080-8880；留空不嗅探此协议",
                        kind: FieldKind::Text,
                        default: "443",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "域名过滤",
                fields: vec![
                    FieldSpec {
                        key: "SkipDomain",
                        label: "跳过域名",
                        description: "每行一条域名规则",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "ForceDomain",
                        label: "强制嗅探域名",
                        description: "每行一条域名规则",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "SkipDstAddress",
                        label: "跳过目标地址",
                        description: "每行一个 IP 地址或 CIDR",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "SkipSrcAddress",
                        label: "跳过来源地址",
                        description: "每行一个 IP 地址或 CIDR",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
        ],
        SettingsKind::Dns => vec![
            SectionSpec {
                title: "基础设置",
                fields: vec![
                    FieldSpec {
                        key: "DnsOverrideEnabled",
                        label: "使用全局 DNS 配置",
                        description: "关闭时使用订阅中的 DNS 和 Hosts 配置",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DnsEnabled",
                        label: "启用 DNS",
                        description: "仅在全局 DNS 配置开启时生效",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DnsListen",
                        label: "监听地址",
                        description: "DNS 服务监听的地址与端口，例如 127.0.0.1:53",
                        kind: FieldKind::Text,
                        default: "127.0.0.1:53",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "EnhancedModeIndex",
                        label: "增强模式",
                        description: "fake-ip 性能更好，redir-host 更兼容",
                        kind: FieldKind::Select,
                        default: "fake-ip",
                        options: &["fake-ip", "redir-host", "normal"],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "UseSystemHosts",
                        label: "系统 Hosts",
                        description: "读取系统 Hosts 文件",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "UseHosts",
                        label: "自定义 Hosts",
                        description: "使用 Hosts 分类中的自定义映射",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DnsIpv6",
                        label: "IPv6",
                        description: "启用 IPv6 DNS 解析",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "PreferH3",
                        label: "优先 HTTP/3",
                        description: "优先使用 HTTP/3 连接 DoH 服务器",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DirectNameserverFollowPolicy",
                        label: "直连 DNS 遵循解析策略",
                        description: "直连请求使用 nameserver-policy 中的匹配结果",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "RespectRules",
                        label: "尊重规则",
                        description: "DNS 请求遵循代理规则",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "Fake-IP",
                fields: vec![
                    FieldSpec {
                        key: "FakeIpRange",
                        label: "Fake-IP 地址池",
                        description: "用于分配 Fake-IP 的地址段",
                        kind: FieldKind::Text,
                        default: "198.18.0.1/16",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "FakeIpRange6",
                        label: "IPv6 Fake-IP 地址池",
                        description: "用于分配 IPv6 Fake-IP 的地址段",
                        kind: FieldKind::Text,
                        default: "fdfe:dcba:9876::1/64",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "FakeIpFilter",
                        label: "Fake-IP 过滤",
                        description: "Fake-IP 匹配规则，每行一条",
                        kind: FieldKind::Multiline,
                        default: "*.lan\n*.local\nlocalhost",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "FakeIpFilterModeIndex",
                        label: "过滤模式",
                        description: "选择过滤规则的匹配方式",
                        kind: FieldKind::Select,
                        default: "黑名单（blacklist）",
                        options: &["黑名单（blacklist）", "白名单（whitelist）", "规则（rule）"],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "DNS 服务器",
                fields: vec![
                    FieldSpec {
                        key: "Nameserver",
                        label: "主 DNS",
                        description: "常规域名解析，每行一个",
                        kind: FieldKind::Multiline,
                        default: "https://dns.alidns.com/dns-query\nhttps://doh.pub/dns-query",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "Fallback",
                        label: "备用 DNS",
                        description: "匹配解析策略中的筛选条件时使用，每行一个",
                        kind: FieldKind::Multiline,
                        default: "https://1.1.1.1/dns-query",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DefaultNameserver",
                        label: "引导 DNS",
                        description: "用于解析 DNS 服务器域名，仅支持 IP 地址",
                        kind: FieldKind::Multiline,
                        default: "223.5.5.5\n119.29.29.29",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DirectNameserver",
                        label: "直连 DNS",
                        description: "用于直连请求，每行一个",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "ProxyServerNameserver",
                        label: "代理节点 DNS",
                        description: "用于解析代理节点域名，每行一个",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "备用 DNS 筛选",
                fields: vec![
                    FieldSpec {
                        key: "FallbackGeoIp",
                        label: "GeoIP",
                        description: "根据 IP 所属地区决定是否使用备用 DNS",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "FallbackGeoIpCode",
                        label: "地区代码",
                        description: "例如 CN",
                        kind: FieldKind::Text,
                        default: "CN",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "FallbackIpCidr",
                        label: "IP 地址段",
                        description: "匹配后使用备用 DNS，每行一个 CIDR",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "FallbackDomain",
                        label: "域名规则",
                        description: "匹配后使用备用 DNS，每行一条",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "解析策略",
                fields: vec![FieldSpec {
                    key: "NameserverPolicy",
                    label: "域名解析策略",
                    description: "每行一条：域名=DNS；多个 DNS 使用分号分隔",
                    kind: FieldKind::Multiline,
                    default: "",
                    options: &[],
                    actions: &[],
                }],
            },
            SectionSpec {
                title: "Hosts",
                fields: vec![FieldSpec {
                    key: "Hosts",
                    label: "自定义 Hosts",
                    description: "每行一条：domain=ip 或 domain:ip",
                    kind: FieldKind::Multiline,
                    default: "",
                    options: &[],
                    actions: &[],
                }],
            },
        ],
        SettingsKind::Core => vec![
            SectionSpec {
                title: "状态",
                fields: vec![
                    FieldSpec {
                        key: "CoreAction0",
                        label: "运行状态",
                        description: "",
                        kind: FieldKind::ReadOnly,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "CoreAction1",
                        label: "当前版本",
                        description: "",
                        kind: FieldKind::ReadOnly,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "版本管理",
                fields: vec![
                    FieldSpec {
                        key: "CoreReleaseIndex",
                        label: "发行通道",
                        description: "选择后点击“应用”，下载并启动对应内核",
                        kind: FieldKind::Select,
                        default: "最新版",
                        options: &["最新版", "预览版", "Smart", "指定版本"],
                        actions: &["应用所选通道"],
                    },
                    FieldSpec {
                        key: "CoreSpecificVersion",
                        label: "指定版本",
                        description: "版本标签，例如 v1.19.13",
                        kind: FieldKind::Select,
                        default: "",
                        options: &[""],
                        actions: &["刷新版本列表"],
                    },
                ],
            },
            SectionSpec {
                title: "内核维护",
                fields: vec![
                    FieldSpec {
                        key: "CoreUwp",
                        label: "UWP 应用代理",
                        description: "选择允许访问本机代理的应用",
                        kind: FieldKind::ReadOnly,
                        default: "",
                        options: &[],
                        actions: &["选择应用"],
                    },
                    FieldSpec {
                        key: "CoreValidate",
                        label: "运行配置",
                        description: "使用当前内核检查候选配置",
                        kind: FieldKind::ReadOnly,
                        default: "",
                        options: &[],
                        actions: &["校验配置"],
                    },
                    FieldSpec {
                        key: "CoreAction6",
                        label: "Fake-IP 缓存",
                        description: "清空内核 DNS 的 Fake-IP 缓存",
                        kind: FieldKind::ReadOnly,
                        default: "",
                        options: &[],
                        actions: &["清空 Fake-IP 缓存"],
                    },
                ],
            },
            SectionSpec {
                title: "监听端口",
                fields: vec![
                    FieldSpec {
                        key: "MixedPort",
                        label: "混合端口",
                        description: "HTTP 与 SOCKS 共用；0 表示关闭",
                        kind: FieldKind::Number,
                        default: "7897",
                        options: &[],
                        actions: &["随机端口"],
                    },
                    FieldSpec {
                        key: "SocksPort",
                        label: "SOCKS 端口",
                        description: "0 表示关闭",
                        kind: FieldKind::Number,
                        default: "0",
                        options: &[],
                        actions: &["随机端口"],
                    },
                    FieldSpec {
                        key: "HttpPort",
                        label: "HTTP 端口",
                        description: "0 表示关闭",
                        kind: FieldKind::Number,
                        default: "0",
                        options: &[],
                        actions: &["随机端口"],
                    },
                    FieldSpec {
                        key: "RedirPort",
                        label: "重定向端口",
                        description: "Windows 通常无需开启；0 表示关闭",
                        kind: FieldKind::Number,
                        default: "0",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "TproxyPort",
                        label: "TProxy 端口",
                        description: "Windows 通常无需开启；0 表示关闭",
                        kind: FieldKind::Number,
                        default: "0",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "隧道",
                fields: vec![FieldSpec {
                    key: "TunnelAction",
                    label: "TCP / UDP 隧道",
                    description: "将本地端口转发到目标，可指定代理组或节点",
                    kind: FieldKind::ReadOnly,
                    default: "",
                    options: &[],
                    actions: &["管理隧道"],
                }],
            },
            SectionSpec {
                title: "外部控制",
                fields: vec![
                    FieldSpec {
                        key: "EnableExternalController",
                        label: "启用外部控制",
                        description: "本应用无需开启；开启后其他程序可通过 HTTP API 控制内核。修改确认后重启生效。",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "ExternalController",
                        label: "控制器地址",
                        description: "外部控制 API 的监听地址",
                        kind: FieldKind::Text,
                        default: "127.0.0.1:9090",
                        options: &[],
                        actions: &["复制地址"],
                    },
                    FieldSpec {
                        key: "Secret",
                        label: "访问密钥",
                        description: "用于保护外部控制 API",
                        kind: FieldKind::Secret,
                        default: "",
                        options: &[],
                        actions: &["生成密钥", "复制密钥"],
                    },
                    FieldSpec {
                        key: "CorsPrivateNetwork",
                        label: "允许私有网络访问",
                        description: "CORS allow-private-network",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "CorsOrigins",
                        label: "CORS 允许来源",
                        description: "每行一个 HTTP/HTTPS 来源，支持 *；离开编辑框后应用",
                        kind: FieldKind::Multiline,
                        default: "https://metacubex.github.io\nhttps://yacd.metacubex.one\nhttps://board.zash.run.place",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "WebUI",
                fields: vec![FieldSpec {
                    key: "NewWebUiName",
                    label: "新增面板",
                    description: "URL 可使用 %host、%port 和 %secret 变量",
                    kind: FieldKind::Text,
                    default: "名称",
                    options: &[],
                    actions: &["添加 WebUI"],
                }],
            },
            SectionSpec {
                title: "访问控制",
                fields: vec![
                    FieldSpec {
                        key: "AllowLan",
                        label: "允许局域网",
                        description: "允许其他设备连接本机代理",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &["网卡信息"],
                    },
                    FieldSpec {
                        key: "LanAllowedIps",
                        label: "允许的地址",
                        description: "仅允许这些地址访问，每行一个 CIDR",
                        kind: FieldKind::Multiline,
                        default: crate::configuration::Preferences::DEFAULT_ALLOWED_IPS,
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "LanDisallowedIps",
                        label: "禁止的地址",
                        description: "拒绝这些地址访问，每行一个 CIDR",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "Authentication",
                        label: "用户认证",
                        description: "每行一个 user:password",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "SkipAuthPrefixes",
                        label: "免认证地址",
                        description: "这些地址无需认证，每行一个 CIDR",
                        kind: FieldKind::Multiline,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "基础参数",
                fields: vec![
                    FieldSpec {
                        key: "Ipv6",
                        label: "IPv6",
                        description: "启用 IPv6 支持",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "UnifiedDelay",
                        label: "统一延迟",
                        description: "统一计算节点延迟",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "TcpConcurrent",
                        label: "TCP 并发",
                        description: "并发尝试多个目标 IP",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "StoreSelected",
                        label: "记住节点选择",
                        description: "重启后保留代理组选择",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "StoreFakeIp",
                        label: "保留 Fake-IP 映射",
                        description: "重启内核后继续使用现有映射",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "LogLevelIndex",
                        label: "日志级别",
                        description: "控制内核日志的详细程度",
                        kind: FieldKind::Select,
                        default: "信息（info）",
                        options: &[
                            "静默（silent）",
                            "错误（error）",
                            "警告（warning）",
                            "信息（info）",
                            "调试（debug）",
                        ],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "FindProcessIndex",
                        label: "进程查找模式",
                        description: "控制连接的进程识别方式",
                        kind: FieldKind::Select,
                        default: "严格（strict）",
                        options: &["关闭（off）", "严格（strict）", "始终（always）"],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "日志维护",
                fields: vec![
                    FieldSpec {
                        key: "MaxLogDays",
                        label: "保留天数",
                        description: "程序日志与内核日志",
                        kind: FieldKind::Number,
                        default: "7",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "MaxLogFileSizeMb",
                        label: "单文件大小（MB）",
                        description: "达到限制后自动创建新日志文件",
                        kind: FieldKind::Number,
                        default: "10",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
        ],
        SettingsKind::Resources => vec![SectionSpec {
            title: "GeoData",
            fields: vec![
                FieldSpec {
                    key: "GeoIpUrl",
                    label: "GeoIP 数据库",
                    description: "",
                    kind: FieldKind::Text,
                    default: crate::configuration::resources::Database::GeoIp.default_url(),
                    options: &[],
                    actions: &["更新 GeoIP"],
                },
                FieldSpec {
                    key: "GeoSiteUrl",
                    label: "GeoSite 数据库",
                    description: "",
                    kind: FieldKind::Text,
                    default: crate::configuration::resources::Database::GeoSite.default_url(),
                    options: &[],
                    actions: &["更新 GeoSite"],
                },
                FieldSpec {
                    key: "MmdbUrl",
                    label: "MMDB 数据库",
                    description: "",
                    kind: FieldKind::Text,
                    default: crate::configuration::resources::Database::Country.default_url(),
                    options: &[],
                    actions: &["更新 MMDB"],
                },
                FieldSpec {
                    key: "AsnUrl",
                    label: "ASN 数据库",
                    description: "",
                    kind: FieldKind::Text,
                    default: crate::configuration::resources::Database::Asn.default_url(),
                    options: &[],
                    actions: &["更新 ASN"],
                },
                FieldSpec {
                    key: "GeoMode",
                    label: "GeoData 格式",
                    description: "",
                    kind: FieldKind::Select,
                    default: "db",
                    options: &["db", "dat"],
                    actions: &[],
                },
                FieldSpec {
                    key: "GeoAutoUpdate",
                    label: "自动更新",
                    description: "",
                    kind: FieldKind::Toggle,
                    default: "false",
                    options: &[],
                    actions: &[],
                },
                FieldSpec {
                    key: "GeoUpdateIntervalValue",
                    label: "更新间隔（小时）",
                    description: "",
                    kind: FieldKind::Number,
                    default: "24",
                    options: &[],
                    actions: &[],
                },
            ],
        }],
        SettingsKind::SysProxy => proxy_sections(),
        SettingsKind::Tun => tun_sections(),
        SettingsKind::General => vec![
            SectionSpec {
                title: "常规",
                fields: vec![
                    FieldSpec {
                        key: "AutoRun",
                        label: "开机自启",
                        description: "登录后自动启动 Suki Clash",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "SilentStart",
                        label: "静默启动",
                        description: "启动时直接进入托盘，不显示主窗口",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "StartupScript",
                        label: "启动脚本",
                        description: "脚本文件的绝对路径",
                        kind: FieldKind::Text,
                        default: "",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "CloseToTray",
                        label: "关闭时最小化到托盘",
                        description: "关闭窗口时保留后台运行",
                        kind: FieldKind::Toggle,
                        default: "true",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "EnvTypeIndex",
                        label: "环境变量类型",
                        description: "托盘复制代理环境变量时使用",
                        kind: FieldKind::Select,
                        default: if cfg!(target_os = "windows") {
                            "PowerShell"
                        } else {
                            "Bash"
                        },
                        options: &["PowerShell", "CMD", "Bash", "Fish", "Nushell"],
                        actions: &["复制环境变量"],
                    },
                    FieldSpec {
                        key: "ThemeIndex",
                        label: "主题",
                        description: "切换应用外观",
                        kind: FieldKind::Select,
                        default: "深色",
                        options: &["跟随系统", "浅色", "深色"],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "BackdropIndex",
                        label: "窗口背景",
                        description: "窗口使用的背景材质",
                        kind: FieldKind::Select,
                        default: if cfg!(target_os = "windows") {
                            "Acrylic"
                        } else {
                            "原生磨砂"
                        },
                        options: if cfg!(target_os = "windows") {
                            &["Acrylic", "纯色"]
                        } else {
                            &["原生磨砂", "纯色"]
                        },
                        actions: &[],
                    },
                    FieldSpec {
                        key: "SettingsAction6",
                        label: "退出应用",
                        description: "停止后台任务并退出 Suki Clash",
                        kind: FieldKind::ReadOnly,
                        default: "",
                        options: &[],
                        actions: &["退出应用"],
                    },
                ],
            },
            SectionSpec {
                title: "网络与订阅",
                fields: vec![
                    FieldSpec {
                        key: "GitHubProxy",
                        label: "GitHub 下载代理",
                        description: "用于 GitHub、Raw、Gist 和发行下载，直接拼接在原地址前",
                        kind: FieldKind::Text,
                        default: "https://get.syf.sh/",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "UserAgent",
                        label: "订阅 User-Agent",
                        description: "单个订阅未设置时使用",
                        kind: FieldKind::Text,
                        default: crate::network::DEFAULT_USER_AGENT,
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "SubscriptionTimeout",
                        label: "订阅超时（秒）",
                        description: "下载或更新订阅的最长等待时间",
                        kind: FieldKind::Number,
                        default: "30",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "ProfileUseProxy",
                        label: "订阅下载代理",
                        description: "新建订阅默认使用内核代理",
                        kind: FieldKind::Toggle,
                        default: "false",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DelayTestUrl",
                        label: "测速地址",
                        description: "节点测速使用的默认 URL",
                        kind: FieldKind::Text,
                        default: "https://www.gstatic.com/generate_204",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DelayTestConcurrency",
                        label: "测速并发数",
                        description: "同时测速的最大节点数",
                        kind: FieldKind::Number,
                        default: "8",
                        options: &[],
                        actions: &[],
                    },
                    FieldSpec {
                        key: "DelayTestTimeout",
                        label: "测速超时（毫秒）",
                        description: "单个节点的最长等待时间",
                        kind: FieldKind::Number,
                        default: "5000",
                        options: &[],
                        actions: &[],
                    },
                ],
            },
            SectionSpec {
                title: "Gist 同步",
                fields: vec![FieldSpec {
                    key: "GistBackup",
                    label: "Gist 备份",
                    description: "配置、订阅与覆写的完整备份",
                    kind: FieldKind::ReadOnly,
                    default: "",
                    options: &[],
                    actions: &["配置 Gist 备份", "创建 Gist 备份", "Gist 备份历史"],
                }],
            },
            SectionSpec {
                title: "内核与配置",
                fields: vec![FieldSpec {
                    key: "AutoCloseConnection",
                    label: "切换代理后关闭连接",
                    description: "切换节点或代理模式后关闭现有连接",
                    kind: FieldKind::Toggle,
                    default: "false",
                    options: &[],
                    actions: &[],
                }],
            },
            SectionSpec {
                title: "维护",
                fields: vec![
                    FieldSpec {
                        key: "SettingsAction28",
                        label: "备份与恢复",
                        description: "配置、订阅与覆写的完整本地备份",
                        kind: FieldKind::ReadOnly,
                        default: "",
                        options: &[],
                        actions: &["创建本地备份", "本地备份历史", "打开备份目录"],
                    },
                    FieldSpec {
                        key: "ResetProgram",
                        label: "重置程序",
                        description: "删除全部配置并退出程序",
                        kind: FieldKind::ReadOnly,
                        default: "",
                        options: &[],
                        actions: &["重置程序"],
                    },
                ],
            },
        ],
    }
}

pub(crate) fn default_bypass() -> &'static str {
    if cfg!(target_os = "windows") {
        "localhost\n127.*\n192.168.*\n10.*\n172.16.*\n172.17.*\n172.18.*\n172.19.*\n172.20.*\n172.21.*\n172.22.*\n172.23.*\n172.24.*\n172.25.*\n172.26.*\n172.27.*\n172.28.*\n172.29.*\n172.30.*\n172.31.*\n*.local\n<local>"
    } else if cfg!(target_os = "linux") {
        "localhost\n127.0.0.1\n192.168.0.0/16\n10.0.0.0/8\n172.16.0.0/12\n::1"
    } else {
        "127.0.0.1\n192.168.0.0/16\n10.0.0.0/8\n172.16.0.0/12\nlocalhost\n*.local\n*.crashlytics.com\n<local>"
    }
}

pub(crate) fn section_tab(kind: SettingsKind, title: &str, key: &str) -> usize {
    match kind {
        SettingsKind::Dns => match title {
            "DNS 服务器" => 1,
            "Fake-IP" => 2,
            "备用 DNS 筛选" | "解析策略" => 3,
            "Hosts" => 4,
            _ => 0,
        },
        SettingsKind::Sniffer => match title {
            "协议端口" => 1,
            "域名过滤" => 2,
            _ => 0,
        },
        SettingsKind::SysProxy => match title {
            "绕过代理" => 1,
            "PAC" => 2,
            _ => 0,
        },
        SettingsKind::Tun => match title {
            "路由" | "DNS" => 1,
            "服务与权限" => 0,
            _ => 0,
        },
        SettingsKind::General => match title {
            "常规" if matches!(key, "ThemeIndex" | "BackdropIndex") => 1,
            "常规" if key == "CloseToTray" => 2,
            "常规" if key == "SettingsAction6" => 4,
            "托盘" => 2,
            "网络与订阅" | "内核与配置" => 3,
            "Gist 同步" => 4,
            "维护" | "工具与更新" | "测速与日志" => 4,
            _ => 0,
        },
        _ => 0,
    }
}

pub(crate) fn field_visible(kind: SettingsKind, key: &str) -> bool {
    match (kind, key) {
        (SettingsKind::Tun, "TunAction1") | (SettingsKind::Core, "CoreUwp") => {
            cfg!(target_os = "windows")
        }
        (SettingsKind::Tun, "AutoRedirect") => cfg!(target_os = "linux"),
        (SettingsKind::General, "EnableTraySpeed") => cfg!(target_os = "macos"),
        (SettingsKind::General, "BackdropIndex") => {
            cfg!(any(target_os = "macos", target_os = "windows"))
        }
        (SettingsKind::General, "TrayEvent") => !cfg!(target_os = "linux"),
        _ => true,
    }
}

pub(crate) fn field_enabled(
    kind: SettingsKind,
    key: &str,
    values: &BTreeMap<String, String>,
) -> bool {
    let enabled = |key: &str| values.get(key).is_some_and(|value| value == "true");
    match (kind, key) {
        (SettingsKind::Dns, "DnsOverrideEnabled")
        | (SettingsKind::Sniffer, "SnifferOverrideEnabled") => true,
        (SettingsKind::Dns, "DnsEnabled") => enabled("DnsOverrideEnabled"),
        (SettingsKind::Sniffer, "SnifferEnabled") => enabled("SnifferOverrideEnabled"),
        (SettingsKind::Dns, key) => {
            enabled("DnsOverrideEnabled")
                && enabled("DnsEnabled")
                && match key {
                    "FakeIpRange" | "FakeIpRange6" | "FakeIpFilter" | "FakeIpFilterModeIndex" => {
                        values
                            .get("EnhancedModeIndex")
                            .is_some_and(|v| v == "fake-ip")
                    }
                    "Hosts" => enabled("UseHosts"),
                    "FallbackGeoIpCode" => enabled("FallbackGeoIp"),
                    _ => true,
                }
        }
        (SettingsKind::Sniffer, _) => {
            enabled("SnifferOverrideEnabled") && enabled("SnifferEnabled")
        }
        (SettingsKind::SysProxy, "ProxyGuardDuration") => enabled("EnableProxyGuard"),
        (SettingsKind::SysProxy, "BypassList") => values
            .get("SystemProxyModeIndex")
            .is_none_or(|value| value != "PAC"),
        (SettingsKind::SysProxy, "PacScript") => values
            .get("SystemProxyModeIndex")
            .is_some_and(|value| value == "PAC"),
        (SettingsKind::Tun, "AutoRedirect" | "RouteExcludeAddress") => enabled("AutoRoute"),
        (SettingsKind::General, "AutoDelayInterval") => enabled("EnableAutoDelay"),
        (SettingsKind::Core, "RedirPort") => !cfg!(windows),
        (SettingsKind::Core, "TproxyPort") => cfg!(target_os = "linux"),
        _ => true,
    }
}

pub(crate) fn split_entries(value: &str) -> impl Iterator<Item = &str> {
    value
        .split([',', ';', '\n', '\r'])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
}

fn valid_cidr(value: &str) -> bool {
    value.parse::<ipnet::IpNet>().is_ok() || value.parse::<std::net::IpAddr>().is_ok()
}

fn valid_host(value: &str) -> bool {
    let value = value.trim().trim_start_matches('[').trim_end_matches(']');
    if value.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    if value.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
        return false;
    }
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        })
}

fn valid_bypass(value: &str) -> bool {
    value == "<local>"
        || value == "localdomain"
        || valid_cidr(value)
        || !value.is_empty()
            && value.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '*'))
            })
}

fn valid_dns_hijack(value: &str) -> bool {
    let value = value
        .strip_prefix("tcp://")
        .or_else(|| value.strip_prefix("udp://"))
        .unwrap_or(value);
    let Some((host, port)) = value.rsplit_once(':') else {
        return false;
    };
    (host == "any" || valid_host(host)) && port.parse::<u16>().is_ok_and(|port| port > 0)
}

fn validate_preferences(values: &BTreeMap<String, String>) -> Option<String> {
    let on = |key: &str| values.get(key).is_some_and(|value| value == "true");
    if let Some(host) = values.get("SystemProxyHost")
        && !valid_host(host)
    {
        return Some("代理主机需填写有效的 IP 地址或主机名".into());
    }
    if values
        .get("SystemProxyModeIndex")
        .is_some_and(|value| value != "PAC")
        && values
            .get("EnableBypassCheck")
            .is_none_or(|value| value != "false")
        && let Some(value) = values.get("BypassList")
        && !split_entries(value).all(valid_bypass)
    {
        return Some("绕过列表包含无效的域名、IP 或 CIDR".into());
    }
    if on("AutoRoute")
        && let Some(value) = values.get("RouteExcludeAddress")
        && !split_entries(value).all(valid_cidr)
    {
        return Some("路由排除地址需填写有效的 IP 或 CIDR".into());
    }
    if let Some(value) = values.get("DnsHijack")
        && !split_entries(value).all(valid_dns_hijack)
    {
        return Some("DNS 劫持需使用 host:port 格式，例如 any:53".into());
    }
    for key in ["DelayTestUrl", "GitHubProxy"] {
        if let Some(value) = values.get(key)
            && !value.trim().is_empty()
            && !valid_web_url(value)
        {
            return Some(format!("{key} 需填写有效的 HTTP / HTTPS 地址"));
        }
    }
    None
}

fn valid_sniffer_ports(value: &str) -> bool {
    split_entries(value).all(|entry| {
        let port = |value: &str| value.trim().parse::<u16>().ok().filter(|port| *port > 0);
        if let Some((start, end)) = entry.split_once('-') {
            matches!((port(start), port(end)), (Some(start), Some(end)) if start <= end)
        } else {
            port(entry).is_some()
        }
    })
}

pub(crate) fn valid_dns_listen(value: &str) -> bool {
    value.parse::<std::net::SocketAddr>().is_ok()
        || value.strip_prefix(':').is_some_and(|port| {
            !port.is_empty()
                && port.bytes().all(|byte| byte.is_ascii_digit())
                && port.parse::<u16>().is_ok()
        })
}

fn validate_flow_settings(values: &BTreeMap<String, String>) -> Option<String> {
    let on = |key: &str| values.get(key).is_some_and(|value| value == "true");
    if on("DnsOverrideEnabled") && on("DnsEnabled") {
        if let Some(listen) = values.get("DnsListen")
            && !valid_dns_listen(listen)
        {
            return Some("DNS 监听地址无效，例如 :53、127.0.0.1:53".into());
        }
        if values
            .get("EnhancedModeIndex")
            .is_some_and(|value| value == "fake-ip")
        {
            for key in ["FakeIpRange", "FakeIpRange6"] {
                if let Some(value) = values.get(key)
                    && (!value.contains('/') || !valid_cidr(value))
                {
                    return Some("Fake-IP 地址池需填写有效的 CIDR 地址段".into());
                }
            }
        }
        if values
            .get("Nameserver")
            .is_none_or(|value| value.trim().is_empty())
        {
            return Some("至少填写一个主 DNS 服务器".into());
        }
        if let Some(value) = values.get("FallbackIpCidr")
            && !split_entries(value).all(valid_cidr)
        {
            return Some("备用 DNS 的 IP 地址段需填写有效的 IP 或 CIDR".into());
        }
    }
    if on("SnifferOverrideEnabled") && on("SnifferEnabled") {
        for key in ["HttpPorts", "TlsPorts", "QuicPorts"] {
            if let Some(value) = values.get(key)
                && !valid_sniffer_ports(value)
            {
                return Some("嗅探端口需在 1–65535 之间，范围起点不能大于终点".into());
            }
        }
        for key in ["SkipDstAddress", "SkipSrcAddress"] {
            if let Some(value) = values.get(key)
                && !split_entries(value).all(valid_cidr)
            {
                return Some("嗅探跳过地址需填写有效的 IP 或 CIDR".into());
            }
        }
    }
    None
}

pub(crate) fn validate(
    sections: &[SectionSpec],
    values: &BTreeMap<String, String>,
) -> Option<String> {
    if let Some(error) = validate_flow_settings(values).or_else(|| validate_preferences(values)) {
        return Some(error);
    }
    for field in sections.iter().flat_map(|s| &s.fields) {
        if matches!(field.key, "GeoIpUrl" | "GeoSiteUrl" | "MmdbUrl" | "AsnUrl")
            && let Some(value) = values.get(field.key)
            && !value.trim().is_empty()
            && !valid_web_url(value)
        {
            return Some(format!("{}需填写 HTTP / HTTPS 地址", field.label));
        }
        if field.key == "GeoUpdateIntervalValue"
            && values.get(field.key).is_some_and(|value| value == "0")
        {
            return Some("自动更新间隔需大于 0 小时".into());
        }
        if field.kind == FieldKind::Number {
            let value = values.get(field.key).map(String::as_str).unwrap_or("");
            let number = match value.parse::<u32>() {
                Ok(n) => n,
                Err(_) => return Some(format!("{}需填写非负整数", field.label)),
            };
            let range = match field.key {
                "Mtu" => Some((576, 65535)),
                "DelayTestTimeout" => Some((100, 60000)),
                "DelayTestConcurrency" => Some((1, 10)),
                "ProxyGuardDuration"
                | "SubscriptionTimeout"
                | "AutoDelayInterval"
                | "AppLogMaxSize"
                | "AppLogMaxCount" => Some((1, u32::MAX)),
                _ => None,
            };
            if let Some((minimum, maximum)) = range
                && !(minimum..=maximum).contains(&number)
            {
                return Some(format!("{}需在 {minimum}–{maximum} 之间", field.label));
            }
            if field.key.to_lowercase().contains("port") && number > 65535 {
                return Some(format!("{}需在 0–65535 之间", field.label));
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::super::SettingsKind;
    use super::{FieldKind, sections, validate};
    use std::collections::BTreeMap;
    #[test]
    fn measurement_settings_reject_values_the_executor_cannot_honor() {
        let sections = sections(SettingsKind::General);
        let mut values = crate::preferences::defaults(SettingsKind::General);
        for (key, valid, invalid) in [
            ("DelayTestTimeout", ["100", "60000"], ["99", "60001"]),
            ("DelayTestConcurrency", ["1", "10"], ["0", "11"]),
        ] {
            let original = values[key].clone();
            for value in valid {
                values.insert(key.into(), value.into());
                assert!(validate(&sections, &values).is_none());
            }
            for value in invalid {
                values.insert(key.into(), value.into());
                assert!(validate(&sections, &values).is_some());
            }
            values.insert(key.into(), original);
        }
    }
    #[test]
    fn general_settings_keep_tray_and_sync_without_removed_customization_features() {
        let general = sections(SettingsKind::General);
        let fields: Vec<_> = general.iter().flat_map(|section| &section.fields).collect();
        for key in [
            "TrayIcon",
            "CustomTrayIcons",
            "ProxyGroupToolsPosition",
            "ServiceMode",
            "WebDavUrl",
            "WebDavPassword",
            "AutoBackupSchedule",
            "FontFamily",
            "CollapseNavbar",
            "EnableGlobalHotkey",
            "AutoLiteMode",
            "LiteModeDelay",
            "PrimaryColor",
            "SecondaryColor",
            "PrimaryTextColor",
            "SecondaryTextColor",
            "InfoColor",
            "WarningColor",
            "ErrorColor",
            "SuccessColor",
        ] {
            assert!(
                !fields.iter().any(|field| field.key == key),
                "removed setting returned: {key}"
            );
        }
        assert!(!fields.iter().any(|field| field.key.starts_with("Hotkey")));
        for key in [
            "TrayEvent",
            "TrayProxyGroups",
            "EnableTraySpeed",
            "GistBackup",
        ] {
            assert!(
                fields.iter().any(|field| field.key == key),
                "retained setting missing: {key}"
            );
        }
        let appearance: std::collections::BTreeSet<_> = general
            .iter()
            .flat_map(|section| {
                section
                    .fields
                    .iter()
                    .filter(|field| {
                        super::section_tab(SettingsKind::General, section.title, field.key) == 1
                    })
                    .map(|field| field.key)
            })
            .collect();
        assert_eq!(
            appearance,
            std::collections::BTreeSet::from(["ThemeIndex", "BackdropIndex"])
        );
    }
    #[test]
    fn flow_settings_dependencies_preserve_disabled_configuration() {
        let mut values = BTreeMap::from([
            ("DnsOverrideEnabled".into(), "true".into()),
            ("DnsEnabled".into(), "true".into()),
            ("EnhancedModeIndex".into(), "redir-host".into()),
            ("Nameserver".into(), "https://doh.pub/dns-query".into()),
            ("UseHosts".into(), "false".into()),
            ("DnsListen".into(), "invalid".into()),
        ]);
        assert!(!super::field_enabled(
            SettingsKind::Dns,
            "FakeIpRange",
            &values
        ));
        assert!(!super::field_enabled(SettingsKind::Dns, "Hosts", &values));
        assert!(super::validate_flow_settings(&values).is_some());
        values.insert("DnsOverrideEnabled".into(), "false".into());
        assert!(super::validate_flow_settings(&values).is_none());
        assert!(!super::field_enabled(
            SettingsKind::Dns,
            "DnsListen",
            &values
        ));
        values.insert("SnifferOverrideEnabled".into(), "true".into());
        values.insert("SnifferEnabled".into(), "false".into());
        assert!(!super::field_enabled(
            SettingsKind::Sniffer,
            "HttpPorts",
            &values
        ));
        assert!(super::field_enabled(
            SettingsKind::Sniffer,
            "SnifferEnabled",
            &values
        ));
    }

    #[test]
    fn sniffer_ports_accept_ranges_and_reject_invalid_boundaries() {
        for value in ["80,443,8080-8880", "1-65535", "", "443\n8443"] {
            assert!(super::valid_sniffer_ports(value), "{value}");
        }
        for value in ["0", "65536", "443-80", "80-", "https", "-1"] {
            assert!(!super::valid_sniffer_ports(value), "{value}");
        }
    }

    #[test]
    fn all_original_setting_pages_have_unique_fields_and_valid_defaults() {
        for page in SettingsKind::ALL {
            let sections = sections(page);
            let mut keys = std::collections::BTreeSet::new();
            let mut values = BTreeMap::new();
            for field in sections.iter().flat_map(|s| &s.fields) {
                assert!(
                    keys.insert(field.key),
                    "duplicate field {} in {:?}",
                    field.key,
                    page
                );
                values.insert(field.key.to_owned(), field.default.to_owned());
                if field.kind == FieldKind::Select {
                    assert!(field.options.contains(&field.default), "{}", field.key);
                }
            }
            assert!(
                validate(&sections, &values).is_none(),
                "{:?}: {:?}",
                page,
                validate(&sections, &values)
            );
        }
    }
    #[test]
    fn proxy_modes_and_route_dependencies_preserve_inactive_values() {
        let mut values = BTreeMap::from([
            ("IsSystemProxyEnabled".into(), "true".into()),
            ("SystemProxyModeIndex".into(), "手动".into()),
            ("AutoRoute".into(), "false".into()),
        ]);
        assert!(super::field_enabled(
            SettingsKind::SysProxy,
            "BypassList",
            &values
        ));
        values.insert("SystemProxyModeIndex".into(), "PAC".into());
        assert!(!super::field_enabled(
            SettingsKind::SysProxy,
            "BypassList",
            &values
        ));
        assert!(super::field_enabled(
            SettingsKind::SysProxy,
            "PacScript",
            &values
        ));
        assert!(!super::field_enabled(
            SettingsKind::Tun,
            "RouteExcludeAddress",
            &values
        ));
        values.insert("AutoRoute".into(), "true".into());
        assert!(super::field_enabled(
            SettingsKind::Tun,
            "RouteExcludeAddress",
            &values
        ));
    }
    #[test]
    fn tun_validation_rejects_bad_cidr_mtu_and_dns_ports() {
        let sections = sections(SettingsKind::Tun);
        let mut values: BTreeMap<_, _> = sections
            .iter()
            .flat_map(|section| &section.fields)
            .map(|field| (field.key.to_owned(), field.default.to_owned()))
            .collect();
        values.insert(
            "RouteExcludeAddress".into(),
            "192.168.1.0/24\n2001:db8::/32".into(),
        );
        assert!(validate(&sections, &values).is_none());
        values.insert("RouteExcludeAddress".into(), "192.168.0.0/33".into());
        assert!(validate(&sections, &values).is_some());
        values.insert("AutoRoute".into(), "false".into());
        assert!(validate(&sections, &values).is_none());
        values.insert("Mtu".into(), "100".into());
        assert!(validate(&sections, &values).is_some());
        values.insert("Mtu".into(), "1500".into());
        values.insert("DnsHijack".into(), "any:70000".into());
        assert!(validate(&sections, &values).is_some());
    }
    #[test]
    fn proxy_validation_accepts_ipv6_and_checks_custom_bypass() {
        let sections = sections(SettingsKind::SysProxy);
        let mut values: BTreeMap<_, _> = sections
            .iter()
            .flat_map(|section| &section.fields)
            .map(|field| (field.key.to_owned(), field.default.to_owned()))
            .collect();
        values.insert("SystemProxyHost".into(), "::1".into());
        values.insert(
            "BypassList".into(),
            "*.example.com,192.168.0.0/16;<local>".into(),
        );
        assert!(validate(&sections, &values).is_none());
        values.insert("BypassList".into(), "https://bad.example".into());
        assert!(validate(&sections, &values).is_some());
        values.insert("EnableBypassCheck".into(), "false".into());
        assert!(validate(&sections, &values).is_none());
        values.insert("EnableBypassCheck".into(), "true".into());
        values.insert("SystemProxyModeIndex".into(), "PAC".into());
        assert!(validate(&sections, &values).is_none());
        values.insert("SystemProxyHost".into(), "999.0.0.1".into());
        assert!(validate(&sections, &values).is_some());
    }
    #[test]
    fn ports_reject_out_of_range_values() {
        let sections = sections(SettingsKind::Core);
        let mut values: BTreeMap<_, _> = sections
            .iter()
            .flat_map(|s| &s.fields)
            .map(|f| (f.key.to_owned(), f.default.to_owned()))
            .collect();
        values.insert("MixedPort".into(), "65536".into());
        assert!(validate(&sections, &values).is_some());
    }
}
