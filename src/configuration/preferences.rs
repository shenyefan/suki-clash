//! Native Mihomo YAML and separate application preferences JSON.
use crate::storage::{AppPaths, Values, atomic_write};
use serde_yaml_ng::{Mapping, Value};
use std::{fs, path::PathBuf};

const LIMIT: u64 = 8 * 1024 * 1024;
const PORTS: [(&str, &str, &str); 5] = [
    ("MixedPort", "mixed-port", "7897"),
    ("SocksPort", "socks-port", "0"),
    ("HttpPort", "port", "0"),
    ("RedirPort", "redir-port", "0"),
    ("TproxyPort", "tproxy-port", "0"),
];
const FLAGS: [(&str, &str, bool); 4] = [
    ("Ipv6", "ipv6", true),
    ("UnifiedDelay", "unified-delay", true),
    ("TcpConcurrent", "tcp-concurrent", true),
    ("AllowLan", "allow-lan", false),
];
const LISTS: [(&str, &str); 4] = [
    ("LanAllowedIps", "lan-allowed-ips"),
    ("LanDisallowedIps", "lan-disallowed-ips"),
    ("Authentication", "authentication"),
    ("SkipAuthPrefixes", "skip-auth-prefixes"),
];

#[derive(Clone)]
pub(crate) struct Preferences {
    native: Mapping,
    app: Values,
}
impl std::fmt::Debug for Preferences {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Preferences { <redacted> }")
    }
}
impl PartialEq for Preferences {
    fn eq(&self, other: &Self) -> bool {
        self.native == other.native && self.app == other.app
    }
}
impl Eq for Preferences {}
impl Preferences {
    pub(crate) const DEFAULT_ALLOWED_IPS: &'static str = "127.0.0.1/32\n::1/128";
    pub(crate) fn native(&self) -> &Mapping {
        &self.native
    }
    pub(crate) fn paths(paths: &AppPaths) -> [PathBuf; 2] {
        let root = paths.settings();
        [
            root.join("mihomo.yaml"),
            crate::preferences::app::path(paths),
        ]
    }
    pub(crate) fn load(paths: &AppPaths) -> Result<Self, String> {
        let [native, _] = Self::paths(paths);
        let mut defaults = crate::preferences::defaults(crate::preferences::SettingsKind::Core);
        defaults.extend(crate::preferences::defaults(
            crate::preferences::SettingsKind::Resources,
        ));
        defaults.extend(crate::preferences::defaults(
            crate::preferences::SettingsKind::Tun,
        ));
        let app_values = crate::preferences::app::read_values(paths, "core", LIMIT)?;
        let saved = if native
            .try_exists()
            .map_err(|error| crate::logging::diagnostic_error("无法检查内核设置", error))?
        {
            let mapping: Mapping =
                serde_yaml_ng::from_slice(&read_bytes(&native)?).map_err(|error| {
                    let location = error
                        .location()
                        .map(|location| {
                            format!("，第 {} 行第 {} 列", location.line(), location.column())
                        })
                        .unwrap_or_default();
                    format!(
                        "{}：YAML 格式无效{location}；请修正文件后重试",
                        native.display()
                    )
                })?;
            Some(Self {
                native: mapping,
                app: app_values,
            })
        } else if app_values.is_empty() {
            None
        } else {
            Some(Self {
                native: Mapping::new(),
                app: app_values,
            })
        };
        if let Some(saved) = &saved {
            super::tun::project(&saved.native)?;
            validate_native(&saved.native)
                .map_err(|error| format!("{}：{error}；请修正文件后重试", native.display()))?;
            if saved.app.contains_key("WebUiConfigured") {
                defaults.retain(|key, _| !key.starts_with("WebUi."));
            }
            defaults.extend(saved.values());
        }
        let defaults = crate::preferences::choices::decode(defaults);
        Self::from_values(&defaults, saved.as_ref())
    }
    pub(crate) fn from_values(values: &Values, previous: Option<&Self>) -> Result<Self, String> {
        let mut native = previous.map(|p| p.native.clone()).unwrap_or_default();
        let mut app = values.clone();
        let get =
            |key: &str, default: &str| values.get(key).cloned().unwrap_or_else(|| default.into());
        for (field, key, default) in PORTS {
            let port = get(field, default).parse::<u16>().map_err(|error| {
                crate::logging::rejected(format!("{field} 必须为 0–65535 的整数"), error)
            })?;
            if key == "redir-port" && cfg!(windows) && port != 0 {
                return Err("Windows 不支持重定向端口".into());
            }
            if key == "tproxy-port" && !cfg!(target_os = "linux") && port != 0 {
                return Err("TProxy 端口仅支持 Linux".into());
            }
            native.insert(key.into(), Value::Number(port.into()));
        }
        let mut ports = std::collections::BTreeSet::new();
        for (_, key, _) in PORTS {
            if let Some(port) = native.get(key).and_then(Value::as_u64)
                && port != 0
                && !ports.insert(port)
            {
                return Err("启用的内核监听端口不能重复".into());
            }
        }
        for (field, key, default) in FLAGS {
            native.insert(key.into(), Value::Bool(boolean(values, field, default)?));
        }
        for (field, key) in LISTS {
            let lines = get(
                field,
                if field == "LanAllowedIps" {
                    Self::DEFAULT_ALLOWED_IPS
                } else {
                    ""
                },
            )
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
            if field != "Authentication" && lines.iter().any(|s| !valid_network(s)) {
                return Err(format!("{field} 含无效 IP/CIDR"));
            }
            if field == "Authentication"
                && lines.iter().any(|s| {
                    !s.split_once(':')
                        .is_some_and(|(user, pass)| !user.is_empty() && !pass.is_empty())
                })
            {
                return Err("用户认证需每行填写 user:password".into());
            }
            native.insert(
                key.into(),
                Value::Sequence(lines.into_iter().map(Value::String).collect()),
            );
        }
        let log = enum_value(
            &get("LogLevelIndex", "信息（info）"),
            &["silent", "error", "warning", "info", "debug"],
        )?;
        let process = enum_value(
            &get("FindProcessIndex", "严格（strict）"),
            &["off", "strict", "always"],
        )?;
        native.insert("log-level".into(), log.into());
        native.insert("find-process-mode".into(), process.into());
        let origins = get(
            "CorsOrigins",
            "https://metacubex.github.io\nhttps://yacd.metacubex.one\nhttps://board.zash.run.place",
        );
        let mut allowed = Vec::new();
        for origin in origins.lines().map(str::trim).filter(|s| !s.is_empty()) {
            let valid = origin == "*"
                || origin.parse::<hyper::Uri>().ok().is_some_and(|uri| {
                    matches!(uri.scheme_str(), Some("http" | "https"))
                        && uri.authority().is_some_and(|a| !a.as_str().contains('@'))
                        && uri.path_and_query().is_none_or(|p| p.as_str() == "/")
                });
            if !valid || origin.len() > 2048 {
                return Err("CORS 来源需为 HTTP/HTTPS 来源（不含路径、查询或凭据），或 *".into());
            }
            if !allowed.contains(&origin) {
                allowed.push(origin);
            }
        }
        if allowed.len() > 128 {
            return Err("CORS 来源不能超过 128 项".into());
        }
        native.insert("external-controller-cors".into(), serde_yaml_ng::to_value(serde_json::json!({"allow-private-network": boolean(values, "CorsPrivateNetwork", true)?, "allow-origins": allowed})).map_err(|error| crate::logging::diagnostic_error("无法编码 CORS 配置", error))?);
        let tunnels = super::tunnels::parse(&get("Tunnels", "[]"))?;
        for tunnel in &tunnels {
            let address = tunnel
                .address
                .parse::<std::net::SocketAddr>()
                .map_err(|error| crate::logging::rejected("隧道监听无效", error))?;
            if PORTS.iter().any(|(_, key, _)| {
                native.get(*key).and_then(Value::as_u64) == Some(u64::from(address.port()))
            }) {
                return Err("隧道端口与代理监听端口重复".into());
            }
        }
        native.insert(
            "tunnels".into(),
            serde_yaml_ng::to_value(tunnels)
                .map_err(|error| crate::logging::diagnostic_error("无法编码隧道配置", error))?,
        );
        let enabled = boolean(values, "EnableExternalController", false)?;
        let controller = get("ExternalController", "127.0.0.1:9090");
        let secret = get("Secret", "");
        if enabled {
            let address = controller
                .parse::<std::net::SocketAddr>()
                .map_err(|error| {
                    crate::logging::rejected("外部控制地址必须为 IP:端口（IPv6 使用方括号）", error)
                })?;
            if address.port() == 0 || secret.trim().is_empty() {
                return Err("启用外部控制时必须设置非零端口及访问密钥".into());
            }
        }
        native.insert(
            "external-controller".into(),
            if enabled {
                controller.clone()
            } else {
                String::new()
            }
            .into(),
        );
        native.insert("secret".into(), secret.into());
        let mut profile = native
            .get("profile")
            .and_then(Value::as_mapping)
            .cloned()
            .unwrap_or_default();
        for (field, key) in [
            ("StoreSelected", "store-selected"),
            ("StoreFakeIp", "store-fake-ip"),
        ] {
            profile.insert(key.into(), Value::Bool(boolean(values, field, true)?));
        }
        native.insert("profile".into(), profile.into());
        app.insert("ExternalController".into(), controller);
        for key in [
            "Secret",
            "Authentication",
            "LanAllowedIps",
            "LanDisallowedIps",
            "SkipAuthPrefixes",
        ] {
            app.remove(key);
        }
        let channel = get("CoreReleaseIndex", "最新版");
        let channel = match channel.as_str() {
            "最新版" | "latest" => "latest",
            "预览版" | "preview" => "preview",
            "Smart" | "smart" => "smart",
            "指定版本" | "specific" => "specific",
            _ => return Err("发行通道无效".into()),
        };
        app.insert("CoreReleaseIndex".into(), channel.into());
        for (key, default) in [("MaxLogDays", "7"), ("MaxLogFileSizeMb", "10")] {
            let value = get(key, default)
                .parse::<u32>()
                .map_err(|error| crate::logging::rejected("日志保留值必须为正整数", error))?;
            if value == 0 || value > 3650 {
                return Err("日志保留值需在 1–3650 之间".into());
            }
            app.insert(key.into(), value.to_string());
        }
        for key in ["CorsOrigins", "CorsPrivateNetwork", "Tunnels"] {
            app.remove(key);
        }
        app.remove("NewWebUiName");
        app.remove("SocksPortEnabled");
        app.remove("HttpPortEnabled");
        for (field, _, _) in PORTS {
            app.remove(field);
        }
        for (field, _, _) in FLAGS {
            app.remove(field);
        }
        for field in [
            "LogLevelIndex",
            "FindProcessIndex",
            "StoreSelected",
            "StoreFakeIp",
            "EnableExternalController",
        ] {
            app.remove(field);
        }
        super::tun::apply(values, &mut native)?;
        for (field, _) in super::tun::FIELDS {
            app.remove(*field);
        }
        super::resources::apply(values, &mut native)?;
        app.retain(|key, _| !super::resources::is_field(key));
        Ok(Self { native, app })
    }
    pub(crate) fn values(&self) -> Values {
        let mut values = self.app.clone();
        values.extend(super::resources::read(&self.native));
        if let Ok(tun) = super::tun::project(&self.native) {
            values.extend(tun);
        }
        if let Some(channel) = values.get_mut("CoreReleaseIndex") {
            *channel = match channel.as_str() {
                "latest" => "最新版",
                "preview" => "预览版",
                "smart" => "Smart",
                "specific" => "指定版本",
                other => other,
            }
            .into();
        }
        for (field, key, default) in PORTS {
            values.insert(
                field.into(),
                self.native
                    .get(key)
                    .and_then(Value::as_u64)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| default.into()),
            );
        }
        for (field, key, default) in FLAGS {
            values.insert(
                field.into(),
                self.native
                    .get(key)
                    .and_then(Value::as_bool)
                    .unwrap_or(default)
                    .to_string(),
            );
        }
        for (field, key) in LISTS {
            values.insert(
                field.into(),
                self.native
                    .get(key)
                    .and_then(Value::as_sequence)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
            );
        }
        for (field, key, default) in [
            ("LogLevelIndex", "log-level", "info"),
            ("FindProcessIndex", "find-process-mode", "strict"),
        ] {
            let value = self
                .native
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or(default);
            let label = match value {
                "silent" => "静默（silent）",
                "error" => "错误（error）",
                "warning" => "警告（warning）",
                "info" => "信息（info）",
                "debug" => "调试（debug）",
                "strict" => "严格（strict）",
                "always" => "始终（always）",
                _ => "关闭（off）",
            };
            values.insert(field.into(), label.into());
        }
        let cors = self.native.get("external-controller-cors");
        values.insert(
            "CorsPrivateNetwork".into(),
            cors.and_then(|v| v.get("allow-private-network"))
                .and_then(Value::as_bool)
                .unwrap_or(true)
                .to_string(),
        );
        values.insert("CorsOrigins".into(), cors.and_then(|v| v.get("allow-origins")).and_then(Value::as_sequence).map(|origins| origins.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap_or_else(|| "https://metacubex.github.io\nhttps://yacd.metacubex.one\nhttps://board.zash.run.place".into()));
        values.insert(
            "Tunnels".into(),
            serde_yaml_ng::to_string(
                self.native
                    .get("tunnels")
                    .unwrap_or(&Value::Sequence(Vec::new())),
            )
            .unwrap_or_else(|_| "[]".into()),
        );
        let controller = self
            .native
            .get("external-controller")
            .and_then(Value::as_str)
            .unwrap_or("");
        values.insert(
            "EnableExternalController".into(),
            (!controller.is_empty()).to_string(),
        );
        values.insert(
            "Secret".into(),
            self.native
                .get("secret")
                .and_then(Value::as_str)
                .unwrap_or("")
                .into(),
        );
        if !controller.is_empty() {
            values.insert("ExternalController".into(), controller.into());
        }
        for (field, key) in [
            ("StoreSelected", "store-selected"),
            ("StoreFakeIp", "store-fake-ip"),
        ] {
            values.insert(
                field.into(),
                self.native
                    .get("profile")
                    .and_then(|value| value.get(key))
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                    .to_string(),
            );
        }
        values
    }
    pub(crate) fn local_proxy_error(&self) -> Option<String> {
        let address = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let contains = |key: &str| {
            self.native
                .get(key)
                .and_then(Value::as_sequence)
                .is_some_and(|entries| {
                    entries.iter().filter_map(Value::as_str).any(|entry| {
                        parse_network(entry).is_some_and(|network| network.contains(&address))
                    })
                })
        };
        if !contains("lan-allowed-ips") {
            Some("允许的地址未包含 127.0.0.1；请检查「内核 → 访问 → 允许的地址」".into())
        } else if contains("lan-disallowed-ips") {
            Some("127.0.0.1 位于禁止的地址中；请检查内核访问设置".into())
        } else {
            None
        }
    }
    pub(crate) fn with_mode(&self, mode: crate::session::ProxyMode) -> Self {
        let mut next = self.clone();
        next.native.insert("mode".into(), mode.label().into());
        next
    }
    pub(crate) fn overlay(&self, config: &mut Mapping) {
        for (key, value) in &self.native {
            if matches!(key.as_str(), Some("profile" | "geox-url" | "tun")) {
                let mut profile = config
                    .get(key)
                    .and_then(Value::as_mapping)
                    .cloned()
                    .unwrap_or_default();
                if let Some(values) = value.as_mapping() {
                    profile.extend(values.clone());
                }
                config.insert(key.clone(), profile.into());
            } else {
                config.insert(key.clone(), value.clone());
            }
        }
        for (_, key, _) in PORTS {
            if let Some(value) = self.native.get(key) {
                config.insert(key.into(), value.clone());
            }
        }
        for (_, key, _) in FLAGS {
            if let Some(value) = self.native.get(key) {
                config.insert(key.into(), value.clone());
            }
        }
        for (_, key) in LISTS {
            if let Some(value) = self.native.get(key) {
                config.insert(key.into(), value.clone());
            }
        }
        ensure_lan_bind_address(config);
    }
    pub(crate) fn save(&self, paths: &AppPaths) -> Result<(), String> {
        let [native, _] = Self::paths(paths);
        let bytes = serde_yaml_ng::to_string(&self.native)
            .map_err(|error| crate::logging::diagnostic_error("无法编码内核设置", error))?;
        let previous = match fs::read(&native) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(crate::logging::diagnostic_error(
                    "无法保留旧内核设置".into(),
                    error,
                ));
            }
        };
        atomic_write(&native, bytes.as_bytes())
            .map_err(|error| crate::logging::diagnostic_error("无法保存内核设置", error))?;
        if let Err(error) = crate::preferences::app::write_values(paths, "core", &self.app) {
            let recovery = match previous {
                Some(bytes) => atomic_write(&native, &bytes).map_err(crate::logging::error_message),
                None => fs::remove_file(&native).map_err(|error| {
                    crate::logging::diagnostic_error("无法恢复内核设置".into(), error)
                }),
            };
            return Err(match recovery {
                Ok(()) => error,
                Err(recovery) => format!("{error}；{recovery}"),
            });
        }
        Ok(())
    }
    pub(crate) fn log_limits(&self) -> (u32, u64) {
        (
            self.app
                .get("MaxLogDays")
                .and_then(|n| n.parse().ok())
                .unwrap_or(7),
            self.app
                .get("MaxLogFileSizeMb")
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(10)
                * 1024
                * 1024,
        )
    }
}
fn validate_native(native: &Mapping) -> Result<(), String> {
    for (_, key, _) in PORTS {
        if let Some(value) = native.get(key)
            && !value
                .as_u64()
                .is_some_and(|port| port <= u64::from(u16::MAX))
        {
            return Err(format!("{key} 必须为 0–65535 的整数"));
        }
    }
    for key in FLAGS
        .iter()
        .map(|(_, key, _)| *key)
        .chain(["geodata-mode", "geo-auto-update"])
    {
        if let Some(value) = native.get(key)
            && !value.is_bool()
        {
            return Err(format!("{key} 必须为布尔值"));
        }
    }
    for key in [
        "secret",
        "external-controller",
        "log-level",
        "find-process-mode",
        "mode",
    ] {
        if let Some(value) = native.get(key)
            && !value.is_string()
        {
            return Err(format!("{key} 必须为文本"));
        }
    }
    for (key, allowed) in [
        (
            "log-level",
            ["silent", "error", "warning", "info", "debug"].as_slice(),
        ),
        ("find-process-mode", ["off", "strict", "always"].as_slice()),
        ("mode", ["rule", "global", "direct"].as_slice()),
    ] {
        if let Some(value) = native.get(key)
            && !value.as_str().is_some_and(|value| allowed.contains(&value))
        {
            return Err(format!("{key} 无效"));
        }
    }
    if let Some(profile) = native.get("profile") {
        let profile = profile.as_mapping().ok_or("profile 必须为对象")?;
        for key in ["store-selected", "store-fake-ip"] {
            if let Some(value) = profile.get(key)
                && !value.is_bool()
            {
                return Err(format!("profile.{key} 必须为布尔值"));
            }
        }
    }
    if let Some(cors) = native.get("external-controller-cors") {
        let cors = cors
            .as_mapping()
            .ok_or("external-controller-cors 必须为对象")?;
        if let Some(value) = cors.get("allow-private-network")
            && !value.is_bool()
        {
            return Err("CORS 私有网络设置必须为布尔值".into());
        }
        if let Some(value) = cors.get("allow-origins")
            && !value
                .as_sequence()
                .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Err("CORS 来源必须为文本数组".into());
        }
    }
    for (_, key) in LISTS {
        if let Some(value) = native.get(key)
            && !value
                .as_sequence()
                .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Err(format!("{key} 必须为文本数组"));
        }
    }
    if let Some(urls) = native.get("geox-url")
        && !urls
            .as_mapping()
            .is_some_and(|urls| urls.values().all(Value::is_string))
    {
        return Err("geox-url 必须为地址映射".into());
    }
    if let Some(value) = native.get("geo-update-interval")
        && !value
            .as_u64()
            .is_some_and(|hours| hours > 0 && hours <= u64::from(u32::MAX))
    {
        return Err("geo-update-interval 必须大于 0".into());
    }
    Ok(())
}
fn boolean(values: &Values, key: &str, default: bool) -> Result<bool, String> {
    match values.get(key).map(String::as_str) {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Err(format!("{key} 必须为布尔值")),
    }
}
fn enum_value(value: &str, options: &[&str]) -> Result<String, String> {
    options
        .iter()
        .find(|option| value == **option || value.ends_with(&format!("（{option}）")))
        .map(|s| s.to_string())
        .ok_or("内核枚举设置无效".into())
}
fn parse_network(value: &str) -> Option<ipnet::IpNet> {
    value.parse().ok().or_else(|| {
        value
            .parse::<std::net::IpAddr>()
            .ok()
            .map(ipnet::IpNet::from)
    })
}
fn valid_network(value: &str) -> bool {
    parse_network(value).is_some()
}

fn read_bytes(path: &std::path::Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| crate::logging::diagnostic_error("无法检查设置文件", error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("设置路径不是普通文件".into());
    }
    let file = fs::File::open(path)
        .map_err(|error| crate::logging::diagnostic_error("无法读取设置文件", error))?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| crate::logging::diagnostic_error("读取设置失败", error))?;
    if bytes.len() as u64 > LIMIT {
        return Err("设置文件超过上限".into());
    }
    Ok(bytes)
}

/// Widen only loopback bindings when LAN access is enabled; explicit interfaces stay authoritative.
pub(crate) fn ensure_lan_bind_address(config: &mut Mapping) {
    if config.get("allow-lan").and_then(Value::as_bool) != Some(true) {
        return;
    }
    let Some(address) = config.get("bind-address").and_then(Value::as_str) else {
        return;
    };
    let address = address.trim();
    let address = address
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(address);
    let loopback = address.eq_ignore_ascii_case("localhost")
        || address.parse::<std::net::IpAddr>().is_ok_and(|address| address.is_loopback())
        || address.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>().is_ok_and(|parts| {
            matches!(parts.as_slice(), [127, tail] if *tail <= 0x00ff_ffff)
                || matches!(parts.as_slice(), [127, middle, tail] if *middle <= 0xff && *tail <= 0xffff)
        });
    if loopback {
        config.insert("bind-address".into(), "*".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lan_overlay_preserves_explicit_interfaces_and_only_widens_loopback() {
        for allowed in [false, true] {
            let preferences = Preferences::from_values(
                &Values::from([("AllowLan".into(), allowed.to_string())]),
                None,
            )
            .unwrap();
            for address in ["192.168.1.2", "2001:db8::10", "*"] {
                let mut config = Mapping::from_iter([("bind-address".into(), address.into())]);
                preferences.overlay(&mut config);
                assert_eq!(config["bind-address"], address);
            }
            for address in ["127.0.0.1", "127.1", "127.0.1", "localhost", "[::1]"] {
                let mut config = Mapping::from_iter([("bind-address".into(), address.into())]);
                preferences.overlay(&mut config);
                assert_eq!(config["bind-address"], if allowed { "*" } else { address });
            }
            let mut config = Mapping::new();
            preferences.overlay(&mut config);
            assert!(!config.contains_key("bind-address"));
        }
    }

    #[test]
    fn native_yaml_roundtrip_preserves_unknown_fields_and_keeps_preferences_separate() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let mut values = Values::from([
            ("MixedPort".into(), "0".into()),
            ("HttpPort".into(), "0".into()),
            ("SocksPort".into(), "0".into()),
            ("Secret".into(), "test-secret".into()),
            ("CoreReleaseIndex".into(), "Smart".into()),
            ("Authentication".into(), "user:test-password".into()),
        ]);
        let mut preferences = Preferences::from_values(&values, None).unwrap();
        preferences.native.insert(
            "custom-key".into(),
            serde_yaml_ng::from_str::<Value>("MixedCase: keep").unwrap(),
        );
        preferences.save(&paths).unwrap();
        let loaded = Preferences::load(&paths).unwrap();
        assert_eq!(loaded.values()["Secret"], "test-secret");
        assert_eq!(loaded.values()["Authentication"], "user:test-password");
        assert_eq!(loaded.values()["CoreReleaseIndex"], "Smart");
        values = loaded.values();
        values.insert("Ipv6".into(), "false".into());
        let changed = Preferences::from_values(&values, Some(&loaded)).unwrap();
        changed.save(&paths).unwrap();
        let [native, app] = Preferences::paths(&paths);
        let native = fs::read_to_string(native).unwrap();
        let app = fs::read_to_string(app).unwrap();
        assert!(native.contains("MixedCase"));
        assert!(native.contains("test-secret"));
        assert!(!app.contains("test-secret"));
        assert!(!native.contains("CoreReleaseIndex"));
        assert!(!app.contains("Authentication"));
        assert!(!format!("{changed:?}").contains("test-secret"));
        let mut config = Mapping::new();
        changed.overlay(&mut config);
        assert_eq!(config["port"], 0);
        assert_eq!(config["socks-port"], 0);
    }
    #[test]
    fn external_controller_and_access_values_are_validated_before_persistence() {
        for (field, value) in [
            ("MixedPort", "65536"),
            ("LanAllowedIps", "bad-cidr"),
            ("Authentication", "missing-password"),
            ("MaxLogDays", "0"),
        ] {
            assert!(
                Preferences::from_values(&Values::from([(field.into(), value.into())]), None)
                    .is_err()
            );
        }
        let mut values = Values::from([("EnableExternalController".into(), "true".into())]);
        assert!(Preferences::from_values(&values, None).is_err());
        values.insert("Secret".into(), "pass".into());
        assert!(Preferences::from_values(&values, None).is_ok());
        values.insert("ExternalController".into(), "not an address".into());
        assert!(Preferences::from_values(&values, None).is_err());
    }
    #[test]
    fn failed_second_file_commit_restores_native_file_without_removing_unrelated_data() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let preferences = Preferences::from_values(&Values::new(), None).unwrap();
        preferences.save(&paths).unwrap();
        let [native, app] = Preferences::paths(&paths);
        let original = fs::read(&native).unwrap();
        fs::remove_file(&app).unwrap();
        fs::create_dir(&app).unwrap();
        fs::write(app.join("keep"), "unrelated").unwrap();
        let changed = Preferences::from_values(
            &Values::from([("Ipv6".into(), "false".into())]),
            Some(&preferences),
        )
        .unwrap();
        assert!(changed.save(&paths).is_err());
        assert_eq!(fs::read(native).unwrap(), original);
        assert_eq!(fs::read_to_string(app.join("keep")).unwrap(), "unrelated");
    }
    #[test]
    fn defaults_allow_only_loopback_and_saved_empty_or_denied_lists_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::isolated(root.path());
        paths.create().unwrap();
        let defaults = Preferences::from_values(&Values::new(), None).unwrap();
        assert!(defaults.local_proxy_error().is_none());
        assert_eq!(
            defaults.values()["LanAllowedIps"],
            Preferences::DEFAULT_ALLOWED_IPS
        );
        let empty = Preferences::from_values(
            &Values::from([("LanAllowedIps".into(), String::new())]),
            Some(&defaults),
        )
        .unwrap();
        empty.save(&paths).unwrap();
        let loaded = Preferences::load(&paths).unwrap();
        assert_eq!(loaded.values()["LanAllowedIps"], "");
        assert!(loaded.local_proxy_error().unwrap().contains("允许"));
        let denied = Preferences::from_values(
            &Values::from([("LanDisallowedIps".into(), "127.0.0.1/32".into())]),
            Some(&defaults),
        )
        .unwrap();
        assert!(denied.local_proxy_error().unwrap().contains("禁止"));
    }
}
