//! Persisted DNS, sniffing and TUN overlays. No UI or network side effects.
use crate::{
    preferences::{self, SettingsKind, schema::FieldKind},
    storage::{AppPaths, Repository, Values},
};
use serde_yaml_ng::{Mapping, Value};

pub(crate) const KEYS: &[(&str, &str)] = &[
    ("DnsEnabled", "enable"),
    ("DnsListen", "listen"),
    ("EnhancedModeIndex", "enhanced-mode"),
    ("UseSystemHosts", "use-system-hosts"),
    ("UseHosts", "use-hosts"),
    ("DnsIpv6", "ipv6"),
    ("PreferH3", "prefer-h3"),
    ("RespectRules", "respect-rules"),
    (
        "DirectNameserverFollowPolicy",
        "direct-nameserver-follow-policy",
    ),
    ("FakeIpRange", "fake-ip-range"),
    ("FakeIpRange6", "fake-ip-range6"),
    ("FakeIpFilter", "fake-ip-filter"),
    ("FakeIpFilterModeIndex", "fake-ip-filter-mode"),
    ("Nameserver", "nameserver"),
    ("Fallback", "fallback"),
    ("DefaultNameserver", "default-nameserver"),
    ("DirectNameserver", "direct-nameserver"),
    ("ProxyServerNameserver", "proxy-server-nameserver"),
    ("NameserverPolicy", "nameserver-policy"),
    ("FallbackGeoIp", "fallback-filter.geoip"),
    ("FallbackGeoIpCode", "fallback-filter.geoip-code"),
    ("FallbackIpCidr", "fallback-filter.ipcidr"),
    ("FallbackDomain", "fallback-filter.domain"),
];
pub(crate) fn scalar(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        _ => String::new(),
    }
}
pub(crate) fn entries(value: &str) -> Vec<&str> {
    value
        .lines()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect()
}
pub(crate) fn parse_mapping(text: &str) -> Result<Value, String> {
    let mut map = Mapping::new();
    for line in entries(text) {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("映射需使用 域名=值 格式：{line}"))?;
        if key.trim().is_empty() || value.trim().is_empty() {
            return Err(format!("映射的域名和值不能为空：{line}"));
        }
        let values: Vec<_> = value
            .split(';')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| Value::String(v.into()))
            .collect();
        map.insert(
            Value::String(key.trim().into()),
            if values.len() == 1 {
                values[0].clone()
            } else {
                Value::Sequence(values)
            },
        );
    }
    Ok(Value::Mapping(map))
}
pub(crate) fn mapping_text(value: &Value) -> Result<String, String> {
    let Some(map) = value.as_mapping() else {
        return Err("Hosts 和解析策略必须是映射".into());
    };
    map.iter()
        .map(|(key, value)| {
            let key = key.as_str().ok_or("域名必须是文本")?;
            let text = if let Some(values) = value.as_sequence() {
                values.iter().map(scalar).collect::<Vec<_>>().join(";")
            } else {
                scalar(value)
            };
            if text.is_empty() {
                return Err(format!("{key} 的映射值不受支持"));
            }
            Ok(format!("{key}={text}"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|lines| lines.join("\n"))
}
pub(crate) fn get<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |value, key| value.get(key))
}
pub(crate) fn put(root: &mut Value, path: &str, value: Value) {
    if !root.is_mapping() {
        *root = Value::Mapping(Mapping::new());
    }
    if let Some((key, rest)) = path.split_once('.') {
        let child = root
            .as_mapping_mut()
            .unwrap()
            .entry(Value::String(key.into()))
            .or_insert_with(|| Value::Mapping(Mapping::new()));
        put(child, rest, value);
    } else {
        root.as_mapping_mut()
            .unwrap()
            .insert(Value::String(path.into()), value);
    }
}

pub(crate) fn dns_document(values: &Values, mut root: Value) -> Result<Value, String> {
    let specs = preferences::schema::sections(SettingsKind::Dns);
    for (field, key) in KEYS {
        let spec = specs
            .iter()
            .flat_map(|section| &section.fields)
            .find(|spec| spec.key == *field)
            .ok_or("DNS 字段无效")?;
        let text = values
            .get(*field)
            .map(String::as_str)
            .unwrap_or(spec.default);
        let value = if *field == "NameserverPolicy" {
            parse_mapping(text)?
        } else if spec.kind == FieldKind::Toggle {
            Value::Bool(text == "true")
        } else if spec.kind == FieldKind::Multiline {
            Value::Sequence(
                entries(text)
                    .into_iter()
                    .map(|value| value.into())
                    .collect(),
            )
        } else if *field == "FakeIpFilterModeIndex" {
            Value::String(
                if text.contains("whitelist") {
                    "whitelist"
                } else if text.contains("blacklist") {
                    "blacklist"
                } else {
                    "rule"
                }
                .into(),
            )
        } else {
            text.into()
        };
        put(&mut root, &format!("dns.{key}"), value);
    }
    put(
        &mut root,
        "hosts",
        parse_mapping(values.get("Hosts").map(String::as_str).unwrap_or(""))?,
    );
    Ok(root)
}
fn read_settings(
    paths: &AppPaths,
    staged: Option<&(SettingsKind, Values)>,
    kind: SettingsKind,
) -> Result<Values, String> {
    if let Some((target, values)) = staged
        && *target == kind
    {
        return Ok(values.clone());
    }
    let repository = Repository::open(paths.clone()).map_err(|_| "无法读取功能设置")?;
    preferences::values(&repository, kind).map_err(|error| error.to_string())
}
pub(crate) fn apply_dns(
    paths: &AppPaths,
    staged: Option<&(SettingsKind, Values)>,
    config: &mut Mapping,
) -> Result<(), String> {
    let dns = read_settings(paths, staged, SettingsKind::Dns)?;
    validate_settings(SettingsKind::Dns, &dns)?;
    let enabled = dns
        .get("DnsOverrideEnabled")
        .is_some_and(|enabled| enabled == "true");
    if enabled {
        let document = if let Some(yaml) = dns.get("DnsYaml") {
            serde_yaml_ng::from_str::<Value>(yaml).map_err(|_| "DNS 设置文档无效")?
        } else {
            dns_document(&preferences::choices::decode(dns), Value::Null)?
        };
        let mapping = document.as_mapping().ok_or("DNS 设置必须为对象")?;
        if !mapping.get("dns").is_some_and(Value::is_mapping) {
            return Err("DNS 设置缺少 dns 对象".into());
        }
        for key in ["dns", "hosts"] {
            if let Some(value) = mapping.get(key) {
                config.insert(key.into(), value.clone());
            }
        }
    }
    Ok(())
}
pub(crate) fn apply_settings(
    paths: &AppPaths,
    staged: Option<&(SettingsKind, Values)>,
    config: &mut Mapping,
) -> Result<(), String> {
    let sniffer = read_settings(paths, staged, SettingsKind::Sniffer)?;
    validate_settings(SettingsKind::Sniffer, &sniffer)?;
    if sniffer
        .get("SnifferOverrideEnabled")
        .is_some_and(|enabled| enabled == "true")
    {
        let document = sniffer_document(&sniffer)?;
        config.insert("sniffer".into(), document);
    }
    Ok(())
}

pub(crate) const SNIFFER_FLAGS: &[(&str, &str)] = &[
    ("SnifferEnabled", "enable"),
    ("OverrideDestination", "override-destination"),
    ("ForceDnsMapping", "force-dns-mapping"),
    ("ParsePureIp", "parse-pure-ip"),
];
pub(crate) const SNIFFER_LISTS: &[(&str, &str)] = &[
    ("SkipDomain", "skip-domain"),
    ("ForceDomain", "force-domain"),
    ("SkipDstAddress", "skip-dst-address"),
    ("SkipSrcAddress", "skip-src-address"),
];
pub(crate) const SNIFFER_PORTS: &[(&str, &str)] = &[
    ("HttpPorts", "HTTP"),
    ("TlsPorts", "TLS"),
    ("QuicPorts", "QUIC"),
];
pub(crate) fn sniffer_form(values: &Values, root: Value) -> Result<Value, String> {
    let mut document = root.as_mapping().cloned().unwrap_or_default();
    for (field, key) in [
        ("SnifferEnabled", "enable"),
        ("OverrideDestination", "override-destination"),
        ("ForceDnsMapping", "force-dns-mapping"),
        ("ParsePureIp", "parse-pure-ip"),
    ] {
        document.insert(
            key.into(),
            values
                .get(field)
                .is_some_and(|value| value == "true")
                .into(),
        );
    }
    let mut sniff = document
        .get("sniff")
        .and_then(Value::as_mapping)
        .cloned()
        .unwrap_or_default();
    for (field, protocol) in [
        ("HttpPorts", "HTTP"),
        ("TlsPorts", "TLS"),
        ("QuicPorts", "QUIC"),
    ] {
        let ports: Vec<_> = values
            .get(field)
            .map(String::as_str)
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                value
                    .parse::<u16>()
                    .map(|port| Value::Number(port.into()))
                    .unwrap_or_else(|_| Value::String(value.into()))
            })
            .collect();
        if !ports.is_empty() {
            let mut details = sniff
                .get(protocol)
                .and_then(Value::as_mapping)
                .cloned()
                .unwrap_or_default();
            details.insert("ports".into(), Value::Sequence(ports));
            sniff.insert(protocol.into(), details.into());
        } else {
            sniff.remove(protocol);
        }
    }
    document.insert("sniff".into(), sniff.into());
    for (field, key) in [
        ("SkipDomain", "skip-domain"),
        ("ForceDomain", "force-domain"),
        ("SkipDstAddress", "skip-dst-address"),
        ("SkipSrcAddress", "skip-src-address"),
    ] {
        document.insert(
            key.into(),
            Value::Sequence(
                entries(values.get(field).map(String::as_str).unwrap_or(""))
                    .into_iter()
                    .map(|value| value.into())
                    .collect(),
            ),
        );
    }

    Ok(Value::Mapping(document))
}
pub(crate) fn sniffer_document(values: &Values) -> Result<Value, String> {
    if let Some(text) = values.get("SnifferYaml") {
        let value: Value = serde_yaml_ng::from_str(text).map_err(|_| "嗅探 YAML 无效")?;
        let map = value.as_mapping().ok_or("嗅探配置需为对象")?;
        if map.len() != 1 || !map.contains_key("sniffer") {
            return Err("嗅探配置仅支持 sniffer 对象".into());
        }
        return Ok(map["sniffer"].clone());
    }
    sniffer_form(values, Value::Null)
}
pub(crate) fn validate_settings(kind: SettingsKind, values: &Values) -> Result<(), String> {
    preferences::validate(kind, values)?;
    if kind == SettingsKind::Dns {
        let root = if let Some(text) = values.get("DnsYaml") {
            serde_yaml_ng::from_str(text).map_err(|_| "DNS YAML 无效")?
        } else {
            dns_document(&preferences::choices::decode(values.clone()), Value::Null)?
        };
        let map = root.as_mapping().ok_or("DNS 配置需为对象")?;
        if map
            .keys()
            .any(|key| !matches!(key.as_str(), Some("dns" | "hosts")))
        {
            return Err("DNS 配置仅支持 dns 和 hosts".into());
        }
        let dns = map
            .get("dns")
            .and_then(Value::as_mapping)
            .ok_or("DNS 配置缺少 dns 对象")?;
        if dns.get("enable").and_then(Value::as_bool) == Some(true) {
            if let Some(listen) = dns.get("listen")
                && !listen
                    .as_str()
                    .is_some_and(preferences::schema::valid_dns_listen)
            {
                return Err("DNS 监听地址无效，例如 :53、127.0.0.1:53".into());
            }
            if !dns
                .get("nameserver")
                .and_then(Value::as_sequence)
                .is_some_and(|items| !items.is_empty())
            {
                return Err("至少填写一个主 DNS 服务器".into());
            }
        }
        if let Some(mode) = dns.get("enhanced-mode")
            && !mode
                .as_str()
                .is_some_and(|mode| ["fake-ip", "redir-host", "normal"].contains(&mode))
        {
            return Err("DNS 增强模式无效".into());
        }
        for key in ["fake-ip-range", "fake-ip-range6"] {
            if let Some(value) = dns.get(key)
                && !value
                    .as_str()
                    .is_some_and(|text| text.parse::<ipnet::IpNet>().is_ok())
            {
                return Err(format!("dns.{key} 需为 CIDR"));
            }
        }
        for key in ["nameserver-policy", "proxy-server-nameserver-policy"] {
            if let Some(value) = dns.get(key) {
                mapping_text(value)?;
            }
        }
        for (key, value) in dns {
            if let Some(key) = key.as_str() {
                if [
                    "enable",
                    "ipv6",
                    "use-hosts",
                    "use-system-hosts",
                    "prefer-h3",
                    "respect-rules",
                    "direct-nameserver-follow-policy",
                ]
                .contains(&key)
                    && !value.is_bool()
                {
                    return Err(format!("dns.{key} 需为布尔值"));
                }
                if [
                    "nameserver",
                    "fallback",
                    "default-nameserver",
                    "direct-nameserver",
                    "proxy-server-nameserver",
                    "fake-ip-filter",
                ]
                .contains(&key)
                    && !value
                        .as_sequence()
                        .is_some_and(|items| items.iter().all(|value| value.is_string()))
                {
                    return Err(format!("dns.{key} 需为文本列表"));
                }
            }
        }
        if let Some(hosts) = map.get("hosts") {
            mapping_text(hosts)?;
        }
    } else if kind == SettingsKind::Sniffer {
        let root = sniffer_document(values)?;
        let map = root.as_mapping().ok_or("sniffer 需为对象")?;
        for (_, key) in SNIFFER_FLAGS {
            if let Some(value) = map.get(*key)
                && !value.is_bool()
            {
                return Err(format!("sniffer.{key} 需为布尔值"));
            }
        }
        for (_, key) in SNIFFER_LISTS {
            if let Some(value) = map.get(*key)
                && !value
                    .as_sequence()
                    .is_some_and(|items| items.iter().all(Value::is_string))
            {
                return Err(format!("sniffer.{key} 需为文本列表"));
            }
        }
        if let Some(sniff) = map.get("sniff") {
            let protocols = sniff.as_mapping().ok_or("sniffer.sniff 需为对象")?;
            for (name, protocol) in protocols {
                let protocol = protocol.as_mapping().ok_or("嗅探协议需为对象")?;
                if let Some(ports) = protocol.get("ports") {
                    let ports = ports.as_sequence().ok_or("嗅探端口需为列表")?;
                    let text = ports.iter().map(scalar).collect::<Vec<_>>().join(",");
                    let candidate = Values::from([
                        ("SnifferOverrideEnabled".into(), "true".into()),
                        ("SnifferEnabled".into(), "true".into()),
                        ("HttpPorts".into(), text),
                    ]);
                    preferences::validate(SettingsKind::Sniffer, &candidate)
                        .map_err(|_| format!("{} 端口无效", name.as_str().unwrap_or("协议")))?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dns_listen_form_and_yaml_accept_kernel_wildcard_and_ip_forms() {
        for listen in [
            ":53",
            "0.0.0.0:1053",
            "127.0.0.1:53",
            "[::]:53",
            "[::1]:1053",
        ] {
            let mut values = preferences::defaults(SettingsKind::Dns);
            values.insert("DnsOverrideEnabled".into(), "true".into());
            values.insert("DnsEnabled".into(), "true".into());
            values.insert("DnsListen".into(), listen.into());
            validate_settings(SettingsKind::Dns, &values).unwrap();
            values.insert(
                "DnsYaml".into(),
                format!("dns: {{enable: true, listen: '{listen}', nameserver: ['1.1.1.1']}}"),
            );
            validate_settings(SettingsKind::Dns, &values).unwrap();
        }
        for listen in [
            "",
            ":",
            ":65536",
            ":-1",
            ":+53",
            "127.0.0.1",
            "::1:53",
            "[::1]:65536",
        ] {
            assert!(!preferences::schema::valid_dns_listen(listen), "{listen}");
        }
    }

    #[test]
    fn sniffer_form_keeps_unknown_protocol_fields_and_uses_native_port_types() {
        let mut values = preferences::defaults(SettingsKind::Sniffer);
        values.insert("HttpPorts".into(), "80,8080-8082".into());
        values.insert("QuicPorts".into(), String::new());
        let root:Value=serde_yaml_ng::from_str("custom: retained\nsniff:\n  HTTP: {ports: [81], override-destination: false}\n  FUTURE: {ports: [1234]}").unwrap();
        let result = sniffer_form(&values, root).unwrap();
        assert_eq!(result["custom"], "retained");
        assert_eq!(result["sniff"]["HTTP"]["ports"][0], 80);
        assert_eq!(result["sniff"]["HTTP"]["ports"][1], "8080-8082");
        assert_eq!(result["sniff"]["HTTP"]["override-destination"], false);
        assert!(result["sniff"].get("FUTURE").is_some());
        assert!(result["sniff"].get("QUIC").is_none());
        values.insert(
            "SnifferYaml".into(),
            serde_yaml_ng::to_string(&Value::Mapping(Mapping::from_iter([(
                "sniffer".into(),
                result,
            )])))
            .unwrap(),
        );
        let bytes = preferences::native::encode(SettingsKind::Sniffer, &values).unwrap();
        let yaml: Value = serde_yaml_ng::from_slice(&bytes).unwrap();
        assert!(yaml["sniffer"].is_mapping());
        assert!(yaml.get("http_ports").is_none());
        assert_eq!(
            preferences::native::decode(SettingsKind::Sniffer, &bytes).unwrap()["SnifferYaml"],
            values["SnifferYaml"]
        );
    }
    #[test]
    fn stopped_flow_validation_rejects_bad_yaml_types_without_touching_disk() {
        for (kind, key, text) in [
            (
                SettingsKind::Dns,
                "DnsYaml",
                "dns: {enable: true, listen: bad, nameserver: ['1.1.1.1']}",
            ),
            (SettingsKind::Dns, "DnsYaml", "dns: {enable: wrong}"),
            (
                SettingsKind::Sniffer,
                "SnifferYaml",
                "sniffer: {enable: true, sniff: {HTTP: {ports: [70000]}}}",
            ),
            (
                SettingsKind::Sniffer,
                "SnifferYaml",
                "sniffer: {skip-domain: [false]}",
            ),
        ] {
            let mut values = preferences::defaults(kind);
            values.insert(key.into(), text.into());
            assert!(validate_settings(kind, &values).is_err(), "{text}");
        }
    }
}
