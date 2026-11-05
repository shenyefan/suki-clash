//! Ordered subscription sequence edits; raw downloads remain untouched.
use crate::storage::Values;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use std::collections::BTreeSet;
#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct SequenceEdit {
    prepend: Vec<Value>,
    append: Vec<Value>,
    delete: Vec<String>,
}
fn name(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("name").and_then(Value::as_str))
}
pub(crate) fn apply(source: &[u8], values: &Values) -> Result<Vec<u8>, String> {
    let root: Value = serde_yaml_ng::from_slice(source)
        .map_err(|error| crate::logging::diagnostic_error("订阅 YAML 无效", error))?;
    let mut config = root.as_mapping().cloned().ok_or("订阅顶层必须是对象")?;
    // Top-level names use the same lowercase contract as ordinary composition.
    config = config
        .into_iter()
        .map(|(key, value)| {
            (
                key.as_str()
                    .map(|key| Value::String(key.to_ascii_lowercase()))
                    .unwrap_or(key),
                value,
            )
        })
        .collect();
    for field in ["rules", "proxies", "proxy-groups"] {
        let Some(text) = values.get(field) else {
            continue;
        };
        if text.len() as u64 > super::INPUT_LIMIT {
            return Err("订阅增强超过上限".into());
        }
        let edit: SequenceEdit = serde_yaml_ng::from_str(text).map_err(|error| {
            crate::logging::diagnostic_error(format!("{field} 增强格式无效"), error)
        })?;
        for value in edit.prepend.iter().chain(&edit.append) {
            let valid = if field == "rules" {
                value.is_string()
            } else {
                value.is_mapping() && value.get("name").and_then(Value::as_str).is_some()
            };
            if !valid {
                return Err(format!("{field} 增强条目无效"));
            }
        }
        let deleted: BTreeSet<_> = edit.delete.iter().map(String::as_str).collect();
        let added = edit
            .prepend
            .iter()
            .chain(&edit.append)
            .filter_map(name)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let existing = match config.remove(Value::String(field.into())) {
            None => vec![],
            Some(Value::Sequence(values)) => values,
            Some(_) => return Err(format!("{field} 必须是列表")),
        };
        let mut entries = edit.prepend;
        entries.extend(
            existing
                .into_iter()
                .filter(|value| name(value).is_none_or(|name| !deleted.contains(name))),
        );
        entries.extend(edit.append);
        config.insert(field.into(), entries.into());
        if field == "proxies" {
            update_groups(&mut config, &deleted, &added);
        }
    }
    let bytes = serde_yaml_ng::to_string(&config)
        .map_err(|error| crate::logging::diagnostic_error("无法编码订阅增强", error))?
        .into_bytes();
    // Reuse composition budgets, including depth and node limits, after expansion.
    super::compose(&bytes, None, &Values::new())
        .map_err(|error| crate::logging::diagnostic_error("订阅增强无效或超过上限".into(), error))
}
fn update_groups(config: &mut Mapping, deleted: &BTreeSet<&str>, added: &[String]) {
    let Some(groups) = config
        .get_mut(Value::String("proxy-groups".into()))
        .and_then(Value::as_sequence_mut)
    else {
        return;
    };
    let mut injected = false;
    for group in groups {
        let Some(group) = group.as_mapping_mut() else {
            continue;
        };
        let selector = group
            .get(Value::String("type".into()))
            .and_then(Value::as_str)
            .is_some_and(|kind| {
                kind.eq_ignore_ascii_case("select") || kind.eq_ignore_ascii_case("selector")
            });
        if let Some(proxies) = group
            .get_mut(Value::String("proxies".into()))
            .and_then(Value::as_sequence_mut)
        {
            proxies.retain(|value| value.as_str().is_none_or(|name| !deleted.contains(name)));
            if selector && !injected && !added.is_empty() {
                let mut combined = added.iter().cloned().map(Value::String).collect::<Vec<_>>();
                combined.append(proxies);
                let mut seen = BTreeSet::new();
                combined.retain(|value| {
                    value
                        .as_str()
                        .is_none_or(|name| seen.insert(name.to_owned()))
                });
                *proxies = combined;
                injected = true;
            }
        } else if selector && !injected && !added.is_empty() {
            group.insert(
                "proxies".into(),
                Value::Sequence(added.iter().cloned().map(Value::String).collect()),
            );
            injected = true;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequence_edits_preserve_raw_source_and_fix_selector_references() {
        let raw=b"proxies: [{name: old, type: direct}]\nproxy-groups: [{name: main, type: select, proxies: [old,DIRECT]}]\nrules: ['MATCH,main']\n";
        let edit = Values::from([
            (
                "rules".into(),
                "prepend: ['DOMAIN,example.com,DIRECT']\nappend: []\ndelete: ['MATCH,main']".into(),
            ),
            (
                "proxies".into(),
                "prepend: [{name: new, type: direct}]\ndelete: [old]".into(),
            ),
        ]);
        let bytes = apply(raw, &edit).unwrap();
        let root: Value = serde_yaml_ng::from_slice(&bytes).unwrap();
        assert_eq!(root["rules"].as_sequence().unwrap().len(), 1);
        assert_eq!(root["proxies"][0]["name"], "new");
        assert_eq!(root["proxy-groups"][0]["proxies"][0], "new");
        assert!(!String::from_utf8(bytes).unwrap().contains("old"));
        assert!(String::from_utf8_lossy(raw).contains("old"));
    }
}
