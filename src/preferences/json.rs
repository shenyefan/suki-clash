//! Structured settings JSON. String-valued forms are an in-memory UI adapter only.
use super::{
    SettingsKind,
    schema::{self, FieldKind},
};
use crate::storage::Values;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

const UNKNOWN: &str = "__json:";
fn fields() -> BTreeMap<String, (String, FieldKind)> {
    SettingsKind::ALL
        .into_iter()
        .flat_map(schema::sections)
        .flat_map(|section| section.fields)
        .map(|field| (snake(field.key), (field.key.to_owned(), field.kind)))
        .collect()
}
fn snake(name: &str) -> String {
    let chars: Vec<_> = name.chars().collect();
    let mut result = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_uppercase()
            && index > 0
            && (chars[index - 1].is_lowercase()
                || (chars[index - 1].is_uppercase()
                    && chars.get(index + 1).is_some_and(|next| next.is_lowercase())))
        {
            result.push('_');
        }
        result.extend(ch.to_lowercase());
    }
    result
}
fn list_field(key: &str, kind: FieldKind) -> bool {
    kind == FieldKind::Multiline && key != "PacScript"
        || matches!(key, "HttpPorts" | "TlsPorts" | "QuicPorts")
}
fn yaml_object(text: &str) -> Result<Value, String> {
    let value: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(text).map_err(|_| "设置对象格式无效")?;
    serde_json::to_value(value).map_err(|_| "设置对象含不受支持的键".into())
}
pub(crate) fn encode(values: &Values) -> Result<Vec<u8>, String> {
    let definitions = fields();
    let mut root = Map::new();
    root.insert("schema_version".into(), 2.into());
    let mut panels: BTreeMap<u64, Map<String, Value>> = BTreeMap::new();
    for (key, text) in values {
        if let Some(name) = key.strip_prefix(UNKNOWN) {
            root.insert(
                name.into(),
                serde_json::from_str(text).map_err(|_| "扩展设置格式无效")?,
            );
            continue;
        }
        if let Some((id, field)) = key
            .strip_prefix("WebUi.")
            .and_then(|value| value.split_once('.'))
        {
            let id = id.parse::<u64>().map_err(|_| "WebUI ID 无效")?;
            if !matches!(field, "name" | "url") {
                return Err("WebUI 字段无效".into());
            }
            let panel = panels.entry(id).or_default();
            panel.insert("id".into(), id.into());
            panel.insert(field.into(), text.clone().into());
            continue;
        }
        if matches!(key.as_str(), "WebUiConfigured" | "NewWebUiName") {
            continue;
        }
        let name = snake(key);
        let kind = definitions.get(&name).map(|(_, kind)| *kind);
        let value = if key == "Tunnels" {
            yaml_object(text)?
        } else if kind == Some(FieldKind::Toggle) || key == "StartCore" {
            match text.as_str() {
                "true" => true.into(),
                "false" => false.into(),
                _ => return Err(format!("{key} 必须为布尔值")),
            }
        } else if kind == Some(FieldKind::Number) {
            text.parse::<u64>()
                .map(Value::from)
                .map_err(|_| format!("{key} 必须为非负整数"))?
        } else if kind.is_some_and(|kind| list_field(key, kind)) {
            let entries = if matches!(key.as_str(), "HttpPorts" | "TlsPorts" | "QuicPorts") {
                text.split(',').collect::<Vec<_>>()
            } else {
                text.lines().collect()
            };
            Value::Array(
                entries
                    .into_iter()
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(|entry| entry.into())
                    .collect(),
            )
        } else {
            text.clone().into()
        };
        root.insert(name, value);
    }
    if values.contains_key("WebUiConfigured") || !panels.is_empty() {
        root.insert(
            "web_ui".into(),
            serde_json::json!({"panels": panels.into_values().collect::<Vec<_>>() }),
        );
    }
    serde_json::to_vec_pretty(&root).map_err(|_| "无法编码设置 JSON".into())
}
pub(crate) fn decode(bytes: &[u8]) -> Result<Values, String> {
    let mut root: Map<String, Value> = serde_json::from_slice(bytes).map_err(|error| {
        format!(
            "JSON 格式无效，第 {} 行第 {} 列",
            error.line(),
            error.column()
        )
    })?;
    if root
        .remove("schema_version")
        .and_then(|version| version.as_u64())
        != Some(2)
    {
        return Err("设置版本不受支持".into());
    }
    let definitions = fields();
    let mut values = Values::new();
    for (name, value) in root {
        if name == "web_ui" {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct WebUi {
                panels: Vec<Panel>,
            }
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Panel {
                id: u64,
                name: String,
                url: String,
            }
            let web_ui: WebUi = serde_json::from_value(value).map_err(|_| "WebUI 设置格式无效")?;
            values.insert("WebUiConfigured".into(), "true".into());
            let mut ids = std::collections::BTreeSet::new();
            for panel in web_ui.panels {
                if !ids.insert(panel.id) {
                    return Err("WebUI ID 重复".into());
                }
                values.insert(format!("WebUi.{}.name", panel.id), panel.name);
                values.insert(format!("WebUi.{}.url", panel.id), panel.url);
            }
            continue;
        }
        if name == "tunnels" {
            if !value.is_array() {
                return Err("tunnels 格式无效".into());
            }
            let text = serde_yaml_ng::to_string(&value).map_err(|_| "无法读取设置对象")?;
            values.insert("Tunnels".into(), text);
            continue;
        }
        let Some((key, kind)) = definitions.get(&name) else {
            if let Some(text) = value.as_str() {
                values.insert(name, text.into());
            } else {
                values.insert(
                    format!("{UNKNOWN}{name}"),
                    serde_json::to_string(&value).map_err(|_| "扩展设置无效")?,
                );
            }
            continue;
        };
        let text = if *kind == FieldKind::Toggle {
            value
                .as_bool()
                .map(|value| value.to_string())
                .ok_or_else(|| format!("{name} 必须为布尔值"))?
        } else if *kind == FieldKind::Number {
            value
                .as_u64()
                .map(|value| value.to_string())
                .ok_or_else(|| format!("{name} 必须为非负整数"))?
        } else if list_field(key, *kind) {
            let entries = value
                .as_array()
                .ok_or_else(|| format!("{name} 必须为数组"))?
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .ok_or_else(|| format!("{name} 数组元素必须为文本"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            entries.join(
                if matches!(key.as_str(), "HttpPorts" | "TlsPorts" | "QuicPorts") {
                    ","
                } else {
                    "\n"
                },
            )
        } else {
            value
                .as_str()
                .ok_or_else(|| format!("{name} 必须为文本"))?
                .to_owned()
        };
        values.insert(key.clone(), text);
    }
    Ok(values)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stored_settings_use_objects_arrays_booleans_and_numbers() {
        let values = Values::from([
            ("WebUiConfigured".into(), "true".into()),
            ("WebUi.7.name".into(), "面板".into()),
            ("WebUi.7.url".into(), "https://example.test/".into()),
            ("Nameserver".into(), "1.1.1.1\n8.8.8.8".into()),
            ("GeoAutoUpdate".into(), "true".into()),
            ("GeoUpdateIntervalValue".into(), "12".into()),
        ]);
        let bytes = encode(&values).unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json.get("WebUi.7.name").is_none());
        assert_eq!(json["web_ui"]["panels"][0]["id"], 7);
        assert_eq!(
            json["nameserver"],
            serde_json::json!(["1.1.1.1", "8.8.8.8"])
        );
        assert_eq!(json["geo_auto_update"], true);
        assert_eq!(json["geo_update_interval_value"], 12);
        assert_eq!(decode(&bytes).unwrap(), values);
    }
    #[test]
    fn malformed_typed_settings_fail_and_unknown_objects_roundtrip() {
        assert!(decode(br#"{"version":1,"values":{"GeoAutoUpdate":"true"}}"#).is_err());
        assert!(decode(br#"{"WebUi.1.name":"panel"}"#).is_err());
        assert!(decode(br#"{"schema_version":2,"geo_auto_update":"true"}"#).is_err());
        assert!(decode(br#"{"schema_version":2,"nameserver":"1.1.1.1\n8.8.8.8"}"#).is_err());
        let bytes = br#"{"schema_version":2,"custom":{"Names":[1,2]}}"#;
        let encoded = encode(&decode(bytes).unwrap()).unwrap();
        let json: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(json["custom"]["Names"], serde_json::json!([1, 2]));
    }
}
