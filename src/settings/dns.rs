use super::{SettingsPage, schema::FieldKind};
use crate::ui;
use gpui_kit::*;
use serde_yaml_ng::{Mapping, Value};

// UI field names and Mihomo document keys stay at the document boundary.
const KEYS: &[(&str, &str)] = &[
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
fn scalar(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        _ => String::new(),
    }
}
fn entries(value: &str) -> Vec<&str> {
    value
        .lines()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect()
}
fn parse_mapping(text: &str) -> Result<Value, String> {
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
fn mapping_text(value: &Value) -> Result<String, String> {
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
fn get<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |value, key| value.get(key))
}
fn put(root: &mut Value, path: &str, value: Value) {
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
impl SettingsPage {
    pub(super) fn dns_to_yaml(&self, cx: &App) -> Result<String, String> {
        let values = self.values(cx);
        let mut root = self.dns_document.clone();
        for (field, key) in KEYS {
            let Some(spec) = self.fields.get(field) else {
                continue;
            };
            let text = values.get(*field).map(String::as_str).unwrap_or("");
            let value = if *field == "NameserverPolicy" {
                parse_mapping(text)?
            } else if spec.spec.kind == FieldKind::Toggle {
                Value::Bool(text == "true")
            } else if spec.spec.kind == FieldKind::Multiline {
                Value::Sequence(
                    entries(text)
                        .into_iter()
                        .map(|v| Value::String(v.into()))
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
                Value::String(text.into())
            };
            put(&mut root, &format!("dns.{key}"), value);
        }
        put(
            &mut root,
            "hosts",
            parse_mapping(values.get("Hosts").map(String::as_str).unwrap_or(""))?,
        );
        serde_yaml_ng::to_string(&root).map_err(|e| e.to_string())
    }
    pub(super) fn read_dns_yaml(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let root: Value = serde_yaml_ng::from_str(self.dns_source.read(cx).value().as_ref())
            .map_err(|e| format!("YAML 格式错误：{e}"))?;
        if !root.is_mapping() || !root.get("dns").is_some_and(Value::is_mapping) {
            return Err("配置需包含 dns 映射".into());
        }
        let mut values = self.values(cx);
        for (field, key) in KEYS {
            let Some(value) = get(&root, &format!("dns.{key}")) else {
                continue;
            };
            let spec = &self.fields[field].spec;
            let text = if *field == "NameserverPolicy" {
                mapping_text(value)?
            } else if spec.kind == FieldKind::Toggle {
                value
                    .as_bool()
                    .ok_or_else(|| format!("{key} 需为 true 或 false"))?
                    .to_string()
            } else if spec.kind == FieldKind::Multiline {
                value
                    .as_sequence()
                    .ok_or_else(|| format!("{key} 需为列表"))?
                    .iter()
                    .map(scalar)
                    .collect::<Vec<_>>()
                    .join("\n")
            } else if *field == "FakeIpFilterModeIndex" {
                match value.as_str() {
                    Some("blacklist") => "黑名单（blacklist）",
                    Some("whitelist") => "白名单（whitelist）",
                    Some("rule") => "规则（rule）",
                    _ => return Err("不支持的 Fake-IP 过滤模式".into()),
                }
                .into()
            } else {
                value
                    .as_str()
                    .ok_or_else(|| format!("{key} 需为文本"))?
                    .to_owned()
            };
            if spec.kind == FieldKind::Select && !spec.options.contains(&text.as_str()) {
                return Err(format!("{key} 的值不受支持"));
            }
            values.insert((*field).into(), text);
        }
        if let Some(hosts) = root.get("hosts") {
            values.insert("Hosts".into(), mapping_text(hosts)?);
        }
        self.dns_document = root;
        self.assign_values(&values, window, cx);
        Ok(())
    }
    pub(super) fn switch_dns_mode(
        &mut self,
        raw: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if raw == self.dns_raw {
            return;
        }
        let result = if raw {
            self.dns_to_yaml(cx).map(|source| {
                self.dns_source
                    .update(cx, |state, cx| state.set_value(source, window, cx))
            })
        } else {
            self.read_dns_yaml(window, cx)
        };
        match result {
            Ok(()) => self.dns_raw = raw,
            Err(error) => ui::show_error(error, window, cx),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{get, mapping_text, parse_mapping, put};
    use serde_yaml_ng::Value;
    #[test]
    fn hosts_and_policy_roundtrip_multiple_values_and_ipv6() {
        let text = "example.com=::1;127.0.0.1\nalias.test=example.com";
        assert_eq!(mapping_text(&parse_mapping(text).unwrap()).unwrap(), text);
        assert!(parse_mapping("bad-line").is_err());
    }
    #[test]
    fn updating_known_dns_fields_preserves_unknown_fields() {
        let mut root: Value = serde_yaml_ng::from_str(
            "dns:\n  cache-algorithm: arc\n  fallback-filter:\n    geosite: [gfw]\n",
        )
        .unwrap();
        put(&mut root, "dns.fallback-filter.geoip", Value::Bool(true));
        assert_eq!(
            get(&root, "dns.cache-algorithm").unwrap().as_str(),
            Some("arc")
        );
        assert!(
            get(&root, "dns.fallback-filter.geosite")
                .unwrap()
                .is_sequence()
        );
    }
}
