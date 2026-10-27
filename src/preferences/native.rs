//! One native document owns form values and YAML. No parallel field persistence.
use super::{SettingsKind, defaults};
use crate::{configuration::flow, storage::Values};
use serde_yaml_ng::{Mapping, Value};
pub(crate) fn key(kind: SettingsKind) -> Option<&'static str> {
    match kind {
        SettingsKind::Dns => Some("DnsYaml"),
        SettingsKind::Sniffer => Some("SnifferYaml"),
        _ => None,
    }
}
pub(crate) fn flag(kind: SettingsKind) -> &'static str {
    if kind == SettingsKind::Dns {
        "DnsOverrideEnabled"
    } else {
        "SnifferOverrideEnabled"
    }
}
pub(crate) fn initial(kind: SettingsKind) -> Result<Value, String> {
    let values = defaults(kind);
    if kind == SettingsKind::Dns {
        flow::dns_document(&super::choices::decode(values), Value::Null)
    } else {
        Ok(Value::Mapping(Mapping::from_iter([(
            "sniffer".into(),
            flow::sniffer_form(&values, Value::Null)?,
        )])))
    }
}
pub(crate) fn project(kind: SettingsKind, root: &Value, enabled: bool) -> Result<Values, String> {
    let mut values = defaults(kind);
    values.insert(flag(kind).into(), enabled.to_string());
    if kind == SettingsKind::Dns {
        let dns = root
            .get("dns")
            .filter(|value| value.is_mapping())
            .ok_or("配置需包含 dns 对象")?;
        for (field, key) in flow::KEYS {
            let Some(value) = flow::get(dns, key).cloned() else {
                continue;
            };
            let text = match *field {
                "NameserverPolicy" => flow::mapping_text(&value)?,
                _ => match value {
                    Value::Bool(value) => value.to_string(),
                    Value::Sequence(items) => items
                        .iter()
                        .map(flow::scalar)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Value::String(value) => value,
                    _ => return Err(format!("dns.{key} 类型无效")),
                },
            };
            values.insert((*field).into(), text);
        }
        if let Some(hosts) = root.get("hosts") {
            values.insert("Hosts".into(), flow::mapping_text(hosts)?);
        }
    } else {
        let document = root
            .get("sniffer")
            .and_then(Value::as_mapping)
            .ok_or("配置需包含 sniffer 对象")?;
        for (field, key) in flow::SNIFFER_FLAGS {
            if let Some(value) = document.get(*key) {
                values.insert(
                    (*field).into(),
                    value.as_bool().ok_or("嗅探开关需为布尔值")?.to_string(),
                );
            }
        }
        for (field, key) in flow::SNIFFER_LISTS {
            let text = match document.get(*key) {
                Some(Value::Sequence(items)) => items
                    .iter()
                    .map(flow::scalar)
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => String::new(),
                _ => return Err(format!("sniffer.{key} 需为列表")),
            };
            values.insert((*field).into(), text);
        }
        for (field, key) in flow::SNIFFER_PORTS {
            let text = document
                .get("sniff")
                .and_then(|value| value.get(*key))
                .and_then(|value| value.get("ports"))
                .and_then(Value::as_sequence)
                .map(|items| items.iter().map(flow::scalar).collect::<Vec<_>>().join(","))
                .unwrap_or_default();
            values.insert((*field).into(), text);
        }
    }
    values.insert(
        key(kind).ok_or("不是原生文档设置")?.into(),
        serde_yaml_ng::to_string(root).map_err(|_| "无法编码原生配置")?,
    );
    Ok(super::choices::encode(values))
}
pub(crate) fn decode(kind: SettingsKind, bytes: &[u8]) -> Result<Values, String> {
    let root: Value = serde_yaml_ng::from_slice(bytes).map_err(|error| {
        error
            .location()
            .map(|location| {
                format!(
                    "YAML 格式无效，第 {} 行第 {} 列",
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "YAML 格式无效".into())
    })?;
    let values = project(kind, &root, false)?;
    flow::validate_settings(kind, &values)?;
    Ok(values)
}
pub(crate) fn encode(kind: SettingsKind, values: &Values) -> Result<Vec<u8>, String> {
    let root = if let Some(text) = values.get(key(kind).ok_or("不是原生文档设置")?) {
        serde_yaml_ng::from_str(text).map_err(|_| "YAML 文档无效")?
    } else {
        let mut merged = defaults(kind);
        merged.extend(values.clone());
        if kind == SettingsKind::Dns {
            flow::dns_document(&super::choices::decode(merged), Value::Null)?
        } else {
            Value::Mapping(Mapping::from_iter([(
                "sniffer".into(),
                flow::sniffer_form(&merged, Value::Null)?,
            )]))
        }
    };
    let root = root.as_mapping().ok_or("设置需为对象")?;
    serde_yaml_ng::to_string(&root)
        .map(String::into_bytes)
        .map_err(|_| "无法编码 YAML 设置".into())
}
pub(crate) fn patch(kind: SettingsKind, root: &Value, changes: &Values) -> Result<Value, String> {
    let mut result = root.clone();
    let projected = project(kind, root, false)?;
    let mut desired = projected.clone();
    desired.extend(super::choices::encode(changes.clone()));
    if kind == SettingsKind::Dns {
        let full = flow::dns_document(&super::choices::decode(desired), root.clone())?;
        for (field, key) in flow::KEYS {
            if changes.contains_key(*field)
                && let Some(value) = flow::get(&full, &format!("dns.{key}"))
            {
                flow::put(&mut result, &format!("dns.{key}"), value.clone());
            }
        }
        if changes.contains_key("Hosts") {
            result["hosts"] = full["hosts"].clone();
        }
    } else {
        let full = flow::sniffer_form(&desired, root.get("sniffer").cloned().unwrap_or_default())?;
        for (field, key) in flow::SNIFFER_FLAGS.iter().chain(flow::SNIFFER_LISTS) {
            if changes.contains_key(*field) {
                flow::put(&mut result, &format!("sniffer.{key}"), full[*key].clone());
            }
        }
        for (field, key) in flow::SNIFFER_PORTS {
            if changes.contains_key(*field) {
                let path = format!("sniffer.sniff.{key}");
                if let Some(value) = full.get("sniff").and_then(|value| value.get(*key)) {
                    flow::put(&mut result, &path, value.clone());
                } else if let Some(sniff) = result
                    .get_mut("sniffer")
                    .and_then(|value| value.get_mut("sniff"))
                    .and_then(Value::as_mapping_mut)
                {
                    sniff.remove(*key);
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn yaml_owns_fields_preserves_unknown_content_and_patches_only_changed_values() {
        let source = b"dns:\n  enable: false\n  ipv6: false\n  nameserver: [1.1.1.1, 8.8.8.8]\n  cache-algorithm: arc\n";
        let values = decode(SettingsKind::Dns, source).unwrap();
        assert_eq!(values["DnsIpv6"], "false");
        assert_eq!(values["Nameserver"], "1.1.1.1\n8.8.8.8");
        let root = serde_yaml_ng::from_str(&values["DnsYaml"]).unwrap();
        let changed = patch(
            SettingsKind::Dns,
            &root,
            &Values::from([("DnsIpv6".into(), "true".into())]),
        )
        .unwrap();
        assert_eq!(changed["dns"]["ipv6"], true);
        assert_eq!(changed["dns"]["cache-algorithm"], "arc");
        assert!(changed["dns"].get("listen").is_none());
        let saved = encode(
            SettingsKind::Dns,
            &project(SettingsKind::Dns, &changed, true).unwrap(),
        )
        .unwrap();
        let saved: Value = serde_yaml_ng::from_slice(&saved).unwrap();
        assert_eq!(saved.as_mapping().unwrap().len(), 1);
        assert!(saved.get("override-enabled").is_none());
        assert!(saved["dns"]["nameserver"].is_sequence());
        assert!(saved.get("dns_ipv6").is_none());
        assert!(saved.get("dns_config").is_none());
    }
    #[test]
    fn yaml_errors_and_invalid_types_are_rejected() {
        for source in ["dns: [", "dns: {ipv6: wrong}", "dns: {nameserver: [true]}"] {
            assert!(
                decode(SettingsKind::Dns, source.as_bytes()).is_err(),
                "{source}"
            );
        }
        assert!(
            decode(
                SettingsKind::Sniffer,
                b"sniffer: {sniff: {HTTP: {ports: wrong}}}"
            )
            .is_err()
        );
    }
}
